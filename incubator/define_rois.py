"""Runtime utility to capture a reference frame and propose slot ROIs.

Run this manually on the Pi (which can reach the camera) to bootstrap or tweak
the tray layout in ``config.json``:

    # Grab a reference frame and re-draw the CURRENT config.json slots over it:
    python define_rois.py

    # Propose a fresh 3x4 grid, print the tray.slots JSON to stdout:
    python define_rois.py --grid 3x4

    # …with custom margins/gaps (fractions of the image), then WRITE it into
    # config.json (otherwise config.json is never touched):
    python define_rois.py --grid 3x4 --margin-x 0.08 --margin-y 0.06 --write

Outputs:
* ``incubator/reference.jpg``          — the captured frame (unless --no-capture)
* ``incubator/reference_annotated.jpg``— the frame with slot boxes + ids drawn on
  it, for eyeballing alignment.

The proposed ``tray.slots`` JSON is printed to stdout so you can copy it by hand;
nothing overwrites ``config.json`` unless ``--write`` is passed.
"""

from __future__ import annotations

import argparse
import json
import logging
import re
import sys
from pathlib import Path

try:
    from . import config as config_module
    from . import camera as camera_module
    from . import roi as roi_module
except ImportError:
    import config as config_module
    import camera as camera_module
    import roi as roi_module

logger = logging.getLogger("incubator.define_rois")

_GRID_RE = re.compile(r"^\s*(\d+)\s*[xX]\s*(\d+)\s*$")

# Repo-root-relative default outputs (config's reference_image is
# "incubator/reference.jpg"); resolve alongside this file's package dir.
_PKG_DIR = Path(__file__).resolve().parent


def _parse_grid(spec: str) -> tuple[int, int]:
    m = _GRID_RE.match(spec)
    if not m:
        raise argparse.ArgumentTypeError(f"--grid must look like ROWSxCOLS (e.g. 3x4), got {spec!r}")
    rows, cols = int(m.group(1)), int(m.group(2))
    if rows < 1 or cols < 1:
        raise argparse.ArgumentTypeError("--grid rows and cols must both be >= 1")
    return rows, cols


def _resolve_output(path_str: str) -> Path:
    """Resolve a config path like ``incubator/reference.jpg`` to an absolute path.

    The config stores it repo-root-relative; map the ``incubator/`` prefix onto
    this package directory so it works regardless of the current directory.
    """
    p = Path(path_str)
    if p.is_absolute():
        return p
    parts = p.parts
    if parts and parts[0] == _PKG_DIR.name:
        return _PKG_DIR.joinpath(*parts[1:])
    return _PKG_DIR / p


def capture_reference(conf, dest: Path, *, cv2_module=None) -> bool:
    """Grab one frame from the configured camera and save it to ``dest``.

    Returns True on success. Requires the camera source env var to be set.
    """
    source = camera_module.create_frame_source(conf, cv2_module=cv2_module)
    try:
        frame = source.grab()
    finally:
        source.close()
    if frame is None:
        logger.error("Failed to grab a reference frame from the camera")
        return False
    cv2 = cv2_module
    if cv2 is None:
        import cv2
    dest.parent.mkdir(parents=True, exist_ok=True)
    if not cv2.imwrite(str(dest), frame):
        logger.error("Failed to write reference image to %s", dest)
        return False
    logger.info("Saved reference frame -> %s", dest)
    return True


def annotate(reference_path: Path, slots, dest: Path, *, cv2_module=None) -> bool:
    """Draw each slot's bbox + id onto a copy of ``reference_path`` -> ``dest``."""
    cv2 = cv2_module
    if cv2 is None:
        import cv2
    img = cv2.imread(str(reference_path))
    if img is None:
        logger.error("Could not read reference image %s to annotate", reference_path)
        return False
    for slot in slots:
        sid = slot["id"] if isinstance(slot, dict) else slot.id
        bbox = slot["bbox"] if isinstance(slot, dict) else slot.bbox
        x, y, w, h = (int(v) for v in bbox)
        cv2.rectangle(img, (x, y), (x + w, y + h), (0, 255, 0), 2)
        cv2.putText(img, str(sid), (x + 3, y + 18), cv2.FONT_HERSHEY_SIMPLEX, 0.6, (0, 255, 0), 2)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if not cv2.imwrite(str(dest), img):
        logger.error("Failed to write annotated image to %s", dest)
        return False
    logger.info("Saved annotated reference -> %s", dest)
    return True


