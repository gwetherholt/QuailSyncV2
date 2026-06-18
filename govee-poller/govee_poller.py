"""QuailSync Govee sensor poller (Govee OpenAPI v2).

Polls the Govee OpenAPI v2 for H5179 (and sibling) WiFi temp/humidity sensors
and POSTs their readings to the QuailSync server at ``POST /api/govee/readings``.
Runs on the Raspberry Pi alongside QuailSync, managed by systemd (see
``govee-poller.service``).

Mirrors the trailcam module: a single-file Python service that polls an external
API on a timer, logs to stdout (captured by the journal), and never lets a
transient error kill the loop.

Why v2: the old v1 API (developer-api.govee.com) does NOT list thermometers.
The v2 OpenAPI (openapi.api.govee.com) does, and returns temperature already in
Fahrenheit plus an explicit online flag.

Flow:
  * On startup (and every 12 cycles ≈ 1 hour) GET the device list and keep the
    ones of type ``devices.types.thermometer``.
  * Each cycle, POST to the device-state endpoint for each thermometer, read
    ``sensorTemperature`` / ``sensorHumidity`` / ``online`` from the capabilities
    array, skip offline sensors, and POST the batch to QuailSync.

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
import uuid
from datetime import datetime, timezone
from pathlib import Path

import requests
import yaml

logger = logging.getLogger("govee_poller")

# --- Govee OpenAPI v2 ------------------------------------------------------
GOVEE_API_BASE = "https://openapi.api.govee.com"
DEVICES_URL = f"{GOVEE_API_BASE}/router/api/v1/user/devices"
DEVICE_STATE_URL = f"{GOVEE_API_BASE}/router/api/v1/device/state"

# Device "type" reported by Govee for temp/humidity sensors.
THERMOMETER_TYPE = "devices.types.thermometer"
# Fallback SKU allow-list, used only when a device omits its `type` field. H5179
# is ours; the rest are common thermo-hygrometer siblings. Matched case-insensitively.
THERMOMETER_SKUS = {
    "H5179",
    "H5075",
    "H5100", "H5101", "H5102", "H5103", "H5104", "H5105",
    "H5174", "H5177", "H5198",
    "H5051", "H5052", "H5071", "H5072", "H5074",
}

# HTTP timeout for every outbound request (connect + read), in seconds.
REQUEST_TIMEOUT = 30
# Small pause between per-device state calls so a cabinet of sensors can't burst
# past Govee's rate limit. Volume is tiny (~864/day for 3 sensors @ 5-min), well
# under the 10,000/day cap — this is just about not bunching requests.
INTER_REQUEST_DELAY = 1.0
# Re-fetch the device list every N cycles (12 × 5 min ≈ 1 hour) to pick up
# newly-added sensors without a restart.
DEVICE_REFRESH_CYCLES = 12


def is_thermometer(device: dict) -> bool:
    """True if ``device`` is a temp/humidity sensor (by type, or known SKU)."""
    if str(device.get("type", "")).strip() == THERMOMETER_TYPE:
        return True
    return str(device.get("sku", "")).strip().upper() in THERMOMETER_SKUS


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

    return {
        "govee_api_key": api_key,
        "quailsync_api_url": quailsync_api_url,
        "poll_interval_seconds": poll_interval_seconds,
    }


# ---------------------------------------------------------------------------
# Govee API access
# ---------------------------------------------------------------------------


def _check_rate_limit(resp: requests.Response, label: str) -> None:
    """Log Govee's remaining-quota header (debug) and warn on a 429."""
    remaining = (
        resp.headers.get("API-RateLimit-Remaining")
        or resp.headers.get("X-RateLimit-Remaining")
        or resp.headers.get("Rate-Limit-Remaining")
    )
    if remaining is not None:
        logger.debug("Govee rate-limit remaining: %s", remaining)
    if resp.status_code == 429:
        retry_after = resp.headers.get("Retry-After", "unknown")
        logger.warning("Govee rate limit hit on %s (Retry-After=%s)", label, retry_after)


def get_devices(session: requests.Session) -> list[dict]:
    """Return the raw device list from ``GET /router/api/v1/user/devices``.

    Raises on transport/HTTP errors (incl. 429); the caller logs and retries.
    """
    resp = session.get(DEVICES_URL, timeout=REQUEST_TIMEOUT)
    _check_rate_limit(resp, "/user/devices")
    resp.raise_for_status()
    return resp.json().get("data", []) or []


def fetch_thermometers(session: requests.Session) -> list[dict]:
    """GET the device list and keep only the thermometers."""
    devices = get_devices(session)
    thermometers = [d for d in devices if is_thermometer(d)]
    logger.info(
        "Device list: %d device(s), %d thermometer(s)", len(devices), len(thermometers)
    )
    return thermometers


def get_device_state(session: requests.Session, sku: str, device: str) -> dict:
    """POST to ``/router/api/v1/device/state`` and return the raw payload.

    A fresh uuid4 ``requestId`` is sent on every call, as the API expects.
    """
    body = {
        "requestId": str(uuid.uuid4()),
        "payload": {"sku": sku, "device": device},
    }
    resp = session.post(DEVICE_STATE_URL, json=body, timeout=REQUEST_TIMEOUT)
    _check_rate_limit(resp, f"/device/state {device}")
    resp.raise_for_status()
    return resp.json()


