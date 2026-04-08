#!/usr/bin/env python3
"""
QuailSync Camera Stream with QR Code Detection
Streams MJPEG video and periodically scans for QR codes
to auto-identify which brooder the camera is looking at.

QR code format: brooder-{id}-{name}  (e.g., brooder-1-texas, brooder-2-pharaoh)

Endpoints:
  /stream    - live MJPEG stream (supports multiple simultaneous clients)
  /snapshot  - single JPEG from latest frame
  /qr-status - JSON with current detected brooder
  /          - simple HTML page with stream + QR status

Dependencies:
  pip3 install pyzbar pillow --break-system-packages
  sudo apt install libzbar0
  pip3 install opencv-python-headless --break-system-packages  (optional, for QR overlay)

Usage:
  python3 camera_stream.py [--server ws://192.168.0.114:3000/ws] [--port 8080] [--camera-index 0] [--brooder-id 1]
  python3 camera_stream.py --width 1920 --height 1080 --collect-snapshots --snapshot-interval 60

Multi-camera example (two cameras on one Pi):
  python3 camera_stream.py --camera-index 0 --port 8080 --brooder-id 1 --server ws://192.168.0.114:3000/ws &
  python3 camera_stream.py --camera-index 1 --port 8081 --brooder-id 2 --server ws://192.168.0.114:3000/ws &
"""

from picamera2 import Picamera2
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
import io
import time
import json
import threading
import argparse
import re
import sys
import html as html_mod
import numpy as np

# Try importing QR scanning library
try:
    from pyzbar import pyzbar
    from PIL import Image
    QR_AVAILABLE = True
    print("\033[32m[qr] pyzbar loaded — QR scanning enabled\033[0m")
except ImportError:
    QR_AVAILABLE = False
    print("\033[33m[qr] pyzbar not installed — QR scanning disabled\033[0m")
    print("     Install with: pip3 install pyzbar pillow --break-system-packages")
    print("     Also: sudo apt install libzbar0")

# Try importing websockets for server communication
try:
    import asyncio
    import websockets
    WS_AVAILABLE = True
except ImportError:
    WS_AVAILABLE = False

# Try importing requests for HTTP POST
try:
    import requests as http_requests
    REQUESTS_AVAILABLE = True
except ImportError:
    REQUESTS_AVAILABLE = False


# === Global State ===
current_brooder_id = None       # last QR-detected brooder ID (raw detection)
current_bloodline_name = None
active_brooder_id = None        # the brooder this camera is currently assigned to
last_qr_scan_time = 0
qr_scan_interval = 2.0  # scan every 2 seconds (not every frame)
qr_detections = {}       # track detection counts for stability
qr_stable_threshold = 3  # need 3 consecutive detections to confirm
last_qr_raw = None
last_qr_rects = []       # bounding boxes from last QR scan (for /qr-status)
server_url = None
default_brooder_id = 1
stream_port = 8080
advertise_ip = None  # if set, use this IP in stream announcements instead of auto-detect
picam2 = None  # initialized in main()
jpeg_quality = 85       # quality for saved snapshots (YOLO training data)
stream_quality = 70     # quality for MJPEG stream (lower = less bandwidth)
_camera_ready_time = 0  # set after AWB settling in init_camera

# White balance: RGB multipliers applied per-frame via numpy.
# Protected by a lock so the /wb-settings POST can update mid-stream.
_wb_lock = threading.Lock()
_wb_gains = {"r": 1.0, "g": 1.0, "b": 1.0}
WB_SETTINGS_FILE = "wb_settings.json"


def _load_wb_settings():
    """Load saved white balance from wb_settings.json if it exists."""
    global _wb_gains
    try:
        import os
        if os.path.exists(WB_SETTINGS_FILE):
            with open(WB_SETTINGS_FILE) as f:
                data = json.load(f)
            with _wb_lock:
                _wb_gains = {
                    "r": float(data.get("r", 1.0)),
                    "g": float(data.get("g", 1.0)),
                    "b": float(data.get("b", 1.0)),
                }
            print(f"\033[32m[wb] Loaded from {WB_SETTINGS_FILE}: R={_wb_gains['r']:.2f} G={_wb_gains['g']:.2f} B={_wb_gains['b']:.2f}\033[0m")
    except Exception as e:
        print(f"\033[33m[wb] Could not load {WB_SETTINGS_FILE}: {e}\033[0m")


