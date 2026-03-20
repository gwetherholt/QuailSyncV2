# QuailSync — IoT Quail Lifecycle Management Platform

Full-stack IoT platform for managing coturnix quail breeding operations. Tracks eggs from incubation through hatching, brooding, banding, breeding, and processing. Real-time environmental monitoring with smart alerts, NFC-based bird identification, and a native Android app for mobile management.

<img width="1180" height="702" alt="QuailSync Dashboard" src="https://github.com/user-attachments/assets/b1afa4fc-961c-413b-b3ba-00821f493f5e" />

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
 │  (kernel IIO)       │      │   │  /ws       │  │          │  │  smart temp   │   │      │  Live MJPEG feed  │
 │                     │      │   │  /ws/live  │  │          │  │  scheduling   │   │ HTTPS│  Full mgmt UI     │
 │  ArduCam ► camera_  │ HTTP │   └────────────┘  └──────────┘  └───────────────┘   │      └───────────────────┘
 │  Module 3  stream   ├─────►│                                                      │
 │                     │      │   ┌────────────┐  ┌──────────┐  ┌───────────────┐   │      ┌───────────────────┐
 │  QR Scanner (pyzbar)│      │   │   SQLite   │  │   TLS    │  │  rust-embed   │   │      │  Android App      │
 └─────────────────────┘      │   │  quailsync │  │  :3443   │  │  dashboard    │   │ HTTPS│  Kotlin / Compose │
                              │   │   .db      │  │  rcgen   │  │  baked into   │   ├─────►│  NFC R/W          │
                              │   └────────────┘  └──────────┘  │  the binary   │   │      │  Live temps       │
                              │                                 └───────────────┘   │      │  MJPEG feeds      │
                              └──────────────────────────────────────────────────────┘      └───────────────────┘