def _to_float(value) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def extract_reading(state_data: dict) -> tuple[float, float, bool] | None:
    """Pull ``(temperature_f, humidity, online)`` from a v2 state payload.

    The payload's ``capabilities`` is a list of
    ``{"type", "instance", "state": {"value": ...}}`` entries. We read the
    ``sensorTemperature`` (already °F), ``sensorHumidity`` (% RH), and ``online``
    instances. Returns ``None`` if temperature or humidity is missing; ``online``
    defaults to ``True`` when the capability is absent (a device that reports a
    reading is, by definition, reachable).
    """
    capabilities = (state_data or {}).get("payload", {}).get("capabilities", []) or []

    temperature_f = None
    humidity = None
    online = True
    for cap in capabilities:
        if not isinstance(cap, dict):
            continue
        instance = cap.get("instance")
        value = (cap.get("state") or {}).get("value")
        if instance == "sensorTemperature":
            temperature_f = _to_float(value)
        elif instance == "sensorHumidity":
            humidity = _to_float(value)
        elif instance == "online":
            online = bool(value)

    if temperature_f is None or humidity is None:
        return None
    return temperature_f, humidity, online


# ---------------------------------------------------------------------------
# Reading assembly + delivery
# ---------------------------------------------------------------------------


def _now_iso() -> str:
    """Current UTC time as RFC 3339 with a trailing Z, e.g. 2025-06-17T12:00:00Z."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def build_readings(session: requests.Session, thermometers: list[dict]) -> list[dict]:
    """Query each thermometer's state and build the QuailSync payload list.

    Offline sensors are warned-and-skipped; a per-device error is logged and
    skipped so the rest of the batch still goes through.
    """
    readings: list[dict] = []
    logger.info("Polling %d thermometer(s)", len(thermometers))

    for index, device in enumerate(thermometers):
        sku = device.get("sku")
        device_id = device.get("device")
        name = device.get("deviceName")
        if not sku or not device_id:
            logger.warning("Skipping device with missing sku/device: %r", device)
            continue

        if index > 0:
            time.sleep(INTER_REQUEST_DELAY)  # be gentle with Govee's rate limit

        try:
            state = get_device_state(session, sku, device_id)
        except requests.RequestException as exc:
            logger.error("Failed to read state for %s (%s): %s", name or device_id, sku, exc)
            continue

        parsed = extract_reading(state)
        if parsed is None:
            logger.warning(
                "No temperature/humidity in state for %s (%s) — skipping",
                name or device_id,
                sku,
            )
            continue

        temperature_f, humidity, online = parsed
        if not online:
            logger.warning("Sensor %s (%s) is offline — skipping", name or device_id, sku)
            continue

        readings.append(
            {
                "device_id": device_id,
                "model": sku,
                "name": name,
                "temperature_f": round(temperature_f, 2),
                "humidity": round(humidity, 2),
                "recorded_at": _now_iso(),
            }
        )
        logger.info(
            "Read %s (%s): %.1f°F, %.1f%% RH",
            name or device_id,
            sku,
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
    thermometers: list[dict],
) -> None:
    """Run a single poll cycle over the known thermometers. Never raises."""
    if not thermometers:
        logger.info("No thermometers known — nothing to poll this cycle")
        return

    try:
        readings = build_readings(govee_session, thermometers)
    except Exception as exc:  # noqa: BLE001 — one bad cycle must not kill the loop
        logger.exception("Unexpected error while building readings: %s", exc)
        return

    if not readings:
        logger.info("No sensor readings collected this cycle — nothing to post")
        return

    post_readings(quailsync_session, config["quailsync_api_url"], readings)


def _make_govee_session(api_key: str) -> requests.Session:
    session = requests.Session()
    session.headers.update(
        {"Govee-API-Key": api_key, "Content-Type": "application/json"}
    )
    return session


def _refresh_thermometers(session: requests.Session, current: list[dict]) -> list[dict]:
    """Re-fetch the thermometer list, keeping the last-known-good on failure."""
    try:
        return fetch_thermometers(session)
    except requests.RequestException as exc:
        logger.error("Failed to list Govee devices: %s — keeping current list", exc)
    except ValueError as exc:  # bad JSON
        logger.error("Govee /user/devices returned invalid JSON: %s", exc)
    return current


def run_loop(config: dict, run_once: bool = False) -> int:
    """Poll forever (or once with ``run_once``). Returns a process exit code."""
    govee_session = _make_govee_session(config["govee_api_key"])
    quailsync_session = requests.Session()
    interval = config["poll_interval_seconds"]

    logger.info(
        "Govee poller (OpenAPI v2) started — QuailSync=%s, interval=%ds",
        config["quailsync_api_url"],
        interval,
    )

    thermometers: list[dict] = []
    cycle = 0
    try:
        while True:
            # Refresh the device list on startup, every DEVICE_REFRESH_CYCLES,
            # and whenever we still don't have one (e.g. a failed startup fetch
            # retries every cycle until it succeeds).
            if cycle % DEVICE_REFRESH_CYCLES == 0 or not thermometers:
                thermometers = _refresh_thermometers(govee_session, thermometers)

            poll_once(config, govee_session, quailsync_session, thermometers)
            if run_once:
                return 0
            cycle += 1
            time.sleep(interval)
    except KeyboardInterrupt:
        logger.info("Interrupted — shutting down")
        return 0
    finally:
        govee_session.close()
        quailsync_session.close()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="QuailSync Govee sensor poller (OpenAPI v2)")
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
