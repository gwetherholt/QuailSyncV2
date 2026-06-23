"""Tests for capture.py — the ffmpeg and opencv backends, with no real camera.

The ffmpeg backend's subprocess `runner` and the opencv backend's `cv2_module`
are injected so nothing real is spawned. We also assert the security-relevant
behaviour: the opencv backend never passes the URL to a subprocess, and ffmpeg
errors are redacted.
"""

import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from capture import CaptureError, StreamCapture, capture_frame

_JPEG = b"\xff\xd8\xff" + b"\x00" * 4096  # >1KB so it passes the size check


# --- ffmpeg backend --------------------------------------------------------


def _ffmpeg_runner(returncode=0, err=b"", produce=True, record=None):
    """A fake subprocess.run: writes a JPEG to the dest (last arg) and returns a
    completed-process stand-in with the given returncode/stderr."""

    def _run(cmd, stdout=None, stderr=None, timeout=None):
        if record is not None:
            record["cmd"] = cmd
            record["timeout"] = timeout
        if produce:
            from pathlib import Path

            Path(cmd[-1]).write_bytes(_JPEG)
        return SimpleNamespace(returncode=returncode, stdout=b"", stderr=err)

    return _run


def test_ffmpeg_success_builds_expected_command(tmp_path):
    dest = tmp_path / "frame.jpg"
    record = {}
    out = capture_frame(
        dest,
        backend="ffmpeg",
        url="rtsp://u:p@10.0.0.9:554/stream1",  # pragma: allowlist secret
        runner=_ffmpeg_runner(record=record),
    )
    assert out == dest
    assert dest.exists()
    cmd = record["cmd"]
    # One-shot, TCP transport, single video frame, overwrite.
    assert "-rtsp_transport" in cmd and "tcp" in cmd
    assert cmd[cmd.index("-i") + 1] == "rtsp://u:p@10.0.0.9:554/stream1"  # pragma: allowlist secret
    assert "-frames:v" in cmd and cmd[cmd.index("-frames:v") + 1] == "1"
    assert cmd[-1] == str(dest)


def test_ffmpeg_nonzero_exit_redacts_credentials(tmp_path):
    dest = tmp_path / "frame.jpg"
    # ffmpeg echoes the URL (with creds) in its error text; it must be redacted.
    err = b"rtsp://user:hunter2@10.0.0.9:554/stream1: 401 Unauthorized"
    with pytest.raises(CaptureError) as ei:
        capture_frame(
            dest,
            backend="ffmpeg",
            url="rtsp://user:hunter2@10.0.0.9:554/stream1",  # pragma: allowlist secret
            runner=_ffmpeg_runner(returncode=1, err=err, produce=False),
        )
    msg = str(ei.value)
    assert "hunter2" not in msg  # credential never surfaces in the error
    assert "***@10.0.0.9" in msg


def test_ffmpeg_timeout_raises_capture_error(tmp_path):
    dest = tmp_path / "frame.jpg"

    def _timeout_runner(cmd, stdout=None, stderr=None, timeout=None):
        raise subprocess.TimeoutExpired(cmd, timeout or 30)

    with pytest.raises(CaptureError) as ei:
        capture_frame(dest, backend="ffmpeg", url="rtsp://h/s", runner=_timeout_runner)
    assert "timed out" in str(ei.value)


def test_ffmpeg_empty_output_treated_as_failure(tmp_path):
    dest = tmp_path / "frame.jpg"

    def _tiny_runner(cmd, stdout=None, stderr=None, timeout=None):
        from pathlib import Path

        Path(cmd[-1]).write_bytes(b"\xff\xd8")  # 2 bytes, below the min
        return SimpleNamespace(returncode=0, stdout=b"", stderr=b"")

    with pytest.raises(CaptureError) as ei:
        capture_frame(dest, backend="ffmpeg", url="rtsp://h/s", runner=_tiny_runner)
    assert "too small" in str(ei.value)
    assert not dest.exists()  # the bogus tiny file is cleaned up


# --- opencv backend --------------------------------------------------------


class _FakeCap:
    def __init__(self, frames, opened=True):
        self._frames = list(frames)
        self._opened = opened
        self.released = False
        self.url = None

    def isOpened(self):
        return self._opened

    def read(self):
        if self._frames:
            return True, self._frames.pop(0)
        return False, None

    def release(self):
        self.released = True


class _FakeCv2:
    """Minimal cv2 stand-in. VideoCapture records the URL; imwrite writes a real
    (>1KB) file so the size check passes."""

    def __init__(self, cap):
        self._cap = cap
        self.imwrite_calls = 0

    def VideoCapture(self, url):
        self._cap.url = url
        return self._cap

    def imwrite(self, path, frame):
        from pathlib import Path

        Path(path).write_bytes(_JPEG)
        self.imwrite_calls += 1
        return True


