"""Optional Roboflow pre-annotation upload for the trail-cam pipeline.

After YOLO inference, push each processed image plus its predictions to Roboflow
as *reviewable pre-labels* (``is_prediction=True``) so a human can correct them
in Roboflow's annotation UI before they're promoted to training data. The
predictions are written as a standard YOLO ``.txt`` (``class_idx x_center
y_center width height``, normalized) generated from the ``DetectionResult``'s
pixel-space bounding boxes.

Strictly opt-in and best-effort:

* Runs only when ``config.ROBOFLOW_UPLOAD_ENABLED`` is truthy AND
  ``config.ROBOFLOW_API_KEY`` is set. With the key unset the upload is skipped
  silently — nothing in the pipeline breaks.
* The heavy ``roboflow`` SDK is imported lazily (only when an upload actually
  runs), so importing this module never requires the dependency.
* Every failure is caught and logged; an upload error never propagates into the
  pipeline.

The pipeline calls :func:`upload_if_enabled` after the bridge step.
"""

from __future__ import annotations

import logging
import tempfile
from pathlib import Path

# Support both `python roboflow_uploader.py` (script) and package imports.
try:
    from . import config
    from .yolo_detector import DetectionResult
except ImportError:
    import config
    from yolo_detector import DetectionResult

logger = logging.getLogger("trailcam.roboflow_uploader")


# ---------------------------------------------------------------------------
# YOLO annotation generation
# ---------------------------------------------------------------------------


def _clamp_unit(value: float) -> float:
    """Clamp to the [0, 1] range YOLO normalized coordinates must live in."""
    return min(max(value, 0.0), 1.0)


def yolo_annotation_lines(
    result: DetectionResult,
    image_width: int,
    image_height: int,
    class_map: dict[str, int],
) -> list[str]:
    """Convert a ``DetectionResult``'s pixel bboxes into YOLO label lines.

    Each detection becomes ``class_idx x_center y_center width height`` with
    coordinates normalized to the image size. ``class_map`` is a running
    name->index registry (mutated in place) so class indices stay stable across
    a batch — for the single-class quail detector every box is class 0.
    """
    if image_width <= 0 or image_height <= 0:
        return []

    lines: list[str] = []
    for det in result.detections:
        if len(det.bbox) != 4:
            continue
        x1, y1, x2, y2 = det.bbox
        cx = _clamp_unit(((x1 + x2) / 2.0) / image_width)
        cy = _clamp_unit(((y1 + y2) / 2.0) / image_height)
        w = _clamp_unit(abs(x2 - x1) / image_width)
        h = _clamp_unit(abs(y2 - y1) / image_height)
        class_idx = class_map.setdefault(det.class_name, len(class_map))
        lines.append(f"{class_idx} {cx:.6f} {cy:.6f} {w:.6f} {h:.6f}")
    return lines


def _resolve_image_path(image_path_str: str) -> Path | None:
    """Find the on-disk image for a ``DetectionResult``.

    ``DetectionResult.image_path`` is recorded while the image is still in
    ``staging/``; ``process_staging`` then moves it into ``processed/`` (camera
    subdir preserved). So we check the recorded path first, then the
    staging->processed remap. Returns ``None`` if neither exists.
    """
    recorded = Path(image_path_str)
    if recorded.exists():
        return recorded
    try:
        relative = recorded.relative_to(config.STAGING_DIR)
    except ValueError:
        return None
    processed = config.PROCESSED_DIR / relative
    return processed if processed.exists() else None


def _image_size(path: Path) -> tuple[int, int] | None:
    """Return ``(width, height)`` for an image, or ``None`` if it can't open.

    Pillow is imported lazily so this module (and the pipeline) don't depend on
    it unless an upload actually runs.
    """
    try:
        from PIL import Image

        with Image.open(path) as img:
            return img.size  # (width, height)
    except Exception as exc:  # noqa: BLE001 — a bad image just gets skipped
        logger.warning("Could not read image size for %s: %s", path, exc)
        return None


# ---------------------------------------------------------------------------
# Uploader
# ---------------------------------------------------------------------------


