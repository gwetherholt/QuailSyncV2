"""QuailSync Govee sensor poller.

Polls the Govee cloud API for the H5179 (and sibling thermo-hygrometer) WiFi
temp/humidity sensors and POSTs their readings to the QuailSync server at
``POST /api/govee/readings``. Runs on the Raspberry Pi alongside QuailSync,
managed by systemd (see ``govee-poller.service``).

Mirrors the trailcam module: a single-file Python service that polls an
external API on a timer, logs to stdout (captured by the journal), and never
lets a transient error kill the loop.

Each cycle:
  1. GET https://developer-api.govee.com/v1/devices  — list devices
  2. For each thermo-hygrometer, GET its state for temperature + humidity
  3. Build the readings payload and POST it to QuailSync
  4. Log the outcome, sleep, repeat

Run::

    python govee_poller.py                # loop forever using ./config.yaml
    python govee_poller.py --once         # a single cycle (handy for testing)
    python govee_poller.py --config /etc/quailsync/govee.yaml
"""

from __future__ import annotations

import argparse
import logging
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests
import yaml

logger = logging.getLogger("govee_poller")

# --- Govee cloud API -------------------------------------------------------
GOVEE_API_BASE = "https://developer-api.govee.com"
DEVICES_URL = f"{GOVEE_API_BASE}/v1/devices"
DEVICE_STATE_URL = f"{GOVEE_API_BASE}/v1/devices/state"

# HTTP timeout for every outbound request (connect + read), in seconds.
REQUEST_TIMEOUT = 30
# Small pause between per-device state calls so a cabinet full of sensors can't
# burst past Govee's per-minute rate limit. Daily volume is tiny (~600/day at a
# 5-min interval) so this is purely about not bunching requests.
INTER_REQUEST_DELAY = 1.0

# Govee thermo-hygrometer models that report temperature + humidity. H5179 is
# ours; the rest are common siblings so the poller picks them up too without a
# code change. Matched case-insensitively.
THERMO_HYGROMETER_MODELS = {
    "H5179",
    "H5100", "H5101", "H5102", "H5103", "H5104", "H5105",
    "H5174", "H5177", "H5198",
    "H5051", "H5052", "H5071", "H5072", "H5074", "H5075",
}


def is_thermo_hygrometer(model: str | None) -> bool:
    """True if ``model`` is a known temp/humidity sensor."""
    return bool(model) and model.strip().upper() in THERMO_HYGROMETER_MODELS


# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------


class ConfigError(Exception):
    """Raised when config.yaml is missing required, non-placeholder values."""


def load_config(path: Path | str) -> dict:
    """Load and validate config.yaml.

    The API key may instead be supplied via the ``GOVEE_API_KEY`` environment
    variable (which wins), so the secret can be kept out of the file. Raises
    :class:`ConfigError` if no usable API key is configured.
    """
    path = Path(path)
    try:
        with open(path, "r", encoding="utf-8") as fh:
            raw = yaml.safe_load(fh) or {}
    except FileNotFoundError as exc:
        raise ConfigError(f"config file not found: {path}") from exc
    except yaml.YAMLError as exc:
        raise ConfigError(f"config file is not valid YAML: {exc}") from exc

    api_key = (os.environ.get("GOVEE_API_KEY") or raw.get("govee_api_key") or "").strip()
    if not api_key or api_key == "YOUR_API_KEY_HERE":
        raise ConfigError(
            "Govee API key not set — put it in config.yaml (govee_api_key) "
            "or the GOVEE_API_KEY environment variable."
        )

    quailsync_api_url = str(raw.get("quailsync_api_url") or "http://localhost:3000").rstrip("/")

    try:
        poll_interval_seconds = int(raw.get("poll_interval_seconds", 300))
    except (TypeError, ValueError):
        poll_interval_seconds = 300
    poll_interval_seconds = max(1, poll_interval_seconds)

    temperature_unit = str(raw.get("temperature_unit", "F")).strip().upper()
    if temperature_unit not in ("F", "C"):
        logger.warning("Unknown temperature_unit %r — defaulting to F", temperature_unit)
        temperature_unit = "F"

    return {
        "govee_api_key": api_key,
        "quailsync_api_url": quailsync_api_url,
        "poll_interval_seconds": poll_interval_seconds,
        "temperature_unit": temperature_unit,
    }


