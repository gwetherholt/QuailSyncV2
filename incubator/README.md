# Incubator capture pipeline (stage 1)

Data-capture only: grab frames from a fixed camera pointed at the incubator tray,
apply per-slot ROIs, run a per-slot frame-difference detector, and log change
events (plus saved crops) to SQLite. **No YOLO, no classifier, no Roboflow
upload, no backend/UI** — those are later stages. The crops this stage saves are
the labeling dataset the stage-2 classifier will be trained on.

It mirrors the structure/secrets/systemd conventions of the sibling
[`trailcam/`](../trailcam) and [`indoor-cam/`](../indoor-cam) pipelines and runs
directly on the Pi (outside Docker), alongside them.

## How it works

```
grab frame  ->  per-slot crop (ROI)  ->  per-slot diff score  ->  detector  ->  log event (+ crop)
```

Each capture cycle (`capture_interval_seconds`, default 10s):

1. **Capture** — `camera.py` grabs one frame from the configured source (an RTSP
   stream or an HTTP snapshot URL; `cv2.VideoCapture` handles both). The source
   is reached through the `FrameSource` protocol, so tests inject a
   `FakeFrameSource` and never touch a camera.
2. **Crop** — `roi.py` crops each slot's `[x, y, w, h]` region out of the frame.
3. **Score** — `diff.py` grayscales + Gaussian-blurs the crop and scores it as
   the **mean absolute difference** (0–255) against a per-slot running-average
   baseline: `baseline = (1 - alpha) * baseline + alpha * current`. Because the
   score is a mean, it's comparable across differently-sized slots.
4. **Detect** — `detector.py` runs a two-level-hysteresis + cooldown state
   machine per slot so it doesn't flap (see below).
5. **Log** — on an event, `storage.py` saves the ROI crop and inserts a row into
   the `incubation_events` SQLite table.

### Detector state machine (anti-flap)

Each slot is `IDLE` or `ACTIVE`:

- **IDLE → ACTIVE** when `diff_score >= high_threshold`: emit exactly one event,
  stamp `last_event_time`, save a crop.
- **While ACTIVE**: emit no further events (the re-fire guard). If
  `freeze_baseline_while_active` is set, the slot's baseline stops updating so a
  *slow* hatch doesn't self-cancel by dragging the baseline toward the changed
  state.
- **ACTIVE → IDLE** only once `diff_score <= low_threshold` **and**
  `now - last_event_time >= cooldown_seconds`. Baseline updates then resume.
- Detection is suppressed for the first `min_frames_before_detect` frames after
  startup so the baseline can settle.

## Configuration — `config.json`

Tray geometry, thresholds, and paths are checked in. The **camera source is
not** — it's resolved at load time from the environment variable named by
`camera.source_env` (default `INCUBATOR_RTSP_URL`), which systemd loads from the
out-of-repo secrets file. `config.py` validates aggressively (bad bbox, duplicate
slot id, `low > high`, even blur kernel, …) and fails loudly at startup.

```jsonc
{
  "camera":    { "source_env": "INCUBATOR_RTSP_URL", "capture_interval_seconds": 10, "warmup_frames": 3 },
  "storage":   { "db_path": "~/QuailSyncV2/data/quailsync.db",
                 "captures_dir": "~/QuailSyncV2/incubator/captures",
                 "save_crops_on_event": true, "sqlite_busy_timeout_ms": 5000 },
  "detection": { "baseline_alpha": 0.02, "high_threshold": 18.0, "low_threshold": 8.0,
                 "cooldown_seconds": 120, "min_frames_before_detect": 5,
                 "freeze_baseline_while_active": true, "blur_kernel": 5 },
  "tray":      { "reference_image": "incubator/reference.jpg",
                 "slots": [ { "id": "A1", "bbox": [120, 80, 60, 60], "clutch_id": null } ] },
  "roboflow":  { "enabled": true, "project": "incubation-stages", "workspace": "quail",
                 "upload_interval_seconds": 1800, "upload_on_event": true,
                 "api_key_env": "ROBOFLOW_API_KEY" }
}
```

- `bbox` is `[x, y, w, h]` in pixels (top-left origin).
- `clutch_id` is optional and static for now — `null` is fine. A populated value
  is how per-slot identity gets attached to the live clutches in a later stage.
- Point `$INCUBATOR_CONFIG` at an alternate file to override the default.

## Roboflow auto-upload