class RoboflowUploader:
    """Uploads images + YOLO pre-annotations to a Roboflow project."""

    def __init__(
        self,
        api_key: str,
        workspace: str = config.ROBOFLOW_WORKSPACE,
        project: str = config.ROBOFLOW_PROJECT,
        batch_name: str = config.ROBOFLOW_BATCH_NAME,
    ):
        self.api_key = api_key
        self.workspace = workspace
        self.project = project
        self.batch_name = batch_name
        self._project_handle = None  # lazily connected Roboflow project object
        # Running class-name -> index registry, stable across the batch.
        self._class_map: dict[str, int] = {}

    @classmethod
    def from_config(cls) -> "RoboflowUploader":
        """Build an uploader from the module config (requires the API key set)."""
        if not config.ROBOFLOW_API_KEY:
            raise RuntimeError("ROBOFLOW_API_KEY is not set")
        return cls(
            api_key=config.ROBOFLOW_API_KEY,
            workspace=config.ROBOFLOW_WORKSPACE,
            project=config.ROBOFLOW_PROJECT,
            batch_name=config.ROBOFLOW_BATCH_NAME,
        )

    def _connect(self):
        """Lazily resolve the Roboflow project handle (imports the SDK here)."""
        if self._project_handle is None:
            from roboflow import Roboflow  # lazy: heavy optional dependency

            rf = Roboflow(api_key=self.api_key)
            self._project_handle = rf.workspace(self.workspace).project(self.project)
            logger.info(
                "Connected to Roboflow project %s/%s", self.workspace, self.project
            )
        return self._project_handle

    def upload_result(self, result: DetectionResult) -> bool:
        """Upload one image + its pre-annotations. Returns True on success."""
        image_path = _resolve_image_path(result.image_path)
        if image_path is None:
            logger.warning(
                "Skipping Roboflow upload — image not found for %s", result.image_path
            )
            return False

        size = _image_size(image_path)
        if size is None:
            return False
        width, height = size

        lines = yolo_annotation_lines(result, width, height, self._class_map)

        project = self._connect()

        # Write the YOLO label to a temp .txt and hand its path to Roboflow.
        # delete=False + finally-unlink keeps it cross-platform (Windows can't
        # reopen an open NamedTemporaryFile).
        tmp = tempfile.NamedTemporaryFile(
            mode="w", suffix=".txt", delete=False, encoding="utf-8"
        )
        annotation_path = Path(tmp.name)
        try:
            tmp.write("\n".join(lines))
            if lines:
                tmp.write("\n")
            tmp.close()

            project.single_upload(
                image_path=str(image_path),
                annotation_path=str(annotation_path),
                is_prediction=True,
                batch_name=self.batch_name,
            )
            logger.info(
                "Uploaded %s to Roboflow (%d pre-label(s), batch=%s)",
                image_path.name,
                len(lines),
                self.batch_name,
            )
            return True
        except Exception as exc:  # noqa: BLE001 — one bad upload mustn't abort the batch
            logger.warning("Roboflow upload failed for %s: %s", image_path.name, exc)
            return False
        finally:
            annotation_path.unlink(missing_ok=True)

    def upload_results(self, results: list[DetectionResult]) -> tuple[int, int]:
        """Upload many results. Returns ``(uploaded, failed)``."""
        uploaded = 0
        failed = 0
        for result in results:
            if self.upload_result(result):
                uploaded += 1
            else:
                failed += 1
        logger.info("Roboflow upload: %d uploaded, %d failed", uploaded, failed)
        return uploaded, failed


def upload_if_enabled(results: list[DetectionResult]) -> tuple[int, int]:
    """Push ``results`` to Roboflow when enabled and configured; else no-op.

    Pipeline entry point. Returns ``(uploaded, failed)``. Never raises:

    * ``ROBOFLOW_UPLOAD_ENABLED`` falsey -> no-op ``(0, 0)``.
    * key unset -> skipped *silently* (debug log) ``(0, 0)``.
    * any error (incl. the SDK not installed) -> logged, ``(0, len(results))``.
    """
    if not config.ROBOFLOW_UPLOAD_ENABLED:
        logger.debug("Roboflow upload disabled (ROBOFLOW_UPLOAD_ENABLED is false)")
        return (0, 0)
    if not config.ROBOFLOW_API_KEY:
        # Silent skip — enabling without a key is not an error worth shouting about.
        logger.debug("ROBOFLOW_API_KEY not set — skipping Roboflow upload")
        return (0, 0)
    if not results:
        return (0, 0)

    try:
        uploader = RoboflowUploader.from_config()
        return uploader.upload_results(results)
    except Exception as exc:  # noqa: BLE001 — upload is best-effort, never fatal
        logger.warning("Roboflow upload skipped due to error: %s", exc)
        return (0, len(results))


def main() -> int:
    """Standalone: re-run detection over staging and upload the results."""
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    try:
        from .yolo_detector import process_staging
    except ImportError:
        from yolo_detector import process_staging

    config.ensure_dirs()
    results = process_staging()
    uploaded, failed = upload_if_enabled(results)
    logger.info("Done — %d uploaded, %d failed", uploaded, failed)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
