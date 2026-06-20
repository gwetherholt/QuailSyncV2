"""One-time backfill: replay processed ``*_detections.json`` files into the
SQLite-backed ``POST /api/trailcam/observation`` endpoint.

The detector (``yolo_detector.process_staging``) writes a ``{stem}_detections.json``
next to every processed image under ``processed/{camera_id}/``. This script walks
each camera directory, reads those detection files, rebuilds the observation
payload, and POSTs each one to the local QuailSync server — repopulating the
``trail_cam_observations`` table from what's already on disk.

Observations the server rejects (duplicates, validation errors, etc.) are simply
skipped and counted, so it's safe to re-run.

Run once, then delete this script:

    python backfill_to_db.py --url http://localhost:3000
    python backfill_to_db.py --dir ~/trailcam/processed --dry-run
"""

from __future__ import annotations

import argparse
import json
import logging
import sys
from pathlib import Path

import requests

# Support both `python backfill_to_db.py` (script) and package imports, and stay
# usable even if config can't be imported (e.g. run from another directory).
try:
    from . import config
except ImportError:
    try:
        import config
    except ImportError:
        config = None  # type: ignore[assignment]

logger = logging.getLogger("trailcam.backfill_db")


def _default_dir() -> Path:
    """Default scan root: ``{PROCESSED_DIR}`` when config is importable, else
    ``~/trailcam/processed``."""
    if config is not None:
        return config.PROCESSED_DIR
    return Path("~/trailcam/processed").expanduser()


def to_payload(data: dict, camera_id: str, detections_path: Path) -> dict | None:
    """Map one detection JSON record onto the ``/api/trailcam/observation`` body.

    ``camera_id`` comes from the parent directory name (authoritative on disk).
    ``bird_count`` is the detector's ``total_count``. The image basename is taken
    from the record's ``image_path``; if absent, it's derived from the detections
    filename (``{stem}_detections.json`` -> ``{stem}.jpg``). Returns ``None`` if
    there's nothing usable to key on."""
    if not camera_id:
        return None

    detections = data.get("detections") or []
    confidences = [
        d.get("confidence")
        for d in detections
        if isinstance(d, dict) and d.get("confidence") is not None
    ]
    average_confidence = sum(confidences) / len(confidences) if confidences else None
    min_confidence = min(confidences) if confidences else None

    # Image basename: prefer the record's image_path, else derive from the
    # detections filename ("{stem}_detections.json" -> "{stem}.jpg").
    image_filename = None
    image_path = data.get("image_path")
    if image_path:
        image_filename = Path(str(image_path)).name
    if not image_filename:
        stem = detections_path.name[: -len("_detections.json")]
        image_filename = f"{stem}.jpg" if stem else None

    # The detector writes a sibling "{stem}_annotated.jpg" next to the image.
    annotated = None
    if image_filename:
        annotated = f"{Path(image_filename).stem}_annotated.jpg"

    return {
        "camera_id": camera_id,
        "timestamp": data.get("timestamp"),
        "bird_count": data.get("total_count", 0),
        "average_confidence": average_confidence,
        "min_confidence": min_confidence,
        "detections": detections,
        "inference_time_ms": data.get("inference_time_ms", 0.0),
        "image_filename": image_filename,
        "annotated_image_filename": annotated,
        "ambient_temperature_f": data.get("ambient_temperature_f"),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Backfill processed *_detections.json files into QuailSync's SQLite table."
    )
    parser.add_argument("--url", default="http://localhost:3000", help="QuailSync base URL")
    parser.add_argument(
        "--dir",
        default=str(_default_dir()),
        help="Processed directory to scan (default: ~/trailcam/processed)",
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

    root = Path(args.dir).expanduser()
    if not root.is_dir():
        logger.error("Processed directory not found: %s", root)
        return 1

    endpoint = args.url.rstrip("/") + "/api/trailcam/observation"
    session = requests.Session()
    logger.info(
        "Backfilling %s -> %s%s", root, endpoint, " (dry run)" if args.dry_run else ""
    )

    # One sub-directory per camera; its name is the camera_id.
    camera_dirs = sorted(p for p in root.iterdir() if p.is_dir())
    if not camera_dirs:
        logger.warning("No camera sub-directories under %s", root)

    total = posted = skipped = failed = 0
    # Per-camera tallies: {camera_id: {"posted": n, "skipped": n, "failed": n}}.
    per_camera: dict[str, dict[str, int]] = {}

    for camera_dir in camera_dirs:
        camera_id = camera_dir.name
        stats = per_camera.setdefault(
            camera_id, {"posted": 0, "skipped": 0, "failed": 0}
        )
        # Sorted so observations land oldest-first (filenames are timestamped).
        for detections_path in sorted(camera_dir.glob("*_detections.json")):
            total += 1
            try:
                data = json.loads(detections_path.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, OSError) as exc:
                logger.warning("%s: unreadable, skipping (%s)", detections_path, exc)
                skipped += 1
                stats["skipped"] += 1
                continue

            payload = to_payload(data, camera_id, detections_path)
            if payload is None:
                logger.warning("%s: no usable data, skipping", detections_path)
                skipped += 1
                stats["skipped"] += 1
                continue

            if args.dry_run:
                posted += 1
                stats["posted"] += 1
                continue

            try:
                resp = session.post(endpoint, json=payload, timeout=args.timeout)
                if resp.ok:
                    posted += 1
                    stats["posted"] += 1
                else:
                    # Duplicates / validation errors etc. — skip, don't fail.
                    logger.info(
                        "%s: server returned %s, skipping", detections_path, resp.status_code
                    )
                    skipped += 1
                    stats["skipped"] += 1
            except requests.RequestException as exc:
                logger.error("%s: POST failed (%s)", detections_path, exc)
                failed += 1
                stats["failed"] += 1

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
    for cam in sorted(per_camera):
        s = per_camera[cam]
        logger.info(
            "  %s: %d %s, %d skipped, %d failed",
            cam,
            s["posted"],
            verb,
            s["skipped"],
            s["failed"],
        )

    return 0 if failed == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
