# QuailSync V2

An IoT-powered quail lifecycle management platform — real-time environmental monitoring, live camera feeds, NFC bird tagging, and computer vision detection, built in Rust.

<!-- TODO: hero image / dashboard screenshot -->

---

## Architecture

```
                              ┌──────────────────────────────────────────────────────┐
                              │              QuailSync Server                        │
                              │           Rust / Axum / SQLite                       │
 ┌─────────────────────┐      │                                                      │      ┌───────────────────┐
 │   Raspberry Pi 5    │      │   ┌────────────┐  ┌──────────┐  ┌───────────────┐   │      │   Web Dashboard   │
 │                     │ WS   │   │  WebSocket │  │   REST   │  │    Alert      │   │ WS   │                   │
 │  DHT22 ──► pi_agent ├─────►│   │    Hub     │  │   API    │  │    Engine     │   ├─────►│  Real-time temps  │
 │                     │      │   │  /ws       │  │  51 rts  │  │  temp/humid   │   │      │  Live MJPEG feed  │
 │  ArduCam ► camera_  │ HTTP │   │  /ws/live  │  │          │  │  thresholds   │   │ HTTPS│  Sparkline charts │
 │  Module 3  stream   ├─────►│   └────────────┘  └──────────┘  └───────────────┘   ├─────►│  Alert ticker     │
 │                     │      │                                                      │      │  NFC tag mgmt     │
 │  QR Scanner (pyzbar)│      │   ┌────────────┐  ┌──────────┐  ┌───────────────┐   │      │  Flock tracking   │
 │  brooder-1-texas    │      │   │   SQLite   │  │   TLS    │  │  rust-embed   │   │      └───────────────────┘
 └─────────────────────┘      │   │  quailsync │  │  :3443   │  │  dashboard    │   │
                              │   │   .db      │  │  rcgen   │  │  baked into   │   │      ┌───────────────────┐
                              │   └────────────┘  └──────────┘  │  the binary   │   │ HTTPS│  Mobile (Android) │
                              │                                 └───────────────┘   ├─────►│                   │
                              └──────────────────────────────────────────────────────┘      │  NFC tag R/W      │
                                                                                           │  Web NFC API      │
                                                                                           └───────────────────┘
```

The Pi runs two Python processes — one reading the DHT22 sensor and shipping telemetry over WebSocket, the other streaming MJPEG video and scanning for QR codes. The server receives everything, persists to SQLite, evaluates alerts, and broadcasts to any connected dashboard clients over a second WebSocket channel. The whole dashboard is a single HTML file baked into the Rust binary with `rust-embed`, so deploying the server is just copying one executable.

---

## Features

### Environmental Monitoring

Three DHT22 sensors (one per brooder box) feed temperature and humidity readings to the server every 5 seconds. The Pi agent converts the raw Celsius readings to Fahrenheit, wraps them in the same JSON envelope the Rust mock agent uses, and ships them over WebSocket. On the server side, every reading gets persisted to SQLite and run through the alert engine — if temp drifts outside 95–100°F or humidity leaves the 40–60% window, an alert fires immediately. The dashboard picks up new readings via a dedicated `/ws/live` broadcast channel and updates the brooder cards in-place without polling. Each brooder card shows the current temp, humidity, a sparkline chart of the last 20 readings, and any active alert.

### Live Camera

An ArduCam Module 3 (IMX708 sensor) connected to the Pi streams MJPEG at 640x480, roughly 10 FPS. The camera process serves the stream at `/stream` and a single-frame capture at `/snapshot`. On the dashboard, each brooder card gets a small live thumbnail that links to the full Cameras page, where you get the full-resolution feed, a snapshot button, and fullscreen mode. The camera URL is stored per-brooder in the database, so if you have multiple Pis you just set each brooder's URL from the settings gear on the Cameras page.

### QR Code Detection

Each brooder box has a printed QR code in the format `brooder-{id}-{bloodline}` (like `brooder-1-texas` or `brooder-3-fernbank`). The camera process scans every frame for QR codes using `pyzbar`, throttled to one scan every 2 seconds so it doesn't tank the frame rate. A stability filter requires 3 consecutive matching detections before it commits to a new brooder ID — this prevents phantom switches from partial reads. When the camera confirms it's looking at a different brooder, it sends a `CameraAssign` message to the server over WebSocket.

### NFC Bird Tagging

