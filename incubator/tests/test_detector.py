"""Tests for detector.py — the anti-flap hysteresis + cooldown state machine.

Frames are plain grayscale patches and ``blur_kernel=1``, so the diff score is
just ``|current - baseline|`` and the state transitions are easy to reason about.
The clock is injected via ``process(crop, now=...)`` so cooldown is deterministic
without sleeping.
"""

import numpy as np

import config
from detector import DetectionEvent, SlotDetector, SlotState


def _gray(value, size=40):
    return np.full((size, size), value, dtype=np.uint8)


def _detection(**over):
    base = dict(
        baseline_alpha=0.02,
        high_threshold=18.0,
        low_threshold=8.0,
        cooldown_seconds=120,
        min_frames_before_detect=3,
        freeze_baseline_while_active=True,
        blur_kernel=1,
    )
    base.update(over)
    return config.DetectionConfig(**base)


def _slot():
    return config.Slot(id="A1", bbox=(0, 0, 40, 40), clutch_id=42)


def _settle(det, value=100, frames=3, start=1000.0):
    """Feed identical frames to get past min_frames and seed the baseline."""
    now = start
    for _ in range(frames):
        det.process(_gray(value), now)
        now += 10
    return now


def test_fires_once_on_step_change():
    det = SlotDetector(_slot(), _detection())
    now = _settle(det, value=100, frames=4)  # baseline ~100, warmup done

    event = det.process(_gray(200), now)  # |200-100| = 100 >= high(18)
    assert isinstance(event, DetectionEvent)
    assert event.slot_id == "A1"
    assert event.clutch_id == 42
    assert event.high_threshold == 18.0
    assert event.diff_score >= 18.0
    assert det.state is SlotState.ACTIVE


def test_does_not_refire_while_active():
    det = SlotDetector(_slot(), _detection())
    now = _settle(det, value=100, frames=4)

    first = det.process(_gray(200), now)
    assert first is not None
    # Keep feeding the changed frame: still a high score, but no new event.
    for i in range(5):
        again = det.process(_gray(200), now + 10 * (i + 1))
        assert again is None
        assert det.state is SlotState.ACTIVE


def test_returns_to_idle_only_after_settle_and_cooldown():
    det = SlotDetector(_slot(), _detection(cooldown_seconds=120))
    now = _settle(det, value=100, frames=4)

    det.process(_gray(200), now)  # fires at t=now
    fired_at = now

    # Low score but BEFORE cooldown elapses -> stays ACTIVE.
    still_active = det.process(_gray(100), fired_at + 60)
    assert still_active is None
    assert det.state is SlotState.ACTIVE

    # High score again mid-cooldown -> obviously stays ACTIVE, no re-fire.
    assert det.process(_gray(200), fired_at + 90) is None
    assert det.state is SlotState.ACTIVE

    # Low score AND cooldown elapsed -> back to IDLE.
    assert det.process(_gray(100), fired_at + 130) is None
    assert det.state is SlotState.IDLE


def test_high_score_after_cooldown_but_not_settled_stays_active():
    # ACTIVE -> IDLE requires the score to actually subside (<= low). A frame
    # still above low after the cooldown must NOT drop to IDLE.
    det = SlotDetector(_slot(), _detection(cooldown_seconds=100))
    now = _settle(det, value=100, frames=4)
    det.process(_gray(200), now)
    # Cooldown elapsed, but score still high (frozen baseline keeps it high).
    assert det.process(_gray(200), now + 200) is None
    assert det.state is SlotState.ACTIVE


def test_can_fire_again_after_returning_to_idle():
    det = SlotDetector(_slot(), _detection(cooldown_seconds=100))
    now = _settle(det, value=100, frames=4)
    assert det.process(_gray(200), now) is not None            # fire 1

    # Bring it home: low + cooldown -> IDLE. Baseline updates resume at value 100.
    det.process(_gray(100), now + 150)
    assert det.state is SlotState.IDLE

    # Re-settle the baseline at 100, then a fresh step fires a second event.
    n2 = now + 200
    for _ in range(3):
        det.process(_gray(100), n2)
        n2 += 10
    assert det.process(_gray(200), n2) is not None             # fire 2
    assert det.state is SlotState.ACTIVE


def test_honors_min_frames_before_detect():
    # A big change during the warmup window must not fire.
    det = SlotDetector(_slot(), _detection(min_frames_before_detect=5))
    now = 1000.0
    det.process(_gray(0), now)          # frame 1 seeds baseline at 0
    # Frames 2..5: huge scores, but suppressed.
    for i in range(2, 6):
        event = det.process(_gray(255), now + i)
        assert event is None
        assert det.state is SlotState.IDLE
    # Frame 6 is past the warmup: now a step can fire. Baseline has drifted up
    # from repeated 255s, so drop back to 0 for an unambiguous large diff.
    event = det.process(_gray(0), now + 6)
    assert event is not None
    assert det.state is SlotState.ACTIVE


def test_frozen_baseline_does_not_drift_while_active():
    det = SlotDetector(_slot(), _detection(freeze_baseline_while_active=True))
    now = _settle(det, value=100, frames=4)
    det.process(_gray(200), now)  # ACTIVE, baseline frozen at ~100
    frozen = det.baseline.baseline.copy()
    for i in range(5):
        det.process(_gray(200), now + 10 * (i + 1))
    assert np.array_equal(det.baseline.baseline, frozen)
