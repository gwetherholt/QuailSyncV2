"""Tests for roboflow_uploader.py — image-only vs YOLO pre-annotation upload.

The HTTP POST is a fake recorder; no network. The uploader uploads the image
alone when given no detections (how incubation stays annotation-free), and adds a
YOLO annotation (+ labelmap) when detections are supplied (the chick path).
"""

import numpy as np

from detector import Detection
from roboflow_uploader import RoboflowUploader


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


def _det(class_name="chick", class_id=0, bbox=(10, 10, 30, 40)):
    return Detection(class_name=class_name, class_id=class_id, confidence=0.9, bbox=list(bbox))


def _uploader(post, project="find-chicks-5"):
    return RoboflowUploader("KEY", "quail", project, post=post)


# --- image-only (no detections) --------------------------------------------


def test_upload_without_detections_is_image_only():
    post = _FakePost()
    assert _uploader(post).upload_frame(_frame(), "img.jpg", []) is True
    assert len(post.calls) == 1                       # only the image upload
    assert post.calls[0]["url"].endswith("/upload")
    assert all("/annotate/" not in c["url"] for c in post.calls)


def test_upload_none_detections_is_image_only():
    post = _FakePost()
    _uploader(post).upload_frame(_frame(), "img.jpg")  # detections defaults to None
    assert len(post.calls) == 1
    assert "/annotate/" not in post.calls[0]["url"]


# --- with detections (YOLO pre-annotation, the chick path) ------------------


def test_upload_with_detections_posts_yolo_annotation():
    post = _FakePost()
    _uploader(post).upload_frame(_frame(), "img.jpg", [_det("chick", 0)])

    assert len(post.calls) == 2                        # image upload + annotate
    annotate = post.calls[1]
    assert "/annotate/" in annotate["url"]
    assert annotate["params"]["name"].endswith(".txt")
    assert "labelmap" in annotate["params"]
    body = annotate["data"].decode("utf-8")
    assert body.startswith("0 ")                        # YOLO index line