# ---------------------------------------------------------------------------
# Govee API access
# ---------------------------------------------------------------------------


def _log_rate_limit(resp: requests.Response) -> None:
    """Surface Govee's rate-limit headers at debug level, if present."""
    remaining = resp.headers.get("API-RateLimit-Remaining") or resp.headers.get(
        "Rate-Limit-Remaining"
    )
    if remaining is not None:
        logger.debug("Govee rate-limit remaining: %s", remaining)


def get_devices(session: requests.Session) -> list[dict]:
    """Return the raw device list from ``GET /v1/devices``.

    Raises on transport/HTTP errors (incl. 429); the caller logs and retries
    next cycle.
    """
    resp = session.get(DEVICES_URL, timeout=REQUEST_TIMEOUT)
    _log_rate_limit(resp)
    if resp.status_code == 429:
        retry_after = resp.headers.get("Retry-After", "unknown")
        logger.warning("Govee rate limit hit on /devices (Retry-After=%s)", retry_after)
    resp.raise_for_status()
    return resp.json().get("data", {}).get("devices", []) or []


def get_device_state(session: requests.Session, device: str, model: str) -> dict:
    """Return the raw state payload for one device from ``GET /v1/devices/state``."""
    resp = session.get(
        DEVICE_STATE_URL,
        params={"device": device, "model": model},
        timeout=REQUEST_TIMEOUT,
    )
    _log_rate_limit(resp)
    if resp.status_code == 429:
        retry_after = resp.headers.get("Retry-After", "unknown")
        logger.warning(
            "Govee rate limit hit on /devices/state for %s (Retry-After=%s)",
            device,
            retry_after,
        )
    resp.raise_for_status()
    return resp.json()


def _to_float(value) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def extract_reading(state_data: dict) -> tuple[float, float] | None:
    """Pull ``(temperature, humidity)`` from a device-state payload.

    Govee returns ``data.properties`` as a list of single-key dicts, e.g.
    ``[{"online": true}, {"temperature": 78.5}, {"humidity": 45.2}]``. We merge
    them and take the first numeric temperature-ish / humidity-ish value.
    Returns ``None`` if either is missing.
    """
    properties = (state_data or {}).get("data", {}).get("properties", []) or []
    merged: dict = {}
    for prop in properties:
        if isinstance(prop, dict):
            merged.update(prop)

    temperature = None
    humidity = None
    for key, value in merged.items():
        key_lower = str(key).lower()
        if key_lower in ("online", "battery"):
            continue
        num = _to_float(value)
        if num is None:
            continue
        if temperature is None and "tem" in key_lower:
            temperature = num
        elif humidity is None and "hum" in key_lower:
            humidity = num

    if temperature is None or humidity is None:
        return None
    return temperature, humidity


# ---------------------------------------------------------------------------
# Reading assembly + delivery
# ---------------------------------------------------------------------------


