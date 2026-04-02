# QuailSync V2 🐦

**IoT-powered quail lifecycle management — from egg to adult.**

A full-stack IoT platform for managing a coturnix quail breeding operation. Real-time temperature monitoring, live camera feeds with QR detection, NFC bird tagging, hatchery tracking with fertility metrics, breeding intelligence with inbreeding analysis, and a native Android app — all built from scratch.

<img width="1180" height="702" alt="QuailSync Dashboard" src="https://github.com/user-attachments/assets/b1afa4fc-961c-413b-b3ba-00821f493f5e" />

---

## The Story

QuailSync started as a "how hard can it be" weekend project to monitor brooder temperatures with a Raspberry Pi. It turned into something much bigger.

During our first real hatch — 45 coturnix eggs across two incubators — I woke up at 2am to a critical alert on my phone. QuailSync had detected Brooder 2 dropping below 60°F while the chicks were only 3 days old. I went out to check and found a corroded connection on the heating panel. The wire insulation had melted and was starting to discolor the plastic housing. If it had gone unnoticed for another few hours, those chicks would have died from cold stress. If it had gone a few days, it could have been a fire.

That was the moment QuailSync stopped being a hobby project and became something I actually depend on. Every feature since then — the age-based temperature scheduling, the Android alerts, the camera feeds — came from a real need on the farm.

<!-- TODO: Screenshot of the 2am critical alert notification -->

---

## Architecture

```
  ┌──────────────┐         ┌──────────────┐
  │  ESP32-C3    │         │  Pi Camera   │
  │  DHT22       │         │  ArduCam     │
  │  sensors     │         │  IMX477 HQ   │
  └──────┬───────┘         └──────┬───────┘
         │ WebSocket              │ MJPEG :8080
         ▼                        ▼
  ┌─────────────────────────────────────────┐
  │         Raspberry Pi 5                  │
  │                                         │
  │   pi_agent.py        camera_stream.py   │
  │   (telemetry)        (stream + QR scan) │
  └──────────┬──────────────────────────────┘
             │ WebSocket :3000/ws
             ▼
  ┌─────────────────────────────────────────┐
  │        Rust/Axum Server (Docker)        │
  │                                         │
  │  REST API ◄──► SQLite ◄──► Alert Engine │
  │  WebSocket Hub    │     Temp Scheduling  │
  │  rust-embed SPA   │     Breeding Calc    │
  └────────┬──────────┼────────┬────────────┘
           │          │        │
     WebSocket    Dashboard   REST + MJPEG
     /ws/live     index.html
           │                   │
           ▼                   ▼
  ┌────────────────┐  ┌────────────────────┐
  │ Web Dashboard  │  │  Android App       │
  │ Vanilla JS SPA │  │  Kotlin / Compose  │
  │ Real-time WS   │  │  NFC tagging       │
  │ Sparkline      │  │  Live temps        │
  │ charts         │  │  QR scanner        │
  └────────────────┘  │  Background alerts │
                      └────────────────────┘
```

---

## Features

### Real-Time Telemetry Dashboard
Live temperature and humidity from each brooder, updated every 5 seconds over WebSocket. Sparkline charts show trends. Status dots go green/yellow/red based on age-appropriate thresholds — week 1 chicks need 97°F, week 6 needs 72°F, and the system knows the difference.

<!-- TODO: Dashboard screenshot with sparklines -->

### Live Camera with QR Overlay
MJPEG streaming from Arducam IMX477 via the Pi. Multi-client support — dashboard, phone, and browser can all watch simultaneously. QR codes on each brooder box are automatically detected with pyzbar; green bounding boxes are drawn with OpenCV when a code is in frame.

<img width="606" height="531" alt="Live camera" src="https://github.com/user-attachments/assets/58c60717-4dea-4419-b523-cf14c7f0b3f2" />