To build the stage-2 labeling dataset, the pipeline auto-uploads **raw**
(unannotated — no model exists yet) frames to Roboflow, mirroring the trail-cam /
indoor-cam pipelines but via the **REST upload API** (not the SDK):

- One **full frame every `upload_interval_seconds`** (default 1800 = 30 min),
  independent of detection — this captures variety across lighting, turner
  positions, and time of day.
- The **full frame on every change-detection event** (the interesting frames:
  pipping, hatching).

Uploads go to `workspace/project` (`quail/incubation-stages`) under the batch
`incubator-auto` so they're distinguishable from manual uploads. It's opt-in and
best-effort: with `roboflow.enabled` false, or `ROBOFLOW_API_KEY` unset, uploads
are skipped **silently** and never break capture. See
[`.incubator-secrets.example`](.incubator-secrets.example) for the key.

## Database

Written into the shared `quailsync.db` that the Rust backend also holds open, so
storage uses **WAL** mode, sets `busy_timeout`, and keeps every write a single
short auto-committed statement.

**The Rust backend owns the `incubation_events` schema.** It creates the table
and its indexes through its migration layer, so the backend must have booted (run
its migrations) **before** the incubator sidecar starts writing — the sidecar
assumes the table already exists and no longer creates it. The authoritative
definition is:

```sql
CREATE TABLE IF NOT EXISTS incubation_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slot_id TEXT NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'change_detected',
    diff_score REAL NOT NULL,
    high_threshold REAL NOT NULL,
    clutch_id INTEGER,
    frame_path TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
```

The backend also exposes read-only endpoints over this table
(`GET /incubation/events`, `GET /incubation/summary`); the sidecar remains the
only writer.

Crops are saved to `captures_dir/YYYY-MM-DD/slot_<id>_<UTC-timestamp>.jpg` and
that path is recorded in `frame_path`.

## Defining slot ROIs — `define_rois.py`

Run this manually on the Pi (which can reach the camera) to bootstrap or tweak
the tray layout:

```bash
# Grab a reference frame and re-draw the CURRENT config.json slots over it:
python define_rois.py

# Propose a fresh 3x4 grid; prints the tray.slots JSON to stdout (config.json
# is NOT touched unless --write is passed):
python define_rois.py --grid 3x4

# ...with custom margins/gaps (fractions of the image), then persist it:
python define_rois.py --grid 3x4 --margin-x 0.08 --margin-y 0.06 --write
```

Outputs `incubator/reference.jpg` and an annotated `incubator/reference_annotated.jpg`
with the slot boxes drawn on for eyeballing alignment. (Both are git-ignored.)

## Running

```bash
python3 -m venv venv
venv/bin/pip install -r requirements.txt   # opencv-python-headless + numpy + requests

export INCUBATOR_RTSP_URL='rtsp://user:pass@camera-ip:554/stream1'
export ROBOFLOW_API_KEY='...'   # optional — enables raw-frame auto-upload
python main.py --once     # one capture cycle, then exit
python main.py --loop     # run continuously (what systemd runs)
```

### As a service

`../deploy/incubator-pipeline.service` mirrors `indoor-cam-pipeline.service`:
runs `main.py --loop` as `gwetherholt`, `WorkingDirectory` in the incubator dir,
`Restart=on-failure`, and loads `~/.incubator-secrets` via `EnvironmentFile`.
Create the secrets file from
[`.incubator-secrets.example`](.incubator-secrets.example):

The secrets file holds `INCUBATOR_RTSP_URL` and — for the raw-frame auto-upload —
`ROBOFLOW_API_KEY` (omit it to skip uploads silently).

```bash
install -m 600 /dev/null /home/gwetherholt/.incubator-secrets
editor /home/gwetherholt/.incubator-secrets      # set INCUBATOR_RTSP_URL (+ ROBOFLOW_API_KEY)
sudo cp ../deploy/incubator-pipeline.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now incubator-pipeline   # enable only when ready
```

## Tests

```bash
python -m pytest        # from the incubator/ dir
```

The suite mocks the camera with a `FakeFrameSource` and never hits the network or
a real device. `tests/conftest.py` puts this package dir on `sys.path` so tests
import the modules by bare name (`import config`, `import roi`, …).

## Out of scope (later stages)

YOLO / classifier / state labeling (egg / pipped / chick); Roboflow upload; Rust
backend endpoints or migrations; Android/web UI; wiring `clutch_id` to the live
clutches/incubation tables beyond the static config field.
