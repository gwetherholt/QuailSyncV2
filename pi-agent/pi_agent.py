#!/usr/bin/env python3
"""QuailSync Raspberry Pi Agent — reads DHT22 via kernel IIO driver + system metrics, sends over WebSocket."""

import argparse
import asyncio
import json
import os
import signal
import time
from datetime import datetime, timezone

# ── ANSI colors ──────────────────────────────────────────────────────────────
GREEN = "\033[32m"
YELLOW = "\033[33m"
RED = "\033[31m"
RESET = "\033[0m"

# ── Optional imports with graceful fallback ──────────────────────────────────
try:
    import psutil
    HAS_PSUTIL = True
except ImportError:
    HAS_PSUTIL = False

try:
    import websockets
except ImportError:
    print(f"{RED}[error]{RESET}  websockets package required: pip install websockets")
    raise SystemExit(1)

# ── Defaults ─────────────────────────────────────────────────────────────────
DEFAULT_SERVER = "ws://192.168.0.228:3000/ws"
BROODER_INTERVAL = 5      # seconds
SYSTEM_INTERVAL = 30       # seconds
SENSOR_READ_DELAY = 0.5    # seconds between reading different sensors

# ── Sensor-to-brooder mapping ────────────────────────────────────────────────
# Each entry maps a kernel IIO device path to a brooder ID (integer, matches DB primary key).
# The dht11 dtoverlay creates devices at /sys/bus/iio/devices/iio:deviceN/.
# The "label" is for log output only.
SENSOR_CONFIG = [
    {"brooder_id": 1, "iio_device": "/sys/bus/iio/devices/iio:device0", "label": "brooder-1-texas"},
    {"brooder_id": 2, "iio_device": "/sys/bus/iio/devices/iio:device1", "label": "brooder-2-pharaoh"},
    {"brooder_id": 3, "iio_device": "/sys/bus/iio/devices/iio:device2", "label": "brooder-3-fernbank"},
]


# ── Sensor reading ───────────────────────────────────────────────────────────
def read_dht22(iio_path, retries=3):
    """Read DHT22 via kernel IIO sysfs, retry on errors. Returns (temp_f, humidity) or None."""
    for attempt in range(1, retries + 1):
        try:
            with open(iio_path + "/in_temp_input") as f:
                temp_c = int(f.read().strip()) / 1000.0
            with open(iio_path + "/in_humidityrelative_input") as f:
                humidity = int(f.read().strip()) / 1000.0
            temp_f = temp_c * 9 / 5 + 32
            return (round(temp_f, 1), round(humidity, 1))
        except (OSError, ValueError) as e:
            print(f"{YELLOW}[sensor]{RESET}  {e}, retry {attempt}/{retries}...")
            time.sleep(0.5)
    return None


# ── System metrics ───────────────────────────────────────────────────────────
def collect_system_metrics():
    """Collect CPU, memory, disk, uptime. Uses psutil if available, else /proc fallback."""
    if HAS_PSUTIL:
        cpu = psutil.cpu_percent(interval=1)
        mem = psutil.virtual_memory()
        disk = psutil.disk_usage("/")
        uptime = int(time.time() - psutil.boot_time())
        return {
            "cpu_usage_percent": cpu,
            "memory_used_bytes": mem.used,
            "memory_total_bytes": mem.total,
            "disk_used_bytes": disk.used,
            "disk_total_bytes": disk.total,
            "uptime_seconds": uptime,
        }

    # Fallback: read /proc directly (Linux only)
    try:
        cpu = _proc_cpu_percent()
        mem_used, mem_total = _proc_meminfo()
        disk_used, disk_total = _proc_disk()
        uptime = _proc_uptime()
        return {
            "cpu_usage_percent": cpu,
            "memory_used_bytes": mem_used,
            "memory_total_bytes": mem_total,
            "disk_used_bytes": disk_used,
            "disk_total_bytes": disk_total,
            "uptime_seconds": uptime,
        }
    except Exception as e:
        print(f"{YELLOW}[system]{RESET}  Failed to collect metrics: {e}")
        return None


def _proc_cpu_percent():
    """Rough CPU usage from two /proc/stat samples 1s apart."""
    def read_idle():
        with open("/proc/stat") as f:
            parts = f.readline().split()
        idle = int(parts[4])
        total = sum(int(p) for p in parts[1:])
        return idle, total

    idle1, total1 = read_idle()
    time.sleep(1)
    idle2, total2 = read_idle()
    idle_d = idle2 - idle1
    total_d = total2 - total1
    if total_d == 0:
        return 0.0
    return round((1 - idle_d / total_d) * 100, 1)


def _proc_meminfo():
    """Returns (used_bytes, total_bytes) from /proc/meminfo."""
    info = {}
    with open("/proc/meminfo") as f:
        for line in f:
            parts = line.split()
            key = parts[0].rstrip(":")
            info[key] = int(parts[1]) * 1024  # kB -> bytes
            if key == "MemAvailable":
                break
    total = info.get("MemTotal", 0)
    available = info.get("MemAvailable", info.get("MemFree", 0))
    return total - available, total


def _proc_disk():
    """Returns (used_bytes, total_bytes) from statvfs."""
    st = os.statvfs("/")
    total = st.f_frsize * st.f_blocks
    free = st.f_frsize * st.f_bfree
    return total - free, total


def _proc_uptime():
    """Returns uptime in seconds from /proc/uptime."""
    with open("/proc/uptime") as f:
        return int(float(f.readline().split()[0]))


# ── Payload builders ─────────────────────────────────────────────────────────
def build_brooder_payload(temp_f, humidity, brooder_id):
    """Build JSON matching TelemetryPayload::Brooder(BrooderReading)."""
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"
    return json.dumps({
        "Brooder": {
            "temperature_f": temp_f,
            "humidity_percent": humidity,
            "timestamp": ts,
            "brooder_id": brooder_id,
        }
    })


