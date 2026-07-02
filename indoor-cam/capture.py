"""Single-frame RTSP capture for the indoor-camera pipeline.

Grabs one JPEG from the camera per poll. Two backends, selected by
``config.CAPTURE_BACKEND``:

* ``opencv`` (default, **secure**) — ``cv2.VideoCapture(url)``. The credentialed
  URL stays in this process's memory; it is never placed on a command line, so
  the RTSP password never appears in ``ps`` / ``/proc/<pid>/cmdline`` for any
  process. This is why it's the default.

* ``ffmpeg`` — the one-shot the task specifies:
  ``ffmpeg -rtsp_transport tcp -i "$RTSP_URL" -frames:v 1 -q:v 2 -y frame.jpg``.
  The URL is read from the environment (never the repo, the unit file, or our
  own CLI), but ffmpeg fundamentally needs the input URL as an ``-i`` argument,
  so it *is* visible in ``ps`` to the same user and root. Prefer ``opencv``
  unless you specifically need ffmpeg; only switch to ``ffmpeg`` where that
  argv exposure is acceptable.

Credentials are redacted in every log line via :func:`config.redact_rtsp`.
"""

from __future__ import annotations

import logging
import subprocess
import threading
from pathlib import Path

# Support both `python pipeline.py` (script, sibling import) and package import.
try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("indoorcam.capture")

# A real JPEG is at least this big; anything smaller is treated as a failed grab
# (e.g. ffmpeg wrote a 0-byte file, or OpenCV encoded nothing).
_MIN_FRAME_BYTES = 1024


class CaptureError(Exception):
    """Raised when a frame couldn't be captured (no URL, tool failure, empty)."""


# How long :meth:`StreamCapture.open` waits for the reader to produce the first
# frame (or detect an immediate drop) before returning.
_FIRST_FRAME_TIMEOUT = 10.0


class StreamCapture:
    """A long-lived OpenCV RTSP capture that always yields the *freshest* frame.

    OpenCV's FFMPEG backend buffers RTSP frames, and ``CAP_PROP_BUFFERSIZE=1``
    isn't honored by every backend — so a sampler that only calls ``read()`` once
    per second gets a frame from the *front* of the queue, seconds stale (the one
    captured right after the previous sample). To avoid that, a background thread
    continuously reads (and discards) frames, keeping only the most recent;
    :meth:`read_to` writes whatever is newest at the instant it's called. That
    keeps the buffer drained so the sampled frame — and the count derived from it
    — are from the current moment, not from just after the last POST.

    The stream is held open across frames (never reopened per grab). The
    credentialed URL stays in this process's memory and never appears on a
    command line, so the RTSP password is never exposed in ``ps`` — this is why
    continuous capture always uses OpenCV, not ffmpeg.

    ``cv2_module`` is injectable so the stream is testable without a camera.
    """

    def __init__(
        self,
        url: str | None = None,
        *,
        cv2_module=None,
        buffer_size: int = 1,
        first_frame_timeout: float = _FIRST_FRAME_TIMEOUT,
    ):
        self.url = url if url is not None else config.rtsp_url()
        if not self.url:
            raise CaptureError(
                "no RTSP URL configured — set RTSP_URL or RTSP_HOST (+ credentials) "
                "in the indoor-cam secrets file"
            )
        self._cv2 = cv2_module
        self._buffer_size = buffer_size
        self._first_frame_timeout = first_frame_timeout
        self._cap = None
        # Background reader state.
        self._thread = None
        self._lock = threading.Lock()
        self._latest = None       # newest decoded frame (protected by _lock)
        self._alive = False       # False once the stream drops -> caller reconnects
        self._running = False     # reader-thread run flag
        self._ready = None        # Event: first frame decoded (or drop detected)

    def _cv2mod(self):
        if self._cv2 is None:
            try:
                import cv2 as _cv2  # lazy: only the live stream needs it
            except ImportError as exc:
                raise CaptureError(
                    "opencv is required for the indoor-cam stream "
                    "(pip install opencv-python)"
                ) from exc
            self._cv2 = _cv2
        return self._cv2

    def open(self) -> None:
        """Open the stream and start the background reader. Raises
        :class:`CaptureError` if the stream won't connect."""
        cv2 = self._cv2mod()
        cap = cv2.VideoCapture(self.url)
        # Ask the backend to keep only the newest frame (belt-and-suspenders; the
        # reader thread is the real defense since this isn't always honored).
        try:
            cap.set(cv2.CAP_PROP_BUFFERSIZE, self._buffer_size)
        except Exception:  # noqa: BLE001 — not all backends support it
            pass
        if not cap.isOpened():
            cap.release()
            raise CaptureError(f"could not open RTSP stream {config.redact_rtsp(self.url)}")

        self._cap = cap
        with self._lock:
            self._latest = None
        self._alive = True
        self._running = True
        self._ready = threading.Event()
        self._thread = threading.Thread(target=self._reader_loop, args=(cap,), daemon=True)
        self._thread.start()
        logger.info("Opened RTSP stream %s", config.redact_rtsp(self.url))
        # Block until the first frame lands (or the stream drops / times out) so
        # the caller's first read_to() has a frame to write.
        self._ready.wait(self._first_frame_timeout)

    def _reader_loop(self, cap) -> None:
        """Continuously drain the stream, keeping only the newest frame. Exits on
        the first read failure (a dropped stream) so the caller reconnects."""
        while self._running:
            try:
                ok, frame = cap.read()  # blocks ~1/fps on a live stream
            except Exception:  # noqa: BLE001 — a read error is a dropped stream
                ok, frame = False, None
            if not ok or frame is None:
                self._alive = False
                if self._ready is not None:
                    self._ready.set()
                return
            with self._lock:
                self._latest = frame
            if self._ready is not None:
                self._ready.set()

    def read_to(self, dest: Path | str) -> bool:
        """Write the newest frame the reader has to ``dest``. Returns False when
        the stream has dropped (caller should reconnect) — never raises."""
        if self._cap is None:
            self.open()
        with self._lock:
            frame = self._latest if self._alive else None
        if frame is None:
            return False
        cv2 = self._cv2mod()
        dest = Path(dest)
        dest.parent.mkdir(parents=True, exist_ok=True)
        return bool(cv2.imwrite(str(dest), frame))

    def reconnect(self) -> None:
        """Release and reopen the stream (after a drop). The caller owns the
        backoff sleep between attempts."""
        self.release()
        self.open()

    def release(self) -> None:
        self._running = False
        thread = self._thread
        if thread is not None and thread.is_alive():
            thread.join(timeout=2.0)
        self._thread = None
        if self._cap is not None:
            try:
                self._cap.release()
            finally:
                self._cap = None
        with self._lock:
            self._latest = None
        self._alive = False