### NFC Bird Tagging
Every bird gets an NTAG215 NFC tag on its leg band. Tap the phone to a bird, get its full profile — weight history, lineage, breeding group, notes. Batch graduation workflow lets you tag an entire chick group in one session.

<table>
  <tr>
    <td><img width="306" alt="NFC pull up" src="https://github.com/user-attachments/assets/91da687c-a133-404b-9785-101893f89e02" /></td>
    <td><img width="306" alt="NFC page" src="https://github.com/user-attachments/assets/5a79a0b4-2a87-411a-a610-541f092bba58" /></td>
  </tr>
</table>

### Hatchery Tracking
17-day incubation timeline with visual progress rings. Candling records, hatch outcome logging — eggs hatched, stillborn, quit, infertile, damaged. Fertility rate and hatch rate displayed prominently with color coding. Android push notifications at key milestones (day 7 candle, day 14 lockdown, hatch day).

<img width="1185" height="494" alt="Clutches page" src="https://github.com/user-attachments/assets/3f63f6fb-fe06-4c97-8ec3-784a46e33a5d" />

### Nursery with Graduation
Chick groups track mortality daily. When birds are old enough (28 days), graduate them to the main flock with per-bird sex selection, NFC tag writing, and weight logging — all in one flow on the Android app.

### Breeding Intelligence
Inbreeding coefficient calculated for every possible male-female pairing. Flags anything above 6.25% as risky. Breeding groups enforce 3-to-5 females per male. Safe pairing suggestions scored by genetic distance across bloodlines.

<img width="1164" height="413" alt="Breeding page" src="https://github.com/user-attachments/assets/dfed3688-052f-4f46-bbd6-6f3275c33e3e" />

### Processing Pipeline
Cull recommendations based on excess male ratio, underweight birds, and inbreeding risk. Batch cull operations update multiple birds in one call. Kanban tracking from recommended through scheduled to completed.

### Temperature Scheduling by Chick Age
The alert engine automatically adjusts thresholds based on the youngest chick group in each brooder. Week 1: 97°F target. Steps down 5°F per week until week 6 when they're at room temperature. No manual threshold management needed.

---

## Tech Stack

| Layer | Technology |
|---|---|
| Server | Rust, Axum 0.8, SQLite (rusqlite), Tokio, rust-embed |
| Web Dashboard | Vanilla JS single-file SPA, WebSocket, hash-based routing, no build step |
| Android App | Kotlin, Jetpack Compose, ML Kit barcode scanning, CameraX, NFC/NDEF |
| Pi Sensor Agent | Python 3, Linux kernel IIO drivers (DHT22), websockets, psutil |
| Pi Camera | Python 3, picamera2, pyzbar, OpenCV, ThreadingHTTPServer |
| ESP32 Nodes | ESP32-C3 Super Mini, DHT22, Arduino framework, WebSocket client |
| CI/CD | GitHub Actions — `cargo fmt`, `clippy`, `cargo test` |
| Deployment | Docker Compose (server), systemd (cameras), Arduino IDE (ESP32) |

---

## Hardware

| Component | Details |
|---|---|
| Raspberry Pi 5 (8GB) | Runs server (Docker), sensor agent + camera stream (systemd) |
| ESP32-C3 Super Mini | Wireless DHT22 sensor nodes, one per brooder |
| Arducam IMX477 HQ Camera | 12.3MP Sony sensor, 6mm CS-mount lens, MJPEG streaming |
| DHT22 Sensors | Temperature + humidity, one per brooder (3 wired + ESP32 wireless) |
| XH-W3002 Controller | Backup thermostat, hardware failsafe independent of QuailSync |
| NFC Tags | NTAG215 on leg bands, read/written via Android |
| Nurture Right 360 | Incubator for coturnix quail eggs (17-day hatch cycle) |

### 3D Printed Camera Stand

