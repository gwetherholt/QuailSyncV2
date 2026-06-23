"""Tests for config.py — env-driven settings, RTSP URL assembly, redaction."""

import importlib

import config

# Every env var the config reads, cleared before a defaults test.
_ALL_KEYS = (
    "RTSP_URL", "RTSP_USERNAME", "RTSP_PASSWORD", "RTSP_HOST", "RTSP_PORT", "RTSP_PATH",
    "RTSP_TRANSPORT", "STREAM_RECONNECT_BACKOFF", "STREAM_MAX_BACKOFF",
    "CAPTURE_BACKEND", "FFMPEG_BIN", "CAPTURE_TIMEOUT",
    "INDOOR_CAMERA_ID", "INDOORCAM_BASE_DIR", "YOLO_MODEL_PATH", "YOLO_CONFIDENCE",
    "TRAILCAM_DIR", "QUAILSYNC_API_URL",
    "FRAME_INTERVAL", "POST_INTERVAL", "COUNT_CHANGE_THRESHOLD", "SMOOTHING_WINDOW",
    "LOW_CONFIDENCE_THRESHOLD", "HEARTBEAT_IMAGE_INTERVAL", "IMAGE_RETENTION_DAYS",
    "ROBOFLOW_UPLOAD_ENABLED", "ROBOFLOW_API_KEY", "ROBOFLOW_WORKSPACE",
    "ROBOFLOW_PROJECT", "ROBOFLOW_BATCH_NAME",
    "POLL_INTERVAL",  # removed — assert it's gone
)


def _reload_clean(monkeypatch, tmp_path, **overrides):
    for key in _ALL_KEYS:
        monkeypatch.delenv(key, raising=False)
    monkeypatch.setenv("INDOORCAM_BASE_DIR", str(tmp_path / "ic"))
    for k, v in overrides.items():
        monkeypatch.setenv(k, v)
    importlib.reload(config)
    return config


def test_continuous_mode_defaults(monkeypatch, tmp_path):
    cfg = _reload_clean(monkeypatch, tmp_path)
    # Continuous-mode cadence (POLL_INTERVAL is gone).
    assert not hasattr(cfg, "POLL_INTERVAL")
    assert cfg.FRAME_INTERVAL == 1.0
    assert cfg.POST_INTERVAL == 60
    assert cfg.COUNT_CHANGE_THRESHOLD == 2
    assert cfg.SMOOTHING_WINDOW == 5
    assert cfg.YOLO_CONFIDENCE == 0.5
    assert cfg.CAMERA_ID == "indoor-1"
    assert cfg.QUAILSYNC_API_URL == "https://quailsync.tail01d133.ts.net"
    assert cfg.rtsp_url() is None


def test_chick_model_is_the_default(monkeypatch, tmp_path):
    cfg = _reload_clean(monkeypatch, tmp_path)
    # Defaults to the ~6.2MB chick model, NOT the trail-cam model.
    assert cfg.YOLO_MODEL_PATH == cfg._CHICK_MODEL_DEFAULT
    assert cfg.YOLO_MODEL_PATH.as_posix().endswith("training/chick-detector/weights/best.pt")


def test_storage_strategy_defaults(monkeypatch, tmp_path):
    cfg = _reload_clean(monkeypatch, tmp_path)
    assert cfg.LOW_CONFIDENCE_THRESHOLD == 0.4
    assert cfg.HEARTBEAT_IMAGE_INTERVAL == 3600
    assert cfg.IMAGE_RETENTION_DAYS == 7


def test_roboflow_defaults(monkeypatch, tmp_path):
    cfg = _reload_clean(monkeypatch, tmp_path)
    assert cfg.ROBOFLOW_UPLOAD_ENABLED is True  # enabled by default
    assert cfg.ROBOFLOW_PROJECT == "find-chicks-5"  # NOT the trail cam's project
    assert cfg.ROBOFLOW_WORKSPACE == "quail"
    assert cfg.ROBOFLOW_API_KEY is None  # comes from the secrets file


