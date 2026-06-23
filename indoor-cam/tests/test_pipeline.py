"""Tests for the continuous indoor-cam pipeline: CountBatcher smoothing/batching
and run_stream's capture -> detect -> smart-batch-post -> storage -> Roboflow
wiring, all driven with injected fakes (no camera/model/server/Roboflow)."""

import importlib
import threading
from pathlib import Path

import config
import pipeline
from pipeline import CountBatcher, run_stream


# ---------------------------------------------------------------------------
# CountBatcher
# ---------------------------------------------------------------------------


def test_batcher_median_smoothing():
    b = CountBatcher(window=5, post_interval=60, change_threshold=2)
    assert b.add(3) == 3
    assert b.add(3) == 3  # median([3,3]) = 3
    assert b.add(9) == 3  # median([3,3,9]) = 3 (median ignores the spike)
    assert b.window == [3, 3, 9]


def test_batcher_window_is_bounded():
    b = CountBatcher(window=3, post_interval=60, change_threshold=2)
    for n in (1, 2, 3, 4, 5):
        b.add(n)
    assert b.window == [3, 4, 5]  # only the last 3 kept


def test_batcher_should_post_decisions():
    b = CountBatcher(window=5, post_interval=60, change_threshold=2)
    # First call always posts.
    assert b.should_post(4, now=100.0) == (True, "first")
    b.record_post(4, now=100.0)
    # No change, within the interval -> no post.
    assert b.should_post(4, now=130.0) == (False, "")
    assert b.should_post(5, now=130.0) == (False, "")  # +1 < threshold
    # Change of >= 2 -> immediate post.
    assert b.should_post(6, now=130.0) == (True, "count_change")
    assert b.should_post(2, now=130.0) == (True, "count_change")
    # Interval elapsed -> post even with no change.
    assert b.should_post(4, now=160.0) == (True, "interval")


# ---------------------------------------------------------------------------
# run_stream fakes
# ---------------------------------------------------------------------------


class FakeStream:
    def __init__(self, frames_ok, clock_holder, tick=1.0):
        self.frames_ok = list(frames_ok)
        self.clock = clock_holder
        self.tick = tick
        self.reconnects = 0
        self.released = False

    def read_to(self, dest):
        self.clock["t"] += self.tick  # advance the (fake) monotonic clock
        ok = self.frames_ok.pop(0) if self.frames_ok else False
        if ok:
            Path(dest).parent.mkdir(parents=True, exist_ok=True)
            Path(dest).write_bytes(b"\xff\xd8\xff" + b"\x00" * 100)
        return ok

    def reconnect(self):
        self.reconnects += 1

    def release(self):
        self.released = True


class FakeBridge:
    def __init__(self):
        self.posts = []
        self.cleared = []
        self._next_id = 100

    def post(self, result, timestamp=None, detection_count=None, include_image=True):
        self.posts.append(
            {
                "count": detection_count,
                "include_image": include_image,
                "image_path": result.image_path,
            }
        )
        rid = self._next_id
        self._next_id += 1
        return rid

    def clear_image(self, observation_id):
        self.cleared.append(observation_id)
        return True


class FakeUploader:
    def __init__(self, ok=True, enabled=True):
        self.ok = ok
        self.enabled = enabled
        self.uploads = []

    def upload(self, result):
        self.uploads.append(result.image_path)
        return self.ok


class FakeSettings:
    """Stand-in for SettingsClient — fixed toggle values, no network."""

    def __init__(self, roboflow=True, image_save=True):
        self._roboflow = roboflow
        self._image_save = image_save

    def roboflow_upload_enabled(self):
        return self._roboflow

    def image_save_enabled(self):
        return self._image_save


def _detect_from(results):
    it = iter(results)

    def detect_fn(path, camera_id):
        return next(it)

    return detect_fn


def _reload_config(monkeypatch, tmp_path, **over):
    monkeypatch.setenv("INDOORCAM_BASE_DIR", str(tmp_path))
    monkeypatch.setenv("INDOOR_CAMERA_ID", "indoor-1")
    monkeypatch.setenv("FRAME_INTERVAL", "0")  # no real pacing sleep in tests
    monkeypatch.setenv("POST_INTERVAL", "60")
    monkeypatch.setenv("COUNT_CHANGE_THRESHOLD", "2")
    monkeypatch.setenv("LOW_CONFIDENCE_THRESHOLD", "0.4")
    monkeypatch.setenv("HEARTBEAT_IMAGE_INTERVAL", "3600")
    monkeypatch.setenv("STREAM_RECONNECT_BACKOFF", "0")
    monkeypatch.setenv("STREAM_MAX_BACKOFF", "0")
    for k, v in over.items():
        monkeypatch.setenv(k, v)
    importlib.reload(config)
    return config


def _run(cfg, *, frames_ok, results, bridge, uploader, settings=None, tick=1.0, max_iterations=None):
    clock_holder = {"t": 1000.0}
    stream = FakeStream(frames_ok, clock_holder, tick=tick)
    run_stream(
        capture=stream,
        detect_fn=_detect_from(results),
        annotate_fn=lambda *a: True,  # no-op (doesn't write an annotated file)
        bridge=bridge,
        uploader=uploader,
        settings_client=settings if settings is not None else FakeSettings(),  # both ON
        clock=lambda: clock_holder["t"],
        wall_clock=lambda: 1_700_000_000.0,
        stop_event=threading.Event(),
        install_signals=False,
        max_iterations=max_iterations if max_iterations is not None else len(frames_ok),
    )
    return stream


