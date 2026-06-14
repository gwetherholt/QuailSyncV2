"""Advanced security tests for the trail-cam pipeline.

Each test below targets one concrete attack vector against the poller/bridge.
Grouped by theme:

  * TLS verification  — never fetch over plaintext / with verification disabled
  * SSRF protection   — never connect to private/internal addresses
  * Malformed input   — survive hostile/garbage API responses without crashing
  * Image validation  — only accept genuine JPEG photos
  * EXIF sanitization — never propagate embedded metadata downstream
  * Credential leaks  — secrets never reach logs, files, or repr()
  * Bridge sanitization — never forward injection payloads to QuailSync

Nothing here touches the real SpyPoint API or the network: numeric-IP SSRF
checks use the local resolver (no DNS needed), hostname cases mock
``socket.getaddrinfo``, and all HTTP is served by in-process fakes.
"""

import io
import json
import logging
import socket
import sys
import types

import pytest
from PIL import Image

import spypoint_poller
from quailsync_bridge import QuailSyncBridge
from spypoint_poller import (
    InsecureURLError,
    InvalidImageError,
    PhotoState,
    SpypointPoller,
    UnsafeURLError,
    _strip_exif,
)
from yolo_detector import Detection, DetectionResult


def _valid_jpeg(width=160, height=160) -> bytes:
    """A real (>1KB) JPEG of random noise — large enough and structurally valid
    so it passes the poller's ``_validate_image`` checks."""
    buf = io.BytesIO()
    Image.effect_noise((width, height), 80).convert("RGB").save(buf, format="JPEG", quality=92)
    return buf.getvalue()


VALID_JPEG = _valid_jpeg()
USERNAME = "gwetherholt"
PASSWORD = "SuperSecretPassword123!"


# --- Fakes -----------------------------------------------------------------
# In-process stand-ins so no test hits the real API, network, or filesystem
# beyond tmp_path.


class FakeResponse:
    """Mimics the slice of ``requests.Response`` the downloader uses: a headers
    dict, ``raise_for_status()``, and chunked ``iter_content()``."""

    def __init__(self, content=VALID_JPEG, headers=None):
        self._content = content
        self.headers = headers or {}

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size=8192):
        for i in range(0, len(self._content), chunk_size):
            yield self._content[i : i + chunk_size]


class CapturingSession:
    """A fake requests.Session that records every ``get()`` call's kwargs (so a
    test can assert TLS verification was never disabled) and exposes the real
    default ``verify=True``."""

    def __init__(self, response=None):
        self.response = response if response is not None else FakeResponse()
        self.calls = []
        self.verify = True

    def get(self, url, **kwargs):
        self.calls.append((url, kwargs))
        return self.response


class FakePhoto:
    """A minimal SpyPoint photo: ``id``, ``camera_id``, and a ``url(size)``
    method. Defaults to a CDN URL on the reserved ``.test`` TLD (never
    resolves, so the SSRF check lets it through in tests)."""

    def __init__(self, photo_id="p1", camera_id="camA", url="https://cdn.spypoint.test/p1/large.jpg"):
        self.id = photo_id
        self.camera_id = camera_id
        self._url = url
        self.date = "2026-01-02T03:04:05+00:00"

    def url(self, size="large"):
        return self._url


class FakeClient:
    """A fake spypoint.Client. Either returns a list of photos, or raises
    ``photos_exc`` from ``photos()`` to simulate a hostile/garbage response."""

    def __init__(self, photos=None, photos_exc=None):
        self._photos = photos or []
        self._exc = photos_exc

    def login(self):
        pass

    def cameras(self):
        return ["camA"]

    def photos(self, cameras, limit=25):
        if self._exc is not None:
            raise self._exc
        return self._photos[:limit]


def make_poller(tmp_path, photos=None, session=None, client=None, **kwargs):
    """Build a poller wired to fakes and a fresh state file under tmp_path.
    ``client`` is injected directly so ``poll()`` never calls the real
    ``login()`` (which would import the real spypoint library)."""
    state = PhotoState(tmp_path / "state.json")
    poller = SpypointPoller(
        USERNAME, PASSWORD, tmp_path / "staging", state, session=session or CapturingSession(), **kwargs
    )
    poller.client = client if client is not None else FakeClient(photos or [])
    return poller, state


# ===========================================================================
# TLS verification
# ===========================================================================


