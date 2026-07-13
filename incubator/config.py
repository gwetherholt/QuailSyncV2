"""Configuration for the stage-1 incubator capture pipeline.

Unlike the trail-cam / indoor-cam pipelines (which are configured entirely from
environment variables), the incubator pipeline is driven by a checked-in
``config.json`` describing the tray geometry (per-slot ROIs), the detector
thresholds, and where to write. That file is *static structure* — the one thing
that must NOT live in it is the camera address, which differs per install and is
a (mildly) sensitive URL. So ``camera.source`` is resolved at load time from the
environment variable named by ``camera.source_env`` (default
``INCUBATOR_RTSP_URL``), which the systemd unit loads from the out-of-repo
``~/.incubator-secrets`` file.

Usage::

    from incubator import config as cfg
    conf = cfg.load_config()             # reads incubator/config.json + env
    conf = cfg.load_config(path, env={}) # explicit path + injected environment

:func:`load_config` validates aggressively and raises :class:`ConfigError` with
an actionable message on anything malformed (bad bbox, duplicate slot id,
low > high threshold, even blur kernel, …) so a typo fails loudly at startup
rather than silently mis-detecting for a whole hatch.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Mapping

# The config.json shipped alongside this module is the default.
DEFAULT_CONFIG_PATH: Path = Path(__file__).resolve().parent / "config.json"

# Env var pointing at an alternate config file (systemd unit can override).
CONFIG_PATH_ENV = "INCUBATOR_CONFIG"


class ConfigError(ValueError):
    """Raised when config.json is missing, unparseable, or fails validation."""


@dataclass(frozen=True)
class CameraConfig:
    source_env: str
    capture_interval_seconds: float
    warmup_frames: int
    # Resolved from ``os.environ[source_env]`` at load time. ``None`` when the
    # env var is unset — the pipeline surfaces a clear "camera not configured"
    # error when it actually tries to connect, rather than failing to load.
    source: str | None = None


@dataclass(frozen=True)
class StorageConfig:
    db_path: Path
    captures_dir: Path
    save_crops_on_event: bool
    sqlite_busy_timeout_ms: int


@dataclass(frozen=True)
class DetectionConfig:
    baseline_alpha: float
    high_threshold: float
    low_threshold: float
    cooldown_seconds: float
    min_frames_before_detect: int
    freeze_baseline_while_active: bool
    blur_kernel: int


@dataclass(frozen=True)
class Slot:
    id: str
    # (x, y, w, h) in pixels, top-left origin.
    bbox: tuple[int, int, int, int]
    # Optional, static for now. A populated value is how per-slot identity gets
    # attached to the live clutches later; ``None`` is fine for stage 1.
    clutch_id: int | None = None


@dataclass(frozen=True)
class TrayConfig:
    reference_image: str
    slots: tuple[Slot, ...] = field(default_factory=tuple)


@dataclass(frozen=True)
class RoboflowConfig:
    # Auto-upload raw frames to Roboflow to build a labeling dataset. Best-effort
    # and opt-in: with ``enabled`` false, or the API key unset, uploads are
    # skipped silently and never break the pipeline (mirrors trail-cam/indoor-cam).
    enabled: bool
    project: str
    workspace: str
    upload_interval_seconds: float
    upload_on_event: bool
    api_key_env: str
    # Resolved from ``os.environ[api_key_env]`` at load time (like camera.source).
    # ``None`` when unset — the uploader is then a no-op.
    api_key: str | None = None


@dataclass(frozen=True)
class Config:
    camera: CameraConfig
    storage: StorageConfig
    detection: DetectionConfig
    tray: TrayConfig
    roboflow: RoboflowConfig
    # Absolute path the config was loaded from (handy for logging / define_rois).
    source_path: Path | None = None


# --- validation helpers ----------------------------------------------------


def _require(section: Mapping[str, Any], key: str, where: str) -> Any:
    if key not in section:
        raise ConfigError(f"{where}: missing required key {key!r}")
    return section[key]


def _as_number(value: Any, where: str) -> float:
    # bool is an int subclass; reject it so `true` isn't silently read as 1.
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ConfigError(f"{where}: expected a number, got {value!r}")
    return float(value)


def _as_int(value: Any, where: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ConfigError(f"{where}: expected an integer, got {value!r}")
    return value


def _as_bool(value: Any, where: str) -> bool:
    if not isinstance(value, bool):
        raise ConfigError(f"{where}: expected a boolean, got {value!r}")
    return value


def _parse_bbox(value: Any, where: str) -> tuple[int, int, int, int]:
    """Validate and coerce a ``[x, y, w, h]`` bbox in pixels.

    Rejects anything that isn't exactly four non-negative integers with a
    positive width and height — a malformed bbox would otherwise crop to an
    empty / wrapped-around region and silently break detection for that slot.
    """
    if not isinstance(value, (list, tuple)) or len(value) != 4:
        raise ConfigError(f"{where}: bbox must be a 4-element [x, y, w, h] list, got {value!r}")
    coords = []
    for name, component in zip(("x", "y", "w", "h"), value):
        if isinstance(component, bool) or not isinstance(component, int):
            raise ConfigError(f"{where}: bbox {name} must be an integer, got {component!r}")
        coords.append(component)
    x, y, w, h = coords
    if x < 0 or y < 0:
        raise ConfigError(f"{where}: bbox x/y must be >= 0, got x={x}, y={y}")
    if w <= 0 or h <= 0:
        raise ConfigError(f"{where}: bbox w/h must be > 0, got w={w}, h={h}")
    return (x, y, w, h)


def _parse_slot(raw: Any, index: int) -> Slot:
    where = f"tray.slots[{index}]"
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where}: each slot must be an object, got {raw!r}")
    slot_id = _require(raw, "id", where)
    if not isinstance(slot_id, str) or not slot_id.strip():
        raise ConfigError(f"{where}: slot id must be a non-empty string, got {slot_id!r}")
    bbox = _parse_bbox(_require(raw, "bbox", where), f"{where}({slot_id})")
    clutch_id = raw.get("clutch_id")
    if clutch_id is not None:
        clutch_id = _as_int(clutch_id, f"{where}({slot_id}).clutch_id")
    return Slot(id=slot_id, bbox=bbox, clutch_id=clutch_id)


# --- loaders ---------------------------------------------------------------


def _parse_camera(raw: Mapping[str, Any], env: Mapping[str, str]) -> CameraConfig:
    where = "camera"
    source_env = _require(raw, "source_env", where)
    if not isinstance(source_env, str) or not source_env.strip():
        raise ConfigError(f"{where}.source_env must be a non-empty string, got {source_env!r}")
    interval = _as_number(_require(raw, "capture_interval_seconds", where), f"{where}.capture_interval_seconds")
    if interval <= 0:
        raise ConfigError(f"{where}.capture_interval_seconds must be > 0, got {interval}")
    warmup = _as_int(raw.get("warmup_frames", 0), f"{where}.warmup_frames")
    if warmup < 0:
        raise ConfigError(f"{where}.warmup_frames must be >= 0, got {warmup}")
    # Resolve the camera source from the environment (never from the file).
    source = env.get(source_env)
    if source is not None:
        source = source.strip() or None
    return CameraConfig(
        source_env=source_env,
        capture_interval_seconds=interval,
        warmup_frames=warmup,
        source=source,
    )


def _parse_storage(raw: Mapping[str, Any]) -> StorageConfig:
    where = "storage"
    db_path = Path(str(_require(raw, "db_path", where))).expanduser()
    captures_dir = Path(str(_require(raw, "captures_dir", where))).expanduser()
    save_crops = _as_bool(raw.get("save_crops_on_event", True), f"{where}.save_crops_on_event")
    busy_timeout = _as_int(raw.get("sqlite_busy_timeout_ms", 5000), f"{where}.sqlite_busy_timeout_ms")
    if busy_timeout < 0:
        raise ConfigError(f"{where}.sqlite_busy_timeout_ms must be >= 0, got {busy_timeout}")
    return StorageConfig(
        db_path=db_path,
        captures_dir=captures_dir,
        save_crops_on_event=save_crops,
        sqlite_busy_timeout_ms=busy_timeout,
    )


def _parse_detection(raw: Mapping[str, Any]) -> DetectionConfig:
    where = "detection"
    alpha = _as_number(_require(raw, "baseline_alpha", where), f"{where}.baseline_alpha")
    if not 0.0 < alpha <= 1.0:
        raise ConfigError(f"{where}.baseline_alpha must be in (0, 1], got {alpha}")
    high = _as_number(_require(raw, "high_threshold", where), f"{where}.high_threshold")
    low = _as_number(_require(raw, "low_threshold", where), f"{where}.low_threshold")
    if low < 0 or high < 0:
        raise ConfigError(f"{where}: thresholds must be >= 0, got low={low}, high={high}")
    if low > high:
        raise ConfigError(
            f"{where}: low_threshold ({low}) must be <= high_threshold ({high}) "
            "for the hysteresis to make sense"
        )
    cooldown = _as_number(_require(raw, "cooldown_seconds", where), f"{where}.cooldown_seconds")
    if cooldown < 0:
        raise ConfigError(f"{where}.cooldown_seconds must be >= 0, got {cooldown}")
    min_frames = _as_int(raw.get("min_frames_before_detect", 0), f"{where}.min_frames_before_detect")
    if min_frames < 0:
        raise ConfigError(f"{where}.min_frames_before_detect must be >= 0, got {min_frames}")
    freeze = _as_bool(raw.get("freeze_baseline_while_active", True), f"{where}.freeze_baseline_while_active")
    blur = _as_int(raw.get("blur_kernel", 5), f"{where}.blur_kernel")
    if blur < 1 or blur % 2 == 0:
        raise ConfigError(f"{where}.blur_kernel must be a positive odd integer, got {blur}")
    return DetectionConfig(
        baseline_alpha=alpha,
        high_threshold=high,
        low_threshold=low,
        cooldown_seconds=cooldown,
        min_frames_before_detect=min_frames,
        freeze_baseline_while_active=freeze,
        blur_kernel=blur,
    )


def _parse_tray(raw: Mapping[str, Any]) -> TrayConfig:
    where = "tray"
    reference_image = str(raw.get("reference_image", "incubator/reference.jpg"))
    slots_raw = _require(raw, "slots", where)
    if not isinstance(slots_raw, (list, tuple)):
        raise ConfigError(f"{where}.slots must be a list, got {slots_raw!r}")
    slots = tuple(_parse_slot(slot, i) for i, slot in enumerate(slots_raw))
    seen: set[str] = set()
    for slot in slots:
        if slot.id in seen:
            raise ConfigError(f"{where}.slots: duplicate slot id {slot.id!r}")
        seen.add(slot.id)
    return TrayConfig(reference_image=reference_image, slots=slots)


def _parse_roboflow(raw: Any, env: Mapping[str, str]) -> RoboflowConfig:
    """Parse the optional ``roboflow`` section (defaults to disabled if absent).

    The API key is never in the file — it's resolved from the environment
    variable named by ``api_key_env`` (default ``ROBOFLOW_API_KEY``), which the
    systemd unit loads from ``~/.incubator-secrets``.
    """
    where = "roboflow"
    if raw is None:
        raw = {}
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    enabled = _as_bool(raw.get("enabled", False), f"{where}.enabled")
    project = str(raw.get("project", "incubation-stages"))
    workspace = str(raw.get("workspace", "quail"))
    interval = _as_number(
        raw.get("upload_interval_seconds", 1800), f"{where}.upload_interval_seconds"
    )
    if interval <= 0:
        raise ConfigError(f"{where}.upload_interval_seconds must be > 0, got {interval}")
    upload_on_event = _as_bool(raw.get("upload_on_event", True), f"{where}.upload_on_event")
    api_key_env = str(raw.get("api_key_env", "ROBOFLOW_API_KEY"))
    if not api_key_env.strip():
        raise ConfigError(f"{where}.api_key_env must be a non-empty string")
    api_key = env.get(api_key_env)
    if api_key is not None:
        api_key = api_key.strip() or None
    return RoboflowConfig(
        enabled=enabled,
        project=project,
        workspace=workspace,
        upload_interval_seconds=interval,
        upload_on_event=upload_on_event,
        api_key_env=api_key_env,
        api_key=api_key,
    )


def load_config(
    path: str | os.PathLike[str] | None = None,
    *,
    env: Mapping[str, str] | None = None,
) -> Config:
    """Load, validate, and return the pipeline configuration.

    ``path`` defaults to ``$INCUBATOR_CONFIG`` if set, else the ``config.json``
    shipped next to this module. ``env`` (defaulting to ``os.environ``) is where
    the camera source is resolved from — inject a dict in tests to avoid touching
    the real environment. Raises :class:`ConfigError` on any problem.
    """
    if env is None:
        env = os.environ
    if path is None:
        path = env.get(CONFIG_PATH_ENV) or DEFAULT_CONFIG_PATH
    path = Path(path).expanduser()

    try:
        raw_text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ConfigError(f"could not read config file {path}: {exc}") from exc
    try:
        data = json.loads(raw_text)
    except json.JSONDecodeError as exc:
        raise ConfigError(f"{path} is not valid JSON: {exc}") from exc
    if not isinstance(data, Mapping):
        raise ConfigError(f"{path}: top level must be a JSON object, got {type(data).__name__}")

    return Config(
        camera=_parse_camera(_require(data, "camera", "config"), env),
        storage=_parse_storage(_require(data, "storage", "config")),
        detection=_parse_detection(_require(data, "detection", "config")),
        tray=_parse_tray(_require(data, "tray", "config")),
        roboflow=_parse_roboflow(data.get("roboflow"), env),
        source_path=path,
    )


def ensure_dirs(conf: Config) -> None:
    """Create the directories the pipeline writes into (captures + the DB's
    parent). Idempotent — safe to call on every startup."""
    conf.storage.captures_dir.mkdir(parents=True, exist_ok=True)
    conf.storage.db_path.parent.mkdir(parents=True, exist_ok=True)


if __name__ == "__main__":
    # Convenience: `python config.py` loads + validates and prints the resolved
    # configuration (the camera source is masked — it may embed credentials).
    conf = load_config()
    cam = conf.camera
    print(f"config           = {conf.source_path}")
    print(f"camera.source_env= {cam.source_env}")
    print(f"camera.source    = {'<set>' if cam.source else '<unset>'}")
    print(f"capture_interval = {cam.capture_interval_seconds}s")
    print(f"warmup_frames    = {cam.warmup_frames}")
    print(f"db_path          = {conf.storage.db_path}")
    print(f"captures_dir     = {conf.storage.captures_dir}")
    print(f"save_crops       = {conf.storage.save_crops_on_event}")
    print(f"busy_timeout_ms  = {conf.storage.sqlite_busy_timeout_ms}")
    print(f"high/low thresh  = {conf.detection.high_threshold} / {conf.detection.low_threshold}")
    print(f"baseline_alpha   = {conf.detection.baseline_alpha}")
    print(f"cooldown         = {conf.detection.cooldown_seconds}s")
    print(f"min_frames       = {conf.detection.min_frames_before_detect}")
    print(f"freeze_active    = {conf.detection.freeze_baseline_while_active}")
    print(f"blur_kernel      = {conf.detection.blur_kernel}")
    print(f"slots            = {len(conf.tray.slots)}: {[s.id for s in conf.tray.slots]}")
    rf = conf.roboflow
    print(f"roboflow         = {'enabled' if rf.enabled else 'disabled'} -> {rf.workspace}/{rf.project}")
    print(f"  api_key ({rf.api_key_env}) = {'<set>' if rf.api_key else '<unset>'}")
    print(f"  upload_interval  = {rf.upload_interval_seconds}s, on_event={rf.upload_on_event}")