```

**Rust/Axum Server** — REST API + WebSocket hub running on Windows (moving to Pi soon). SQLite database. Handles telemetry ingestion, alert engine, breeding genetics calculations, and serves the web dashboard as a single HTML file baked into the binary with `rust-embed`.

**Raspberry Pi 5 Sensor Agent** — Python agent reading 3x DHT22 temperature/humidity sensors via kernel IIO driver (`dht11` dtoverlay). Sends telemetry to server over WebSocket every 5 seconds. Runs as systemd service (`quailsync-sensors`).

**Raspberry Pi 5 Camera Stream** — MJPEG camera stream using ArduCam Module 3 (IMX708). Auto-registers with the server on startup. Supports QR code scanning for brooder identification. Runs as systemd service (`quailsync-camera`).

**Android App (Kotlin/Jetpack Compose)** — Native app with 5 tabs: Dashboard (live temps with smart age-based targets), Cameras (MJPEG feeds), Flock (bird profiles with photos), NFC (tag scanning/writing + batch graduation workflow), Clutches (incubation tracking with progress rings).

**Web Dashboard** — Single-page vanilla JS dashboard with real-time WebSocket updates. Full management UI for all entities — brooders, flock, breeding, clutches, processing, cameras.

---

## Key Features

### Smart Temperature Scheduling
Auto-adjusts alert thresholds by chick age — 95°F in week 1, stepping down 5°F per week to room temp by week 6. Three DHT22 sensors (one per brooder) feed readings every 5 seconds. The alert engine evaluates every reading against age-appropriate targets and fires warnings or critical alerts immediately. The dashboard updates in real-time via WebSocket without polling.

### NFC Bird Identification
Every bird gets an NTAG215 NFC tag on its leg band, written with a `QUAIL-XXXXXX` identifier. The Android app reads/writes tags natively. Tapping a tagged bird opens its full profile — weight history, lineage, breeding group, notes. Overwrite protection prevents accidentally reassigning tags.

<table>
  <tr>
    <td><img width="306" alt="NFC pull up" src="https://github.com/user-attachments/assets/91da687c-a133-404b-9785-101893f89e02" /></td>
    <td><img width="306" alt="NFC page" src="https://github.com/user-attachments/assets/5a79a0b4-2a87-411a-a610-541f092bba58" /></td>
  </tr>
</table>

### Batch Graduation Workflow
Per-bird sex selection, photo capture, weight logging, and NFC tag writing — all in one flow on the Android app. Graduate chick groups from the nursery into the main flock with full traceability.

### Live Camera Streaming
<img width="606" height="531" alt="Live camera" src="https://github.com/user-attachments/assets/58c60717-4dea-4419-b523-cf14c7f0b3f2" />

ArduCam Module 3 (IMX708) streams MJPEG at 640x480. QR codes on each brooder box enable automatic camera-to-brooder association — the camera scans every frame with `pyzbar`, requires 3 consecutive matching detections before committing, and sends a `CameraAssign` message to the server. Camera auto-registers its stream URL on boot.

### Breeding Genetics
<img width="1187" height="483" alt="Flock management" src="https://github.com/user-attachments/assets/6460b755-756c-4e41-aa97-1d175ea0f4e4" />
<img width="1164" height="413" alt="Breeding page" src="https://github.com/user-attachments/assets/dfed3688-052f-4f46-bbd6-6f3275c33e3e" />

Inbreeding coefficient calculation for every possible male-female pairing. Flags anything above 0.0625 as risky. Breeding groups enforce 3-to-5 females-per-male ratio. Safe pairing suggestions scored by genetics.

### Clutch & Incubation Tracking
<img width="1185" height="494" alt="Clutches page" src="https://github.com/user-attachments/assets/3f63f6fb-fe06-4c97-8ec3-784a46e33a5d" />

Automatic 17-day hatch date calculation, visual progress rings color-coded by stage, candling records, and detailed hatch outcome logging — eggs hatched, stillborn, quit, infertile, damaged. Horizontal timeline of all active incubations.

### Processing Pipeline
Cull recommendations based on flock criteria, scheduling, and kanban tracking (Recommended → Scheduled → Completed).

### Background Notifications
Temperature alerts, sensor offline detection, and hatch countdown — delivered as Android notifications.

---

## Hardware

| Component | Details |
|---|---|
| Raspberry Pi 5 | 8GB, runs sensor agent + camera stream as systemd services |
| DHT22 sensors | x3, one per brooder, kernel IIO driver on GPIO 4, 17, 27 |
| ArduCam Module 3 | IMX708 sensor, MJPEG streaming |
| NFC tags | NTAG215 on leg bands, read/written via Android NFC |
| Nurture Right 360 | Incubator for coturnix quail eggs (17-day hatch) |
| Brooder enclosures | x3, each with QR code, DHT22 sensor, and camera coverage |

### Sensor Wiring (Pi 5 GPIO)

| Sensor | VCC | DATA | GND |
|---|---|---|---|
| Brooder 1 (Texas) | Pin 1 (3.3V) | Pin 7 (GPIO4) | Pin 9 |
| Brooder 2 (Pharaoh) | Pin 1 (3.3V) | Pin 11 (GPIO17) | Pin 9 |
| Brooder 3 (Fernbank) | Pin 17 (3.3V) | Pin 13 (GPIO27) | Pin 9 |

### Pi Setup

Add to `/boot/firmware/config.txt` under `[all]`:

```
dtoverlay=dht11,gpiopin=4
dtoverlay=dht11,gpiopin=17
dtoverlay=dht11,gpiopin=27
```

This creates IIO devices at `/sys/bus/iio/devices/iio:device{0,1,2}/` with `in_temp_input` (millidegrees C) and `in_humidityrelative_input` (millipercent) sysfs files.

Systemd services: `quailsync-sensors`, `quailsync-camera`

---

## Tech Stack

| Layer | Technology |
|---|---|
| Server | Rust, Axum 0.8, SQLite (rusqlite), Tokio, rust-embed |
| Web Dashboard | Vanilla JS single-file SPA, WebSocket, hash-based routing |
| Android App | Kotlin, Jetpack Compose, NFC/NDEF, MJPEG, background notifications |
| Pi Sensor Agent | Python 3, Linux kernel IIO drivers, websockets, psutil |
| Pi Camera | Python 3, picamera2, pyzbar, Pillow |
| Deployment | Self-signed TLS via rcgen, HTTP :3000 + HTTPS :3443 |

---

## Project Structure

```
QuailSyncV2/
├── Cargo.toml                        # Workspace root
├── crates/
│   ├── quailsync-common/             # Shared types, constants, serde models
│   │   └── src/lib.rs
│   ├── quailsync-server/             # Axum web server + REST API + WebSocket hub
│   │   ├── src/lib.rs                #   All routes, DB schema, alert engine
│   │   ├── src/main.rs               #   Startup, TLS cert gen, dual HTTP/HTTPS listeners
│   │   └── tests/api_tests.rs        #   Integration tests
│   ├── quailsync-agent/              # Mock Rust agent (dev/testing — generates fake telemetry)
│   │   └── src/main.rs
│   └── quailsync-cli/                # Full CLI — clap-based, colored output, QR gen
│       └── src/main.rs
├── dashboard/
│   └── index.html                    # Single-file SPA (HTML + CSS + JS, no build step)
├── android/                          # Native Android app (Kotlin/Jetpack Compose)
├── pi-agent/
│   ├── pi_agent.py                   # Real Pi agent — DHT22 via IIO + system metrics over WS
│   ├── camera_stream.py              # MJPEG server + QR scanner + brooder auto-ID
│   └── requirements-pi.txt           # Python deps
├── CAD/
│   ├── camera_stand_v4.scad          # Parametric OpenSCAD source (current version)
│   ├── camera_stand_v4.stl           # Print-ready STL
│   ├── quailsync_backplate.stl       # Snap-on camera back plate
│   └── quailsync_stand.stl           # Stand column piece
├── certs/                            # Auto-generated self-signed TLS certs
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

