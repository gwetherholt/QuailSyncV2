"""Tests for config.py — valid load, env-var camera resolution, bbox validation."""

import json

import pytest

import config as cfg


def _base_config():
    return {
        "camera": {
            "source_env": "INCUBATOR_RTSP_URL",
            "capture_interval_seconds": 10,
            "warmup_frames": 3,
        },
        "storage": {
            "db_path": "~/QuailSyncV2/data/quailsync.db",
            "captures_dir": "~/QuailSyncV2/incubator/captures",
            "save_crops_on_event": True,
            "sqlite_busy_timeout_ms": 5000,
        },
        "detection": {
            "baseline_alpha": 0.02,
            "high_threshold": 18.0,
            "low_threshold": 8.0,
            "cooldown_seconds": 120,
            "min_frames_before_detect": 5,
            "freeze_baseline_while_active": True,
            "blur_kernel": 5,
        },
        "tray": {
            "reference_image": "incubator/reference.jpg",
            "slots": [
                {"id": "A1", "bbox": [120, 80, 60, 60], "clutch_id": None},
                {"id": "A2", "bbox": [188, 80, 60, 60], "clutch_id": 7},
            ],
        },
    }


def _write(tmp_path, data):
    p = tmp_path / "config.json"
    p.write_text(json.dumps(data), encoding="utf-8")
    return p


def test_valid_load(tmp_path):
    path = _write(tmp_path, _base_config())
    conf = cfg.load_config(path, env={})

    assert conf.camera.source_env == "INCUBATOR_RTSP_URL"
    assert conf.camera.capture_interval_seconds == 10.0
    assert conf.camera.warmup_frames == 3
    assert conf.storage.save_crops_on_event is True
    assert conf.storage.sqlite_busy_timeout_ms == 5000
    # ~ is expanded.
    assert "~" not in str(conf.storage.db_path)
    assert conf.detection.high_threshold == 18.0
    assert conf.detection.low_threshold == 8.0
    assert conf.detection.blur_kernel == 5
    assert [s.id for s in conf.tray.slots] == ["A1", "A2"]
    assert conf.tray.slots[0].bbox == (120, 80, 60, 60)
    assert conf.tray.slots[0].clutch_id is None
    assert conf.tray.slots[1].clutch_id == 7
    assert conf.source_path == path


def test_shipped_config_json_is_valid():
    # The config.json that ships in the package must itself load + validate.
    conf = cfg.load_config(cfg.DEFAULT_CONFIG_PATH, env={})
    assert conf.tray.slots
    assert conf.camera.source is None  # env empty -> unresolved, not an error


def test_env_var_resolution(tmp_path):
    path = _write(tmp_path, _base_config())
    url = "rtsp://user:pass@10.0.0.5:554/stream1"  # pragma: allowlist secret
    conf = cfg.load_config(path, env={"INCUBATOR_RTSP_URL": url})
    assert conf.camera.source == url


def test_env_var_unset_leaves_source_none(tmp_path):
    path = _write(tmp_path, _base_config())
    conf = cfg.load_config(path, env={})
    assert conf.camera.source is None


def test_env_var_blank_is_treated_as_unset(tmp_path):
    path = _write(tmp_path, _base_config())
    conf = cfg.load_config(path, env={"INCUBATOR_RTSP_URL": "   "})
    assert conf.camera.source is None


def test_custom_source_env_name(tmp_path):
    data = _base_config()
    data["camera"]["source_env"] = "MY_CAM"
    path = _write(tmp_path, data)
    conf = cfg.load_config(path, env={"MY_CAM": "rtsp://host/s", "INCUBATOR_RTSP_URL": "ignored"})
    assert conf.camera.source == "rtsp://host/s"


@pytest.mark.parametrize(
    "bad_bbox",
    [
        [120, 80, 60],           # too few
        [120, 80, 60, 60, 10],   # too many
        [120, 80, 60, 0],        # zero height
        [120, 80, -5, 60],       # negative width
        [-1, 80, 60, 60],        # negative x
        [120, 80, 60, "60"],     # non-numeric
        [120, 80, 60, 60.5],     # non-integer
        "120,80,60,60",          # not a list
    ],
)
def test_rejects_malformed_bbox(tmp_path, bad_bbox):
    data = _base_config()
    data["tray"]["slots"][0]["bbox"] = bad_bbox
    path = _write(tmp_path, data)
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(path, env={})


def test_rejects_duplicate_slot_id(tmp_path):
    data = _base_config()
    data["tray"]["slots"][1]["id"] = "A1"
    path = _write(tmp_path, data)
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(path, env={})


def test_rejects_low_above_high_threshold(tmp_path):
    data = _base_config()
    data["detection"]["low_threshold"] = 25.0  # > high (18)
    path = _write(tmp_path, data)
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(path, env={})


def test_rejects_even_blur_kernel(tmp_path):
    data = _base_config()
    data["detection"]["blur_kernel"] = 4
    path = _write(tmp_path, data)
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(path, env={})


def test_rejects_alpha_out_of_range(tmp_path):
    data = _base_config()
    data["detection"]["baseline_alpha"] = 0.0
    path = _write(tmp_path, data)
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(path, env={})


def test_rejects_missing_section(tmp_path):
    data = _base_config()
    del data["detection"]
    path = _write(tmp_path, data)
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(path, env={})


def test_rejects_unparseable_json(tmp_path):
    p = tmp_path / "config.json"
    p.write_text("{not valid json", encoding="utf-8")
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(p, env={})


def test_missing_file_raises_config_error(tmp_path):
    with pytest.raises(cfg.ConfigError):
        cfg.load_config(tmp_path / "nope.json", env={})


def test_ensure_dirs_creates_captures_and_db_parent(tmp_path):
    data = _base_config()
    data["storage"]["db_path"] = str(tmp_path / "db" / "quailsync.db")
    data["storage"]["captures_dir"] = str(tmp_path / "caps")
    path = _write(tmp_path, data)
    conf = cfg.load_config(path, env={})
    cfg.ensure_dirs(conf)
    assert conf.storage.captures_dir.is_dir()
    assert conf.storage.db_path.parent.is_dir()
    cfg.ensure_dirs(conf)  # idempotent