def _save_wb_settings():
    """Save current white balance to wb_settings.json."""
    try:
        with _wb_lock:
            data = dict(_wb_gains)
        with open(WB_SETTINGS_FILE, "w") as f:
            json.dump(data, f, indent=2)
        print(f"\033[32m[wb] Saved to {WB_SETTINGS_FILE}: R={data['r']:.2f} G={data['g']:.2f} B={data['b']:.2f}\033[0m")
    except Exception as e:
        print(f"\033[31m[wb] Save failed: {e}\033[0m")

# Snapshot collection for YOLO training
collect_snapshots = False
snapshot_interval = 600  # seconds between snapshots
snapshot_dir = "./snapshots"
max_snapshots = 1000
last_snapshot_time = 0
_snapshot_lock = threading.Lock()

# === Shared Frame Buffer (multi-client support) ===
_frame_lock = threading.Lock()
_frame_condition = threading.Condition(_frame_lock)
_latest_frame = None       # latest JPEG bytes
_frame_counter = 0         # increments each new frame


def init_camera(camera_index=0, width=2028, height=1080):
    """Initialize the Picamera2 instance with the given camera index."""
    global picam2, _camera_ready_time
    picam2 = Picamera2(camera_index)
    config = picam2.create_still_configuration(
        main={"size": (width, height)}
    )
    picam2.configure(config)
    picam2.start()
    time.sleep(1)
    # Enable continuous autofocus (silently skip if camera doesn't support it)
    try:
        picam2.set_controls({"AfMode": 2, "AfTrigger": 0})
        print(f"\033[32m[camera] Continuous autofocus enabled\033[0m")
    except Exception:
        print(f"\033[33m[camera] Autofocus not available on this camera\033[0m")
    _camera_ready_time = time.time()
    print(f"\033[32m[camera] Started camera {camera_index} — {width}x{height} (still mode, native format)\033[0m")


# === Camera Capture Thread ===

def _capture_loop():
    """Continuously capture frames from the camera and store in shared buffer.
    One thread does all capturing; HTTP handlers just read the latest frame."""
    global _latest_frame, _frame_counter
    frame_count = 0
    while True:
        try:
            # Capture array directly — use as-is, no channel flipping.
            array = picam2.capture_array()
            frame_count += 1

            # Optional fine-tune WB via /settings page (defaults 1.0 = no-op)
            with _wb_lock:
                r, g, b = _wb_gains["r"], _wb_gains["g"], _wb_gains["b"]
            if r != 1.0 or g != 1.0 or b != 1.0:
                array = array.copy()
                if r != 1.0:
                    array[:, :, 0] = np.clip(array[:, :, 0].astype(np.uint16) * r, 0, 255).astype(np.uint8)
                if g != 1.0:
                    array[:, :, 1] = np.clip(array[:, :, 1].astype(np.uint16) * g, 0, 255).astype(np.uint8)
                if b != 1.0:
                    array[:, :, 2] = np.clip(array[:, :, 2].astype(np.uint16) * b, 0, 255).astype(np.uint8)

            # QR scan every Nth frame (~every 30 frames ≈ 3s at 10fps)
            qr_rects = []
            if frame_count % 30 == 0:
                qr_rects = _scan_array_for_qr(array)

            # Encode to PIL Image once, then save at two quality levels
            frame_img = Image.fromarray(array)

            # High quality for YOLO training snapshots (full resolution, quality 85)
            snapshot_buf = io.BytesIO()
            frame_img.save(snapshot_buf, format="JPEG", quality=jpeg_quality)
            _maybe_save_snapshot(snapshot_buf.getvalue())

            # Lower quality for MJPEG stream (reduces bandwidth for Tailscale/remote)
            stream_buf = io.BytesIO()
            frame_img.save(stream_buf, format="JPEG", quality=stream_quality)
            stream_frame = stream_buf.getvalue()

            # Publish stream frame to shared buffer
            with _frame_condition:
                _latest_frame = stream_frame
                _frame_counter += 1
                _frame_condition.notify_all()

            time.sleep(0.1)  # ~10 FPS
        except Exception as e:
            print(f"\033[31m[capture] Error: {e}\033[0m")
            time.sleep(1)