def _slots_as_config(slots) -> list[dict]:
    """Normalize slots (dicts or Slot objects) to the config.json shape."""
    out = []
    for slot in slots:
        if isinstance(slot, dict):
            out.append({"id": slot["id"], "bbox": list(slot["bbox"]), "clutch_id": slot.get("clutch_id")})
        else:
            out.append({"id": slot.id, "bbox": list(slot.bbox), "clutch_id": slot.clutch_id})
    return out


def _write_slots_into_config(config_path: Path, slots) -> None:
    """Replace ``tray.slots`` in ``config_path`` in place, preserving the rest."""
    data = json.loads(config_path.read_text(encoding="utf-8"))
    data.setdefault("tray", {})["slots"] = _slots_as_config(slots)
    config_path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    logger.info("Wrote %d slot(s) into %s", len(slots), config_path)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Capture a reference frame + propose incubator slot ROIs")
    parser.add_argument("--config", default=None, help="path to config.json (default: incubator/config.json)")
    parser.add_argument("--grid", type=_parse_grid, default=None, metavar="ROWSxCOLS",
                        help="auto-propose an evenly-spaced grid of this many rows x cols")
    parser.add_argument("--margin-x", type=float, default=0.05, help="outer left/right margin, fraction of width (default 0.05)")
    parser.add_argument("--margin-y", type=float, default=0.05, help="outer top/bottom margin, fraction of height (default 0.05)")
    parser.add_argument("--gap-x", type=float, default=0.01, help="horizontal gap between cells, fraction of width (default 0.01)")
    parser.add_argument("--gap-y", type=float, default=0.01, help="vertical gap between cells, fraction of height (default 0.01)")
    parser.add_argument("--no-capture", action="store_true", help="don't grab a new frame; use the existing reference.jpg")
    parser.add_argument("--write", action="store_true", help="write the proposed slots into config.json (default: print only)")
    parser.add_argument("--log-level", default="INFO", type=lambda s: s.upper(),
                        choices=["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"])
    args = parser.parse_args(argv)

    logging.basicConfig(level=getattr(logging, args.log_level, logging.INFO),
                        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s")

    try:
        conf = config_module.load_config(args.config)
    except config_module.ConfigError as exc:
        logger.error("Bad configuration: %s", exc)
        return 1

    reference_path = _resolve_output(conf.tray.reference_image)
    annotated_path = reference_path.with_name(reference_path.stem + "_annotated" + reference_path.suffix)

    # 1. Grab a reference frame (unless reusing the existing one).
    if not args.no_capture:
        try:
            if not capture_reference(conf, reference_path):
                return 1
        except camera_module.CaptureError as exc:
            logger.error("%s", exc)
            return 1
    elif not reference_path.exists():
        logger.error("--no-capture set but no reference image at %s", reference_path)
        return 1

    # 2. Determine the slots to draw/propose.
    if args.grid is not None:
        import cv2  # need the reference dimensions
        img = cv2.imread(str(reference_path))
        if img is None:
            logger.error("Could not read reference image %s for grid sizing", reference_path)
            return 1
        h, w = img.shape[:2]
        rows, cols = args.grid
        slots = roi_module.generate_grid(
            w, h, rows, cols,
            margin_x=args.margin_x, margin_y=args.margin_y,
            gap_x=args.gap_x, gap_y=args.gap_y,
        )
        logger.info("Proposed a %dx%d grid over %dx%d reference (%d slots)", rows, cols, w, h, len(slots))
    else:
        slots = conf.tray.slots  # re-draw the current config
        logger.info("Using %d existing slot(s) from config", len(slots))

    # 3. Annotate for eyeballing.
    annotate(reference_path, slots, annotated_path)

    # 4. Emit the proposed tray.slots JSON (stdout), and optionally write it.
    proposed = _slots_as_config(slots)
    print(json.dumps({"slots": proposed}, indent=2))

    if args.write:
        if args.grid is None:
            logger.info("--write with no --grid: config.json already holds these slots; nothing to change")
        else:
            _write_slots_into_config(Path(conf.source_path), slots)
    else:
        logger.info("Dry run — config.json not modified (pass --write to persist the proposed slots)")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
