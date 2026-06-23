"""Bridge from indoor-camera detections to QuailSync.

Turns a ``DetectionResult`` (from the reused trail-cam ``yolo_detector``) into an
observation payload and POSTs it to QuailSync's
``POST /api/indoorcam/observation`` endpoint (which stores it in SQLite). If the
API is unreachable or rejects the request, the observation is appended to a
local ``observations.jsonl`` write-ahead log in the processed directory so
nothing is lost and it can be replayed later.

Mirrors ``trailcam/quailsync_bridge.py``, but posts to the indoor endpoint and
uses ``detection_count`` (indoor models count chicks) instead of ``bird_count``.
"""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path

try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("indoorcam.bridge")


class IndoorBridge:
    """Builds observation payloads and delivers them to QuailSync.

    "Deliver" POSTs to ``/api/indoorcam/observation``; on failure it falls back
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
        # frames. Only written when the POST fails, so it can be replayed later.
        self.output_path = (
            Path(output_path) if output_path else config.PROCESSED_DIR / "observations.jsonl"
        )
        # Optional injected HTTP client (a `requests`-like object with `.post`);
        # defaults to the `requests` module, imported lazily in `_post_http`.
        self.session = session

    # -- payload ------------------------------------------------------------

    def build_payload(
        self,
        result,
        timestamp: str | None = None,
        detection_count: int | None = None,
        include_image: bool = True,
    ) -> dict:
        """Shape a ``DetectionResult`` into the QuailSync observation payload.

        ``timestamp`` overrides the result's own timestamp (the poller passes the
        frame's capture time). ``detection_count`` overrides the headcount with
        the smoothed/median value (the ``detections`` array still reflects the
        current frame's raw boxes). ``include_image=False`` nulls the image
        filenames — used for the majority of (routine) posts where no frame was
        saved to disk, so the JSON observation is recorded without an image.

        Free-text fields that originate from the camera/model (``camera_id``,
        ``class_name``) are sanitized before they leave this process (defense in
        depth — the server validates too).
        """
        confidences = [d.confidence for d in result.detections]
        average_confidence = (
            round(sum(confidences) / len(confidences), 4) if confidences else None
        )
        min_confidence = round(min(confidences), 4) if confidences else None

        payload = {
            "camera_id": self._sanitize_string(result.camera_id),
            "timestamp": timestamp if timestamp is not None else result.timestamp,
            # The smoothed median count when provided; else the raw frame total.
            "detection_count": (
                detection_count if detection_count is not None else result.total_count
            ),
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
            # Image fields are nullable: only set when a frame was actually saved
            # to disk (a "notable" frame). Most observations carry no image.
            "image_filename": None,
            "annotated_image_filename": None,
        }
        if include_image:
            image = Path(result.image_path)
            # Basenames only — the server serves images from processed/{cam}/.
            payload["image_filename"] = image.name
            # The detector writes a sibling "{stem}_annotated.jpg"; the server
            # only advertises it when that file is actually present on disk.
            payload["annotated_image_filename"] = f"{image.stem}_annotated.jpg"
        return payload

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

    def post(
        self,
        result,
        timestamp: str | None = None,
        detection_count: int | None = None,
        include_image: bool = True,
    ) -> int | None:
        """Deliver one observation to QuailSync.

        POSTs to ``/api/indoorcam/observation``; if the API is unreachable or
        rejects the request, the observation is appended to the local JSONL
        write-ahead log so nothing is lost and it can be replayed later.

        ``detection_count`` (smoothed count) and ``include_image`` (whether a
        frame was saved) are passed through to :meth:`build_payload`.

        Returns the server-assigned observation **id** when delivered to the API
        (so the caller can later clear its image fields via :meth:`clear_image`),
        or ``None`` when the post was written to the WAL or fully lost.
        """
        payload = self.build_payload(
            result,
            timestamp=timestamp,
            detection_count=detection_count,
            include_image=include_image,
        )
        delivered, observation_id = self._post_http(payload)
        if delivered:
            logger.info(
                "Observation posted: camera=%s detections=%d id=%s",
                payload["camera_id"],
                payload["detection_count"],
                observation_id,
            )
            return observation_id
        # Server unreachable / rejected — preserve the observation in the WAL.
        try:
            self._append_jsonl(payload)
            logger.warning(
                "QuailSync unreachable — wrote observation to write-ahead log %s (camera=%s)",
                self.output_path.name,
                payload["camera_id"],
            )
        except Exception as exc:  # noqa: BLE001 — nothing more we can do; data lost
            logger.error("Failed to write WAL for %s: %s", result.image_path, exc)
        return None

    def _post_http(self, payload: dict) -> tuple[bool, int | None]:
        """POST one observation.

        Returns ``(delivered, observation_id)``: ``delivered`` is True on a 2xx
        response (the caller then does NOT fall back to the WAL), and
        ``observation_id`` is the new row id parsed from the response body (or
        ``None`` if the body had none). On any transport/HTTP error returns
        ``(False, None)``.
        """
        url = f"{self.api_url}/api/indoorcam/observation"
        try:
            resp = self._session().post(url, json=payload, timeout=30)
            resp.raise_for_status()
        except Exception as exc:  # noqa: BLE001 — network/HTTP errors fall back to WAL
            logger.error("POST %s failed: %s", url, exc)
            return False, None
        # Delivered. Best-effort parse of the {"stored":1,"id":N} response.
        observation_id = None
        try:
            data = resp.json()
            if isinstance(data, dict) and data.get("id") is not None:
                observation_id = int(data["id"])
        except Exception:  # noqa: BLE001 — a missing/odd body doesn't undo delivery
            observation_id = None
        return True, observation_id

    def clear_image(self, observation_id: int) -> bool:
        """Tell the server to null an observation's image fields.

        Called after a saved frame is uploaded to Roboflow and its local file is
        deleted, so the read endpoints stop advertising an image URL that would
        now 404. Best-effort: returns True on success, False on any error (the
        observation simply keeps its now-dangling filename — no worse than before
        this call existed).
        """
        url = f"{self.api_url}/api/indoorcam/observation/{observation_id}"
        try:
            resp = self._session().patch(url, timeout=30)
            resp.raise_for_status()
            logger.debug("Cleared image fields for observation %s", observation_id)
            return True
        except Exception as exc:  # noqa: BLE001 — never break the stream over this
            logger.warning(
                "Failed to clear image fields for observation %s: %s", observation_id, exc
            )
            return False

    def _session(self):
        """The injected HTTP client, or the lazily-imported ``requests`` module."""
        if self.session is not None:
            return self.session
        import requests  # lazy: keeps the module importable without requests

        return requests

    def _append_jsonl(self, payload: dict) -> None:
        """Append one JSON object as a line to the observations file."""
        self.output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.output_path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(payload) + "\n")
