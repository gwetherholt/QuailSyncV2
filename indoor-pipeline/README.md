# Indoor pipeline (stage 3) — assignment-aware YOLO

One service that **supersedes both** `incubator-pipeline.service` (frame-diff ROI
event logging) and `indoor-cam-pipeline.service` (YOLO chick detection). Instead
of two pipelines hard-wired to two cameras/models, this single
`indoor-pipeline.service` reads the indoor camera's **assignment** from the
backend and runs the matching YOLO model — hot-swapping in-process when the
assignment changes, never restarting.

```
  poll assignment ──▶ (hot-swap model if changed) ──▶ grab frame
        ▲                                                  │
        │ every poll_seconds (60s)                         ▼
   backend GET                                         run YOLO
   /api/cameras/indoor_tapo/assignment                     │
                                          ┌─────────────────┴─────────────────┐
                                          ▼                                    ▼
                          log incubation_events                    upload frame + YOLO
                          (incubation mode only)                  pre-labels → Roboflow
```

## How the model is chosen

The backend owns the mapping (see `active_model_for()` in `quailsync-common`);
this pipeline just reads the derived `active_model` field:

| camera assignment | backend `active_model` | weights                    | Roboflow project    | incubation events |
| ----------------- | ---------------------- | -------------------------- | ------------------- | ----------------- |
| `incubator`       | `incubation`           | `incubator/models/incubation-best.pt` | `incubation-stages` | **yes**           |
| `brooder`         | `chick`                | `indoor-cam/models/chick-best.pt`     | `find-chicks-5`     | no                |

On startup and every `assignment.poll_seconds` (default 60), the loop does
`GET {backend_url}/api/cameras/{camera_id}/assignment` and reads `active_model`.
`config.json`'s `models` map is keyed by that value (`incubation` / `chick`).

- **Model hot-swap** — when the polled `active_model` differs from what's loaded,
  the loop logs the switch, unloads the current model, loads the new one, resets
  the Roboflow upload timer, and retargets the uploader's project. No restart.
- **Backend unreachable** — the last-known model keeps running; the loop retries
  next poll (`assignment.default_mode` is used only before the first successful
  poll — it may be an assignment name like `incubator` or a model name like
  `incubation`; both resolve).
- **Model not found** — a missing `.pt` is logged and inference is skipped for
  that cycle; the load is retried each cycle until the file appears.

## Modules

| file                   | responsibility                                                             |
| ---------------------- | -------------------------------------------------------------------------- |
| `config.py`            | load + validate `config.json`; resolve camera URL / API key from env; assignment→model mode mapping |
| `camera.py`            | `FrameSource` protocol + OpenCV RTSP/snapshot grabber (adapted from `incubator/camera.py`) |
| `detector.py`          | YOLO inference wrapper with in-process model hot-swap; `Detection(class_name, class_id, confidence, bbox)` |
| `assignment.py`        | polls the backend assignment endpoint; tracks current vs previous mode; resilient to outages |
| `roboflow_uploader.py` | REST upload of the full frame **+ YOLO pre-annotations** (with `annotation_labelmap`); project per mode |
| `storage.py`           | `incubation_events` logging (incubation mode only); WAL + `busy_timeout`; backend owns the schema |
| `snapshots.py`         | rolling `latest.jpg` (raw) + `latest_annotated.jpg` (YOLO boxes), written atomically each cycle |
| `observations.py`      | POST one observation per cycle to the backend (mirrors the old indoor-cam bridge) so the dashboard/app show live data |
| `main.py`              | the service loop tying it all together                                     |

The heavy imports (`cv2`, `ultralytics`, `requests`) are all lazy — importing any
module is cheap, and the tests run without a camera, torch, or the network.

## Configuration

Everything non-secret lives in `config.json` (see the shipped file). The two
**secrets** are resolved from the environment at load time and never live in the
repo:

- `INDOOR_RTSP_URL` — the Tapo camera RTSP URL (embeds credentials)
- `ROBOFLOW_API_KEY` — required only for uploads; unset = uploads skipped silently

Both come from the out-of-repo `~/.indoor-pipeline-secrets` file that the systemd
unit loads via `EnvironmentFile=`. Copy `.indoor-pipeline-secrets.example` to
create it. Point at an alternate config with `$INDOOR_PIPELINE_CONFIG`.

### Incubation event logging

