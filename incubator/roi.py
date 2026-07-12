"""Slot cropping + grid generation math for the incubator tray.

Two responsibilities, both pure (no cv2 / camera):

* :func:`crop` — extract a slot's region-of-interest from a full frame given its
  ``[x, y, w, h]`` bbox. Clamps to the frame bounds so an off-by-a-few bbox near
  an edge yields a smaller crop rather than an out-of-range slice.

* :func:`generate_grid` — propose an evenly-spaced ``rows x cols`` grid of slot
  bboxes over an image, with configurable outer margins and inter-cell gaps. This
  is what ``define_rois.py --grid ROWSxCOLS`` uses to bootstrap a tray layout the
  operator then nudges by hand.

Slot ids follow a spreadsheet convention: row 0 -> ``A``, col 0 -> ``1``, so the
top-left cell is ``A1`` and the cell one down/one right is ``B2``.
"""

from __future__ import annotations

import string
from typing import Any, Sequence

# A bbox is (x, y, w, h) in pixels.
Bbox = tuple[int, int, int, int]


def slot_label(row: int, col: int) -> str:
    """Spreadsheet-style label for a grid cell: ``(0, 0) -> "A1"``.

    Rows use letters (A, B, …, Z, AA, AB, …); columns use 1-based numbers.
    """
    if row < 0 or col < 0:
        raise ValueError(f"row/col must be >= 0, got row={row}, col={col}")
    letters = string.ascii_uppercase
    n = row
    label = ""
    while True:
        label = letters[n % 26] + label
        n = n // 26 - 1
        if n < 0:
            break
    return f"{label}{col + 1}"


def crop(frame: Any, bbox: Sequence[int]) -> Any:
    """Return the ``bbox`` region of ``frame`` (a HxW or HxWxC numpy array).

    ``bbox`` is ``(x, y, w, h)``. The slice is clamped to the frame so a bbox
    that runs past an edge returns the in-bounds portion instead of raising or
    wrapping (numpy negative-index) — a clamped crop still scores sensibly; a
    wrapped one would compare unrelated pixels.
    """
    x, y, w, h = (int(v) for v in bbox)
    if w <= 0 or h <= 0:
        raise ValueError(f"bbox w/h must be > 0, got w={w}, h={h}")
    height = frame.shape[0]
    width = frame.shape[1]
    x0 = max(0, min(x, width))
    y0 = max(0, min(y, height))
    x1 = max(x0, min(x + w, width))
    y1 = max(y0, min(y + h, height))
    return frame[y0:y1, x0:x1]


def generate_grid(
    image_width: int,
    image_height: int,
    rows: int,
    cols: int,
    *,
    margin_x: float = 0.05,
    margin_y: float = 0.05,
    gap_x: float = 0.01,
    gap_y: float = 0.01,
) -> list[dict]:
    """Propose an evenly-spaced ``rows x cols`` grid of slot bboxes.

    Margins and gaps are given as *fractions of the image dimension* (0.05 = 5%),
    so the same grid spec works regardless of capture resolution:

    * ``margin_x`` / ``margin_y`` — blank border left around the whole grid.
    * ``gap_x`` / ``gap_y`` — spacing between adjacent cells.

    Returns a list of ``{"id", "bbox": [x, y, w, h], "clutch_id": None}`` dicts in
    row-major order (A1, A2, …, B1, …), matching the ``tray.slots`` config shape.
    """
    if rows < 1 or cols < 1:
        raise ValueError(f"rows/cols must be >= 1, got rows={rows}, cols={cols}")
    if image_width < 1 or image_height < 1:
        raise ValueError(
            f"image dimensions must be >= 1, got {image_width}x{image_height}"
        )
    for name, value in (("margin_x", margin_x), ("margin_y", margin_y), ("gap_x", gap_x), ("gap_y", gap_y)):
        if value < 0:
            raise ValueError(f"{name} must be >= 0, got {value}")

    m_x = margin_x * image_width
    m_y = margin_y * image_height
    g_x = gap_x * image_width
    g_y = gap_y * image_height

    # Usable span after the outer margins on both sides.
    span_x = image_width - 2 * m_x
    span_y = image_height - 2 * m_y
    if span_x <= 0 or span_y <= 0:
        raise ValueError("margins leave no room for any cell — reduce margin_x/margin_y")

    # cols cells + (cols-1) gaps must fit in span_x.
    cell_w = (span_x - g_x * (cols - 1)) / cols
    cell_h = (span_y - g_y * (rows - 1)) / rows
    if cell_w <= 0 or cell_h <= 0:
        raise ValueError("gaps leave no room for any cell — reduce gap_x/gap_y or margins")

    slots: list[dict] = []
    for row in range(rows):
        for col in range(cols):
            x = m_x + col * (cell_w + g_x)
            y = m_y + row * (cell_h + g_y)
            bbox = [int(round(x)), int(round(y)), int(round(cell_w)), int(round(cell_h))]
            slots.append({"id": slot_label(row, col), "bbox": bbox, "clutch_id": None})
    return slots
