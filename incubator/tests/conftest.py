"""Shared test setup for the incubator suite.

Puts the package directory (``incubator/``) on ``sys.path`` so the tests can
``import config`` / ``import roi`` / … by bare name — the same convention the
trail-cam and indoor-cam suites use. The pipeline modules themselves fall back
from ``from . import config`` to ``import config`` when imported this way, so the
bare-name and package imports resolve to the same objects.

Also provides small helpers for synthesizing frames and slots so no test needs a
real camera.
"""

from __future__ import annotations

import os
import sqlite3
import sys

import numpy as np
import pytest

_PKG_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PKG_DIR not in sys.path:
    sys.path.insert(0, _PKG_DIR)


# --- incubation_events schema (test-only) ----------------------------------
# In production the Rust backend owns this schema (created via its migration
# layer) and the sidecar assumes the table exists — so storage.py no longer
# carries any CREATE TABLE. The storage tests still need a self-contained temp
# DB, so the DDL lives here, in test-only setup, mirroring the migration.
# Keep this byte-for-byte in sync with the backend migration.
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


@pytest.fixture
def event_store(tmp_path):
    """Factory returning a ``storage.EventStore`` on a temp DB whose schema is
    pre-created (standing in for the backend's migration).

    Call ``event_store()`` for the default temp DB, or ``event_store(path)`` to
    use a specific path (e.g. to reopen the same DB twice). All created stores
    are closed at teardown.
    """
    import storage

    created = []

    def _make(db_path=None, busy_timeout_ms=5000):
        if db_path is None:
            db_path = tmp_path / "incubator.db"
        apply_incubation_schema(db_path)
        store = storage.EventStore(db_path, busy_timeout_ms=busy_timeout_ms)
        created.append(store)
        return store

    yield _make

    for store in created:
        store.close()


@pytest.fixture
def solid_frame():
    """Factory: a solid-gray HxWx3 BGR uint8 frame."""

    def _make(height=240, width=320, value=100):
        return np.full((height, width, 3), value, dtype=np.uint8)

    return _make


@pytest.fixture
def make_slot():
    """Factory for a config.Slot with sensible defaults."""
    import config

    def _make(slot_id="A1", bbox=(10, 10, 40, 40), clutch_id=None):
        return config.Slot(id=slot_id, bbox=tuple(bbox), clutch_id=clutch_id)

    return _make
