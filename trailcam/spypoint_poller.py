"""SpyPoint trail-camera poller for the QuailSync pipeline.

Polls a SpyPoint account via the ``pyspypoint`` library, downloads any photos
we haven't seen before into ``staging/{camera_id}/`` with timestamped
filenames, and drops a JSON metadata sidecar next to each image. Already-seen
photo IDs are persisted to a JSON state file so re-runs never re-download.

Run a single poll standalone:

    python spypoint_poller.py

The continuous poll loop (every ``POLL_INTERVAL`` seconds) lives in
``pipeline.py`` — this module deliberately does one poll per invocation so it's
easy to test and to drive from the pipeline or a cron/systemd timer.
"""

from __future__ import annotations

import ipaddress
import json
import logging
import os
import re
import socket
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

import requests

# Support both `python spypoint_poller.py` (script, sibling import) and
# `from trailcam import spypoint_poller` (package, relative import).
try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("trailcam.spypoint_poller")

# Single path component built from API data must never exceed this (ext4/most
# filesystems cap a name at 255 bytes).
MAX_FILENAME_LENGTH = 255

# Image sanity bounds. JPEG Start-Of-Image marker; a real photo is at least 1KB.
JPEG_MAGIC = b"\xff\xd8\xff"
MIN_PHOTO_SIZE_BYTES = 1024


class PhotoTooLargeError(Exception):
    """Raised when a download exceeds the configured size cap (non-retryable)."""


# These subclass ValueError so callers can catch either the specific type or a
# plain ValueError (the public _validate_url contract raises "ValueError").
class InsecureURLError(ValueError):
    """Raised when a photo URL is not HTTPS (non-retryable)."""


class UnsafeURLError(ValueError):
    """Raised when a URL resolves to a private/internal address (SSRF guard)."""


class InvalidImageError(ValueError):
    """Raised when downloaded bytes don't look like a sane JPEG."""


def sanitize_filename(value, *, fallback: str = "unknown", max_length: int = MAX_FILENAME_LENGTH) -> str:
    """Make an API-supplied string safe to use as a *single* path component.

    Untrusted ``camera_id`` / ``photo_id`` values from the SpyPoint API are used
    to build on-disk paths; without this a value like ``../../etc`` or one
    containing ``/`` or a null byte could write outside the staging tree. This:

    * removes null bytes and control characters,
    * replaces anything outside ``[A-Za-z0-9._-]`` (notably ``/`` and ``\\``)
      with ``_``,
    * rejects names that are empty or consist only of dots (``.``/``..``), and
    * caps the length.

    The result never contains a path separator, a null byte, or is ``.``/``..``,
    so ``base_dir / sanitize_filename(x)`` always stays inside ``base_dir``.
    """
    text = str(value).replace("\x00", "")
    # Drop other control characters, then allowlist-replace everything unsafe.
    text = "".join(ch for ch in text if ch >= " ")
    text = re.sub(r"[^A-Za-z0-9._-]", "_", text).strip()
    # A name of only dots (e.g. "." or "..") is a traversal/no-op component.
    if not text or set(text) <= {"."}:
        text = fallback
    return text[:max_length]


def _strip_exif(image_path: Path) -> None:
    """Rewrite a JPEG with all EXIF/metadata removed.

    Trail-cam photos can carry GPS, device, and timestamp EXIF that we don't
    want flowing into downstream systems (EXIF can also carry injection
    payloads). We rebuild the image from raw pixels into a fresh object with no
    ``info``/``exif``, then re-save. Best-effort: if Pillow is missing or the
    re-encode fails, we log and leave the original file in place.
    """
    try:
        from PIL import Image
    except ImportError:  # pragma: no cover - Pillow ships with ultralytics
        logger.warning("Pillow not installed — cannot strip EXIF from %s", image_path)
        return
    try:
        with Image.open(image_path) as im:
            rgb = im.convert("RGB")
            clean = Image.new("RGB", rgb.size)
            clean.putdata(list(rgb.getdata()))  # fresh image carries no metadata
        clean.save(image_path, format="JPEG", quality=95)
    except Exception as exc:  # noqa: BLE001 — never fail a download over EXIF
        logger.warning("Could not strip EXIF from %s: %s", image_path, exc)


