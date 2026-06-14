"""Tests for PhotoState — JSON-backed dedup tracking."""

from spypoint_poller import PhotoState


def test_missing_file_starts_empty(tmp_path):
    state = PhotoState(tmp_path / "state.json")
    assert len(state) == 0
    assert not state.has_seen("anything")


def test_mark_and_query(tmp_path):
    state = PhotoState(tmp_path / "state.json")
    state.mark_seen("photo-1")
    assert state.has_seen("photo-1")
    assert not state.has_seen("photo-2")
    assert len(state) == 1


def test_ids_normalized_to_strings(tmp_path):
    state = PhotoState(tmp_path / "state.json")
    state.mark_seen(123)  # int in
    assert state.has_seen(123)
    assert state.has_seen("123")  # queried as str


def test_save_and_reload_persists(tmp_path):
    path = tmp_path / "state.json"
    state = PhotoState(path)
    state.mark_seen("a")
    state.mark_seen("b")
    state.save()
    assert path.exists()

    reloaded = PhotoState(path)
    assert reloaded.has_seen("a")
    assert reloaded.has_seen("b")
    assert len(reloaded) == 2


def test_save_is_atomic_no_tmp_left(tmp_path):
    path = tmp_path / "state.json"
    state = PhotoState(path)
    state.mark_seen("a")
    state.save()
    assert not path.with_suffix(".json.tmp").exists()


def test_corrupt_file_starts_empty(tmp_path):
    path = tmp_path / "state.json"
    path.write_text("{ this is not valid json", encoding="utf-8")
    state = PhotoState(path)  # must not raise
    assert len(state) == 0
