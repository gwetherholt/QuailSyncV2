"""Unified configuration for the assignment-aware indoor pipeline (stage 3).

This one pipeline supersedes both the incubator ``incubator/`` frame-diff
pipeline and the ``indoor-cam/`` YOLO pipeline. Which YOLO model it runs is not
baked into the config — it's chosen at runtime from the backend camera
*assignment* (see :mod:`assignment`). So ``config.json`` describes *both* modes
up front (``models.incubation`` and ``models.chick``) plus how to poll the
backend, and the live loop picks the active one.

Like the incubator pipeline, the one thing that must NOT live in ``config.json``
is the camera address (a mildly sensitive, per-install URL) and the Roboflow API
key. Both are resolved at load time from the environment variables named by
``camera.source_env`` (default ``INDOOR_RTSP_URL``) and ``roboflow.api_key_env``
(default ``ROBOFLOW_API_KEY``), which the systemd unit loads from the out-of-repo
``~/.indoor-pipeline-secrets`` file.

Usage::

    from indoor_pipeline import config as cfg
    conf = cfg.load_config()               # reads indoor-pipeline/config.json + env
    conf = cfg.load_config(path, env={})   # explicit path + injected environment

:func:`load_config` validates aggressively and raises :class:`ConfigError` with
an actionable message on anything malformed so a typo fails loudly at startup.
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
CONFIG_PATH_ENV = "INDOOR_PIPELINE_CONFIG"

# Roboflow upload-frequency controls, resolved from the environment (non-secret;
# typically set via the systemd unit's Environment= lines). The on-detection
# trigger defaults OFF; the spacing floor defaults to 1800s.
ROBOFLOW_UPLOAD_ON_DETECTION_ENV = "ROBOFLOW_UPLOAD_ON_DETECTION"
ROBOFLOW_MIN_UPLOAD_SPACING_ENV = "ROBOFLOW_MIN_UPLOAD_SPACING_S"
DEFAULT_MIN_UPLOAD_SPACING_S = 1800.0

# --- Assignment → model mapping --------------------------------------------
# The backend's GET /api/cameras/{id}/assignment returns ``active_model`` already
# derived from the assignment (mirrors ``active_model_for()`` in quailsync-common:
# incubator → "incubation", brooder → "chick"). We key ``models`` by that derived
# model name. ``resolve_mode`` also accepts the raw assignment names so a config
# ``default_mode`` of "incubator"/"brooder" (or a direct "incubation"/"chick")
# both work.
MODE_INCUBATION = "incubation"
MODE_CHICK = "chick"
VALID_MODES: tuple[str, ...] = (MODE_INCUBATION, MODE_CHICK)

# Raw backend assignment value -> derived model name. Keep in lockstep with
# quailsync-common::active_model_for.
ASSIGNMENT_TO_MODEL: dict[str, str] = {
    "incubator": MODE_INCUBATION,
    "brooder": MODE_CHICK,
}


def resolve_mode(value: str | None) -> str | None:
    """Resolve a mode name from an ``active_model`` or a raw assignment value.

    Returns the canonical model key (``"incubation"``/``"chick"``) for either a
    model name or an assignment name, or ``None`` for anything unrecognized (so
    callers can keep their last-known mode instead of switching to garbage).
    """
    if value is None:
        return None
    value = value.strip()
    if value in VALID_MODES:
        return value
    return ASSIGNMENT_TO_MODEL.get(value)


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
class AssignmentConfig:
    backend_url: str
    camera_id: str
    poll_seconds: float
    # The mode used before the first successful poll, and whenever the backend is
    # unreachable and nothing has ever been fetched. May be an assignment name
    # ("incubator"/"brooder") or a model name ("incubation"/"chick"); it is
    # resolved to a canonical model key at load time.
    default_mode: str


@dataclass(frozen=True)
class ModelConfig:
    # The mode key this config is registered under ("incubation"/"chick").
    mode: str
    weights: Path
    confidence: float
    roboflow_project: str
    log_events: bool


@dataclass(frozen=True)
class RoboflowConfig:
    # Auto-upload frames to Roboflow to grow the labeling dataset. Best-effort and
    # opt-in: with ``enabled`` false, or the API key unset, uploads are skipped
    # silently and never break the pipeline. The *project* is per-mode (see
    # :class:`ModelConfig`); the workspace/key/batch are shared here.
    #
    # Frequency: a periodic ``upload_interval_seconds`` upload, plus optional
    # on-detection uploads. ``upload_on_detection`` is ENV-driven (default OFF).
    # ``min_upload_spacing_s`` is a hard floor between ANY two uploads (env-driven,
    # default 1800s) so even if on-detection is re-enabled it can't flood Roboflow.
    enabled: bool
    workspace: str
    upload_interval_seconds: float
    # Resolved from env ``ROBOFLOW_UPLOAD_ON_DETECTION`` at load time; default False.
    upload_on_detection: bool
    # Resolved from env ``ROBOFLOW_MIN_UPLOAD_SPACING_S`` at load time; default 1800.
    min_upload_spacing_s: float
    api_key_env: str
    batch_name: str
    # Resolved from ``os.environ[api_key_env]`` at load time (like camera.source).
    # ``None`` when unset — the uploader is then a no-op.
    api_key: str | None = None


@dataclass(frozen=True)
class StorageConfig:
    db_path: Path
    sqlite_busy_timeout_ms: int


@dataclass(frozen=True)
class SnapshotsConfig:
    # Rolling "latest" frames the backend/app serve as the live feed. The raw
    # frame and its YOLO-annotated copy are overwritten every cycle (atomically)
    # so a reader never sees a half-written image. These paths must match where
    # the backend reads indoor-cam images: {INDOORCAM_PROCESSED_DIR}/{camera_id}/
    # latest.jpg and latest_annotated.jpg.
    latest_path: Path
    latest_annotated_path: Path


@dataclass(frozen=True)
class ObservationsConfig:
    # POST one observation per cycle to the backend so the dashboard/app show a
    # live detection count + image (mirrors the old indoor-cam bridge). The
    # ``camera_id`` here is the OBSERVATION/serving id the backend, dashboard, and
    # app key on (``indoor-1``) — deliberately DIFFERENT from the assignment
    # ``camera_id`` (``indoor_tapo``), which only drives the mode toggle.
    enabled: bool
    backend_url: str
    camera_id: str


@dataclass(frozen=True)
class Config:
    camera: CameraConfig
    assignment: AssignmentConfig
    models: dict[str, ModelConfig]
    roboflow: RoboflowConfig
    storage: StorageConfig
    # Optional rolling-snapshot output. ``None`` disables snapshot writing.
    snapshots: SnapshotsConfig | None = None
    # Optional observation POSTing. ``None`` disables it.
    observations: ObservationsConfig | None = None
    # Absolute path the config was loaded from (handy for logging).
    source_path: Path | None = None

    def model_for(self, mode: str) -> ModelConfig | None:
        """Return the :class:`ModelConfig` for a resolved mode, or ``None``."""
        return self.models.get(mode)


# --- validation helpers ----------------------------------------------------


def _require(section: Mapping[str, Any], key: str, where: str) -> Any:
    if key not in section:
        raise ConfigError(f"{where}: missing required key {key!r}")
    return section[key]


def _env_flag(env: Mapping[str, str], name: str, *, default: bool = False) -> bool:
    """Parse a boolean env var (``1/true/yes/on`` = True). Unset -> ``default``."""
    raw = env.get(name)
    if raw is None:
        return default
    return raw.strip().lower() in ("1", "true", "yes", "on")


def _env_number(env: Mapping[str, str], name: str, default: float, where: str) -> float:
    """Parse a numeric env var. Unset/blank -> ``default``; non-numeric ->
    :class:`ConfigError` (fail loud at startup, like the rest of the config)."""
    raw = env.get(name)
    if raw is None or not raw.strip():
        return float(default)
    try:
        return float(raw.strip())
    except ValueError as exc:
        raise ConfigError(f"{where}: {name}={raw!r} is not a number") from exc


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


def _as_nonempty_str(value: Any, where: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ConfigError(f"{where}: expected a non-empty string, got {value!r}")
    return value


# --- section loaders -------------------------------------------------------


def _parse_camera(raw: Mapping[str, Any], env: Mapping[str, str]) -> CameraConfig:
    where = "camera"
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    source_env = _as_nonempty_str(_require(raw, "source_env", where), f"{where}.source_env")
    interval = _as_number(
        _require(raw, "capture_interval_seconds", where), f"{where}.capture_interval_seconds"
    )
    if interval <= 0:
        raise ConfigError(f"{where}.capture_interval_seconds must be > 0, got {interval}")
    warmup = _as_int(raw.get("warmup_frames", 3), f"{where}.warmup_frames")
    if warmup < 0:
        raise ConfigError(f"{where}.warmup_frames must be >= 0, got {warmup}")
    source = env.get(source_env)
    if source is not None:
        source = source.strip() or None
    return CameraConfig(
        source_env=source_env,
        capture_interval_seconds=interval,
        warmup_frames=warmup,
        source=source,
    )


def _parse_assignment(raw: Mapping[str, Any]) -> AssignmentConfig:
    where = "assignment"
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    backend_url = _as_nonempty_str(
        _require(raw, "backend_url", where), f"{where}.backend_url"
    ).rstrip("/")
    camera_id = _as_nonempty_str(_require(raw, "camera_id", where), f"{where}.camera_id")
    poll_seconds = _as_number(raw.get("poll_seconds", 60), f"{where}.poll_seconds")
    if poll_seconds <= 0:
        raise ConfigError(f"{where}.poll_seconds must be > 0, got {poll_seconds}")
    default_mode = _as_nonempty_str(
        raw.get("default_mode", "incubator"), f"{where}.default_mode"
    )
    if resolve_mode(default_mode) is None:
        raise ConfigError(
            f"{where}.default_mode {default_mode!r} is not a valid mode — expected one "
            f"of {VALID_MODES} or an assignment name ({sorted(ASSIGNMENT_TO_MODEL)})"
        )
    return AssignmentConfig(
        backend_url=backend_url,
        camera_id=camera_id,
        poll_seconds=poll_seconds,
        default_mode=default_mode,
    )


def _parse_model(mode: str, raw: Any) -> ModelConfig:
    where = f"models.{mode}"
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    weights = Path(str(_require(raw, "weights", where))).expanduser()
    confidence = _as_number(raw.get("confidence", 0.5), f"{where}.confidence")
    if not 0.0 < confidence <= 1.0:
        raise ConfigError(f"{where}.confidence must be in (0, 1], got {confidence}")
    roboflow_project = _as_nonempty_str(
        _require(raw, "roboflow_project", where), f"{where}.roboflow_project"
    )
    log_events = _as_bool(raw.get("log_events", False), f"{where}.log_events")
    return ModelConfig(
        mode=mode,
        weights=weights,
        confidence=confidence,
        roboflow_project=roboflow_project,
        log_events=log_events,
    )


def _parse_models(raw: Any) -> dict[str, ModelConfig]:
    where = "models"
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    models: dict[str, ModelConfig] = {}
    for mode, model_raw in raw.items():
        if mode not in VALID_MODES:
            raise ConfigError(
                f"{where}.{mode}: unknown mode {mode!r} — expected one of {VALID_MODES}"
            )
        models[mode] = _parse_model(mode, model_raw)
    for mode in VALID_MODES:
        if mode not in models:
            raise ConfigError(f"{where}: missing required mode {mode!r}")
    return models


def _parse_roboflow(raw: Any, env: Mapping[str, str]) -> RoboflowConfig:
    where = "roboflow"
    if raw is None:
        raw = {}
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    enabled = _as_bool(raw.get("enabled", False), f"{where}.enabled")
    workspace = str(raw.get("workspace", "quail"))
    interval = _as_number(
        raw.get("upload_interval_seconds", 1800), f"{where}.upload_interval_seconds"
    )
    if interval <= 0:
        raise ConfigError(f"{where}.upload_interval_seconds must be > 0, got {interval}")
    # On-detection trigger is ENV-driven and OFF by default, so it can be flipped
    # per-deploy without editing config.json.
    upload_on_detection = _env_flag(env, ROBOFLOW_UPLOAD_ON_DETECTION_ENV, default=False)
    # Hard floor between ANY two uploads (env-driven, default 1800s) — an anti-flood
    # cap independent of the trigger.
    min_upload_spacing_s = _env_number(
        env, ROBOFLOW_MIN_UPLOAD_SPACING_ENV, DEFAULT_MIN_UPLOAD_SPACING_S,
        f"{where}.{ROBOFLOW_MIN_UPLOAD_SPACING_ENV}",
    )
    if min_upload_spacing_s < 0:
        raise ConfigError(
            f"{where}: {ROBOFLOW_MIN_UPLOAD_SPACING_ENV} must be >= 0, got {min_upload_spacing_s}"
        )
    api_key_env = _as_nonempty_str(
        raw.get("api_key_env", "ROBOFLOW_API_KEY"), f"{where}.api_key_env"
    )
    batch_name = str(raw.get("batch_name", "indoor-auto"))
    api_key = env.get(api_key_env)
    if api_key is not None:
        api_key = api_key.strip() or None
    return RoboflowConfig(
        enabled=enabled,
        workspace=workspace,
        upload_interval_seconds=interval,
        upload_on_detection=upload_on_detection,
        min_upload_spacing_s=min_upload_spacing_s,
        api_key_env=api_key_env,
        batch_name=batch_name,
        api_key=api_key,
    )


def _parse_storage(raw: Mapping[str, Any]) -> StorageConfig:
    where = "storage"
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    db_path = Path(str(_require(raw, "db_path", where))).expanduser()
    busy_timeout = _as_int(raw.get("sqlite_busy_timeout_ms", 5000), f"{where}.sqlite_busy_timeout_ms")
    if busy_timeout < 0:
        raise ConfigError(f"{where}.sqlite_busy_timeout_ms must be >= 0, got {busy_timeout}")
    return StorageConfig(db_path=db_path, sqlite_busy_timeout_ms=busy_timeout)


def _parse_snapshots(raw: Any) -> SnapshotsConfig | None:
    """Parse the optional ``snapshots`` section (``None`` when absent → disabled).

    When present, both paths are required. They should match the backend's
    indoor-cam serving path so the live feed works without backend changes.
    """
    where = "snapshots"
    if raw is None:
        return None
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    latest_path = Path(
        _as_nonempty_str(_require(raw, "latest_path", where), f"{where}.latest_path")
    ).expanduser()
    latest_annotated_path = Path(
        _as_nonempty_str(_require(raw, "latest_annotated_path", where), f"{where}.latest_annotated_path")
    ).expanduser()
    return SnapshotsConfig(latest_path=latest_path, latest_annotated_path=latest_annotated_path)


def _parse_observations(raw: Any) -> ObservationsConfig | None:
    """Parse the optional ``observations`` section (``None`` when absent → off).

    When present, ``backend_url`` and ``camera_id`` are required. ``camera_id`` is
    the observation/serving id (``indoor-1``) the backend/dashboard/app key on —
    distinct from the assignment ``camera_id``.
    """
    where = "observations"
    if raw is None:
        return None
    if not isinstance(raw, Mapping):
        raise ConfigError(f"{where} must be an object, got {raw!r}")
    enabled = _as_bool(raw.get("enabled", True), f"{where}.enabled")
    backend_url = _as_nonempty_str(
        _require(raw, "backend_url", where), f"{where}.backend_url"
    ).rstrip("/")
    camera_id = _as_nonempty_str(_require(raw, "camera_id", where), f"{where}.camera_id")
    return ObservationsConfig(enabled=enabled, backend_url=backend_url, camera_id=camera_id)


def load_config(
    path: str | os.PathLike[str] | None = None,
    *,
    env: Mapping[str, str] | None = None,
) -> Config:
    """Load, validate, and return the pipeline configuration.

    ``path`` defaults to ``$INDOOR_PIPELINE_CONFIG`` if set, else the
    ``config.json`` shipped next to this module. ``env`` (defaulting to
    ``os.environ``) is where the camera source and Roboflow key are resolved
    from — inject a dict in tests to avoid touching the real environment. Raises
    :class:`ConfigError` on any problem.
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
        assignment=_parse_assignment(_require(data, "assignment", "config")),
        models=_parse_models(_require(data, "models", "config")),
        roboflow=_parse_roboflow(data.get("roboflow"), env),
        storage=_parse_storage(_require(data, "storage", "config")),
        snapshots=_parse_snapshots(data.get("snapshots")),
        observations=_parse_observations(data.get("observations")),
        source_path=path,
    )


