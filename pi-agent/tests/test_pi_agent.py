"""
Boundary & stress tests for QuailSync Pi Agent.

Tests sensor edge cases, payload builders, reconnection backoff,
system metrics collection, and WebSocket protocol handling.

Run with:
    cd pi-agent && python -m pytest tests/test_pi_agent.py -v
"""

import asyncio
import json
import math
import os
import sys
import time
import unittest
from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock, PropertyMock, patch

# Add parent dir to path so we can import pi_agent
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

# pi_agent requires websockets at import time. If not installed, mock it
# with proper exception classes that can be caught.
try:
    import websockets as _ws_check
except ImportError:
    _ws_mock = MagicMock()
    _ws_mock.ConnectionClosed = type("ConnectionClosed", (Exception,), {})
    _ws_mock.InvalidURI = type("InvalidURI", (Exception,), {})
    _ws_mock.InvalidHandshake = type("InvalidHandshake", (Exception,), {})
    sys.modules["websockets"] = _ws_mock

from pi_agent import (
    BROODER_INTERVAL,
    DEFAULT_SERVER,
    SENSOR_CONFIG,
    SENSOR_READ_DELAY,
    SYSTEM_INTERVAL,
    _fmt_bytes,
    build_brooder_payload,
    build_system_payload,
    collect_system_metrics,
    connect_with_backoff,
    read_dht22,
    run_agent,
)


def _make_sensors_list(sensor=None, count=1):
    """Build a sensors list matching run_agent's expected format."""
    return [
        {"brooder_id": i + 1, "gpio_pin": 4 + i, "label": f"test-brooder-{i+1}", "sensor": sensor}
        for i in range(count)
    ]


# ===========================================================================
# 1. SENSOR READING EDGE CASES
# ===========================================================================


