"""Tests for detector.py — inference parsing, model-not-found, and hot-swap.

A fake YOLO factory stands in for ``ultralytics.YOLO``; no torch, no real
weights. The fake models expose the ``.predict(...) -> [Results]`` shape the
detector flattens (each Results has ``.boxes`` and ``.names``).
"""

import numpy as np

from detector import Detection, Detector


# --- fake ultralytics shapes -----------------------------------------------


class _FakeBox:
    def __init__(self, cls, conf, xyxy):
        self.cls = [cls]
        self.conf = [conf]
        self.xyxy = [np.array(xyxy, dtype=float)]


class _FakeResults:
    def __init__(self, boxes, names):
        self.boxes = boxes
        self.names = names


class _FakeModel:
    """Returns a fixed set of detections; records the predict kwargs it saw."""

    def __init__(self, names, boxes, tag="model"):
        self.names = names
        self._boxes = boxes
        self.tag = tag
        self.predict_calls = []

    def predict(self, frame, conf=0.25, verbose=True):
        self.predict_calls.append({"conf": conf, "verbose": verbose})
        return [_FakeResults(self._boxes, self.names)]


def _frame():
    return np.full((48, 64, 3), 100, dtype=np.uint8)


def _egg_model():
    names = {0: "egg", 1: "pipped"}
    boxes = [
        _FakeBox(0, 0.91, [10, 10, 20, 20]),
        _FakeBox(1, 0.55, [30, 30, 44, 46]),
    ]
    return _FakeModel(names, boxes, tag="egg")


def _chick_model():
    names = {0: "chick"}
    boxes = [_FakeBox(0, 0.80, [5, 5, 15, 15])]
    return _FakeModel(names, boxes, tag="chick")


# --- load + inference ------------------------------------------------------


def test_load_then_detect_returns_parsed_detections():
    model = _egg_model()
    det = Detector(yolo_factory=lambda w: model)
    assert det.load("/models/incubation-best.pt", 0.5) is True
    assert det.loaded is True

    detections = det.detect(_frame())
    assert len(detections) == 2
    assert all(isinstance(d, Detection) for d in detections)
    first = detections[0]
    assert first.class_name == "egg"
    assert first.class_id == 0
    assert first.confidence == 0.91
    assert first.bbox == [10.0, 10.0, 20.0, 20.0]
    # The configured confidence is passed through to predict().
    assert model.predict_calls[0]["conf"] == 0.5
    assert model.predict_calls[0]["verbose"] is False


def test_detect_without_loaded_model_returns_empty():
    det = Detector(yolo_factory=lambda w: _egg_model())
    assert det.loaded is False
    assert det.detect(_frame()) == []


def test_class_names_exposes_model_labelmap():
    det = Detector(yolo_factory=lambda w: _egg_model())
    det.load("/w.pt", 0.5)
    assert det.class_names() == {0: "egg", 1: "pipped"}


# --- model-not-found -------------------------------------------------------


def test_model_not_found_returns_false_and_stays_unloaded():
    def factory(weights):
        raise FileNotFoundError(f"missing: {weights}")

    det = Detector(yolo_factory=factory)
    assert det.load("/models/nope.pt", 0.5) is False
    assert det.loaded is False
    # Inference is safely skipped when the model failed to load.
    assert det.detect(_frame()) == []


def test_reload_after_missing_model_recovers():
    calls = {"n": 0}

    def factory(weights):
        calls["n"] += 1
        if calls["n"] == 1:
            raise FileNotFoundError("first attempt missing")
        return _chick_model()

    det = Detector(yolo_factory=factory)
    assert det.load("/models/chick-best.pt", 0.5) is False
    # Retry (as the service loop does each cycle) now succeeds.
    assert det.load("/models/chick-best.pt", 0.5) is True
    assert det.loaded is True


# --- hot-swap --------------------------------------------------------------


def test_hot_swap_loads_new_weights():
    loaded = []

    def factory(weights):
        loaded.append(weights)
        return _egg_model() if "incubation" in weights else _chick_model()

    det = Detector(yolo_factory=factory)
    det.load("/models/incubation-best.pt", 0.5)
    assert det.detect(_frame())[0].class_name == "egg"

    # Swap to the chick model — new weights loaded, old dropped.
    det.load("/models/chick-best.pt", 0.5)
    assert det.detect(_frame())[0].class_name == "chick"
    assert loaded == ["/models/incubation-best.pt", "/models/chick-best.pt"]


def test_reloading_same_weights_is_a_noop():
    n = {"count": 0}

    def factory(weights):
        n["count"] += 1
        return _egg_model()

    det = Detector(yolo_factory=factory)
    det.load("/models/incubation-best.pt", 0.5)
    det.load("/models/incubation-best.pt", 0.6)  # same weights, new conf
    assert n["count"] == 1  # not reloaded
    assert det.confidence == 0.6  # confidence still refreshed


def test_unload_drops_the_model():
    det = Detector(yolo_factory=lambda w: _egg_model())
    det.load("/w.pt", 0.5)
    det.unload()
    assert det.loaded is False
    assert det.class_names() == {}
