"""Tests for yolo_detector — the Ultralytics model is mocked via the
``mock_yolo`` fixture (see conftest), so no real weights are loaded."""

import json

import pytest
from PIL import Image

from yolo_detector import Detection, DetectionResult, detect, process_staging


def test_detect_returns_expected_result(tmp_path, mock_yolo, make_image_with_sidecar):
    camera_dir = tmp_path / "staging" / "test_camera"
    image = make_image_with_sidecar(camera_dir, stem="20260101-120000_abc", camera_id="test_camera")

    result = detect(image, model_path="stub.pt", confidence=0.5)

    assert isinstance(result, DetectionResult)
    assert result.camera_id == "test_camera"
    assert result.timestamp == "2026-01-01T12:00:00+00:00"
    assert result.total_count == 1
    assert result.model_version == "stub.pt"

    assert len(result.detections) == 1
    detection = result.detections[0]
    assert isinstance(detection, Detection)
    assert detection.class_name == "quail"
    assert detection.confidence == 0.85
    assert detection.bbox == [100.0, 100.0, 200.0, 200.0]


def test_detect_passes_confidence_to_model(tmp_path, mock_yolo, make_image_with_sidecar):
    image = make_image_with_sidecar(tmp_path / "staging" / "cam")
    detect(image, model_path="stub.pt", confidence=0.42)
    assert mock_yolo.predict_calls[-1]["conf"] == 0.42


def test_detect_falls_back_to_dirname_without_sidecar(tmp_path, mock_yolo):
    camera_dir = tmp_path / "staging" / "cam_from_dir"
    camera_dir.mkdir(parents=True)
    image = camera_dir / "20260101-120000_x.jpg"
    Image.new("RGB", (640, 640), color=(10, 20, 30)).save(image, format="JPEG")

    result = detect(image, model_path="stub.pt")

    assert result.camera_id == "cam_from_dir"  # parent dir name
    assert result.timestamp is None


def test_process_staging_moves_files_and_writes_detections(tmp_path, mock_yolo, make_image_with_sidecar):
    staging = tmp_path / "staging"
    processed = tmp_path / "processed"
    camera_dir = staging / "test_camera"
    make_image_with_sidecar(camera_dir, stem="20260101-120000_a")
    make_image_with_sidecar(camera_dir, stem="20260101-120001_b")

    results = process_staging(staging_dir=staging, processed_dir=processed, model_path="stub.pt")

    assert len(results) == 2

    # Staging emptied of images; processed populated.
    assert list(camera_dir.glob("*.jpg")) == []
    processed_camera = processed / "test_camera"
    assert len(list(processed_camera.glob("*.jpg"))) == 2

    detection_files = list(processed_camera.glob("*_detections.json"))
    assert len(detection_files) == 2

    sidecars = [p for p in processed_camera.glob("*.json") if not p.name.endswith("_detections.json")]
    assert len(sidecars) == 2

    payload = json.loads(detection_files[0].read_text())
    assert payload["total_count"] == 1
    assert payload["detections"][0]["class_name"] == "quail"
    assert payload["camera_id"] == "test_camera"


def test_detect_selects_per_camera_model(tmp_path, mock_yolo, make_image_with_sidecar, monkeypatch):
    import yolo_detector
    from pathlib import Path

    monkeypatch.setattr(
        yolo_detector.config, "CAMERA_MODEL_MAP", {"cam-special": Path("/models/special.pt")}
    )
    monkeypatch.setattr(yolo_detector.config, "YOLO_MODEL_PATH", Path("/models/global.pt"))

    camera_dir = tmp_path / "staging" / "cam-special"
    image = make_image_with_sidecar(camera_dir, stem="20260101-120000_a", camera_id="cam-special")

    # No explicit model_path -> model chosen from the sidecar's camera_id.
    result = detect(image)
    assert result.model_version == "special.pt"