class PhotoState:
    """Tracks already-downloaded photo IDs in a JSON file for cross-run dedup.

    The file format is simply ``{"seen_ids": ["...", ...]}``. IDs are stored as
    strings so the dedup is robust regardless of whether the library hands back
    ints or strings.
    """

    def __init__(self, path: Path | str):
        self.path = Path(path)
        self._seen: set[str] = set()
        self.load()

    def load(self) -> None:
        """Load seen IDs from disk. A missing or corrupt file starts empty."""
        if not self.path.exists():
            return
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
            self._seen = {str(pid) for pid in data.get("seen_ids", [])}
            logger.debug("Loaded %d seen photo id(s) from %s", len(self._seen), self.path)
        except (json.JSONDecodeError, OSError) as exc:
            logger.warning(
                "Could not read photo state %s (%s) — starting with an empty set",
                self.path,
                exc,
            )
            self._seen = set()

    def has_seen(self, photo_id) -> bool:
        return str(photo_id) in self._seen

    def mark_seen(self, photo_id) -> None:
        self._seen.add(str(photo_id))

    def save(self) -> None:
        """Persist the seen set atomically (write to temp, then rename).

        The file is locked down to ``0o600`` (owner read/write only) so another
        user can't tamper with the dedup state — clearing it to force mass
        re-downloads, or injecting ids to suppress real photos. The mode is set
        on the temp file *before* the rename so the final file is never briefly
        group/other-readable. (chmod is a no-op for these bits on Windows.)
        """
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(self.path.suffix + ".tmp")
        tmp.write_text(
            json.dumps({"seen_ids": sorted(self._seen)}, indent=2),
            encoding="utf-8",
        )
        try:
            os.chmod(tmp, 0o600)
        except OSError as exc:  # pragma: no cover - platform dependent
            logger.debug("Could not chmod state file %s: %s", tmp, exc)
        tmp.replace(self.path)

    def __len__(self) -> int:
        return len(self._seen)