def _scan_array_for_qr(array):
    """Scan a numpy RGB array for QR codes. Returns list of bounding rects."""
    global current_brooder_id, current_bloodline_name, last_qr_scan_time
    global qr_detections, last_qr_raw, last_qr_rects

    if not QR_AVAILABLE:
        return []

    last_qr_scan_time = time.time()
    rects = []

    try:
        img = Image.fromarray(array)
        decoded = pyzbar.decode(img)

        for obj in decoded:
            data = obj.data.decode("utf-8").strip()
            rect = obj.rect
            rects.append((rect.left, rect.top, rect.width, rect.height))

            # Match 'brooder-N' or 'brooder-N-bloodline' format
            match = re.match(r"^brooder-(\d+)(?:-(.+))?$", data, re.IGNORECASE)
            if match:
                brooder_id = int(match.group(1))
                bloodline_name = match.group(2)  # None for simple 'brooder-N' format
                last_qr_raw = data

                if current_brooder_id != brooder_id:
                    old = current_brooder_id
                    current_brooder_id = brooder_id
                    current_bloodline_name = bloodline_name
                    bl_str = f" (bloodline: {bloodline_name})" if bloodline_name else ""
                    print(f"\033[36m[qr] Detected: {old} → {brooder_id}{bl_str}\033[0m")
                # Update active assignment immediately on first detection
                if active_brooder_id != brooder_id:
                    _set_active_brooder(brooder_id)

        last_qr_rects = rects
        return rects

    except Exception as e:
        print(f"\033[31m[qr] Scan error: {e}\033[0m")
        return []


def scan_frame_for_qr(frame_bytes):
    """Legacy wrapper: scan JPEG bytes. Used only if needed externally."""
    global last_qr_scan_time
    if not QR_AVAILABLE:
        return None
    now = time.time()
    if now - last_qr_scan_time < qr_scan_interval:
        return current_brooder_id
    try:
        img = Image.open(io.BytesIO(frame_bytes))
        arr = np.array(img)
        _scan_array_for_qr(arr)
    except Exception:
        pass
    return current_brooder_id


def _maybe_save_snapshot(frame_bytes):
    """Queue a snapshot save if the interval has elapsed. Actual I/O runs in a background thread."""
    global last_snapshot_time
    if not collect_snapshots:
        return
    now = time.time()
    # Don't save snapshots until 3s after camera is ready (AWB settling)
    if now - _camera_ready_time < 3.0:
        return
    if now - last_snapshot_time < snapshot_interval:
        return
    last_snapshot_time = now
    data = bytes(frame_bytes)
    threading.Thread(target=_save_snapshot, args=(data,), daemon=True).start()


def _save_snapshot(frame_bytes):
    """Save a JPEG snapshot to disk and enforce max_snapshots limit."""
    import os
    from datetime import datetime

    with _snapshot_lock:
        os.makedirs(snapshot_dir, exist_ok=True)

        bid = active_brooder_id if active_brooder_id is not None else default_brooder_id
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        filename = f"brooder{bid}_{timestamp}.jpg"
        filepath = os.path.join(snapshot_dir, filename)

        try:
            with open(filepath, "wb") as f:
                f.write(frame_bytes)
        except Exception as e:
            print(f"\033[31m[snapshot] Save failed: {e}\033[0m")
            return

        existing = sorted(
            [f for f in os.listdir(snapshot_dir) if f.endswith(".jpg")],
            key=lambda f: os.path.getmtime(os.path.join(snapshot_dir, f))
        )
        deleted = 0
        while len(existing) > max_snapshots:
            oldest = existing.pop(0)
            try:
                os.remove(os.path.join(snapshot_dir, oldest))
                deleted += 1
            except OSError:
                pass

        size_kb = len(frame_bytes) / 1024
        count = len(existing)
        msg = f"\033[36m[snapshot] Saved {filename} ({size_kb:.0f} KB, {count}/{max_snapshots})\033[0m"
        if deleted:
            msg += f" (deleted {deleted} oldest)"
        print(msg)


def _get_local_ip():
    """Get the Pi's LAN IP address."""
    import socket
    local_ip = socket.gethostbyname(socket.gethostname())
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))
        local_ip = s.getsockname()[0]
        s.close()
    except Exception:
        pass
    return local_ip


def _set_active_brooder(new_id):
    """Change which brooder this camera is assigned to. Clears the old
    brooder's camera_url on the server and announces to the new one."""
    global active_brooder_id
    old_id = active_brooder_id
    active_brooder_id = new_id
    print(f"\033[32m[camera] Active brooder: {old_id} → {new_id}\033[0m")
    if WS_AVAILABLE and server_url:
        threading.Thread(target=_announce_to_server, args=(new_id, old_id), daemon=True).start()


