"""Build an MP4 timelapse from a camera's processed trail-cam frames.

Reads frames out of ``processed/{camera_id}/`` (where the pipeline leaves the
JPEG, its ``{stem}.json`` metadata sidecar, and its ``{stem}_detections.json``
YOLO results), keeps the ones whose capture date matches ``--date``, sorts them
by timestamp, optionally overlays detection boxes + a bird count, and stitches
them into ``processed/timelapses/{camera_id}_{date}.mp4`` with ffmpeg.

    python timelapse.py --camera CAM1 --date 2026-06-15 --fps 4 --annotate

``--annotate`` draws a green rectangle per detected bird and a "Birds: N" label
in the top-left of each frame; without it the raw images are used. ffmpeg must
be installed and on PATH.
"""

from __future__ import annotations

import argparse
import json
import logging
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime
from pathlib import Path

# Support both `python timelapse.py` (script) and `from trailcam import …`.
try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("trailcam.timelapse")

DEFAULT_FPS = 4
BOX_COLOR = (0, 255, 0)  # green bounding boxes
LABEL_FG = (255, 255, 255)
LABEL_BG = (0, 0, 0)


class FfmpegNotFound(RuntimeError):
    """Raised when the ffmpeg binary isn't available on PATH."""


# ---------------------------------------------------------------------------
# Frame discovery
# ---------------------------------------------------------------------------


def _frame_timestamp(image_path: Path) -> datetime | None:
    """Capture time for a frame, from its ``{stem}.json`` sidecar, falling back
    to the ``YYYYMMDD-HHMMSS`` prefix of the filename. ``None`` if unparseable."""
    sidecar = image_path.with_suffix(".json")
    if sidecar.exists():
        try:
            ts = json.loads(sidecar.read_text(encoding="utf-8")).get("timestamp")
            if ts:
                return datetime.fromisoformat(str(ts))
        except (json.JSONDecodeError, ValueError, OSError):
            pass
    try:
        return datetime.strptime(image_path.name[:15], "%Y%m%d-%H%M%S")
    except ValueError:
        return None


def collect_frames(camera_dir: Path, target_date) -> list[Path]:
    """Return the camera's JPEGs captured on ``target_date``, sorted oldest→newest."""
    dated: list[tuple[datetime, Path]] = []
    for image in camera_dir.glob("*.jpg"):
        ts = _frame_timestamp(image)
        if ts is None:
            logger.debug("Skipping %s — no parseable timestamp", image.name)
            continue
        if ts.date() == target_date:
            dated.append((ts, image))
    dated.sort(key=lambda pair: pair[0])
    return [image for _ts, image in dated]


# ---------------------------------------------------------------------------
# Annotation
# ---------------------------------------------------------------------------


