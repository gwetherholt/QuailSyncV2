"""YOLO detection stage for the QuailSync trail-camera pipeline.

Runs an Ultralytics YOLO model over staged trail-cam photos and records what it
finds. The model is **lazy-loaded** on first use (and cached) so importing this
module — or using the dataclasses in tests — never pays the import/load cost of
ultralytics + torch.

Flow:
  * ``detect(image)``         — run inference on one image, return a
                                ``DetectionResult`` (enriched with camera id +
                                timestamp read from the image's JSON sidecar).
  * ``process_staging()``     — detect over every staged ``*.jpg``, write a
                                ``{stem}_detections.json`` result file, then move
                                the image + its sidecar + the result into the
                                processed/ tree (camera subdirs preserved).

Run standalone to process whatever is currently staged:

    python yolo_detector.py
"""

from __future__ import annotations

import hashlib
import json
import logging
import shutil
import stat
import time
from dataclasses import asdict, dataclass
from pathlib import Path

# Support both `python yolo_detector.py` (script) and `from trailcam import …`.
try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("trailcam.yolo_detector")


# ===========================================================================
# Result types
# ===========================================================================


@dataclass
class Detection:
    """A single detected object."""

    class_name: str
    confidence: float
    bbox: list[float]  # [x1, y1, x2, y2] in pixel coordinates (xyxy)


@dataclass
class DetectionResult:
    """The full outcome of running detection on one image."""

    image_path: str
    camera_id: str
    timestamp: str | None
    total_count: int
    detections: list[Detection]
    inference_time_ms: float
    model_version: str

    def to_dict(self) -> dict:
        """JSON-serializable dict (nested ``Detection``s become dicts too)."""
        return asdict(self)


# ===========================================================================
# Lazy model loading
# ===========================================================================

# Cache YOLO instances by model path so repeated detect() calls reuse the same
# loaded weights instead of reloading per image.
_MODEL_CACHE: dict[str, object] = {}


