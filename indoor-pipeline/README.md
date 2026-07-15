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

Full frames are uploaded with their YOLO detections as **reviewable pre-labels**
over the REST API (upload the image, then POST a YOLO `.txt` annotation with an
`annotation_labelmap` mapping the model's class indices → names). Uploads fire on
the timer (`upload_interval_seconds`) and on any detection
(`upload_on_detection`). The target **project follows the active mode**.

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