def test_session_verify_enabled_by_default(tmp_path):
    """The poller's HTTP session must keep TLS certificate verification on.

    requests defaults ``verify=True``; this guards against a regression that
    sets ``verify=False`` (which would silently accept MITM'd connections).
    """
    import requests

    assert requests.Session().verify is True
    poller, _ = make_poller(tmp_path)
    assert getattr(poller.session, "verify", True) is not False


def test_download_never_disables_tls_verification(tmp_path):
    """Every outbound download call must not pass ``verify=False``.

    Drives a real ``poll()`` through a CapturingSession and inspects the kwargs
    of each ``get()`` — none may disable verification.
    """
    session = CapturingSession()
    poller, _ = make_poller(tmp_path, photos=[FakePhoto()], session=session)
    poller.poll()
    assert session.calls, "expected at least one download request"
    for _url, kwargs in session.calls:
        assert kwargs.get("verify", True) is not False


def test_validate_url_requires_https(tmp_path):
    """``_validate_url`` accepts https and rejects everything else.

    Plaintext (http) and other schemes (ftp) raise ValueError /
    InsecureURLError — the URL is never silently upgraded.
    """
    poller, _ = make_poller(tmp_path)
    poller._validate_url("https://ok.example/x.jpg")  # no raise
    with pytest.raises(ValueError):
        poller._validate_url("http://insecure.example/x.jpg")
    with pytest.raises(InsecureURLError):
        poller._validate_url("ftp://example/x")


def test_http_photo_rejected_before_request(tmp_path):
    """An http photo URL is rejected *before* any network call is made.

    Asserts the CapturingSession recorded zero gets and the photo wasn't marked
    seen (so it isn't silently dropped — it just never downloads over http).
    """
    session = CapturingSession()
    poller, state = make_poller(
        tmp_path, photos=[FakePhoto(url="http://cdn.spypoint.test/x.jpg")], session=session
    )
    assert poller.poll() == []
    assert session.calls == []  # rejected before any network call
    assert not state.has_seen("p1")


# ===========================================================================
# SSRF protection
# ===========================================================================


@pytest.mark.parametrize(
    "url",
    [
        "http://127.0.0.1/malicious",
        "https://127.0.0.1/malicious",
        "http://192.168.0.114:3000/api/admin",
        "https://10.0.0.5/",
        "http://172.16.5.4/",
        "http://169.254.169.254/latest/meta-data/",  # cloud metadata endpoint
        "http://[::1]/",
    ],
)
def test_is_safe_url_rejects_private_targets(tmp_path, url):
    """``_is_safe_url`` returns False for any URL whose host is an internal IP.

    Covers IPv4 loopback, the three RFC1918 private ranges, link-local
    (incl. the 169.254.169.254 cloud-metadata trap), and IPv6 loopback. These
    are numeric literals, so the real resolver handles them offline.
    """
    poller, _ = make_poller(tmp_path)
    assert poller._is_safe_url(url) is False


def test_is_safe_url_rejects_dns_rebinding(tmp_path, monkeypatch):
    """A public-looking hostname that *resolves* to a private IP is rejected.

    This is the DNS-rebinding case: the name looks innocent, but
    getaddrinfo returns 10.x. We mock the resolver to simulate that and assert
    the URL is judged unsafe.
    """
    poller, _ = make_poller(tmp_path)

    def fake_getaddrinfo(host, *args, **kwargs):
        return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("10.1.2.3", 0))]

    monkeypatch.setattr(spypoint_poller.socket, "getaddrinfo", fake_getaddrinfo)
    assert poller._is_safe_url("https://totally-legit.example/photo.jpg") is False


def test_is_safe_url_allows_public_cdn(tmp_path, monkeypatch):
    """A hostname resolving to a public IP passes validation.

    The complement of the rebinding test — confirms the SSRF guard isn't a
    blanket deny: a legit CDN (mocked to a public IP) is allowed.
    """
    poller, _ = make_poller(tmp_path)

    def fake_getaddrinfo(host, *args, **kwargs):
        return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("93.184.216.34", 0))]

    monkeypatch.setattr(spypoint_poller.socket, "getaddrinfo", fake_getaddrinfo)
    assert poller._is_safe_url("https://cdn.spypoint.com/photo.jpg") is True


