"""Tests for storage.py — sqlite event logging and crop saving.

The Rust backend owns the ``incubation_events`` schema in production, so these
tests create it via the ``event_store`` fixture (see conftest.py) rather than
relying on the sidecar to build it.
"""

from datetime import datetime, timezone

import numpy as np

import storage


def test_insert_and_read_back(event_store):
    store = event_store()
    event_id = store.record_event(
        slot_id="A1",
        diff_score=42.5,
        high_threshold=18.0,
        clutch_id=7,
        frame_path="/caps/2026-07-12/slot_A1_x.jpg",
    )
    assert event_id >= 1

    row = store.conn.execute(
        "SELECT slot_id, event_type, diff_score, high_threshold, clutch_id, frame_path "
        "FROM incubation_events WHERE id=?",
        (event_id,),
    ).fetchone()

    assert row[0] == "A1"
    assert row[1] == "change_detected"          # column default
    assert row[2] == 42.5
    assert row[3] == 18.0
    assert row[4] == 7
    assert row[5] == "/caps/2026-07-12/slot_A1_x.jpg"


def test_created_at_defaults_to_utc_iso(event_store):
    store = event_store()
    eid = store.record_event(slot_id="B2", diff_score=1.0, high_threshold=1.0)
    created = store.conn.execute(
        "SELECT created_at FROM incubation_events WHERE id=?", (eid,)
    ).fetchone()[0]
    # e.g. 2026-07-12T15:30:00.123Z — ends in Z, parseable as a datetime.
    assert created.endswith("Z")
    datetime.fromisoformat(created.replace("Z", "+00:00"))


def test_null_clutch_and_frame_path_are_allowed(event_store):
    store = event_store()
    eid = store.record_event(slot_id="A1", diff_score=5.0, high_threshold=18.0)
    row = store.conn.execute(
        "SELECT clutch_id, frame_path FROM incubation_events WHERE id=?", (eid,)
    ).fetchone()
    assert row == (None, None)


def test_wal_mode_and_busy_timeout_applied(event_store):
    store = event_store(busy_timeout_ms=1234)
    mode = store.conn.execute("PRAGMA journal_mode;").fetchone()[0]
    timeout = store.conn.execute("PRAGMA busy_timeout;").fetchone()[0]
    assert mode.lower() == "wal"
    assert timeout == 1234


def test_connect_does_not_create_schema(tmp_path):
    # The sidecar no longer owns the schema: opening a store against a DB with no
    # incubation_events table must NOT create it (that's the backend's job).
    store = storage.EventStore(tmp_path / "empty.db")
    try:
        exists = store.conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='incubation_events'"
        ).fetchone()
    finally:
        store.close()
    assert exists is None


def test_indexes_present_from_migration(event_store):
    store = event_store()
    names = {
        r[0]
        for r in store.conn.execute(
            "SELECT name FROM sqlite_master WHERE type='index'"
        ).fetchall()
    }
    assert "idx_incubation_events_slot" in names
    assert "idx_incubation_events_created" in names


def test_rows_persist_across_store_reopen(event_store, tmp_path):
    db = tmp_path / "persist.db"
    s1 = event_store(db)
    s1.record_event(slot_id="A1", diff_score=1.0, high_threshold=1.0)
    s1.close()
    # A second connection (as the Rust backend would) sees the row; the schema
    # was created once (by the migration/fixture) and survives the reopen.
    s2 = storage.EventStore(db)
    try:
        count = s2.conn.execute("SELECT COUNT(*) FROM incubation_events").fetchone()[0]
    finally:
        s2.close()
    assert count == 1


# --- crop path / saving ----------------------------------------------------


def test_crop_path_layout():
    when = datetime(2026, 7, 12, 15, 30, 45, 123000, tzinfo=timezone.utc)
    path = storage.crop_path("/caps", "A1", when)
    assert path.parent.name == "2026-07-12"
    assert path.name == "slot_A1_20260712T153045123Z.jpg"


def test_crop_path_sanitizes_slot_id():
    when = datetime(2026, 7, 12, 0, 0, 0, tzinfo=timezone.utc)
    path = storage.crop_path("/caps", "A/1 weird", when)
    assert "/" not in path.name.replace("slot_", "", 1).split(".")[0]
    assert path.name.startswith("slot_A_1_weird_")


def test_save_crop_writes_file_and_returns_path(tmp_path):
    crop = np.full((20, 20, 3), 128, dtype=np.uint8)
    when = datetime(2026, 7, 12, 15, 30, 45, 0, tzinfo=timezone.utc)
    out = storage.save_crop(crop, tmp_path / "caps", "A1", when)
    assert out.exists()
    assert out.parent.name == "2026-07-12"
    assert out.suffix == ".jpg"


def test_frame_path_records_saved_crop_path(event_store, tmp_path):
    # End-to-end: save a crop, then record its path in the DB and read it back.
    crop = np.full((16, 16, 3), 200, dtype=np.uint8)
    when = datetime(2026, 7, 12, 12, 0, 0, tzinfo=timezone.utc)
    saved = storage.save_crop(crop, tmp_path / "caps", "C3", when)

    store = event_store()
    eid = store.record_event(
        slot_id="C3", diff_score=20.0, high_threshold=18.0, frame_path=saved
    )
    recorded = store.conn.execute(
        "SELECT frame_path FROM incubation_events WHERE id=?", (eid,)
    ).fetchone()[0]
    assert recorded == str(saved)
