"""Shared pytest fixtures for the trail-cam test suite.

Provides a fake Ultralytics YOLO model (so tests never load real weights) and a
helper to drop a PIL-generated image + metadata sidecar into a staging dir.
"""

import json

import pytest
from PIL import Image


# ---------------------------------------------------------------------------
# Fake YOLO model
# ---------------------------------------------------------------------------
# These stand in for the Ultralytics result shape consumed by
# yolo_detector.detect(): results is a list of objects with `.boxes` and
# `.names`; each box exposes `.cls[0]`, `.conf[0]`, and `.xyxy[0].tolist()`.


class _FakeXYXY:
    def __init__(self, values):
        self._values = list(values)

    def tolist(self):
        return list(self._values)


class _FakeBox:
    def __init__(self, class_id, confidence, xyxy):
        self.cls = [class_id]
        self.conf = [confidence]
        self.xyxy = [_FakeXYXY(xyxy)]


class _FakeResult:
    def __init__(self, boxes, names):
        self.boxes = boxes
        self.names = names


class FakeYOLO:
    """Predictable stand-in for ultralytics.YOLO.

    Every ``predict()`` returns exactly one detection: class "quail",
    confidence 0.85, bbox [100, 100, 200, 200].
    """

    def __init__(self, *args, **kwargs):
        self.predict_calls = []

    def predict(self, source=None, conf=None, verbose=False, **kwargs):
        self.predict_calls.append({"source": source, "conf": conf})
        box = _FakeBox(0, 0.85, [100.0, 100.0, 200.0, 200.0])
        return [_FakeResult(boxes=[box], names={0: "quail"})]


@pytest.fixture
def fake_yolo():
    """A bare FakeYOLO instance (for tests that want to assert on its calls)."""
    return FakeYOLO()


@pytest.fixture
def mock_yolo(monkeypatch):
    """Patch yolo_detector so detection uses FakeYOLO instead of real weights.

    Returns the FakeYOLO instance so a test can inspect ``predict_calls``.
    """
    import yolo_detector

    model = FakeYOLO()
    yolo_detector._MODEL_CACHE.clear()
    monkeypatch.setattr(yolo_detector, "_load_model", lambda model_path: model)
    return model


# ---------------------------------------------------------------------------
# Image + sidecar helper
# ---------------------------------------------------------------------------


@pytest.fixture
def make_image_with_sidecar():
    """Return a function that writes a 640x640 solid-color JPEG plus the
    ``{stem}.json`` metadata sidecar the poller would have produced."""

    def _make(
        camera_dir,
        stem="20260101-120000_abc123",
        camera_id="test_camera",
        color=(120, 160, 90),
        timestamp="2026-01-01T12:00:00+00:00",
    ):
        camera_dir.mkdir(parents=True, exist_ok=True)
        image_path = camera_dir / f"{stem}.jpg"
        Image.new("RGB", (640, 640), color=color).save(image_path, format="JPEG")
        sidecar = {
            "photo_id": stem.split("_")[-1],
            "camera_id": camera_id,
            "timestamp": timestamp,
            "download_time": timestamp,
        }
        (camera_dir / f"{stem}.json").write_text(json.dumps(sidecar), encoding="utf-8")
        return image_path

    return _make