# ---------------------------------------------------------------------------
# run_stream behaviour
# ---------------------------------------------------------------------------


def test_startup_frame_saved_uploaded_and_deleted(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    results = [make_result(camera_id="indoor-1", confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader)

    # One POST, smoothed count = 3, image included (startup save).
    assert len(bridge.posts) == 1
    assert bridge.posts[0]["count"] == 3
    assert bridge.posts[0]["include_image"] is True
    # Uploaded to Roboflow, then the local frame was deleted (upload ok)...
    assert len(uploader.uploads) == 1
    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    assert list(cam_dir.glob("*.jpg")) == []  # reclaimed after successful upload
    # ...and the server was told to clear that observation's image fields.
    assert bridge.cleared == [100]  # the first POST's observation id


def test_routine_post_has_no_image(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    # Two identical, high-confidence frames; advance 61s/frame so frame 2 posts
    # on the interval but is NOT notable -> no image saved.
    results = [
        make_result(confidences=(0.9,), total=3),
        make_result(confidences=(0.9,), total=3),
    ]
    _run(cfg, frames_ok=[True, True], results=results, bridge=bridge, uploader=uploader, tick=61.0)

    assert len(bridge.posts) == 2
    assert bridge.posts[0]["include_image"] is True   # startup
    assert bridge.posts[1]["include_image"] is False  # routine interval post
    assert bridge.posts[1]["count"] == 3
    assert len(uploader.uploads) == 1  # only the startup frame was uploaded
    # Only the uploaded+deleted startup frame had its image fields cleared.
    assert bridge.cleared == [100]


def test_count_change_triggers_immediate_post_and_smoothing(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=False)  # uploads fail -> frames kept for retry
    # frame1 count 2 (startup); frame2 count 6 within the interval -> smoothed
    # median([2,6])=4, which is +2 from 2 -> immediate count_change post.
    results = [
        make_result(confidences=(0.9,), total=2),
        make_result(confidences=(0.9,), total=6),
    ]
    _run(cfg, frames_ok=[True, True], results=results, bridge=bridge, uploader=uploader, tick=1.0)

    assert [p["count"] for p in bridge.posts] == [2, 4]
    assert [p["include_image"] for p in bridge.posts] == [True, True]
    # Both uploads attempted but failed -> both frames kept on disk for retry,
    # and no image fields were cleared (the files still exist to be served).
    assert len(uploader.uploads) == 2
    assert bridge.cleared == []
    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    assert len(list(cam_dir.glob("*.jpg"))) == 2


def test_low_confidence_frame_is_saved(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=False)
    # Two frames, same count, but frame 2 has a low-confidence detection (<0.4)
    # and posts on the interval -> notable (low_confidence) -> image saved.
    results = [
        make_result(confidences=(0.9,), total=3),
        make_result(confidences=(0.3,), total=3),
    ]
    _run(cfg, frames_ok=[True, True], results=results, bridge=bridge, uploader=uploader, tick=61.0)

    assert bridge.posts[1]["include_image"] is True  # saved for being uncertain
    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    assert len(list(cam_dir.glob("*.jpg"))) == 2  # startup + low-confidence


def test_dropped_frame_triggers_reconnect(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    # First read drops (reconnect), second succeeds (posts).
    results = [make_result(confidences=(0.9,), total=4)]
    stream = _run(
        cfg, frames_ok=[False, True], results=results, bridge=bridge, uploader=uploader
    )

    assert stream.reconnects == 1
    assert len(bridge.posts) == 1
    assert stream.released is True  # stream cleaned up on exit


# ---------------------------------------------------------------------------
# in-app toggles (system settings) gate save + upload, never the JSON POST
# ---------------------------------------------------------------------------


def test_roboflow_toggle_off_skips_upload_but_keeps_image(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    settings = FakeSettings(roboflow=False, image_save=True)
    results = [make_result(confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader, settings=settings)

    # Observation posted WITH an image; nothing uploaded or cleared.
    assert len(bridge.posts) == 1
    assert bridge.posts[0]["include_image"] is True
    assert uploader.uploads == []
    assert bridge.cleared == []
    # The frame is kept on disk (saved to PC).
    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    assert len(list(cam_dir.glob("*.jpg"))) == 1


def test_image_save_toggle_off_skips_disk_but_still_uploads(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    settings = FakeSettings(roboflow=True, image_save=False)
    results = [make_result(confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader, settings=settings)

    # Posted WITHOUT an image; uploaded to Roboflow; the temp frame is not kept.
    assert len(bridge.posts) == 1
    assert bridge.posts[0]["include_image"] is False
    assert len(uploader.uploads) == 1
    assert bridge.cleared == []  # nothing to clear (no image fields were set)
    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    assert list(cam_dir.glob("*.jpg")) == []  # not saved to PC


def test_both_toggles_off_posts_json_only(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    settings = FakeSettings(roboflow=False, image_save=False)
    results = [make_result(confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader, settings=settings)

    # JSON observation still posted; no image, no upload, no disk write.
    assert len(bridge.posts) == 1
    assert bridge.posts[0]["include_image"] is False
    assert bridge.posts[0]["count"] == 3
    assert uploader.uploads == []
    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    assert not cam_dir.exists() or list(cam_dir.glob("*.jpg")) == []
