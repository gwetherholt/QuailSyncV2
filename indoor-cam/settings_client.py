"""Cached client for the server's system settings (the in-app indoor-cam toggles).

The pipeline checks two runtime toggles each POST cycle:
  * ``indoor_cam_roboflow_upload_enabled`` — upload notable frames to Roboflow,
  * ``indoor_cam_image_save_enabled``       — save notable frames to disk (PC).

To avoid hitting the API on every frame, the ``GET /api/system-settings``
response is cached and only re-fetched every ``ttl`` seconds (default 60). On a
fetch error the last good cache is reused; before the first successful fetch the
toggles default to ON (matching the server defaults), so a transient API blip
never silently disables saving/uploading.
"""

from __future__ import annotations

import logging
import time as _time

try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("indoorcam.settings_client")

# Server defaults (both ON) — used only before the first successful fetch.
_DEFAULTS = {
    "indoor_cam_roboflow_upload_enabled": True,
    "indoor_cam_image_save_enabled": True,
}


class SettingsClient:
    """Fetches and caches the system settings, refreshing at most every ``ttl``s."""

    def __init__(self, api_url: str | None = None, *, session=None, ttl: float = 60.0, clock=_time.monotonic):
        self.api_url = (api_url or config.QUAILSYNC_API_URL).rstrip("/")
        self.session = session
        self.ttl = ttl
        self.clock = clock
        self._cache: dict | None = None
        self._fetched_at: float | None = None

    def _session_obj(self):
        if self.session is not None:
            return self.session
        import requests  # lazy: keeps the module importable without requests

        return requests

    def get(self) -> dict:
        """Return the current settings dict, fetching only when the cache is cold
        or older than ``ttl``. Never raises."""
        now = self.clock()
        if (
            self._cache is not None
            and self._fetched_at is not None
            and (now - self._fetched_at) < self.ttl
        ):
            return self._cache
        try:
            resp = self._session_obj().get(f"{self.api_url}/api/system-settings", timeout=10)
            resp.raise_for_status()
            data = resp.json()
            if not isinstance(data, dict):
                raise ValueError("system settings response was not a JSON object")
            self._cache = data
            self._fetched_at = now
            return data
        except Exception as exc:  # noqa: BLE001 — settings must never break the stream
            if self._cache is not None:
                logger.warning("System settings refresh failed (%s) — using cached values", exc)
                self._fetched_at = now  # back off a full ttl before retrying
                return self._cache
            logger.warning("System settings fetch failed (%s) — defaulting toggles ON", exc)
            return _DEFAULTS  # no _fetched_at set: keep retrying until first success

    def _flag(self, key: str) -> bool:
        return bool(self.get().get(key, True))

    def roboflow_upload_enabled(self) -> bool:
        return self._flag("indoor_cam_roboflow_upload_enabled")

    def image_save_enabled(self) -> bool:
        return self._flag("indoor_cam_image_save_enabled")