def test_download_rejects_ssrf_target(tmp_path, monkeypatch):
    """The SSRF check is actually wired into the download path, not just a
    standalone helper.

    With the resolver mocked to loopback, ``_download_with_retry`` raises
    UnsafeURLError (and ``time.sleep`` is stubbed so a retry path wouldn't hang).
    """
    monkeypatch.setattr("spypoint_poller.time.sleep", lambda _s: None)

    def fake_getaddrinfo(host, *args, **kwargs):
        return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("127.0.0.1", 0))]

    monkeypatch.setattr(spypoint_poller.socket, "getaddrinfo", fake_getaddrinfo)
    poller, _ = make_poller(tmp_path)
    with pytest.raises(UnsafeURLError):
        poller._download_with_retry("https://rebind.example/x.jpg", tmp_path / "out.jpg")


# ===========================================================================
# Malformed API responses
# ===========================================================================


@pytest.mark.parametrize(
    "exc",
    [
        ValueError("response is not valid JSON"),
        RecursionError("maximum recursion depth exceeded"),  # deep nesting
        MemoryError("payload bomb"),  # 10MB+ JSON
        TypeError("unexpected structure"),
        KeyError("large"),
    ],
)
def test_malformed_photo_list_returns_empty(tmp_path, exc):
    """If fetching/parsing the photo list blows up, ``poll()`` logs and returns
    [] instead of propagating.

    Each parametrized exception models a hostile response: invalid JSON, a
    deeply-nested structure (RecursionError), a payload bomb (MemoryError),
    a wrong-shaped object (TypeError), or a missing field (KeyError). None may
    crash the service.
    """
    poller, _ = make_poller(tmp_path, client=FakeClient(photos_exc=exc))
    assert poller.poll() == []  # logged + empty, never raises


def test_photo_missing_id_skipped(tmp_path):
    """A photo object with no ``id`` is skipped, not fatal.

    Without an id we can't dedup or name the file, so it's logged and ignored;
    ``poll()`` returns [] rather than raising AttributeError.
    """

    class NoId:
        def url(self, size="large"):
            return "https://cdn.spypoint.test/x.jpg"

    poller, _ = make_poller(tmp_path, photos=[NoId()])
    assert poller.poll() == []


def test_photo_missing_url_skipped(tmp_path):
    """A photo missing its download URL (no ``large.host``) is skipped.

    Calling ``url()`` raises AttributeError mid-download; that photo is dropped
    and — crucially — left *unseen* so a later, well-formed response can retry.
    """

    class NoUrl:
        id = "p9"  # has id but no url() -> AttributeError during download

    poller, state = make_poller(tmp_path, photos=[NoUrl()])
    assert poller.poll() == []
    assert not state.has_seen("p9")


def test_photo_id_nested_object_does_not_crash(tmp_path):
    """A photo whose ``id`` is a deeply-nested object can't crash the poll.

    ``str()`` on a 50-deep dict (or worse) must not take down the loop — the id
    handling is wrapped so the worst case is a skipped photo, not an exception.
    """
    deep = {"a": 1}
    for _ in range(50):
        deep = {"nested": deep}
    poller, _ = make_poller(tmp_path, photos=[FakePhoto(photo_id=deep)])
    result = poller.poll()  # must not raise
    assert isinstance(result, list)


# ===========================================================================
# Image validation
# ===========================================================================


def test_validate_image_accepts_valid_jpeg(tmp_path):
    """The happy path: a genuine JPEG passes ``_validate_image`` silently."""
    poller, _ = make_poller(tmp_path)
    poller._validate_image(VALID_JPEG)  # no raise


def test_validate_image_rejects_png(tmp_path):
    """A PNG renamed to .jpg is rejected on its magic bytes.

    Content-type/extension can't be trusted; the first bytes (``\\x89PNG``)
    aren't a JPEG SOI marker, so it's refused.
    """
    poller, _ = make_poller(tmp_path)
    png = b"\x89PNG\r\n\x1a\n" + b"\x00" * 2000
    with pytest.raises(InvalidImageError):
        poller._validate_image(png)


def test_validate_image_rejects_zero_byte(tmp_path):
    """An empty download is rejected by the minimum-size check (a real photo is
    at least ~1KB)."""
    poller, _ = make_poller(tmp_path)
    with pytest.raises(InvalidImageError):
        poller._validate_image(b"")


