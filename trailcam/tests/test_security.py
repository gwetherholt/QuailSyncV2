"""Security tests: path traversal, download size caps, HTTPS enforcement,
credential leakage, state-file permissions, and model-file integrity."""

import io
import logging
import os
import stat

import pytest
from PIL import Image

import yolo_detector
from spypoint_poller import (
    InsecureURLError,
    PhotoState,
    PhotoTooLargeError,
    SpypointPoller,
    sanitize_filename,
)
from yolo_detector import ModelIntegrityError

POSIX_ONLY = pytest.mark.skipif(os.name != "posix", reason="POSIX file permissions")


def _valid_jpeg() -> bytes:
    """A real (>1KB) JPEG so downloads pass the poller's image validation."""
    buf = io.BytesIO()
    Image.effect_noise((160, 160), 80).convert("RGB").save(buf, format="JPEG", quality=92)
    return buf.getvalue()


VALID_JPEG = _valid_jpeg()


# --- Fakes -----------------------------------------------------------------


class FakePhoto:
    def __init__(self, photo_id="p1", camera_id="camA", url="https://cdn.spypoint.test/p1/large.jpg"):
        self.id = photo_id
        self.camera_id = camera_id
        self._url = url
        self.date = "2026-01-02T03:04:05+00:00"

    def url(self, size="large"):
        return self._url


class FakeClient:
    def __init__(self, photos):
        self._photos = photos

    def login(self):
        pass

    def cameras(self):
        return ["camA"]

    def photos(self, cameras, limit=25):
        return self._photos[:limit]


class FakeResponse:
    def __init__(self, content=VALID_JPEG, headers=None):
        self._content = content
        self.headers = headers or {}

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size=8192):
        for i in range(0, len(self._content), chunk_size):
            yield self._content[i : i + chunk_size]


class FakeSession:
    def __init__(self, response=None):
        self.response = response if response is not None else FakeResponse()
        self.calls = 0

    def get(self, url, timeout=None, stream=None):
        self.calls += 1
        return self.response


def _poller(tmp_path, photos, session=None, **kwargs):
    state = PhotoState(tmp_path / "state.json")
    poller = SpypointPoller(
        "user", "pw", tmp_path / "staging", state, session=session or FakeSession(), **kwargs
    )
    poller.client = FakeClient(photos)
    return poller, state


# --- sanitize_filename -----------------------------------------------------


@pytest.mark.parametrize(
    "raw",
    [
        "../../etc",
        "../../../tmp/pwned",
        "a/b\\c",
        "with\x00null",
        "..",
        ".",
        "name with spaces",
        "../" * 50,
        "x" * 1000,
    ],
)
def test_sanitize_never_yields_unsafe_component(raw):
    safe = sanitize_filename(raw)
    assert "/" not in safe
    assert "\\" not in safe
    assert "\x00" not in safe
    assert safe not in ("", ".", "..")
    assert len(safe) <= 255


def test_sanitize_fallback_for_dotonly_and_empty():
    assert sanitize_filename("") == "unknown"
    assert sanitize_filename("..") == "unknown"
    assert sanitize_filename(".") == "unknown"
    assert sanitize_filename("\x00\x00") == "unknown"


def test_sanitize_preserves_normal_ids():
    assert sanitize_filename("camA") == "camA"
    assert sanitize_filename("5f3a9b2c-01") == "5f3a9b2c-01"


# --- Path traversal --------------------------------------------------------


def test_poll_path_traversal_stays_in_staging(tmp_path):
    staging = tmp_path / "staging"
    photo = FakePhoto(photo_id="../../../tmp/pwned", camera_id="../../etc")
    poller, _ = _poller(tmp_path, [photo])

    downloaded = poller.poll()

    assert len(downloaded) == 1
    resolved = downloaded[0].resolve()
    assert resolved.is_relative_to(staging.resolve())
    # Nothing was written anywhere outside the staging tree.
    assert not (tmp_path / "etc").exists()
    assert not (tmp_path / "tmp" / "pwned").exists()


def test_photo_id_special_chars_are_neutralized(tmp_path):
    for bad_id in ["a/b/c", "x\x00y", "..\\..\\win", "/abs/path"]:
        photo = FakePhoto(photo_id=bad_id, camera_id="camA")
        poller, _ = _poller(tmp_path, [photo])
        downloaded = poller.poll()
        assert len(downloaded) == 1
        name = downloaded[0].name
        assert "/" not in name and "\\" not in name and "\x00" not in name
        assert downloaded[0].resolve().is_relative_to((tmp_path / "staging").resolve())


# --- Download size limits --------------------------------------------------


def test_oversized_content_length_rejected(tmp_path, monkeypatch):
    monkeypatch.setattr("spypoint_poller.time.sleep", lambda _s: None)
    resp = FakeResponse(headers={"Content-Length": str(2 * 1024 * 1024 * 1024)})  # 2 GB
    poller, state = _poller(tmp_path, [FakePhoto()], session=FakeSession(resp), max_photo_size=20 * 1024 * 1024)

    downloaded = poller.poll()

    assert downloaded == []
    assert not state.has_seen("p1")  # not marked, will retry later
    assert list((tmp_path / "staging").rglob("*.jpg")) == []


