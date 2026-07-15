"""POST one observation per cycle to the backend (mirrors the old indoor-cam bridge).

The dashboard and Android app show the indoor camera's live detection count +
image by reading ``GET /api/indoorcam/latest/{camera_id}`` — which only returns
data once observations have been POSTed. The old ``indoor-cam`` pipeline did this
via ``indoor-cam/bridge.py``; this module replicates that exact endpoint and
payload so the backend serves the unified pipeline's data unchanged.

Contract (matching ``ObservationRequest`` in ``routes/indoorcam.rs`` and the old
bridge's ``build_payload``):

* ``POST {backend_url}/api/indoorcam/observation`` with a JSON body carrying
  ``camera_id``, ``timestamp``, ``detection_count``, ``average_confidence``,
  ``min_confidence``, ``detections`` (``[{class_name, confidence, bbox}]``),
  ``inference_time_ms``, ``image_filename`` and ``annotated_image_filename``
  (basenames only — the backend serves ``processed/{camera_id}/{filename}``).
* The ``camera_id`` is the OBSERVATION/serving id (``indoor-1``) the backend,
  dashboard, and app key on — distinct from the assignment camera id.
* ``class_name`` comes straight from the YOLO detections (``egg`` in incubation
  mode, ``chick`` in brooder mode) — never hardcoded.

Best-effort: a POST that fails (backend unreachable / non-2xx) is logged and
swallowed so it never breaks the capture loop. ``requests`` is imported lazily
and the HTTP session is injectable, so this module imports cheaply and
unit-tests without the network.
"""

from __future__ import annotations

import logging
import re

logger = logging.getLogger("indoorpipeline.observations")


class ObservationClient:
    """Builds observation payloads and POSTs them to the backend."""

    def __init__(self, backend_url: str, camera_id: str, *, session=None, timeout: float = 30.0):
        self.url = f"{backend_url.rstrip('/')}/api/indoorcam/observation"
        self.camera_id = camera_id
        self.session = session
        self.timeout = timeout

    @staticmethod
    def _sanitize_string(value, *, max_length: int = 200):
        """Strip dangerous content from a free-text field bound for the server.

        Mirrors the old bridge: removes null bytes, HTML/script tags, and
        SQL/HTML metacharacters, collapses whitespace, and caps length (defense in
        depth — the server validates too). ``None`` passes through unchanged.
        """
        if value is None:
            return None
        text = str(value).replace("\x00", "")
        text = re.sub(r"<[^>]*>", "", text)  # drop HTML/script tags entirely
        text = re.sub(r"[;'\"`<>]", "", text)  # drop SQL/HTML metacharacters
        text = re.sub(r"\s+", " ", text).strip()
        return text[:max_length]

    def build_payload(
        self,
        detections,
        *,
        timestamp: str | None,
        image_filename: str | None = None,
        annotated_image_filename: str | None = None,
        inference_time_ms: float = 0.0,
    ) -> dict:
        """Shape a list of YOLO detections into the QuailSync observation payload.

        ``detection_count`` is this cycle's raw detection count, and the
        ``detections`` array carries each box's actual ``class_name`` (so the
        backend derives its class breakdown / label from real model output).
        """
        confidences = [d.confidence for d in detections]
        average_confidence = round(sum(confidences) / len(confidences), 4) if confidences else None
        min_confidence = round(min(confidences), 4) if confidences else None
        return {
            "camera_id": self.camera_id,
            "timestamp": timestamp,
            "detection_count": len(detections),
            "average_confidence": average_confidence,
            "min_confidence": min_confidence,
            "detections": [
                {
                    "class_name": self._sanitize_string(d.class_name),
                    "confidence": d.confidence,
                    "bbox": list(d.bbox),
                }
                for d in detections
            ],
            "inference_time_ms": inference_time_ms,
            # Basenames only — the server serves images from processed/{camera_id}/.
            "image_filename": image_filename,
            "annotated_image_filename": annotated_image_filename,
        }

    def _session_obj(self):
        """The injected HTTP client, or the lazily-imported ``requests`` module."""
        if self.session is not None:
            return self.session
        import requests  # lazy: keeps the module importable without requests

        return requests

    def post(
        self,
        detections,
        *,
        timestamp: str | None,
        image_filename: str | None = None,
        annotated_image_filename: str | None = None,
        inference_time_ms: float = 0.0,
    ) -> int | None:
        """POST one observation. Returns the new observation id, or ``None``.

        Never raises: a transport/HTTP failure (e.g. backend unreachable) is
        logged and swallowed so the capture loop keeps running.
        """
        payload = self.build_payload(
            detections,
            timestamp=timestamp,
            image_filename=image_filename,
            annotated_image_filename=annotated_image_filename,
            inference_time_ms=inference_time_ms,
        )
        try:
            resp = self._session_obj().post(self.url, json=payload, timeout=self.timeout)
            resp.raise_for_status()
        except Exception as exc:  # noqa: BLE001 — a failed POST must never crash the loop
            logger.warning(
                "Observation POST to %s failed (%s) — dashboard may show stale data until next cycle",
                self.url,
                exc,
            )
            return None
        # Best-effort parse of the {"stored":1,"id":N} response body.
        observation_id = None
        try:
            data = resp.json()
            if isinstance(data, dict) and data.get("id") is not None:
                observation_id = int(data["id"])
        except Exception:  # noqa: BLE001 — a missing/odd body doesn't undo delivery
            observation_id = None
        logger.info(
            "Observation posted: camera=%s detections=%d id=%s",
            payload["camera_id"],
            payload["detection_count"],
            observation_id,
        )
        return observation_id


def build_observation_client(conf, *, session=None) -> ObservationClient | None:
    """Return an :class:`ObservationClient` when observation POSTing is enabled,
    else ``None`` (a no-op — every post attempt is skipped)."""
    obs = conf.observations
    if obs is None or not obs.enabled:
        logger.debug("Observation POSTing disabled")
        return None
    logger.info(
        "Observation POSTing enabled -> %s/api/indoorcam/observation (camera_id=%s)",
        obs.backend_url,
        obs.camera_id,
    )
    return ObservationClient(obs.backend_url, obs.camera_id, session=session)