def _now_iso() -> str:
    """Current UTC time as RFC 3339 with a trailing Z, e.g. 2025-06-17T12:00:00Z."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def build_readings(
    session: requests.Session,
    devices: list[dict],
    temperature_unit: str,
) -> list[dict]:
    """Query each thermo-hygrometer's state and build the QuailSync payload list.

    A failure on one device is logged and skipped — the rest of the batch still
    goes through.
    """
    readings: list[dict] = []
    sensors = [d for d in devices if is_thermo_hygrometer(d.get("model"))]
    logger.info(
        "Found %d device(s); %d thermo-hygrometer sensor(s) to poll",
        len(devices),
        len(sensors),
    )

    for index, device in enumerate(sensors):
        device_id = device.get("device")
        model = device.get("model")
        name = device.get("deviceName")
        if not device_id or not model:
            logger.warning("Skipping device with missing device/model: %r", device)
            continue

        if index > 0:
            time.sleep(INTER_REQUEST_DELAY)  # be gentle with Govee's rate limit

        try:
            state = get_device_state(session, device_id, model)
        except requests.RequestException as exc:
            logger.error("Failed to read state for %s (%s): %s", name or device_id, model, exc)
            continue

        parsed = extract_reading(state)
        if parsed is None:
            logger.warning(
                "No temperature/humidity in state for %s (%s) — skipping",
                name or device_id,
                model,
            )
            continue

        temperature, humidity = parsed
        if temperature_unit == "C":
            temperature_f = round(temperature * 9 / 5 + 32, 2)
        else:
            temperature_f = round(temperature, 2)

        readings.append(
            {
                "device_id": device_id,
                "model": model,
                "name": name,
                "temperature_f": temperature_f,
                "humidity": round(humidity, 2),
                "recorded_at": _now_iso(),
            }
        )
        logger.info(
            "Read %s (%s): %.1f°F, %.1f%% RH",
            name or device_id,
            model,
            temperature_f,
            humidity,
        )

    return readings


def post_readings(session: requests.Session, quailsync_api_url: str, readings: list[dict]) -> bool:
    """POST the batch to QuailSync. Returns True on success.

    Network / HTTP errors are logged (not raised) so the loop survives a
    QuailSync restart or a transient blip and just retries next cycle.
    """
    url = f"{quailsync_api_url}/api/govee/readings"
    try:
        resp = session.post(url, json={"readings": readings}, timeout=REQUEST_TIMEOUT)
        resp.raise_for_status()
    except requests.RequestException as exc:
        logger.error("Failed to POST %d reading(s) to QuailSync: %s", len(readings), exc)
        return False

    try:
        stored = resp.json().get("stored")
    except ValueError:
        stored = None
    logger.info("Posted %d reading(s) to QuailSync (stored=%s)", len(readings), stored)
    return True


# ---------------------------------------------------------------------------
# Poll cycle + main loop
# ---------------------------------------------------------------------------


def poll_once(
    config: dict,
    govee_session: requests.Session,
    quailsync_session: requests.Session,
) -> None:
    """Run a single poll cycle. Never raises — all errors are logged."""
    try:
        devices = get_devices(govee_session)
    except requests.RequestException as exc:
        logger.error("Failed to list Govee devices: %s — retrying next cycle", exc)
        return
    except ValueError as exc:  # bad JSON
        logger.error("Govee /devices returned invalid JSON: %s", exc)
        return

    try:
        readings = build_readings(govee_session, devices, config["temperature_unit"])
    except Exception as exc:  # noqa: BLE001 — one bad cycle must not kill the loop
        logger.exception("Unexpected error while building readings: %s", exc)
        return

    if not readings:
        logger.info("No sensor readings collected this cycle — nothing to post")
        return

    post_readings(quailsync_session, config["quailsync_api_url"], readings)


def _make_govee_session(api_key: str) -> requests.Session:
    session = requests.Session()
    session.headers.update({"Govee-API-Key": api_key})
    return session


def run_loop(config: dict, run_once: bool = False) -> int:
    """Poll forever (or once with ``run_once``). Returns a process exit code."""
    govee_session = _make_govee_session(config["govee_api_key"])
    quailsync_session = requests.Session()
    interval = config["poll_interval_seconds"]

    logger.info(
        "Govee poller started — QuailSync=%s, interval=%ds, temp_unit=%s",
        config["quailsync_api_url"],
        interval,
        config["temperature_unit"],
    )

    try:
        while True:
            poll_once(config, govee_session, quailsync_session)
            if run_once:
                return 0
            time.sleep(interval)
    except KeyboardInterrupt:
        logger.info("Interrupted — shutting down")
        return 0
    finally:
        govee_session.close()
        quailsync_session.close()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="QuailSync Govee sensor poller")
    parser.add_argument(
        "--config",
        default=str(Path(__file__).with_name("config.yaml")),
        help="Path to config.yaml (default: alongside this script)",
    )
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run a single poll cycle and exit (for testing)",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        help="Logging level (DEBUG, INFO, WARNING, ERROR)",
    )
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper(), logging.INFO),
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        stream=sys.stdout,
    )

    try:
        config = load_config(args.config)
    except ConfigError as exc:
        logger.error("Configuration error: %s", exc)
        return 1

    return run_loop(config, run_once=args.once)


if __name__ == "__main__":
    raise SystemExit(main())
