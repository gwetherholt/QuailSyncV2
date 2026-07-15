"""Tests for main.py — the assignment-aware service loop.

Every heavy dependency is faked: a scripted frame source, a fake YOLO factory, a
fake assignment session, a fake Roboflow uploader, and a real (temp) SQLite
EventStore so incubation-event writes are exercised end-to-end. No camera,
ultralytics, or network.
"""

import sqlite3

import numpy as np
import pytest

from camera import FakeFrameSource
from detector import Detector
from assignment import AssignmentPoller
from main import IndoorPipeline
from storage import EventStore

from conftest import apply_incubation_schema


# --- fakes -----------------------------------------------------------------


class _FakeBox:
    def __init__(self, cls, conf, xyxy):
        self.cls = [cls]
        self.conf = [conf]
        self.xyxy = [np.array(xyxy, dtype=float)]


class _FakeResults:
    def __init__(self, boxes, names):
        self.boxes = boxes
        self.names = names


class _FakeModel:
    def __init__(self, names, boxes):
        self.names = names
        self._boxes = boxes

    def predict(self, frame, conf=0.25, verbose=True):
        return [_FakeResults(self._boxes, self.names)]


def _yolo_factory(weights):
    if "incubation" in weights:
        return _FakeModel({0: "egg", 1: "pipped"}, [
            _FakeBox(0, 0.91, [10, 10, 20, 20]),
            _FakeBox(1, 0.60, [30, 30, 44, 46]),
        ])
    return _FakeModel({0: "chick"}, [_FakeBox(0, 0.80, [5, 5, 15, 15])])


class _FakeResp:
    def __init__(self, payload):
        self._payload = payload

    def raise_for_status(self):
        pass

    def json(self):
        return self._payload


class _FakeSession:
    def __init__(self, active_models):
        self._responses = [_FakeResp({"active_model": m}) for m in active_models]

    def get(self, url, timeout=None):
        return self._responses.pop(0)


class _FakeUploader:
    """Records upload_frame calls with the project active at call time."""

    def __init__(self, project="incubation-stages"):
        self.project = project
        self.calls = []

    def upload_frame(self, frame, name, detections=None, *, cv2_module=None):
        self.calls.append({"project": self.project, "name": name, "detections": detections or []})
        return True


def _frames(n=6):
    return FakeFrameSource([np.full((48, 64, 3), 100, dtype=np.uint8) for _ in range(n)])


def _build(conf, active_models, *, uploader=None, store="real"):
    """Assemble an IndoorPipeline wired to fakes.

    ``store="real"`` injects a temp EventStore (schema pre-applied); ``store=None``
    forces event logging off; or pass an explicit store object.
    """
    if store == "real":
        apply_incubation_schema(conf.storage.db_path)
        store = EventStore(conf.storage.db_path, conf.storage.sqlite_busy_timeout_ms)
    poller = AssignmentPoller(
        conf.assignment.backend_url,
        conf.assignment.camera_id,
        conf.assignment.default_mode,
        session=_FakeSession(active_models),
    )
    return IndoorPipeline(
        conf,
        frame_source=_frames(),
        detector=Detector(yolo_factory=_yolo_factory),
        poller=poller,
        uploader=uploader if uploader is not None else _FakeUploader(),
        store=store,
    )


def _rows(db_path):
    conn = sqlite3.connect(str(db_path))
    try:
        return conn.execute(
            "SELECT slot_id, event_type, diff_score, high_threshold FROM incubation_events ORDER BY id"
        ).fetchall()
    finally:
        conn.close()


# --- runs the correct model per assignment ---------------------------------


def test_runs_incubation_model_when_assigned_to_incubator(make_config):
    conf = make_config()
    pipe = _build(conf, ["incubation"])
    detections = pipe.run_once(now=1000.0)
    assert {d.class_name for d in detections} == {"egg", "pipped"}
    assert pipe._active_mode == "incubation"


def test_runs_chick_model_when_assigned_to_brooder(make_config):
    conf = make_config()
    pipe = _build(conf, ["chick"])
    detections = pipe.run_once(now=1000.0)
    assert [d.class_name for d in detections] == ["chick"]
    assert pipe._active_mode == "chick"


# --- switches model on assignment change -----------------------------------


def test_switches_model_when_assignment_changes(make_config):
    conf = make_config()
    pipe = _build(conf, ["incubation", "chick"])

    first = pipe.run_once(now=1000.0)
    assert pipe._active_mode == "incubation"
    assert {d.class_name for d in first} == {"egg", "pipped"}

    # Next poll (interval elapsed) returns chick — model hot-swaps in-process.
    second = pipe.run_once(now=1000.0 + 61)
    assert pipe._active_mode == "chick"
    assert [d.class_name for d in second] == ["chick"]