def build_system_payload(metrics):
    """Build JSON matching TelemetryPayload::System(SystemMetrics)."""
    return json.dumps({"System": metrics})


# ── Formatted output helpers ─────────────────────────────────────────────────
def _fmt_bytes(b):
    """Human-readable byte size."""
    if b >= 1_000_000_000:
        return f"{b / 1_000_000_000:.1f}G"
    return f"{b // 1_000_000}MB"


# ── Main async loop ──────────────────────────────────────────────────────────
async def run_agent(server_url, sensors):
    """Connect to WebSocket and send telemetry on intervals.

    sensors is a list of dicts: {"brooder_id": int, "label": str, "iio_device": str, "available": bool}
    """
    async with websockets.connect(server_url) as ws:
        print(f"{GREEN}[connected]{RESET} WebSocket connected")

        last_brooder = 0.0
        last_system = 0.0
        # Track consecutive failures per sensor (keyed by brooder_id)
        consecutive_failures = {s["brooder_id"]: 0 for s in sensors}

        while True:
            now = time.monotonic()

            # Brooder readings every 5s — iterate all sensors
            if (now - last_brooder) >= BROODER_INTERVAL:
                for i, entry in enumerate(sensors):
                    bid = entry["brooder_id"]
                    label = entry["label"]
                    iio_device = entry["iio_device"]

                    if not entry["available"]:
                        continue

                    reading = read_dht22(iio_device)
                    if reading:
                        temp_f, humidity = reading
                        payload = build_brooder_payload(temp_f, humidity, bid)
                        await ws.send(payload)
                        print(f"{GREEN}[sensor]{RESET}  {label}: {temp_f}°F  {humidity}% humidity -> sent")
                        consecutive_failures[bid] = 0
                    else:
                        consecutive_failures[bid] += 1
                        if consecutive_failures[bid] >= 3:
                            print(
                                f"{RED}[error]{RESET}   "
                                f"Sensor at {iio_device} for {label} "
                                f"has failed {consecutive_failures[bid]} consecutive reads"
                            )
                        else:
                            print(
                                f"{YELLOW}[warn]{RESET}    "
                                f"Sensor read failed for {label} ({iio_device}), skipping"
                            )

                    # Small delay between sensor reads (not after the last one)
                    if i < len(sensors) - 1:
                        await asyncio.sleep(SENSOR_READ_DELAY)

                last_brooder = now

            # System metrics every 30s
            if (now - last_system) >= SYSTEM_INTERVAL:
                metrics = collect_system_metrics()
                if metrics:
                    payload = build_system_payload(metrics)
                    await ws.send(payload)
                    m = metrics
                    print(
                        f"{GREEN}[system]{RESET}  "
                        f"CPU {m['cpu_usage_percent']}%  "
                        f"RAM {_fmt_bytes(m['memory_used_bytes'])}/{_fmt_bytes(m['memory_total_bytes'])}  "
                        f"Disk {_fmt_bytes(m['disk_used_bytes'])}/{_fmt_bytes(m['disk_total_bytes'])}"
                    )
                last_system = now

            await asyncio.sleep(0.1)


async def connect_with_backoff(server_url, sensors):
    """Reconnection wrapper with exponential backoff."""
    delay = 1
    while True:
        started = time.monotonic()
        try:
            await run_agent(server_url, sensors)
        except (OSError, websockets.ConnectionClosed, websockets.InvalidURI,
                websockets.InvalidHandshake) as e:
            print(f"{RED}[error]{RESET}   {e}")
        # If we ran for a while, the connection was established — reset backoff
        if time.monotonic() - started > 5:
            delay = 1
        print(f"{YELLOW}[reconnect]{RESET} Reconnecting in {delay}s...")
        await asyncio.sleep(delay)
        delay = min(delay * 2, 60)


# ── Entry point ──────────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(description="QuailSync Raspberry Pi Agent")
    parser.add_argument("--server", type=str, default=None, help="WebSocket server URL")
    args = parser.parse_args()

    server_url = args.server or os.environ.get("QUAILSYNC_SERVER", DEFAULT_SERVER)

    # Init sensors from SENSOR_CONFIG — check IIO device availability
    sensors = []
    for cfg in SENSOR_CONFIG:
        iio_path = cfg["iio_device"]
        available = os.path.isdir(iio_path)
        entry = {
            "brooder_id": cfg["brooder_id"],
            "iio_device": iio_path,
            "label": cfg["label"],
            "available": available,
        }
        if available:
            print(f"{GREEN}[init]{RESET}     {cfg['label']}: IIO device at {iio_path}")
        else:
            print(f"{YELLOW}[warn]{RESET}    IIO device not found at {iio_path} — sensor reads disabled for {cfg['label']}")
        sensors.append(entry)

    if not HAS_PSUTIL:
        print(f"{YELLOW}[warn]{RESET}    psutil not available — using /proc fallback for system metrics")

    # Startup banner
    labels = ", ".join(cfg["label"] for cfg in SENSOR_CONFIG)
    print(f"{GREEN}[QuailSync Agent]{RESET} {len(SENSOR_CONFIG)} sensors [{labels}] -> {server_url}")

    # Clean shutdown
    loop = asyncio.new_event_loop()

    def shutdown(sig, frame):
        print(f"\n{YELLOW}[shutdown]{RESET} Caught signal, exiting...")
        loop.stop()
        raise SystemExit(0)

    signal.signal(signal.SIGINT, shutdown)
    signal.signal(signal.SIGTERM, shutdown)

    loop.run_until_complete(connect_with_backoff(server_url, sensors))


if __name__ == "__main__":
    main()
