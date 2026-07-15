"""Assignment-aware indoor pipeline service loop (stage 3).

Unifies the incubator and indoor-cam pipelines into one service that reads the
camera assignment from the backend and runs the matching YOLO model:

    poll assignment  ->  (hot-swap model if changed)  ->  grab frame
                     ->  run YOLO  ->  log events (incubator mode) + upload to Roboflow

* ``run_once()`` polls the assignment when due, ensures the right model is
  loaded, grabs one frame, runs inference, logs incubation events (only in
  incubation mode with ``log_events``), and uploads to Roboflow (timer + on
  detection). Returns the detections.
* ``run_loop()`` calls ``run_once()`` every ``capture_interval_seconds`` until
  SIGTERM/SIGINT, sleeping in 1-second steps so shutdown is prompt.

Model hot-swap: when the polled ``active_model`` differs from what's loaded, the
loop logs the switch, unloads the current model, loads the new one, and resets
the Roboflow upload timer + retargets the uploader's project. The service is
never restarted for a reassignment. If the backend is unreachable the last-known
model keeps running and the loop retries next poll. A model that fails to load
(missing weights) leaves inference skipped and is retried each cycle.

CLI::

    python main.py --once                  # one cycle, then exit
    python main.py --loop                  # run continuously (systemd)
    python main.py --loop --log-level DEBUG
    python main.py --config /path/to/config.json
"""

from __future__ import annotations

import argparse
import logging
import signal
import threading
import time
from datetime import datetime, timezone

try:
    from . import config as config_module
    from . import camera as camera_module
    from . import roboflow_uploader as roboflow_module
    from . import snapshots as snapshots_module
    from . import observations as observations_module
    from .assignment import AssignmentPoller
    from .detector import Detector
    from .storage import EventStore
except ImportError:  # plain-script / bare-name import (tests)
    import config as config_module
    import camera as camera_module
    import roboflow_uploader as roboflow_module
    import snapshots as snapshots_module
    import observations as observations_module
    from assignment import AssignmentPoller
    from detector import Detector
    from storage import EventStore

logger = logging.getLogger("indoorpipeline.main")

# Sentinel so callers can pass ``uploader=None`` / ``store=None`` to force a
# component off, distinct from "not supplied" (build lazily / from config).
_UNSET = object()