def test_detect_falls_back_to_global_model_for_unmapped_camera(
    tmp_path, mock_yolo, make_image_with_sidecar, monkeypatch
):
    import yolo_detector
    from pathlib import Path

    monkeypatch.setattr(
        yolo_detector.config, "CAMERA_MODEL_MAP", {"cam-special": Path("/models/special.pt")}
    )
    monkeypatch.setattr(yolo_detector.config, "YOLO_MODEL_PATH", Path("/models/global.pt"))

    camera_dir = tmp_path / "staging" / "cam-other"
    image = make_image_with_sidecar(camera_dir, stem="20260101-120000_b", camera_id="cam-other")

    result = detect(image)
    assert result.model_version == "global.pt"


def test_explicit_model_path_overrides_per_camera(
    tmp_path, mock_yolo, make_image_with_sidecar, monkeypatch
):
    import yolo_detector
    from pathlib import Path

    monkeypatch.setattr(
        yolo_detector.config, "CAMERA_MODEL_MAP", {"cam-special": Path("/models/special.pt")}
    )
    camera_dir = tmp_path / "staging" / "cam-special"
    image = make_image_with_sidecar(camera_dir, stem="20260101-120000_c", camera_id="cam-special")

    # An explicit model_path wins over the per-camera map.
    result = detect(image, model_path="forced.pt")
    assert result.model_version == "forced.pt"


def test_process_staging_uses_per_camera_models(
    tmp_path, mock_yolo, make_image_with_sidecar, monkeypatch
):
    import yolo_detector
    from pathlib import Path

    monkeypatch.setattr(
        yolo_detector.config, "CAMERA_MODEL_MAP", {"cam-a": Path("/models/a.pt")}
    )
    monkeypatch.setattr(yolo_detector.config, "YOLO_MODEL_PATH", Path("/models/global.pt"))

    staging = tmp_path / "staging"
    processed = tmp_path / "processed"
    make_image_with_sidecar(staging / "cam-a", stem="20260101-120000_a", camera_id="cam-a")
    make_image_with_sidecar(staging / "cam-b", stem="20260101-120000_b", camera_id="cam-b")

    # No explicit model_path: each image's camera_id drives model selection.
    process_staging(staging_dir=staging, processed_dir=processed)

    payload_a = json.loads(next((processed / "cam-a").glob("*_detections.json")).read_text())
    payload_b = json.loads(next((processed / "cam-b").glob("*_detections.json")).read_text())
    assert payload_a["model_version"] == "a.pt"  # mapped camera
    assert payload_b["model_version"] == "global.pt"  # unmapped -> fallback


def test_process_staging_empty_dir_returns_nothing(tmp_path, mock_yolo):
    results = process_staging(
        staging_dir=tmp_path / "staging",
        processed_dir=tmp_path / "processed",
        model_path="stub.pt",
    )
    assert results == []


@pytest.mark.integration
def test_detect_with_real_yolov8n(tmp_path):
    """Run the genuine stock YOLOv8n model end-to-end through detect().

    Slow and network-dependent (ultralytics auto-downloads yolov8n.pt on first
    use), so it's marked 'integration' — skip with `pytest -m "not integration"`.
    Note: NOT the custom quail model; YOLOv8n is COCO-trained, so we assert on
    the result *structure*, never on specific detection counts.
    """
    pytest.importorskip("ultralytics")

    camera_dir = tmp_path / "staging" / "real_cam"
    camera_dir.mkdir(parents=True)
    image = camera_dir / "20260101-120000_real.jpg"
    Image.new("RGB", (640, 640), color=(120, 130, 140)).save(image, format="JPEG")

    # Low confidence so the COCO model is more likely to emit *something* on a
    # synthetic image — but we still don't assert it must.
    result = detect(image, model_path="yolov8n.pt", confidence=0.1)

    assert isinstance(result, DetectionResult)
    assert result.image_path == str(image)
    assert result.model_version == "yolov8n.pt"
    assert result.inference_time_ms > 0
    assert result.total_count == len(result.detections)

    for detection in result.detections:
        assert isinstance(detection, Detection)
        assert isinstance(detection.class_name, str) and detection.class_name
        assert isinstance(detection.confidence, float)
        assert len(detection.bbox) == 4
        assert all(isinstance(coord, float) for coord in detection.bbox)
