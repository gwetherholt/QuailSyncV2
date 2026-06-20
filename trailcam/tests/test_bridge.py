"""Tests for QuailSyncBridge — payload structure, HTTP delivery, and the
JSONL write-ahead-log fallback when the API is unreachable."""

import json

import pytest

from quailsync_bridge import QuailSyncBridge
from yolo_detector import Detection, DetectionResult


# --- Fake HTTP sessions (a `requests`-like object with `.post`) -------------


class _FakeResp:
    def raise_for_status(self):
        pass


class SuccessSession:
    """Records POSTs and returns a 2xx-ish response."""

    def __init__(self):
        self.posts = []

    def post(self, url, json=None, timeout=None):
        self.posts.append((url, json))
        return _FakeResp()


class FailingSession:
    """Simulates an unreachable / erroring API."""

    def __init__(self):
        self.attempts = 0

    def post(self, url, json=None, timeout=None):
        self.attempts += 1
        raise OSError("connection refused")


def make_result(camera="camA", confidences=(0.85,), total=None, temperature_f=None):
    detections = [Detection("quail", c, [100.0, 100.0, 200.0, 200.0]) for c in confidences]
    return DetectionResult(
        image_path="/staging/camA/img.jpg",
        camera_id=camera,
        timestamp="2026-01-01T00:00:00+00:00",
        total_count=len(detections) if total is None else total,
        detections=detections,
        inference_time_ms=12.3,
        model_version="stub.pt",
        ambient_temperature_f=temperature_f,
    )


def test_build_payload_structure(tmp_path):
    bridge = QuailSyncBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(confidences=(0.8, 0.9)))

    assert payload["camera_id"] == "camA"
    assert payload["timestamp"] == "2026-01-01T00:00:00+00:00"
    assert payload["bird_count"] == 2
    assert payload["average_confidence"] == pytest.approx(0.85)
    assert payload["min_confidence"] == 0.8
    assert payload["inference_time_ms"] == 12.3
    # Temperature passes through; absent on the result -> null in the payload.
    assert payload["ambient_temperature_f"] is None
    # Basenames only — no host paths leave the bridge.
    assert payload["image_filename"] == "img.jpg"
    assert payload["annotated_image_filename"] == "img_annotated.jpg"
    assert "image_path" not in payload
    assert "source" not in payload
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


def test_ambient_temperature_passes_through(tmp_path):
    bridge = QuailSyncBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(temperature_f=68.4))
    assert payload["ambient_temperature_f"] == pytest.approx(68.4)


def test_empty_detections_confidence_is_none(tmp_path):
    bridge = QuailSyncBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(confidences=(), total=0))
    assert payload["bird_count"] == 0
    assert payload["average_confidence"] is None
    assert payload["min_confidence"] is None
    assert payload["detections"] == []


def test_post_delivers_via_http(tmp_path):
    out = tmp_path / "observations.jsonl"
    session = SuccessSession()
    bridge = QuailSyncBridge(api_url="http://qs.test", output_path=out, session=session)

    assert bridge.post(make_result()) is True

    # POSTed to the observation endpoint; nothing written to the WAL on success.
    assert len(session.posts) == 1
    url, payload = session.posts[0]
    assert url == "http://qs.test/api/trailcam/observation"
    assert payload["camera_id"] == "camA"
    assert not out.exists()


def test_post_falls_back_to_wal_when_api_down(tmp_path):
    out = tmp_path / "observations.jsonl"
    session = FailingSession()
    bridge = QuailSyncBridge(output_path=out, session=session)

    # Delivery failed, but the observation is preserved in the WAL -> True.
    assert bridge.post(make_result()) is True
    assert session.attempts == 1
    assert out.exists()
    lines = out.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 1
    record = json.loads(lines[0])
    assert record["camera_id"] == "camA"
    assert record["image_filename"] == "img.jpg"


def test_post_batch_counts_delivered(tmp_path):
    out = tmp_path / "observations.jsonl"
    session = SuccessSession()
    bridge = QuailSyncBridge(output_path=out, session=session)

    success, failure = bridge.post_batch([make_result(camera="camA"), make_result(camera="camB")])

    assert (success, failure) == (2, 0)
    assert len(session.posts) == 2
    cameras = {p["camera_id"] for _, p in session.posts}
    assert cameras == {"camA", "camB"}
    assert not out.exists()  # all delivered, nothing in the WAL


def test_post_returns_false_when_http_and_wal_fail(tmp_path, monkeypatch):
    out = tmp_path / "observations.jsonl"
    bridge = QuailSyncBridge(output_path=out, session=FailingSession())

    def boom(_payload):
        raise IOError("disk full")

    monkeypatch.setattr(bridge, "_append_jsonl", boom)

    # Both the POST and the WAL write failed -> data lost -> False.
    assert bridge.post(make_result()) is False
    success, failure = bridge.post_batch([make_result()])
    assert (success, failure) == (0, 1)
