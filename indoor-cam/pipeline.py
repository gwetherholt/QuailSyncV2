"""Indoor-camera pipeline orchestrator — continuous ~1fps stream processing.

Unlike the trail cam (a snapshot poller), the indoor camera holds its RTSP
stream open and processes it continuously:

    open RTSP (OpenCV)  ->  sample ~1 frame/sec  ->  YOLO (chick model)  ->
    rolling-median smoothing  ->  smart-batched POST  ->  Roboflow active learning

Smart batching: a rolling median over the last N frames smooths the count; we
POST at most every ``POST_INTERVAL`` seconds, but immediately when the smoothed
count moves by ``COUNT_CHANGE_THRESHOLD``. Raw per-frame counts log at DEBUG;
POSTs log at INFO.

Image storage: the JSON observation is posted every cycle, but a frame is only
written to disk when notable (count change / low confidence / startup /
hourly heartbeat). Saved frames that upload to Roboflow are deleted on success;
the rest are auto-pruned past ``IMAGE_RETENTION_DAYS``.

CLI:

    python pipeline.py                       # run the stream (systemd)
    python pipeline.py --log-level DEBUG
    python pipeline.py --max-frames 5        # bounded run (smoke test)
"""

from __future__ import annotations

import argparse
import logging
import re
import signal
import statistics
import threading
import time
from collections import deque
from datetime import datetime, timezone
from pathlib import Path

try:
    from . import config
    from .bridge import IndoorBridge
except ImportError:
    import config
    from bridge import IndoorBridge

logger = logging.getLogger("indoorcam.pipeline")

# Auto-prune cadence — once per day, independent of the per-frame loop.
PRUNE_INTERVAL_SECONDS = 24 * 3600


def _sanitize_segment(value: str, *, fallback: str = "indoor") -> str:
    """Make a string safe to use as a single path component (camera subdir)."""
    text = re.sub(r"[^A-Za-z0-9._-]", "_", str(value)).strip("._")
    return text or fallback


class CountBatcher:
    """Rolling-median smoothing + the smart-batch POST decision.

    Counts are pushed in per frame; :meth:`smoothed` is the median over the last
    ``window`` frames. :meth:`should_post` decides when to emit an observation.
    """

    def __init__(self, *, window: int, post_interval: float, change_threshold: int):
        self._counts: deque[int] = deque(maxlen=max(1, window))
        self.post_interval = post_interval
        self.change_threshold = change_threshold
        self.last_post_time: float | None = None
        self.last_posted_count: int | None = None

    def add(self, raw_count: int) -> int:
        self._counts.append(int(raw_count))
        return self.smoothed()

    @property
    def window(self) -> list[int]:
        return list(self._counts)

    def smoothed(self) -> int:
        if not self._counts:
            return 0
        return int(round(statistics.median(self._counts)))

    def should_post(self, smoothed: int, now: float) -> tuple[bool, str]:
        """Return ``(should_post, reason)``. Reasons: ``first`` (no prior post),
        ``count_change`` (moved >= threshold), ``interval`` (>= POST_INTERVAL)."""
        if self.last_post_time is None:
            return True, "first"
        if abs(smoothed - (self.last_posted_count or 0)) >= self.change_threshold:
            return True, "count_change"
        if (now - self.last_post_time) >= self.post_interval:
            return True, "interval"
        return False, ""

    def record_post(self, smoothed: int, now: float) -> None:
        self.last_post_time = now
        self.last_posted_count = smoothed


