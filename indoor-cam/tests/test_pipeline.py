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


def _jpg_names(cam_dir):
    return sorted(p.name for p in cam_dir.glob("*.jpg"))


def _timestamped(names):
    return [n for n in names if n not in ("latest.jpg", "latest_annotated.jpg")]


def test_startup_frame_uploaded_and_rolling_latest_written(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    results = [make_result(camera_id="indoor-1", confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader)

    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    # One POST; it always carries the rolling latest image.
    assert len(bridge.posts) == 1
    assert bridge.posts[0]["count"] == 3
    assert bridge.posts[0]["include_image"] is True
    assert bridge.posts[0]["image_path"].endswith("latest.jpg")
    # The NOTABLE (timestamped) frame is what goes to Roboflow — never the latest.
    assert len(uploader.uploads) == 1
    assert not uploader.uploads[0].endswith("latest.jpg")
    # After a successful upload the timestamped frame is reclaimed; the rolling
    # latest stays so the app keeps a current image.
    assert _jpg_names(cam_dir) == ["latest.jpg"]
    # We no longer clear image fields — the observation points at the rolling latest.
    assert bridge.cleared == []


def test_routine_post_still_carries_rolling_latest(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    # Two identical high-confidence frames, 61s apart: frame 2 posts on the
    # interval but is NOT notable — yet it still carries the rolling latest image.
    results = [
        make_result(confidences=(0.9,), total=3),
        make_result(confidences=(0.9,), total=3),
    ]
    _run(cfg, frames_ok=[True, True], results=results, bridge=bridge, uploader=uploader, tick=61.0)

    cam_dir = cfg.PROCESSED_DIR / "indoor-1"
    assert len(bridge.posts) == 2
    assert all(p["include_image"] for p in bridge.posts)
    assert all(p["image_path"].endswith("latest.jpg") for p in bridge.posts)
    assert len(uploader.uploads) == 1  # only the notable startup frame was uploaded
    # Just the single overwritten rolling file remains (no disk growth).
    assert _jpg_names(cam_dir) == ["latest.jpg"]


def test_count_change_smoothing_and_retained_frames(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=False)  # uploads fail -> notable frames kept for retry
    # frame1 count 2 (startup); frame2 count 6 -> smoothed median([2,6])=4, +2
    # from 2 -> immediate count_change post.
    results = [
        make_result(confidences=(0.9,), total=2),
        make_result(confidences=(0.9,), total=6),
    ]
    _run(cfg, frames_ok=[True, True], results=results, bridge=bridge, uploader=uploader, tick=1.0)

    assert [p["count"] for p in bridge.posts] == [2, 4]
    assert all(p["include_image"] for p in bridge.posts)
    assert len(uploader.uploads) == 2  # both notable frames attempted
    assert bridge.cleared == []
    # Both notable timestamped frames kept (upload failed) alongside the latest.
    names = _jpg_names(cfg.PROCESSED_DIR / "indoor-1")
    assert "latest.jpg" in names
    assert len(_timestamped(names)) == 2


def test_low_confidence_frame_is_retained(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=False)
    # frame 2 has a low-confidence detection (<0.4) -> notable (low_confidence).
    results = [
        make_result(confidences=(0.9,), total=3),
        make_result(confidences=(0.3,), total=3),
    ]
    _run(cfg, frames_ok=[True, True], results=results, bridge=bridge, uploader=uploader, tick=61.0)

    assert bridge.posts[1]["include_image"] is True
    names = _jpg_names(cfg.PROCESSED_DIR / "indoor-1")
    # startup + low-confidence timestamped frames retained + the rolling latest.
    assert "latest.jpg" in names
    assert len(_timestamped(names)) == 2


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
# in-app toggles gate retention + Roboflow upload, but NEVER the rolling latest
# (the app always gets a current image) nor the JSON POST.
# ---------------------------------------------------------------------------


def test_roboflow_toggle_off_skips_upload_but_retains_and_shows_image(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    settings = FakeSettings(roboflow=False, image_save=True)
    results = [make_result(confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader, settings=settings)

    # Observation posted WITH the rolling-latest image; nothing uploaded.
    assert len(bridge.posts) == 1
    assert bridge.posts[0]["include_image"] is True
    assert uploader.uploads == []
    assert bridge.cleared == []
    # image-save on -> the notable frame is retained on disk alongside the latest.
    names = _jpg_names(cfg.PROCESSED_DIR / "indoor-1")
    assert "latest.jpg" in names
    assert len(_timestamped(names)) == 1


def test_image_save_toggle_off_still_uploads_and_shows_latest(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    settings = FakeSettings(roboflow=True, image_save=False)
    results = [make_result(confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader, settings=settings)

    # The rolling latest still gives the app an image; the notable frame still
    # uploads to Roboflow, but it's NOT retained on disk (image-save off).
    assert bridge.posts[0]["include_image"] is True
    assert len(uploader.uploads) == 1
    assert not uploader.uploads[0].endswith("latest.jpg")
    assert bridge.cleared == []
    assert _jpg_names(cfg.PROCESSED_DIR / "indoor-1") == ["latest.jpg"]


def test_both_toggles_off_still_shows_rolling_latest(monkeypatch, tmp_path, make_result):
    cfg = _reload_config(monkeypatch, tmp_path)
    bridge = FakeBridge()
    uploader = FakeUploader(ok=True)
    settings = FakeSettings(roboflow=False, image_save=False)
    results = [make_result(confidences=(0.9,), total=3)]

    _run(cfg, frames_ok=[True], results=results, bridge=bridge, uploader=uploader, settings=settings)

    # Even with both toggles off, the rolling latest is written and referenced —
    # only retention + Roboflow are skipped.
    assert len(bridge.posts) == 1
    assert bridge.posts[0]["include_image"] is True
    assert bridge.posts[0]["count"] == 3
    assert uploader.uploads == []
    assert _jpg_names(cfg.PROCESSED_DIR / "indoor-1") == ["latest.jpg"]