def _load_font(size: int = 22):
    from PIL import ImageFont

    for name in ("DejaVuSans-Bold.ttf", "DejaVuSans.ttf", "Arial.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def annotate_frame(image_path: Path, dest_path: Path) -> None:
    """Render ``image_path`` to ``dest_path`` with a green box per detection from
    ``{stem}_detections.json`` and a "Birds: N" label in the top-left corner."""
    from PIL import Image, ImageDraw

    detections: list[dict] = []
    count = 0
    det_path = image_path.with_name(f"{image_path.stem}_detections.json")
    if det_path.exists():
        try:
            data = json.loads(det_path.read_text(encoding="utf-8"))
            detections = data.get("detections", []) or []
            count = int(data.get("total_count", len(detections)))
        except (json.JSONDecodeError, ValueError, OSError) as exc:
            logger.warning("Could not read detections for %s: %s", image_path.name, exc)

    with Image.open(image_path) as opened:
        image = opened.convert("RGB")
    draw = ImageDraw.Draw(image)

    for det in detections:
        bbox = det.get("bbox")
        if bbox and len(bbox) == 4:
            draw.rectangle([float(v) for v in bbox], outline=BOX_COLOR, width=3)

    # Count label with a filled background so it reads on any frame.
    label = f"Birds: {count}"
    font = _load_font()
    pad = 5
    try:
        left, top, right, bottom = draw.textbbox((0, 0), label, font=font)
        text_w, text_h = right - left, bottom - top
    except Exception:  # noqa: BLE001 — very old Pillow without textbbox
        text_w, text_h = draw.textsize(label, font=font)
    draw.rectangle([0, 0, text_w + 2 * pad, text_h + 2 * pad], fill=LABEL_BG)
    draw.text((pad, pad), label, fill=LABEL_FG, font=font)

    image.save(dest_path, "JPEG", quality=90)


# ---------------------------------------------------------------------------
# ffmpeg
# ---------------------------------------------------------------------------


def _require_ffmpeg() -> None:
    if shutil.which("ffmpeg") is None:
        raise FfmpegNotFound(
            "ffmpeg is not installed or not on PATH. Install it and retry "
            "(Debian/Ubuntu: `sudo apt install ffmpeg`, macOS: `brew install ffmpeg`)."
        )


def _run_ffmpeg(frames_dir: Path, fps: int, output: Path) -> None:
    """Stitch ``frames_dir/frame_NNNNN.jpg`` into ``output`` at ``fps``."""
    cmd = [
        "ffmpeg",
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-framerate",
        str(fps),
        "-i",
        str(frames_dir / "frame_%05d.jpg"),
        # libx264 + yuv420p needs even dimensions; pad odd frames up by a pixel.
        "-vf",
        "pad=ceil(iw/2)*2:ceil(ih/2)*2",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        str(output),
    ]
    logger.debug("Running: %s", " ".join(cmd))
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(
            f"ffmpeg failed (exit {result.returncode}):\n{result.stderr.strip()[-2000:]}"
        )


# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------


def build_timelapse(
    camera_id: str,
    date_str: str,
    fps: int = DEFAULT_FPS,
    annotate: bool = False,
    processed_dir: Path | str | None = None,
) -> Path:
    """Build the timelapse and return the output path. Raises on no frames,
    missing ffmpeg, or an ffmpeg failure."""
    _require_ffmpeg()
    if annotate:
        try:
            import PIL  # noqa: F401
        except ImportError as exc:
            raise RuntimeError("--annotate needs Pillow (`pip install pillow`)") from exc

    target_date = datetime.strptime(date_str, "%Y-%m-%d").date()
    processed_dir = Path(processed_dir) if processed_dir else config.PROCESSED_DIR
    camera_dir = processed_dir / camera_id
    if not camera_dir.is_dir():
        raise FileNotFoundError(
            f"no processed frames for camera '{camera_id}' (missing {camera_dir})"
        )

    frames = collect_frames(camera_dir, target_date)
    if not frames:
        raise ValueError(f"no frames for camera '{camera_id}' on {date_str}")
    logger.info("Found %d frame(s) for '%s' on %s", len(frames), camera_id, date_str)

    out_dir = processed_dir / "timelapses"
    out_dir.mkdir(parents=True, exist_ok=True)
    output = out_dir / f"{camera_id}_{date_str}.mp4"

    # Stage sequentially-numbered frames in a temp dir so ffmpeg's image2 reader
    # gets a clean run regardless of the source filenames/timestamps.
    with tempfile.TemporaryDirectory(prefix="trailcam-timelapse-") as tmp:
        tmp_dir = Path(tmp)
        for index, image in enumerate(frames, start=1):
            dest = tmp_dir / f"frame_{index:05d}.jpg"
            if annotate:
                annotate_frame(image, dest)
            else:
                shutil.copyfile(image, dest)
        _run_ffmpeg(tmp_dir, fps, output)

    logger.info(
        "Wrote %s (%d frame(s) @ %d fps%s)",
        output,
        len(frames),
        fps,
        ", annotated" if annotate else "",
    )
    return output


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Build an MP4 timelapse from processed trail-cam frames."
    )
    parser.add_argument("--camera", required=True, help="camera_id (subdir under processed/)")
    parser.add_argument("--date", required=True, help="capture date to include (YYYY-MM-DD)")
    parser.add_argument(
        "--fps", type=int, default=DEFAULT_FPS, help=f"frames per second (default: {DEFAULT_FPS})"
    )
    parser.add_argument(
        "--annotate",
        action="store_true",
        help="overlay YOLO bounding boxes + bird count on each frame",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        type=lambda s: s.upper(),
        choices=["DEBUG", "INFO", "WARNING", "ERROR"],
    )
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=getattr(logging, args.log_level, logging.INFO),
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )

    try:
        datetime.strptime(args.date, "%Y-%m-%d")
    except ValueError:
        logger.error("--date must be YYYY-MM-DD, got %r", args.date)
        return 2
    if args.fps < 1:
        logger.error("--fps must be >= 1, got %d", args.fps)
        return 2

    try:
        output = build_timelapse(
            args.camera, args.date, fps=args.fps, annotate=args.annotate
        )
    except FfmpegNotFound as exc:
        logger.error("%s", exc)
        return 1
    except (FileNotFoundError, ValueError, RuntimeError) as exc:
        logger.error("%s", exc)
        return 1

    print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
