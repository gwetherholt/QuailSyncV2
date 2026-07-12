"""Tests for diff.py — mean-abs-diff scoring and running-average baseline."""

import numpy as np

import diff


def _gray(value, size=40):
    return np.full((size, size), value, dtype=np.uint8)


def _bgr(value, size=40):
    return np.full((size, size, 3), value, dtype=np.uint8)


def test_identical_frames_score_near_zero():
    tracker = diff.BaselineTracker(alpha=0.02, blur_kernel=5)
    base = _gray(100)
    assert tracker.score(base) == 0.0  # first frame seeds baseline
    # Same frame again -> essentially no difference.
    assert tracker.score(base.copy()) < 1e-3


def test_injected_patch_scores_above_high_threshold():
    high_threshold = 18.0
    tracker = diff.BaselineTracker(alpha=0.02, blur_kernel=5)
    tracker.score(_gray(100))  # seed baseline at value 100

    changed = _gray(100)
    # Paint a bright patch over a quarter of the ROI: 10x40 of the 40x40 goes to
    # 255. Mean abs diff ~= (10*40)*155 / (40*40) ~= 38.75 -> well above 18.
    changed[:10, :] = 255
    score = tracker.score(changed)
    assert score >= high_threshold


def test_baseline_converges_under_running_average():
    # Feed a constant new value; the baseline should approach it geometrically,
    # so the diff score decays toward 0 over successive frames.
    alpha = 0.2
    tracker = diff.BaselineTracker(alpha=alpha, blur_kernel=1)
    tracker.score(_gray(0))  # baseline seeded at 0

    scores = [tracker.score(_gray(100)) for _ in range(20)]
    # First diff is ~100 (baseline still ~0); each subsequent diff shrinks.
    assert scores[0] > 50
    assert scores[-1] < 5
    # Monotonically decreasing (baseline never overshoots a constant target).
    for earlier, later in zip(scores, scores[1:]):
        assert later <= earlier + 1e-6
    # Baseline has essentially reached the target.
    assert abs(float(tracker.baseline.mean()) - 100) < 5


def test_freeze_holds_baseline():
    alpha = 0.5
    tracker = diff.BaselineTracker(alpha=alpha, blur_kernel=1)
    tracker.score(_gray(0))  # baseline = 0
    baseline_before = tracker.baseline.copy()

    # A frozen update scores but does NOT move the baseline.
    score = tracker.score(_gray(100), freeze=True)
    assert score > 90
    assert np.array_equal(tracker.baseline, baseline_before)

    # An unfrozen update DOES move it.
    tracker.score(_gray(100), freeze=False)
    assert float(tracker.baseline.mean()) > 0


def test_bgr_frames_are_grayscaled():
    tracker = diff.BaselineTracker(alpha=0.02, blur_kernel=3)
    assert tracker.score(_bgr(120)) == 0.0
    # Identical BGR frame -> ~0 diff after grayscale.
    assert tracker.score(_bgr(120)) < 1e-3


def test_mean_abs_diff_scale_is_0_255():
    a = _gray(0).astype(np.float32)
    b = _gray(255).astype(np.float32)
    assert diff.mean_abs_diff(a, b) == 255.0
    assert diff.mean_abs_diff(a, a) == 0.0


def test_to_gray_blur_rejects_even_kernel():
    import pytest

    with pytest.raises(ValueError):
        diff.to_gray_blur(_gray(100), blur_kernel=4)