def _announce_to_server(new_brooder_id, old_brooder_id=None):
    """Announce camera stream URL to new brooder; clear old brooder's camera_url."""
    if not WS_AVAILABLE or not server_url:
        return
    try:
        ip = advertise_ip if advertise_ip else _get_local_ip()
        stream_url = f"http://{ip}:{stream_port}/stream"
        snapshot_url = f"http://{ip}:{stream_port}/snapshot"

        # Clear old brooder's camera_url via REST
        if old_brooder_id is not None and old_brooder_id != new_brooder_id:
            try:
                http_base = server_url.replace("ws://", "http://").replace("wss://", "https://")
                http_base = re.sub(r"/ws$", "", http_base)
                url = f"{http_base}/api/brooders/{old_brooder_id}"
                body = json.dumps({"camera_url": None}).encode()
                import urllib.request
                req = urllib.request.Request(url, data=body, method="PUT",
                                            headers={"Content-Type": "application/json"})
                urllib.request.urlopen(req, timeout=5)
                print(f"\033[33m[camera] Cleared camera_url on brooder {old_brooder_id}\033[0m")
            except Exception as e:
                print(f"\033[33m[camera] Failed to clear old brooder {old_brooder_id}: {e}\033[0m")

        # Announce to new brooder via WebSocket
        async def _send():
            async with websockets.connect(server_url) as ws:
                payload = json.dumps({
                    "CameraAnnounce": {
                        "brooder_id": new_brooder_id,
                        "stream_url": stream_url,
                        "snapshot_url": snapshot_url
                    }
                })
                await ws.send(payload)
                print(f"\033[32m[camera] Announced to server: brooder {new_brooder_id} stream={stream_url}\033[0m")

        asyncio.run(_send())

        # Also send QrDetected if we have QR data
        if last_qr_raw:
            try:
                async def _send_qr():
                    async with websockets.connect(server_url) as ws:
                        payload = json.dumps({
                            "QrDetected": {
                                "brooder_id": new_brooder_id,
                                "bloodline": current_bloodline_name or "",
                                "qr_code": last_qr_raw
                            }
                        })
                        await ws.send(payload)
                asyncio.run(_send_qr())
            except Exception:
                pass

    except Exception as e:
        print(f"\033[33m[camera] Announce failed: {e}\033[0m")


# === HTTP Handler ===

class StreamHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/stream":
            self._handle_stream()
        elif self.path == "/snapshot":
            self._handle_snapshot()
        elif self.path == "/qr-status":
            self._handle_qr_status()
        elif self.path == "/settings":
            self._handle_settings_page()
        elif self.path == "/wb-settings":
            self._handle_wb_get()
        else:
            self._handle_index()

    def do_POST(self):
        if self.path == "/wb-settings":
            self._handle_wb_post()
        else:
            self.send_response(404)
            self.end_headers()

    def _handle_stream(self):
        """MJPEG stream — reads from shared frame buffer so multiple clients work."""
        self.send_response(200)
        self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-cache, no-store, must-revalidate")
        self.end_headers()
        last_counter = 0
        try:
            while True:
                with _frame_condition:
                    while _frame_counter == last_counter:
                        _frame_condition.wait(timeout=2.0)
                    frame = _latest_frame
                    last_counter = _frame_counter

                if frame is None:
                    continue

                # Write complete MJPEG part with proper boundary + headers.
                # Content-Length ensures clients don't render partial frames.
                part = (
                    b"--frame\r\n"
                    b"Content-Type: image/jpeg\r\n"
                    + f"Content-Length: {len(frame)}\r\n".encode()
                    + b"\r\n"
                    + frame
                    + b"\r\n"
                )
                self.wfile.write(part)
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _handle_snapshot(self):
        """Return the latest frame from the shared buffer (no new capture needed)."""
        with _frame_lock:
            frame = _latest_frame

        if frame is None:
            self.send_response(503)
            self.end_headers()
            self.wfile.write(b"Camera not ready")
            return

        self.send_response(200)
        self.send_header("Content-Type", "image/jpeg")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(frame)

    def _handle_qr_status(self):
        status = {
            "active_brooder_id": active_brooder_id,
            "brooder_id": current_brooder_id,
            "bloodline_name": current_bloodline_name,
            "qr_scanning": QR_AVAILABLE,
            "last_scan": last_qr_scan_time,
            "last_raw": last_qr_raw,
        }
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps(status).encode())

    def _handle_index(self):
        safe_name = html_mod.escape(str(current_bloodline_name)) if current_bloodline_name else ""
        brooder_label = f"Brooder {current_brooder_id} ({safe_name})" if current_brooder_id else "No QR detected"
        qr_status = "enabled" if QR_AVAILABLE else "disabled (install pyzbar)"
        html = f"""<!DOCTYPE html>
<html>
<head><title>QuailSync Camera</title>
<style>
  body {{ font-family: sans-serif; background: #1a1a2e; color: #eee; text-align: center; padding: 20px; }}
  .status {{ background: #16213e; padding: 10px 20px; border-radius: 8px; display: inline-block; margin: 10px; }}
  .brooder {{ color: #00d4ff; font-size: 1.3em; }}
  img {{ border-radius: 8px; margin-top: 10px; }}
</style>
</head>
<body>
  <h1>QuailSync Camera</h1>
  <div class="status">
    <span class="brooder">{brooder_label}</span>
  </div>
  <div class="status">QR Scanning: {qr_status}</div>
  <br>
  <img src="/stream" width="2028" style="max-width:100%">
  <br><br>
  <a href="/snapshot" style="color:#00d4ff">Snapshot</a> |
  <a href="/qr-status" style="color:#00d4ff">QR Status JSON</a>
  <script>
    setInterval(() => {{
      fetch('/qr-status')
        .then(r => r.json())
        .then(d => {{
          const label = d.brooder_id ? 'Brooder ' + d.brooder_id + (d.bloodline_name ? ' (' + d.bloodline_name + ')' : '') : 'No QR detected';
          document.querySelector('.brooder').textContent = label;
        }});
    }}, 5000);
  </script>
</body>
</html>"""
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.end_headers()
        self.wfile.write(html.encode())

    def _handle_wb_get(self):
        with _wb_lock:
            data = dict(_wb_gains)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def _handle_wb_post(self):
        global _wb_gains
        try:
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length))
            with _wb_lock:
                if "r" in body:
                    _wb_gains["r"] = max(0.0, min(3.0, float(body["r"])))
                if "g" in body:
                    _wb_gains["g"] = max(0.0, min(3.0, float(body["g"])))
                if "b" in body:
                    _wb_gains["b"] = max(0.0, min(3.0, float(body["b"])))
            save = body.get("save", False)
            if save:
                _save_wb_settings()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            with _wb_lock:
                self.wfile.write(json.dumps(_wb_gains).encode())
        except Exception as e:
            self.send_response(400)
            self.end_headers()
            self.wfile.write(f'{{"error": "{e}"}}'.encode())

    def _handle_settings_page(self):
        html = """<!DOCTYPE html>
<html>
<head><title>QuailSync Camera — White Balance</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { font-family: -apple-system, sans-serif; background: #1a1a2e; color: #eee; padding: 20px; max-width: 800px; margin: 0 auto; }
  h1 { font-size: 1.3rem; margin-bottom: 12px; }
  .preview { width: 100%; border-radius: 8px; margin-bottom: 16px; background: #111; }
  .panel { background: #16213e; border-radius: 10px; padding: 16px; margin-bottom: 12px; }
  .slider-row { display: flex; align-items: center; gap: 12px; margin: 10px 0; }
  .slider-row label { width: 50px; font-weight: 600; }
  .slider-row input[type=range] { flex: 1; accent-color: #7d8b6a; }
  .slider-row .val { width: 50px; text-align: right; font-family: monospace; font-size: 1.1rem; }
  .actions { display: flex; gap: 10px; margin-top: 14px; }
  .btn { padding: 10px 24px; border: none; border-radius: 8px; font-size: .9rem; font-weight: 600; cursor: pointer; }
  .btn-save { background: #7d8b6a; color: #fff; }
  .btn-reset { background: #444; color: #ccc; }
  .btn:hover { opacity: 0.85; }
  .status { font-size: .8rem; color: #7d8b6a; margin-top: 8px; min-height: 1.2em; }
  a { color: #7d8b6a; }
</style>
</head>
<body>
  <h1>White Balance Tuning</h1>
  <img class="preview" src="/stream">
  <div class="panel">
    <div class="slider-row">
      <label style="color:#ff6b6b">Red</label>
      <input type="range" id="r" min="0.5" max="2.0" step="0.01" value="1.0">
      <span class="val" id="rv">1.00</span>
    </div>
    <div class="slider-row">
      <label style="color:#6bff6b">Green</label>
      <input type="range" id="g" min="0.5" max="2.0" step="0.01" value="1.0">
      <span class="val" id="gv">1.00</span>
    </div>
    <div class="slider-row">
      <label style="color:#6b6bff">Blue</label>
      <input type="range" id="b" min="0.5" max="2.0" step="0.01" value="1.0">
      <span class="val" id="bv">1.00</span>
    </div>
    <div class="actions">
      <button class="btn btn-save" onclick="save()">Save</button>
      <button class="btn btn-reset" onclick="reset()">Reset (1.0)</button>
    </div>
    <div class="status" id="status"></div>
  </div>
  <p style="font-size:.8rem;color:#666;margin-top:8px"><a href="/">Back to main</a></p>
<script>
  const rs=document.getElementById('r'), gs=document.getElementById('g'), bs=document.getElementById('b');
  const rv=document.getElementById('rv'), gv=document.getElementById('gv'), bv=document.getElementById('bv');
  const st=document.getElementById('status');
  let debounce=null;

  function show(r,g,b){ rs.value=r; gs.value=g; bs.value=b; rv.textContent=Number(r).toFixed(2); gv.textContent=Number(g).toFixed(2); bv.textContent=Number(b).toFixed(2); }

  function send(){
    const body={r:parseFloat(rs.value),g:parseFloat(gs.value),b:parseFloat(bs.value)};
    rv.textContent=body.r.toFixed(2); gv.textContent=body.g.toFixed(2); bv.textContent=body.b.toFixed(2);
    fetch('/wb-settings',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  }

  function onSlider(){ clearTimeout(debounce); debounce=setTimeout(send, 80); rv.textContent=parseFloat(rs.value).toFixed(2); gv.textContent=parseFloat(gs.value).toFixed(2); bv.textContent=parseFloat(bs.value).toFixed(2); }
  rs.addEventListener('input', onSlider);
  gs.addEventListener('input', onSlider);
  bs.addEventListener('input', onSlider);

  function save(){
    const body={r:parseFloat(rs.value),g:parseFloat(gs.value),b:parseFloat(bs.value),save:true};
    fetch('/wb-settings',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)})
      .then(()=>{ st.textContent='Saved!'; setTimeout(()=>st.textContent='',3000); });
  }

  function reset(){ show(1.0,1.0,1.0); send(); st.textContent='Reset to defaults'; setTimeout(()=>st.textContent='',3000); }

  // Load current values on page open
  fetch('/wb-settings').then(r=>r.json()).then(d=>show(d.r,d.g,d.b));
</script>
</body>
</html>"""
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.end_headers()
        self.wfile.write(html.encode())

    def log_message(self, format, *args):
        if len(args) > 1 and "200" not in str(args[1]):
            print(f"[camera] {args[0]}")


