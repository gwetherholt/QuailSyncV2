"""Tests for SettingsClient — 60s caching, refresh, and error fallback. No
network: a fake session returns queued payloads (or raises)."""

from settings_client import SettingsClient


class _Resp:
    def __init__(self, payload):
        self._payload = payload

    def raise_for_status(self):
        pass

    def json(self):
        return self._payload


class FakeSession:
    """`get()` returns queued items in order (reusing the last once exhausted);
    an ``Exception`` item is raised instead of returned."""

    def __init__(self, items):
        assert items, "need at least one item"
        self._items = list(items)
        self._last = self._items[0]
        self.calls = 0

    def get(self, url, timeout=None):
        self.calls += 1
        if self._items:
            self._last = self._items.pop(0)
        if isinstance(self._last, Exception):
            raise self._last
        return _Resp(self._last)


def _client(session, clock_holder, ttl=60.0):
    return SettingsClient(
        api_url="http://qs.test", session=session, ttl=ttl, clock=lambda: clock_holder["t"]
    )


def test_fetches_once_and_caches_within_ttl():
    t = {"t": 0.0}
    session = FakeSession([{"indoor_cam_roboflow_upload_enabled": False, "indoor_cam_image_save_enabled": True}])
    c = _client(session, t)

    assert c.roboflow_upload_enabled() is False  # fetch #1
    assert c.image_save_enabled() is True        # served from cache
    t["t"] = 59.0
    assert c.roboflow_upload_enabled() is False   # still cached
    assert session.calls == 1


def test_refetches_after_ttl():
    t = {"t": 0.0}
    session = FakeSession([
        {"indoor_cam_roboflow_upload_enabled": True, "indoor_cam_image_save_enabled": True},
        {"indoor_cam_roboflow_upload_enabled": False, "indoor_cam_image_save_enabled": True},
    ])
    c = _client(session, t)

    assert c.roboflow_upload_enabled() is True
    t["t"] = 61.0  # past ttl -> refetch
    assert c.roboflow_upload_enabled() is False
    assert session.calls == 2


def test_error_with_no_cache_defaults_both_on():
    t = {"t": 0.0}
    session = FakeSession([OSError("connection refused")])
    c = _client(session, t)

    # Never fetched successfully -> both toggles default ON (server defaults).
    assert c.roboflow_upload_enabled() is True
    assert c.image_save_enabled() is True


def test_error_after_success_uses_stale_cache():
    t = {"t": 0.0}
    session = FakeSession([
        {"indoor_cam_roboflow_upload_enabled": False, "indoor_cam_image_save_enabled": False},
        OSError("blip"),
    ])
    c = _client(session, t)

    assert c.roboflow_upload_enabled() is False  # cached good values
    t["t"] = 61.0  # ttl expired -> tries refetch, fails -> stale cache
    assert c.roboflow_upload_enabled() is False
    assert c.image_save_enabled() is False
    assert session.calls == 2
    # The failed attempt backed off a full ttl: next call within ttl is cached.
    t["t"] = 100.0
    assert c.roboflow_upload_enabled() is False
    assert session.calls == 2


def test_non_dict_response_falls_back_to_defaults():
    t = {"t": 0.0}
    session = FakeSession([["not", "a", "dict"]])
    c = _client(session, t)
    assert c.roboflow_upload_enabled() is True  # treated as a failure -> default ON
