"""Configuration for the QuailSync indoor-camera pipeline (continuous mode).

The indoor camera runs as a **continuous ~1fps stream processor** (not a snapshot
poller): the RTSP stream is held open with OpenCV, one frame per second is
sampled and run through the chick-detection YOLO model, counts are smoothed with
a rolling median, and observations are POSTed with smart batching. Each posted
frame is also uploaded to Roboflow for active learning.

Every setting is loaded from an environment variable so the pipeline is
configured entirely from the systemd unit (``indoor-cam-pipeline.service``) or a
shell profile — no code edits to change the camera, model, or cadence.

Secrets vs. non-secrets:

* RTSP **credentials** and the **Roboflow API key** come from the out-of-repo
  ``EnvironmentFile`` (``/home/gwetherholt/.indoor-cam-secrets``, root-owned,
  ``chmod 600``) — never committed, never in the unit's ``Environment=`` lines,
  never logged.
* Everything else (model path, cadence, API URL, camera id) is non-secret and
  set via ``Environment=`` lines.

Import this module to read settings; call :func:`ensure_dirs` once at startup to
create the working directories.
"""

from __future__ import annotations

import os
import re
from pathlib import Path
from urllib.parse import quote


def _env_flag(name: str, default: str = "false") -> bool:
    return os.environ.get(name, default).strip().lower() in ("1", "true", "yes", "on")


# --- RTSP source -----------------------------------------------------------
# Either provide the full RTSP_URL (with credentials) directly, or supply the
# components and let :func:`rtsp_url` assemble it. Credentials live ONLY in the
# environment (the chmod-600 secrets file) — never in the repo, the unit file,
# logs, or this process's argv. Continuous capture always uses OpenCV (the URL
# stays in-process), so the password is never exposed to `ps`.
RTSP_URL: str | None = os.environ.get("RTSP_URL")
RTSP_USERNAME: str | None = os.environ.get("RTSP_USERNAME")
RTSP_PASSWORD: str | None = os.environ.get("RTSP_PASSWORD")
RTSP_HOST: str | None = os.environ.get("RTSP_HOST")
RTSP_PORT: int = int(os.environ.get("RTSP_PORT", "554"))
RTSP_PATH: str = os.environ.get("RTSP_PATH", "/stream1")
RTSP_TRANSPORT: str = os.environ.get("RTSP_TRANSPORT", "tcp")

# Reconnection backoff when the stream drops (seconds): grows with each
# consecutive failure, capped at STREAM_MAX_BACKOFF.
STREAM_RECONNECT_BACKOFF: float = float(os.environ.get("STREAM_RECONNECT_BACKOFF", "2.0"))
STREAM_MAX_BACKOFF: float = float(os.environ.get("STREAM_MAX_BACKOFF", "30.0"))

# --- Camera identity -------------------------------------------------------
# Stable slug the poller posts observations with; the server keys
# indoor_cameras / indoor_camera_observations on this.
CAMERA_ID: str = os.environ.get("INDOOR_CAMERA_ID", "indoor-1")

# --- Base working directory ------------------------------------------------
BASE_DIR: Path = Path(os.environ.get("INDOORCAM_BASE_DIR", "~/indoor-cam")).expanduser()

# Working subdirectories (created by ``ensure_dirs()``):
#   capture/   — the latest sampled frame is written here for inference
#   processed/ — frames we POST are persisted here (server serves images from
#                processed/{camera_id}/) plus the annotated copies
#   models/    — YOLO weights live here by default
CAPTURE_DIR: Path = BASE_DIR / "capture"
PROCESSED_DIR: Path = BASE_DIR / "processed"
MODELS_DIR: Path = BASE_DIR / "models"

# --- YOLO detector (CHICK model) -------------------------------------------
# Inference uses the chick-detection model (YOLOv8n, ~6.2MB best.pt from the
# chick-detector training run) — NOT the adult-quail trail-cam model. The repo
# default points at training/chick-detector/weights/best.pt; the systemd unit
# overrides YOLO_MODEL_PATH for the Pi. Inference itself is reused from
# trailcam/yolo_detector.py (incl. CLAHE IR/night handling) via detector.py.
_REPO_ROOT = Path(__file__).resolve().parent.parent
_CHICK_MODEL_DEFAULT = _REPO_ROOT / "training" / "chick-detector" / "weights" / "best.pt"
YOLO_MODEL_PATH: Path = Path(
    os.environ.get("YOLO_MODEL_PATH", _CHICK_MODEL_DEFAULT)
).expanduser()
YOLO_CONFIDENCE: float = float(os.environ.get("YOLO_CONFIDENCE", "0.5"))