class IndoorPipeline:
    """Holds the long-lived state (frame source, detector, poller, DB) across cycles."""

    def __init__(
        self,
        conf,
        *,
        frame_source=None,
        detector=None,
        poller=None,
        uploader=_UNSET,
        store=_UNSET,
        observation_client=_UNSET,
        cv2_module=None,
        yolo_factory=None,
        session=None,
    ):
        self.conf = conf
        self.cv2_module = cv2_module
        self.frame_source = frame_source or camera_module.create_frame_source(
            conf, cv2_module=cv2_module
        )
        self.detector = detector or Detector(yolo_factory=yolo_factory)
        self.poller = poller or AssignmentPoller(
            conf.assignment.backend_url,
            conf.assignment.camera_id,
            conf.assignment.default_mode,
            session=session,
        )
        # Roboflow uploader is targeted at the *initial* mode's project; it's
        # retargeted on every swap. ``None`` when disabled / unkeyed.
        initial_cfg = conf.models[self.poller.mode]
        self.uploader = (
            roboflow_module.build_uploader(conf, initial_cfg.roboflow_project)
            if uploader is _UNSET
            else uploader
        )
        # Observation POSTing so the dashboard/app show live data. ``None`` when
        # disabled (or forced off via observation_client=None).
        self.observation_client = (
            observations_module.build_observation_client(conf, session=session)
            if observation_client is _UNSET
            else observation_client
        )
        # EventStore is opened lazily on first incubation-mode write (chick-only
        # runs never touch the DB). ``None`` sentinel via _UNSET forces it off.
        self._store = None if store is _UNSET else store
        self._store_forced_off = store is None
        # The mode currently loaded into the detector (None until first load).
        self._active_mode: str | None = None
        self._last_poll_time: float | None = None
        self._last_upload_time: float | None = None

    # --- assignment polling + model swap ----------------------------------

    def _maybe_poll(self, now: float) -> None:
        """Poll the backend when the poll interval is due (and always on first
        cycle). Updates ``self.poller.mode`` in place."""
        due = (
            self._last_poll_time is None
            or (now - self._last_poll_time) >= self.conf.assignment.poll_seconds
        )
        if not due:
            return
        self._last_poll_time = now
        self.poller.poll()

    def _ensure_model(self) -> bool:
        """Load the model for the current mode if it isn't already. Returns
        whether a usable model is loaded (False = skip inference this cycle)."""
        mode = self.poller.mode
        cfg = self.conf.models.get(mode)
        if cfg is None:  # pragma: no cover - guarded by config validation
            logger.error("No model configured for mode %r — skipping inference", mode)
            return False

        if self._active_mode == mode and self.detector.loaded:
            return True

        if self._active_mode is not None and self._active_mode != mode:
            logger.info("Assignment changed — swapping model %s -> %s", self._active_mode, mode)

        if self.detector.load(cfg.weights, cfg.confidence):
            self._on_swap(mode, cfg)
            return True
        # Load failed (e.g. missing weights): leave unloaded; retry next cycle.
        return False

    def _on_swap(self, mode: str, cfg) -> None:
        """React to a completed model (re)load: retarget uploads, reset the
        upload timer, and reset any per-frame baselines (YOLO has none)."""
        self._active_mode = mode
        # Reset the Roboflow cadence so the new model gets a fresh upload window,
        # and point uploads at the new mode's project.
        self._last_upload_time = None
        if self.uploader is not None:
            self.uploader.project = cfg.roboflow_project
        logger.info("Now running mode %r (weights=%s, project=%s)", mode, cfg.weights, cfg.roboflow_project)

    # --- incubation event logging (incubator mode only) -------------------

    def _event_store(self) -> "EventStore | None":
        """Return the EventStore, opening it lazily on first use. ``None`` if
        forced off in construction."""
        if self._store_forced_off:
            return None
        if self._store is None:
            self._store = EventStore(
                self.conf.storage.db_path, self.conf.storage.sqlite_busy_timeout_ms
            )
        return self._store

    def _log_events(self, detections, cfg) -> None:
        """Log each YOLO detection to ``incubation_events`` (incubation mode with
        ``log_events`` only)."""
        if cfg.mode != config_module.MODE_INCUBATION or not cfg.log_events:
            return
        store = self._event_store()
        if store is None:
            return
        for det in detections:
            try:
                store.record_detection(
                    event_type=det.class_name,
                    confidence=det.confidence,
                    slot_id=self.conf.assignment.camera_id,
                    confidence_threshold=cfg.confidence,
                )
            except Exception:  # noqa: BLE001 — a DB hiccup must not lose the cycle
                logger.exception("Failed to log incubation event for class %s", det.class_name)

    # --- Roboflow upload --------------------------------------------------

    def _maybe_upload(self, frame, detections, now: float, when, mode: str) -> None:
        """Upload the full frame (+ YOLO pre-labels) on the timer and/or on any
        detection. At most one upload per cycle; best-effort — never raises."""
        if self.uploader is None:
            return
        rf = self.conf.roboflow
        timer_due = (
            self._last_upload_time is None
            or (now - self._last_upload_time) >= rf.upload_interval_seconds
        )
        detection_due = rf.upload_on_detection and len(detections) > 0
        if not (timer_due or detection_due):
            return

        reason = "timer" if timer_due else "detection"
        name = f"indoor_{mode}_{when:%Y%m%dT%H%M%S%fZ}_{reason}.jpg"
        try:
            self.uploader.upload_frame(frame, name, detections, cv2_module=self.cv2_module)
        except Exception:  # noqa: BLE001 — upload is best-effort, never fatal
            logger.warning("Roboflow upload raised; continuing", exc_info=True)
        # Only a timer-driven upload advances the cadence, so detection uploads
        # don't shift the periodic schedule.
        if timer_due:
            self._last_upload_time = now

    # --- rolling snapshots (live feed) ------------------------------------

    def _write_snapshots(self, frame, detections) -> None:
        """Write latest.jpg (raw) + latest_annotated.jpg (YOLO boxes) atomically.

        The backend/app serve these as the live feed. Best-effort — a write
        failure is logged but never breaks the cycle. Skipped when no snapshot
        paths are configured."""
        snap = self.conf.snapshots
        if snap is None:
            return
        try:
            snapshots_module.write_snapshots(
                frame,
                detections,
                snap.latest_path,
                snap.latest_annotated_path,
                cv2_module=self.cv2_module,
            )
        except Exception:  # noqa: BLE001 — snapshot writing must not break the loop
            logger.warning("Failed to write rolling snapshot(s); continuing", exc_info=True)

    # --- observation POST (live dashboard/app data) -----------------------

    def _post_observation(self, detections, when) -> None:
        """POST this cycle's observation so the dashboard/app show live data.

        The image fields point at the rolling snapshot basenames (``latest.jpg`` /
        ``latest_annotated.jpg``) written this cycle, which the backend serves
        from ``processed/{camera_id}/``. Best-effort — the client swallows POST
        failures so an unreachable backend never breaks the loop."""
        if self.observation_client is None:
            return
        image_filename = annotated_image_filename = None
        snap = self.conf.snapshots
        if snap is not None:
            image_filename = snap.latest_path.name
            annotated_image_filename = snap.latest_annotated_path.name
        self.observation_client.post(
            detections,
            timestamp=when.isoformat(),
            image_filename=image_filename,
            annotated_image_filename=annotated_image_filename,
        )

    # --- the cycle --------------------------------------------------------

    def run_once(self, *, now: float | None = None):
        """One cycle: poll -> ensure model -> grab -> detect -> log/upload."""
        now = time.time() if now is None else now
        self._maybe_poll(now)

        mode = self.poller.mode
        cfg = self.conf.models.get(mode)
        if not self._ensure_model():
            logger.warning("No usable model for mode %r this cycle — skipping inference", mode)
            return []

        frame = self.frame_source.grab()
        if frame is None:
            logger.warning("No frame this cycle — skipping")
            return []

        when = datetime.fromtimestamp(now, tz=timezone.utc)
        detections = self.detector.detect(frame)
        logger.info(
            "Mode=%s: %d detection(s) [%s]",
            mode,
            len(detections),
            ", ".join(f"{d.class_name}:{d.confidence:.2f}" for d in detections) or "none",
        )

        # Refresh the rolling live-feed snapshots first so the backend/app get a
        # current frame even if logging/upload are slow, then POST the observation
        # so the dashboard/app pick up the fresh count + image.
        self._write_snapshots(frame, detections)
        self._post_observation(detections, when)
        self._log_events(detections, cfg)
        self._maybe_upload(frame, detections, now, when, mode)
        return detections

    def close(self) -> None:
        try:
            self.frame_source.close()
        finally:
            self.detector.unload()
            if self._store is not None:
                self._store.close()


