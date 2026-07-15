"""Tests for snapshots.py — atomic writes, box drawing, class-name labels.

Uses real OpenCV (installed) to verify pixels actually change where boxes are
drawn, plus a fake cv2 to assert the tmp→replace atomic-write sequence
independently of the filesystem.
"""

import os
from pathlib import Path

import numpy as np
import pytest

import snapshots
from detector import Detection


def _frame(value=100):
    return np.full((48, 64, 3), value, dtype=np.uint8)


def _dets(*specs):
    # specs: (class_name, class_id, confidence, bbox)
    return [Detection(class_name=n, class_id=i, confidence=c, bbox=b) for (n, i, c, b) in specs]


# --- atomic write ----------------------------------------------------------


class _RecordingCv2:
    """Records the atomic-write sequence: imencode, tmp path seen, replace."""

    def __init__(self):
        self.encoded = 0
        self.tmp_seen_during_write = None
        self.orig_replace = os.replace

    def imencode(self, ext, frame):
        self.encoded += 1
        assert ext == ".jpg"
        return True, np.frombuffer(b"\xff\xd8\xff", dtype=np.uint8)


def test_write_atomic_uses_tmp_then_replace(tmp_path, monkeypatch):
    fake = _RecordingCv2()
    dest = tmp_path / "sub" / "latest.jpg"

    # Intercept os.replace to prove the source is the sibling ".latest.jpg.tmp"
    # and that at replace time the tmp (not the final file) holds the bytes.
    seen = {}
    real_replace = os.replace

    def spy_replace(src, dst):
        seen["src"] = src
        seen["dst"] = dst
        seen["tmp_exists"] = Path(src).is_file()
        real_replace(src, dst)

    monkeypatch.setattr(snapshots.os, "replace", spy_replace)
    out = snapshots.write_atomic(_frame(), dest, cv2_module=fake)

    assert out == dest and dest.is_file()
    assert fake.encoded == 1
    assert Path(seen["src"]).name == ".latest.jpg.tmp"
    assert Path(seen["src"]).parent == dest.parent  # same dir → same-fs rename
    assert seen["tmp_exists"] is True               # tmp written before replace
    assert Path(seen["dst"]) == dest
    assert not (dest.parent / ".latest.jpg.tmp").exists()  # tmp gone after replace


def test_write_atomic_no_partial_file_on_encode_failure(tmp_path):
    class _FailCv2:
        def imencode(self, ext, frame):
            return False, None  # encode reports failure

    dest = tmp_path / "latest.jpg"
    with pytest.raises(OSError):
        snapshots.write_atomic(_frame(), dest, cv2_module=_FailCv2())
    # Neither the final file nor the temp is left behind.
    assert not dest.exists()
    assert not (tmp_path / ".latest.jpg.tmp").exists()


def test_write_atomic_real_cv2_roundtrips(tmp_path):
    import cv2

    dest = tmp_path / "latest.jpg"
    snapshots.write_atomic(_frame(123), dest)
    assert dest.is_file()
    back = cv2.imread(str(dest))
    assert back is not None and back.shape == (48, 64, 3)


# --- box drawing (real cv2) ------------------------------------------------


def test_draw_detections_marks_the_frame():
    frame = _frame(100)
    annotated = snapshots.draw_detections(frame, _dets(("egg", 0, 0.91, [10, 10, 30, 30])))
    # Original untouched; a copy is returned.
    assert np.all(frame == 100)
    assert annotated.shape == frame.shape
    # Something (box + label) was drawn — the copy differs from the flat frame.
    assert np.any(annotated != 100)
    # Green box pixels are present along the rectangle edge.
    assert np.any((annotated[..., 1] > 150) & (annotated[..., 0] < 80) & (annotated[..., 2] < 80))


def test_draw_detections_label_uses_class_name():
    # The overlay caption reflects the model's actual class, not a hardcoded word.
    assert snapshots.detection_label(_dets(("egg", 0, 0.91, [0, 0, 5, 5]))[0]) == "egg 91%"
    assert snapshots.detection_label(_dets(("chick", 0, 0.80, [0, 0, 5, 5]))[0]) == "chick 80%"


def test_draw_detections_skips_malformed_bbox():
    # A bad bbox is skipped, not crashed on; frame stays clean.
    annotated = snapshots.draw_detections(_frame(100), _dets(("egg", 0, 0.9, [1, 2, 3])))
    assert np.all(annotated == 100)


# --- write_snapshots: raw + annotated always written -----------------------


def test_write_snapshots_writes_both_with_detections(tmp_path):
    import cv2

    raw = tmp_path / "latest.jpg"
    annotated = tmp_path / "latest_annotated.jpg"
    frame = _frame(100)
    snapshots.write_snapshots(frame, _dets(("chick", 0, 0.8, [5, 5, 40, 40])), raw, annotated)

    assert raw.is_file() and annotated.is_file()
    raw_img = cv2.imread(str(raw))
    ann_img = cv2.imread(str(annotated))
    # Raw is the untouched flat frame; annotated has boxes so it differs.
    assert np.all(raw_img == 100)
    assert np.any(ann_img != 100)


def test_write_snapshots_writes_annotated_even_with_no_detections(tmp_path):
    import cv2

    raw = tmp_path / "latest.jpg"
    annotated = tmp_path / "latest_annotated.jpg"
    snapshots.write_snapshots(_frame(100), [], raw, annotated)

    # Both files exist; with no detections the annotated copy equals the raw.
    assert raw.is_file() and annotated.is_file()
    assert np.array_equal(cv2.imread(str(raw)), cv2.imread(str(annotated)))


def test_write_snapshots_refreshes_existing_files(tmp_path):
    import cv2

    raw = tmp_path / "latest.jpg"
    annotated = tmp_path / "latest_annotated.jpg"
    # First cycle: 3 chicks.
    snapshots.write_snapshots(_frame(50), _dets(("chick", 0, 0.8, [5, 5, 20, 20])), raw, annotated)
    first = raw.read_bytes()
    # Next cycle: a different frame overwrites the same paths atomically.
    snapshots.write_snapshots(_frame(200), [], raw, annotated)
    assert raw.read_bytes() != first
    # The average pixel reflects the new brighter frame.
    assert cv2.imread(str(raw)).mean() > 150