def run_stream(
    *,
    camera_id: str | None = None,
    capture=None,
    detect_fn=None,
    annotate_fn=None,
    bridge: IndoorBridge | None = None,
    uploader=None,
    batcher: CountBatcher | None = None,
    clock=time.monotonic,
    wall_clock=time.time,
    stop_event: threading.Event | None = None,
    install_signals: bool = True,
    max_iterations: int | None = None,
) -> None:
    """Run the continuous capture -> detect -> smart-batch-post loop.

    Every collaborator is injectable so the loop is fully testable without a
    camera, model, server, or Roboflow. ``max_iterations`` bounds the loop (tests
    / ``--max-frames``); ``stop_event`` + signals stop it gracefully in prod.
    """
    config.ensure_dirs()
    camera_id = camera_id or config.CAMERA_ID
    cam_slug = _sanitize_segment(camera_id)

    # Lazily wire the real implementations so importing this module (and the
    # tests that inject fakes) never requires opencv/ultralytics/trailcam/roboflow.
    if bridge is None:
        bridge = IndoorBridge()
    if batcher is None:
        batcher = CountBatcher(
            window=config.SMOOTHING_WINDOW,
            post_interval=config.POST_INTERVAL,
            change_threshold=config.COUNT_CHANGE_THRESHOLD,
        )
    if capture is None:
        from capture import StreamCapture

        capture = StreamCapture()
    if detect_fn is None:
        from detector import detect as detect_fn  # type: ignore[no-redef]
    if annotate_fn is None:
        from detector import annotate_image as annotate_fn  # type: ignore[no-redef]
    if uploader is None:
        from active_learning import ActiveLearningUploader

        uploader = ActiveLearningUploader()

    # Storage helpers (pure + disk ops) — imported here so tests can monkeypatch.
    from storage import delete_files, notable_reasons, persist_frame, prune_old_images

    if stop_event is None:
        stop_event = threading.Event()
    if install_signals:
        def _handle_signal(signum, _frame):
            logger.info(
                "Received %s — stopping after the current frame",
                signal.Signals(signum).name,
            )
            stop_event.set()

        signal.signal(signal.SIGTERM, _handle_signal)
        signal.signal(signal.SIGINT, _handle_signal)

    live_path = config.CAPTURE_DIR / f"{cam_slug}_live.jpg"
    dest_dir = config.PROCESSED_DIR / cam_slug
    fps = (1.0 / config.FRAME_INTERVAL) if config.FRAME_INTERVAL > 0 else 0.0
    logger.info(
        "Starting indoor-cam stream (camera=%s, ~%.1f fps sampling, post every %ds or ±%d, "
        "roboflow=%s)",
        camera_id,
        fps,
        config.POST_INTERVAL,
        config.COUNT_CHANGE_THRESHOLD,
        "on" if getattr(uploader, "enabled", False) else "off",
    )

    # Prune once at startup, then once per day.
    prune_old_images(config.PROCESSED_DIR, config.IMAGE_RETENTION_DAYS, now=wall_clock())
    last_prune = clock()

    last_image_time: float | None = None  # monotonic time of the last saved frame
    benchmarked = False
    consecutive_failures = 0
    iterations = 0
    saved_seq = 0  # monotonic per-run counter -> unique frame filenames

    try:
        while not stop_event.is_set():
            if max_iterations is not None and iterations >= max_iterations:
                break
            iterations += 1
            cycle_start = clock()

            # 1. Grab one fresh frame; reconnect with backoff on a drop.
            try:
                ok = capture.read_to(live_path)
            except Exception as exc:  # noqa: BLE001 — treat any read error as a drop
                logger.warning("Frame grab error: %s", exc)
                ok = False
            if not ok:
                consecutive_failures += 1
                backoff = min(
                    config.STREAM_RECONNECT_BACKOFF * consecutive_failures,
                    config.STREAM_MAX_BACKOFF,
                )
                logger.warning(
                    "No frame (failure #%d) — reconnecting in %.1fs",
                    consecutive_failures,
                    backoff,
                )
                try:
                    capture.reconnect()
                except Exception as exc:  # noqa: BLE001 — keep retrying next loop
                    logger.error("Reconnect failed: %s", exc)
                stop_event.wait(backoff)  # interruptible
                continue
            consecutive_failures = 0

            # 2. Run YOLO (chick model) on the sampled frame.
            try:
                result = detect_fn(live_path, camera_id)
            except Exception as exc:  # noqa: BLE001 — skip a bad frame, keep streaming
                logger.error("Inference failed: %s", exc)
                stop_event.wait(_remaining(clock, cycle_start))
                continue

            raw_count = result.total_count
            smoothed = batcher.add(raw_count)

            # Benchmark: log the first frame's inference time (throughput ceiling).
            if not benchmarked:
                benchmarked = True
                inf_ms = getattr(result, "inference_time_ms", None)
                if inf_ms:
                    ceil = 1000.0 / inf_ms if inf_ms > 0 else 0.0
                    logger.info(
                        "Benchmark: first-frame inference %.1f ms (~%.1f fps inference "
                        "ceiling on this host)",
                        inf_ms,
                        ceil,
                    )

            logger.debug("frame raw=%d smoothed=%d window=%s", raw_count, smoothed, batcher.window)

            now = clock()
            should, reason = batcher.should_post(smoothed, now)
            if should:
                is_first = batcher.last_post_time is None
                confidences = [d.confidence for d in result.detections]
                min_conf = min(confidences) if confidences else None
                secs_since_img = None if last_image_time is None else (now - last_image_time)

                # Decide whether this frame is notable enough to keep on disk.
                save_reasons = notable_reasons(
                    post_reason=reason,
                    min_confidence=min_conf,
                    is_first=is_first,
                    seconds_since_last_image=secs_since_img,
                    low_confidence_threshold=config.LOW_CONFIDENCE_THRESHOLD,
                    heartbeat_interval=config.HEARTBEAT_IMAGE_INTERVAL,
                )
                save = bool(save_reasons)

                persisted = annotated = None
                if save:
                    # Seq suffix guarantees a unique name even for two saves in
                    # the same second (second-granular timestamps would collide).
                    saved_seq += 1
                    stem = f"{datetime.now(timezone.utc):%Y%m%d-%H%M%S}_{cam_slug}_{saved_seq:05d}"
                    persisted = persist_frame(live_path, dest_dir, stem)
                    annotated = dest_dir / f"{stem}_annotated.jpg"
                    try:
                        annotate_fn(persisted, result, annotated)
                    except Exception as exc:  # noqa: BLE001 — annotation is best-effort
                        logger.warning("Annotation failed: %s", exc)
                    result.image_path = str(persisted)
                    last_image_time = now

                # 3. Always POST the JSON observation (image fields only set when saved).
                ts = datetime.now(timezone.utc).isoformat()
                observation_id = bridge.post(
                    result, timestamp=ts, detection_count=smoothed, include_image=save
                )
                batcher.record_post(smoothed, now)
                logger.info(
                    "POST count=%d reason=%s delivered=%s image=%s",
                    smoothed,
                    reason,
                    observation_id is not None,
                    ",".join(save_reasons) if save else "none",
                )

                # 4. Roboflow active learning on saved frames; on a successful
                #    upload, delete the local file AND clear the observation's
                #    image fields (so reads don't serve a now-404 URL). On
                #    failure, keep the file for a later retry / the daily prune.
                if save and uploader is not None:
                    if uploader.upload(result):
                        delete_files(persisted, annotated)
                        if observation_id is not None:
                            bridge.clear_image(observation_id)
                        logger.debug(
                            "Uploaded to Roboflow; removed local frame %s and cleared image refs",
                            persisted.name,
                        )
                    else:
                        logger.info(
                            "Kept %s (Roboflow upload disabled/failed) for retry",
                            persisted.name if persisted else "?",
                        )

            # 5. Daily auto-prune of any leftover saved frames.
            if (clock() - last_prune) >= PRUNE_INTERVAL_SECONDS:
                prune_old_images(config.PROCESSED_DIR, config.IMAGE_RETENTION_DAYS, now=wall_clock())
                last_prune = clock()

            # 6. Pace to ~FRAME_INTERVAL (interruptible so shutdown is prompt).
            stop_event.wait(_remaining(clock, cycle_start))
    finally:
        capture.release()
    logger.info("Indoor-cam stream stopped.")


def _remaining(clock, cycle_start) -> float:
    """Seconds left in the current frame budget (>= 0)."""
    return max(0.0, config.FRAME_INTERVAL - (clock() - cycle_start))


def _require_rtsp() -> None:
    if not config.rtsp_url():
        raise RuntimeError(
            "no RTSP source configured — set RTSP_URL or RTSP_HOST (+ credentials) "
            "in the indoor-cam secrets file"
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="QuailSync indoor-camera pipeline (continuous)")
    parser.add_argument(
        "--log-level",
        default="INFO",
        type=lambda s: s.upper(),
        choices=["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"],
        help="logging verbosity (default: INFO)",
    )
    parser.add_argument(
        "--max-frames",
        type=int,
        default=0,
        help="process at most N frames then exit (0 = run until stopped)",
    )
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=getattr(logging, args.log_level, logging.INFO),
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    try:
        _require_rtsp()
        run_stream(max_iterations=args.max_frames or None)
    except RuntimeError as exc:
        logger.error("%s", exc)
        return 1
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
