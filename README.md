# QuailSync

A cozy, all-in-one quail farm management system built in Rust. Monitors brooder conditions in real time, tracks your entire flock with colored leg bands, manages clutch incubation from set to hatch, and even helps you pick the safest breeding pairs so your covey stays healthy and happy.

## Architecture

QuailSync is a Rust workspace with four crates and an embedded web dashboard:

```
┌─────────────┐   WebSocket    ┌─────────────────┐   REST API   ┌──────────────┐
│  quailsync  │───────────────>│   quailsync     │<────────────│  quailsync   │
│    agent    │  telemetry     │     server      │  queries    │     cli      │
│             │  (JSON)        │                 │             │              │
│  Collects   │                │  Axum + SQLite  │             │  clap-based  │
│  sensor &   │                │  Alert engine   │             │  dashboard   │
│  system data│                │  REST endpoints │             │              │
└─────────────┘                │  Web dashboard  │             └──────────────┘
                               └─────────────────┘
                                       │
                                  embedded SPA
                               ┌─────────────────┐
                               │    dashboard/    │
                               │   index.html     │
                               │  (sidebar nav,   │
                               │   hash router)   │
                               └─────────────────┘
```

| Crate | Type | Description |
|---|---|---|
| `quailsync-common` | Library | Shared types: telemetry payloads, flock models, alert config, breeding coefficients |
| `quailsync-agent` | Binary | Collects brooder readings and system metrics, streams to the server over WebSocket |
| `quailsync-server` | Binary | Receives telemetry, stores in SQLite, evaluates alerts, serves REST API + web dashboard |
| `quailsync-cli` | Binary | Full CLI for managing birds, bloodlines, clutches, breeding, and viewing telemetry |

## Features

### Brooder Monitoring
- **Real-time telemetry** -- Agent streams brooder temperature/humidity and system metrics over WebSocket every 5 seconds
- **Persistent storage** -- All readings stored in SQLite with automatic table creation
- **Configurable alert engine** -- Threshold-based alerts for temperature and humidity with Warning/Critical severity levels
- **Zero-config startup** -- Sensible defaults for all thresholds (95-100 F temp, 40-60% humidity)

### Flock Management
- **Individual bird tracking** -- Register birds with colored leg bands, sex, bloodline, hatch date, generation, and parentage
- **Bloodline registry** -- Track breeding lines with name, source, and notes
- **Flock summary** -- At-a-glance stats: total/active birds, male/female counts, birds per bloodline
- **Status lifecycle** -- Mark birds as Active, Culled, Deceased, or Sold

### Clutch & Incubation
- **Clutch tracking** -- Log clutches with egg counts, set dates, and automatic 17-day hatch date calculation
- **Incubation progress** -- Visual progress bars color-coded by stage (green early, orange mid, dusty rose late)
- **Candling records** -- Update fertile egg counts during incubation
- **Hatch recording** -- Log final hatch counts and mark clutches as Hatched or Failed

### Breeding Intelligence
- **Inbreeding-aware pair suggestions** -- Automatically scores every possible male/female pairing by relatedness
- **Coefficient calculation** -- Considers shared parents and shared bloodlines to compute inbreeding risk
- **Safe/risky classification** -- Pairs below 0.0625 coefficient are flagged safe; above is risky

### Web Dashboard
- **Multi-page SPA** -- Sidebar navigation with hash-based routing between Dashboard, Flock, and Clutches views
- **Dashboard overview** -- Live brooder sparklines, alert feed, flock summary, and breeding pair suggestions
- **Flock management page** -- Filterable bird table (by status, sex, bloodline), Add Bird modal, inline status updates
- **Clutch tracker page** -- Clutch cards with progress bars, inline candling/hatch forms, incubation timeline
- **Embedded in the server** -- No separate frontend build step; the HTML is compiled right into the binary with `rust-embed`
- **Responsive** -- Sidebar collapses on mobile with hamburger toggle

### CLI
- **Full management suite** -- `status`, `brood`, `system`, `alerts`, `bloodline`, `bird`, `flock`, `clutch`, `breeding` subcommands
- **Colored terminal output** -- Status indicators, reading tables, and alert history with ANSI colors
- **Remote server support** -- Point the CLI at any QuailSync server with `--server`

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.75+)

### Build

```bash
cargo build --release
```

### Run

Start the server, then the agent:

```bash
# Terminal 1 -- Start the server
cargo run --bin quailsync-server

# Terminal 2 -- Start the agent (streams mock telemetry)
cargo run --bin quailsync-agent
```

Open `http://localhost:3000` in your browser for the web dashboard.

Or use the CLI:

```bash
# Connection status
cargo run --bin quailsync-cli -- status

# Brooder readings
cargo run --bin quailsync-cli -- brood
cargo run --bin quailsync-cli -- brood --history 60

# Flock management
cargo run --bin quailsync-cli -- bloodline add "Golden" --source "Breeder Co" --notes "Gorgeous line"
cargo run --bin quailsync-cli -- bird add --sex Female --bloodline 1 --band-color gold
cargo run --bin quailsync-cli -- flock

# Clutch tracking
cargo run --bin quailsync-cli -- clutch add --bloodline 1 --eggs 14
cargo run --bin quailsync-cli -- clutch list

# Breeding suggestions
cargo run --bin quailsync-cli -- breeding suggest

# System metrics & alerts
cargo run --bin quailsync-cli -- system
cargo run --bin quailsync-cli -- alerts
```

Point the CLI at a different server with `--server`:

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
| GET | `/api/bloodlines` | List all bloodlines |
| POST | `/api/bloodlines` | Create a bloodline |
| GET | `/api/birds` | List all birds |
| POST | `/api/birds` | Create a bird |
| PUT | `/api/birds/{id}` | Update a bird (status, notes) |
| GET | `/api/breeding-pairs` | List breeding pairs |
| POST | `/api/breeding-pairs` | Create a breeding pair |
| GET | `/api/clutches` | List all clutches |
| POST | `/api/clutches` | Create a clutch |
| PUT | `/api/clutches/{id}` | Update a clutch (fertile count, hatched count, status) |
| GET | `/api/flock/summary` | Flock stats: totals, sex counts, bloodline breakdown |
| GET | `/api/breeding/suggest` | Inbreeding-aware breeding pair suggestions |

## Tests

19 tests covering telemetry serde roundtrips, alert config defaults, inbreeding coefficients, clutch status handling, and server API integration:

```bash
cargo test
```

## Roadmap

- [x] **Web dashboard** -- Multi-page SPA with live brooder sparklines, alerts, flock overview, and breeding suggestions
- [x] **Flock management UI** -- Add, edit, and track individual birds with colored band identification
- [x] **Clutch tracker UI** -- Visual incubation progress, candling records, and hatch recording from the browser
- [x] **Inbreeding-aware breeding engine** -- Automatic pair scoring with safe/risky classification
- [x] **Bloodline tracking** -- Full CRUD for breeding lines with per-bird lineage
- [ ] **NFC band scanning** -- Scan a quail's leg band to pull up its full profile, lineage, and breeding history
- [ ] **DHT22 sensor integration** -- Replace mock data with real readings from DHT22 temperature/humidity sensors via GPIO
- [ ] **Detection event pipeline** -- Camera-based species detection with classification model integration
- [ ] **Configurable alerts via config file** -- Load alert thresholds from a TOML/JSON config file instead of compile-time defaults
- [ ] **Multi-agent support** -- Connect multiple brooder agents to a single server with per-agent dashboards

---

*Built with love (and a lot of quail eggs) by Georgia*
