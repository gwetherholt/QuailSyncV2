# QuailSync V2 🐦


**IoT-powered quail lifecycle management — from egg to adult.**

A full-stack IoT platform for managing a coturnix quail breeding operation. Real-time temperature monitoring, live camera feeds with QR detection, NFC bird tagging, hatchery tracking with fertility metrics, breeding intelligence with inbreeding analysis, and a native Android app — all built from scratch.
<p align="center">
<img width="200" alt="QuailSync Logo" src="https://github.com/user-attachments/assets/1270090b-1e2f-43c4-87a7-5a7b1bb22bd8" />
</p>

<p align="center">
  <img width="700" alt="QuailSync Dashboard" src="https://github.com/user-attachments/assets/b1afa4fc-961c-413b-b3ba-00821f493f5e" />
</p>
<p align="center">
  <img width="700" alt="QuailSync Dashboard Detail" src="https://github.com/user-attachments/assets/2f48c9ab-fce4-4728-ac91-e3f911cff6fe" />
</p>

---

## The Story

QuailSync started as a "how hard can it be" weekend project to monitor brooder temperatures with a Raspberry Pi. It turned into something much bigger.

<p align="center">
  <img width="300" alt="Quail chick" src="https://github.com/user-attachments/assets/ea72dbf2-f348-49cb-a22d-fa6431ba335e" />
</p>

During our first real hatch — 45 coturnix eggs across two incubators — I woke up at 2am to a critical alert on my phone. QuailSync had detected Brooder 2 dropping below 60°F while the chicks were only 3 days old. I went out to check and found a corroded connection on the heating panel. The wire insulation had melted and was starting to discolor the plastic housing. If it had gone unnoticed for another few hours, those chicks would have died from cold stress. If it had gone a few days, it could have been a fire.

<p align="center">
  <img width="220" alt="Critical alert notification" src="https://github.com/user-attachments/assets/60a7a6ac-67bb-42d3-944a-07144c740025" />
</p>

That was the moment QuailSync stopped being a hobby project and became something I actually depend on. Every feature since then — the age-based temperature scheduling, the Android alerts, the camera feeds — came from a real need on the farm.

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

<p align="center">
  <img width="300" alt="Temperature scheduling by age" src="https://github.com/user-attachments/assets/8a153088-bc53-4a57-851e-1444c1fabce2" />
</p>

### Live Camera with QR Overlay

MJPEG streaming from Arducam IMX477 via the Pi. Multi-client support — dashboard, phone, and browser can all watch simultaneously. QR codes on each brooder box are automatically detected with pyzbar; green bounding boxes are drawn with OpenCV when a code is in frame.

<p align="center">
  <img width="500" alt="Camera with QR overlay" src="https://github.com/user-attachments/assets/ec1e1e32-44b6-4924-ba8c-ba647196bb58" />
</p>

### NFC Bird Tagging

Every bird gets an NTAG215 NFC tag on its leg band. Tap the phone to a bird, get its full profile — weight history, lineage, breeding group, notes. Batch graduation workflow lets you tag an entire chick group in one session.

<table>
  <tr>
    <td><img width="280" alt="NFC pull up" src="https://github.com/user-attachments/assets/91da687c-a133-404b-9785-101893f89e02" /></td>
    <td><img width="280" alt="NFC page" src="https://github.com/user-attachments/assets/5a79a0b4-2a87-411a-a610-541f092bba58" /></td>
    <td><img width="280" alt="NFC graduation flow" src="https://github.com/user-attachments/assets/d9fef58a-cb39-4b81-a852-37ab8cd271b9" /></td>
  </tr>
  <tr>
    <td align="center"><em>Tap to pull up bird</em></td>
    <td align="center"><em>NFC tag management</em></td>
    <td align="center"><em>Batch graduation</em></td>
  </tr>
</table>

### Hatchery Tracking

17-day incubation timeline with visual progress rings. Candling records, hatch outcome logging — eggs hatched, stillborn, quit, infertile, damaged. Fertility rate and hatch rate displayed prominently with color coding. Android push notifications at key milestones (day 7 candle, day 14 lockdown, hatch day).

<table>
  <tr>
    <td><img width="300" alt="Clutch progress rings" src="https://github.com/user-attachments/assets/bc159339-9b8f-4770-967f-f7af726c1a81" /></td>
    <td><img width="300" alt="Hatch outcomes" src="https://github.com/user-attachments/assets/45f6c4e7-e68f-44f1-9980-b3ae678c1017" /></td>
  </tr>
  <tr>
    <td align="center"><em>Incubation progress</em></td>
    <td align="center"><em>Hatch outcome tracking</em></td>
  </tr>
</table>

### Nursery with Graduation

Chick groups track mortality daily. When birds are old enough (28 days), graduate them to the main flock with per-bird sex selection, NFC tag writing, and weight logging — all in one flow on the Android app.