Two-piece parametric design in OpenSCAD (`CAD/camera_stand_v4.scad`). Weighted base with coin pockets, tapered column with cable channel, 3-sided cradle holding the Arducam at a 3-degree downward tilt. Snap-on back plate with ribbon cable exit slots.

**Print settings:** PLA, 0.2mm layer height, 20% infill, no supports needed.

<!-- TODO: Photo of printed camera stand -->

---

## Project Structure

```
QuailSyncV2/
├── crates/
│   ├── quailsync-server/        # Axum REST API + WebSocket + alert engine
│   │   ├── src/
│   │   │   ├── lib.rs           # Router setup, static file handler
│   │   │   ├── main.rs          # Startup, TLS, dual HTTP/HTTPS
│   │   │   ├── routes/          # Modular route handlers
│   │   │   ├── db/              # Schema, migrations, helpers
│   │   │   ├── ws.rs            # WebSocket telemetry + broadcast
│   │   │   ├── state.rs         # AppState, sensor tracking
│   │   │   └── alerts.rs        # Temperature alert engine
│   │   └── tests/               # Integration tests
│   ├── quailsync-common/        # Shared types, enums, constants
│   ├── quailsync-agent/         # Mock agent for dev (fake telemetry)
│   └── quailsync-cli/           # CLI tool — flock mgmt, QR generation
├── dashboard/
│   └── index.html               # Single-file SPA (HTML + CSS + JS)
├── android/                     # Kotlin/Jetpack Compose native app
├── pi-agent/
│   ├── pi_agent.py              # DHT22 sensor agent (IIO + WebSocket)
│   ├── camera_stream.py         # MJPEG + QR scanner + multi-client
│   └── requirements-pi.txt
├── hardware/
│   └── esp32/                   # ESP32-C3 sensor firmware (Arduino)
├── CAD/
│   ├── camera_stand_v4.scad     # Parametric OpenSCAD source
│   └── *.stl                    # Print-ready meshes
├── deploy/
│   └── quailsync-camera.service # systemd unit for camera stream
├── docker-compose.yml           # Server deployment
└── brooder-*-qr.svg             # Pre-generated QR codes per brooder
```

---

## Setup & Deployment

### Server (Docker on Pi)

```bash
docker compose up -d --build
```

The server runs on port 3000 (HTTP) and 3443 (HTTPS with auto-generated self-signed certs). The SQLite database is created on first run.

### Camera (systemd on Pi)

Cameras run as systemd services directly on the Pi — not in Docker, because the Pi camera driver needs direct hardware access.

```bash
sudo cp deploy/quailsync-camera.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now quailsync-camera
```

### ESP32 Sensor Nodes

Flash via Arduino IDE. Set the WebSocket server URL and brooder ID in the firmware. Each ESP32-C3 Super Mini reads its DHT22 and sends telemetry every 5 seconds.

### Android App

Build from the `android/` directory in Android Studio. Requires a device with NFC for tag scanning. The server URL is configurable in Settings (defaults to `192.168.0.114:3000`).

### Deployment Workflow

My actual workflow: edit locally with Claude Code, `git push`, then on the Pi:

```bash
cd ~/quailsync && git reset --hard origin/main && docker compose up -d --build
```

Camera service picks up changes with `sudo systemctl restart quailsync-camera`.

---

## Testing

```bash
cargo fmt --check    # Formatting
cargo clippy         # Lints
cargo test           # Unit + integration tests
```

CI runs all three on every push via GitHub Actions. Tests cover serde roundtrips for all telemetry payloads, alert threshold logic, inbreeding coefficient calculations, clutch status handling, and API integration tests.

---

## API Endpoints

### WebSocket

| Path | Description |
|---|---|
| `/ws` | Agent telemetry ingestion (Brooder, System, Detection, CameraAnnounce, QrDetected) |
| `/ws/live` | Live broadcast to dashboard/app clients |

### REST