Every bird in the flock gets an NTAG215 NFC tag on its leg band, written with a `QUAIL-XXXXXX` identifier (6 random alphanumeric characters). The dashboard's NFC page uses the Web NFC API (Chrome on Android over HTTPS) to read and write tags. Tapping a tagged bird opens its full profile — weight history, lineage, breeding group, notes, everything. Writing a new tag has overwrite protection: if the tag already has a QUAIL ID assigned to another bird, the write is blocked and you get a link to that bird's profile so you can reassign it intentionally. There's also a manual lookup field for when you just want to type in the tag ID.

### Brooder Management

The system supports multiple brooders, each with a name, life stage (Chick, Adolescent, Adult), optional bloodline assignment, and camera URL. The dashboard shows all brooders in a grid with live readings, alert status, and camera thumbnails. Each brooder's QR code ties it to a physical box so the camera always knows which brooder it's pointed at.

### Flock & Breeding

Individual bird tracking with colored leg bands, sex, bloodline, hatch date, generation, parentage, and NFC tag. The breeding engine scores every possible male-female pairing by inbreeding coefficient (considering shared parents and shared bloodlines) and flags anything above 0.0625 as risky. Breeding groups enforce a 3-to-5 females-per-male ratio with warnings if you go outside the range. The processing page is a kanban board (Recommended → Scheduled → Completed) for managing culls. Chick groups track nursery batches from hatch through graduation into the main flock.

### Clutch & Incubation

Clutch tracking with automatic 17-day hatch date calculation (Coturnix), visual progress bars color-coded by stage, candling records for fertile egg counts, and detailed hatch outcome logging — eggs hatched, stillborn, quit, infertile, damaged. The clutch cards show ring-style progress indicators and a horizontal timeline of all active incubations.

### Computer Vision (In Progress)

The detection pipeline infrastructure is built — frame capture, per-frame detection result storage, camera-to-brooder association, and detection summary aggregation are all wired up in the API. The next step is training a YOLOv8 model on Roboflow to do quail counting, with sex identification and behavior detection further down the line.

---

## Tech Stack

| Layer | Technology |
|---|---|
| Server | Rust, Axum 0.8, SQLite (rusqlite), Tokio, rust-embed |
| Dashboard | Vanilla JS single-file SPA, WebSocket, hash-based routing |
| Pi Agent | Python 3, adafruit-circuitpython-dht, websockets, psutil |
| Camera | Python 3, picamera2, pyzbar, Pillow |
| NFC | Web NFC API (Chrome/Android, requires HTTPS) |
| CV Model | Roboflow, YOLOv8 (in progress) |
| 3D Prints | OpenSCAD (parametric), PLA on FDM |
| Deployment | Self-signed TLS via rcgen, HTTP :3000 + HTTPS :3443 |

---

## Hardware

| Component | Details |
|---|---|
| Raspberry Pi 5 | 8GB, running both the sensor agent and camera stream |
| ArduCam Module 3 | IMX708 sensor, connected via ribbon cable, 640x480 MJPEG |
| DHT22 sensors | x3, one per brooder, wired to GPIO4 |
| NTAG215 NFC tags | Leg band tags, written/read via Android phone |
| Artillery Sidewinder X1 | FDM printer for camera stands and mounts (PLA) |
| Elegoo Saturn 8K | Resin printer for finer detail parts |
| Brooder boxes | 3 wooden boxes, each with a QR code, DHT22 sensor, and camera |

---

## Project Structure

```
QuailSyncV2/
├── Cargo.toml                        # Workspace root — 4 crates
├── crates/
│   ├── quailsync-common/             # Shared types, constants, serde models
│   │   └── src/lib.rs                #   TelemetryPayload, Bird, Clutch, AlertConfig, etc.
│   ├── quailsync-server/             # Axum web server + REST API + WebSocket hub
│   │   ├── src/lib.rs                #   All routes, DB schema, alert engine (~2800 lines)
│   │   ├── src/main.rs               #   Startup, TLS cert gen, dual HTTP/HTTPS listeners
│   │   └── tests/api_tests.rs        #   Integration tests
│   ├── quailsync-agent/              # Mock Rust agent (dev/testing — generates fake telemetry)
│   │   └── src/main.rs
│   └── quailsync-cli/                # Full CLI — clap-based, colored output, QR gen
│       └── src/main.rs
├── dashboard/
│   └── index.html                    # Single-file SPA (HTML + CSS + JS, no build step)
├── pi-agent/
│   ├── pi_agent.py                   # Real Pi agent — DHT22 + system metrics over WebSocket
│   ├── camera_stream.py              # MJPEG server + QR scanner + brooder auto-ID
│   └── requirements-pi.txt           # Python deps
├── CAD/
│   ├── camera_stand_v4.scad          # Parametric OpenSCAD source (current version)
│   ├── camera_stand_v4.stl           # Print-ready STL
│   ├── quailsync_backplate.stl       # Snap-on camera back plate
│   └── quailsync_stand.stl           # Stand column piece
├── certs/                            # Auto-generated self-signed TLS certs
│   ├── quailsync.crt
│   └── quailsync.key
└── quailsync.db                      # SQLite database (created on first run)
```

