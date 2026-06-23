"""Shared pytest fixtures for the indoor-cam test suite.

These tests never touch a real camera, model, or server — capture, detection,
and HTTP are all faked. The ``make_result`` fixture returns a factory producing
a duck-typed stand-in for the trail-cam ``DetectionResult`` the bridge/pipeline
consume (``.detections`` with ``.class_name``/``.confidence``/``.bbox``, plus
``.total_count``, ``.image_path``, ``.timestamp``, ``.camera_id``,
``.inference_time_ms``).
"""

from __future__ import annotations

from types import SimpleNamespace

import pytest


def _make_detection(class_name="quail", confidence=0.85, bbox=(100.0, 100.0, 200.0, 200.0)):
    return SimpleNamespace(class_name=class_name, confidence=confidence, bbox=list(bbox))


def _make_result(
    camera_id="indoor-1",
    confidences=(0.85,),
    total=None,
    image_path="/processed/indoor-1/20260101-120000_indoor-1.jpg",
    timestamp="2026-01-01T00:00:00+00:00",
):
    detections = [_make_detection(confidence=c) for c in confidences]
    return SimpleNamespace(
        image_path=image_path,
        camera_id=camera_id,
        timestamp=timestamp,
        total_count=len(detections) if total is None else total,
        detections=detections,
        inference_time_ms=12.3,
        model_version="stub.pt",
    )


@pytest.fixture
def make_result():
    """Factory for a fake DetectionResult (see module docstring)."""
    return _make_result