def ensure_dirs(conf: Config) -> None:
    """Create the directories the pipeline writes into (the DB's parent, and the
    snapshot dirs). Idempotent — safe to call on every startup."""
    conf.storage.db_path.parent.mkdir(parents=True, exist_ok=True)
    if conf.snapshots is not None:
        conf.snapshots.latest_path.parent.mkdir(parents=True, exist_ok=True)
        conf.snapshots.latest_annotated_path.parent.mkdir(parents=True, exist_ok=True)


if __name__ == "__main__":
    # Convenience: `python config.py` loads + validates and prints the resolved
    # configuration (the camera source is masked — it may embed credentials).
    conf = load_config()
    cam = conf.camera
    asg = conf.assignment
    rf = conf.roboflow
    print(f"config            = {conf.source_path}")
    print(f"camera.source_env = {cam.source_env}")
    print(f"camera.source     = {'<set>' if cam.source else '<unset>'}")
    print(f"capture_interval  = {cam.capture_interval_seconds}s")
    print(f"warmup_frames     = {cam.warmup_frames}")
    print(f"backend_url       = {asg.backend_url}")
    print(f"camera_id         = {asg.camera_id}")
    print(f"poll_seconds      = {asg.poll_seconds}")
    print(f"default_mode      = {asg.default_mode} -> {resolve_mode(asg.default_mode)}")
    for mode, m in conf.models.items():
        print(f"model[{mode}]      = {m.weights} conf={m.confidence} "
              f"project={m.roboflow_project} log_events={m.log_events}")
    print(f"roboflow          = {'enabled' if rf.enabled else 'disabled'} ws={rf.workspace}")
    print(f"  api_key ({rf.api_key_env}) = {'<set>' if rf.api_key else '<unset>'}")
    print(f"  upload_interval   = {rf.upload_interval_seconds}s, on_detection={rf.upload_on_detection}"
          f", min_spacing={rf.min_upload_spacing_s}s")
    print(f"db_path           = {conf.storage.db_path}")
    print(f"busy_timeout_ms   = {conf.storage.sqlite_busy_timeout_ms}")
    if conf.snapshots is not None:
        print(f"snapshot latest   = {conf.snapshots.latest_path}")
        print(f"snapshot annot.   = {conf.snapshots.latest_annotated_path}")
    else:
        print("snapshots         = disabled")
    if conf.observations is not None:
        obs = conf.observations
        print(f"observations      = {'enabled' if obs.enabled else 'disabled'} "
              f"-> {obs.backend_url}/api/indoorcam/observation (camera_id={obs.camera_id})")
    else:
        print("observations      = disabled")