def test_directory_layout_and_ensure_dirs(monkeypatch, tmp_path):
    base = tmp_path / "base"
    cfg = _reload_clean(monkeypatch, tmp_path, INDOORCAM_BASE_DIR=str(base))
    assert cfg.BASE_DIR == base
    assert cfg.CAPTURE_DIR == base / "capture"
    assert cfg.PROCESSED_DIR == base / "processed"
    assert cfg.MODELS_DIR == base / "models"

    assert not cfg.PROCESSED_DIR.exists()
    cfg.ensure_dirs()
    for d in (cfg.CAPTURE_DIR, cfg.PROCESSED_DIR, cfg.MODELS_DIR):
        assert d.is_dir()
    cfg.ensure_dirs()  # idempotent


def test_env_overrides(monkeypatch, tmp_path):
    cfg = _reload_clean(
        monkeypatch,
        tmp_path,
        FRAME_INTERVAL="0.5",
        POST_INTERVAL="30",
        COUNT_CHANGE_THRESHOLD="3",
        SMOOTHING_WINDOW="7",
        LOW_CONFIDENCE_THRESHOLD="0.25",
        HEARTBEAT_IMAGE_INTERVAL="900",
        IMAGE_RETENTION_DAYS="3",
        YOLO_CONFIDENCE="0.75",
        INDOOR_CAMERA_ID="brooder-cam-2",
        ROBOFLOW_UPLOAD_ENABLED="false",
        ROBOFLOW_PROJECT="other-proj",
        QUAILSYNC_API_URL="https://example.test/",
        YOLO_MODEL_PATH=str(tmp_path / "custom.pt"),
    )
    assert cfg.FRAME_INTERVAL == 0.5
    assert cfg.POST_INTERVAL == 30
    assert cfg.COUNT_CHANGE_THRESHOLD == 3
    assert cfg.SMOOTHING_WINDOW == 7
    assert cfg.LOW_CONFIDENCE_THRESHOLD == 0.25
    assert cfg.HEARTBEAT_IMAGE_INTERVAL == 900
    assert cfg.IMAGE_RETENTION_DAYS == 3
    assert cfg.YOLO_CONFIDENCE == 0.75
    assert cfg.CAMERA_ID == "brooder-cam-2"
    assert cfg.ROBOFLOW_UPLOAD_ENABLED is False
    assert cfg.ROBOFLOW_PROJECT == "other-proj"
    assert cfg.QUAILSYNC_API_URL == "https://example.test/"
    assert cfg.YOLO_MODEL_PATH == tmp_path / "custom.pt"


def test_rtsp_url_explicit_wins(monkeypatch, tmp_path):
    cfg = _reload_clean(
        monkeypatch,
        tmp_path,
        RTSP_URL="rtsp://u:p@10.0.0.9:554/h264",  # pragma: allowlist secret
        RTSP_HOST="ignored.example",
    )
    assert cfg.rtsp_url() == "rtsp://u:p@10.0.0.9:554/h264"  # pragma: allowlist secret


def test_rtsp_url_assembled_from_components(monkeypatch, tmp_path):
    cfg = _reload_clean(
        monkeypatch,
        tmp_path,
        RTSP_USERNAME="admin",
        RTSP_PASSWORD="p@ss:word",  # pragma: allowlist secret — special chars
        RTSP_HOST="192.0.2.10",  # RFC 5737 documentation IP — not a real device
        RTSP_PORT="554",
        RTSP_PATH="stream1",  # no leading slash -> added
    )
    assert cfg.rtsp_url() == "rtsp://admin:p%40ss%3Aword@192.0.2.10:554/stream1"


def test_redact_rtsp(monkeypatch, tmp_path):
    cfg = _reload_clean(monkeypatch, tmp_path)
    assert cfg.redact_rtsp("rtsp://user:secret@host:554/s1") == "rtsp://***@host:554/s1"
    assert cfg.redact_rtsp("rtsp://host:554/s1") == "rtsp://host:554/s1"
    assert cfg.redact_rtsp(None) == "<unset>"


def test_config_reload_to_defaults_for_other_tests(monkeypatch, tmp_path):
    # Leave the module clean so import-order doesn't leak env between files.
    _reload_clean(monkeypatch, tmp_path)
