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

import json
import logging
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests

# Support both `python spypoint_poller.py` (script, sibling import) and
# `from trailcam import spypoint_poller` (package, relative import).
try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("trailcam.spypoint_poller")


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
        """Persist the seen set atomically (write to temp, then rename)."""
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(self.path.suffix + ".tmp")
        tmp.write_text(
            json.dumps({"seen_ids": sorted(self._seen)}, indent=2),
            encoding="utf-8",
        )
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
        self.client = None  # set by login()

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

        cameras = self.client.cameras()
        logger.info("Found %d camera(s)", len(cameras))
        photos = self.client.photos(cameras, limit=self.limit)

        downloaded: list[Path] = []
        for photo in photos:
            photo_id = str(photo.id)
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
        camera_id = self._camera_id(photo)
        taken_at = self._photo_timestamp(photo)
        download_time = datetime.now(timezone.utc)

        camera_dir = self.staging_dir / camera_id
        camera_dir.mkdir(parents=True, exist_ok=True)

        # Timestamp-based filename, suffixed with the photo id to guarantee
        # uniqueness even if two photos share a second.
        stem = f"{taken_at:%Y%m%d-%H%M%S}_{photo.id}"
        image_path = camera_dir / f"{stem}.jpg"
        sidecar_path = camera_dir / f"{stem}.json"

        url = photo.url(size=self.photo_size)
        self._download_with_retry(url, image_path)

        metadata = {
            "photo_id": str(photo.id),
            "camera_id": camera_id,
            "timestamp": taken_at.isoformat(),
            "download_time": download_time.isoformat(),
            "source_url": url,
            "image_file": image_path.name,
        }
        sidecar_path.write_text(json.dumps(metadata, indent=2), encoding="utf-8")

        logger.info("Downloaded photo %s -> %s", photo.id, image_path)
        return image_path

    def _download_with_retry(self, url: str, dest: Path) -> None:
        """Download ``url`` to ``dest`` with up to ``max_retries`` attempts and
        exponential backoff (``backoff_base ** (attempt - 1)`` seconds)."""
        last_exc: Exception | None = None
        for attempt in range(1, self.max_retries + 1):
            try:
                resp = self.session.get(url, timeout=30, stream=True)
                resp.raise_for_status()
                tmp = dest.with_suffix(dest.suffix + ".part")
                with open(tmp, "wb") as fh:
                    for chunk in resp.iter_content(chunk_size=8192):
                        if chunk:
                            fh.write(chunk)
                tmp.replace(dest)  # atomic: only a complete file appears at dest
                return
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
        dest.with_suffix(dest.suffix + ".part").unlink(missing_ok=True)
        raise RuntimeError(f"download failed after {self.max_retries} attempts: {url}") from last_exc

    # -- photo attribute extraction (defensive) -----------------------------
    # The pyspypoint Photo object's exact field names for camera and timestamp
    # aren't guaranteed across versions, so we probe a few common names and
    # fall back sensibly. Adjust the candidate lists if your library differs.

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
