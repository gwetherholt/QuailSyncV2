"""Tests for roboflow_uploader.py — VOC XML (incubation) vs YOLO+labelmap (chick).

The HTTP POST is a fake recorder; no network. Verifies the incubation path
uploads Pascal VOC XML with the class **name inline** (the fix for boxes landing
under class "0"), and that the chick path is left on the legacy YOLO+labelmap
upload unchanged.
"""

import numpy as np

from detector import Detection
from roboflow_uploader import (
    INCUBATION_CLASS_NAMES,
    RoboflowUploader,
    voc_xml_annotation,
)


class _FakeResp:
    def __init__(self, status_code=200, payload=None, text="ok"):
        self.status_code = status_code
        self._payload = payload if payload is not None else {"id": "img123"}
        self.text = text

    def json(self):
        return self._payload


class _FakePost:
    """Records every POST; returns an image id for the upload call."""

    def __init__(self):
        self.calls = []

    def __call__(self, url, **kwargs):
        self.calls.append({"url": url, **kwargs})
        return _FakeResp()


def _frame(h=48, w=64):
    return np.full((h, w, 3), 100, dtype=np.uint8)


def _det(class_name="egg", class_id=0, bbox=(10, 10, 30, 40)):
    return Detection(class_name=class_name, class_id=class_id, confidence=0.9, bbox=list(bbox))


def _incubation_uploader(post):
    return RoboflowUploader(
        "KEY", "quail", "incubation-stages", post=post, class_names=INCUBATION_CLASS_NAMES
    )


def _chick_uploader(post):
    return RoboflowUploader("KEY", "quail", "find-chicks-5", post=post, class_names=None)


# --- VOC builder ------------------------------------------------------------


def test_voc_xml_has_inline_class_name_and_bbox():
    xml = voc_xml_annotation([_det()], "f.jpg", 64, 48, INCUBATION_CLASS_NAMES)
    assert "<annotation>" in xml
    assert "<name>egg</name>" in xml               # class name inline, not "0"
    assert "<filename>f.jpg</filename>" in xml
    assert "<bndbox>" in xml and "<xmin>10</xmin>" in xml and "<ymax>40</ymax>" in xml


def test_voc_uses_canonical_mapping_not_model_name():
    # Even if the model's own class_name differs, the canonical map is the source
    # of truth — index 0 -> "egg".
    xml = voc_xml_annotation([_det(class_name="weird", class_id=0)], "f.jpg", 64, 48, INCUBATION_CLASS_NAMES)
    assert "<name>egg</name>" in xml
    assert "weird" not in xml


def test_voc_empty_when_no_valid_boxes():
    assert voc_xml_annotation([], "f.jpg", 64, 48, INCUBATION_CLASS_NAMES) == ""
    # Degenerate bbox is skipped -> no objects -> empty.
    assert voc_xml_annotation([_det(bbox=(5, 5, 5, 5))], "f.jpg", 64, 48, INCUBATION_CLASS_NAMES) == ""


# --- incubation upload path (VOC XML) ---------------------------------------


def test_incubation_upload_posts_voc_xml_with_name_egg():
    post = _FakePost()
    up = _incubation_uploader(post)
    assert up.upload_frame(_frame(), "img.jpg", [_det()]) is True

    # Two calls: image upload, then annotate.
    assert len(post.calls) == 2
    annotate = post.calls[1]
    assert "/annotate/" in annotate["url"]
    assert annotate["params"]["name"].endswith(".xml")
    # VOC is self-describing: NO labelmap param (that was the broken bit).
    assert "labelmap" not in annotate["params"]
    body = annotate["data"].decode("utf-8")
    assert "<name>egg</name>" in body
    assert "xml" in annotate["headers"]["Content-Type"]


def test_incubation_no_annotation_when_no_detections():
    post = _FakePost()
    _incubation_uploader(post).upload_frame(_frame(), "img.jpg", [])
    assert len(post.calls) == 1  # image only, no annotate call


# --- chick upload path (legacy YOLO+labelmap, unchanged) --------------------


def test_chick_upload_keeps_yolo_txt_and_labelmap():
    post = _FakePost()
    _chick_uploader(post).upload_frame(_frame(), "img.jpg", [_det(class_name="chick", class_id=0)])

    assert len(post.calls) == 2
    annotate = post.calls[1]
    assert annotate["params"]["name"].endswith(".txt")     # still YOLO txt
    assert "labelmap" in annotate["params"]                 # legacy behavior intact
    body = annotate["data"].decode("utf-8")
    assert body.startswith("0 ")                            # YOLO index line
    assert "<name>" not in body                             # not VOC
