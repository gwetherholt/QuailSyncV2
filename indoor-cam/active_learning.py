"""Roboflow active-learning upload for the indoor-cam pipeline.

After each POST cycle that saved a frame, the frame is uploaded to Roboflow as a
reviewable pre-label so it can be folded back into training — classic active
learning. We reuse the trail-cam ``RoboflowUploader`` (image + YOLO predictions
via the Roboflow SDK), but pointed at a DIFFERENT project (``find-chicks-5``, not
the trail cam's quail detector) while sharing the SAME ``ROBOFLOW_API_KEY``.

Best-effort and strictly opt-in: uploads run only when
``ROBOFLOW_UPLOAD_ENABLED`` is truthy AND ``ROBOFLOW_API_KEY`` is set. Any
failure (missing key, SDK not installed, network) is swallowed — it never breaks
the stream — and a failed upload leaves the local frame in place for retry.
"""

from __future__ import annotations

import logging

try:
    from . import config, detector
except ImportError:
    import config
    import detector

logger = logging.getLogger("indoorcam.active_learning")


class ActiveLearningUploader:
    """Lazily-connected Roboflow uploader scoped to the chick project."""

    def __init__(self):
        self._uploader = None  # reused across cycles (keeps the connection warm)

    @property
    def enabled(self) -> bool:
        """Whether uploads will actually run (flag on AND key present)."""
        return bool(config.ROBOFLOW_UPLOAD_ENABLED and config.ROBOFLOW_API_KEY)

    def _get_uploader(self):
        if self._uploader is None:
            roboflow_uploader = detector.import_trailcam_module("roboflow_uploader")
            self._uploader = roboflow_uploader.RoboflowUploader(
                api_key=config.ROBOFLOW_API_KEY,
                workspace=config.ROBOFLOW_WORKSPACE,
                project=config.ROBOFLOW_PROJECT,
                batch_name=config.ROBOFLOW_BATCH_NAME,
            )
            logger.info(
                "Roboflow active learning enabled -> %s/%s",
                config.ROBOFLOW_WORKSPACE,
                config.ROBOFLOW_PROJECT,
            )
        return self._uploader

    def upload(self, result) -> bool:
        """Upload one saved frame + its predictions. Returns True on success.

        Returns False (no-op) when disabled/unconfigured, or on any error — the
        caller keeps the local file for a later retry when this returns False.
        """
        if not config.ROBOFLOW_UPLOAD_ENABLED:
            return False
        if not config.ROBOFLOW_API_KEY:
            logger.debug("ROBOFLOW_API_KEY not set — skipping active-learning upload")
            return False
        try:
            return bool(self._get_uploader().upload_result(result))
        except Exception as exc:  # noqa: BLE001 — best-effort; never break the stream
            logger.warning("Roboflow active-learning upload failed: %s", exc)
            return False