def test_oversized_stream_aborted(tmp_path, monkeypatch):
    monkeypatch.setattr("spypoint_poller.time.sleep", lambda _s: None)
    # No Content-Length header, but the body is bigger than the (tiny) cap.
    resp = FakeResponse(content=b"\xff\xd8\xff" + b"x" * 1000, headers={})
    poller, state = _poller(tmp_path, [FakePhoto()], session=FakeSession(resp), max_photo_size=64)

    downloaded = poller.poll()

    assert downloaded == []
    staging = tmp_path / "staging"
    assert list(staging.rglob("*.jpg")) == []
    assert list(staging.rglob("*.part")) == []  # partial cleaned up


def test_too_large_is_not_retried(tmp_path):
    # Direct call: a size rejection raises immediately, not after max_retries.
    resp = FakeResponse(headers={"Content-Length": str(10**12)})
    session = FakeSession(resp)
    poller, _ = _poller(tmp_path, [FakePhoto()], session=session, max_photo_size=100, max_retries=3)
    with pytest.raises(PhotoTooLargeError):
        poller._download_with_retry("https://cdn.spypoint.test/x.jpg", tmp_path / "out.jpg")
    assert session.calls == 1  # one attempt, no retries


# --- HTTPS enforcement -----------------------------------------------------


def test_http_url_rejected_before_request(tmp_path):
    session = FakeSession()
    photo = FakePhoto(url="http://cdn.spypoint.test/p1/large.jpg")
    poller, state = _poller(tmp_path, [photo], session=session)

    downloaded = poller.poll()

    assert downloaded == []
    assert not state.has_seen("p1")
    assert session.calls == 0  # rejected before any network call


def test_download_with_retry_raises_on_http(tmp_path):
    poller, _ = _poller(tmp_path, [])
    with pytest.raises(InsecureURLError):
        poller._download_with_retry("http://insecure.test/x.jpg", tmp_path / "out.jpg")


# --- Credential handling ---------------------------------------------------


def test_password_never_logged_on_login_error(tmp_path, monkeypatch, caplog):
    import sys
    import types

    password = "SuperSecret-P@ssw0rd!"  # pragma: allowlist secret

    class Client:
        def __init__(self, username, pw):
            pass

        def login(self):
            raise RuntimeError("authentication rejected by server")

    fake_mod = types.ModuleType("spypoint")
    fake_mod.Client = Client
    monkeypatch.setitem(sys.modules, "spypoint", fake_mod)

    state = PhotoState(tmp_path / "state.json")
    poller = SpypointPoller("alice", password, tmp_path / "staging", state, session=FakeSession())  # pragma: allowlist secret

    with caplog.at_level(logging.DEBUG):
        with pytest.raises(RuntimeError):
            poller.login()

    assert password not in caplog.text
    for record in caplog.records:
        assert password not in record.getMessage()


def test_credentials_not_written_to_any_file(tmp_path):
    password = "PWD_SECRET_98765"  # pragma: allowlist secret
    state = PhotoState(tmp_path / "state.json")
    poller = SpypointPoller("alice", password, tmp_path / "staging", state, session=FakeSession())  # pragma: allowlist secret
    poller.client = FakeClient([FakePhoto()])

    poller.poll()
    state.save()

    for path in tmp_path.rglob("*"):
        if path.is_file():
            assert password.encode() not in path.read_bytes(), f"password leaked into {path}"


# --- State file permissions ------------------------------------------------


@POSIX_ONLY
def test_state_file_is_0600(tmp_path):
    path = tmp_path / "state.json"
    state = PhotoState(path)
    state.mark_seen("a")
    state.save()
    mode = stat.S_IMODE(os.stat(path).st_mode)
    assert mode == 0o600


# --- Model file integrity --------------------------------------------------


@POSIX_ONLY
def test_world_writable_model_warns(tmp_path, caplog):
    model = tmp_path / "best.pt"
    model.write_bytes(b"fake-weights")
    os.chmod(model, 0o666)

    with caplog.at_level(logging.WARNING):
        yolo_detector._verify_model_file(model)

    assert any("world-writable" in r.getMessage() for r in caplog.records)


def test_model_checksum_mismatch_raises(tmp_path, monkeypatch):
    model = tmp_path / "best.pt"
    model.write_bytes(b"weights-v1")
    monkeypatch.setattr(yolo_detector.config, "YOLO_MODEL_SHA256", "deadbeef" * 8)

    with pytest.raises(ModelIntegrityError):
        yolo_detector._verify_model_file(model)


def test_model_checksum_match_passes(tmp_path, monkeypatch):
    import hashlib

    content = b"weights-v1"
    model = tmp_path / "best.pt"
    model.write_bytes(content)
    monkeypatch.setattr(yolo_detector.config, "YOLO_MODEL_SHA256", hashlib.sha256(content).hexdigest())

    yolo_detector._verify_model_file(model)  # must not raise


def test_missing_model_skips_checks(tmp_path, monkeypatch):
    # A not-yet-downloaded named model (e.g. yolov8n.pt) must not raise.
    monkeypatch.setattr(yolo_detector.config, "YOLO_MODEL_SHA256", "deadbeef" * 8)
    yolo_detector._verify_model_file(tmp_path / "does_not_exist.pt")
