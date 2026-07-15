"""Assignment-aware indoor pipeline (stage 3) for QuailSync.

One service that supersedes both ``incubator/`` (frame-diff event logging) and
``indoor-cam/`` (YOLO chick detection). It reads the indoor camera's assignment
from the backend and runs the matching YOLO model — the incubation-stage model
when the camera is assigned to the incubator, the chick model when assigned to a
brooder — hot-swapping in-process when the assignment changes.

Per cycle: poll the assignment, ensure the right model is loaded, grab a frame,
run YOLO, log incubation events (incubator mode only), and upload frames + YOLO
pre-annotations to the mode's Roboflow project.

The modules avoid importing ``cv2`` / ``ultralytics`` at import time (the heavy
imports are lazy inside the functions that need them), so the pure logic stays
cheap to import and easy to unit-test.
"""

__all__ = [
    "config",
    "camera",
    "detector",
    "assignment",
    "roboflow_uploader",
    "storage",
    "snapshots",
    "main",
]