def test_no_poll_before_interval_keeps_model(make_config):
    conf = make_config()
    # Only one response queued; a premature second poll would IndexError.
    pipe = _build(conf, ["incubation"])
    pipe.run_once(now=1000.0)
    # 10s later (< 60s poll interval): no new poll, same model, no crash.
    pipe.run_once(now=1010.0)
    assert pipe._active_mode == "incubation"


# --- uploads to the correct Roboflow project per mode ----------------------


def test_uploads_to_incubation_project_in_incubator_mode(make_config):
    conf = make_config()
    uploader = _FakeUploader()
    pipe = _build(conf, ["incubation"], uploader=uploader)
    pipe.run_once(now=1000.0)
    assert len(uploader.calls) == 1
    assert uploader.calls[0]["project"] == "incubation-stages"
    assert uploader.calls[0]["name"].startswith("indoor_incubation_")


def test_upload_project_follows_model_swap(make_config):
    conf = make_config()
    uploader = _FakeUploader()
    pipe = _build(conf, ["incubation", "chick"], uploader=uploader)
    pipe.run_once(now=1000.0)
    pipe.run_once(now=1000.0 + 61)
    projects = [c["project"] for c in uploader.calls]
    assert projects == ["incubation-stages", "find-chicks-5"]
    assert uploader.calls[1]["name"].startswith("indoor_chick_")


def test_upload_disabled_builds_no_uploader(make_config):
    # With roboflow disabled, the config-built uploader is None and cycles run
    # fine without uploading. (Don't inject a fake uploader here — let main build
    # from config so we exercise build_uploader's disabled path.)
    conf = make_config(roboflow={"enabled": False})
    pipe = IndoorPipeline(
        conf,
        frame_source=_frames(),
        detector=Detector(yolo_factory=_yolo_factory),
        poller=AssignmentPoller(
            conf.assignment.backend_url, conf.assignment.camera_id,
            conf.assignment.default_mode, session=_FakeSession(["incubation"]),
        ),
        store=None,
    )
    assert pipe.uploader is None
    assert pipe.run_once(now=1000.0)  # runs fine with no uploader


# --- logs incubation events only in incubator mode -------------------------


def test_logs_events_in_incubator_mode(make_config):
    conf = make_config()
    pipe = _build(conf, ["incubation"])
    pipe.run_once(now=1000.0)
    rows = _rows(conf.storage.db_path)
    assert len(rows) == 2  # one row per YOLO detection
    slot_ids = {r[0] for r in rows}
    event_types = {r[1] for r in rows}
    assert slot_ids == {"indoor_tapo"}          # slot_id <- camera_id
    assert event_types == {"egg", "pipped"}     # event_type <- class name
    # diff_score <- confidence; high_threshold <- model confidence threshold.
    by_type = {r[1]: r for r in rows}
    assert by_type["egg"][2] == pytest.approx(0.91)
    assert by_type["egg"][3] == pytest.approx(0.5)


def test_no_events_logged_in_chick_mode(make_config):
    conf = make_config()
    pipe = _build(conf, ["chick"])
    pipe.run_once(now=1000.0)
    assert _rows(conf.storage.db_path) == []  # chick mode logs nothing


def test_switch_stops_logging_when_moving_to_chick(make_config):
    conf = make_config()
    pipe = _build(conf, ["incubation", "chick"])
    pipe.run_once(now=1000.0)          # incubation: 2 rows
    pipe.run_once(now=1000.0 + 61)     # chick: no new rows
    assert len(_rows(conf.storage.db_path)) == 2


# --- model-not-found skips the cycle ---------------------------------------


def test_missing_model_skips_inference_without_crashing(make_config):
    conf = make_config()

    def missing_factory(weights):
        raise FileNotFoundError(weights)

    poller = AssignmentPoller(
        conf.assignment.backend_url, conf.assignment.camera_id,
        conf.assignment.default_mode, session=_FakeSession(["incubation"]),
    )
    pipe = IndoorPipeline(
        conf,
        frame_source=_frames(),
        detector=Detector(yolo_factory=missing_factory),
        poller=poller,
        uploader=_FakeUploader(),
        store=None,
    )
    # No usable model -> empty result, no exception.
    assert pipe.run_once(now=1000.0) == []