def test_opencv_success_keeps_url_out_of_argv(tmp_path, monkeypatch):
    dest = tmp_path / "frame.jpg"
    cap = _FakeCap(frames=["frame-a", "frame-b"])
    cv2 = _FakeCv2(cap)

    # Guard: the opencv path must never spawn a subprocess (URL would leak to ps).
    def _boom(*a, **k):
        raise AssertionError("opencv backend must not call subprocess.run")

    monkeypatch.setattr("capture.subprocess.run", _boom)

    out = capture_frame(
        dest,
        backend="opencv",
        url="rtsp://u:p@cam/stream1",  # pragma: allowlist secret
        cv2_module=cv2,
    )
    assert out == dest
    assert dest.exists()
    assert cv2.imwrite_calls == 1
    assert cap.released is True
    assert cap.url == "rtsp://u:p@cam/stream1"  # pragma: allowlist secret


def test_opencv_stream_not_opened_raises(tmp_path):
    dest = tmp_path / "frame.jpg"
    cap = _FakeCap(frames=[], opened=False)
    with pytest.raises(CaptureError) as ei:
        capture_frame(dest, backend="opencv", url="rtsp://cam/s", cv2_module=_FakeCv2(cap))
    assert "could not open" in str(ei.value)
    assert cap.released is True


def test_opencv_no_frame_raises(tmp_path):
    dest = tmp_path / "frame.jpg"
    cap = _FakeCap(frames=[], opened=True)
    with pytest.raises(CaptureError) as ei:
        capture_frame(dest, backend="opencv", url="rtsp://cam/s", cv2_module=_FakeCv2(cap))
    assert "no frame" in str(ei.value)


# --- continuous StreamCapture ----------------------------------------------


class _StreamCap:
    def __init__(self, frames, opened=True):
        self._frames = list(frames)
        self._opened = opened
        self.released = False
        self.buffer_set = None
        self.url = None

    def isOpened(self):
        return self._opened

    def set(self, prop, value):
        self.buffer_set = (prop, value)
        return True

    def read(self):
        if self._frames:
            return True, self._frames.pop(0)
        return False, None

    def release(self):
        self.released = True


class _StreamCv2:
    CAP_PROP_BUFFERSIZE = 38  # the real cv2 constant value

    def __init__(self, caps):
        self._caps = list(caps)
        self.opened_urls = []

    def VideoCapture(self, url):
        self.opened_urls.append(url)
        return self._caps.pop(0)

    def imwrite(self, path, frame):
        Path(path).write_bytes(_JPEG)
        return True


def test_stream_open_sets_buffer_and_reads_fresh_frame(tmp_path):
    cap = _StreamCap(frames=["frame-a"])
    cv2 = _StreamCv2([cap])
    stream = StreamCapture(url="rtsp://u:p@cam/stream1", cv2_module=cv2)  # pragma: allowlist secret
    stream.open()
    # Buffer trimmed to the latest frame so the 1fps sampler reads fresh data.
    assert cap.buffer_set == (cv2.CAP_PROP_BUFFERSIZE, 1)

    dest = tmp_path / "live.jpg"
    assert stream.read_to(dest) is True
    assert dest.exists()
    assert cv2.opened_urls == ["rtsp://u:p@cam/stream1"]  # pragma: allowlist secret


def test_stream_open_raises_when_not_opened(tmp_path):
    cap = _StreamCap(frames=[], opened=False)
    stream = StreamCapture(url="rtsp://cam/s", cv2_module=_StreamCv2([cap]))
    with pytest.raises(CaptureError):
        stream.open()
    assert cap.released is True  # the failed handle is released


def test_stream_read_to_returns_false_on_drop(tmp_path):
    cap = _StreamCap(frames=[])  # opened, but no frames -> read() fails
    stream = StreamCapture(url="rtsp://cam/s", cv2_module=_StreamCv2([cap]))
    assert stream.read_to(tmp_path / "live.jpg") is False  # caller reconnects


def test_stream_reconnect_reopens_with_a_fresh_handle(tmp_path):
    first = _StreamCap(frames=[])
    second = _StreamCap(frames=["frame-b"])
    cv2 = _StreamCv2([first, second])
    stream = StreamCapture(url="rtsp://cam/s", cv2_module=cv2)
    stream.open()
    stream.reconnect()
    assert first.released is True  # old handle closed
    assert len(cv2.opened_urls) == 2  # reopened
    assert stream.read_to(tmp_path / "live.jpg") is True  # new handle works


def test_stream_requires_a_url(monkeypatch):
    import config

    monkeypatch.setattr(config, "RTSP_URL", None, raising=False)
    monkeypatch.setattr(config, "RTSP_HOST", None, raising=False)
    with pytest.raises(CaptureError):
        StreamCapture(url=None, cv2_module=_StreamCv2([]))


# --- dispatch / config -----------------------------------------------------


def test_missing_url_raises(tmp_path):
    with pytest.raises(CaptureError) as ei:
        capture_frame(tmp_path / "f.jpg", backend="ffmpeg", url="")
    assert "no RTSP URL" in str(ei.value)


def test_unknown_backend_raises(tmp_path):
    with pytest.raises(CaptureError) as ei:
        capture_frame(tmp_path / "f.jpg", backend="magic", url="rtsp://cam/s")
    assert "unknown CAPTURE_BACKEND" in str(ei.value)
