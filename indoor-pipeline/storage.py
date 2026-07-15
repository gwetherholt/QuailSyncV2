"""SQLite ``incubation_events`` logging for the indoor pipeline (incubator mode).

Adapted from ``incubator/storage.py``. The frame-diff detector is gone —
detections now come from YOLO — so each *YOLO detection* (in incubation mode
only) becomes one ``incubation_events`` row: ``event_type`` is the detected class
name (``egg``, ``pipped``, …) and ``diff_score`` is repurposed to carry the
detection confidence. Chick mode writes nothing here.

As before, the Rust QuailSync backend **owns the ``incubation_events`` schema**
(created via its migration layer); this sidecar is a write-only, polite
co-tenant that assumes the table already exists. Because the DB file is shared
with the backend under WAL, this module opens in WAL mode, sets ``busy_timeout``
so momentary lock contention retries instead of failing, and keeps every write a
single short auto-committed statement.
"""

from __future__ import annotations

import logging
import sqlite3
from pathlib import Path

logger = logging.getLogger("indoorpipeline.storage")


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


class EventStore:
    """Thin wrapper around the ``incubation_events`` table.

    Construct once when incubation mode is first entered; call
    :meth:`record_detection` per YOLO detection. Each insert is its own short
    auto-committed transaction.
    """

    def __init__(self, db_path: Path | str, busy_timeout_ms: int = 5000):
        self.db_path = Path(db_path)
        self.busy_timeout_ms = int(busy_timeout_ms)
        self._conn = connect(self.db_path, self.busy_timeout_ms)

    @property
    def conn(self) -> sqlite3.Connection:
        return self._conn

    def record_detection(
        self,
        *,
        event_type: str,
        confidence: float,
        slot_id: str,
        confidence_threshold: float,
        clutch_id: int | None = None,
        frame_path: str | Path | None = None,
        created_at: str | None = None,
    ) -> int:
        """Insert one YOLO detection as an ``incubation_events`` row; return its id.

        Column mapping (the schema predates YOLO, so a couple of columns are
        repurposed):

        * ``event_type``     ← the detected class name (``egg``, ``pipped``, …),
        * ``diff_score``     ← the detection confidence,
        * ``high_threshold`` ← the model's confidence threshold (the bar this
          detection cleared),
        * ``slot_id``        ← the camera id (there are no per-slot ROIs now).

        ``created_at`` defaults to SQLite's ``strftime`` UTC clock (the column
        default) when omitted, keeping timestamps consistent with backend rows.
        """
        frame_path_str = str(frame_path) if frame_path is not None else None
        if created_at is None:
            cur = self._conn.execute(
                """
                INSERT INTO incubation_events
                    (slot_id, event_type, diff_score, high_threshold, clutch_id, frame_path)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                (slot_id, event_type, float(confidence), float(confidence_threshold), clutch_id, frame_path_str),
            )
        else:
            cur = self._conn.execute(
                """
                INSERT INTO incubation_events
                    (slot_id, event_type, diff_score, high_threshold, clutch_id, frame_path, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (slot_id, event_type, float(confidence), float(confidence_threshold), clutch_id, frame_path_str, created_at),
            )
        self._conn.commit()
        return int(cur.lastrowid)

    def close(self) -> None:
        try:
            self._conn.close()
        except sqlite3.Error as exc:  # pragma: no cover - defensive
            logger.warning("Error closing indoor pipeline DB: %s", exc)

    def __enter__(self) -> "EventStore":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
