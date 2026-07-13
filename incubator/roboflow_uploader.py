"""Optional Roboflow raw-frame upload for the incubator pipeline.

Auto-uploads raw incubator frames to Roboflow so a labeling dataset builds up
over time (across lighting, turner positions, and time of day) plus every
interesting change-detection frame (pipping, hatching). There is **no model
yet**, so unlike the trail-cam / indoor-cam uploaders this uploads *raw,
unannotated* images only — no YOLO pre-labels, no labelmap. The frames land in
Roboflow ready for manual labeling.

Mirrors the sibling pipelines' contract — strictly opt-in and best-effort:

* Runs only when ``roboflow.enabled`` is true AND the API key is set. With the
  key unset the upload is skipped *silently* — nothing in the pipeline breaks.
* Uses Roboflow's **REST upload API** (``requests``), not the ``roboflow`` SDK:
  a raw image POST needs neither the heavy SDK nor a project handle.
* ``requests`` is imported lazily (only when an upload actually runs), so
  importing this module never requires the dependency, and the HTTP call is
  injectable (``post=``) for testing.
* Every failure is caught and logged; an upload error never propagates into the
  service loop.

The service loop builds an uploader once via :func:`build_uploader` (``None`` when
disabled / unkeyed) and calls :meth:`RoboflowUploader.upload_frame`.
"""

from __future__ import annotations

import logging

logger = logging.getLogger("incubator.roboflow_uploader")

# Distinguishes these auto-uploads from manual uploads in the Roboflow UI.
BATCH_NAME = "incubator-auto"

# Roboflow REST upload endpoint. The workspace is implied by the API key; the
# project (dataset) id goes in the path. A raw multipart image POST creates an
# unannotated image in the project, ready for manual labeling.
UPLOAD_URL = "https://api.roboflow.com/dataset/{project}/upload"


class RoboflowUploader:
    """Uploads raw JPEG frames to a Roboflow project over the REST upload API.

    ``post`` (defaulting to ``requests.post``) and ``cv2_module`` are injectable
    so uploads are testable without the network or OpenCV.
    """

    def __init__(
        self,
        api_key: str,
        project: str,
        workspace: str,
        *,
        batch_name: str = BATCH_NAME,
        post=None,
        timeout: float = 30.0,
    ):
        self.api_key = api_key
        self.project = project
        self.workspace = workspace
        self.batch_name = batch_name
        self._post = post
        self.timeout = timeout

    @classmethod
    def from_config(cls, conf, *, post=None) -> "RoboflowUploader":
        """Build an uploader from a loaded :class:`config.Config` (requires the
        key set)."""
        rf = conf.roboflow
        if not rf.api_key:
            raise RuntimeError(f"{rf.api_key_env} is not set")
        return cls(
            api_key=rf.api_key,
            project=rf.project,
            workspace=rf.workspace,
            post=post,
        )

    def _post_fn(self):
        """Resolve the HTTP POST callable, importing ``requests`` lazily."""
        if self._post is None:
            try:
                import requests  # lazy: only a real upload needs it
            except ImportError as exc:  # pragma: no cover - runtime-only path
                raise RuntimeError(
                    "requests is required for Roboflow upload (pip install requests)"
                ) from exc
            self._post = requests.post
        return self._post

    def _encode_jpeg(self, frame, cv2_module=None) -> bytes | None:
        """Encode a BGR numpy frame to JPEG bytes, or ``None`` on failure."""
        cv2 = cv2_module
        if cv2 is None:
            try:
                import cv2  # lazy: shared with the rest of the pipeline
            except ImportError as exc:  # pragma: no cover - runtime-only path
                logger.warning("opencv not available to encode frame: %s", exc)
                return None
        ok, buf = cv2.imencode(".jpg", frame)
        if not ok:
            logger.warning("Failed to JPEG-encode frame for Roboflow upload")
            return None
        return buf.tobytes()

    def upload_image_bytes(self, data: bytes, name: str) -> bool:
        """POST raw JPEG ``data`` to Roboflow as an unannotated image.

        Returns True on success. Never raises — any error is logged and swallowed
        so a failed upload can't break the pipeline.
        """
        url = UPLOAD_URL.format(project=self.project)
        params = {
            "api_key": self.api_key,
            "name": name,
            "batch_name": self.batch_name,
            "split": "train",
        }
        post = self._post_fn()
        try:
            resp = post(
                url,
                params=params,
                files={"file": (name, data, "image/jpeg")},
                timeout=self.timeout,
            )
        except Exception as exc:  # noqa: BLE001 — upload is best-effort, never fatal
            logger.warning("Roboflow upload error for %s: %s", name, exc)
            return False

        status = getattr(resp, "status_code", 200)
        if status >= 400:
            body = getattr(resp, "text", "")
            logger.warning("Roboflow upload failed (HTTP %s) for %s: %s", status, name, body)
            return False
        logger.info(
            "Uploaded %s to Roboflow %s/%s (batch=%s)",
            name,
            self.workspace,
            self.project,
            self.batch_name,
        )
        return True

    def upload_frame(self, frame, name: str, *, cv2_module=None) -> bool:
        """Encode ``frame`` (a BGR numpy image) to JPEG and upload it as ``name``."""
        data = self._encode_jpeg(frame, cv2_module=cv2_module)
        if data is None:
            return False
        return self.upload_image_bytes(data, name)


def build_uploader(conf, *, post=None) -> RoboflowUploader | None:
    """Return an uploader when Roboflow upload is enabled AND keyed, else ``None``.

    A ``None`` return means "don't upload" — the two silent-skip cases:

    * ``roboflow.enabled`` false -> no-op.
    * key unset -> skipped *silently* (debug log), never an error.

    The service loop calls this once at startup; a ``None`` result makes every
    upload attempt a no-op.
    """
    rf = conf.roboflow
    if not rf.enabled:
        logger.debug("Roboflow upload disabled (roboflow.enabled is false)")
        return None
    if not rf.api_key:
        # Silent skip — enabling without a key is not worth shouting about.
        logger.debug("%s not set — skipping Roboflow upload", rf.api_key_env)
        return None
    logger.info(
        "Roboflow auto-upload enabled -> %s/%s (batch=%s, every %ss, on_event=%s)",
        rf.workspace,
        rf.project,
        BATCH_NAME,
        rf.upload_interval_seconds,
        rf.upload_on_event,
    )
    return RoboflowUploader.from_config(conf, post=post)