<table>
  <tr>
    <td><img width="300" alt="Nursery chick groups" src="https://github.com/user-attachments/assets/622bd568-780f-4878-8d8d-b76dced49bea" /></td>
    <td><img width="300" alt="Graduation workflow" src="https://github.com/user-attachments/assets/c44d33ac-1b8c-4d58-80e1-0725b7dd5ab5" /></td>
  </tr>
  <tr>
    <td align="center"><em>Chick group tracking</em></td>
    <td align="center"><em>Graduation to flock</em></td>
  </tr>
</table>

### Breeding Intelligence

Inbreeding coefficient calculated for every possible male-female pairing. Flags anything above 6.25% as risky. Breeding groups enforce 3-to-5 females per male. Safe pairing suggestions scored by genetic distance across bloodlines.

<table>
  <tr>
    <td><img width="300" height="1520" alt="image" src="https://github.com/user-attachments/assets/c5c510aa-0589-435c-9f12-5a13185d218d" /></td>
    <td><img width="300" alt="Breeding suggestions" src="https://github.com/user-attachments/assets/729f1a70-73fe-4247-ba42-19dd21ddc8a4" /></td>
  </tr>
  <tr>
    <td align="center"><em>Pair check with coefficient</em></td>
    <td align="center"><em>Breeding group management</em></td>
  </tr>
</table>

<p align="center">
  <img width="600" alt="Breeding page — web dashboard" src="https://github.com/user-attachments/assets/dfed3688-052f-4f46-bbd6-6f3275c33e3e" />
</p>

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
| Observability | Prometheus (metrics scraping), Grafana (dashboards), `metrics` crate |
| Remote Access | Tailscale (mesh VPN, no port forwarding) |
| Deployment | Docker Compose (server + Prometheus + Grafana), systemd (cameras), Arduino IDE (ESP32) |

---

## Testing

```bash
cargo fmt --check    # Formatting
cargo clippy         # Lints
cargo test           # Unit + integration tests
```

### Rust — 59 tests across two suites

**`boundary_tests.rs` — 40 boundary & stress tests**

These are the "try to break it" tests. They cover:
- **API input validation** — empty names, 10,000-character strings, SQL injection attempts, XSS payloads, unicode/emoji, null bytes in every POST/PUT endpoint
- **Database boundaries** — inserting readings for non-existent brooders, duplicate NFC tag IDs, deleting bloodlines that have birds referencing them, querying brooders with zero readings vs. 100,000 readings
- **WebSocket edge cases** — connect and immediately disconnect, send empty messages, send 1MB messages, send binary instead of text, send 1,000 messages per second, 100 concurrent client connections
- **Alert engine boundaries** — readings exactly at threshold, rapid oscillation above/below threshold, alerts with no config set, negative values, NaN, Infinity
- **Path traversal / security** — backup restore with filenames like `../../etc/passwd`, null bytes, encoded characters (`%2e%2e%2f`), extremely long URL paths
- **Concurrent write stress** — 50 simultaneous tasks inserting readings to verify SQLite handles contention without data loss

**`api_tests.rs` — 19 unit + integration tests**

Each test spins up a fresh Axum server on a random port with an in-memory SQLite database — fully isolated, no shared state:
- Serde roundtrip tests for all `TelemetryPayload` variants (System, Brooder, Detection, Unknown)
- `AlertConfig` default values and serialization
- `InbreedingCoefficient` threshold logic — safe below 6.25%, unsafe at/above, serde roundtrip
- `ClutchStatus` enum behavior and JSON string values
- Full API integration: create and list bloodlines, create and list birds, breeding suggestions for same-bloodline pairs (coefficient 0.25, unsafe), different-bloodline pairs (coefficient 0.0, safe), and full siblings (coefficient 0.5, unsafe)

### Python — 58 tests

**`test_pi_agent.py`**
- **Sensor edge cases** — `None` temperature, `None` humidity, both `None`, checksum failures 10 times in a row, extreme values (-40°C, 80°C, 0% humidity, 100% humidity)
- **WebSocket resilience** — server unreachable on startup, connection drops mid-send, reconnection backoff verification (confirms it actually backs off instead of hammering the server)
- **Camera stream** — multi-client MJPEG serving, snapshot endpoint under load
- **QR code parsing** — empty strings, 10,000-character payloads, XSS injection, SQL injection, null bytes, unicode, brooder IDs of 0, -1, and 999999999

### CI Pipeline

GitHub Actions runs `cargo fmt --check`, `cargo clippy`, and `cargo test` on every push to `main`. All three must pass before merging.

---

## Hardware

### Raspberry Pi 5 (8GB)

The brain of the operation. Runs the Rust/Axum server in Docker, the camera stream as a systemd service, and coordinates all sensor data. Ubuntu Server, headless, accessed via SSH.

### ESP32-C3 Super Mini — Wireless Sensor Nodes

One per brooder. Each board has a DHT22 wired to GPIO4, connects over WiFi, and sends temperature/humidity readings every 5 seconds via WebSocket. Auto-creates its brooder entry on the server when it first connects. Powered by USB-C wall adapters.

