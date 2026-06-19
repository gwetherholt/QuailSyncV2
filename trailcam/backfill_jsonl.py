"""One-time backfill: replay observations.jsonl into the SQLite-backed
``POST /api/trailcam/observation`` endpoint.

Reads the existing ``observations.jsonl`` (both the legacy
``{source, image_path}`` shape and the newer ``{image_filename}`` write-ahead-log
shape are handled) and POSTs each entry to the local QuailSync server, populating
the ``trail_cam_observations`` table with all historical observations — including
the NWQuail camera's data.

Run once, then delete this script:

    python backfill_jsonl.py --url http://localhost:3000
    python backfill_jsonl.py --file /path/to/observations.jsonl --dry-run
"""

from __future__ import annotations

import argparse
import json
import logging
import sys
from pathlib import Path

import requests

# Support both `python backfill_jsonl.py` (script) and package imports, and stay
# usable even if config can't be imported (e.g. run from another directory).
try:
    from . import config
except ImportError:
    try:
        import config
    except ImportError:
        config = None  # type: ignore[assignment]

logger = logging.getLogger("trailcam.backfill")


def _default_file() -> Path:
    """Default WAL location: ``{PROCESSED_DIR}/observations.jsonl`` when config
    is importable, else the current directory."""
    if config is not None:
        return config.PROCESSED_DIR / "observations.jsonl"
    return Path("observations.jsonl")


def to_payload(entry: dict) -> dict | None:
    """Map one JSONL record onto the ``/api/trailcam/observation`` body.

    Handles both the legacy shape (``image_path``, ``source``) and the newer WAL
    shape (``image_filename``/``annotated_image_filename``). Returns ``None`` for
    a record with no ``camera_id`` (nothing to key on)."""
    camera_id = entry.get("camera_id")
    if not camera_id:
        return None

    # Prefer an explicit basename; otherwise derive it from the legacy path.
    image_filename = entry.get("image_filename")
    if not image_filename:
        image_path = entry.get("image_path")
        if image_path:
            image_filename = Path(str(image_path)).name

    # Annotated copy: the detector writes "{stem}_annotated.jpg" next to it.
    annotated = entry.get("annotated_image_filename")
    if not annotated and image_filename:
        annotated = f"{Path(image_filename).stem}_annotated.jpg"

    return {
        "camera_id": camera_id,
        "timestamp": entry.get("timestamp"),
        "bird_count": entry.get("bird_count", 0),
        "average_confidence": entry.get("average_confidence"),
        "min_confidence": entry.get("min_confidence"),
        "detections": entry.get("detections", []),
        "inference_time_ms": entry.get("inference_time_ms", 0.0),
        "image_filename": image_filename,
        "annotated_image_filename": annotated,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Backfill observations.jsonl into QuailSync's SQLite table."
    )
    parser.add_argument("--url", default="http://localhost:3000", help="QuailSync base URL")
    parser.add_argument(
        "--file",
        default=str(_default_file()),
        help="Path to observations.jsonl (default: processed/observations.jsonl)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Parse and report, but don't POST anything",
    )
    parser.add_argument("--timeout", type=float, default=30.0, help="Per-request timeout (s)")
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        stream=sys.stdout,
    )

    path = Path(args.file)
    if not path.exists():
        logger.error("Observations file not found: %s", path)
        return 1

    endpoint = args.url.rstrip("/") + "/api/trailcam/observation"
    session = requests.Session()
    logger.info(
        "Backfilling %s -> %s%s", path, endpoint, " (dry run)" if args.dry_run else ""
    )

    total = posted = skipped = failed = 0
    per_camera: dict[str, int] = {}

    with open(path, "r", encoding="utf-8") as fh:
        for lineno, raw in enumerate(fh, start=1):
            raw = raw.strip()
            if not raw:
                continue
            total += 1

            try:
                entry = json.loads(raw)
            except json.JSONDecodeError as exc:
                logger.warning("Line %d: invalid JSON, skipping (%s)", lineno, exc)
                skipped += 1
                continue

            payload = to_payload(entry)
            if payload is None:
                logger.warning("Line %d: no camera_id, skipping", lineno)
                skipped += 1
                continue

            per_camera[payload["camera_id"]] = per_camera.get(payload["camera_id"], 0) + 1

            if args.dry_run:
                posted += 1
                continue

            try:
                resp = session.post(endpoint, json=payload, timeout=args.timeout)
                resp.raise_for_status()
                posted += 1
            except Exception as exc:  # noqa: BLE001 — keep going past a bad line
                logger.error("Line %d: POST failed (%s)", lineno, exc)
                failed += 1

    verb = "would post" if args.dry_run else "posted"
    logger.info(
        "Backfill %s: %d read, %d %s, %d skipped, %d failed",
        "(dry run)" if args.dry_run else "complete",
        total,
        posted,
        verb,
        skipped,
        failed,
    )
    for cam, n in sorted(per_camera.items()):
        logger.info("  %s: %d observation(s)", cam, n)

    return 0 if failed == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
