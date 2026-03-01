# QuailSync

Real-time telemetry and monitoring system for quail brooder environments.

## Architecture

QuailSync is a Rust workspace with four crates:

```
┌─────────────┐   WebSocket    ┌─────────────────┐   REST API   ┌──────────────┐
│  quailsync  │───────────────>│   quailsync     │<────────────│  quailsync   │
│    agent    │  telemetry     │     server      │  queries    │     cli      │
│             │  (JSON)        │                 │             │              │
│  Collects   │                │  Axum + SQLite  │             │  clap-based  │
│  sensor &   │                │  Alert engine   │             │  dashboard   │
│  system data│                │  REST endpoints │             │              │
└─────────────┘                └─────────────────┘             └──────────────┘
```

| Crate | Type | Description |
|---|---|---|
| `quailsync-common` | Library | Shared types: telemetry payloads, alert config, severity levels |
| `quailsync-agent` | Binary | Collects brooder readings and system metrics, streams them to the server over WebSocket |
| `quailsync-server` | Binary | Receives telemetry, stores it in SQLite, evaluates alert thresholds, serves REST API |
| `quailsync-cli` | Binary | Queries the server and displays status, readings, metrics, and alerts in the terminal |

## Features

- **Real-time telemetry** -- Agent streams brooder temperature/humidity and system metrics over WebSocket every 5 seconds
- **Persistent storage** -- All readings stored in SQLite with automatic table creation
- **Configurable alert engine** -- Threshold-based alerts for temperature and humidity with Warning/Critical severity levels
- **REST API** -- Endpoints for latest readings, historical queries, system metrics, status summary, and alerts
- **CLI dashboard** -- Colored terminal output with status indicators, reading tables, and alert history
- **Zero-config startup** -- Sensible defaults for all thresholds (95-100°F temp, 40-60% humidity)

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.75+)

### Build

```bash
cargo build --release
```

### Run

Start the server, then the agent, then query with the CLI:

```bash
# Terminal 1 -- Start the server
cargo run --bin quailsync-server
```

```bash
# Terminal 2 -- Start the agent
cargo run --bin quailsync-agent
```

```bash
# Terminal 3 -- Query with the CLI
cargo run --bin quailsync-cli -- status
cargo run --bin quailsync-cli -- brood
cargo run --bin quailsync-cli -- brood --history 60
cargo run --bin quailsync-cli -- system
cargo run --bin quailsync-cli -- alerts
```

The server listens on `http://localhost:3000` by default. Point the CLI at a different server with `--server`:

```bash
cargo run --bin quailsync-cli -- --server http://192.168.1.50:3000 status
```

## API Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Server health check |
| GET | `/ws` | WebSocket endpoint for agent telemetry |
| GET | `/api/status` | Agent connection status and last-seen timestamps |
| GET | `/api/brooder/latest` | Most recent brooder reading |
| GET | `/api/brooder/history?minutes=N` | Brooder readings from the last N minutes |
| GET | `/api/system/latest` | Most recent system metrics |
| GET | `/api/alerts?minutes=N` | Alerts from the last N minutes |

## Example CLI Output

```
$ quailsync-cli status
QuailSync Status

  Agent:    ● connected

  Last Seen
    Brooder:   2026-03-01 20:22:48
    System:    2026-03-01 20:22:43
    Detection: no data

  Health:   healthy
```

```
$ quailsync-cli brood --history 60
Brooder — Last 60 Minutes (4 readings)

  Timestamp                     Temp (°F)   Humidity
  --------------------------------------------------
  2026-03-01 20:22:48.148 UTC       86.9°      27.4%
  2026-03-01 20:22:38.121 UTC       96.4°      52.6%
  2026-03-01 20:22:28.105 UTC       98.4°      48.0%
  2026-03-01 20:22:18.081 UTC       97.9°      46.3%
```

```
$ quailsync-cli system
System Metrics

  CPU:     62.2%
  Memory:  1447 / 4096 MB (35.4%)
  Disk:    31 / 50 GB (63.7%)
  Uptime:  101h 24m
```

```
$ quailsync-cli alerts
Alerts — Last 60 Minutes (2 total)

  2026-03-01 20:22:48 [WARN] Humidity LOW: 27.4% (min 40.0%)
  2026-03-01 20:22:48 [CRIT] Temperature LOW: 86.9°F (min 95.0°F, 8.1°F below)
```

## Roadmap

- [ ] **DHT22 sensor integration** -- Replace mock data with real readings from DHT22 temperature/humidity sensors via GPIO
- [ ] **Detection event pipeline** -- Camera-based species detection with classification model integration
- [ ] **Configurable alerts via config file** -- Load alert thresholds from a TOML/JSON config file instead of compile-time defaults
- [ ] **Web dashboard** -- Browser-based UI with live charts, alert history, and system overview
