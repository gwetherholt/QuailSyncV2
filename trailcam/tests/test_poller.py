"""Tests for SpypointPoller — spypoint.Client and requests.Session are mocked;
the real API is never contacted."""

import io
import json
import sys
import types

from PIL import Image

from spypoint_poller import PhotoState, SpypointPoller


def _valid_jpeg() -> bytes:
    """A real (>1KB) JPEG so downloads pass the poller's image validation."""
    buf = io.BytesIO()
    Image.effect_noise((160, 160), 80).convert("RGB").save(buf, format="JPEG", quality=92)
    return buf.getvalue()


VALID_JPEG = _valid_jpeg()


# --- Fakes -----------------------------------------------------------------


class FakePhoto:
    def __init__(self, photo_id, camera_id="camA", date="2026-01-02T03:04:05+00:00"):
        self.id = photo_id
        self.camera_id = camera_id
        self.date = date

    def url(self, size="large"):
        return f"https://fake.spypoint/{self.id}/{size}.jpg"


class FakeClient:
    def __init__(self, photos):
        self._photos = photos
        self.logged_in = False

    def login(self):
        self.logged_in = True

    def cameras(self):
        return ["camA"]

    def photos(self, cameras, limit=25):
        return self._photos[:limit]


class FakeResponse:
    def __init__(self, content=VALID_JPEG):
        self._content = content
        self.headers = {}

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size=8192):
        for i in range(0, len(self._content), chunk_size):
            yield self._content[i : i + chunk_size]


class FakeSession:
    """Records GET calls; fails the first ``fail_times`` calls, then succeeds."""

    def __init__(self, fail_times=0):
        self.fail_times = fail_times
        self.calls = 0

    def get(self, url, timeout=None, stream=None):
        self.calls += 1
        if self.calls <= self.fail_times:
            raise ConnectionError("simulated network failure")
        return FakeResponse()


def _make_poller(tmp_path, photos, session, **kwargs):
    state = PhotoState(tmp_path / "state.json")
    poller = SpypointPoller(
        username="u",
        password="p",
        staging_dir=tmp_path / "staging",
        state=state,
        session=session,
        **kwargs,
    )
    # Inject the client directly so poll() doesn't call login() (which would
    # import the real spypoint library).
    poller.client = FakeClient(photos)
    return poller, state


# --- Tests -----------------------------------------------------------------


def test_login_uses_spypoint_client(tmp_path, monkeypatch):
    captured = {}

    class Client:
        def __init__(self, username, password):
            captured["args"] = (username, password)
            self.logged_in = False

        def login(self):
            self.logged_in = True

    fake_module = types.ModuleType("spypoint")
    fake_module.Client = Client
    monkeypatch.setitem(sys.modules, "spypoint", fake_module)

    state = PhotoState(tmp_path / "state.json")
    poller = SpypointPoller("user", "pass", tmp_path / "staging", state, session=FakeSession())
    poller.login()

    assert captured["args"] == ("user", "pass")
    assert isinstance(poller.client, Client)
    assert poller.client.logged_in is True


def test_poll_downloads_new_photos_with_sidecars(tmp_path):
    poller, state = _make_poller(tmp_path, [FakePhoto("p1"), FakePhoto("p2")], FakeSession())
    downloaded = poller.poll()

    assert len(downloaded) == 2
    camera_dir = tmp_path / "staging" / "camA"
    assert len(list(camera_dir.glob("*.jpg"))) == 2

    sidecars = list(camera_dir.glob("*.json"))
    assert len(sidecars) == 2
    meta = json.loads(sidecars[0].read_text())
    assert meta["camera_id"] == "camA"
    assert {"photo_id", "camera_id", "timestamp", "download_time"} <= set(meta)

    # Both ids are now persisted as seen.
    assert state.has_seen("p1") and state.has_seen("p2")


def test_poll_skips_already_seen(tmp_path):
    poller, state = _make_poller(tmp_path, [FakePhoto("p1"), FakePhoto("p2")], FakeSession())
    state.mark_seen("p1")

    downloaded = poller.poll()

    assert len(downloaded) == 1
    assert "p2" in downloaded[0].name


def test_download_retries_then_succeeds(tmp_path, monkeypatch):
    monkeypatch.setattr("spypoint_poller.time.sleep", lambda _seconds: None)
    session = FakeSession(fail_times=2)  # 2 failures, 3rd attempt works
    poller, state = _make_poller(tmp_path, [FakePhoto("p1")], session, max_retries=3)

    downloaded = poller.poll()

    assert len(downloaded) == 1
    assert session.calls == 3
    assert state.has_seen("p1")


def test_download_gives_up_and_leaves_unseen(tmp_path, monkeypatch):
    monkeypatch.setattr("spypoint_poller.time.sleep", lambda _seconds: None)
    session = FakeSession(fail_times=99)  # always fails
    poller, state = _make_poller(tmp_path, [FakePhoto("p1")], session, max_retries=3)

    downloaded = poller.poll()

    assert downloaded == []
    assert not state.has_seen("p1")  # so the next poll retries it
    assert session.calls == 3  # exactly max_retries attempts
    camera_dir = tmp_path / "staging" / "camA"
    assert list(camera_dir.glob("*.part")) == []  # partial cleaned up
    assert list(camera_dir.glob("*.jpg")) == []