In **incubation mode** (and only when the mode's `log_events` is true), each YOLO
detection becomes one `incubation_events` row. The schema predates YOLO, so a
couple of columns are repurposed:

- `event_type` ← the detected class name (`egg`, `pipped`, …)
- `diff_score` ← the detection confidence
- `high_threshold` ← the model's confidence threshold (the bar the detection cleared)
- `slot_id` ← the camera id (there are no per-slot ROIs anymore)

As before, the **Rust backend owns the `incubation_events` schema** (created by
its migration layer); this sidecar is a write-only co-tenant that assumes the
table exists, opens the shared DB in WAL mode, and sets `busy_timeout`.

### Roboflow uploads

Full frames are uploaded over the REST API to the active mode's **project**
(incubation → `incubation-stages`, chick → `find-chicks-5`). Incubation frames go
up **image-only** (no annotations, matching the `incubator/` pipeline); chick
frames also POST a YOLO `.txt` pre-annotation.

**Frequency / throttling:**

- A periodic upload every `upload_interval_seconds` (config.json, default 1800s).
- Optional on-detection uploads, **env-driven and OFF by default** — set
  `ROBOFLOW_UPLOAD_ON_DETECTION=true` to enable.
- A hard floor between *any* two uploads, `ROBOFLOW_MIN_UPLOAD_SPACING_S` (env,
  default 1800s), enforced regardless of trigger — so even with on-detection
  enabled the pipeline can't flood Roboflow.

The effective settings are printed in the `Roboflow auto-upload enabled …`
startup log line and by `python config.py`.

### Rolling live-feed snapshots

Each cycle overwrites two flat files the backend/app serve as the live feed:

- `latest.jpg` — the raw frame
- `latest_annotated.jpg` — a copy with each detection's box + class label +
  confidence drawn on it (e.g. `egg 91%` / `chick 80%`); when there are no
  detections it's a plain copy of the raw frame, so the file always exists and
  stays fresh.

Both are written **atomically** (encode → sibling `.tmp` → `os.replace`), so the
backend never serves a half-written JPEG. Configure the paths under `snapshots`
in `config.json` — they must match where the backend reads indoor-cam images:
`{INDOORCAM_PROCESSED_DIR}/{camera_id}/latest.jpg` (and `…_annotated.jpg`). With
the default deploy that's `~/indoor-cam/processed/indoor-1/` on the host,
bind-mounted into the server container at `/indoor-cam/processed`. Omit the
`snapshots` section to disable snapshot writing.

### Observation POSTing (live dashboard/app data)

The dashboard and app show the indoor camera's live count + image by reading
`GET /api/indoorcam/latest/{camera_id}`, which only has data once the pipeline
**POSTs observations**. Each cycle the pipeline POSTs one observation to
`{backend_url}/api/indoorcam/observation` (the exact endpoint + payload the old
`indoor-cam` bridge used), carrying `camera_id`, `detection_count`, the
`detections` array (each box's real `class_name` — `egg`/`chick`, never
hardcoded), a timestamp, and the rolling-snapshot image basenames. Configure it
under `observations` in `config.json`; a failed POST (backend unreachable) is
logged and swallowed so it never breaks the loop. Omit the section (or set
`enabled: false`) to disable.

### The two camera IDs (`indoor-1` vs `indoor_tapo`)

There are **two distinct camera ids**, on purpose:

- `assignment.camera_id` = **`indoor_tapo`** — only drives the mode toggle
  (`GET /api/cameras/indoor_tapo/assignment`). Do not change it.
- `observations.camera_id` + the `snapshots` output dir = **`indoor-1`** — the
  serving id the backend, dashboard, and app key on for the live feed. The
  snapshot files therefore live in `…/processed/indoor-1/` and the observations
  post `camera_id: "indoor-1"`.

The backend's `GET /api/indoorcam/latest/{camera_id}` also returns a
`class_counts` breakdown (`{"egg": 5}`) and a ready `detection_label`
(`"5 eggs detected"`) derived from the model's actual classes — so the web
dashboard and Android app label the count by what the camera's mode really
detects instead of hardcoding "chicks".

## Running

```bash
python main.py --once                  # one cycle, then exit
python main.py --loop                  # run continuously (systemd uses this)
python main.py --loop --log-level DEBUG
python main.py --config /path/to/config.json
python config.py                       # load + validate + print resolved config
```

## Deploying (systemd)

Unit file: `deploy/indoor-pipeline.service`. Venv at
`~/QuailSyncV2/indoor-pipeline/venv/`:

```bash
python3 -m venv /home/gwetherholt/QuailSyncV2/indoor-pipeline/venv
/home/gwetherholt/QuailSyncV2/indoor-pipeline/venv/bin/pip install -r requirements.txt
```

### Migration from the two old services

This unit **replaces** `incubator-pipeline` and `indoor-cam-pipeline`. Once the
venv, weights, and `~/.indoor-pipeline-secrets` are in place:

```bash
sudo systemctl stop incubator-pipeline indoor-cam-pipeline
sudo systemctl disable incubator-pipeline indoor-cam-pipeline
sudo cp deploy/indoor-pipeline.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now indoor-pipeline
journalctl -u indoor-pipeline -f
```

> These steps are documented here, not executed. The old `incubator/` and
> `indoor-cam/` code is left untouched — this directory supersedes both.

## Testing

```bash
cd indoor-pipeline
python -m pytest -q
```

The suite fakes every heavy dependency (scripted frame source, fake YOLO factory,
fake assignment session, fake Roboflow uploader, temp SQLite DB) — no camera,
ultralytics, or network. `tests/conftest.py` puts this directory on `sys.path` so
modules import by bare name, and carries the test-only `incubation_events` DDL
(kept in sync with the backend migration).

## Out of scope

Backend / assignment endpoints, the trail-cam pipeline, Android/web UI, and the
old frame-diff logic (replaced by YOLO). The existing `incubator/` and
`indoor-cam/` directories are not modified.
