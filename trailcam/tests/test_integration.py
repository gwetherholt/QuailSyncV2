"""End-to-end test of the detect -> process -> post chain with a mocked YOLO
model. Uses PIL to generate real 640x640 JPEGs so the image handling is
exercised for real; only the model inference is faked."""

import json

from PIL import Image

from quailsync_bridge import QuailSyncBridge
from yolo_detector import process_staging


def _stage_image(camera_dir, stem, camera_id="test_camera", color=(80, 120, 160)):
    camera_dir.mkdir(parents=True, exist_ok=True)
    image_path = camera_dir / f"{stem}.jpg"
    Image.new("RGB", (640, 640), color=color).save(image_path, format="JPEG")
    sidecar = {
        "photo_id": stem.split("_")[-1],
        "camera_id": camera_id,
        "timestamp": "2026-01-01T12:00:00+00:00",
        "download_time": "2026-01-01T12:00:00+00:00",
    }
    (camera_dir / f"{stem}.json").write_text(json.dumps(sidecar), encoding="utf-8")
    return image_path


def test_full_chain(tmp_path, mock_yolo):
    staging = tmp_path / "staging"
    processed = tmp_path / "processed"
    camera_dir = staging / "test_camera"

    stems = ["20260101-120000_a", "20260101-120001_b", "20260101-120002_c"]
    for index, stem in enumerate(stems):
        _stage_image(camera_dir, stem, color=(40 * index, 100, 150))

    # 1. Detect over staging (mocked model) and move finished sets to processed/.
    results = process_staging(staging_dir=staging, processed_dir=processed, model_path="stub.pt")
    assert len(results) == 3

    # 2. Post observations.
    observations = processed / "observations.jsonl"
    bridge = QuailSyncBridge(output_path=observations)
    success, failure = bridge.post_batch(results)
    assert (success, failure) == (3, 0)

    # --- Assert the full chain's side effects --------------------------------

    # Images moved out of staging into processed/, camera subdir preserved.
    assert list(camera_dir.glob("*.jpg")) == []
    processed_camera = processed / "test_camera"
    assert len(list(processed_camera.glob("*.jpg"))) == 3

    # Detection results + original sidecars all landed in processed/.
    assert len(list(processed_camera.glob("*_detections.json"))) == 3
    moved_sidecars = [
        p for p in processed_camera.glob("*.json") if not p.name.endswith("_detections.json")
    ]
    assert len(moved_sidecars) == 3

    # Observations logged — one JSONL line per image, correctly shaped.
    lines = observations.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 3
    record = json.loads(lines[0])
    assert record["source"] == "trailcam"
    assert record["camera_id"] == "test_camera"
    assert record["bird_count"] == 1
    assert record["detections"][0]["class_name"] == "quail"
    assert record["detections"][0]["confidence"] == 0.85