---

## Getting Started

### Server

```bash
# Build everything
cargo build --release

# Start the server (HTTP :3000, HTTPS :3443, WebSocket on both)
cargo run --bin quailsync-server
```

TLS certificates are generated automatically on first launch. Open `http://localhost:3000` for the dashboard, or `https://localhost:3443` if you need NFC support (Web NFC requires HTTPS).

The mock agent is useful for development without a Pi:

```bash
cargo run --bin quailsync-agent
```

### Raspberry Pi

Copy the `pi-agent/` directory to the Pi (or clone the repo), then install dependencies:

```bash
pip3 install adafruit-circuitpython-dht websockets psutil --break-system-packages
pip3 install pyzbar pillow --break-system-packages
sudo apt install libzbar0
```

Start the sensor agent and camera stream:

```bash
# Terminal 1 — Temperature/humidity telemetry
python3 pi_agent.py --brooder-id 1 --server ws://192.168.0.228:3000/ws

# Terminal 2 — MJPEG camera + QR scanning
python3 camera_stream.py --server ws://192.168.0.228:3000/ws --port 8080
```

The sensor agent sends brooder readings every 5 seconds and system metrics every 30 seconds. If the DHT22 library isn't available (testing on a desktop), it gracefully skips sensor reads and still sends system metrics. The camera stream serves MJPEG at `http://<pi-ip>:8080/stream`.

### NFC Tagging

NFC requires HTTPS and Chrome on Android. Open `https://<server-ip>:3443` on your phone (accept the self-signed cert warning), navigate to the NFC page, and tap a tag against your phone. The dashboard will either pull up the bird's profile or prompt you to register a new bird with that tag.

### CLI

```bash
# Check server connection
cargo run --bin quailsync-cli -- status

# Manage bloodlines and birds
cargo run --bin quailsync-cli -- bloodline add "Texas A&M" --source "Stromberg's" --notes "White Coturnix"
cargo run --bin quailsync-cli -- bird add --sex Female --bloodline 1 --band-color gold

# Breeding suggestions with inbreeding scoring
cargo run --bin quailsync-cli -- breeding suggest

# Point at a remote server
cargo run --bin quailsync-cli -- --server http://192.168.0.228:3000 flock
```

---

## API Endpoints

### WebSocket

| Path | Direction | Description |
|---|---|---|
| `/ws` | Pi → Server | Agent telemetry — receives `Brooder`, `System`, and `Detection` payloads |
| `/ws/live` | Server → Dashboard | Broadcasts every telemetry message to connected dashboard clients |

