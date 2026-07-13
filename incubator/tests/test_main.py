"""Tests for the service-loop Roboflow upload wiring in main.py.

Drives ``IncubatorPipeline.run_once`` with a fake frame source, a fake store, and
a recording uploader (no camera, DB, or network) to verify:

* the full-frame upload fires on the timer interval (independent of detection),
* the full-frame upload fires on a change-detection event,
* uploads are off (pipeline builds no uploader) when disabled or unkeyed.
"""

import json

import numpy as np

import config as cfg
from camera import FakeFrameSource
from main import IncubatorPipeline
from roboflow_uploader import build_uploader


class _FakeStore:
    def __init__(self):
        self.events = []

    def record_event(self, **kw):
        self.events.append(kw)
        return len(self.events)

    def close(self):
        pass


class _RecordingUploader:
    """Stand-in for RoboflowUploader — records the names it was asked to upload."""

    def __init__(self):
        self.names = []

    def upload_frame(self, frame, name, *, cv2_module=None):
        self.names.append(name)
        return True


def _config(tmp_path, roboflow, env):
    data = {
        "camera": {"source_env": "INCUBATOR_RTSP_URL", "capture_interval_seconds": 10, "warmup_frames": 0},
        "storage": {
            "db_path": str(tmp_path / "q.db"),
            "captures_dir": str(tmp_path / "caps"),
            # Injected fake store handles events; keep crop-writing out of the way.
            "save_crops_on_event": False,
            "sqlite_busy_timeout_ms": 5000,
        },
        "detection": {
            "baseline_alpha": 0.02, "high_threshold": 18.0, "low_threshold": 8.0,
            "cooldown_seconds": 120, "min_frames_before_detect": 2,
            "freeze_baseline_while_active": True, "blur_kernel": 1,
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


def _quiet():
    return np.full((40, 40, 3), 100, dtype=np.uint8)


def _loud():
    return np.full((40, 40, 3), 220, dtype=np.uint8)


def test_upload_fires_on_timer_interval(tmp_path):
    conf = _config(tmp_path, _rf(upload_interval_seconds=100), env={"ROBOFLOW_API_KEY": "k"})
    up = _RecordingUploader()
    pipe = IncubatorPipeline(
        conf,
        frame_source=FakeFrameSource([_quiet()]),  # constant frame -> no events
        store=_FakeStore(),
        uploader=up,
    )

    pipe.run_once(now=1000.0)  # first frame -> timer due (last None) -> upload
    assert len(up.names) == 1
    pipe.run_once(now=1050.0)  # 50s < 100s -> not yet
    assert len(up.names) == 1
    pipe.run_once(now=1100.0)  # 100s >= interval -> upload
    assert len(up.names) == 2
    # All timer-driven (no detection events on a constant frame).
    assert all(n.endswith("_timer.jpg") for n in up.names)


def test_upload_fires_on_event(tmp_path):
    # Huge interval so the timer only bootstraps once; the event drives the rest.
    conf = _config(tmp_path, _rf(upload_interval_seconds=1_000_000_000), env={"ROBOFLOW_API_KEY": "k"})
    up = _RecordingUploader()
    fs = FakeFrameSource([_quiet(), _quiet(), _quiet(), _loud()])
    pipe = IncubatorPipeline(conf, frame_source=fs, store=_FakeStore(), uploader=up)

    pipe.run_once(now=1000.0)  # frame 1: bootstrap timer upload; detection warming up
    pipe.run_once(now=1010.0)  # frame 2: still in warmup
    pipe.run_once(now=1020.0)  # frame 3: baseline settled, quiet -> no event
    before = len(up.names)

    events = pipe.run_once(now=1030.0)  # frame 4: step change -> event
    assert events, "expected a change-detection event on the loud frame"
    assert len(up.names) == before + 1  # the event triggered exactly one upload
    assert up.names[-1].endswith("_event.jpg")


def test_event_upload_suppressed_when_upload_on_event_false(tmp_path):
    conf = _config(
        tmp_path,
        _rf(upload_interval_seconds=1_000_000_000, upload_on_event=False),
        env={"ROBOFLOW_API_KEY": "k"},
    )
    up = _RecordingUploader()
    fs = FakeFrameSource([_quiet(), _quiet(), _quiet(), _loud()])
    pipe = IncubatorPipeline(conf, frame_source=fs, store=_FakeStore(), uploader=up)

    for now in (1000.0, 1010.0, 1020.0, 1030.0):
        pipe.run_once(now=now)
    # Only the single bootstrap timer upload; the event did NOT upload.
    assert up.names == [n for n in up.names if n.endswith("_timer.jpg")]
    assert len(up.names) == 1


def test_disabled_pipeline_builds_no_uploader_and_never_uploads(tmp_path):
    conf = _config(tmp_path, _rf(enabled=False), env={"ROBOFLOW_API_KEY": "k"})
    assert build_uploader(conf) is None
    fs = FakeFrameSource([_quiet(), _quiet(), _quiet(), _loud()])
    # No uploader injected -> pipeline builds one from config -> None (disabled).
    pipe = IncubatorPipeline(conf, frame_source=fs, store=_FakeStore())
    assert pipe.uploader is None
    for now in (1000.0, 1010.0, 1020.0, 1030.0):
        pipe.run_once(now=now)  # must not raise despite an event on the loud frame


def test_missing_key_pipeline_skips_uploads_silently(tmp_path):
    conf = _config(tmp_path, _rf(enabled=True), env={})  # key not set
    pipe = IncubatorPipeline(conf, frame_source=FakeFrameSource([_quiet()]), store=_FakeStore())
    assert pipe.uploader is None
    pipe.run_once(now=1000.0)  # no-op upload, no error
