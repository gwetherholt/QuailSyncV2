"""Incubator capture service loop.

Ties the pieces together:

    grab frame  ->  per-slot crop  ->  per-slot diff + detect  ->  log event (+ crop)

* ``run_once()`` grabs one frame and runs every slot's detector against it,
  saving a crop and inserting an ``incubation_events`` row for each slot that
  transitioned IDLE→ACTIVE this frame. Returns the events emitted.
* ``run_loop()`` calls ``run_once()`` every ``capture_interval_seconds`` until
  SIGTERM/SIGINT, sleeping in 1-second steps so shutdown is prompt.

CLI::

    python main.py --once                 # one capture cycle, then exit
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
    from . import roi as roi_module
    from .detector import build_detectors
    from .storage import EventStore, save_crop
except ImportError:  # plain-script / top-level import
    import config as config_module
    import camera as camera_module
    import roi as roi_module
    from detector import build_detectors
    from storage import EventStore, save_crop

logger = logging.getLogger("incubator.main")


class IncubatorPipeline:
    """Holds the long-lived state (frame source, detectors, DB) across cycles."""

    def __init__(self, conf, *, frame_source=None, store=None, cv2_module=None):
        self.conf = conf
        self.cv2_module = cv2_module
        self.frame_source = frame_source or camera_module.create_frame_source(
            conf, cv2_module=cv2_module
        )
        self.store = store or EventStore(
            conf.storage.db_path, conf.storage.sqlite_busy_timeout_ms
        )
        self.detectors = build_detectors(
            conf.tray.slots, conf.detection, cv2_module=cv2_module
        )

    def run_once(self, *, now: float | None = None):
        """Grab one frame, run every slot, log events. Returns emitted events."""
        now = time.time() if now is None else now
        frame = self.frame_source.grab()
        if frame is None:
            logger.warning("No frame this cycle — skipping")
            return []

        when = datetime.fromtimestamp(now, tz=timezone.utc)
        events = []
        for slot in self.conf.tray.slots:
            detector = self.detectors[slot.id]
            crop = roi_module.crop(frame, slot.bbox)
            event = detector.process(crop, now)
            if event is None:
                continue

            frame_path = None
            if self.conf.storage.save_crops_on_event:
                try:
                    frame_path = save_crop(
                        crop,
                        self.conf.storage.captures_dir,
                        slot.id,
                        when,
                        cv2_module=self.cv2_module,
                    )
                except OSError as exc:  # a failed crop write must not lose the event
                    logger.error("Could not save crop for slot %s: %s", slot.id, exc)

            event_id = self.store.record_event(
                slot_id=event.slot_id,
                diff_score=event.diff_score,
                high_threshold=event.high_threshold,
                clutch_id=event.clutch_id,
                frame_path=frame_path,
                event_type=event.event_type,
            )
            logger.info(
                "Event #%d slot=%s diff=%.2f (>= %.2f) crop=%s",
                event_id,
                event.slot_id,
                event.diff_score,
                event.high_threshold,
                frame_path.name if frame_path else "<none>",
            )
            events.append(event)
        return events

    def close(self) -> None:
        try:
            self.frame_source.close()
        finally:
            self.store.close()


def run_loop(conf, *, pipeline=None) -> None:
    """Run ``run_once`` every ``capture_interval_seconds`` until signalled."""
    config_module.ensure_dirs(conf)
    pipeline = pipeline or IncubatorPipeline(conf)

    stop = threading.Event()

    def _handle_signal(signum, _frame):
        logger.info("Received %s — stopping after this cycle", signal.Signals(signum).name)
        stop.set()

    signal.signal(signal.SIGTERM, _handle_signal)
    signal.signal(signal.SIGINT, _handle_signal)

    interval = conf.camera.capture_interval_seconds
    logger.info(
        "Starting incubator capture loop: %d slot(s), interval %.1fs",
        len(conf.tray.slots),
        interval,
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
    logger.info("Incubator capture loop stopped cleanly.")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="QuailSync incubator capture pipeline (stage 1)")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--once", action="store_true", help="run a single capture cycle, then exit")
    mode.add_argument("--loop", action="store_true", help="run continuously (default; used by systemd)")
    parser.add_argument("--config", default=None, help="path to config.json (default: incubator/config.json or $INCUBATOR_CONFIG)")
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
            pipeline = IncubatorPipeline(conf)
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
