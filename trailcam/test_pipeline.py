"""Manual smoke test for the trail-camera pipeline mechanics.

NOT a pytest suite — just run it:

    python test_pipeline.py

It exercises the full chain with the *stock* ``yolov8n.pt`` model (downloaded
automatically by ultralytics on first use — needs internet once) rather than
the custom quail model, purely to confirm the plumbing works:

    ensure_dirs() -> stage fake photos -> process_staging() (YOLO) ->
    QuailSyncBridge.post_batch() -> verify side effects

Test images are real Creative-Commons bird photos when they can be downloaded,
otherwise simple PIL-generated placeholders (colored rectangles). Either way
the assertions only check pipeline mechanics — file moves, JSON written,
observations recorded — not what the model actually detects.
"""

from __future__ import annotations

import io
import json
import logging
import shutil
import uuid
from datetime import datetime, timezone
from pathlib import Path

import requests

# Support both `python test_pipeline.py` and package execution.
try:
    from . import config
    from .yolo_detector import process_staging
    from .quailsync_bridge import QuailSyncBridge
except ImportError:
    import config
    from yolo_detector import process_staging
    from quailsync_bridge import QuailSyncBridge

logger = logging.getLogger("trailcam.test_pipeline")

TEST_CAMERA = "test_camera"
STOCK_MODEL = "yolov8n.pt"  # auto-downloaded by ultralytics; NOT our quail model
NUM_IMAGES = 3

# Best-effort real bird photos (Wikimedia Special:FilePath redirects to the
# current file). If any fail/aren't JPEG, we fall back to a PIL placeholder, so
# the test never depends on these being reachable.
BIRD_IMAGE_URLS = [
    "https://commons.wikimedia.org/wiki/Special:FilePath/House_sparrow_-_natures_pic.jpg?width=640",
    "https://commons.wikimedia.org/wiki/Special:FilePath/Common_Blackbird.jpg?width=640",
    "https://commons.wikimedia.org/wiki/Special:FilePath/Eurasian_blue_tit_Lancashire.jpg?width=640",
]
_HTTP_HEADERS = {"User-Agent": "QuailSync-trailcam-test/1.0 (pipeline smoke test)"}


# ---------------------------------------------------------------------------
# Test-image acquisition
# ---------------------------------------------------------------------------


def _download_jpeg(url: str) -> bytes | None:
    """Return JPEG bytes for ``url``, or None on any failure / non-JPEG."""
    try:
        resp = requests.get(url, headers=_HTTP_HEADERS, timeout=15)
        resp.raise_for_status()
        data = resp.content
        if data[:3] == b"\xff\xd8\xff":  # JPEG SOI marker
            return data
        logger.warning("Skipping %s — not a JPEG", url)
    except Exception as exc:  # noqa: BLE001 — best effort, fall back to PIL
        logger.warning("Download failed for %s (%s)", url, exc)
    return None


def _pil_placeholder_jpeg(index: int) -> bytes:
    """Generate a simple placeholder JPEG (colored rectangles on a backdrop)."""
    from PIL import Image, ImageDraw

    backgrounds = [(135, 206, 235), (180, 200, 160), (210, 180, 140)]
    shapes = [(200, 80, 60), (90, 140, 90), (120, 100, 180)]
    img = Image.new("RGB", (640, 480), color=backgrounds[index % len(backgrounds)])
    draw = ImageDraw.Draw(img)
    offset = 40 * index
    draw.rectangle([80 + offset, 120, 280 + offset, 320], fill=shapes[index % len(shapes)])
    draw.ellipse([320, 160 + offset, 520, 360 + offset], fill=shapes[(index + 1) % len(shapes)])
    draw.text((20, 20), f"trailcam test image #{index}", fill=(20, 20, 20))
    buf = io.BytesIO()
    img.save(buf, format="JPEG", quality=85)
    return buf.getvalue()


def _obtain_image_bytes(count: int) -> list[tuple[bytes, str]]:
    """Return ``count`` ``(jpeg_bytes, source)`` pairs, downloading real birds
    where possible and filling the rest with PIL placeholders."""
    images: list[tuple[bytes, str]] = []
    for url in BIRD_IMAGE_URLS:
        if len(images) >= count:
            break
        data = _download_jpeg(url)
        if data:
            images.append((data, "download"))
    while len(images) < count:
        images.append((_pil_placeholder_jpeg(len(images)), "pil"))
    return images