| Method | Path | Description |
|---|---|---|
| GET | `/api/brooders` | List brooders with latest readings |
| POST | `/api/brooders` | Create brooder |
| PUT | `/api/brooders/{id}` | Update brooder (name, camera_url, qr_code, bloodline_id) |
| DELETE | `/api/brooders/{id}` | Delete brooder + all readings |
| GET | `/api/brooders/{id}/status` | Current reading + alert state |
| GET | `/api/brooders/{id}/readings` | Historical readings |
| GET | `/api/brooders/{id}/target-temp` | Age-based temperature schedule |
| GET | `/api/brooders/{id}/alerts` | Per-brooder alerts |
| GET | `/api/birds` | List all birds |
| POST | `/api/birds` | Create bird |
| PUT | `/api/birds/{id}` | Update bird |
| DELETE | `/api/birds/{id}` | Delete bird |
| POST | `/api/birds/{id}/weights` | Log weight |
| GET | `/api/birds/{id}/weights` | Weight history |
| GET | `/api/nfc/{tag_id}` | Look up bird by NFC tag |
| GET | `/api/bloodlines` | List bloodlines |
| POST | `/api/bloodlines` | Create bloodline |
| GET | `/api/breeding-groups` | List breeding groups |
| POST | `/api/breeding-groups` | Create group (validates male:female ratio) |
| GET | `/api/breeding/suggest` | Inbreeding-scored pair suggestions |
| GET | `/api/inbreeding-check` | Check single male-female pair |
| GET | `/api/clutches` | List clutches |
| POST | `/api/clutches` | Create clutch (auto 17-day hatch date) |
| PUT | `/api/clutches/{id}` | Update (candling, hatch outcome) |
| GET | `/api/chick-groups` | List chick groups |
| POST | `/api/chick-groups` | Create chick group |
| POST | `/api/chick-groups/{id}/mortality` | Log chick losses |
| POST | `/api/chick-groups/{id}/graduate` | Promote to main flock |
| GET | `/api/flock/summary` | Flock statistics |
| GET | `/api/flock/cull-recommendations` | Birds flagged for processing |
| POST | `/api/cull-batch` | Batch cull operation |
| GET | `/api/processing` | Processing records |
| POST | `/api/processing` | Schedule processing |
| GET | `/api/cameras` | List cameras |
| POST | `/api/cameras` | Add camera |
| POST | `/api/backup` | Create database backup |
| GET | `/api/backups` | List backups |
| POST | `/api/restore` | Restore from backup |

---

## Roadmap

### Done
- [x] Real-time temperature & humidity monitoring
- [x] Live MJPEG camera streaming (multi-client)
- [x] QR code detection with OpenCV overlay
- [x] NFC bird tagging with batch graduation
- [x] WebSocket telemetry broadcast
- [x] Age-based temperature alert engine
- [x] Hatchery tracking with fertility/hatch rate metrics
- [x] Breeding intelligence with inbreeding coefficients
- [x] Nursery management with mortality logging
- [x] Processing pipeline with batch cull
- [x] ESP32-C3 wireless sensor nodes
- [x] Native Android app (Compose)
- [x] 3D printed camera hardware
- [x] Docker Compose deployment
- [x] systemd service for cameras
- [x] CI pipeline (fmt, clippy, test)
- [x] Brooder deletion with cascade cleanup
- [x] Configurable server URL (Android Settings)

### Planned
- [ ] YOLOv8 quail detection from camera feeds
- [ ] Automated headcount from video
- [ ] Male/female classification by plumage
- [ ] Behavior anomaly detection
- [ ] Weight estimation from camera (no scale needed)
- [ ] Historical data export (CSV/JSON)
- [ ] Multi-Pi support (separate sensor clusters)
- [ ] Second camera on Pi cam1 port

---

## License

Personal project by [Georgia Wetherholt](https://github.com/gwetherholt).

---

*Built with Rust, Python, solder, and way too many quail eggs.*
