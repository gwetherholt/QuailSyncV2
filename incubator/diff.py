"""Frame-difference scoring for a single incubator slot.

Per slot we keep a running-average *baseline* of what the slot normally looks
like, and score each new crop by how far it has drifted from that baseline:

1. Convert the ROI crop to grayscale and Gaussian-blur it (``blur_kernel``) to
   suppress sensor noise and sub-pixel jitter.
2. ``diff_score`` = mean absolute difference between the current (gray, blurred)
   crop and the baseline, on a 0–255 scale. Because it's a *mean*, the score is
   normalized by pixel count and therefore comparable across differently-sized
   slots.
3. Update the baseline toward the current frame:
   ``baseline = (1 - alpha) * baseline + alpha * current`` — unless the caller
   asks to freeze it (the detector freezes an ACTIVE slot so a slow hatch doesn't
   self-cancel by dragging the baseline toward the changed state).

cv2 is imported lazily and is injectable (``cv2_module``) so the scorer is
unit-testable without OpenCV installed — though a plain-numpy fallback is used
for the grayscale/blur when cv2 is genuinely absent.
"""

from __future__ import annotations

import numpy as np


def _cv2():
    import cv2  # lazy: keeps importing this module cheap and test-friendly
    return cv2


def to_gray_blur(crop, blur_kernel: int, *, cv2_module=None) -> np.ndarray:
    """Grayscale + Gaussian-blur ``crop``, returned as ``float32`` (0–255 range).

    ``crop`` may be a 3-channel BGR array (from the camera) or an already-gray
    2-D array (from tests). ``blur_kernel`` must be a positive odd integer.
    """
    if blur_kernel < 1 or blur_kernel % 2 == 0:
        raise ValueError(f"blur_kernel must be a positive odd integer, got {blur_kernel}")
    arr = np.asarray(crop)
    if arr.size == 0:
        raise ValueError("cannot score an empty crop (zero-area ROI)")

    cv2 = cv2_module if cv2_module is not None else _try_cv2()
    if arr.ndim == 3:
        if cv2 is not None:
            gray = cv2.cvtColor(arr, cv2.COLOR_BGR2GRAY)
        else:
            # Rec.601 luma; matches cv2's BGR2GRAY closely enough for scoring.
            b, g, r = arr[..., 0], arr[..., 1], arr[..., 2]
            gray = 0.114 * b + 0.587 * g + 0.299 * r
    else:
        gray = arr

    gray = gray.astype(np.float32)
    if cv2 is not None:
        gray = cv2.GaussianBlur(gray, (blur_kernel, blur_kernel), 0)
    elif blur_kernel > 1:
        gray = _box_blur(gray, blur_kernel)
    return gray.astype(np.float32)


def _try_cv2():
    try:
        return _cv2()
    except ImportError:
        return None


def _box_blur(gray: np.ndarray, kernel: int) -> np.ndarray:
    """Separable box blur — a cv2-free stand-in used only when OpenCV is absent.

    Not identical to a Gaussian, but the scorer only needs *some* low-pass to
    tame single-pixel noise; the running-average baseline does the rest.
    """
    pad = kernel // 2
    padded = np.pad(gray, pad, mode="edge")
    # Horizontal then vertical moving average via cumulative sums.
    def _blur_axis(a: np.ndarray) -> np.ndarray:
        csum = np.cumsum(a, axis=1, dtype=np.float64)
        csum = np.concatenate([np.zeros((a.shape[0], 1)), csum], axis=1)
        windowed = csum[:, kernel:] - csum[:, :-kernel]
        return windowed / kernel
    out = _blur_axis(padded)
    out = _blur_axis(out.T).T
    return out.astype(np.float32)


def mean_abs_diff(current: np.ndarray, baseline: np.ndarray) -> float:
    """Mean absolute difference between two same-shape arrays, as a float.

    On grayscale 0–255 inputs this is the 0–255 ``diff_score``.
    """
    if current.shape != baseline.shape:
        raise ValueError(
            f"shape mismatch: current {current.shape} vs baseline {baseline.shape}"
        )
    return float(np.mean(np.abs(current.astype(np.float32) - baseline.astype(np.float32))))


class BaselineTracker:
    """Per-slot running-average baseline and diff scorer.

    Call :meth:`score` once per captured frame for the slot. The first frame
    seeds the baseline and scores 0 (nothing to compare against yet); subsequent
    frames score against the baseline and then blend into it — unless ``freeze``
    is passed, in which case the baseline is held (the detector does this while a
    slot is ACTIVE).
    """

    def __init__(self, alpha: float, blur_kernel: int, *, cv2_module=None):
        if not 0.0 < alpha <= 1.0:
            raise ValueError(f"alpha must be in (0, 1], got {alpha}")
        self.alpha = float(alpha)
        self.blur_kernel = int(blur_kernel)
        self._cv2 = cv2_module
        self.baseline: np.ndarray | None = None
        self.frames_seen = 0

    def preprocess(self, crop) -> np.ndarray:
        return to_gray_blur(crop, self.blur_kernel, cv2_module=self._cv2)

    def score(self, crop, *, freeze: bool = False) -> float:
        """Score ``crop`` against the baseline and (unless frozen) update it.

        Returns the 0–255 ``diff_score``. The very first call seeds the baseline
        and returns 0.0.
        """
        current = self.preprocess(crop)
        self.frames_seen += 1

        if self.baseline is None:
            self.baseline = current
            return 0.0

        if current.shape != self.baseline.shape:
            # ROI size changed (e.g. a clamped edge crop shifted) — reseed rather
            # than crash. Treated as "no change" for this frame.
            self.baseline = current
            return 0.0

        diff = mean_abs_diff(current, self.baseline)
        if not freeze:
            self.baseline = (1.0 - self.alpha) * self.baseline + self.alpha * current
        return diff
