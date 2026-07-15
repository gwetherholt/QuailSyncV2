"""Frame grabbing for the indoor pipeline (adapted from ``incubator/camera.py``).

The rest of the pipeline consumes frames through the :class:`FrameSource`
protocol — ``grab()`` returns one BGR numpy frame (or ``None`` on failure) and
``close()`` releases resources. That indirection is deliberate: the service loop,
detector, and tests all talk to the protocol, so a test passes a
:class:`FakeFrameSource` yielding canned frames and never touches a camera.

The production implementation, :class:`OpenCVFrameSource`, opens the configured
source (an RTSP stream *or* an HTTP snapshot URL — ``cv2.VideoCapture`` handles
both) once per grab, reads a few ``warmup_frames`` to flush any stale buffered
frame, and returns the freshest one. Re-opening per grab is fine at the indoor
cadence (~1 frame / 10 s) and keeps the credentialed URL in this process's
memory — never on a command line, so it can't leak via ``ps``.
"""

from __future__ import annotations

import logging
import re
from typing import Optional, Protocol, runtime_checkable

logger = logging.getLogger("indoorpipeline.camera")


class CaptureError(Exception):
    """Raised when a frame source can't be opened or read."""


@runtime_checkable
class FrameSource(Protocol):
    """Anything that can hand the pipeline a frame.

    ``grab()`` returns a single BGR frame as a numpy ``ndarray`` (H×W×3), or
    ``None`` if this cycle's grab failed (the loop skips and tries next cycle).
    ``close()`` releases any held resources and must be idempotent.
    """

    def grab(self): ...

    def close(self) -> None: ...


def redact_source(source: Optional[str]) -> str:
    """Mask any ``user:pass@`` credentials in a source URL for safe logging."""
    if not source:
        return "<unset>"
    return re.sub(r"://[^@/]*@", "://***@", source)


class OpenCVFrameSource:
    """A :class:`FrameSource` backed by ``cv2.VideoCapture``.

    Opens the source per :meth:`grab`, reads ``warmup_frames`` throwaway frames
    to clear stale buffer, then returns the next frame. ``cv2_module`` is
    injectable so the class is testable without OpenCV.
    """

    def __init__(self, source: str, *, warmup_frames: int = 3, cv2_module=None):
        if not source:
            raise CaptureError(
                "no camera source configured — set the env var named by "
                "camera.source_env (e.g. INDOOR_RTSP_URL) in the indoor-pipeline secrets file"
            )
        self.source = source
        self.warmup_frames = max(0, int(warmup_frames))
        self._cv2 = cv2_module

    def _cv2mod(self):
        if self._cv2 is None:
            try:
                import cv2  # lazy: only a real grab needs OpenCV
            except ImportError as exc:
                raise CaptureError(
                    "opencv is required to grab frames (pip install opencv-python-headless)"
                ) from exc
            self._cv2 = cv2
        return self._cv2

    def grab(self):
        """Grab the freshest frame, or ``None`` on failure (never raises for a
        transient read miss — the loop just skips the cycle)."""
        cv2 = self._cv2mod()
        cap = cv2.VideoCapture(self.source)
        try:
            if not cap.isOpened():
                logger.warning("Could not open camera %s", redact_source(self.source))
                return None
            frame = None
            # Read warmup frames + one keeper; hold the last good frame.
            for _ in range(self.warmup_frames + 1):
                ok, candidate = cap.read()
                if ok and candidate is not None:
                    frame = candidate
            if frame is None:
                logger.warning("No frame read from camera %s", redact_source(self.source))
            return frame
        finally:
            cap.release()

    def close(self) -> None:
        # Nothing held between grabs (opened per-grab), so close is a no-op.
        return None


class FakeFrameSource:
    """A scripted :class:`FrameSource` for tests / dry runs.

    Yields the given ``frames`` in order; once exhausted it repeats the last
    frame (or returns ``None`` if constructed empty). Records ``grab_count``.
    """

    def __init__(self, frames):
        self._frames = list(frames)
        self.grab_count = 0
        self.closed = False

    def grab(self):
        idx = min(self.grab_count, len(self._frames) - 1) if self._frames else -1
        self.grab_count += 1
        if idx < 0:
            return None
        return self._frames[idx]

    def close(self) -> None:
        self.closed = True


def create_frame_source(conf, *, cv2_module=None) -> FrameSource:
    """Build the production :class:`FrameSource` from a loaded :class:`Config`.

    Raises :class:`CaptureError` if the camera source env var wasn't resolved.
    """
    source = conf.camera.source
    if not source:
        raise CaptureError(
            f"camera source is unset — export {conf.camera.source_env} "
            "(the indoor-pipeline secrets file provides it to the systemd unit)"
        )
    return OpenCVFrameSource(
        source,
        warmup_frames=conf.camera.warmup_frames,
        cv2_module=cv2_module,
    )
