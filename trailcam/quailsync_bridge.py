"""Bridge from trail-cam detections to QuailSync.

Turns ``DetectionResult`` objects (from ``yolo_detector``) into observation
payloads and "posts" them. The real QuailSync endpoint
(``POST /api/trailcam/observation``) doesn't exist yet, so for now each
observation is appended to a local ``observations.jsonl`` file in the processed
directory. When the endpoint ships, swap the fallback for the HTTP call (see
the TODO in :meth:`QuailSyncBridge.post`) — the payload is already in final
shape.

Run standalone to process staging and emit observations end-to-end:

    python quailsync_bridge.py
"""

from __future__ import annotations

import json
import logging
from dataclasses import asdict
from pathlib import Path

# Support both `python quailsync_bridge.py` (script) and package imports.
try:
    from . import config
    from .yolo_detector import DetectionResult
except ImportError:
    import config
    from yolo_detector import DetectionResult

logger = logging.getLogger("trailcam.quailsync_bridge")


class QuailSyncBridge:
    """Builds observation payloads and delivers them to QuailSync.

    Today "deliver" means appending to a JSONL file; see :meth:`post`.
    """

    def __init__(
        self,
        api_url: str = config.QUAILSYNC_API_URL,
        output_path: Path | str | None = None,
        session=None,
    ):
        self.api_url = api_url.rstrip("/")
        # One observation per line; lives alongside the processed photos.
        self.output_path = Path(output_path) if output_path else config.PROCESSED_DIR / "observations.jsonl"
        self.session = session  # reserved for the real HTTP path (see post())

    # -- payload ------------------------------------------------------------

    @staticmethod
    def build_payload(result: DetectionResult) -> dict:
        """Shape a ``DetectionResult`` into the QuailSync observation payload."""
        confidences = [d.confidence for d in result.detections]
        average_confidence = (
            round(sum(confidences) / len(confidences), 4) if confidences else None
        )
        min_confidence = round(min(confidences), 4) if confidences else None

        return {
            "source": "trailcam",
            "camera_id": result.camera_id,
            "timestamp": result.timestamp,
            # NOTE: bird_count == total detections. Fine while the model only
            # detects birds; if it gains other classes, filter by class_name
            # here before counting.
            "bird_count": result.total_count,
            "average_confidence": average_confidence,
            "min_confidence": min_confidence,
            "detections": [asdict(d) for d in result.detections],
            "inference_time_ms": result.inference_time_ms,
            "image_path": result.image_path,
        }

    # -- delivery -----------------------------------------------------------

    def post(self, result: DetectionResult) -> bool:
        """Deliver a single observation. Returns True on success.

        TODO: once the server implements ``POST /api/trailcam/observation``,
        replace the local-file fallback with:

            resp = (self.session or requests).post(
                f"{self.api_url}/api/trailcam/observation",
                json=payload, timeout=30,
            )
            resp.raise_for_status()

        Until then we append the payload to a local JSONL file so nothing is
        lost and the data can be backfilled when the endpoint exists.
        """
        payload = self.build_payload(result)
        try:
            self._append_jsonl(payload)
            logger.info(
                "Observation recorded: camera=%s birds=%d -> %s",
                payload["camera_id"],
                payload["bird_count"],
                self.output_path.name,
            )
            return True
        except Exception as exc:  # noqa: BLE001 — one bad write shouldn't abort the batch
            logger.error("Failed to record observation for %s: %s", result.image_path, exc)
            return False

    def post_batch(self, results: list[DetectionResult]) -> tuple[int, int]:
        """Deliver many observations. Returns ``(success_count, failure_count)``."""
        success = 0
        failure = 0
        for result in results:
            if self.post(result):
                success += 1
            else:
                failure += 1
        logger.info("post_batch: %d succeeded, %d failed", success, failure)
        return success, failure

    def _append_jsonl(self, payload: dict) -> None:
        """Append one JSON object as a line to the observations file."""
        self.output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.output_path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(payload) + "\n")


def main() -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    # Imported here (not at module top) so the bridge stays usable without
    # ultralytics; process_staging pulls it in lazily on first detection.
    try:
        from .yolo_detector import process_staging
    except ImportError:
        from yolo_detector import process_staging

    config.ensure_dirs()
    results = process_staging()
    bridge = QuailSyncBridge()
    success, failure = bridge.post_batch(results)
    logger.info("Done — %d observation(s) recorded, %d failed", success, failure)
    return 0 if failure == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