# Where the reused trail-cam detector/uploader live. Its parent is added to
# sys.path and modules are imported as ``{name}.<module>`` so their internal
# ``from . import config`` resolves to the trail-cam config, not this package's
# (see detector.py).
TRAILCAM_DIR: Path = Path(
    os.environ.get("TRAILCAM_DIR", _REPO_ROOT / "trailcam")
).expanduser()

# --- Continuous capture / inference cadence --------------------------------
# Sample roughly one frame per second (NOT the stream's native ~15fps). The Pi
# can't run YOLO at 15fps and we don't need that temporal resolution for a
# headcount.
FRAME_INTERVAL: float = float(os.environ.get("FRAME_INTERVAL", "1.0"))

# --- Smart batching --------------------------------------------------------
# Counts are smoothed with a rolling median over the last SMOOTHING_WINDOW
# frames. We POST at most every POST_INTERVAL seconds, but POST immediately when
# the smoothed count moves by >= COUNT_CHANGE_THRESHOLD from the last posted
# value (so a real change isn't hidden for up to a minute).
SMOOTHING_WINDOW: int = int(os.environ.get("SMOOTHING_WINDOW", "5"))
POST_INTERVAL: int = int(os.environ.get("POST_INTERVAL", "60"))
COUNT_CHANGE_THRESHOLD: int = int(os.environ.get("COUNT_CHANGE_THRESHOLD", "2"))

# --- Image storage strategy ------------------------------------------------
# At ~1fps we'd generate ~86k frames/day — far too many to keep. The JSON
# observation (count, confidences, timestamp, inference time) is persisted every
# POST, but a frame is only written to disk when *notable*:
#   * the smoothed count moved (COUNT_CHANGE_THRESHOLD trigger),
#   * the frame's lowest detection confidence < LOW_CONFIDENCE_THRESHOLD (the
#     model is uncertain — valuable training data),
#   * the first frame after startup, or
#   * one "heartbeat" image every HEARTBEAT_IMAGE_INTERVAL seconds.
# Saved frames that upload to Roboflow successfully are deleted immediately; the
# rest are auto-pruned once past IMAGE_RETENTION_DAYS.
LOW_CONFIDENCE_THRESHOLD: float = float(os.environ.get("LOW_CONFIDENCE_THRESHOLD", "0.4"))
HEARTBEAT_IMAGE_INTERVAL: int = int(os.environ.get("HEARTBEAT_IMAGE_INTERVAL", "3600"))
IMAGE_RETENTION_DAYS: int = int(os.environ.get("IMAGE_RETENTION_DAYS", "7"))

# --- QuailSync server ------------------------------------------------------
QUAILSYNC_API_URL: str = os.environ.get(
    "QUAILSYNC_API_URL", "https://quailsync.tail01d133.ts.net"
)

# --- Roboflow active learning ----------------------------------------------
# After each POST cycle the frame is uploaded to Roboflow as a reviewable
# pre-label, on the same cadence as POSTs (every POST_INTERVAL or on a
# significant count change). Uses the SAME ROBOFLOW_API_KEY as the trail-cam
# pipeline (read from the env / secrets file), but a DIFFERENT project
# (find-chicks-5, not the trail cam's quail-detector). Best-effort: a missing
# key or SDK silently skips the upload — it never breaks the stream.
ROBOFLOW_UPLOAD_ENABLED: bool = _env_flag("ROBOFLOW_UPLOAD_ENABLED", "true")
ROBOFLOW_API_KEY: str | None = os.environ.get("ROBOFLOW_API_KEY")
ROBOFLOW_WORKSPACE: str = os.environ.get("ROBOFLOW_WORKSPACE", "quail")
ROBOFLOW_PROJECT: str = os.environ.get("ROBOFLOW_PROJECT", "find-chicks-5")
ROBOFLOW_BATCH_NAME: str = os.environ.get("ROBOFLOW_BATCH_NAME", "indoor-cam-auto")

