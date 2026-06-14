"""Tests for config.py — env-driven settings and directory creation."""

import importlib

import config


def test_directory_layout(monkeypatch, tmp_path):
    base = tmp_path / "tc"
    monkeypatch.setenv("TRAILCAM_BASE_DIR", str(base))
    importlib.reload(config)

    assert config.BASE_DIR == base
    assert config.STAGING_DIR == base / "staging"
    assert config.PROCESSED_DIR == base / "processed"
    assert config.ARCHIVE_DIR == base / "archive"
    assert config.MODELS_DIR == base / "models"


def test_ensure_dirs_creates_tree(monkeypatch, tmp_path):
    monkeypatch.setenv("TRAILCAM_BASE_DIR", str(tmp_path / "base"))
    importlib.reload(config)

    assert not config.STAGING_DIR.exists()
    config.ensure_dirs()
    for directory in (
        config.STAGING_DIR,
        config.PROCESSED_DIR,
        config.ARCHIVE_DIR,
        config.MODELS_DIR,
    ):
        assert directory.is_dir()

    # Idempotent — a second call is a no-op, not an error.
    config.ensure_dirs()


def test_defaults(monkeypatch, tmp_path):
    for key in ("YOLO_CONFIDENCE", "POLL_INTERVAL", "PHOTO_LIMIT", "QUAILSYNC_API_URL", "YOLO_MODEL_PATH"):
        monkeypatch.delenv(key, raising=False)
    monkeypatch.setenv("TRAILCAM_BASE_DIR", str(tmp_path))
    importlib.reload(config)

    assert config.YOLO_CONFIDENCE == 0.5
    assert config.POLL_INTERVAL == 900
    assert config.PHOTO_LIMIT == 25
    assert config.QUAILSYNC_API_URL == "https://quailsync.tail01d133.ts.net"
    assert config.YOLO_MODEL_PATH == config.MODELS_DIR / "best.pt"


def test_env_overrides(monkeypatch, tmp_path):
    monkeypatch.setenv("TRAILCAM_BASE_DIR", str(tmp_path))
    monkeypatch.setenv("YOLO_CONFIDENCE", "0.75")
    monkeypatch.setenv("POLL_INTERVAL", "60")
    monkeypatch.setenv("PHOTO_LIMIT", "7")
    monkeypatch.setenv("QUAILSYNC_API_URL", "https://example.test/")
    monkeypatch.setenv("YOLO_MODEL_PATH", str(tmp_path / "custom.pt"))
    importlib.reload(config)

    assert config.YOLO_CONFIDENCE == 0.75
    assert config.POLL_INTERVAL == 60
    assert config.PHOTO_LIMIT == 7
    assert config.QUAILSYNC_API_URL == "https://example.test/"
    assert config.YOLO_MODEL_PATH == tmp_path / "custom.pt"


def test_credentials_from_env(monkeypatch, tmp_path):
    monkeypatch.setenv("TRAILCAM_BASE_DIR", str(tmp_path))
    monkeypatch.setenv("SPYPOINT_USERNAME", "alice")
    monkeypatch.setenv("SPYPOINT_PASSWORD", "s3cret")
    importlib.reload(config)

    assert config.SPYPOINT_USERNAME == "alice"
    assert config.SPYPOINT_PASSWORD == "s3cret"
