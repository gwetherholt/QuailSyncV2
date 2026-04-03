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

# Try importing OpenCV for QR overlay drawing
try:
    import cv2
    CV2_AVAILABLE = True
except ImportError:
    CV2_AVAILABLE = False
    print("\033[33m[camera] OpenCV not installed — QR overlay disabled\033[0m")

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
current_brooder_id = None
current_bloodline_name = None
last_qr_scan_time = 0
qr_scan_interval = 2.0  # scan every 2 seconds (not every frame)
qr_detections = {}       # track detection counts for stability
qr_stable_threshold = 3  # need 3 consecutive detections to confirm
last_qr_raw = None
last_qr_rects = []       # list of (x, y, w, h) bounding boxes for QR overlay
server_url = None
default_brooder_id = 1
stream_port = 8080
picam2 = None  # initialized in main()
jpeg_quality = 85

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


def init_camera(camera_index=0, width=2028, height=1080, awb_gains=None):
    """Initialize the Picamera2 instance with the given camera index."""
    global picam2
    picam2 = Picamera2(camera_index)
    config = picam2.create_video_configuration(
        main={"size": (width, height), "format": "RGB888"}
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
    # Manual white balance for UVA/UVB brooder lighting
    if awb_gains is not None:
        try:
            picam2.set_controls({"AwbEnable": False, "ColourGains": (awb_gains[0], awb_gains[1])})
            print(f"\033[32m[camera] Manual white balance: red={awb_gains[0]}, blue={awb_gains[1]}\033[0m")
        except Exception as e:
            print(f"\033[33m[camera] Failed to set white balance: {e}\033[0m")
    print(f"\033[32m[camera] Started camera {camera_index} — {width}x{height} RGB888\033[0m")


# === Camera Capture Thread ===

def _capture_loop():
    """Continuously capture frames from the camera and store in shared buffer.
    One thread does all capturing; HTTP handlers just read the latest frame."""
    global _latest_frame, _frame_counter
    frame_count = 0
    while True:
        try:
            # Capture as numpy array for potential overlay drawing
            array = picam2.capture_array()
            frame_count += 1

            # QR scan every Nth frame (~every 30 frames ≈ 3s at 10fps)
            qr_rects = []
            if frame_count % 30 == 0:
                qr_rects = _scan_array_for_qr(array)

            # Encode raw frame FIRST (no overlay) for clean YOLO snapshots
            if CV2_AVAILABLE:
                bgr_raw = cv2.cvtColor(array, cv2.COLOR_RGB2BGR)
                _, jpeg_raw = cv2.imencode('.jpg', bgr_raw, [cv2.IMWRITE_JPEG_QUALITY, jpeg_quality])
                raw_frame = jpeg_raw.tobytes()
            else:
                img = Image.fromarray(array)
                buf = io.BytesIO()
                img.save(buf, format="JPEG", quality=jpeg_quality)
                raw_frame = buf.getvalue()

            # Save raw (no overlay) snapshot for YOLO training
            _maybe_save_snapshot(raw_frame)

            # Draw QR overlay on the array for the live stream
            rects_to_draw = qr_rects if qr_rects else last_qr_rects
            if rects_to_draw and CV2_AVAILABLE:
                for (x, y, w, h) in rects_to_draw:
                    cv2.rectangle(array, (x, y), (x + w, y + h), (0, 255, 0), 3)
                    label = last_qr_raw or ""
                    if label:
                        cv2.putText(array, label, (x, y - 10),
                                    cv2.FONT_HERSHEY_SIMPLEX, 0.7, (0, 255, 0), 2)

            # Encode overlaid frame for stream/snapshot clients
            if rects_to_draw and CV2_AVAILABLE:
                bgr_overlay = cv2.cvtColor(array, cv2.COLOR_RGB2BGR)
                _, jpeg_overlay = cv2.imencode('.jpg', bgr_overlay, [cv2.IMWRITE_JPEG_QUALITY, jpeg_quality])
                frame = jpeg_overlay.tobytes()
            else:
                frame = raw_frame

            # Publish to shared buffer
            with _frame_condition:
                _latest_frame = frame
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

            # Match brooder-{id}-{name} pattern
            match = re.match(r"^brooder-(\d+)-(.+)$", data, re.IGNORECASE)
            if match:
                brooder_id = int(match.group(1))
                bloodline_name = match.group(2)

                # Stability check
                if data.lower() == (last_qr_raw or "").lower():
                    qr_detections[data] = qr_detections.get(data, 0) + 1
                else:
                    qr_detections = {data: 1}
                last_qr_raw = data

                if qr_detections.get(data, 0) >= qr_stable_threshold:
                    if current_brooder_id != brooder_id:
                        old = current_brooder_id
                        current_brooder_id = brooder_id
                        current_bloodline_name = bloodline_name
                        print(f"\033[36m[qr] Brooder changed: {old} → {brooder_id} (bloodline: {bloodline_name})\033[0m")
                        if WS_AVAILABLE and server_url:
                            threading.Thread(target=_notify_server, args=(brooder_id,), daemon=True).start()

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

        bid = current_brooder_id if current_brooder_id is not None else default_brooder_id
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


_announced = False


def _announce_camera(brooder_id):
    """Announce this camera's stream URL to the server via WebSocket. Only once per process."""
    global _announced
    if _announced:
        print(f"[camera] Already announced, skipping (restart script to re-announce)")
        return
    if not WS_AVAILABLE or not server_url:
        return
    try:
        import socket
        local_ip = socket.gethostbyname(socket.gethostname())
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            s.connect(("8.8.8.8", 80))
            local_ip = s.getsockname()[0]
            s.close()
        except Exception:
            pass

        stream_url = f"http://{local_ip}:{stream_port}/stream"
        snapshot_url = f"http://{local_ip}:{stream_port}/snapshot"

        async def _send():
            async with websockets.connect(server_url) as ws:
                payload = json.dumps({
                    "CameraAnnounce": {
                        "brooder_id": brooder_id,
                        "stream_url": stream_url,
                        "snapshot_url": snapshot_url
                    }
                })
                await ws.send(payload)
                print(f"\033[32m[camera] Announced to server: brooder {brooder_id} stream={stream_url}\033[0m")

        asyncio.run(_send())
        _announced = True
    except Exception as e:
        print(f"\033[33m[camera] Announce failed: {e} (will not retry)\033[0m")
        _announced = True


def _notify_server(brooder_id):
    """Send QR detection + camera-brooder association to server."""
    global _announced

    # Send QR detected event via WebSocket
    if WS_AVAILABLE and server_url and current_bloodline_name:
        try:
            qr_code = f"brooder-{brooder_id}-{current_bloodline_name}"

            async def _send_qr():
                async with websockets.connect(server_url) as ws:
                    payload = json.dumps({
                        "QrDetected": {
                            "brooder_id": brooder_id,
                            "bloodline": current_bloodline_name,
                            "qr_code": qr_code
                        }
                    })
                    await ws.send(payload)
                    print(f"\033[32m[qr] Sent QrDetected to server: brooder {brooder_id} bloodline={current_bloodline_name}\033[0m")

            asyncio.run(_send_qr())
        except Exception as e:
            print(f"\033[33m[qr] QrDetected send failed: {e}\033[0m")

    # POST to /api/qr-scan so the server can do its own lookup
    if server_url and current_bloodline_name:
        try:
            # Derive HTTP base URL from the WebSocket URL
            http_base = server_url.replace("ws://", "http://").replace("wss://", "https://")
            http_base = re.sub(r"/ws$", "", http_base)
            url = f"{http_base}/api/qr-scan"
            body = {"brooder_id": brooder_id, "qr_text": last_qr_raw or ""}
            if REQUESTS_AVAILABLE:
                resp = http_requests.post(url, json=body, timeout=5)
                print(f"\033[32m[qr] POST /api/qr-scan -> {resp.status_code}\033[0m")
            else:
                # Fallback with urllib
                import urllib.request
                req = urllib.request.Request(url, data=json.dumps(body).encode(),
                                            headers={"Content-Type": "application/json"})
                with urllib.request.urlopen(req, timeout=5) as resp:
                    print(f"\033[32m[qr] POST /api/qr-scan -> {resp.status}\033[0m")
        except Exception as e:
            print(f"\033[33m[qr] POST /api/qr-scan failed: {e}\033[0m")

    # Re-announce camera for the new brooder
    _announced = False
    _announce_camera(brooder_id)


# === HTTP Handler ===

class StreamHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/stream":
            self._handle_stream()
        elif self.path == "/snapshot":
            self._handle_snapshot()
        elif self.path == "/qr-status":
            self._handle_qr_status()
        else:
            self._handle_index()

    def _handle_stream(self):
        """MJPEG stream — reads from shared frame buffer so multiple clients work."""
        self.send_response(200)
        self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        last_counter = 0
        try:
            while True:
                with _frame_condition:
                    # Wait until a new frame is available
                    while _frame_counter == last_counter:
                        _frame_condition.wait(timeout=2.0)
                    frame = _latest_frame
                    last_counter = _frame_counter

                if frame is None:
                    continue

                self.wfile.write(b"--frame\r\n")
                self.wfile.write(b"Content-Type: image/jpeg\r\n")
                self.wfile.write(f"Content-Length: {len(frame)}\r\n\r\n".encode())
                self.wfile.write(frame)
                self.wfile.write(b"\r\n")
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

    def log_message(self, format, *args):
        if len(args) > 1 and "200" not in str(args[1]):
            print(f"[camera] {args[0]}")


def main():
    global server_url, default_brooder_id, stream_port, jpeg_quality
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
                        help="JPEG compression quality 1-100 (default: 85)")
    parser.add_argument("--collect-snapshots", action="store_true", default=False,
                        help="Save periodic snapshots for YOLO training to ./snapshots/")
    parser.add_argument("--snapshot-interval", type=int, default=600,
                        help="Seconds between saved snapshots (default: 600)")
    parser.add_argument("--snapshot-dir", type=str, default="./snapshots",
                        help="Directory for saved snapshots (default: ./snapshots)")
    parser.add_argument("--max-snapshots", type=int, default=1000,
                        help="Max snapshots to keep; oldest deleted when exceeded (default: 1000)")
    parser.add_argument("--awb-gains", type=float, nargs=2, metavar=("RED", "BLUE"), default=None,
                        help="Manual white balance gains (e.g. --awb-gains 2.5 2.3 for UVA/UVB brooder lighting). Omit for auto WB.")
    args = parser.parse_args()

    server_url = args.server
    default_brooder_id = args.brooder_id
    stream_port = args.port
    jpeg_quality = args.jpeg_quality
    collect_snapshots = args.collect_snapshots
    snapshot_interval = args.snapshot_interval
    snapshot_dir = args.snapshot_dir
    max_snapshots = args.max_snapshots

    # Initialize the selected camera
    init_camera(args.camera_index, args.width, args.height, awb_gains=args.awb_gains)

    print(f"\n\033[1m[QuailSync Camera]\033[0m")
    print(f"  Camera:     index {args.camera_index}")
    print(f"  Resolution: {args.width}x{args.height}")
    print(f"  JPEG:       quality {jpeg_quality}")
    print(f"  AWB:        {'manual red={} blue={}'.format(*args.awb_gains) if args.awb_gains else 'auto'}")
    print(f"  Stream:     http://0.0.0.0:{args.port}/stream")
    print(f"  Snapshot:   http://0.0.0.0:{args.port}/snapshot")
    print(f"  QR JSON:    http://0.0.0.0:{args.port}/qr-status")
    print(f"  QR Scan:    {'enabled' if QR_AVAILABLE else 'DISABLED'}")
    print(f"  QR Overlay: {'enabled' if CV2_AVAILABLE else 'disabled (install opencv-python-headless)'}")
    print(f"  Server:     {server_url or 'not configured'}")
    print(f"  Brooder:    {default_brooder_id}")
    if collect_snapshots:
        print(f"  Snapshots:  every {snapshot_interval}s -> {snapshot_dir}/ (max {max_snapshots})")
    print()

    # Start the background capture thread
    capture_thread = threading.Thread(target=_capture_loop, daemon=True)
    capture_thread.start()

    # Auto-register this camera with the server on startup
    if server_url:
        threading.Thread(
            target=_announce_camera,
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