def run_loop(conf, *, pipeline=None) -> None:
    """Run ``run_once`` every ``capture_interval_seconds`` until signalled."""
    config_module.ensure_dirs(conf)
    pipeline = pipeline or IndoorPipeline(conf)

    stop = threading.Event()

    def _handle_signal(signum, _frame):
        logger.info("Received %s — stopping after this cycle", signal.Signals(signum).name)
        stop.set()

    signal.signal(signal.SIGTERM, _handle_signal)
    signal.signal(signal.SIGINT, _handle_signal)

    interval = conf.camera.capture_interval_seconds
    logger.info(
        "Starting indoor pipeline loop: capture %.1fs, assignment poll %.0fs, backend %s",
        interval,
        conf.assignment.poll_seconds,
        conf.assignment.backend_url,
    )
    try:
        while not stop.is_set():
            try:
                pipeline.run_once()
            except Exception:  # noqa: BLE001 — one bad cycle must not kill the loop
                logger.exception("Capture cycle failed; retrying next interval")
            # Sleep in 1-second steps so SIGTERM/SIGINT are honoured promptly.
            waited = 0.0
            while waited < interval and not stop.is_set():
                stop.wait(min(1.0, interval - waited))
                waited += 1.0
    finally:
        pipeline.close()
    logger.info("Indoor pipeline loop stopped cleanly.")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="QuailSync indoor pipeline (stage 3, assignment-aware)")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--once", action="store_true", help="run a single cycle, then exit")
    mode.add_argument("--loop", action="store_true", help="run continuously (default; used by systemd)")
    parser.add_argument("--config", default=None, help="path to config.json (default: indoor-pipeline/config.json or $INDOOR_PIPELINE_CONFIG)")
    parser.add_argument(
        "--log-level",
        default="INFO",
        type=lambda s: s.upper(),
        choices=["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"],
        help="logging verbosity (default: INFO)",
    )
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=getattr(logging, args.log_level, logging.INFO),
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    try:
        conf = config_module.load_config(args.config)
    except config_module.ConfigError as exc:
        logger.error("Bad configuration: %s", exc)
        return 1

    try:
        if args.once:
            config_module.ensure_dirs(conf)
            pipeline = IndoorPipeline(conf)
            try:
                pipeline.run_once()
            finally:
                pipeline.close()
        else:
            run_loop(conf)
    except camera_module.CaptureError as exc:
        logger.error("%s", exc)
        return 1
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
