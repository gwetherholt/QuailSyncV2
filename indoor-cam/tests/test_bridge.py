"""Tests for IndoorBridge — payload structure, HTTP delivery to the indoor
observation endpoint, and the JSONL write-ahead-log fallback."""

import json

import pytest

from bridge import IndoorBridge


# --- Fake HTTP sessions (a `requests`-like object with `.post`) -------------


class _FakeResp:
    def __init__(self, payload=None):
        self._payload = payload if payload is not None else {}

    def raise_for_status(self):
        pass

    def json(self):
        return self._payload


class SuccessSession:
    def __init__(self):
        self.posts = []
        self.patches = []
        self._next_id = 1

    def post(self, url, json=None, timeout=None):
        self.posts.append((url, json))
        rid = self._next_id
        self._next_id += 1
        return _FakeResp({"stored": 1, "id": rid})

    def patch(self, url, timeout=None):
        self.patches.append(url)
        return _FakeResp({"id": 0, "image_cleared": True})


class FailingSession:
    def __init__(self):
        self.attempts = 0

    def post(self, url, json=None, timeout=None):
        self.attempts += 1
        raise OSError("connection refused")

    def patch(self, url, timeout=None):
        raise OSError("connection refused")


def test_build_payload_structure(make_result, tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(confidences=(0.8, 0.9)))

    assert payload["camera_id"] == "indoor-1"
    assert payload["timestamp"] == "2026-01-01T00:00:00+00:00"
    assert payload["detection_count"] == 2
    assert payload["average_confidence"] == pytest.approx(0.85)
    assert payload["min_confidence"] == 0.8
    assert payload["inference_time_ms"] == 12.3
    # Basenames only — no host paths leave the bridge.
    assert payload["image_filename"] == "20260101-120000_indoor-1.jpg"
    assert payload["annotated_image_filename"] == "20260101-120000_indoor-1_annotated.jpg"
    assert "image_path" not in payload
    assert len(payload["detections"]) == 2
    assert payload["detections"][0] == {
        "class_name": "quail",
        "confidence": 0.8,
        "bbox": [100.0, 100.0, 200.0, 200.0],
    }


def test_timestamp_override(make_result, tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(), timestamp="2026-06-22T10:00:00+00:00")
    # The capture-time override wins over the result's own timestamp.
    assert payload["timestamp"] == "2026-06-22T10:00:00+00:00"


def test_detection_count_tracks_total_count(make_result, tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(confidences=(0.85,), total=5))
    assert payload["detection_count"] == 5


def test_smoothed_count_override(make_result, tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "observations.jsonl")
    # The smoothed/median count overrides the raw frame total; detections (the
    # current frame's raw boxes) are unchanged.
    payload = bridge.build_payload(make_result(confidences=(0.8, 0.9), total=2), detection_count=7)
    assert payload["detection_count"] == 7
    assert len(payload["detections"]) == 2


def test_routine_post_has_null_image_fields(make_result, tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "observations.jsonl")
    # Most posts carry no image — the JSON observation still records the count.
    payload = bridge.build_payload(make_result(), detection_count=3, include_image=False)
    assert payload["image_filename"] is None
    assert payload["annotated_image_filename"] is None
    assert payload["detection_count"] == 3
    # Counts/confidences/inference are still present for routine posts.
    assert payload["average_confidence"] is not None
    assert payload["inference_time_ms"] == 12.3


def test_notable_post_includes_image_fields(make_result, tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(), include_image=True)
    assert payload["image_filename"] == "20260101-120000_indoor-1.jpg"
    assert payload["annotated_image_filename"] == "20260101-120000_indoor-1_annotated.jpg"


def test_empty_detections_confidence_is_none(make_result, tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "observations.jsonl")
    payload = bridge.build_payload(make_result(confidences=(), total=0))
    assert payload["detection_count"] == 0
    assert payload["average_confidence"] is None
    assert payload["min_confidence"] is None
    assert payload["detections"] == []


def test_post_delivers_via_http_and_returns_observation_id(make_result, tmp_path):
    out = tmp_path / "observations.jsonl"
    session = SuccessSession()
    bridge = IndoorBridge(api_url="http://qs.test", output_path=out, session=session)

    # Delivered -> returns the server-assigned observation id (for clear_image).
    assert bridge.post(make_result(), timestamp="2026-06-22T10:00:00+00:00") == 1

    assert len(session.posts) == 1
    url, payload = session.posts[0]
    assert url == "http://qs.test/api/indoorcam/observation"
    assert payload["camera_id"] == "indoor-1"
    assert payload["timestamp"] == "2026-06-22T10:00:00+00:00"
    assert not out.exists()  # nothing written to the WAL on success


def test_post_falls_back_to_wal_when_api_down(make_result, tmp_path):
    out = tmp_path / "observations.jsonl"
    session = FailingSession()
    bridge = IndoorBridge(output_path=out, session=session)

    # WAL'd -> returns None (no server id), but the data is preserved on disk.
    assert bridge.post(make_result()) is None
    assert session.attempts == 1
    assert out.exists()
    lines = out.read_text(encoding="utf-8").strip().splitlines()
    assert len(lines) == 1
    record = json.loads(lines[0])
    assert record["camera_id"] == "indoor-1"
    assert record["detection_count"] == 1


def test_post_returns_none_when_http_and_wal_fail(make_result, tmp_path, monkeypatch):
    out = tmp_path / "observations.jsonl"
    bridge = IndoorBridge(output_path=out, session=FailingSession())

    def boom(_payload):
        raise IOError("disk full")

    monkeypatch.setattr(bridge, "_append_jsonl", boom)

    # Both the POST and the WAL write failed -> data lost -> None.
    assert bridge.post(make_result()) is None


def test_clear_image_patches_the_observation(tmp_path):
    session = SuccessSession()
    bridge = IndoorBridge(api_url="http://qs.test", output_path=tmp_path / "o.jsonl", session=session)

    assert bridge.clear_image(7) is True
    assert session.patches == ["http://qs.test/api/indoorcam/observation/7"]


def test_clear_image_returns_false_on_error(tmp_path):
    bridge = IndoorBridge(output_path=tmp_path / "o.jsonl", session=FailingSession())
    # Best-effort: a failure is swallowed (never breaks the stream).
    assert bridge.clear_image(7) is False
