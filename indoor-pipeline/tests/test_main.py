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
from observations import ObservationClient
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


def _build(conf, active_models, *, uploader=None, store="real", observation_client=None):
    """Assemble an IndoorPipeline wired to fakes.

    ``store="real"`` injects a temp EventStore (schema pre-applied); ``store=None``
    forces event logging off; or pass an explicit store object. ``observation_client``
    defaults to ``None`` (forced off, so no accidental network); pass a fake to
    exercise the POST path.
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
        observation_client=observation_client,
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
    # Incubation uploads are image-only: no detections are handed to the uploader.
    assert uploader.calls[0]["detections"] == []


def test_upload_project_follows_model_swap(make_config):
    conf = make_config()
    uploader = _FakeUploader()
    pipe = _build(conf, ["incubation", "chick"], uploader=uploader)
    pipe.run_once(now=1000.0)
    pipe.run_once(now=1000.0 + 61)
    projects = [c["project"] for c in uploader.calls]
    assert projects == ["incubation-stages", "find-chicks-5"]
    assert uploader.calls[1]["name"].startswith("indoor_chick_")


class _RfResp:
    status_code = 200
    text = "ok"

    def json(self):
        return {"id": "img1"}


def _capturing_pipeline(make_config, active_model, project):
    """Build a pipeline with a real RoboflowUploader whose POSTs are captured."""
    from roboflow_uploader import RoboflowUploader

    calls = []

    def fake_post(url, **kw):
        calls.append({"url": url, **kw})
        return _RfResp()

    conf = make_config()
    uploader = RoboflowUploader("KEY", "quail", project, post=fake_post)
    poller = AssignmentPoller(
        conf.assignment.backend_url, conf.assignment.camera_id,
        conf.assignment.default_mode, session=_FakeSession([active_model]),
    )
    pipe = IndoorPipeline(
        conf,
        frame_source=_frames(),
        detector=Detector(yolo_factory=_yolo_factory),
        poller=poller,
        uploader=uploader,
        store=None,
        observation_client=None,
    )
    return pipe, calls


def test_incubation_mode_uploads_image_only_no_annotation(make_config):
    """incubation-stages must receive raw images — main passes no detections to
    the uploader in incubation mode, so there is no /annotate call."""
    pipe, calls = _capturing_pipeline(make_config, "incubation", "incubation-stages")
    pipe.run_once(now=1000.0)

    assert calls, "an upload happened"
    assert any(c["url"].endswith("/upload") for c in calls)      # the image went up
    assert all("/annotate/" not in c["url"] for c in calls)      # but NO annotation


def test_chick_mode_still_uploads_yolo_annotation(make_config):
    """The chick path (find-chicks-5) is untouched — it still posts a YOLO
    annotation + labelmap."""
    pipe, calls = _capturing_pipeline(make_config, "chick", "find-chicks-5")
    pipe.run_once(now=1000.0)

    annotate = [c for c in calls if "/annotate/" in c["url"]]
    assert annotate, "chick mode posts a YOLO annotation"
    assert annotate[0]["params"]["name"].endswith(".txt")
    assert "labelmap" in annotate[0]["params"]


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


# --- rolling snapshots -----------------------------------------------------


def test_run_once_writes_rolling_snapshots_to_indoor1_path(make_config):
    import cv2

    conf = make_config(snapshots=True)  # snapshots section under tmp_path
    pipe = _build(conf, ["incubation"], store=None)
    pipe.run_once(now=1000.0)

    raw = conf.snapshots.latest_path
    annotated = conf.snapshots.latest_annotated_path
    # Snapshots land in the OBSERVATION/serving camera dir (indoor-1), which is
    # what the backend reads — NOT the assignment id (indoor_tapo).
    assert raw.parent.name == "indoor-1"
    assert annotated.parent.name == "indoor-1"
    assert raw.is_file(), "raw latest.jpg written"
    assert annotated.is_file(), "latest_annotated.jpg written"
    # The incubation fake model returns detections, so the annotated frame has
    # boxes and differs from the raw frame.
    assert not np.array_equal(cv2.imread(str(raw)), cv2.imread(str(annotated)))


def test_run_once_without_snapshots_config_is_a_noop(make_config):
    # No snapshots section -> conf.snapshots is None -> no files, no crash.
    conf = make_config()  # no snapshots
    assert conf.snapshots is None
    pipe = _build(conf, ["chick"], store=None)
    assert pipe.run_once(now=1000.0)  # runs fine


# --- observation POSTing ---------------------------------------------------


class _FakeObsResp:
    def raise_for_status(self):
        pass

    def json(self):
        return {"stored": 1, "id": 1}


class _FakeObsSession:
    """Records observation POSTs (or raises a queued error to simulate outage)."""

    def __init__(self, error=None):
        self.calls = []
        self._error = error

    def post(self, url, json=None, timeout=None):
        self.calls.append({"url": url, "json": json})
        if self._error is not None:
            raise self._error
        return _FakeObsResp()


def _obs_client(session):
    return ObservationClient("http://localhost:3000", "indoor-1", session=session)


def test_run_once_posts_observation_each_cycle_with_camera_id_and_classes(make_config):
    conf = make_config(snapshots=True)  # so image basenames are attached
    session = _FakeObsSession()
    pipe = _build(conf, ["incubation", "incubation"], store=None, observation_client=_obs_client(session))

    pipe.run_once(now=1000.0)
    pipe.run_once(now=1000.0 + 61)

    assert len(session.calls) == 2, "one POST per cycle"
    body = session.calls[0]["json"]
    assert session.calls[0]["url"] == "http://localhost:3000/api/indoorcam/observation"
    assert body["camera_id"] == "indoor-1"  # observation id, not indoor_tapo
    assert body["detection_count"] == 2
    # Class names come from the model output (egg/pipped), never hardcoded.
    assert sorted(d["class_name"] for d in body["detections"]) == ["egg", "pipped"]
    # Image fields point at the rolling snapshot basenames the backend serves.
    assert body["image_filename"] == "latest.jpg"
    assert body["annotated_image_filename"] == "latest_annotated.jpg"


def test_observation_class_names_reflect_brooder_mode(make_config):
    conf = make_config()
    session = _FakeObsSession()
    pipe = _build(conf, ["chick"], store=None, observation_client=_obs_client(session))
    pipe.run_once(now=1000.0)

    body = session.calls[0]["json"]
    assert body["camera_id"] == "indoor-1"
    assert [d["class_name"] for d in body["detections"]] == ["chick"]  # brooder mode


def test_observation_post_graceful_when_backend_unreachable(make_config):
    conf = make_config()
    session = _FakeObsSession(error=ConnectionError("backend down"))
    pipe = _build(conf, ["incubation"], store=None, observation_client=_obs_client(session))

    # The POST fails, but the cycle still completes and returns detections.
    detections = pipe.run_once(now=1000.0)
    assert {d.class_name for d in detections} == {"egg", "pipped"}
    assert len(session.calls) == 1  # it did attempt the POST


def test_no_observation_client_means_no_posts(make_config):
    # Default _build forces the observation client off -> no POSTs, no crash.
    conf = make_config()
    pipe = _build(conf, ["chick"], store=None)
    assert pipe.observation_client is None
    assert pipe.run_once(now=1000.0)


def test_observations_built_from_config_when_enabled(make_config):
    # With an observations section, the pipeline builds a client from config.
    conf = make_config(observations=True)
    poller = AssignmentPoller(
        conf.assignment.backend_url, conf.assignment.camera_id,
        conf.assignment.default_mode, session=_FakeSession(["chick"]),
    )
    pipe = IndoorPipeline(
        conf,
        frame_source=_frames(),
        detector=Detector(yolo_factory=_yolo_factory),
        poller=poller,
        uploader=_FakeUploader(),
        store=None,
        # observation_client left as default -> built from config
    )
    assert isinstance(pipe.observation_client, ObservationClient)
    assert pipe.observation_client.camera_id == "indoor-1"


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