def stage_test_images(staging_camera_dir: Path, count: int) -> list[Path]:
    """Write ``count`` test images + metadata sidecars into the camera dir.

    Mirrors what the poller produces: ``{stem}.jpg`` plus a ``{stem}.json``
    sidecar, where stem is ``{timestamp}_{photo_id}``.
    """
    staging_camera_dir.mkdir(parents=True, exist_ok=True)
    paths: list[Path] = []
    for jpeg_bytes, source in _obtain_image_bytes(count):
        now = datetime.now(timezone.utc)
        photo_id = uuid.uuid4().hex
        stem = f"{now:%Y%m%d-%H%M%S}_{photo_id}"
        image_path = staging_camera_dir / f"{stem}.jpg"
        image_path.write_bytes(jpeg_bytes)
        sidecar = {
            "photo_id": photo_id,
            "camera_id": TEST_CAMERA,
            "timestamp": now.isoformat(),
            "download_time": now.isoformat(),
            "source": f"test_pipeline:{source}",
        }
        (staging_camera_dir / f"{stem}.json").write_text(
            json.dumps(sidecar, indent=2), encoding="utf-8"
        )
        logger.info("Staged %s (%s)", image_path.name, source)
        paths.append(image_path)
    return paths


# ---------------------------------------------------------------------------
# Verification helpers
# ---------------------------------------------------------------------------


def _count_lines(path: Path) -> int:
    if not path.exists():
        return 0
    with open(path, encoding="utf-8") as fh:
        return sum(1 for _ in fh)


def main() -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    # (1) Directory structure.
    config.ensure_dirs()

    staging_cam = config.STAGING_DIR / TEST_CAMERA
    processed_cam = config.PROCESSED_DIR / TEST_CAMERA
    observations = config.PROCESSED_DIR / "observations.jsonl"

    # Clean only this test's camera dirs so re-runs are deterministic; leave any
    # real cameras and the shared observations.jsonl alone.
    shutil.rmtree(staging_cam, ignore_errors=True)
    shutil.rmtree(processed_cam, ignore_errors=True)

    # (2) Stage fake photos + sidecars.
    print(f"\n=== Staging {NUM_IMAGES} test image(s) into {staging_cam} ===")
    staged = stage_test_images(staging_cam, NUM_IMAGES)

    obs_before = _count_lines(observations)

    # (3) Run detection with the stock model.
    print(f"\n=== Running process_staging() with stock model '{STOCK_MODEL}' ===")
    results = process_staging(model_path=STOCK_MODEL)
    for r in results:
        print(f"  {Path(r.image_path).name}: {r.total_count} detection(s), "
              f"{r.inference_time_ms} ms")

    # (4) Post observations.
    print("\n=== Posting observations via QuailSyncBridge ===")
    bridge = QuailSyncBridge()
    success, failure = bridge.post_batch(results)
    print(f"  post_batch -> success={success} failure={failure}")

    obs_after = _count_lines(observations)

    # (5) Verify side effects.
    leftover_jpgs = list(staging_cam.glob("*.jpg"))
    moved_jpgs = list(processed_cam.glob("*.jpg"))
    detection_files = list(processed_cam.glob("*_detections.json"))
    moved_sidecars = [
        p for p in processed_cam.glob("*.json") if not p.name.endswith("_detections.json")
    ]

    checks = [
        (f"staged {NUM_IMAGES} image(s)", len(staged) == NUM_IMAGES),
        ("all images detected/processed", len(results) == NUM_IMAGES),
        ("staging/ emptied of images", len(leftover_jpgs) == 0),
        ("images moved to processed/", len(moved_jpgs) == NUM_IMAGES),
        ("metadata sidecars moved to processed/", len(moved_sidecars) == NUM_IMAGES),
        ("detection JSON files created", len(detection_files) == NUM_IMAGES),
        ("observations.jsonl exists", observations.exists()),
        ("observations appended for each result", obs_after - obs_before == len(results)),
        ("post_batch reported no failures", failure == 0),
    ]

    print("\n=== Verification ===")
    all_ok = True
    for label, ok in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {label}")
        all_ok = all_ok and ok

    total_detections = sum(r.total_count for r in results)
    print("\n=== Summary ===")
    print(f"  images staged:        {len(staged)}")
    print(f"  images processed:     {len(results)}")
    print(f"  total detections:     {total_detections}")
    print(f"  observations written: {obs_after - obs_before} (file: {observations})")
    print(f"  processed dir:        {processed_cam}")
    print(f"\n  RESULT: {'PASS — pipeline mechanics OK' if all_ok else 'FAIL — see checks above'}\n")

    return 0 if all_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
