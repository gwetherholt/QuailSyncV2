"""Tests for roi.py — crop correctness and grid generation math."""

import numpy as np
import pytest

import roi


def _gradient_frame(height=100, width=120):
    # A frame whose pixel value encodes its position so crops are identifiable:
    # value = y*1000 + x (mod 256 per channel doesn't matter; use a 2-D array).
    ys, xs = np.mgrid[0:height, 0:width]
    return (ys * 1000 + xs).astype(np.int32)


def test_crop_returns_exact_region():
    frame = _gradient_frame()
    out = roi.crop(frame, (10, 20, 30, 40))  # x=10, y=20, w=30, h=40
    assert out.shape == (40, 30)
    # Top-left pixel of the crop is frame[y=20, x=10].
    assert out[0, 0] == 20 * 1000 + 10
    # Bottom-right pixel is frame[y=59, x=39].
    assert out[-1, -1] == 59 * 1000 + 39


def test_crop_on_3channel_frame_keeps_channels():
    frame = np.zeros((100, 100, 3), dtype=np.uint8)
    out = roi.crop(frame, (5, 5, 20, 10))
    assert out.shape == (10, 20, 3)


def test_crop_clamps_to_frame_bounds():
    frame = _gradient_frame(height=50, width=50)
    # bbox runs off the right/bottom edge; result is clamped, not wrapped.
    out = roi.crop(frame, (40, 40, 30, 30))
    assert out.shape == (10, 10)  # only 50-40 = 10 px available each way
    assert out[0, 0] == 40 * 1000 + 40


def test_crop_rejects_nonpositive_size():
    frame = _gradient_frame()
    with pytest.raises(ValueError):
        roi.crop(frame, (0, 0, 0, 10))


def test_slot_label_spreadsheet_scheme():
    assert roi.slot_label(0, 0) == "A1"
    assert roi.slot_label(0, 1) == "A2"
    assert roi.slot_label(1, 0) == "B1"
    assert roi.slot_label(2, 3) == "C4"
    assert roi.slot_label(25, 0) == "Z1"
    assert roi.slot_label(26, 0) == "AA1"


def test_generate_grid_count_and_ids():
    slots = roi.generate_grid(1000, 800, rows=2, cols=3)
    assert len(slots) == 6
    assert [s["id"] for s in slots] == ["A1", "A2", "A3", "B1", "B2", "B3"]
    for s in slots:
        assert s["clutch_id"] is None
        assert len(s["bbox"]) == 4


def test_generate_grid_cells_are_within_bounds():
    W, H = 1000, 800
    slots = roi.generate_grid(W, H, rows=3, cols=4, margin_x=0.05, margin_y=0.05)
    for s in slots:
        x, y, w, h = s["bbox"]
        assert x >= 0 and y >= 0
        assert x + w <= W
        assert y + h <= H
        assert w > 0 and h > 0


def test_generate_grid_cells_do_not_overlap():
    slots = roi.generate_grid(1000, 800, rows=2, cols=2, gap_x=0.02, gap_y=0.02)
    by_id = {s["id"]: s["bbox"] for s in slots}
    a1x, a1y, a1w, a1h = by_id["A1"]
    a2x = by_id["A2"][0]
    b1y = by_id["B1"][1]
    # A2 starts to the right of A1's right edge (there's a gap).
    assert a2x >= a1x + a1w
    # B1 starts below A1's bottom edge.
    assert b1y >= a1y + a1h


def test_generate_grid_is_evenly_spaced():
    slots = roi.generate_grid(1000, 100, rows=1, cols=4, margin_x=0.0, gap_x=0.0, margin_y=0.0)
    xs = [s["bbox"][0] for s in slots]
    widths = [s["bbox"][2] for s in slots]
    # With no margin/gap, 4 cells of width ~250 tile the 1000px width.
    assert all(abs(w - 250) <= 1 for w in widths)
    assert xs == sorted(xs)


def test_generate_grid_rejects_bad_dims():
    with pytest.raises(ValueError):
        roi.generate_grid(1000, 800, rows=0, cols=3)
    with pytest.raises(ValueError):
        roi.generate_grid(1000, 800, rows=2, cols=2, margin_x=0.6)  # margins eat everything
