"""SQLite event logging + crop saving for the incubator pipeline.

The Rust QuailSync backend **owns the ``incubation_events`` schema** — it creates
the table (and its indexes) via its migration layer. This sidecar is a
write-only, polite co-tenant that assumes the table already exists: the backend
must have booted (run its migrations) before the sidecar starts writing. That
keeps a single authoritative schema definition and avoids two live ``CREATE
TABLE`` sources drifting apart.

Because the DB file is shared with the backend under WAL, this module:

* Opens in **WAL** mode so readers (the backend) and this writer don't block each
  other.
* Sets ``busy_timeout`` so a momentary lock contends-then-retries instead of
  failing with ``SQLITE_BUSY``.
* Keeps every write a single short auto-committed statement — never holds a long
  write transaction open across a capture cycle.

On an event we also save the ROI crop to
``captures_dir/YYYY-MM-DD/slot_<id>_<UTC-timestamp>.jpg`` and record that path in
``frame_path``. Those crops are the labeling dataset for the stage-2 classifier,
so this write path matters more than its size suggests — get the filename/dir
scheme wrong and the dataset is a pain to organize later.
"""

from __future__ import annotations

import logging
import re
import sqlite3
from datetime import datetime, timezone
from pathlib import Path

logger = logging.getLogger("incubator.storage")

# Slot ids appear in filenames; keep them filesystem-safe.
_SLOT_ID_SAFE = re.compile(r"[^A-Za-z0-9_-]")


def connect(db_path: Path | str, busy_timeout_ms: int) -> sqlite3.Connection:
    """Open ``db_path`` in WAL mode with ``busy_timeout`` applied.

    The parent directory is created if missing. The schema is **not** created
    here — the Rust backend's migration layer owns the ``incubation_events``
    table, so the backend must have booted before this sidecar writes.
    """
    db_path = Path(db_path)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(db_path))
    # WAL lets the backend read while we write; busy_timeout turns transient
    # lock contention into a short wait instead of an immediate SQLITE_BUSY.
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute(f"PRAGMA busy_timeout={int(busy_timeout_ms)};")
    return conn


def _utc_timestamp_for_filename(when: datetime | None) -> str:
    """Compact UTC timestamp for filenames: ``YYYYMMDDTHHMMSSmmmZ``."""
    if when is None:
        when = datetime.now(timezone.utc)
    elif when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    when = when.astimezone(timezone.utc)
    return when.strftime("%Y%m%dT%H%M%S") + f"{when.microsecond // 1000:03d}Z"


def crop_path(captures_dir: Path | str, slot_id: str, when: datetime | None = None) -> Path:
    """Compute the destination path for a slot crop.

    ``captures_dir/YYYY-MM-DD/slot_<id>_<UTC-timestamp>.jpg`` — the crops are
    day-bucketed so a hatch's worth of images stays browsable.
    """
    if when is None:
        when = datetime.now(timezone.utc)
    elif when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    when = when.astimezone(timezone.utc)
    safe_id = _SLOT_ID_SAFE.sub("_", slot_id)
    day = when.strftime("%Y-%m-%d")
    stamp = _utc_timestamp_for_filename(when)
    return Path(captures_dir) / day / f"slot_{safe_id}_{stamp}.jpg"


def save_crop(
    crop,
    captures_dir: Path | str,
    slot_id: str,
    when: datetime | None = None,
    *,
    cv2_module=None,
) -> Path:
    """Write ``crop`` (a numpy image) to its :func:`crop_path` and return it.

    Creates the day directory as needed. ``cv2_module`` is injectable for tests.
    """
    dest = crop_path(captures_dir, slot_id, when)
    dest.parent.mkdir(parents=True, exist_ok=True)
    cv2 = cv2_module
    if cv2 is None:
        import cv2  # lazy: only the real save path needs OpenCV
    if not cv2.imwrite(str(dest), crop):
        raise OSError(f"failed to write crop to {dest}")
    return dest


class EventStore:
    """Thin wrapper around the ``incubation_events`` table.

    Construct once at startup; call :meth:`record_event` per detected event.
    Each insert is its own short auto-committed transaction.
    """

    def __init__(self, db_path: Path | str, busy_timeout_ms: int = 5000):
        self.db_path = Path(db_path)
        self.busy_timeout_ms = int(busy_timeout_ms)
        self._conn = connect(self.db_path, self.busy_timeout_ms)

    @property
    def conn(self) -> sqlite3.Connection:
        return self._conn

    def record_event(
        self,
        *,
        slot_id: str,
        diff_score: float,
        high_threshold: float,
        clutch_id: int | None = None,
        frame_path: str | Path | None = None,
        event_type: str = "change_detected",
        created_at: str | None = None,
    ) -> int:
        """Insert one event row and return its ``id``.

        ``created_at`` defaults to SQLite's ``strftime`` UTC clock (the column
        default) when omitted, keeping timestamps consistent with rows the
        backend might write.
        """
        frame_path_str = str(frame_path) if frame_path is not None else None
        if created_at is None:
            cur = self._conn.execute(
                """
                INSERT INTO incubation_events
                    (slot_id, event_type, diff_score, high_threshold, clutch_id, frame_path)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                (slot_id, event_type, float(diff_score), float(high_threshold), clutch_id, frame_path_str),
            )
        else:
            cur = self._conn.execute(
                """
                INSERT INTO incubation_events
                    (slot_id, event_type, diff_score, high_threshold, clutch_id, frame_path, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (slot_id, event_type, float(diff_score), float(high_threshold), clutch_id, frame_path_str, created_at),
            )
        self._conn.commit()
        return int(cur.lastrowid)

    def close(self) -> None:
        try:
            self._conn.close()
        except sqlite3.Error as exc:  # pragma: no cover - defensive
            logger.warning("Error closing incubator DB: %s", exc)

    def __enter__(self) -> "EventStore":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