# --- Legacy one-shot capture backend ---------------------------------------
# The continuous stream always uses OpenCV. CAPTURE_BACKEND only affects the
# legacy single-shot capture_frame() helper, retained for ad-hoc use/tests.
CAPTURE_BACKEND: str = os.environ.get("CAPTURE_BACKEND", "opencv").strip().lower()
FFMPEG_BIN: str = os.environ.get("FFMPEG_BIN", "ffmpeg")
CAPTURE_TIMEOUT: int = int(os.environ.get("CAPTURE_TIMEOUT", "30"))

# Every directory the pipeline writes into — the single source of truth for
# ``ensure_dirs()``.
_ALL_DIRS: tuple[Path, ...] = (CAPTURE_DIR, PROCESSED_DIR, MODELS_DIR)


def rtsp_url() -> str | None:
    """The resolved RTSP URL the poller should connect to.

    An explicit ``RTSP_URL`` wins; otherwise the URL is assembled from
    ``RTSP_USERNAME``/``RTSP_PASSWORD``/``RTSP_HOST``/``RTSP_PORT``/``RTSP_PATH``,
    percent-encoding the userinfo so a password with ``@``/``:`` stays valid.
    Returns ``None`` when neither a full URL nor a host is configured.
    """
    if RTSP_URL:
        return RTSP_URL
    if not RTSP_HOST:
        return None
    userinfo = ""
    if RTSP_USERNAME:
        userinfo = quote(RTSP_USERNAME, safe="")
        if RTSP_PASSWORD:
            userinfo += ":" + quote(RTSP_PASSWORD, safe="")
        userinfo += "@"
    path = RTSP_PATH if RTSP_PATH.startswith("/") else f"/{RTSP_PATH}"
    return f"rtsp://{userinfo}{RTSP_HOST}:{RTSP_PORT}{path}"


def redact_rtsp(url: str | None) -> str:
    """Mask any credentials in an RTSP URL for safe logging.

    ``rtsp://user:pass@host:554/stream1`` -> ``rtsp://***@host:554/stream1``.
    ``None`` becomes ``"<unset>"``.
    """
    if not url:
        return "<unset>"
    return re.sub(r"://[^@/]*@", "://***@", url)


def ensure_dirs() -> None:
    """Create the base directory and all working subdirectories if missing.

    Idempotent — safe to call on every startup.
    """
    for directory in _ALL_DIRS:
        directory.mkdir(parents=True, exist_ok=True)


if __name__ == "__main__":
    # Convenience: `python config.py` creates the directory tree and prints the
    # resolved configuration (RTSP credentials + Roboflow key are masked).
    ensure_dirs()
    print(f"BASE_DIR          = {BASE_DIR}")
    print(f"  capture/        = {CAPTURE_DIR}")
    print(f"  processed/      = {PROCESSED_DIR}")
    print(f"  models/         = {MODELS_DIR}")
    print(f"CAMERA_ID         = {CAMERA_ID}")
    print(f"RTSP_URL          = {redact_rtsp(rtsp_url())}")
    print(f"YOLO_MODEL_PATH   = {YOLO_MODEL_PATH}  (chick model)")
    print(f"YOLO_CONFIDENCE   = {YOLO_CONFIDENCE}")
    print(f"TRAILCAM_DIR      = {TRAILCAM_DIR}")
    print(f"FRAME_INTERVAL    = {FRAME_INTERVAL}s")
    print(f"POST_INTERVAL     = {POST_INTERVAL}s")
    print(f"COUNT_CHANGE_THRESHOLD = {COUNT_CHANGE_THRESHOLD}")
    print(f"SMOOTHING_WINDOW  = {SMOOTHING_WINDOW}")
    print(f"LOW_CONFIDENCE_THRESHOLD = {LOW_CONFIDENCE_THRESHOLD}")
    print(f"HEARTBEAT_IMAGE_INTERVAL = {HEARTBEAT_IMAGE_INTERVAL}s")
    print(f"IMAGE_RETENTION_DAYS = {IMAGE_RETENTION_DAYS}")
    print(f"QUAILSYNC_API_URL = {QUAILSYNC_API_URL}")
    print(f"ROBOFLOW_UPLOAD   = {'enabled' if ROBOFLOW_UPLOAD_ENABLED else 'disabled'}")
    print(f"ROBOFLOW_API_KEY  = {'<set>' if ROBOFLOW_API_KEY else '<unset>'}")
    print(f"ROBOFLOW_PROJECT  = {ROBOFLOW_WORKSPACE}/{ROBOFLOW_PROJECT}")
