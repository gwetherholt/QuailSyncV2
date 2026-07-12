"""Per-slot hysteresis + cooldown state machine.

A frame-difference score alone flaps: a slot right at the threshold would fire an
event on every frame. This state machine turns the raw score into *at most one
event per change episode*, using two-level hysteresis plus a cooldown.

Each slot is ``IDLE`` or ``ACTIVE``:

* **IDLE → ACTIVE** when ``diff_score >= high_threshold``. Emit exactly one
  event, stamp ``last_event_time``, and (upstream) save a crop.
* **While ACTIVE** no further events are emitted — this is the re-fire guard.
  If ``freeze_baseline_while_active`` is set, the slot's baseline stops updating
  so a slow hatch doesn't self-cancel by drifting the baseline toward the changed
  state.
* **ACTIVE → IDLE** only once ``diff_score <= low_threshold`` *and*
  ``now - last_event_time >= cooldown_seconds``. Baseline updates then resume.

Detection is fully suppressed for the first ``min_frames_before_detect`` frames
after startup so the baseline can settle before it can trigger anything.

The state machine is pure and clock-injected: :meth:`SlotDetector.process` takes
the current time, so tests drive it deterministically without sleeping. Crop
saving and DB writes live in :mod:`storage`; this module only decides *when* an
event happens.
"""

from __future__ import annotations

import enum
from dataclasses import dataclass

try:
    from . import config as config_module
    from .diff import BaselineTracker
except ImportError:  # running as a plain script / imported top-level in tests
    import config as config_module
    from diff import BaselineTracker

Slot = config_module.Slot
DetectionConfig = config_module.DetectionConfig


class SlotState(enum.Enum):
    IDLE = "idle"
    ACTIVE = "active"


@dataclass(frozen=True)
class DetectionEvent:
    """One 'change detected' event for a slot, ready to hand to storage."""

    slot_id: str
    diff_score: float
    high_threshold: float
    clutch_id: int | None
    event_type: str = "change_detected"


class SlotDetector:
    """Diff scorer + hysteresis/cooldown state machine for one slot."""

    def __init__(
        self,
        slot: Slot,
        detection: DetectionConfig,
        *,
        cv2_module=None,
    ):
        self.slot = slot
        self.cfg = detection
        self.state = SlotState.IDLE
        self.frames_seen = 0
        self.last_event_time: float | None = None
        self.last_score: float = 0.0
        self._baseline = BaselineTracker(
            alpha=detection.baseline_alpha,
            blur_kernel=detection.blur_kernel,
            cv2_module=cv2_module,
        )

    @property
    def baseline(self) -> BaselineTracker:
        return self._baseline

    def process(self, crop, now: float) -> DetectionEvent | None:
        """Feed one ROI crop for this slot at time ``now`` (epoch seconds).

        Returns a :class:`DetectionEvent` on an IDLE→ACTIVE transition, else
        ``None``. Always advances the baseline (unless the slot is ACTIVE and
        freezing is enabled).
        """
        self.frames_seen += 1

        # Freeze the baseline while ACTIVE (if configured) so a slow change
        # doesn't blend itself away before it's scored below low_threshold.
        freeze = self.state is SlotState.ACTIVE and self.cfg.freeze_baseline_while_active
        score = self._baseline.score(crop, freeze=freeze)
        self.last_score = score

        # Let the baseline settle before anything can trigger.
        if self.frames_seen <= self.cfg.min_frames_before_detect:
            return None

        if self.state is SlotState.IDLE:
            if score >= self.cfg.high_threshold:
                self.state = SlotState.ACTIVE
                self.last_event_time = now
                return DetectionEvent(
                    slot_id=self.slot.id,
                    diff_score=score,
                    high_threshold=self.cfg.high_threshold,
                    clutch_id=self.slot.clutch_id,
                )
            return None

        # ACTIVE: hold (no re-fire) until the change has both subsided and the
        # cooldown has elapsed.
        elapsed = float("inf") if self.last_event_time is None else now - self.last_event_time
        if score <= self.cfg.low_threshold and elapsed >= self.cfg.cooldown_seconds:
            self.state = SlotState.IDLE
        return None


def build_detectors(
    slots,
    detection: DetectionConfig,
    *,
    cv2_module=None,
) -> "dict[str, SlotDetector]":
    """Construct one :class:`SlotDetector` per slot, keyed by slot id."""
    return {
        slot.id: SlotDetector(slot, detection, cv2_module=cv2_module)
        for slot in slots
    }
