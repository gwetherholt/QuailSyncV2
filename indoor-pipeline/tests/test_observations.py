"""Tests for observations.py — payload shape, endpoint, and graceful failure.

The HTTP layer is a fake session (``.post`` returns a canned response); no
network, no ``requests`` dependency. The payload is asserted against the old
indoor-cam bridge / backend ``ObservationRequest`` contract.
"""

import config as cfg
from observations import ObservationClient, build_observation_client
from detector import Detection


class _FakeResp:
    def __init__(self, payload=None, *, status_ok=True):
        self._payload = payload if payload is not None else {"stored": 1, "id": 42}
        self._status_ok = status_ok

    def raise_for_status(self):
        if not self._status_ok:
            raise RuntimeError("HTTP 500")

    def json(self):
        return self._payload


class _FakeSession:
    """Records POST calls; returns a canned response or raises a queued error."""

    def __init__(self, response=None):
        self._response = response if response is not None else _FakeResp()
        self.calls = []

    def post(self, url, json=None, timeout=None):
        self.calls.append({"url": url, "json": json, "timeout": timeout})
        if isinstance(self._response, Exception):
            raise self._response
        return self._response


def _dets(*specs):
    # specs: (class_name, class_id, confidence, bbox)
    return [Detection(class_name=n, class_id=i, confidence=c, bbox=b) for (n, i, c, b) in specs]


def _client(session):
    return ObservationClient("http://localhost:3000", "indoor-1", session=session)


# --- endpoint + payload shape ----------------------------------------------


def test_post_hits_the_observation_endpoint():
    session = _FakeSession()
    ObservationClient("http://localhost:3000/", "indoor-1", session=session).post(
        _dets(("egg", 0, 0.9, [1, 2, 3, 4])), timestamp="2026-07-15T00:00:00+00:00"
    )
    assert session.calls[0]["url"] == "http://localhost:3000/api/indoorcam/observation"


def test_payload_uses_indoor1_camera_id_and_model_class_names():
    session = _FakeSession()
    _client(session).post(
        _dets(("egg", 0, 0.9, [1, 2, 3, 4]), ("egg", 0, 0.8, [5, 6, 7, 8])),
        timestamp="2026-07-15T00:00:00+00:00",
        image_filename="latest.jpg",
        annotated_image_filename="latest_annotated.jpg",
    )
    body = session.calls[0]["json"]
    assert body["camera_id"] == "indoor-1"  # observation/serving id, not indoor_tapo
    assert body["detection_count"] == 2
    assert body["timestamp"] == "2026-07-15T00:00:00+00:00"
    # Class names come straight from the YOLO detections — not hardcoded.
    assert [d["class_name"] for d in body["detections"]] == ["egg", "egg"]
    assert body["detections"][0]["bbox"] == [1, 2, 3, 4]
    # Image basenames the backend serves from processed/{camera_id}/.
    assert body["image_filename"] == "latest.jpg"
    assert body["annotated_image_filename"] == "latest_annotated.jpg"
    # Confidence aggregates.
    assert body["average_confidence"] == 0.85
    assert body["min_confidence"] == 0.8


def test_payload_class_names_reflect_mode_chick():
    # Brooder mode: the chick model emits "chick" -> that's what gets posted.
    session = _FakeSession()
    _client(session).post(_dets(("chick", 0, 0.8, [1, 2, 3, 4])), timestamp="t")
    body = session.calls[0]["json"]
    assert body["detections"][0]["class_name"] == "chick"
    assert body["detection_count"] == 1


def test_empty_detections_gives_zero_count_and_null_confidence():
    session = _FakeSession()
    _client(session).post([], timestamp="t")
    body = session.calls[0]["json"]
    assert body["detection_count"] == 0
    assert body["detections"] == []
    assert body["average_confidence"] is None
    assert body["min_confidence"] is None


def test_post_returns_observation_id():
    session = _FakeSession(_FakeResp({"stored": 1, "id": 7}))
    assert _client(session).post(_dets(("egg", 0, 0.9, [1, 2, 3, 4])), timestamp="t") == 7


def test_class_name_is_sanitized():
    session = _FakeSession()
    _client(session).post(_dets(("<script>egg", 0, 0.9, [1, 2, 3, 4])), timestamp="t")
    assert session.calls[0]["json"]["detections"][0]["class_name"] == "egg"


# --- graceful failure ------------------------------------------------------


def test_backend_unreachable_returns_none_without_raising():
    session = _FakeSession(ConnectionError("backend down"))
    # Must not raise — a failed POST can't be allowed to crash the capture loop.
    assert _client(session).post(_dets(("egg", 0, 0.9, [1, 2, 3, 4])), timestamp="t") is None


def test_http_error_status_returns_none_without_raising():
    session = _FakeSession(_FakeResp(status_ok=False))
    assert _client(session).post(_dets(("egg", 0, 0.9, [1, 2, 3, 4])), timestamp="t") is None


# --- build_observation_client gating ---------------------------------------


def _conf(tmp_path, observations):
    import json

    data = {
        "camera": {"source_env": "INDOOR_RTSP_URL", "capture_interval_seconds": 10},
        "assignment": {"backend_url": "http://localhost:3000", "camera_id": "indoor_tapo",
                       "poll_seconds": 60, "default_mode": "incubator"},
        "models": {
            "incubation": {"weights": "/m/i.pt", "confidence": 0.5, "roboflow_project": "incubation-stages", "log_events": True},
            "chick": {"weights": "/m/c.pt", "confidence": 0.5, "roboflow_project": "find-chicks-5", "log_events": False},
        },
        "roboflow": {"enabled": False, "workspace": "quail", "upload_interval_seconds": 1800,
                     "upload_on_detection": True, "api_key_env": "ROBOFLOW_API_KEY", "batch_name": "indoor-auto"},
        "storage": {"db_path": str(tmp_path / "q.db"), "sqlite_busy_timeout_ms": 5000},
    }
    if observations is not None:
        data["observations"] = observations
    path = tmp_path / "config.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    return cfg.load_config(path, env={"INDOOR_RTSP_URL": "rtsp://x"})


def test_build_client_enabled(tmp_path):
    conf = _conf(tmp_path, {"enabled": True, "backend_url": "http://localhost:3000", "camera_id": "indoor-1"})
    client = build_observation_client(conf)
    assert isinstance(client, ObservationClient)
    assert client.camera_id == "indoor-1"
    assert client.url == "http://localhost:3000/api/indoorcam/observation"


def test_build_client_disabled_returns_none(tmp_path):
    conf = _conf(tmp_path, {"enabled": False, "backend_url": "http://localhost:3000", "camera_id": "indoor-1"})
    assert build_observation_client(conf) is None


def test_build_client_absent_section_returns_none(tmp_path):
    conf = _conf(tmp_path, None)
    assert conf.observations is None
    assert build_observation_client(conf) is None