def main():
    global server_url, default_brooder_id, stream_port, jpeg_quality, stream_quality, active_brooder_id, advertise_ip
    global collect_snapshots, snapshot_interval, snapshot_dir, max_snapshots

    parser = argparse.ArgumentParser(description="QuailSync Camera Stream")
    parser.add_argument("--port", type=int, default=8080, help="HTTP port (default: 8080)")
    parser.add_argument("--server", type=str, default=None,
                        help="QuailSync server WebSocket URL (e.g., ws://192.168.0.114:3000/ws)")
    parser.add_argument("--brooder-id", type=int, default=1,
                        help="Brooder ID to register this camera with (default: 1)")
    parser.add_argument("--camera-index", type=int, default=0,
                        help="Camera index for Picamera2 (0=cam0, 1=cam1, default: 0)")
    parser.add_argument("--width", type=int, default=2028,
                        help="Capture width in pixels (default: 2028)")
    parser.add_argument("--height", type=int, default=1080,
                        help="Capture height in pixels (default: 1080)")
    parser.add_argument("--jpeg-quality", type=int, default=85,
                        help="Snapshot JPEG quality 1-100 for YOLO training data (default: 85)")
    parser.add_argument("--stream-quality", type=int, default=70,
                        help="MJPEG stream JPEG quality 1-100, lower = less bandwidth (default: 70)")
    parser.add_argument("--collect-snapshots", action="store_true", default=False,
                        help="Save periodic snapshots for YOLO training to ./snapshots/")
    parser.add_argument("--snapshot-interval", type=int, default=600,
                        help="Seconds between saved snapshots (default: 600)")
    parser.add_argument("--snapshot-dir", type=str, default="./snapshots",
                        help="Directory for saved snapshots (default: ./snapshots)")
    parser.add_argument("--max-snapshots", type=int, default=1000,
                        help="Max snapshots to keep; oldest deleted when exceeded (default: 1000)")
    parser.add_argument("--awb-gains", type=float, nargs="+", metavar="VAL", default=None,
                        help="White balance multipliers: R G B (3 values) or R B (2 values, G=1.0). e.g. --awb-gains 1.25 1.0 0.72. Overrides saved wb_settings.json.")
    parser.add_argument("--advertise-ip", type=str, default=None,
                        help="IP address to announce in stream URLs instead of auto-detected LAN IP (e.g. Tailscale IP: 100.109.222.48)")
    args = parser.parse_args()

    server_url = args.server
    default_brooder_id = args.brooder_id
    stream_port = args.port
    advertise_ip = args.advertise_ip
    jpeg_quality = args.jpeg_quality
    stream_quality = args.stream_quality
    collect_snapshots = args.collect_snapshots
    snapshot_interval = args.snapshot_interval
    snapshot_dir = args.snapshot_dir
    max_snapshots = args.max_snapshots

    # Load saved WB settings first, then CLI override if given
    _load_wb_settings()
    if args.awb_gains is not None:
        gains = args.awb_gains
        with _wb_lock:
            if len(gains) >= 3:
                _wb_gains["r"] = gains[0]
                _wb_gains["g"] = gains[1]
                _wb_gains["b"] = gains[2]
            elif len(gains) == 2:
                _wb_gains["r"] = gains[0]
                _wb_gains["g"] = 1.0
                _wb_gains["b"] = gains[1]
            elif len(gains) == 1:
                _wb_gains["r"] = gains[0]
        print(f"\033[32m[wb] CLI override: R={_wb_gains['r']:.2f} G={_wb_gains['g']:.2f} B={_wb_gains['b']:.2f}\033[0m")

    # Initialize the selected camera
    init_camera(args.camera_index, args.width, args.height)

    with _wb_lock:
        wb_str = f"R={_wb_gains['r']:.2f} G={_wb_gains['g']:.2f} B={_wb_gains['b']:.2f}"
    print(f"\n\033[1m[QuailSync Camera]\033[0m")
    print(f"  Camera:     index {args.camera_index}")
    print(f"  Resolution: {args.width}x{args.height}")
    print(f"  JPEG:       snapshot q{jpeg_quality}, stream q{stream_quality}")
    print(f"  WB:         {wb_str}")
    print(f"  WB Tuning:  http://0.0.0.0:{args.port}/settings")
    print(f"  Stream:     http://0.0.0.0:{args.port}/stream")
    print(f"  Snapshot:   http://0.0.0.0:{args.port}/snapshot")
    print(f"  QR JSON:    http://0.0.0.0:{args.port}/qr-status")
    print(f"  QR Scan:    {'enabled' if QR_AVAILABLE else 'DISABLED'}")
    print(f"  Server:     {server_url or 'not configured'}")
    print(f"  Advertise:  {advertise_ip or 'auto-detect'}")
    print(f"  Brooder:    {default_brooder_id}")
    if collect_snapshots:
        print(f"  Snapshots:  every {snapshot_interval}s -> {snapshot_dir}/ (max {max_snapshots})")
    print()

    # Start the background capture thread
    capture_thread = threading.Thread(target=_capture_loop, daemon=True)
    capture_thread.start()

    # Set initial active brooder from CLI arg and announce to server
    active_brooder_id = default_brooder_id
    if server_url:
        threading.Thread(
            target=_announce_to_server,
            args=(default_brooder_id,),
            daemon=True
        ).start()

    # Wait for first frame before starting HTTP server
    with _frame_condition:
        while _latest_frame is None:
            _frame_condition.wait(timeout=5.0)
    print("\033[32m[camera] First frame captured, starting HTTP server\033[0m")

    try:
        server = ThreadingHTTPServer(("0.0.0.0", args.port), StreamHandler)
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[camera] Shutting down...")
        picam2.stop()


if __name__ == "__main__":
    main()