### REST

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Health check |
| GET | `/api/status` | Agent connection status + last-seen timestamps |
| GET | `/api/alerts?minutes=N` | Recent alerts (default 60 min) |
| **Brooders** | | |
| GET | `/api/brooders` | List all brooders |
| POST | `/api/brooders` | Create a brooder |
| PUT | `/api/brooders/{id}` | Update brooder (name, camera_url, notes) |
| GET | `/api/brooders/{id}/status` | Latest reading + alert status for a brooder |
| GET | `/api/brooders/{id}/readings?minutes=N` | Reading history for a brooder |
| GET | `/api/brooder/latest` | Most recent reading (any brooder) |
| GET | `/api/brooder/history?minutes=N` | All readings in time window |
| **Birds & Flock** | | |
| GET | `/api/birds` | List all birds |
| POST | `/api/birds` | Create a bird |
| PUT | `/api/birds/{id}` | Update bird (status, notes, nfc_tag_id) |
| POST | `/api/birds/{id}/weight` | Log a weight record |
| GET | `/api/birds/{id}/weights` | Weight history for a bird |
| GET | `/api/flock/summary` | Flock stats — totals, sex counts, bloodline breakdown |
| GET | `/api/flock/cull-recommendations` | Birds flagged for processing |
| GET | `/api/nfc/{tag_id}` | Look up a bird by NFC tag |
| **Bloodlines** | | |
| GET | `/api/bloodlines` | List all bloodlines |
| POST | `/api/bloodlines` | Create a bloodline |
| **Breeding** | | |
| GET | `/api/breeding-pairs` | List breeding pairs |
| POST | `/api/breeding-pairs` | Create a breeding pair |
| GET | `/api/breeding-groups` | List breeding groups |
| POST | `/api/breeding-groups` | Create a breeding group (male + females) |
| GET | `/api/breeding-groups/{id}` | Get a breeding group with member IDs |
| GET | `/api/breeding/suggest` | Inbreeding-scored pair suggestions |
| **Clutches** | | |
| GET | `/api/clutches` | List all clutches |
| POST | `/api/clutches` | Create a clutch (auto-calculates 17-day hatch date) |
| PUT | `/api/clutches/{id}` | Update clutch (fertile count, hatch outcome, status) |
| **Nursery** | | |
| GET | `/api/chick-groups` | List chick groups |
| POST | `/api/chick-groups` | Create a chick group |
| GET | `/api/chick-groups/{id}` | Get a chick group |
| PUT | `/api/chick-groups/{id}/mortality` | Log chick losses |
| PUT | `/api/chick-groups/{id}/graduate` | Promote chicks to the main flock as individual birds |
| **Processing** | | |
| GET | `/api/processing` | List all processing records |
| POST | `/api/processing` | Schedule a bird for processing |
| GET | `/api/processing/queue` | Active queue (Scheduled only) |
| PUT | `/api/processing/{id}` | Update record (complete, cancel, add final weight) |
| **Cameras & Detection** | | |
| GET | `/api/cameras` | List cameras |
| POST | `/api/cameras` | Add a camera |
| PUT | `/api/cameras/{id}/brooder` | Link camera to a brooder |
| GET | `/api/cameras/{id}/detections/summary` | Detection counts by label (last 60 min) |
| GET | `/api/frames` | List frame captures |
| POST | `/api/frames` | Store a captured frame |
| POST | `/api/frames/{id}/detections` | Store detection results for a frame |
| **System** | | |
| GET | `/api/system/latest` | Most recent system metrics |
| POST | `/api/backup` | Create a database backup |
| GET | `/api/backups` | List available backups |
| POST | `/api/restore` | Restore from a backup |

---

## 3D Printed Camera Stand

The camera mount is a two-piece design modeled in OpenSCAD (`CAD/camera_stand_v4.scad`). The main piece is a stand with a weighted base (70x50mm footprint, two pockets for ballast coins), a tapered column, and a 3-sided cradle that holds the ArduCam Module 3 at a slight 3-degree downward tilt. A cable channel runs down the back of the column for the ribbon cable.

The second piece is a snap-on back plate that locks into four hook clips on the cradle. It has a pull tab for removal and two ribbon cable exit slots (top or bottom routing). Vent holes keep airflow moving. "QuailSync" is embossed on the base.

Everything is parametric — camera dimensions, tolerances, wall thickness, and tilt angle are all variables at the top of the `.scad` file, so adapting it to a different camera module is straightforward.

**Print settings:** PLA, 0.2mm layer height, 20% infill, no supports needed. Both pieces print flat on the bed.

---

## Tests

```bash
cargo test
```

19 unit tests cover serde roundtrips for all telemetry payload variants, alert config defaults, inbreeding coefficient thresholds, clutch status handling, and server API integration tests (spawn a test server with an in-memory DB, hit endpoints, verify responses).

---

## Roadmap

- [x] Real-time brooder monitoring (temp, humidity, sparklines, configurable thresholds)
- [x] WebSocket broadcast to dashboard (live updates without polling)
- [x] Live MJPEG camera streaming with dashboard thumbnails and fullscreen
- [x] QR code brooder auto-identification (pyzbar, stability filter)
- [x] NFC bird tagging (Web NFC API, read/write, overwrite protection)
- [x] Alert engine with Warning/Critical severity
- [x] Flock management with leg bands, weight tracking, lineage
- [x] Inbreeding-aware breeding suggestions
- [x] Clutch incubation tracking with hatch outcome breakdown
- [x] Chick nursery groups with mortality logging and graduation
- [x] Processing queue (kanban board)
- [x] 3D printed parametric camera stand
- [x] Database backup/restore
- [x] Self-signed TLS with automatic cert generation
- [x] Full CLI with QR code generation
- [ ] YOLOv8 quail detection model (Roboflow training in progress)
- [ ] Automated headcount from camera feed
- [ ] Sex identification from plumage patterns
- [ ] Behavior detection (feeding, nesting, aggression)
- [ ] Multi-camera support (multiple Pis, one per brooder)
- [ ] CSV/JSON export for flock and weight data
- [ ] Incubation temperature tracking (separate sensor inside incubator)
- [ ] Outdoor enclosure monitoring (weather station integration)

---

Built with Rust, Python, solder, and way too many quail eggs.

[Georgia](https://github.com/GeorgiaK)