class ModelIntegrityError(Exception):
    """Raised when the model file fails its configured SHA-256 integrity check."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _verify_model_file(model_path: Path | str) -> None:
    """Pre-load safety checks on the weights file.

    PyTorch ``.pt`` files are unpickled on load — i.e. loading attacker-swapped
    weights is arbitrary code execution. So before we hand the path to
    ultralytics we:

    * warn if the file is world-writable (anyone could swap it), and
    * if ``config.YOLO_MODEL_SHA256`` is set, refuse to load a file whose digest
      doesn't match (raises :class:`ModelIntegrityError`).

    A non-existent path is left alone — ultralytics auto-downloads named models
    like ``yolov8n.pt`` on first use; there's nothing to verify yet.
    """
    path = Path(model_path)
    if not path.exists():
        return

    try:
        mode = path.stat().st_mode
        if mode & stat.S_IWOTH:
            logger.warning(
                "Model file %s is world-writable (mode %o) — anyone could swap the "
                "weights; loading a tampered .pt is arbitrary code execution. "
                "Tighten with: chmod 600 %s",
                path,
                stat.S_IMODE(mode),
                path,
            )
    except OSError as exc:  # pragma: no cover - platform dependent
        logger.debug("Could not stat model file %s: %s", path, exc)

    expected = config.YOLO_MODEL_SHA256
    if expected:
        actual = _sha256(path)
        if actual.lower() != expected.lower():
            raise ModelIntegrityError(
                f"model checksum mismatch for {path}: expected {expected}, got {actual}"
            )
        logger.info("Model checksum verified for %s", path)


def _load_model(model_path: Path | str):
    """Return a cached YOLO model for ``model_path``, loading it on first use.

    The ``ultralytics`` import happens here, not at module import, so the heavy
    dependency is only paid once detection actually runs.
    """
    key = str(model_path)
    model = _MODEL_CACHE.get(key)
    if model is None:
        _verify_model_file(model_path)
        from ultralytics import YOLO  # lazy: imported only on first detection

        logger.info("Loading YOLO model from %s", key)
        model = YOLO(key)
        _MODEL_CACHE[key] = model
    return model


# ===========================================================================
# Detection
# ===========================================================================


def detect(
    image_path: Path | str,
    model_path: Path | str = config.YOLO_MODEL_PATH,
    confidence: float = config.YOLO_CONFIDENCE,
) -> DetectionResult:
    """Run YOLO inference on a single image and return a ``DetectionResult``.

    ``camera_id`` and ``timestamp`` are read from the image's JSON sidecar
    (``{stem}.json``, written by the poller); if it's missing we fall back to
    the parent directory name for the camera and ``None`` for the timestamp.
    """
    image_path = Path(image_path)
    camera_id, timestamp = _read_sidecar(image_path)

    model = _load_model(model_path)

    start = time.perf_counter()
    results = model.predict(source=str(image_path), conf=confidence, verbose=False)
    inference_time_ms = round((time.perf_counter() - start) * 1000, 1)

    detections: list[Detection] = []
    # Ultralytics returns one Results object per source image; we passed one.
    for result in results:
        boxes = getattr(result, "boxes", None)
        if boxes is None:
            continue
        names = getattr(result, "names", {})  # {class_id: class_name}
        for box in boxes:
            class_id = int(box.cls[0])
            detections.append(
                Detection(
                    class_name=names.get(class_id, str(class_id)),
                    confidence=round(float(box.conf[0]), 4),
                    bbox=[round(float(v), 2) for v in box.xyxy[0].tolist()],
                )
            )

    return DetectionResult(
        image_path=str(image_path),
        camera_id=camera_id,
        timestamp=timestamp,
        total_count=len(detections),
        detections=detections,
        inference_time_ms=inference_time_ms,
        model_version=Path(model_path).name,
    )


def _read_sidecar(image_path: Path) -> tuple[str, str | None]:
    """Return ``(camera_id, timestamp)`` from the image's ``{stem}.json``
    sidecar, falling back to the parent dir name / ``None`` if unavailable."""
    sidecar = image_path.with_suffix(".json")
    if sidecar.exists():
        try:
            data = json.loads(sidecar.read_text(encoding="utf-8"))
            return (
                str(data.get("camera_id") or image_path.parent.name),
                data.get("timestamp"),
            )
        except (json.JSONDecodeError, OSError) as exc:
            logger.warning("Could not read sidecar %s (%s)", sidecar, exc)
    return image_path.parent.name, None


# ===========================================================================
# Staging-directory batch processing
# ===========================================================================


def process_staging(
    staging_dir: Path | str = config.STAGING_DIR,
    processed_dir: Path | str = config.PROCESSED_DIR,
    model_path: Path | str = config.YOLO_MODEL_PATH,
    confidence: float = config.YOLO_CONFIDENCE,
) -> list[DetectionResult]:
    """Detect over every staged ``*.jpg``, write results, and move the finished
    set (image + sidecar + detections) into ``processed/``.

    Camera subdirectories are preserved: ``staging/CAM/img.jpg`` ends up at
    ``processed/CAM/img.jpg``. Returns the list of ``DetectionResult``s
    produced (images that error out are logged and skipped, left in staging so
    they can be retried).
    """
    staging_dir = Path(staging_dir)
    processed_dir = Path(processed_dir)

    images = sorted(staging_dir.rglob("*.jpg"))
    logger.info("Found %d staged image(s) under %s", len(images), staging_dir)

    results: list[DetectionResult] = []
    for image_path in images:
        try:
            result = detect(image_path, model_path=model_path, confidence=confidence)
        except Exception as exc:  # noqa: BLE001 — skip a bad image, keep going
            logger.exception("Detection failed for %s: %s — left in staging", image_path, exc)
            continue

        # Write the detections result next to the image (in staging) first…
        detections_path = image_path.with_name(f"{image_path.stem}_detections.json")
        detections_path.write_text(
            json.dumps(result.to_dict(), indent=2), encoding="utf-8"
        )

        # …then move the whole set into processed/, preserving the camera subdir.
        relative_dir = image_path.parent.relative_to(staging_dir)
        dest_dir = processed_dir / relative_dir
        dest_dir.mkdir(parents=True, exist_ok=True)

        sidecar_path = image_path.with_suffix(".json")
        for src in (image_path, sidecar_path, detections_path):
            if src.exists():
                shutil.move(str(src), str(dest_dir / src.name))

        logger.info(
            "Processed %s — %d detection(s) -> %s",
            image_path.name,
            result.total_count,
            dest_dir,
        )
        results.append(result)

    logger.info("process_staging complete: %d image(s) processed", len(results))
    return results


def main() -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    config.ensure_dirs()
    results = process_staging()
    total = sum(r.total_count for r in results)
    logger.info("Done — %d image(s), %d total detection(s)", len(results), total)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
