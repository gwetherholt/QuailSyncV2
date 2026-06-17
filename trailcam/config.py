"""Configuration for the QuailSync trail-camera pipeline.

Every setting is loaded from an environment variable so the pipeline can be
configured entirely from the systemd unit (``trailcam-pipeline.service``) or a
shell profile — no code edits to change a camera account, model, or schedule.

Import this module to read settings; call :func:`ensure_dirs` once at startup
to create the working directories.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

# --- SpyPoint account credentials -----------------------------------------
# Required for polling; left as ``None`` when unset so the poller can surface a
# clear "credentials not configured" error rather than failing here on import.
SPYPOINT_USERNAME: str | None = os.environ.get("SPYPOINT_USERNAME")
SPYPOINT_PASSWORD: str | None = os.environ.get("SPYPOINT_PASSWORD")

# --- Base working directory ------------------------------------------------
# Everything the pipeline writes lives under here. ``~`` is expanded so the
# systemd unit can use a tilde path.
BASE_DIR: Path = Path(os.environ.get("TRAILCAM_BASE_DIR", "~/trailcam")).expanduser()

# Working subdirectories (created by ``ensure_dirs()``):
#   staging/   — photos freshly downloaded from SpyPoint, awaiting detection
#   processed/ — photos that have been run through YOLO + reported to QuailSync
#   archive/   — long-term retention of originals
#   models/    — YOLO weights live here by default
STAGING_DIR: Path = BASE_DIR / "staging"
PROCESSED_DIR: Path = BASE_DIR / "processed"
ARCHIVE_DIR: Path = BASE_DIR / "archive"
MODELS_DIR: Path = BASE_DIR / "models"

# --- YOLO detector ---------------------------------------------------------
# Defaults to ``best.pt`` inside the models/ directory; set YOLO_MODEL_PATH to
# point at a model stored elsewhere.
YOLO_MODEL_PATH: Path = Path(
    os.environ.get("YOLO_MODEL_PATH", MODELS_DIR / "best.pt")
).expanduser()
YOLO_CONFIDENCE: float = float(os.environ.get("YOLO_CONFIDENCE", "0.5"))
# Optional integrity pin for the model weights. PyTorch .pt files are unpickled
# on load (arbitrary code execution if swapped), so when set the detector
# refuses to load a model whose SHA-256 doesn't match. Unset = no check.
YOLO_MODEL_SHA256: str | None = os.environ.get("YOLO_MODEL_SHA256")


# --- Per-camera model overrides --------------------------------------------
# Map of camera_id -> model path, loaded from the JSON env var CAMERA_MODELS, e.g.
#   CAMERA_MODELS='{"6a304dac5a82bf1a819b56d9": "/home/gwetherholt/trailcam/models/quail-detector.pt"}'
# A camera not listed here falls back to YOLO_MODEL_PATH (see model_for_camera).
def _load_camera_model_map() -> dict[str, Path]:
    raw = os.environ.get("CAMERA_MODELS", "").strip()
    if not raw:
        return {}
    try:
        data = json.loads(raw)
    except (json.JSONDecodeError, ValueError) as exc:
        # A misconfigured map shouldn't take the whole pipeline down on import.
        print(f"[config] WARNING: CAMERA_MODELS is not valid JSON ({exc}) — ignoring", file=sys.stderr)
        return {}
    if not isinstance(data, dict):
        print("[config] WARNING: CAMERA_MODELS must be a JSON object — ignoring", file=sys.stderr)
        return {}
    return {str(cid): Path(str(path)).expanduser() for cid, path in data.items()}


CAMERA_MODEL_MAP: dict[str, Path] = _load_camera_model_map()


def model_for_camera(camera_id: str | None) -> Path:
    """Model path for ``camera_id``, falling back to the global YOLO_MODEL_PATH
    when the camera isn't in CAMERA_MODEL_MAP."""
    if camera_id is not None:
        override = CAMERA_MODEL_MAP.get(camera_id)
        if override is not None:
            return override
    return YOLO_MODEL_PATH

# --- QuailSync server ------------------------------------------------------
QUAILSYNC_API_URL: str = os.environ.get(
    "QUAILSYNC_API_URL", "https://quailsync.tail01d133.ts.net"
)

# --- Polling behaviour -----------------------------------------------------
POLL_INTERVAL: int = int(os.environ.get("POLL_INTERVAL", "900"))  # seconds
PHOTO_LIMIT: int = int(os.environ.get("PHOTO_LIMIT", "25"))
# Hard cap on a single photo download (bytes). Guards against a malicious /
# buggy API response pointing at a huge file that would fill the Pi's disk.
MAX_PHOTO_SIZE_BYTES: int = int(os.environ.get("MAX_PHOTO_SIZE_BYTES", str(20_971_520)))  # 20 MiB

# Every directory the pipeline writes into — the single source of truth for
# ``ensure_dirs()``.
_ALL_DIRS: tuple[Path, ...] = (STAGING_DIR, PROCESSED_DIR, ARCHIVE_DIR, MODELS_DIR)


def ensure_dirs() -> None:
    """Create the base directory and all working subdirectories if missing.

    Idempotent — safe to call on every startup.
    """
    for directory in _ALL_DIRS:
        directory.mkdir(parents=True, exist_ok=True)


if __name__ == "__main__":
    # Convenience: `python config.py` creates the directory tree and prints the
    # resolved configuration (credentials are masked).
    ensure_dirs()
    print(f"BASE_DIR          = {BASE_DIR}")
    print(f"  staging/        = {STAGING_DIR}")
    print(f"  processed/      = {PROCESSED_DIR}")
    print(f"  archive/        = {ARCHIVE_DIR}")
    print(f"  models/         = {MODELS_DIR}")
    print(f"YOLO_MODEL_PATH   = {YOLO_MODEL_PATH}")
    print(f"CAMERA_MODEL_MAP  = {len(CAMERA_MODEL_MAP)} override(s): {dict(CAMERA_MODEL_MAP)}")
    print(f"YOLO_CONFIDENCE   = {YOLO_CONFIDENCE}")
    print(f"QUAILSYNC_API_URL = {QUAILSYNC_API_URL}")
    print(f"POLL_INTERVAL     = {POLL_INTERVAL}s")
    print(f"PHOTO_LIMIT       = {PHOTO_LIMIT}")
    print(f"SPYPOINT_USERNAME = {'<set>' if SPYPOINT_USERNAME else '<unset>'}")
    print(f"SPYPOINT_PASSWORD = {'<set>' if SPYPOINT_PASSWORD else '<unset>'}")