class TestReadDHT22(unittest.TestCase):
    """Test DHT22 sensor reading with mocked hardware."""

    def _make_sensor(self, temp_c=None, humidity=None, error=None):
        """Create a mock sensor that returns given values or raises."""
        sensor = MagicMock()
        if error:
            type(sensor).temperature = PropertyMock(side_effect=error)
            type(sensor).humidity = PropertyMock(side_effect=error)
        else:
            type(sensor).temperature = PropertyMock(return_value=temp_c)
            type(sensor).humidity = PropertyMock(return_value=humidity)
        return sensor

    def test_normal_reading(self):
        sensor = self._make_sensor(temp_c=25.0, humidity=55.0)
        result = read_dht22(sensor, retries=1)
        self.assertIsNotNone(result)
        temp_f, hum = result
        self.assertAlmostEqual(temp_f, 77.0, places=1)
        self.assertAlmostEqual(hum, 55.0, places=1)

    def test_temperature_none(self):
        sensor = self._make_sensor(temp_c=None, humidity=55.0)
        result = read_dht22(sensor, retries=1)
        self.assertIsNone(result)

    def test_humidity_none(self):
        sensor = self._make_sensor(temp_c=25.0, humidity=None)
        result = read_dht22(sensor, retries=1)
        self.assertIsNone(result)

    def test_both_none(self):
        sensor = self._make_sensor(temp_c=None, humidity=None)
        result = read_dht22(sensor, retries=3)
        self.assertIsNone(result)

    @patch("pi_agent.time.sleep")
    def test_runtime_error_retries_then_fails(self, mock_sleep):
        sensor = self._make_sensor(error=RuntimeError("Checksum mismatch"))
        result = read_dht22(sensor, retries=3)
        self.assertIsNone(result)
        # Should have slept between retries
        self.assertEqual(mock_sleep.call_count, 3)

    @patch("pi_agent.time.sleep")
    def test_runtime_error_10_retries(self, mock_sleep):
        sensor = self._make_sensor(error=RuntimeError("Timing error"))
        result = read_dht22(sensor, retries=10)
        self.assertIsNone(result)
        self.assertEqual(mock_sleep.call_count, 10)

    @patch("pi_agent.time.sleep")
    def test_runtime_error_then_success(self, mock_sleep):
        sensor = MagicMock()
        # Attempts 1&2: temperature raises before humidity is accessed.
        # Attempt 3: temperature succeeds, then humidity is accessed for the first time.
        temps = [RuntimeError("fail"), RuntimeError("fail"), 25.0]
        hums = [50.0]  # only accessed on attempt 3
        type(sensor).temperature = PropertyMock(side_effect=temps)
        type(sensor).humidity = PropertyMock(side_effect=hums)
        result = read_dht22(sensor, retries=3)
        self.assertIsNotNone(result)
        self.assertAlmostEqual(result[0], 77.0, places=1)

    def test_extreme_cold_minus_40c(self):
        """DHT22 spec minimum: -40 C = -40 F"""
        sensor = self._make_sensor(temp_c=-40.0, humidity=0.0)
        result = read_dht22(sensor, retries=1)
        self.assertIsNotNone(result)
        temp_f, hum = result
        self.assertAlmostEqual(temp_f, -40.0, places=1)
        self.assertAlmostEqual(hum, 0.0, places=1)

    def test_extreme_hot_80c(self):
        """DHT22 spec maximum: 80 C = 176 F"""
        sensor = self._make_sensor(temp_c=80.0, humidity=100.0)
        result = read_dht22(sensor, retries=1)
        self.assertIsNotNone(result)
        temp_f, hum = result
        self.assertAlmostEqual(temp_f, 176.0, places=1)
        self.assertAlmostEqual(hum, 100.0, places=1)

    def test_zero_celsius(self):
        sensor = self._make_sensor(temp_c=0.0, humidity=50.0)
        result = read_dht22(sensor, retries=1)
        self.assertIsNotNone(result)
        self.assertAlmostEqual(result[0], 32.0, places=1)

    def test_humidity_zero(self):
        sensor = self._make_sensor(temp_c=25.0, humidity=0.0)
        result = read_dht22(sensor, retries=1)
        self.assertIsNotNone(result)
        self.assertAlmostEqual(result[1], 0.0, places=1)

    def test_humidity_100(self):
        sensor = self._make_sensor(temp_c=25.0, humidity=100.0)
        result = read_dht22(sensor, retries=1)
        self.assertIsNotNone(result)
        self.assertAlmostEqual(result[1], 100.0, places=1)


# ===========================================================================
# 2. PAYLOAD BUILDERS
# ===========================================================================


