"""Tests for roboflow_uploader.py — REST raw-image upload + enable/skip gating.

The HTTP call is mocked throughout (a fake ``post``); no network, no real
Roboflow, no ``requests`` dependency needed.
"""

import json

import numpy as np

import config as cfg
from roboflow_uploader import BATCH_NAME, UPLOAD_URL, RoboflowUploader, build_uploader


class _FakeResp:
    def __init__(self, status_code=200, text="ok"):
        self.status_code = status_code
        self.text = text


class _FakePost:
    """Records calls; returns a canned status. Stands in for ``requests.post``."""

    def __init__(self, status_code=200):
        self.calls = []
        self._status = status_code

    def __call__(self, url, **kwargs):
        self.calls.append({"url": url, **kwargs})
        return _FakeResp(self._status)


def _frame():
    return np.full((16, 16, 3), 128, dtype=np.uint8)


def _config(tmp_path, roboflow, env):
    data = {
        "camera": {"source_env": "INCUBATOR_RTSP_URL", "capture_interval_seconds": 10, "warmup_frames": 0},
        "storage": {
            "db_path": str(tmp_path / "q.db"),
            "captures_dir": str(tmp_path / "caps"),
            "save_crops_on_event": True,
            "sqlite_busy_timeout_ms": 5000,
        },
        "detection": {
            "baseline_alpha": 0.02, "high_threshold": 18.0, "low_threshold": 8.0,
            "cooldown_seconds": 120, "min_frames_before_detect": 5,
            "freeze_baseline_while_active": True, "blur_kernel": 5,
        },
        "tray": {"reference_image": "incubator/reference.jpg",
                 "slots": [{"id": "A1", "bbox": [0, 0, 40, 40], "clutch_id": None}]},
        "roboflow": roboflow,
    }
    path = tmp_path / "config.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    return cfg.load_config(path, env=env)


def _rf(**over):
    base = {
        "enabled": True, "project": "incubation-stages", "workspace": "quail",
        "upload_interval_seconds": 1800, "upload_on_event": True,
        "api_key_env": "ROBOFLOW_API_KEY",
    }
    base.update(over)
    return base


# --- REST upload ------------------------------------------------------------


def test_upload_frame_posts_raw_image_via_rest():
    post = _FakePost(status_code=200)
    up = RoboflowUploader(api_key="KEY", project="incubation-stages", workspace="quail", post=post)

    assert up.upload_frame(_frame(), "incubator_x.jpg") is True
    assert len(post.calls) == 1
    call = post.calls[0]
    # Correct endpoint + project.
    assert call["url"] == UPLOAD_URL.format(project="incubation-stages")
    # Query params carry the key, image name, and the distinguishing batch.
    assert call["params"]["api_key"] == "KEY"
    assert call["params"]["name"] == "incubator_x.jpg"
    assert call["params"]["batch_name"] == BATCH_NAME
    # Raw image in the multipart body; NO annotation file (no model yet).
    assert "file" in call["files"]
    assert "annotation" not in call and "annotation_path" not in call


def test_upload_uses_configured_batch_name():
    up = RoboflowUploader("KEY", "p", "w", post=_FakePost())
    assert up.batch_name == "incubator-auto"


def test_upload_http_error_returns_false_without_raising():
    post = _FakePost(status_code=500)
    up = RoboflowUploader("KEY", "p", "w", post=post)
    assert up.upload_frame(_frame(), "n.jpg") is False


def test_upload_post_exception_is_swallowed():
    def boom(*_a, **_k):
        raise RuntimeError("network down")

    up = RoboflowUploader("KEY", "p", "w", post=boom)
    assert up.upload_frame(_frame(), "n.jpg") is False


# --- build_uploader gating (enabled / key) ----------------------------------


def test_build_uploader_disabled_returns_none(tmp_path):
    conf = _config(tmp_path, _rf(enabled=False), env={"ROBOFLOW_API_KEY": "k"})
    post = _FakePost()
    assert build_uploader(conf, post=post) is None
    assert post.calls == []  # nothing uploaded


def test_build_uploader_missing_key_returns_none_silently(tmp_path):
    conf = _config(tmp_path, _rf(enabled=True), env={})  # key not in env
    assert conf.roboflow.api_key is None
    post = _FakePost()
    assert build_uploader(conf, post=post) is None
    assert post.calls == []  # skipped silently — no HTTP


def test_build_uploader_enabled_with_key_returns_uploader(tmp_path):
    conf = _config(tmp_path, _rf(enabled=True), env={"ROBOFLOW_API_KEY": "k"})
    up = build_uploader(conf, post=_FakePost())
    assert isinstance(up, RoboflowUploader)
    assert up.project == "incubation-stages"
    assert up.workspace == "quail"
    assert up.batch_name == BATCH_NAME
    assert up.api_key == "k"


def test_blank_api_key_env_value_is_treated_as_unset(tmp_path):
    conf = _config(tmp_path, _rf(enabled=True), env={"ROBOFLOW_API_KEY": "   "})
    assert conf.roboflow.api_key is None
    assert build_uploader(conf) is None
