"""Reuse the trail-cam YOLO detector (incl. CLAHE IR/night preprocessing).

Rather than duplicate inference logic, the indoor pipeline reuses
``trailcam/yolo_detector.py`` — the same model loading, CLAHE enhancement for
IR/night frames, and ``DetectionResult`` shape the trail cam already uses.

The wrinkle: both pipelines have a top-level ``config.py``. ``yolo_detector``
does ``from . import config`` (package) falling back to ``import config``
(script). If we just put the trail-cam dir on ``sys.path`` and ran
``import yolo_detector``, that bare ``import config`` could bind to *this*
package's config and explode. So instead we add the trail-cam dir's **parent**
to ``sys.path`` and import it as a submodule of the ``trailcam`` namespace
package (``trailcam.yolo_detector``); its ``from . import config`` then resolves
to ``trailcam.config`` and our own ``config`` is left untouched.

The heavy import (ultralytics) is still lazy inside the trail-cam detector, so
importing this module stays cheap and test-friendly.
"""

from __future__ import annotations

import importlib
import logging
import sys
from pathlib import Path

try:
    from . import config
except ImportError:
    import config

logger = logging.getLogger("indoorcam.detector")

_module_cache: dict[str, object] = {}  # cached trail-cam module handles by name


def import_trailcam_module(module: str):
    """Import ``{TRAILCAM_DIR.name}.<module>`` as a namespace-package submodule.

    Adds the trail-cam dir's *parent* to ``sys.path`` and imports the module
    under the ``trailcam`` package so its internal ``from . import config``
    resolves to the trail-cam config — never this package's ``config``. Cached
    per module name. Used for both ``yolo_detector`` and ``roboflow_uploader``.
    """
    cached = _module_cache.get(module)
    if cached is not None:
        return cached
    trailcam_dir = Path(config.TRAILCAM_DIR)
    parent = str(trailcam_dir.parent)
    if parent not in sys.path:
        sys.path.insert(0, parent)
    module_name = f"{trailcam_dir.name}.{module}"
    try:
        loaded = importlib.import_module(module_name)
    except Exception as exc:  # noqa: BLE001 — surface a clear, actionable error
        raise RuntimeError(
            f"could not import the trail-cam module ({module_name}) from "
            f"{trailcam_dir} — set TRAILCAM_DIR to the trailcam/ checkout: {exc}"
        ) from exc
    _module_cache[module] = loaded
    logger.debug("Loaded %s from %s", module_name, trailcam_dir)
    return loaded


def _load_trailcam_detector():
    """The reused trail-cam ``yolo_detector`` module."""
    return import_trailcam_module("yolo_detector")


def detect(image_path: Path | str, camera_id: str | None = None):
    """Run YOLO on ``image_path`` and return a trail-cam ``DetectionResult``.

    Uses this package's configured model + confidence, and passes ``camera_id``
    explicitly so detection doesn't depend on a sidecar (the indoor poller writes
    none). IR/night frames are CLAHE-enhanced for inference by the reused
    detector; the on-disk original is untouched.
    """
    yolo = _load_trailcam_detector()
    return yolo.detect(
        image_path,
        camera_id=camera_id if camera_id is not None else config.CAMERA_ID,
        model_path=config.YOLO_MODEL_PATH,
        confidence=config.YOLO_CONFIDENCE,
    )


def annotate_image(image_path: Path | str, result, dest_path: Path | str) -> bool:
    """Draw ``result``'s detections onto a copy of the image (best-effort).

    Thin pass-through to the trail-cam detector's annotator so the indoor
    pipeline produces the same green-box annotated JPEGs.
    """
    yolo = _load_trailcam_detector()
    return yolo.annotate_image(image_path, result, dest_path)
