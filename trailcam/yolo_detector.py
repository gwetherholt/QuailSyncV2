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
import tempfile
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
    # Ambient temperature (°F) reported by the camera, if the poller captured
    # it; ``None`` when unavailable.
    ambient_temperature_f: float | None = None

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
# IR / night-vision preprocessing
# ===========================================================================
#
# SpyPoint cameras switch to infrared illumination at night, emitting nearly
# monochrome frames where quail blend into a dark, low-contrast background. For
# those frames we run inference on a CLAHE-enhanced copy (local contrast pulled
# up so shapes stand out) while leaving the original untouched for display and
# Roboflow upload.

# Mean absolute per-channel difference (0-255) below which a frame is treated as
# grayscale / IR. Daytime color frames have strong channel separation and sit
# well above this; true IR frames are ~0.
_IR_CHANNEL_DIFF_THRESHOLD = 12.0
# CLAHE parameters applied to the LAB L (lightness) channel.
_CLAHE_CLIP_LIMIT = 3.0
_CLAHE_TILE_GRID = (8, 8)


def _is_ir_image(cv2, bgr) -> bool:
    """True when ``bgr`` is (near-)grayscale — an IR / night-vision frame.

    Measures the mean absolute difference between the B/G/R channels: identical
    channels (monochrome IR) give ~0, while daytime color sits far above
    :data:`_IR_CHANNEL_DIFF_THRESHOLD`."""
    b, g, r = cv2.split(bgr)
    diff = (
        float(cv2.absdiff(b, g).mean())
        + float(cv2.absdiff(g, r).mean())
        + float(cv2.absdiff(r, b).mean())
    ) / 3.0
    return diff < _IR_CHANNEL_DIFF_THRESHOLD


def _apply_clahe(cv2, bgr):
    """Return ``bgr`` with CLAHE applied to its lightness channel.

    Converts to LAB, equalizes the L channel with Contrast Limited Adaptive
    Histogram Equalization (``clipLimit=3.0``, ``tileGridSize=(8, 8)``), merges
    back and returns BGR — enhancing local contrast without touching color."""
    lab = cv2.cvtColor(bgr, cv2.COLOR_BGR2LAB)
    l_chan, a_chan, b_chan = cv2.split(lab)
    clahe = cv2.createCLAHE(clipLimit=_CLAHE_CLIP_LIMIT, tileGridSize=_CLAHE_TILE_GRID)
    merged = cv2.merge((clahe.apply(l_chan), a_chan, b_chan))
    return cv2.cvtColor(merged, cv2.COLOR_LAB2BGR)


def _inference_image(image_path: Path) -> tuple[Path, Path | None]:
    """Choose the image YOLO should run on, enhancing IR frames in place of a copy.

    Returns ``(source_path, temp_path)``. For an IR/night frame the contrast is
    boosted with CLAHE and written to a temp JPEG; ``temp_path`` is that file and
    the caller must delete it after inference. For normal color frames — or if
    OpenCV is unavailable or anything fails — returns ``(image_path, None)`` and
    the original is used unchanged.

    The on-disk original is never modified: it remains the source of truth for
    display and Roboflow upload; only inference sees the enhanced copy."""
    try:
        import cv2  # lazy: only frames reaching detection pay the import cost
    except ImportError:
        return image_path, None

    try:
        bgr = cv2.imread(str(image_path))
        if bgr is None or not _is_ir_image(cv2, bgr):
            return image_path, None

        enhanced = _apply_clahe(cv2, bgr)
        tmp = tempfile.NamedTemporaryFile(prefix="clahe_", suffix=".jpg", delete=False)
        tmp.close()
        tmp_path = Path(tmp.name)
        if not cv2.imwrite(str(tmp_path), enhanced):
            tmp_path.unlink(missing_ok=True)
            return image_path, None

        logger.debug("IR frame detected — running inference on CLAHE-enhanced copy of %s", image_path)
        return tmp_path, tmp_path
    except Exception as exc:  # noqa: BLE001 — preprocessing must never break detection
        logger.warning("CLAHE preprocessing failed for %s (%s); using original", image_path, exc)
        return image_path, None


# ===========================================================================
# Detection
# ===========================================================================


def detect(
    image_path: Path | str,
    camera_id: str | None = None,
    model_path: Path | str | None = None,
    confidence: float = config.YOLO_CONFIDENCE,
) -> DetectionResult:
    """Run YOLO inference on a single image and return a ``DetectionResult``.

    ``camera_id`` / ``timestamp`` default to the image's JSON sidecar
    (``{stem}.json``, written by the poller); pass ``camera_id`` to override the
    sidecar's value (and to drive model selection).

    The model is chosen **per camera** via :func:`config.model_for_camera`
    (which falls back to ``YOLO_MODEL_PATH`` for cameras with no override),
    unless ``model_path`` is passed explicitly — then that model is used as-is.
    """
    image_path = Path(image_path)
    sidecar_camera_id, timestamp, ambient_temperature_f = _read_sidecar(image_path)
    camera_id = camera_id if camera_id is not None else sidecar_camera_id

    resolved_model = (
        Path(model_path) if model_path is not None else config.model_for_camera(camera_id)
    )
    model = _load_model(resolved_model)

    # Preprocess before inference: IR/night frames run on a CLAHE-enhanced copy;
    # color frames run on the original. The original is kept for display/upload.
    inference_source, tmp_enhanced = _inference_image(image_path)

    start = time.perf_counter()
    try:
        results = model.predict(source=str(inference_source), conf=confidence, verbose=False)
        inference_time_ms = round((time.perf_counter() - start) * 1000, 1)
    finally:
        if tmp_enhanced is not None:
            tmp_enhanced.unlink(missing_ok=True)

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
        model_version=resolved_model.name,
        ambient_temperature_f=ambient_temperature_f,
    )