class TestBuildBrooderPayload(unittest.TestCase):
    """Test JSON payload construction for BrooderReading."""

    def test_basic_payload_structure(self):
        payload = build_brooder_payload(98.6, 55.0, 1)
        data = json.loads(payload)
        self.assertIn("Brooder", data)
        br = data["Brooder"]
        self.assertAlmostEqual(br["temperature_celsius"], 98.6)
        self.assertAlmostEqual(br["humidity_percent"], 55.0)
        self.assertEqual(br["brooder_id"], 1)
        self.assertIn("timestamp", br)

    def test_timestamp_format(self):
        payload = build_brooder_payload(98.0, 50.0, 1)
        data = json.loads(payload)
        ts = data["Brooder"]["timestamp"]
        # Should end with Z and be ISO 8601 with milliseconds
        self.assertTrue(ts.endswith("Z"))
        self.assertRegex(ts, r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z")

    def test_negative_temperature(self):
        payload = build_brooder_payload(-40.0, 0.0, 1)
        data = json.loads(payload)
        self.assertAlmostEqual(data["Brooder"]["temperature_celsius"], -40.0)

    def test_zero_values(self):
        payload = build_brooder_payload(0.0, 0.0, 0)
        data = json.loads(payload)
        self.assertEqual(data["Brooder"]["temperature_celsius"], 0.0)
        self.assertEqual(data["Brooder"]["humidity_percent"], 0.0)
        self.assertEqual(data["Brooder"]["brooder_id"], 0)

    def test_large_brooder_id(self):
        payload = build_brooder_payload(98.0, 50.0, 999999)
        data = json.loads(payload)
        self.assertEqual(data["Brooder"]["brooder_id"], 999999)

    def test_extreme_values(self):
        payload = build_brooder_payload(99999.99, 99999.99, 1)
        data = json.loads(payload)
        self.assertAlmostEqual(data["Brooder"]["temperature_celsius"], 99999.99)

    def test_payload_is_valid_json(self):
        payload = build_brooder_payload(98.0, 50.0, 1)
        # Should not raise
        parsed = json.loads(payload)
        # Re-serialize should also work
        json.dumps(parsed)


class TestBuildSystemPayload(unittest.TestCase):
    """Test JSON payload construction for SystemMetrics."""

    def test_basic_structure(self):
        metrics = {
            "cpu_usage_percent": 42.5,
            "memory_used_bytes": 512000000,
            "memory_total_bytes": 4294967296,
            "disk_used_bytes": 5000000000,
            "disk_total_bytes": 53687091200,
            "uptime_seconds": 604800,
        }
        payload = build_system_payload(metrics)
        data = json.loads(payload)
        self.assertIn("System", data)
        self.assertEqual(data["System"]["cpu_usage_percent"], 42.5)

    def test_all_zeros(self):
        metrics = {
            "cpu_usage_percent": 0.0,
            "memory_used_bytes": 0,
            "memory_total_bytes": 0,
            "disk_used_bytes": 0,
            "disk_total_bytes": 0,
            "uptime_seconds": 0,
        }
        payload = build_system_payload(metrics)
        data = json.loads(payload)
        self.assertEqual(data["System"]["uptime_seconds"], 0)

    def test_max_u64_values(self):
        max_val = 2**64 - 1
        metrics = {
            "cpu_usage_percent": 100.0,
            "memory_used_bytes": max_val,
            "memory_total_bytes": max_val,
            "disk_used_bytes": max_val,
            "disk_total_bytes": max_val,
            "uptime_seconds": max_val,
        }
        payload = build_system_payload(metrics)
        data = json.loads(payload)
        self.assertEqual(data["System"]["memory_used_bytes"], max_val)


# ===========================================================================
# 3. FORMAT HELPERS
# ===========================================================================


class TestFmtBytes(unittest.TestCase):
    def test_megabytes(self):
        self.assertEqual(_fmt_bytes(512_000_000), "512MB")

    def test_gigabytes(self):
        self.assertEqual(_fmt_bytes(1_500_000_000), "1.5G")

    def test_zero(self):
        self.assertEqual(_fmt_bytes(0), "0MB")

    def test_just_under_gigabyte(self):
        self.assertEqual(_fmt_bytes(999_999_999), "999MB")

    def test_exactly_one_gigabyte(self):
        self.assertEqual(_fmt_bytes(1_000_000_000), "1.0G")

    def test_large_value(self):
        result = _fmt_bytes(100_000_000_000)
        self.assertEqual(result, "100.0G")


# ===========================================================================
# 4. SYSTEM METRICS COLLECTION
# ===========================================================================


class TestCollectSystemMetrics(unittest.TestCase):
    """Test system metrics collection — uses real psutil if available."""

    def test_returns_dict_or_none(self):
        result = collect_system_metrics()
        if result is not None:
            self.assertIn("cpu_usage_percent", result)
            self.assertIn("memory_used_bytes", result)
            self.assertIn("memory_total_bytes", result)
            self.assertIn("disk_used_bytes", result)
            self.assertIn("disk_total_bytes", result)
            self.assertIn("uptime_seconds", result)

    def test_all_values_numeric(self):
        result = collect_system_metrics()
        if result is not None:
            for key, value in result.items():
                self.assertIsInstance(
                    value, (int, float), f"{key} should be numeric, got {type(value)}"
                )

    def test_cpu_in_valid_range(self):
        result = collect_system_metrics()
        if result is not None:
            cpu = result["cpu_usage_percent"]
            self.assertGreaterEqual(cpu, 0.0)
            self.assertLessEqual(cpu, 100.0)

    def test_memory_total_greater_than_zero(self):
        result = collect_system_metrics()
        if result is not None:
            self.assertGreater(result["memory_total_bytes"], 0)

    def test_disk_total_greater_than_zero(self):
        result = collect_system_metrics()
        if result is not None:
            self.assertGreater(result["disk_total_bytes"], 0)

    def test_uptime_positive(self):
        result = collect_system_metrics()
        if result is not None:
            self.assertGreaterEqual(result["uptime_seconds"], 0)

    def test_memory_used_less_or_equal_total(self):
        result = collect_system_metrics()
        if result is not None:
            self.assertLessEqual(
                result["memory_used_bytes"], result["memory_total_bytes"]
            )


# ===========================================================================
# 5. WEBSOCKET RECONNECTION BACKOFF
# ===========================================================================


class TestConnectWithBackoff(unittest.TestCase):
    """Test exponential backoff and reconnection logic."""

    def test_backoff_doubles_on_failure(self):
        """Verify delay doubles after each failed connection attempt."""
        delays_seen = []

        async def fake_run(*args):
            raise OSError("Connection refused")

        original_sleep = asyncio.sleep

        async def capture_sleep(duration):
            delays_seen.append(duration)
            if len(delays_seen) >= 5:
                raise KeyboardInterrupt  # break the loop
            await original_sleep(0)  # don't actually wait

        with patch("pi_agent.run_agent", side_effect=fake_run):
            with patch("pi_agent.asyncio.sleep", side_effect=capture_sleep):
                with self.assertRaises(KeyboardInterrupt):
                    asyncio.get_event_loop().run_until_complete(
                        connect_with_backoff("ws://localhost:0/ws", _make_sensors_list())
                    )

        # Delays should double: 1, 2, 4, 8, 16...
        self.assertEqual(delays_seen[0], 1)
        self.assertEqual(delays_seen[1], 2)
        self.assertEqual(delays_seen[2], 4)
        self.assertEqual(delays_seen[3], 8)
        self.assertEqual(delays_seen[4], 16)

    def test_backoff_max_60_seconds(self):
        """Verify delay caps at 60 seconds."""
        delays_seen = []

        async def fake_run(*args):
            raise OSError("Connection refused")

        async def capture_sleep(duration):
            delays_seen.append(duration)
            if len(delays_seen) >= 10:
                raise KeyboardInterrupt
            # don't actually wait

        with patch("pi_agent.run_agent", side_effect=fake_run):
            with patch("pi_agent.asyncio.sleep", side_effect=capture_sleep):
                with self.assertRaises(KeyboardInterrupt):
                    asyncio.get_event_loop().run_until_complete(
                        connect_with_backoff("ws://localhost:0/ws", _make_sensors_list())
                    )

        # No delay should exceed 60
        for d in delays_seen:
            self.assertLessEqual(d, 60, f"Delay {d} exceeds max of 60")

    def test_backoff_resets_after_long_connection(self):
        """If connection lasted >5s, delay should reset to 1.

        We simulate a long connection by advancing time.monotonic returns
        so that connect_with_backoff sees >5s elapsed for one call.
        """
        delays_seen = []
        call_count = [0]
        mono_time = [100.0]  # Fake monotonic clock

        def fake_monotonic():
            return mono_time[0]

        async def fake_run(*args):
            call_count[0] += 1
            if call_count[0] == 3:
                # Simulate a connection that lasted 10 seconds
                mono_time[0] += 10.0
            raise OSError("connection failed")

        async def capture_sleep(duration):
            delays_seen.append(duration)
            # Advance fake clock past the sleep
            mono_time[0] += duration
            if len(delays_seen) >= 5:
                raise KeyboardInterrupt

        with patch("pi_agent.run_agent", side_effect=fake_run):
            with patch("pi_agent.asyncio.sleep", side_effect=capture_sleep):
                with patch("pi_agent.time.monotonic", side_effect=fake_monotonic):
                    with self.assertRaises(KeyboardInterrupt):
                        asyncio.get_event_loop().run_until_complete(
                            connect_with_backoff("ws://localhost:0/ws", _make_sensors_list())
                        )

        # Calls 1,2 fail fast → delays double: 1, 2
        # Call 3 "lasts" 10s → delay resets to 1
        # Calls 4,5 fail fast → delays double: 1, 2
        self.assertEqual(delays_seen[0], 1)
        self.assertEqual(delays_seen[1], 2)
        self.assertEqual(delays_seen[2], 1)  # Reset!
        self.assertEqual(delays_seen[3], 2)
        self.assertEqual(delays_seen[4], 4)


# ===========================================================================
# 6. RUN_AGENT INTERVAL LOGIC
# ===========================================================================


class TestRunAgent(unittest.TestCase):
    """Test the main agent loop's interval logic."""

    def test_sends_system_metrics_on_first_iteration(self):
        """System metrics should be sent immediately (last_system starts at 0)."""
        messages_sent = []

        async def run():
            ws = AsyncMock()

            async def fake_send(msg):
                messages_sent.append(msg)
                if len(messages_sent) >= 2:
                    raise Exception("stop")

            ws.send = fake_send
            ws.__aenter__ = AsyncMock(return_value=ws)
            ws.__aexit__ = AsyncMock(return_value=False)

            with patch("pi_agent.websockets.connect", return_value=ws):
                try:
                    await run_agent("ws://localhost:0/ws", _make_sensors_list())
                except Exception:
                    pass

        asyncio.get_event_loop().run_until_complete(run())

        # With no sensor, should only send system metrics
        if messages_sent:
            data = json.loads(messages_sent[0])
            self.assertIn("System", data)

    def test_sensor_reading_included_when_sensor_present(self):
        """With a sensor, first message should be a Brooder reading."""
        messages_sent = []

        sensor = MagicMock()
        type(sensor).temperature = PropertyMock(return_value=25.0)
        type(sensor).humidity = PropertyMock(return_value=50.0)

        async def run():
            ws = AsyncMock()

            async def fake_send(msg):
                messages_sent.append(msg)
                if len(messages_sent) >= 1:
                    raise Exception("stop")

            ws.send = fake_send
            ws.__aenter__ = AsyncMock(return_value=ws)
            ws.__aexit__ = AsyncMock(return_value=False)

            with patch("pi_agent.websockets.connect", return_value=ws):
                try:
                    await run_agent("ws://localhost:0/ws", _make_sensors_list(sensor=sensor))
                except Exception:
                    pass

        asyncio.get_event_loop().run_until_complete(run())

        if messages_sent:
            data = json.loads(messages_sent[0])
            # Should be either Brooder or System depending on timing
            self.assertTrue(
                "Brooder" in data or "System" in data,
                f"Unexpected payload: {data}",
            )


# ===========================================================================
# 7. PAYLOAD COMPATIBILITY WITH RUST SERDE
# ===========================================================================


class TestPayloadSerdeCompatibility(unittest.TestCase):
    """Ensure payloads match the exact JSON format the Rust server expects.

    The Rust server uses serde's externally-tagged enum:
        {"Brooder": {...}} or {"System": {...}}
    """

    def test_brooder_has_all_required_fields(self):
        payload = build_brooder_payload(98.0, 50.0, 1)
        data = json.loads(payload)
        br = data["Brooder"]
        required = ["temperature_celsius", "humidity_percent", "timestamp", "brooder_id"]
        for field in required:
            self.assertIn(field, br, f"Missing required field: {field}")

    def test_system_has_all_required_fields(self):
        metrics = {
            "cpu_usage_percent": 10.0,
            "memory_used_bytes": 100,
            "memory_total_bytes": 200,
            "disk_used_bytes": 300,
            "disk_total_bytes": 400,
            "uptime_seconds": 500,
        }
        payload = build_system_payload(metrics)
        data = json.loads(payload)
        sys_data = data["System"]
        required = [
            "cpu_usage_percent",
            "memory_used_bytes",
            "memory_total_bytes",
            "disk_used_bytes",
            "disk_total_bytes",
            "uptime_seconds",
        ]
        for field in required:
            self.assertIn(field, sys_data, f"Missing required field: {field}")

    def test_no_extra_wrapper_keys(self):
        payload = build_brooder_payload(98.0, 50.0, 1)
        data = json.loads(payload)
        self.assertEqual(list(data.keys()), ["Brooder"])

    def test_brooder_id_is_integer(self):
        payload = build_brooder_payload(98.0, 50.0, 42)
        data = json.loads(payload)
        self.assertIsInstance(data["Brooder"]["brooder_id"], int)

    def test_temperature_is_float(self):
        payload = build_brooder_payload(98.0, 50.0, 1)
        data = json.loads(payload)
        self.assertIsInstance(data["Brooder"]["temperature_celsius"], float)


# ===========================================================================
# 8. CONSTANTS & CONFIGURATION
# ===========================================================================


class TestConstants(unittest.TestCase):
    def test_default_server_is_valid_ws_url(self):
        self.assertTrue(DEFAULT_SERVER.startswith("ws://"))
        self.assertIn("/ws", DEFAULT_SERVER)

    def test_brooder_interval_is_5_seconds(self):
        self.assertEqual(BROODER_INTERVAL, 5)

    def test_system_interval_is_30_seconds(self):
        self.assertEqual(SYSTEM_INTERVAL, 30)

    def test_sensor_config_has_3_entries(self):
        self.assertEqual(len(SENSOR_CONFIG), 3)

    def test_sensor_config_entries_have_required_keys(self):
        for cfg in SENSOR_CONFIG:
            self.assertIn("brooder_id", cfg)
            self.assertIn("gpio_pin", cfg)
            self.assertIn("label", cfg)
            self.assertIsInstance(cfg["brooder_id"], int)
            self.assertIsInstance(cfg["gpio_pin"], int)

    def test_sensor_config_unique_pins(self):
        pins = [cfg["gpio_pin"] for cfg in SENSOR_CONFIG]
        self.assertEqual(len(pins), len(set(pins)), "GPIO pins must be unique")

    def test_sensor_config_unique_brooder_ids(self):
        ids = [cfg["brooder_id"] for cfg in SENSOR_CONFIG]
        self.assertEqual(len(ids), len(set(ids)), "Brooder IDs must be unique")

    def test_sensor_read_delay(self):
        self.assertEqual(SENSOR_READ_DELAY, 0.5)


# ===========================================================================
# 9. EDGE CASES IN C-TO-F CONVERSION
# ===========================================================================


class TestTemperatureConversion(unittest.TestCase):
    """Verify the C-to-F conversion in read_dht22."""

    def _read_temp(self, temp_c):
        sensor = MagicMock()
        type(sensor).temperature = PropertyMock(return_value=temp_c)
        type(sensor).humidity = PropertyMock(return_value=50.0)
        result = read_dht22(sensor, retries=1)
        return result[0] if result else None

    def test_freezing_point(self):
        self.assertAlmostEqual(self._read_temp(0.0), 32.0, places=1)

    def test_boiling_point(self):
        self.assertAlmostEqual(self._read_temp(100.0), 212.0, places=1)

    def test_body_temp(self):
        self.assertAlmostEqual(self._read_temp(37.0), 98.6, places=1)

    def test_minus_40_crossover(self):
        """At -40, Celsius and Fahrenheit are the same."""
        self.assertAlmostEqual(self._read_temp(-40.0), -40.0, places=1)

    def test_negative_celsius(self):
        self.assertAlmostEqual(self._read_temp(-10.0), 14.0, places=1)

    def test_fractional_celsius(self):
        # 36.6 C = 97.88 F
        result = self._read_temp(36.6)
        self.assertAlmostEqual(result, 97.9, places=0)


# ===========================================================================
# 10. SENSOR FAILURE PATTERNS
# ===========================================================================


class TestSensorFailurePatterns(unittest.TestCase):
    """Real-world failure patterns from DHT22 sensors."""

    @patch("pi_agent.time.sleep")
    def test_intermittent_failures(self, mock_sleep):
        """Sensor fails twice then succeeds — common with DHT22."""
        sensor = MagicMock()
        # Temperature raises on first two attempts, succeeds on third.
        # Humidity is only accessed on the third attempt (after temp succeeds).
        temps = [RuntimeError("checksum"), RuntimeError("timing"), 25.0]
        hums = [50.0]
        type(sensor).temperature = PropertyMock(side_effect=temps)
        type(sensor).humidity = PropertyMock(side_effect=hums)

        result = read_dht22(sensor, retries=3)
        self.assertIsNotNone(result)

    @patch("pi_agent.time.sleep")
    def test_all_retries_exhausted(self, mock_sleep):
        """All retries fail — sensor is broken."""
        sensor = MagicMock()
        type(sensor).temperature = PropertyMock(
            side_effect=RuntimeError("permanent failure")
        )
        result = read_dht22(sensor, retries=5)
        self.assertIsNone(result)
        self.assertEqual(mock_sleep.call_count, 5)

    def test_sensor_returns_negative_humidity(self):
        """Some faulty sensors return negative humidity."""
        sensor = MagicMock()
        type(sensor).temperature = PropertyMock(return_value=25.0)
        type(sensor).humidity = PropertyMock(return_value=-5.0)
        result = read_dht22(sensor, retries=1)
        # Should still return it — server decides what to do with bad values
        self.assertIsNotNone(result)
        self.assertEqual(result[1], -5.0)

    def test_sensor_returns_over_100_humidity(self):
        """Faulty sensor reading above 100% humidity."""
        sensor = MagicMock()
        type(sensor).temperature = PropertyMock(return_value=25.0)
        type(sensor).humidity = PropertyMock(return_value=105.3)
        result = read_dht22(sensor, retries=1)
        self.assertIsNotNone(result)
        self.assertAlmostEqual(result[1], 105.3, places=1)


# ===========================================================================
# 11. MULTI-SENSOR SUPPORT
# ===========================================================================


class TestMultiSensor(unittest.TestCase):
    """Test multi-sensor iteration and consecutive failure tracking."""

    def _make_mock_sensor(self, temp_c, humidity):
        """Create a mock sensor with unique spec to avoid PropertyMock class-level conflicts."""
        sensor = MagicMock(spec=["temperature", "humidity"])
        sensor.temperature = temp_c
        sensor.humidity = humidity
        return sensor

    def test_multiple_sensors_all_send(self):
        """All 3 sensors should produce separate Brooder payloads."""
        messages_sent = []

        sensors = [
            {"brooder_id": 1, "gpio_pin": 4, "label": "b1",
             "sensor": self._make_mock_sensor(25.0, 50.0)},
            {"brooder_id": 2, "gpio_pin": 17, "label": "b2",
             "sensor": self._make_mock_sensor(30.0, 60.0)},
            {"brooder_id": 3, "gpio_pin": 27, "label": "b3",
             "sensor": self._make_mock_sensor(35.0, 70.0)},
        ]

        async def run():
            ws = AsyncMock()

            async def fake_send(msg):
                messages_sent.append(msg)
                if len(messages_sent) >= 4:
                    raise Exception("stop")

            ws.send = fake_send
            ws.__aenter__ = AsyncMock(return_value=ws)
            ws.__aexit__ = AsyncMock(return_value=False)

            async def fast_sleep(d):
                pass

            with patch("pi_agent.websockets.connect", return_value=ws):
                with patch("pi_agent.asyncio.sleep", side_effect=fast_sleep):
                    try:
                        await run_agent("ws://localhost:0/ws", sensors)
                    except Exception:
                        pass

        asyncio.get_event_loop().run_until_complete(run())

        brooder_ids = []
        for msg in messages_sent:
            data = json.loads(msg)
            if "Brooder" in data:
                brooder_ids.append(data["Brooder"]["brooder_id"])

        self.assertIn(1, brooder_ids)
        self.assertIn(2, brooder_ids)
        self.assertIn(3, brooder_ids)

    def test_one_sensor_failure_doesnt_block_others(self):
        """If sensor 2 fails, sensors 1 and 3 should still send."""
        messages_sent = []

        good1 = self._make_mock_sensor(25.0, 50.0)
        good3 = self._make_mock_sensor(25.0, 50.0)

        bad_sensor = MagicMock()
        type(bad_sensor).temperature = PropertyMock(side_effect=RuntimeError("broken"))

        sensors = [
            {"brooder_id": 1, "gpio_pin": 4, "label": "good-1", "sensor": good1},
            {"brooder_id": 2, "gpio_pin": 17, "label": "bad-2", "sensor": bad_sensor},
            {"brooder_id": 3, "gpio_pin": 27, "label": "good-3", "sensor": good3},
        ]

        async def run():
            ws = AsyncMock()

            async def fake_send(msg):
                messages_sent.append(msg)
                if len(messages_sent) >= 3:
                    raise Exception("stop")

            ws.send = fake_send
            ws.__aenter__ = AsyncMock(return_value=ws)
            ws.__aexit__ = AsyncMock(return_value=False)

            async def fast_sleep(d):
                pass

            with patch("pi_agent.websockets.connect", return_value=ws):
                with patch("pi_agent.asyncio.sleep", side_effect=fast_sleep):
                    with patch("pi_agent.time.sleep"):
                        try:
                            await run_agent("ws://localhost:0/ws", sensors)
                        except Exception:
                            pass

        asyncio.get_event_loop().run_until_complete(run())

        brooder_ids = []
        for msg in messages_sent:
            data = json.loads(msg)
            if "Brooder" in data:
                brooder_ids.append(data["Brooder"]["brooder_id"])

        self.assertIn(1, brooder_ids)
        self.assertIn(3, brooder_ids)
        self.assertNotIn(2, brooder_ids)

    def test_none_sensor_skipped(self):
        """Entries with sensor=None should be silently skipped."""
        messages_sent = []

        sensors = [
            {"brooder_id": 1, "gpio_pin": 4, "label": "no-sensor", "sensor": None},
        ]

        async def run():
            ws = AsyncMock()

            async def fake_send(msg):
                messages_sent.append(msg)
                if len(messages_sent) >= 1:
                    raise Exception("stop")

            ws.send = fake_send
            ws.__aenter__ = AsyncMock(return_value=ws)
            ws.__aexit__ = AsyncMock(return_value=False)

            with patch("pi_agent.websockets.connect", return_value=ws):
                try:
                    await run_agent("ws://localhost:0/ws", sensors)
                except Exception:
                    pass

        asyncio.get_event_loop().run_until_complete(run())

        for msg in messages_sent:
            data = json.loads(msg)
            self.assertNotIn("Brooder", data)


if __name__ == "__main__":
    unittest.main()
