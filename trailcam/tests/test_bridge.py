"""Tests for QuailSyncBridge — payload structure and JSONL output."""

import json

import pytest

from quailsync_bridge import QuailSyncBridge
from yolo_detector import Detection, DetectionResult


def make_result(camera="camA", confidences=(0.85,), total=None):
    detections = [Detection("quail", c, [100.0, 100.0, 200.0, 200.0]) for c in confidences]
    return DetectionResult(
        image_path="/staging/camA/img.jpg",
        camera_id=camera,
        timestamp="2026-01-01T00:00:00+00:00",
        total_count=len(detections) if total is None else total,
        detections=detections,
        inference_time_ms=12.3,
        model_version="stub.pt",
    )


def test_build_payload_structure(tmp_path):
    bridge = QuailSyncBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(confidences=(0.8, 0.9)))

    assert payload["source"] == "trailcam"
    assert payload["camera_id"] == "camA"
    assert payload["timestamp"] == "2026-01-01T00:00:00+00:00"
    assert payload["bird_count"] == 2
    assert payload["average_confidence"] == pytest.approx(0.85)
    assert payload["min_confidence"] == 0.8
    assert payload["inference_time_ms"] == 12.3
    assert payload["image_path"] == "/staging/camA/img.jpg"
    assert len(payload["detections"]) == 2
    assert payload["detections"][0] == {
        "class_name": "quail",
        "confidence": 0.8,
        "bbox": [100.0, 100.0, 200.0, 200.0],
    }


def test_bird_count_tracks_total_count(tmp_path):
    bridge = QuailSyncBridge(output_path=tmp_path / "observations.jsonl")
    # total_count is authoritative even if it differs from len(detections).
    payload = bridge.build_payload(make_result(confidences=(0.85,), total=5))
    assert payload["bird_count"] == 5


def test_empty_detections_confidence_is_none(tmp_path):
    bridge = QuailSyncBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(confidences=(), total=0))
    assert payload["bird_count"] == 0
    assert payload["average_confidence"] is None
    assert payload["min_confidence"] is None
    assert payload["detections"] == []


def test_post_writes_one_jsonl_line(tmp_path):
    out = tmp_path / "observations.jsonl"
    bridge = QuailSyncBridge(output_path=out)

    assert bridge.post(make_result()) is True
    assert out.exists()

    lines = out.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 1
    record = json.loads(lines[0])
    assert record["source"] == "trailcam"
    assert record["camera_id"] == "camA"


def test_post_batch_appends_and_counts(tmp_path):
    out = tmp_path / "observations.jsonl"
    bridge = QuailSyncBridge(output_path=out)

    success, failure = bridge.post_batch([make_result(camera="camA"), make_result(camera="camB")])

    assert (success, failure) == (2, 0)
    lines = out.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 2
    cameras = {json.loads(line)["camera_id"] for line in lines}
    assert cameras == {"camA", "camB"}


def test_post_handles_write_failure(tmp_path, monkeypatch):
    out = tmp_path / "observations.jsonl"
    bridge = QuailSyncBridge(output_path=out)

    def boom(_payload):
        raise IOError("disk full")

    monkeypatch.setattr(bridge, "_append_jsonl", boom)

    assert bridge.post(make_result()) is False
    success, failure = bridge.post_batch([make_result()])
    assert (success, failure) == (0, 1)