def _read_sidecar(image_path: Path) -> tuple[str, str | None, float | None]:
    """Return ``(camera_id, timestamp, ambient_temperature_f)`` from the image's
    ``{stem}.json`` sidecar, falling back to the parent dir name / ``None`` if
    unavailable."""
    sidecar = image_path.with_suffix(".json")
    if sidecar.exists():
        try:
            data = json.loads(sidecar.read_text(encoding="utf-8"))
            temp = data.get("ambient_temperature_f")
            try:
                temp = float(temp) if temp is not None else None
            except (TypeError, ValueError):
                temp = None
            return (
                str(data.get("camera_id") or image_path.parent.name),
                data.get("timestamp"),
                temp,
            )
        except (json.JSONDecodeError, OSError) as exc:
            logger.warning("Could not read sidecar %s (%s)", sidecar, exc)
    return image_path.parent.name, None, None


def annotate_image(image_path: Path | str, result: DetectionResult, dest_path: Path | str) -> bool:
    """Draw ``result``'s detections onto a copy of the image and save it.

    Each detection gets a green rectangle around its bbox plus a caption like
    ``"Quail 87%"`` (title-cased class name + confidence percentage). The
    annotated copy is written as JPEG to ``dest_path``.

    Best-effort: returns ``True`` on success, ``False`` (with a warning) if the
    image can't be opened/drawn/saved — annotation never aborts the pipeline.
    Pillow is imported lazily so importing this module stays cheap.
    """
    image_path = Path(image_path)
    dest_path = Path(dest_path)
    try:
        from PIL import Image, ImageDraw, ImageFont

        with Image.open(image_path) as src:
            img = src.convert("RGB")

        draw = ImageDraw.Draw(img)
        try:
            font = ImageFont.load_default()
        except Exception:  # pragma: no cover - font backend missing
            font = None

        green = (0, 200, 0)
        white = (255, 255, 255)
        for det in result.detections:
            if len(det.bbox) != 4:
                continue
            x1, y1, x2, y2 = det.bbox
            draw.rectangle([x1, y1, x2, y2], outline=green, width=3)

            label = f"{det.class_name.title()} {round(det.confidence * 100)}%"
            # Measure the caption so its background box hugs the text.
            try:
                left, top, right, bottom = draw.textbbox((0, 0), label, font=font)
                tw, th = right - left, bottom - top
            except Exception:  # pragma: no cover - very old Pillow
                tw, th = len(label) * 6, 11

            # Caption sits just above the bbox; tuck it inside if there's no room.
            ly = y1 - th - 4
            if ly < 0:
                ly = y1 + 2
            draw.rectangle([x1, ly, x1 + tw + 6, ly + th + 4], fill=green)
            draw.text((x1 + 3, ly + 2), label, fill=white, font=font)

        dest_path.parent.mkdir(parents=True, exist_ok=True)
        img.save(dest_path, format="JPEG", quality=90)
        return True
    except Exception as exc:  # noqa: BLE001 — annotation is best-effort
        logger.warning("Could not annotate %s: %s", image_path, exc)
        return False


# ===========================================================================
# Staging-directory batch processing
# ===========================================================================


def process_staging(
    staging_dir: Path | str = config.STAGING_DIR,
    processed_dir: Path | str = config.PROCESSED_DIR,
    model_path: Path | str | None = None,
    confidence: float = config.YOLO_CONFIDENCE,
) -> list[DetectionResult]:
    """Detect over every staged ``*.jpg``, write results, and move the finished
    set (image + sidecar + detections) into ``processed/``.

    Camera subdirectories are preserved: ``staging/CAM/img.jpg`` ends up at
    ``processed/CAM/img.jpg``. Returns the list of ``DetectionResult``s
    produced (images that error out are logged and skipped, left in staging so
    they can be retried).

    Each image's camera_id is read from its JSON sidecar and passed to
    :func:`detect`, which selects that camera's model (``config.model_for_camera``).
    Pass ``model_path`` to force a single model for every image instead.
    """
    staging_dir = Path(staging_dir)
    processed_dir = Path(processed_dir)

    images = sorted(staging_dir.rglob("*.jpg"))
    logger.info("Found %d staged image(s) under %s", len(images), staging_dir)

    results: list[DetectionResult] = []
    for image_path in images:
        camera_id, _, _ = _read_sidecar(image_path)
        try:
            result = detect(
                image_path,
                camera_id=camera_id,
                model_path=model_path,
                confidence=confidence,
            )
        except Exception as exc:  # noqa: BLE001 — skip a bad image, keep going
            logger.exception("Detection failed for %s: %s — left in staging", image_path, exc)
            continue

        # Write the detections result next to the image (in staging) first…
        detections_path = image_path.with_name(f"{image_path.stem}_detections.json")
        detections_path.write_text(
            json.dumps(result.to_dict(), indent=2), encoding="utf-8"
        )

        # …and an annotated copy with bounding boxes drawn on it. Best-effort:
        # if it fails the original still gets processed (just no annotated file).
        annotated_path = image_path.with_name(f"{image_path.stem}_annotated.jpg")
        annotate_image(image_path, result, annotated_path)

        # …then move the whole set into processed/, preserving the camera subdir.
        relative_dir = image_path.parent.relative_to(staging_dir)
        dest_dir = processed_dir / relative_dir
        dest_dir.mkdir(parents=True, exist_ok=True)

        sidecar_path = image_path.with_suffix(".json")
        for src in (image_path, sidecar_path, detections_path, annotated_path):
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