def test_validate_image_rejects_jpeg_header_then_garbage(tmp_path):
    """Right JPEG magic bytes but a junk body is still rejected.

    Catches a file that spoofs the SOI marker to pass a naive magic-byte check —
    PIL's structural parse fails, so it's refused.
    """
    poller, _ = make_poller(tmp_path)
    data = b"\xff\xd8\xff" + b"\x00" * 4000  # right magic, junk body
    with pytest.raises(InvalidImageError):
        poller._validate_image(data)


def test_validate_image_rejects_html_login_page(tmp_path):
    """An HTML page (what a redirect to a login page returns) is rejected.

    A 200 response whose body is ``<!DOCTYPE html>...`` rather than an image
    must not be stored as a photo.
    """
    poller, _ = make_poller(tmp_path)
    html = b"<!DOCTYPE html><html><body>Please log in</body></html>" + b" " * 2000
    with pytest.raises(InvalidImageError):
        poller._validate_image(html)


def test_download_of_html_is_rejected_and_unmarked(tmp_path):
    """End-to-end: a download that returns HTML is rejected by the full poll.

    Validation is wired into ``_download_photo``, so the file is removed, the
    photo stays unseen (retryable), and nothing lands in staging.
    """
    html = b"<html>login</html>" + b" " * 2000
    session = CapturingSession(FakeResponse(content=html))
    poller, state = make_poller(tmp_path, photos=[FakePhoto()], session=session)
    assert poller.poll() == []
    assert not state.has_seen("p1")
    assert list((tmp_path / "staging").rglob("*.jpg")) == []


# ===========================================================================
# EXIF sanitization
# ===========================================================================


def test_strip_exif_removes_metadata(tmp_path):
    """``_strip_exif`` removes embedded EXIF from a JPEG on disk.

    Writes a JPEG carrying an ImageDescription tag, confirms it's present, runs
    the stripper, and asserts the re-saved file has no EXIF and no raw exif
    block in ``info``.
    """
    path = tmp_path / "img.jpg"
    img = Image.effect_noise((64, 64), 50).convert("RGB")
    exif = img.getexif()
    exif[0x010E] = "secret GPS / device description"  # ImageDescription tag
    img.save(path, format="JPEG", exif=exif)

    assert len(Image.open(path).getexif()) > 0  # present before

    _strip_exif(path)

    assert len(Image.open(path).getexif()) == 0  # gone after
    assert Image.open(path).info.get("exif") is None


def test_poll_strips_exif_from_downloaded_photo(tmp_path):
    """End-to-end: EXIF is stripped as part of a normal download.

    Serves a JPEG that *has* EXIF; the staged file produced by ``poll()`` must
    have none — proving the strip step runs in the real pipeline, not just when
    called directly.
    """
    img = Image.effect_noise((96, 96), 60).convert("RGB")
    exif = img.getexif()
    exif[0x010E] = "leak me"
    buf = io.BytesIO()
    img.save(buf, format="JPEG", exif=exif)

    session = CapturingSession(FakeResponse(content=buf.getvalue()))
    poller, _ = make_poller(tmp_path, photos=[FakePhoto()], session=session)
    downloaded = poller.poll()

    assert len(downloaded) == 1
    assert len(Image.open(downloaded[0]).getexif()) == 0


# ===========================================================================
# Credential / token leak prevention
# ===========================================================================


def test_repr_redacts_credentials(tmp_path):
    """``repr(poller)`` must not expose the username or password.

    repr() shows up in logs, tracebacks, and debuggers, so it returns a masked
    username (e.g. ``g*********t``) and an ``authenticated=`` flag — never the
    raw secrets.
    """
    poller, _ = make_poller(tmp_path)
    text = repr(poller)
    assert PASSWORD not in text
    assert USERNAME not in text
    assert text.startswith("SpypointPoller(")
    assert "authenticated=" in text
    assert "*" in text  # username is masked


def test_password_never_in_logs_success_and_failure(tmp_path, monkeypatch, caplog):
    """The password never appears in log output, on either login path.

    Captures logs at DEBUG while driving both a successful login and a failing
    one (mocked spypoint.Client), then asserts the password is absent from every
    record and the combined log text.
    """
    state = PhotoState(tmp_path / "state.json")

    class GoodClient:
        def __init__(self, username, pw):
            pass

        def login(self):
            pass

    class BadClient:
        def __init__(self, username, pw):
            pass

        def login(self):
            raise RuntimeError("authentication rejected")

    fake_mod = types.ModuleType("spypoint")
    monkeypatch.setitem(sys.modules, "spypoint", fake_mod)

    with caplog.at_level(logging.DEBUG):
        fake_mod.Client = GoodClient
        SpypointPoller("alice", PASSWORD, tmp_path / "staging", state, session=CapturingSession()).login()

        fake_mod.Client = BadClient
        with pytest.raises(RuntimeError):
            SpypointPoller("alice", PASSWORD, tmp_path / "staging", state, session=CapturingSession()).login()

    assert PASSWORD not in caplog.text
    for record in caplog.records:
        assert PASSWORD not in record.getMessage()


