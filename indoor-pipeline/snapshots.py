"""Rolling snapshot writer: latest.jpg (raw) + latest_annotated.jpg (YOLO boxes).

After each capture cycle the pipeline overwrites two flat files the backend/app
serve as the live camera feed:

* ``latest.jpg``           — the raw frame,
* ``latest_annotated.jpg`` — a copy with each YOLO detection's bounding box,
  class label, and confidence drawn on it (or, when there are no detections, a
  plain copy of the raw frame so the file always exists and stays fresh).

Both are written **atomically** — encoded to a sibling ``.tmp`` file, then
``os.replace``d into place — so a reader (the backend serving the feed) never
sees a half-written JPEG. ``os.replace`` is atomic on the same filesystem, and
the temp file lives in the destination directory to guarantee that.

This mirrors the rolling-latest behavior of the old ``indoor-cam`` pipeline
(``persist_frame(..., atomic=True)`` + annotate-to-temp-then-replace) so the
existing backend camera-serving code works unchanged. ``cv2`` is injectable for
testing.
"""

from __future__ import annotations

import logging
import os
from pathlib import Path

logger = logging.getLogger("indoorpipeline.snapshots")

# BGR (OpenCV) colors for the annotation overlay.
_BOX_COLOR = (0, 200, 0)      # green boxes
_LABEL_BG = (0, 200, 0)       # green caption background
_LABEL_TEXT = (255, 255, 255)  # white caption text
_FONT_SCALE = 0.5
_FONT_THICKNESS = 1
_BOX_THICKNESS = 2


def _cv2_module(cv2_module=None):
    """Resolve the OpenCV module, importing it lazily when not injected."""
    if cv2_module is not None:
        return cv2_module
    import cv2  # lazy: only a real write needs OpenCV

    return cv2


def detection_label(det) -> str:
    """The overlay caption for one detection: ``"<class> <conf>%"``.

    Uses the detection's actual class name (never a hardcoded label), so the
    annotated frame shows what the model detected — ``egg 91%`` in incubation
    mode, ``chick 80%`` in brooder mode.
    """
    return f"{det.class_name} {round(det.confidence * 100)}%"


def draw_detections(frame, detections, *, cv2_module=None):
    """Return a copy of ``frame`` with each detection's box + label drawn.

    Green rectangle around each ``bbox``, with a class-name + confidence caption
    (e.g. ``egg 91%``) on a filled background just above the box. The input frame
    is never mutated.
    """
    cv2 = _cv2_module(cv2_module)
    annotated = frame.copy()
    height = annotated.shape[0]
    for det in detections:
        if len(det.bbox) != 4:
            continue
        x1, y1, x2, y2 = (int(round(v)) for v in det.bbox)
        cv2.rectangle(annotated, (x1, y1), (x2, y2), _BOX_COLOR, _BOX_THICKNESS)

        label = detection_label(det)
        (tw, th), baseline = cv2.getTextSize(
            label, cv2.FONT_HERSHEY_SIMPLEX, _FONT_SCALE, _FONT_THICKNESS
        )
        # Caption sits just above the box; tuck it below the top edge if there's
        # no room above (box hugging the top of the frame).
        top = y1 - th - baseline - 2
        if top < 0:
            top = min(y1 + 2, max(0, height - th - baseline - 2))
        cv2.rectangle(
            annotated,
            (x1, top),
            (x1 + tw + 2, top + th + baseline + 2),
            _LABEL_BG,
            thickness=-1,  # filled
        )
        cv2.putText(
            annotated,
            label,
            (x1 + 1, top + th + 1),
            cv2.FONT_HERSHEY_SIMPLEX,
            _FONT_SCALE,
            _LABEL_TEXT,
            _FONT_THICKNESS,
            cv2.LINE_AA,
        )
    return annotated


def write_atomic(frame, path, *, cv2_module=None) -> Path:
    """Encode ``frame`` to ``path`` atomically (write ``.tmp`` then ``os.replace``).

    The frame is JPEG-encoded in memory (``cv2.imencode``) and the bytes written
    to a sibling ``.tmp`` file in the destination directory, then ``os.replace``d
    into place — a same-filesystem atomic rename, so a concurrent reader sees
    either the old file or the fully-written new one, never a partial image.
    (Encoding in memory also sidesteps ``cv2.imwrite`` picking its codec from the
    ``.tmp`` extension.) Returns the final path.
    """
    cv2 = _cv2_module(cv2_module)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    ok, buf = cv2.imencode(".jpg", frame)
    if not ok:
        raise OSError(f"failed to JPEG-encode snapshot for {path}")
    tmp = path.with_name(f".{path.name}.tmp")
    try:
        tmp.write_bytes(buf.tobytes())
        os.replace(str(tmp), str(path))
    except OSError:
        # Leave no half-written temp behind on a write/replace failure.
        try:
            tmp.unlink()
        except OSError:
            pass
        raise
    return path


def write_snapshots(frame, detections, latest_path, latest_annotated_path, *, cv2_module=None) -> None:
    """Write the rolling raw + annotated snapshots for one cycle.

    ``latest_path`` gets the raw frame; ``latest_annotated_path`` gets a copy
    with YOLO boxes drawn when there are detections, or a plain copy of the raw
    frame when there are none (so the annotated file always exists and stays
    fresh). Both writes are atomic.
    """
    cv2 = _cv2_module(cv2_module)
    write_atomic(frame, latest_path, cv2_module=cv2)
    annotated = draw_detections(frame, detections, cv2_module=cv2) if detections else frame
    write_atomic(annotated, latest_annotated_path, cv2_module=cv2)
    logger.debug(
        "Wrote rolling snapshots: %s + %s (%d box(es))",
        latest_path,
        latest_annotated_path,
        len(detections),
    )
