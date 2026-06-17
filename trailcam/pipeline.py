"""Trail-camera pipeline orchestrator.

Ties the three stages together:

    SpyPoint poll  ->  YOLO detection  ->  QuailSync observation

* ``run_once()`` runs one full cycle (poll new photos, detect over staging,
  post observations) and returns ``(success_count, failure_count)``.
* ``run_loop()`` runs ``run_once()`` forever, ``POLL_INTERVAL`` seconds apart,
  shutting down cleanly on SIGTERM/SIGINT (it sleeps in 1-second increments so
  a stop request is honoured within ~1s rather than after a full interval).

CLI:

    python pipeline.py --mode once          # one cycle, then exit
    python pipeline.py --mode loop           # run continuously (systemd)
    python pipeline.py --mode loop --log-level DEBUG
"""

from __future__ import annotations

import argparse
import logging
import signal
import threading
from pathlib import Path

# Support both `python pipeline.py` (script) and `from trailcam import …`.
try:
    from . import config
    from .spypoint_poller import PhotoState, SpypointPoller
    from .yolo_detector import process_staging
    from .quailsync_bridge import QuailSyncBridge
    from .roboflow_uploader import upload_if_enabled
except ImportError:
    import config
    from spypoint_poller import PhotoState, SpypointPoller
    from yolo_detector import process_staging
    from quailsync_bridge import QuailSyncBridge
    from roboflow_uploader import upload_if_enabled

logger = logging.getLogger("trailcam.pipeline")


def _require_credentials() -> None:
    if not (config.SPYPOINT_USERNAME and config.SPYPOINT_PASSWORD):
        raise RuntimeError(
            "SPYPOINT_USERNAME / SPYPOINT_PASSWORD are not set in the environment"
        )


def _build_poller(state: PhotoState) -> SpypointPoller:
    return SpypointPoller(
        username=config.SPYPOINT_USERNAME,
        password=config.SPYPOINT_PASSWORD,
        staging_dir=config.STAGING_DIR,
        state=state,
        limit=config.PHOTO_LIMIT,
    )


def run_once(
    poller: SpypointPoller | None = None,
    bridge: QuailSyncBridge | None = None,
) -> tuple[int, int]:
    """Run one poll -> detect -> post cycle.

    ``poller``/``bridge`` may be supplied so the loop reuses one logged-in
    client/session across cycles; when omitted they're built from ``config``.
    Returns ``(success_count, failure_count)`` from the observation post.
    """
    config.ensure_dirs()

    if poller is None:
        _require_credentials()
        state = PhotoState(config.BASE_DIR / "photo_state.json")
        poller = _build_poller(state)
    if bridge is None:
        bridge = QuailSyncBridge()

    # 1. Poll SpyPoint for new photos into staging/.
    downloaded = poller.poll()
    logger.info("Poll: %d new photo(s) downloaded", len(downloaded))

    # 2. Run YOLO over everything currently staged (includes any leftovers from
    #    a previous cycle that failed after download), moving finished sets to
    #    processed/.
    results = process_staging()

    # 3. Post the detections to QuailSync (currently the JSONL fallback).
    success, failure = bridge.post_batch(results)
    logger.info("Cycle complete: %d observation(s) posted, %d failed", success, failure)

    # 4. Optionally push images + predictions to Roboflow as reviewable
    #    pre-labels. Opt-in (ROBOFLOW_UPLOAD_ENABLED) and best-effort — a no-op
    #    when disabled / unconfigured, and never raises into the cycle.
    upload_if_enabled(results)

    return success, failure


def run_loop() -> None:
    """Run ``run_once()`` on a ``POLL_INTERVAL`` cadence until signalled to stop.

    SIGTERM/SIGINT set a stop flag; the current cycle finishes and then the loop
    exits. The inter-poll wait is done in 1-second steps so shutdown is prompt.
    """
    config.ensure_dirs()
    _require_credentials()

    state = PhotoState(config.BASE_DIR / "photo_state.json")
    poller = _build_poller(state)
    bridge = QuailSyncBridge()

    stop = threading.Event()

    def _handle_signal(signum, _frame):
        logger.info("Received %s — will stop after the current cycle", signal.Signals(signum).name)
        stop.set()

    signal.signal(signal.SIGTERM, _handle_signal)
    signal.signal(signal.SIGINT, _handle_signal)

    logger.info("Starting poll loop (interval %ds)", config.POLL_INTERVAL)
    while not stop.is_set():
        try:
            run_once(poller=poller, bridge=bridge)
        except Exception:  # noqa: BLE001 — never let one bad cycle kill the loop
            logger.exception("Poll cycle failed; retrying next interval")

        # Sleep in 1-second increments so SIGTERM/SIGINT are honoured promptly.
        for _ in range(config.POLL_INTERVAL):
            if stop.wait(1):
                break

    logger.info("Poll loop stopped cleanly.")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="QuailSync trail-camera pipeline")
    parser.add_argument(
        "--mode",
        choices=["once", "loop"],
        default="once",
        help="run a single cycle ('once') or poll continuously ('loop')",
    )
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
        if args.mode == "once":
            run_once()
        else:
            run_loop()
    except RuntimeError as exc:
        logger.error("%s", exc)
        return 1
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
