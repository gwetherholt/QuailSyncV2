"""Stage-1 incubator capture pipeline for QuailSync.

Data-capture only: grab frames from a fixed camera over the incubator tray, apply
per-slot ROIs, run a per-slot frame-difference detector, and log change events
(plus saved crops) to SQLite. No YOLO / classifier / Roboflow / backend — those
are later stages. See ``README.md`` for the architecture and the stage boundary.

The modules deliberately avoid importing ``cv2`` / ``numpy`` at import time where
practical (the heavy imports are lazy inside the functions that need a real
camera), so the pure logic — ROI math, the diff scorer, the detector state
machine, storage — stays cheap to import and easy to unit-test.
"""

__all__ = [
    "config",
    "camera",
    "roi",
    "diff",
    "detector",
    "storage",
]