### Arducam IMX477 HQ Camera

12.3MP Sony sensor with a 6mm CS-mount manual focus lens. Streams MJPEG at 640x480 to support dual simultaneous streams without exhausting DMA memory. Runs outside Docker via systemd because Pi camera drivers need direct hardware access.

### DHT22 Temperature/Humidity Sensors

One per brooder, soldered to the ESP32-C3 nodes. Reads every 5 seconds. The alert engine cross-references these readings against age-based temperature targets for whatever chick group is in that brooder.

### XH-W3002 Temperature Controller

Hardware backup thermostat on the brooders, independent of QuailSync. P0/P1 set points control the heating panel directly. This is the failsafe — if the Pi goes down, the brooders still have heat control.

### NFC Tags (NTAG215)

Attached to leg bands on each bird. Read/written via the Android app using the phone's built-in NFC. Stores the bird's database ID so a tap pulls up its full profile instantly.

### Incubators

Nurture Right 360 (primary) and Magicfly (secondary) for staggered hatches across bloodlines. Coturnix quail have a 17-day incubation cycle.

---

## 3D Printed Parts

All parts designed in OpenSCAD and trimesh, printed on an Artillery Sidewinder X in PLA.

**Print settings:** 0.2mm layer height, 20% infill, no supports needed.

### Camera Stand

Two-piece parametric design (`CAD/camera_stand_v4.scad`). Weighted base with coin pockets for stability, tapered column with cable channel, 3-sided cradle holding the Arducam at a 3° downward tilt. Snap-on backplate with ribbon cable exit slots and a catch lip.

<!-- TODO: Photo of printed camera stand -->

### ESP32 Sensor Enclosures

Vented housing with an L-hook for mounting on 15mm brooder walls. Snap-fit lid, USB cable trough along the hook, and internal standoffs for the DHT22. Three labeled versions (Sensor 1, 2, 3). Iterated from wired sensor pods to wireless ESP32 enclosures as the project evolved.

<table>
  <tr>
    <td><img width="400" alt="Sensor enclosure v1 — wired" src="https://github.com/user-attachments/assets/b60ee06d-a7c7-402d-9583-6c4cce3b5c83" /></td>
    <td><img width="400" alt="Sensor enclosure v2 — ESP32 wireless with C bracket" src="https://github.com/user-attachments/assets/4696f9f6-58b1-4ad4-961b-4d35fff2bc07" /></td>
  </tr>
  <tr>
    <td align="center"><em>v1 — Wired sensor pods</em></td>
    <td align="center"><em>v2 — Wireless ESP32 with C bracket</em></td>
  </tr>
</table>

### Pi 5 Case Lid

Custom lid for the CanaKit Turbine case with camera ribbon cable slots rotated 90° for cleaner routing.

<!-- TODO: Upload camera case STLs and incubator dividers -->

STL files are in the `/CAD` directory.

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
│   │   └── tests/               # Integration + boundary tests
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

## Observability

Prometheus and Grafana run alongside the QuailSync server via Docker Compose, providing real-time metrics and historical dashboards.

The Rust server exposes a `GET /metrics` endpoint in Prometheus text format. Prometheus scrapes it every 15 seconds and collects:

- **`quailsync_temperature_fahrenheit`** — current temperature per brooder (gauge, labeled by brooder ID)
- **`quailsync_humidity_percent`** — current humidity per brooder (gauge)
- **`quailsync_alerts_total`** — alert count by severity: info, warning, critical (counter)
- **`quailsync_websocket_connections`** — active WebSocket connections by type: agent, live (gauge)
- **`quailsync_http_requests_total`** — HTTP request count by endpoint path (counter)

Grafana runs on port 3001 (default login: admin / quailsync) and connects to Prometheus as a data source. Use it to build dashboards for brooder temperature trends over time, alert frequency, and system health.

```bash
# Everything starts together
docker compose up -d

# Prometheus UI: http://<pi-ip>:9090
# Grafana UI:    http://<pi-ip>:3001
```

---

## Remote Access

[Tailscale](https://tailscale.com) provides secure remote access to the entire QuailSync stack without port forwarding or firewall changes. Install Tailscale on the Pi and on any client device (phone, laptop), and everything is accessible over a private mesh VPN.

With Tailscale running, the full stack works from anywhere — cellular, coffee shop WiFi, or another network entirely:

- **Dashboard**: `http://<tailscale-ip>:3000`
- **Grafana**: `http://<tailscale-ip>:3001`
- **Camera stream**: `http://<tailscale-ip>:8080/stream`
- **Android app**: Set the server URL in Settings to the Tailscale IP

The Android app automatically rewrites camera stream URLs to use the configured server host, so camera feeds work seamlessly over Tailscale even though the Pi announces its LAN IP internally.

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

Personal project by [Georgia Wetherholt](https://linkedin.com/in/gwetherholt).

---

*Built with Rust, Python, solder, and way too many quail eggs.*
