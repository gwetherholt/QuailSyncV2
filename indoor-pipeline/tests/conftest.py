"""Shared test setup for the indoor-pipeline suite.

Puts the package directory (``indoor-pipeline/``) on ``sys.path`` so the tests
can ``import config`` / ``import detector`` / … by bare name — the same
convention the incubator suite uses. The pipeline modules fall back from
``from . import config`` to ``import config`` when imported this way, so the
bare-name and package imports resolve to the same objects.

No real camera, ultralytics model, or backend is ever touched: frames are numpy
arrays, YOLO is a fake factory, and HTTP is a fake session/post.
"""

from __future__ import annotations

import json
import os
import sqlite3
import sys

import numpy as np
import pytest

_PKG_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PKG_DIR not in sys.path:
    sys.path.insert(0, _PKG_DIR)


# --- incubation_events schema (test-only) ----------------------------------
# In production the Rust backend owns this schema; the sidecar assumes the table
# exists. The storage tests need a self-contained temp DB, so the DDL lives here,
# mirroring the backend migration. Keep in sync with the incubator suite.
INCUBATION_EVENTS_DDL = (
    """
    CREATE TABLE IF NOT EXISTS incubation_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        slot_id TEXT NOT NULL,
        event_type TEXT NOT NULL DEFAULT 'change_detected',
        diff_score REAL NOT NULL,
        high_threshold REAL NOT NULL,
        clutch_id INTEGER,
        frame_path TEXT,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    """,
    "CREATE INDEX IF NOT EXISTS idx_incubation_events_slot ON incubation_events(slot_id);",
    "CREATE INDEX IF NOT EXISTS idx_incubation_events_created ON incubation_events(created_at);",
)


def apply_incubation_schema(db_path) -> None:
    """Create the incubation_events table + indexes in ``db_path`` (test setup)."""
    conn = sqlite3.connect(str(db_path))
    try:
        for stmt in INCUBATION_EVENTS_DDL:
            conn.execute(stmt)
        conn.commit()
    finally:
        conn.close()


def _config_dict(tmp_path, *, roboflow=None, storage=None, assignment=None, snapshots=None, observations=None):
    """A full, valid config dict pointing at temp paths (overridable per section).

    ``snapshots``/``observations`` may be ``True`` (add the section, pointing
    under tmp_path / at indoor-1), a dict (explicit values), or ``None``/omitted.
    """
    data = {
        "camera": {"source_env": "INDOOR_RTSP_URL", "capture_interval_seconds": 10},
        "assignment": {
            "backend_url": "http://localhost:3000",
            "camera_id": "indoor_tapo",
            "poll_seconds": 60,
            "default_mode": "incubator",
        },
        "models": {
            "incubation": {
                "weights": "/models/incubation-best.pt",
                "confidence": 0.5,
                "roboflow_project": "incubation-stages",
                "log_events": True,
            },
            "chick": {
                "weights": "/models/chick-best.pt",
                "confidence": 0.5,
                "roboflow_project": "find-chicks-5",
                "log_events": False,
            },
        },
        "roboflow": {
            "enabled": True,
            "workspace": "quail",
            "upload_interval_seconds": 1800,
            "upload_on_detection": True,
            "api_key_env": "ROBOFLOW_API_KEY",
            "batch_name": "indoor-auto",
        },
        "storage": {
            "db_path": str(tmp_path / "quailsync.db"),
            "sqlite_busy_timeout_ms": 5000,
        },
    }
    if roboflow is not None:
        data["roboflow"].update(roboflow)
    if storage is not None:
        data["storage"].update(storage)
    if assignment is not None:
        data["assignment"].update(assignment)
    if snapshots is True:
        # Serving/snapshot dir is keyed on the OBSERVATION id (indoor-1), not the
        # assignment id (indoor_tapo).
        snap_dir = tmp_path / "processed" / "indoor-1"
        data["snapshots"] = {
            "latest_path": str(snap_dir / "latest.jpg"),
            "latest_annotated_path": str(snap_dir / "latest_annotated.jpg"),
        }
    elif isinstance(snapshots, dict):
        data["snapshots"] = snapshots
    if observations is True:
        data["observations"] = {
            "enabled": True,
            "backend_url": "http://localhost:3000",
            "camera_id": "indoor-1",
        }
    elif isinstance(observations, dict):
        data["observations"] = observations
    return data


@pytest.fixture
def make_config(tmp_path):
    """Factory: write a config.json to a temp path and load it.

    ``make_config(env=..., roboflow=..., storage=..., assignment=..., snapshots=...,
    observations=...)``. ``snapshots=True``/``observations=True`` add those
    sections (pointing under tmp_path / at indoor-1).
    """
    import config as cfg

    def _make(*, env=None, roboflow=None, storage=None, assignment=None, snapshots=None, observations=None):
        data = _config_dict(
            tmp_path, roboflow=roboflow, storage=storage, assignment=assignment,
            snapshots=snapshots, observations=observations,
        )
        path = tmp_path / "config.json"
        path.write_text(json.dumps(data), encoding="utf-8")
        return cfg.load_config(path, env=env if env is not None else {"INDOOR_RTSP_URL": "rtsp://x"})

    return _make


@pytest.fixture
def frame():
    """Factory: a solid-gray HxWx3 BGR uint8 frame."""

    def _make(height=48, width=64, value=100):
        return np.full((height, width, 3), value, dtype=np.uint8)

    return _make
