"""Tests for storage.py — the notability decision and the disk helpers."""

import os
import time

from storage import delete_files, notable_reasons, persist_frame, prune_old_images


# --- notable_reasons -------------------------------------------------------


def _reasons(**over):
    base = dict(
        post_reason="interval",
        min_confidence=0.9,
        is_first=False,
        seconds_since_last_image=10.0,
        low_confidence_threshold=0.4,
        heartbeat_interval=3600,
    )
    base.update(over)
    return notable_reasons(**base)


def test_routine_frame_is_not_saved():
    # High confidence, no count change, recent image, not first -> no save.
    assert _reasons() == []


def test_startup_frame_is_saved():
    assert "startup" in _reasons(is_first=True, seconds_since_last_image=None)


def test_count_change_frame_is_saved():
    assert "count_change" in _reasons(post_reason="count_change")


def test_low_confidence_frame_is_saved():
    assert "low_confidence" in _reasons(min_confidence=0.3)
    # Exactly at the threshold is NOT low (strict <).
    assert "low_confidence" not in _reasons(min_confidence=0.4)
    # No detections -> no confidence signal -> not a low-confidence save.
    assert "low_confidence" not in _reasons(min_confidence=None)


def test_heartbeat_when_interval_elapsed():
    assert _reasons(seconds_since_last_image=3600) == ["heartbeat"]
    assert _reasons(seconds_since_last_image=4000) == ["heartbeat"]
    assert _reasons(seconds_since_last_image=3599) == []


def test_heartbeat_when_never_saved_and_not_first():
    # Posted before but never saved an image -> due for a heartbeat.
    assert _reasons(seconds_since_last_image=None) == ["heartbeat"]


def test_multiple_reasons_accumulate():
    reasons = _reasons(post_reason="count_change", min_confidence=0.1, seconds_since_last_image=9999)
    assert set(reasons) == {"count_change", "low_confidence", "heartbeat"}


# --- persist / delete ------------------------------------------------------


def test_persist_frame_copies_into_camera_dir(tmp_path):
    live = tmp_path / "live.jpg"
    live.write_bytes(b"\xff\xd8\xff data")
    dest_dir = tmp_path / "processed" / "indoor-1"
    out = persist_frame(live, dest_dir, "20260101-120000_indoor-1")
    assert out == dest_dir / "20260101-120000_indoor-1.jpg"
    assert out.read_bytes() == b"\xff\xd8\xff data"
    assert live.exists()  # original (live temp) left in place


def test_delete_files_is_tolerant(tmp_path):
    a = tmp_path / "a.jpg"
    a.write_bytes(b"x")
    missing = tmp_path / "missing.jpg"
    delete_files(a, missing, None)  # no error on missing/None
    assert not a.exists()


# --- prune -----------------------------------------------------------------


def test_prune_removes_only_old_images(tmp_path):
    root = tmp_path / "processed"
    cam = root / "indoor-1"
    cam.mkdir(parents=True)
    old = cam / "old.jpg"
    new = cam / "new.jpg"
    old.write_bytes(b"x")
    new.write_bytes(b"x")
    now = time.time()
    # old is 8 days old, new is 1 hour old; retention is 7 days.
    os.utime(old, (now - 8 * 86400, now - 8 * 86400))
    os.utime(new, (now - 3600, now - 3600))

    removed = prune_old_images(root, retention_days=7, now=now)
    assert removed == 1
    assert not old.exists()
    assert new.exists()


def test_prune_missing_root_is_noop(tmp_path):
    assert prune_old_images(tmp_path / "nope", retention_days=7) == 0
