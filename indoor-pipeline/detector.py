"""YOLO inference wrapper with in-process model hot-swap.

The indoor pipeline runs *one* of two YOLO models depending on the live camera
assignment (see :mod:`assignment`): the incubation-stage model or the chick
model. Rather than restart the service to change models, :class:`Detector` loads
a model on demand and can swap to a different set of weights in-process —
unloading the old model first — so the service loop keeps running across a
reassignment.

Inference itself mirrors the trail-cam detector: ``ultralytics.YOLO(weights)``
then ``model.predict(frame, conf=…)``, flattening the results into a list of
:class:`Detection` (class name + id, confidence, xyxy bbox in pixels). The
``ultralytics`` import is lazy (paid only on the first real load) and the model
factory is injectable, so this module imports cheaply and unit-tests without
torch/ultralytics.

Model-not-found is handled gracefully: :meth:`Detector.load` returns ``False``
(logging the error) instead of raising, leaving the detector unloaded so the
service loop skips inference and retries on the next assignment poll.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path

logger = logging.getLogger("indoorpipeline.detector")


@dataclass
class Detection:
    """A single detected object."""

    class_name: str
    class_id: int
    confidence: float
    bbox: list[float]  # [x1, y1, x2, y2] in pixel coordinates (xyxy)


def _default_yolo_factory(weights_path: str):
    """Load a real Ultralytics YOLO model, importing the heavy dep lazily.

    Existence is checked first so a missing weights file surfaces as a clean
    ``FileNotFoundError`` (handled as "model not found") instead of ultralytics
    trying to *download* a named model like ``yolov8n.pt``.
    """
    path = Path(weights_path)
    if not path.exists():
        raise FileNotFoundError(f"model weights not found: {path}")
    from ultralytics import YOLO  # lazy: only a real load needs it

    return YOLO(str(path))


class Detector:
    """Loads a YOLO model and runs inference, with in-process hot-swap.

    ``yolo_factory`` (defaulting to :func:`_default_yolo_factory`) is a callable
    ``weights_path -> model`` — inject a fake in tests. A loaded ``model`` must
    expose ``predict(frame, conf=…, verbose=…)`` returning Ultralytics-style
    results (each with ``.boxes`` and ``.names``).
    """

    def __init__(self, *, yolo_factory=None):
        self._factory = yolo_factory or _default_yolo_factory
        self.model = None
        self.weights: str | None = None
        self.confidence: float = 0.5

    @property
    def loaded(self) -> bool:
        return self.model is not None

    def load(self, weights, confidence: float) -> bool:
        """Ensure ``weights`` is the loaded model at ``confidence``.

        A no-op (returns ``True``) if those weights are already loaded — only the
        confidence is refreshed. Otherwise the current model is unloaded and the
        new one loaded. Returns ``False`` (logging the error, leaving the
        detector unloaded) if the weights can't be loaded — e.g. the file is
        missing — so the caller can skip inference and retry later.
        """
        weights_str = str(weights)
        if self.model is not None and self.weights == weights_str:
            self.confidence = confidence
            return True

        # Swapping: drop the old model before loading the new one.
        self.unload()
        try:
            model = self._factory(weights_str)
        except Exception as exc:  # noqa: BLE001 — model-not-found must not crash the loop
            logger.error("Could not load YOLO model from %s: %s", weights_str, exc)
            return False
        self.model = model
        self.weights = weights_str
        self.confidence = confidence
        logger.info("Loaded YOLO model from %s (conf=%.2f)", weights_str, confidence)
        return True

    def unload(self) -> None:
        """Drop the currently loaded model (idempotent)."""
        if self.model is not None:
            logger.debug("Unloading YOLO model %s", self.weights)
        self.model = None
        self.weights = None

    def class_names(self) -> dict[int, str]:
        """Return the loaded model's ``{class_id: name}`` map (empty if unloaded).

        Used to build the Roboflow annotation labelmap so numeric class indices
        in the YOLO ``.txt`` resolve back to named classes.
        """
        if self.model is None:
            return {}
        names = getattr(self.model, "names", {}) or {}
        return {int(k): str(v) for k, v in dict(names).items()}

    def detect(self, frame) -> list[Detection]:
        """Run inference on one BGR numpy ``frame`` and return its detections.

        Returns an empty list when no model is loaded (the loop skips inference
        until a model is available).
        """
        if self.model is None:
            return []
        results = self.model.predict(frame, conf=self.confidence, verbose=False)
        detections: list[Detection] = []
        # Ultralytics returns one Results object per source image; we passed one.
        for result in results or []:
            boxes = getattr(result, "boxes", None)
            if boxes is None:
                continue
            names = getattr(result, "names", {}) or {}
            for box in boxes:
                class_id = int(box.cls[0])
                detections.append(
                    Detection(
                        class_name=str(names.get(class_id, str(class_id))),
                        class_id=class_id,
                        confidence=round(float(box.conf[0]), 4),
                        bbox=[round(float(v), 2) for v in box.xyxy[0].tolist()],
                    )
                )
        return detections
