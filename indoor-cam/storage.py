"""Image storage strategy for the indoor-cam pipeline.

At ~1fps the stream would produce ~86k frames/day, so frames are kept only when
*notable*. This module holds the pure notability decision plus the disk helpers
(persist / delete / daily prune). The JSON observation is always POSTed; an image
is the exception, not the rule.
"""

from __future__ import annotations

import logging
import shutil
import time
from pathlib import Path

logger = logging.getLogger("indoorcam.storage")


def notable_reasons(
    *,
    post_reason: str,
    min_confidence: float | None,
    is_first: bool,
    seconds_since_last_image: float | None,
    low_confidence_threshold: float,
    heartbeat_interval: float,
) -> list[str]:
    """Return the reasons (if any) this frame is worth saving to disk.

    A non-empty list means "save it". Triggers:
      * ``startup``       — the first frame after the service starts,
      * ``count_change``  — the smoothed count moved past the ± threshold,
      * ``low_confidence``— the frame's lowest detection confidence is below
        ``low_confidence_threshold`` (the model is uncertain — good training
        data),
      * ``heartbeat``     — no image saved within ``heartbeat_interval`` seconds.
    """
    reasons: list[str] = []
    if is_first:
        reasons.append("startup")
    if post_reason == "count_change":
        reasons.append("count_change")
    if min_confidence is not None and min_confidence < low_confidence_threshold:
        reasons.append("low_confidence")
    # Heartbeat: due when we've gone a full interval without saving, or have
    # never saved one (unless this is already the startup frame, which saves).
    if seconds_since_last_image is None:
        if not is_first:
            reasons.append("heartbeat")
    elif seconds_since_last_image >= heartbeat_interval:
        reasons.append("heartbeat")
    return reasons


def persist_frame(live_path: Path | str, dest_dir: Path | str, stem: str) -> Path:
    """Copy the just-sampled frame into ``dest_dir`` as ``{stem}.jpg`` (the
    servable, prunable location). Returns the new path."""
    dest_dir = Path(dest_dir)
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest = dest_dir / f"{stem}.jpg"
    shutil.copyfile(str(live_path), str(dest))
    return dest


def delete_files(*paths) -> None:
    """Best-effort unlink of saved frames (raw + annotated) after a successful
    Roboflow upload. ``None`` and missing files are ignored."""
    for p in paths:
        if p is None:
            continue
        try:
            Path(p).unlink(missing_ok=True)
        except OSError as exc:  # noqa: BLE001 — reclaiming disk must never crash
            logger.warning("Could not delete %s: %s", p, exc)


def prune_old_images(root: Path | str, retention_days: float, *, now: float | None = None) -> int:
    """Delete saved JPEGs under ``root`` older than ``retention_days``.

    The daily safety net for frames the Roboflow step didn't reclaim (upload
    disabled, or it failed and the file was kept for retry). Returns the number
    of files removed. A missing ``root`` is a no-op.
    """
    root = Path(root)
    if not root.exists():
        return 0
    cutoff = (now if now is not None else time.time()) - retention_days * 86400
    removed = 0
    for jpg in root.rglob("*.jpg"):
        try:
            if jpg.stat().st_mtime < cutoff:
                jpg.unlink(missing_ok=True)
                removed += 1
        except OSError:
            continue
    if removed:
        logger.info(
            "Pruned %d image(s) older than %s day(s) from %s", removed, retention_days, root
        )
    return removed
