"""Bridge from trail-cam detections to QuailSync.

Turns ``DetectionResult`` objects (from ``yolo_detector``) into observation
payloads and POSTs them to QuailSync's ``POST /api/trailcam/observation``
endpoint (which stores them in SQLite). If the API is unreachable or rejects
the request, each observation is appended to a local ``observations.jsonl``
write-ahead log in the processed directory so nothing is lost and it can be
replayed later.

Run standalone to process staging and emit observations end-to-end:

    python quailsync_bridge.py
"""

from __future__ import annotations

import json
import logging
import re
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

    "Deliver" POSTs to ``/api/trailcam/observation``; on failure it falls back
    to a JSONL write-ahead log (``output_path``). See :meth:`post`.
    """

    def __init__(
        self,
        api_url: str = config.QUAILSYNC_API_URL,
        output_path: Path | str | None = None,
        session=None,
    ):
        self.api_url = api_url.rstrip("/")
        # Write-ahead log: one observation per line, alongside the processed
        # photos. Only written when the POST fails, so it can be replayed later.
        self.output_path = Path(output_path) if output_path else config.PROCESSED_DIR / "observations.jsonl"
        # Optional injected HTTP client (a `requests`-like object with `.post`);
        # defaults to the `requests` module, imported lazily in `_post_http`.
        self.session = session

    # -- payload ------------------------------------------------------------

    def build_payload(self, result: DetectionResult) -> dict:
        """Shape a ``DetectionResult`` into the QuailSync observation payload.

        Free-text fields that originate from the camera/model (``camera_id``,
        ``class_name``) are sanitized before they leave this process so we never
        forward SQL metacharacters, HTML/script tags, or null bytes to the
        server (defense in depth — the server should validate too)."""
        confidences = [d.confidence for d in result.detections]
        average_confidence = (
            round(sum(confidences) / len(confidences), 4) if confidences else None
        )
        min_confidence = round(min(confidences), 4) if confidences else None

        image = Path(result.image_path)
        return {
            "camera_id": self._sanitize_string(result.camera_id),
            "timestamp": result.timestamp,
            # NOTE: bird_count == total detections. Fine while the model only
            # detects birds; if it gains other classes, filter by class_name
            # here before counting.
            "bird_count": result.total_count,
            "average_confidence": average_confidence,
            "min_confidence": min_confidence,
            "detections": [
                {
                    "class_name": self._sanitize_string(d.class_name),
                    "confidence": d.confidence,
                    "bbox": d.bbox,
                }
                for d in result.detections
            ],
            "inference_time_ms": result.inference_time_ms,
            # Ambient temperature (°F) from the camera, if the poller captured it.
            "ambient_temperature_f": result.ambient_temperature_f,
            # Basenames only — the server serves images from processed/{cam}/.
            "image_filename": image.name,
            # The detector writes a sibling "{stem}_annotated.jpg"; the server
            # only advertises it when that file is actually present on disk.
            "annotated_image_filename": f"{image.stem}_annotated.jpg",
        }

    @staticmethod
    def _sanitize_string(value, *, max_length: int = 200):
        """Strip dangerous content from a free-text field bound for the server.

        Removes null bytes, HTML/script tags, and SQL/HTML metacharacters
        (``; ' " ` < >``), collapses whitespace, and caps length. ``None`` is
        passed through unchanged.
        """
        if value is None:
            return None
        text = str(value).replace("\x00", "")
        text = re.sub(r"<[^>]*>", "", text)  # drop HTML/script tags entirely
        text = re.sub(r"[;'\"`<>]", "", text)  # drop SQL/HTML metacharacters
        text = re.sub(r"\s+", " ", text).strip()
        return text[:max_length]

    # -- delivery -----------------------------------------------------------

    def post(self, result: DetectionResult) -> bool:
        """Deliver one observation to QuailSync.

        POSTs to ``/api/trailcam/observation``; if the API is unreachable or
        rejects the request, the observation is appended to the local JSONL
        write-ahead log so nothing is lost and it can be replayed later.

        Returns True when the observation was delivered OR durably written to
        the WAL; False only if both failed (data lost).
        """
        payload = self.build_payload(result)
        if self._post_http(payload):
            logger.info(
                "Observation posted: camera=%s birds=%d",
                payload["camera_id"],
                payload["bird_count"],
            )
            return True
        # Server unreachable / rejected — preserve the observation in the WAL.
        try:
            self._append_jsonl(payload)
            logger.warning(
                "QuailSync unreachable — wrote observation to write-ahead log %s (camera=%s)",
                self.output_path.name,
                payload["camera_id"],
            )
            return True
        except Exception as exc:  # noqa: BLE001 — nothing more we can do; data lost
            logger.error("Failed to write WAL for %s: %s", result.image_path, exc)
            return False

    def _post_http(self, payload: dict) -> bool:
        """POST one observation. Returns True on a 2xx response, False on any
        transport/HTTP error (the caller then falls back to the WAL)."""
        url = f"{self.api_url}/api/trailcam/observation"
        try:
            session = self.session
            if session is None:
                import requests  # lazy: keeps the module importable without requests

                session = requests
            resp = session.post(url, json=payload, timeout=30)
            resp.raise_for_status()
            return True
        except Exception as exc:  # noqa: BLE001 — network/HTTP errors fall back to WAL
            logger.error("POST %s failed: %s", url, exc)
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