TLS certificates are generated automatically on first launch. Open `http://localhost:3000` for the dashboard, or `https://localhost:3443` for NFC support (Web NFC requires HTTPS).

The mock agent is useful for development without a Pi:

```bash
cargo run --bin quailsync-agent
```

### Raspberry Pi

Copy the `pi-agent/` directory to the Pi, then install dependencies:

```bash
pip3 install websockets psutil --break-system-packages
pip3 install pyzbar pillow --break-system-packages
sudo apt install libzbar0
```

Start the sensor agent and camera stream:

```bash
# Sensor telemetry (reads DHT22 via kernel IIO driver)
python3 pi_agent.py --server ws://192.168.0.228:3000/ws

# MJPEG camera + QR scanning
python3 camera_stream.py --server ws://192.168.0.228:3000/ws --port 8080
```

The sensor agent sends brooder readings every 5 seconds and system metrics every 30 seconds. If IIO devices aren't available (testing on a desktop), it gracefully skips sensor reads and still sends system metrics.

### Android App

Build and install the Android app from the `android/` directory using Android Studio or Gradle. Requires an Android device with NFC for tag scanning/writing.

### CLI

```bash
# Check server connection
cargo run --bin quailsync-cli -- status

# Manage bloodlines and birds
cargo run --bin quailsync-cli -- bloodline add "Texas A&M" --source "Stromberg's" --notes "White Coturnix"
cargo run --bin quailsync-cli -- bird add --sex Female --bloodline 1 --band-color gold

# Breeding suggestions with inbreeding scoring
cargo run --bin quailsync-cli -- breeding suggest
```

---

## API Endpoints

### WebSocket

| Path | Direction | Description |
|---|---|---|
| `/ws` | Pi → Server | Agent telemetry — receives `Brooder`, `System`, and `Detection` payloads |
| `/ws/live` | Server → Dashboard | Broadcasts every telemetry message to connected clients |

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
| PUT | `/api/chick-groups/{id}/graduate` | Promote chicks to the main flock |
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

## Current Flock (March 2026)

| Clutch | Eggs | Set Date | Expected Hatch | Line |
|---|---|---|---|---|
| Fernbank Poultry | 21 | March 6 | ~March 23 | Texas A&M (white) |
| NWQuail | 24 | March 6 | ~March 23 | Heritage pharaoh |
| Bryants Roost | 24 | March 20 | ~April 6 | Tuxedo pattern (16 more waiting) |

---

## 3D Printed Camera Stand

The camera mount is a two-piece parametric design in OpenSCAD (`CAD/camera_stand_v4.scad`). Weighted base with coin pockets, tapered column with cable channel, and a 3-sided cradle holding the ArduCam at 3-degree downward tilt. Snap-on back plate with ribbon cable exit slots.

**Print settings:** PLA, 0.2mm layer height, 20% infill, no supports needed.

---

## Tests

```bash
cargo test
```

Unit tests cover serde roundtrips for all telemetry payload variants, alert config defaults, inbreeding coefficient thresholds, clutch status handling, and server API integration tests.

---

## Future Plans

- ESP32-C3 wireless sensor nodes (replace wired DHT22s)
- YOLO-based bird counting and sex detection from camera feeds
- Move server to Pi (eliminate Windows PC dependency)
- Second camera on Pi cam1 port
- 3D printed sensor pod redesign for ESP32 boards

---

Built with Rust, Kotlin, Python, solder, and way too many quail eggs.

[Georgia](https://github.com/gwetherholt)
