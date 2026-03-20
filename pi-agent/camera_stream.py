#!/usr/bin/env python3
"""
QuailSync Camera Stream with QR Code Detection
Streams MJPEG video and periodically scans for QR codes
to auto-identify which brooder the camera is looking at.

QR code format: brooder-{id}-{name}  (e.g., brooder-1-texas, brooder-2-pharaoh)

Endpoints:
  /stream    - live MJPEG stream
  /snapshot  - single JPEG capture
  /qr-status - JSON with current detected brooder
  /          - simple HTML page with stream + QR status

Dependencies:
  pip3 install pyzbar pillow --break-system-packages
  sudo apt install libzbar0

Usage:
  python3 camera_stream.py [--server ws://192.168.0.228:3000/ws] [--port 8080] [--camera-index 0] [--brooder-id 1]

Multi-camera example (two cameras on one Pi):
  python3 camera_stream.py --camera-index 0 --port 8080 --brooder-id 1 --server ws://192.168.0.228:3000/ws &
  python3 camera_stream.py --camera-index 1 --port 8081 --brooder-id 2 --server ws://192.168.0.228:3000/ws &
"""

from picamera2 import Picamera2
from http.server import HTTPServer, BaseHTTPRequestHandler
import io
import time
import json
import threading
import argparse
import re
import sys
import html as html_mod

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

# === Global State ===
current_brooder_id = None
current_bloodline_name = None
last_qr_scan_time = 0
qr_scan_interval = 2.0  # scan every 2 seconds (not every frame)
qr_detections = {}       # track detection counts for stability
qr_stable_threshold = 3  # need 3 consecutive detections to confirm
last_qr_raw = None
server_url = None
default_brooder_id = 1
stream_port = 8080
picam2 = None  # initialized in main()


def init_camera(camera_index=0):
    """Initialize the Picamera2 instance with the given camera index."""
    global picam2
    picam2 = Picamera2(camera_index)
    config = picam2.create_video_configuration(
        main={"size": (640, 480), "format": "RGB888"}
    )
    picam2.configure(config)
    picam2.start()
    time.sleep(1)
    print(f"\033[32m[camera] Started camera {camera_index} — 640x480 RGB888\033[0m")


def scan_frame_for_qr(frame_bytes):
    """Scan a JPEG frame for QR codes. Returns brooder ID or None."""
    global current_brooder_id, current_bloodline_name, last_qr_scan_time, qr_detections, last_qr_raw

    if not QR_AVAILABLE:
        return None

    now = time.time()
    if now - last_qr_scan_time < qr_scan_interval:
        return current_brooder_id
    last_qr_scan_time = now

    try:
        img = Image.open(io.BytesIO(frame_bytes))
        decoded = pyzbar.decode(img)

        for obj in decoded:
            data = obj.data.decode("utf-8").strip()

            # Match brooder-{id}-{name} pattern (e.g. brooder-1-texas)
            match = re.match(r"^brooder-(\d+)-(.+)$", data, re.IGNORECASE)
            if match:
                brooder_id = int(match.group(1))
                bloodline_name = match.group(2)

                # Stability check — need consecutive detections
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
                        # Notify server in background
                        if WS_AVAILABLE and server_url:
                            threading.Thread(
                                target=_notify_server,
                                args=(brooder_id,),
                                daemon=True
                            ).start()
                    return brooder_id

        # No QR found this scan — don't clear immediately (might be momentary)
        return current_brooder_id

    except Exception as e:
        print(f"\033[31m[qr] Scan error: {e}\033[0m")
        return current_brooder_id


def _announce_camera(brooder_id):
    """Announce this camera's stream URL to the server via WebSocket."""
    if not WS_AVAILABLE or not server_url:
        return
    try:
        import socket
        local_ip = socket.gethostbyname(socket.gethostname())
        # Fallback: try to get the IP from the network interface
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
    except Exception as e:
        print(f"\033[33m[camera] Announce failed: {e}\033[0m")


def _notify_server(brooder_id):
    """Send camera-brooder association to server (QR code detected)."""
    # Re-announce with the new brooder ID
    _announce_camera(brooder_id)


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
        self.send_response(200)
        self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        try:
            while True:
                buf = io.BytesIO()
                picam2.capture_file(buf, format="jpeg")
                frame = buf.getvalue()

                # QR scan on this frame (throttled internally)
                scan_frame_for_qr(frame)

                self.wfile.write(b"--frame\r\n")
                self.wfile.write(b"Content-Type: image/jpeg\r\n\r\n")
                self.wfile.write(frame)
                self.wfile.write(b"\r\n")
                time.sleep(0.1)  # ~10 FPS
        except (BrokenPipeError, ConnectionResetError):
            pass

    def _handle_snapshot(self):
        buf = io.BytesIO()
        picam2.capture_file(buf, format="jpeg")
        frame = buf.getvalue()

        # Also scan this frame
        scan_frame_for_qr(frame)

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
  <img src="/stream" width="640">
  <br><br>
  <a href="/snapshot" style="color:#00d4ff">Snapshot</a> |
  <a href="/qr-status" style="color:#00d4ff">QR Status JSON</a>
  <script>
    // Auto-refresh status every 5 seconds
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
        # Quiet logging — only show non-200 or errors
        if len(args) > 1 and "200" not in str(args[1]):
            print(f"[camera] {args[0]}")


def main():
    global server_url, default_brooder_id, stream_port

    parser = argparse.ArgumentParser(description="QuailSync Camera Stream")
    parser.add_argument("--port", type=int, default=8080, help="HTTP port (default: 8080)")
    parser.add_argument("--server", type=str, default=None,
                        help="QuailSync server WebSocket URL (e.g., ws://192.168.0.228:3000/ws)")
    parser.add_argument("--brooder-id", type=int, default=1,
                        help="Brooder ID to register this camera with (default: 1)")
    parser.add_argument("--camera-index", type=int, default=0,
                        help="Camera index for Picamera2 (0=cam0, 1=cam1, default: 0)")
    args = parser.parse_args()

    server_url = args.server
    default_brooder_id = args.brooder_id
    stream_port = args.port

    # Initialize the selected camera
    init_camera(args.camera_index)

    print(f"\n\033[1m[QuailSync Camera]\033[0m")
    print(f"  Camera:   index {args.camera_index}")
    print(f"  Stream:   http://0.0.0.0:{args.port}/stream")
    print(f"  Snapshot: http://0.0.0.0:{args.port}/snapshot")
    print(f"  QR JSON:  http://0.0.0.0:{args.port}/qr-status")
    print(f"  QR Scan:  {'enabled' if QR_AVAILABLE else 'DISABLED'}")
    print(f"  Server:   {server_url or 'not configured'}")
    print(f"  Brooder:  {default_brooder_id}")
    print()

    # Auto-register this camera with the server on startup
    if server_url:
        threading.Thread(
            target=_announce_camera,
            args=(default_brooder_id,),
            daemon=True
        ).start()

    try:
        HTTPServer(("0.0.0.0", args.port), StreamHandler).serve_forever()
    except KeyboardInterrupt:
        print("\n[camera] Shutting down...")
        picam2.stop()


if __name__ == "__main__":
    main()