def capture_frame(
    dest: Path | str,
    *,
    backend: str | None = None,
    url: str | None = None,
    runner=subprocess.run,
    cv2_module=None,
) -> Path:
    """Capture one frame to ``dest`` (a ``.jpg`` path). Returns ``dest``.

    ``backend``/``url`` default to the configured values. ``runner`` (for the
    ffmpeg backend) and ``cv2_module`` (for the opencv backend) are injectable so
    the capture is testable without a real camera. Raises :class:`CaptureError`
    on any failure.
    """
    dest = Path(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    backend = (backend or config.CAPTURE_BACKEND).strip().lower()
    url = url if url is not None else config.rtsp_url()
    if not url:
        raise CaptureError(
            "no RTSP URL configured — set RTSP_URL or RTSP_HOST (+ credentials) "
            "in the indoor-cam secrets file"
        )

    if backend == "ffmpeg":
        _capture_ffmpeg(url, dest, runner=runner)
    elif backend == "opencv":
        _capture_opencv(url, dest, cv2_module=cv2_module)
    else:
        raise CaptureError(f"unknown CAPTURE_BACKEND {backend!r} (use 'opencv' or 'ffmpeg')")

    _validate_frame(dest)
    logger.info("Captured frame from %s -> %s", config.redact_rtsp(url), dest.name)
    return dest


def _validate_frame(dest: Path) -> None:
    """Ensure a plausible JPEG actually landed on disk."""
    if not dest.exists():
        raise CaptureError(f"capture produced no file at {dest}")
    size = dest.stat().st_size
    if size < _MIN_FRAME_BYTES:
        dest.unlink(missing_ok=True)
        raise CaptureError(f"captured frame is too small ({size} bytes) — treating as failed")


def _capture_ffmpeg(url: str, dest: Path, *, runner) -> None:
    """One-shot ffmpeg grab. NOTE: ``url`` (with credentials) is passed as the
    ``-i`` argument and is therefore visible in ``ps`` — see the module docstring.
    """
    cmd = [
        config.FFMPEG_BIN,
        "-nostdin",
        "-loglevel", "error",
        "-rtsp_transport", config.RTSP_TRANSPORT,
        "-i", url,
        "-frames:v", "1",
        "-q:v", "2",
        "-y",
        str(dest),
    ]
    try:
        proc = runner(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=config.CAPTURE_TIMEOUT,
        )
    except subprocess.TimeoutExpired as exc:
        raise CaptureError(
            f"ffmpeg timed out after {config.CAPTURE_TIMEOUT}s capturing "
            f"{config.redact_rtsp(url)}"
        ) from exc
    except FileNotFoundError as exc:
        raise CaptureError(
            f"ffmpeg binary not found ({config.FFMPEG_BIN!r}); install ffmpeg or set FFMPEG_BIN"
        ) from exc

    returncode = getattr(proc, "returncode", 1)
    if returncode != 0:
        stderr = getattr(proc, "stderr", b"") or b""
        if isinstance(stderr, bytes):
            stderr = stderr.decode("utf-8", "replace")
        # Defensively redact in case the URL is echoed in ffmpeg's error text.
        stderr = config.redact_rtsp(stderr.strip())
        raise CaptureError(f"ffmpeg exited {returncode}: {stderr}")


def _capture_opencv(url: str, dest: Path, *, cv2_module=None) -> None:
    """Grab a frame with OpenCV. The URL stays in-process (never argv).

    A few frames are read and discarded first so we encode a *fresh* frame
    rather than a stale one sitting in the capture buffer.
    """
    if cv2_module is None:
        try:
            import cv2 as cv2_module  # lazy: only the opencv backend needs it
        except ImportError as exc:
            raise CaptureError(
                "opencv backend selected but cv2 is not installed "
                "(pip install opencv-python) — or set CAPTURE_BACKEND=ffmpeg"
            ) from exc

    cap = cv2_module.VideoCapture(url)
    try:
        if not cap.isOpened():
            raise CaptureError(f"could not open RTSP stream {config.redact_rtsp(url)}")
        frame = None
        # Read a handful of frames; keep the last good one for freshness.
        for _ in range(5):
            ok, candidate = cap.read()
            if ok and candidate is not None:
                frame = candidate
        if frame is None:
            raise CaptureError(f"no frame read from {config.redact_rtsp(url)}")
        if not cv2_module.imwrite(str(dest), frame):
            raise CaptureError(f"failed to encode frame to {dest}")
    finally:
        cap.release()