def test_credentials_not_persisted_to_any_file(tmp_path):
    """No file the poller writes may contain the password.

    Runs a full poll plus a state save, then scans every file under tmp_path
    (state JSON, sidecars, images) for the password bytes.
    """
    state = PhotoState(tmp_path / "state.json")
    poller = SpypointPoller("alice", PASSWORD, tmp_path / "staging", state, session=CapturingSession())
    poller.client = FakeClient([FakePhoto()])
    poller.poll()
    state.save()

    for path in tmp_path.rglob("*"):
        if path.is_file():
            assert PASSWORD.encode() not in path.read_bytes(), f"password leaked into {path}"


# ===========================================================================
# QuailSync bridge input sanitization
# ===========================================================================


def _det_result(camera_id="camA", class_name="quail"):
    """Build a one-detection DetectionResult with overridable camera_id /
    class_name so tests can inject adversarial strings."""
    detection = Detection(class_name, 0.9, [1.0, 2.0, 3.0, 4.0])
    return DetectionResult(
        image_path="/x.jpg",
        camera_id=camera_id,
        timestamp="2026-01-01T00:00:00+00:00",
        total_count=1,
        detections=[detection],
        inference_time_ms=5.0,
        model_version="m.pt",
    )


@pytest.mark.parametrize(
    "raw",
    [
        "'; DROP TABLE birds;--",
        "<script>alert(1)</script>",
        "cam\x00null",
        'a"b`c<d>',
    ],
)
def test_bridge_sanitize_string_strips_dangerous_chars(raw):
    """``_sanitize_string`` removes SQL/HTML metacharacters and null bytes.

    For each adversarial input, none of ``; ' " ` < >`` or NUL survives in the
    output — so an injection payload can't be forwarded verbatim to the server.
    """
    out = QuailSyncBridge._sanitize_string(raw)
    for bad in [";", "'", '"', "`", "<", ">", "\x00"]:
        assert bad not in out


def test_bridge_payload_sanitizes_camera_and_class(tmp_path):
    """The two camera/model-derived free-text fields are sanitized in the
    payload.

    Builds an observation from a result with a SQLi ``camera_id`` and a
    ``<script>`` ``class_name`` and asserts both come out clean in the payload
    dict.
    """
    bridge = QuailSyncBridge(output_path=tmp_path / "obs.jsonl")
    result = _det_result(camera_id="'; DROP TABLE birds;--", class_name="<script>x</script>")
    payload = bridge.build_payload(result)
    for bad in [";", "'", "<", ">", "\x00"]:
        assert bad not in payload["camera_id"]
        assert bad not in payload["detections"][0]["class_name"]


def test_bridge_post_writes_sanitized_values(tmp_path):
    """Sanitization survives the JSONL round-trip written by ``post()``.

    Asserts the raw payload markers (``';``, ``<b>``) aren't in the written line
    and the record still parses with non-empty fields.
    """
    out = tmp_path / "obs.jsonl"
    bridge = QuailSyncBridge(output_path=out)
    assert bridge.post(_det_result(camera_id="x';--", class_name="<b>y</b>")) is True
    line = out.read_text(encoding="utf-8").strip()
    assert "';" not in line and "<b>" not in line
    record = json.loads(line)
    assert record["camera_id"] and record["detections"][0]["class_name"]


def test_bridge_preserves_clean_values(tmp_path):
    """Sanitization is non-destructive for legitimate values.

    Confirms ordinary ids/labels (``cam-7A``, ``quail``) pass through unchanged,
    so the filter isn't mangling real data.
    """
    bridge = QuailSyncBridge(output_path=tmp_path / "obs.jsonl")
    payload = bridge.build_payload(_det_result(camera_id="cam-7A", class_name="quail"))
    assert payload["camera_id"] == "cam-7A"
    assert payload["detections"][0]["class_name"] == "quail"