class SpypointPoller:
    """Downloads new SpyPoint photos into the staging area.

    Parameters mirror ``config`` defaults so the poller is configured by the
    environment but remains fully overridable (handy for tests).
    """

    def __init__(
        self,
        username: str,
        password: str,
        staging_dir: Path | str,
        state: PhotoState,
        limit: int = config.PHOTO_LIMIT,
        photo_size: str = "large",
        max_retries: int = 3,
        backoff_base: float = 2.0,
        session: requests.Session | None = None,
        max_photo_size: int = config.MAX_PHOTO_SIZE_BYTES,
    ):
        self.username = username
        self.password = password
        self.staging_dir = Path(staging_dir)
        self.state = state
        self.limit = limit
        self.photo_size = photo_size
        self.max_retries = max_retries
        self.backoff_base = backoff_base
        self.session = session or requests.Session()
        self.max_photo_size = max_photo_size
        self.client = None  # set by login()

    def __repr__(self) -> str:
        # Credentials must never leak via repr (logs, tracebacks, debuggers).
        return (
            f"SpypointPoller(username={self._redact(self.username)!r}, "
            f"authenticated={self.client is not None})"
        )

    @staticmethod
    def _redact(value) -> str:
        """Mask a credential to first+last char, e.g. 'gwetherholt' -> 'g***t'."""
        text = "" if value is None else str(value)
        if len(text) <= 2:
            return "*" * len(text)
        return f"{text[0]}{'*' * (len(text) - 2)}{text[-1]}"

    # -- connection ---------------------------------------------------------

    def login(self) -> None:
        """Authenticate against the SpyPoint API."""
        import spypoint  # imported lazily so PhotoState is usable without the lib

        logger.info("Logging in to SpyPoint as %s", self.username)
        self.client = spypoint.Client(self.username, self.password)
        self.client.login()

    # -- polling ------------------------------------------------------------

    def poll(self) -> list[Path]:
        """Run one poll: download every photo not already in the state file.

        Returns the list of image paths downloaded this run. A photo is only
        marked "seen" after its image AND sidecar are safely on disk, so a
        failed download is simply retried on the next poll.
        """
        if self.client is None:
            self.login()

        # The API/library can return anything: invalid JSON, a payload bomb,
        # deeply nested structures, etc. Any failure fetching the camera list is
        # logged and turned into "no photos this poll" rather than crashing.
        try:
            cameras = self.client.cameras()
        except Exception as exc:  # noqa: BLE001 — bad JSON, payload bomb, deep nesting…
            logger.error("Failed to fetch camera list from SpyPoint: %s", exc)
            return []

        logger.info("Found %d camera(s)", len(cameras))

        # Fetch photos one camera at a time rather than all at once. The shared
        # `limit` applies *per request*, so a single high-volume camera can no
        # longer consume the whole budget and starve the others — each camera
        # gets its own up-to-`limit` slice. A failure on one camera is logged
        # and skipped so the rest of the poll still runs.
        photo_iter: list = []
        for camera in cameras:
            label = self._camera_label(camera)
            try:
                photos = self.client.photos([camera], limit=self.limit)
                camera_photos = list(photos)  # force any lazy parsing here
            except Exception as exc:  # noqa: BLE001 — bad JSON, payload bomb, deep nesting…
                logger.error("Failed to fetch photos for camera %s: %s", label, exc)
                continue
            logger.info("Camera %s: %d photo(s) returned", label, len(camera_photos))
            photo_iter.extend(camera_photos)

        downloaded: list[Path] = []
        for photo in photo_iter:
            # Defensively pull the id — a malformed photo (no id, or an id whose
            # str() blows up, e.g. a pathologically nested object) is skipped
            # rather than crashing the whole poll.
            try:
                raw_id = getattr(photo, "id", None)
                if raw_id is None:
                    logger.error("Skipping malformed photo object (no id)")
                    continue
                photo_id = str(raw_id)
            except Exception as exc:  # noqa: BLE001
                logger.error("Skipping photo with unreadable id: %s", exc)
                continue
            if self.state.has_seen(photo_id):
                continue
            try:
                image_path = self._download_photo(photo)
            except Exception as exc:  # noqa: BLE001 — keep polling other photos
                logger.error(
                    "Giving up on photo %s after %d attempt(s): %s",
                    photo_id,
                    self.max_retries,
                    exc,
                )
                continue  # leave it unseen so the next poll retries
            self.state.mark_seen(photo_id)
            self.state.save()  # persist eagerly so a crash never re-downloads
            downloaded.append(image_path)

        logger.info("Poll complete: %d new photo(s) downloaded", len(downloaded))
        return downloaded

    # -- per-photo download -------------------------------------------------

    # NOTE: download is split into two methods on purpose —
    #   _download_photo()      = policy/orchestration (SpyPoint-specific): work
    #                            out the camera, timestamp, paths, then write
    #                            the image + metadata sidecar.
    #   _download_with_retry() = pure transport (URL -> file): retry, backoff,
    #                            atomic write. Knows nothing about photos.
    # Keeping the fiddly retry loop quarantined keeps this method readable, and
    # makes the transport helper reusable (e.g. quailsync_bridge.py) and
    # testable on its own without constructing a fake photo object.
    def _download_photo(self, photo) -> Path:
        # camera_id and photo.id come straight from the API — sanitize BOTH
        # before they touch the filesystem so a malicious value (e.g.
        # "../../etc") can't escape the staging tree. The dedup key in poll()
        # still uses the raw id; only the on-disk name is sanitized.
        camera_id = sanitize_filename(self._camera_id(photo))
        safe_photo_id = sanitize_filename(str(photo.id))
        taken_at = self._photo_timestamp(photo)
        download_time = datetime.now(timezone.utc)

        camera_dir = self.staging_dir / camera_id
        camera_dir.mkdir(parents=True, exist_ok=True)

        # Timestamp-based filename, suffixed with the (sanitized) photo id to
        # keep names unique even if two photos share a second.
        stem = f"{taken_at:%Y%m%d-%H%M%S}_{safe_photo_id}"
        image_path = camera_dir / f"{stem}.jpg"
        sidecar_path = camera_dir / f"{stem}.json"

        # Defense in depth: confirm the resolved target really is inside staging
        # before writing anything (catches any sanitizer gap).
        staging_root = self.staging_dir.resolve()
        if not image_path.resolve().is_relative_to(staging_root):
            raise ValueError(f"refusing to write outside staging dir: {image_path}")

        url = photo.url(size=self.photo_size)
        self._download_with_retry(url, image_path)

        # Validate the bytes really are a sane JPEG (not HTML from a redirect to
        # a login page, a renamed PNG, garbage, etc.). On failure, remove the
        # file and bail so the photo isn't marked seen.
        try:
            self._validate_image(image_path.read_bytes())
        except Exception:
            image_path.unlink(missing_ok=True)
            raise
        # Strip EXIF before anything downstream touches the image.
        _strip_exif(image_path)

        metadata = {
            "photo_id": str(photo.id),
            "camera_id": camera_id,
            "timestamp": taken_at.isoformat(),
            "download_time": download_time.isoformat(),
            "source_url": url,
            "image_file": image_path.name,
            # Ambient temperature the camera reported (°F), if the API exposes
            # it. EXIF is stripped from the image, so this is the only path.
            "ambient_temperature_f": self._photo_temperature(photo),
        }
        sidecar_path.write_text(json.dumps(metadata, indent=2), encoding="utf-8")

        logger.info("Downloaded photo %s -> %s", photo.id, image_path)
        return image_path

    def _download_with_retry(self, url: str, dest: Path) -> None:
        """Download ``url`` to ``dest`` with up to ``max_retries`` attempts and
        exponential backoff (``backoff_base ** (attempt - 1)`` seconds).

        Two non-retryable guards run here: the URL must be HTTPS, and the
        download must stay under ``max_photo_size`` (checked both against the
        declared ``Content-Length`` and the actual streamed byte count). These
        raise immediately rather than burning retries on a request that can
        never succeed.
        """
        # HTTPS-only, and not pointed at a private/internal address (SSRF).
        self._validate_url(url)
        if not self._is_safe_url(url):
            raise UnsafeURLError(f"refusing URL resolving to a private address: {url}")

        part = dest.with_suffix(dest.suffix + ".part")
        last_exc: Exception | None = None
        for attempt in range(1, self.max_retries + 1):
            try:
                resp = self.session.get(url, timeout=30, stream=True)
                resp.raise_for_status()
                self._reject_if_too_large_declared(resp)

                total = 0
                with open(part, "wb") as fh:
                    for chunk in resp.iter_content(chunk_size=8192):
                        if not chunk:
                            continue
                        total += len(chunk)
                        if total > self.max_photo_size:
                            raise PhotoTooLargeError(
                                f"download exceeded {self.max_photo_size} bytes — aborting"
                            )
                        fh.write(chunk)
                part.replace(dest)  # atomic: only a complete file appears at dest
                return
            except (PhotoTooLargeError, InsecureURLError):
                # Size/security rejections are deterministic — don't retry.
                part.unlink(missing_ok=True)
                raise
            except Exception as exc:  # noqa: BLE001 — broad on purpose, we retry
                last_exc = exc
                if attempt == self.max_retries:
                    break
                delay = self.backoff_base ** (attempt - 1)
                logger.warning(
                    "Download attempt %d/%d failed for %s (%s) — retrying in %.1fs",
                    attempt,
                    self.max_retries,
                    url,
                    exc,
                    delay,
                )
                time.sleep(delay)
        # Exhausted retries — clean up any partial file and re-raise.
        part.unlink(missing_ok=True)
        raise RuntimeError(f"download failed after {self.max_retries} attempts: {url}") from last_exc

    def _reject_if_too_large_declared(self, resp) -> None:
        """Reject before downloading if the server's Content-Length already
        exceeds the cap. Missing/garbage headers are ignored (the streaming
        guard still enforces the real limit)."""
        headers = getattr(resp, "headers", {}) or {}
        declared = headers.get("Content-Length")
        if declared is None:
            return
        try:
            size = int(declared)
        except (TypeError, ValueError):
            return
        if size > self.max_photo_size:
            raise PhotoTooLargeError(
                f"declared size {size} bytes exceeds limit {self.max_photo_size}"
            )

    # -- URL / SSRF / image validation --------------------------------------

    @staticmethod
    def _validate_url(url) -> None:
        """Require an HTTPS scheme. Raises ``ValueError`` (``InsecureURLError``)
        for anything else — never silently downgrades."""
        scheme = urlparse(str(url)).scheme.lower()
        if scheme != "https":
            raise InsecureURLError(f"refusing non-HTTPS URL (scheme={scheme!r}): {url}")

    def _is_safe_url(self, url) -> bool:
        """Return False if the URL's host resolves to a private/internal address.

        Guards against SSRF (incl. DNS rebinding) by resolving the hostname via
        ``socket.getaddrinfo`` and rejecting loopback / RFC1918 / link-local /
        ULA / reserved / multicast targets. A host that can't be resolved is
        allowed through — there's nothing to SSRF *to*, and the actual request
        will simply fail.
        """
        host = urlparse(str(url)).hostname
        if not host:
            return False
        try:
            infos = socket.getaddrinfo(host, None, proto=socket.IPPROTO_TCP)
        except (socket.gaierror, UnicodeError, OSError) as exc:
            logger.debug("Could not resolve %s for SSRF check (%s) — allowing", host, exc)
            return True
        for info in infos:
            ip_text = info[4][0]
            try:
                ip = ipaddress.ip_address(ip_text)
            except ValueError:
                return False
            if self._is_blocked_ip(ip):
                logger.warning("Refusing SSRF-unsafe URL %s -> %s", url, ip_text)
                return False
        return True

    @staticmethod
    def _is_blocked_ip(ip) -> bool:
        """True for any address we must never connect to from the poller."""
        return (
            ip.is_private
            or ip.is_loopback
            or ip.is_link_local
            or ip.is_reserved
            or ip.is_multicast
            or ip.is_unspecified
        )

    def _validate_image(self, data: bytes) -> None:
        """Reject anything that isn't a plausible JPEG photo.

        Checks size bounds, the JPEG magic bytes, and (if Pillow is available)
        that PIL can actually parse it — catching renamed PNGs, HTML login
        pages returned by a redirect, truncated/garbage files, etc.
        """
        if len(data) < MIN_PHOTO_SIZE_BYTES:
            raise InvalidImageError(f"image too small: {len(data)} bytes (< {MIN_PHOTO_SIZE_BYTES})")
        if len(data) > self.max_photo_size:
            raise InvalidImageError(f"image too large: {len(data)} bytes (> {self.max_photo_size})")
        if not data.startswith(JPEG_MAGIC):
            raise InvalidImageError("not a JPEG (bad magic bytes)")
        try:
            from PIL import Image
        except ImportError:  # pragma: no cover - Pillow ships with ultralytics
            logger.warning("Pillow not installed — skipping deep image validation")
            return
        import io

        try:
            with Image.open(io.BytesIO(data)) as im:
                im.verify()  # structural parse without full decode
        except Exception as exc:  # noqa: BLE001 — any parse failure = invalid
            raise InvalidImageError(f"PIL could not parse image: {exc}") from exc

    # -- photo attribute extraction (defensive) -----------------------------
    # The pyspypoint Photo object's exact field names for camera and timestamp
    # aren't guaranteed across versions, so we probe a few common names and
    # fall back sensibly. Adjust the candidate lists if your library differs.

    @staticmethod
    def _camera_label(camera) -> str:
        """Best-effort identifier for a camera object from ``client.cameras()``,
        used only for per-camera log lines. Probes common id/name fields on
        either a dict or an object and falls back to ``str(camera)``. Never
        raises — a bad label must not break the poll loop."""
        try:
            if isinstance(camera, dict):
                for key in ("id", "camera_id", "cameraId", "name"):
                    value = camera.get(key)
                    if value:
                        return str(value)
            else:
                for attr in ("id", "camera_id", "cameraId", "name"):
                    value = getattr(camera, attr, None)
                    if value:
                        return str(value)
            return str(camera)
        except Exception:  # noqa: BLE001 — a label must never break the poll
            return "camera"

    @staticmethod
    def _camera_id(photo) -> str:
        for attr in ("camera_id", "cameraId", "camera"):
            value = getattr(photo, attr, None)
            if value:
                # `camera` may be an object with its own id.
                return str(getattr(value, "id", value))
        logger.warning("Photo %s has no recognizable camera id — using 'unknown'", getattr(photo, "id", "?"))
        return "unknown"

    @staticmethod
    def _photo_timestamp(photo) -> datetime:
        for attr in ("date", "timestamp", "origin_date", "taken", "originDate"):
            value = getattr(photo, attr, None)
            if not value:
                continue
            if isinstance(value, datetime):
                return value
            try:
                # Tolerate ISO-8601 with a trailing 'Z'.
                return datetime.fromisoformat(str(value).replace("Z", "+00:00"))
            except ValueError:
                continue
        # No usable timestamp on the photo — fall back to "now".
        return datetime.now(timezone.utc)

    @staticmethod
    def _photo_temperature(photo) -> float | None:
        """Best-effort ambient temperature (°F) from the SpyPoint photo metadata.

        EXIF is stripped from the saved image, so the API photo object is the
        only source. Field names vary across pyspypoint versions / camera
        models, so probe several common ones. SpyPoint accounts configured for
        Fahrenheit report °F directly; we pass the value through as °F and
        return ``None`` when nothing usable is present."""
        for attr in (
            "temperature_f", "temperatureF", "temp_f", "tempF",
            "temperature", "temp",
        ):
            value = getattr(photo, attr, None)
            if value is None:
                continue
            # The field may itself be a dict/object wrapping the number.
            if isinstance(value, dict):
                value = (
                    value.get("fahrenheit")
                    or value.get("value")
                    or value.get("f")
                )
            else:
                value = getattr(value, "value", value)
            try:
                return round(float(value), 1)
            except (TypeError, ValueError):
                continue
        return None


def main() -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    config.ensure_dirs()

    if not config.SPYPOINT_USERNAME or not config.SPYPOINT_PASSWORD:
        logger.error(
            "SPYPOINT_USERNAME / SPYPOINT_PASSWORD are not set — "
            "configure them in the environment (or the systemd unit) and retry."
        )
        return 1

    state = PhotoState(config.BASE_DIR / "photo_state.json")
    poller = SpypointPoller(
        username=config.SPYPOINT_USERNAME,
        password=config.SPYPOINT_PASSWORD,
        staging_dir=config.STAGING_DIR,
        state=state,
        limit=config.PHOTO_LIMIT,
    )

    try:
        new_photos = poller.poll()
    except Exception as exc:  # noqa: BLE001 — top-level guard for a CLI run
        logger.exception("Poll failed: %s", exc)
        return 1

    logger.info("Done — %d new photo(s) in %s", len(new_photos), config.STAGING_DIR)
    return 0


if __name__ == "__main__":
    sys.exit(main())
