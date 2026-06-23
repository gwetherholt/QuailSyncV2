"""Tests for active_learning.py — the Roboflow upload gating and project wiring.
The trail-cam RoboflowUploader is faked so no SDK/network is needed."""

from types import SimpleNamespace

import active_learning
import config
import detector


class _FakeRoboflowUploader:
    def __init__(self, **kwargs):
        self.kwargs = kwargs
        self.uploaded = []

    def upload_result(self, result):
        self.uploaded.append(result)
        return True


def _fake_trailcam_module():
    return SimpleNamespace(RoboflowUploader=_FakeRoboflowUploader)


def test_disabled_when_flag_off(monkeypatch):
    monkeypatch.setattr(config, "ROBOFLOW_UPLOAD_ENABLED", False, raising=False)
    monkeypatch.setattr(config, "ROBOFLOW_API_KEY", "key", raising=False)
    up = active_learning.ActiveLearningUploader()
    assert up.enabled is False
    assert up.upload(object()) is False


def test_disabled_when_no_key(monkeypatch):
    monkeypatch.setattr(config, "ROBOFLOW_UPLOAD_ENABLED", True, raising=False)
    monkeypatch.setattr(config, "ROBOFLOW_API_KEY", None, raising=False)
    up = active_learning.ActiveLearningUploader()
    assert up.enabled is False
    assert up.upload(object()) is False


def test_upload_uses_chick_project_and_shared_key(monkeypatch):
    monkeypatch.setattr(config, "ROBOFLOW_UPLOAD_ENABLED", True, raising=False)
    monkeypatch.setattr(config, "ROBOFLOW_API_KEY", "shared-key", raising=False)
    monkeypatch.setattr(config, "ROBOFLOW_PROJECT", "find-chicks-5", raising=False)
    monkeypatch.setattr(config, "ROBOFLOW_WORKSPACE", "quail", raising=False)
    monkeypatch.setattr(detector, "import_trailcam_module", lambda name: _fake_trailcam_module())

    up = active_learning.ActiveLearningUploader()
    assert up.enabled is True
    result = SimpleNamespace(image_path="/x/frame.jpg", detections=[])
    assert up.upload(result) is True

    # Constructed against the chick project with the shared trail-cam key.
    assert up._uploader.kwargs["project"] == "find-chicks-5"
    assert up._uploader.kwargs["api_key"] == "shared-key"
    assert up._uploader.uploaded == [result]


def test_upload_swallows_errors(monkeypatch):
    monkeypatch.setattr(config, "ROBOFLOW_UPLOAD_ENABLED", True, raising=False)
    monkeypatch.setattr(config, "ROBOFLOW_API_KEY", "key", raising=False)

    def _boom(name):
        raise RuntimeError("no roboflow SDK")

    monkeypatch.setattr(detector, "import_trailcam_module", _boom)
    up = active_learning.ActiveLearningUploader()
    # A broken upload never raises into the stream.
    assert up.upload(SimpleNamespace(image_path="/x.jpg", detections=[])) is False
