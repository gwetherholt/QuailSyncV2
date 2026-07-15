"""Tests for assignment.py — parsing, mode-change detection, and resilience.

The HTTP layer is a fake session (``.get`` returns a canned response); no
network, no ``requests`` dependency.
"""

import pytest

import config as cfg
from assignment import AssignmentPoller, AssignmentResult


class _FakeResp:
    def __init__(self, payload, *, status_ok=True):
        self._payload = payload
        self._status_ok = status_ok

    def raise_for_status(self):
        if not self._status_ok:
            raise RuntimeError("HTTP 500")

    def json(self):
        return self._payload


class _FakeSession:
    """Returns queued responses (or raises queued exceptions) in order."""

    def __init__(self, responses):
        self._responses = list(responses)
        self.calls = []

    def get(self, url, timeout=None):
        self.calls.append({"url": url, "timeout": timeout})
        item = self._responses.pop(0)
        if isinstance(item, Exception):
            raise item
        return item


def _poller(responses, *, default_mode="incubator"):
    return AssignmentPoller(
        "http://localhost:3000",
        "indoor_tapo",
        default_mode,
        session=_FakeSession(responses),
    )


# --- resolve_mode ----------------------------------------------------------


def test_resolve_mode_accepts_model_and_assignment_names():
    assert cfg.resolve_mode("incubation") == "incubation"
    assert cfg.resolve_mode("chick") == "chick"
    # Raw assignment names map like active_model_for() in quailsync-common.
    assert cfg.resolve_mode("incubator") == "incubation"
    assert cfg.resolve_mode("brooder") == "chick"


def test_resolve_mode_rejects_unknown():
    assert cfg.resolve_mode("hutch") is None
    assert cfg.resolve_mode("") is None
    assert cfg.resolve_mode(None) is None


# --- URL + parsing ---------------------------------------------------------


def test_poll_hits_the_camera_assignment_endpoint():
    session = _FakeSession([_FakeResp({"active_model": "incubation"})])
    poller = AssignmentPoller("http://localhost:3000/", "indoor_tapo", "incubator", session=session)
    poller.poll()
    assert session.calls[0]["url"] == "http://localhost:3000/api/cameras/indoor_tapo/assignment"


def test_poll_reads_active_model_field():
    poller = _poller([_FakeResp({"camera_id": "indoor_tapo", "assignment": "brooder", "active_model": "chick"})])
    result = poller.poll()
    assert isinstance(result, AssignmentResult)
    assert result.mode == "chick"
    assert result.reachable is True
    assert result.raw_active_model == "chick"
    assert poller.mode == "chick"


# --- first-run / default_mode ----------------------------------------------


def test_default_mode_before_any_poll_is_resolved():
    # default_mode is the assignment name "incubator" -> model "incubation".
    poller = _poller([])
    assert poller.mode == "incubation"


def test_first_poll_matching_default_is_not_a_change():
    poller = _poller([_FakeResp({"active_model": "incubation"})])
    result = poller.poll()
    assert result.mode == "incubation"
    assert result.changed is False  # first successful fetch is never a "change"


def test_backend_unreachable_on_first_run_keeps_default_mode():
    poller = _poller([ConnectionError("backend down")])
    result = poller.poll()
    assert result.reachable is False
    assert result.changed is False
    assert result.mode == "incubation"  # the resolved default
    assert poller.mode == "incubation"


# --- mode-change detection -------------------------------------------------


def test_detects_mode_change_across_polls():
    poller = _poller([
        _FakeResp({"active_model": "incubation"}),
        _FakeResp({"active_model": "chick"}),
    ])
    first = poller.poll()
    assert first.changed is False and first.mode == "incubation"
    second = poller.poll()
    assert second.changed is True and second.mode == "chick"
    assert poller.mode == "chick"


def test_unchanged_assignment_reports_no_change():
    poller = _poller([
        _FakeResp({"active_model": "chick"}),
        _FakeResp({"active_model": "chick"}),
    ])
    poller.poll()
    second = poller.poll()
    assert second.changed is False
    assert second.mode == "chick"


# --- resilience: keep last mode --------------------------------------------


def test_unreachable_after_success_keeps_last_mode():
    poller = _poller([
        _FakeResp({"active_model": "chick"}),
        TimeoutError("network blip"),
    ])
    poller.poll()  # -> chick
    result = poller.poll()  # backend down
    assert result.reachable is False
    assert result.changed is False
    assert result.mode == "chick"  # last-known kept
    assert poller.mode == "chick"


def test_http_error_status_keeps_last_mode():
    poller = _poller([
        _FakeResp({"active_model": "incubation"}),
        _FakeResp({}, status_ok=False),  # raise_for_status blows up
    ])
    poller.poll()
    result = poller.poll()
    assert result.reachable is False
    assert result.mode == "incubation"


def test_unrecognized_active_model_keeps_last_mode():
    poller = _poller([
        _FakeResp({"active_model": "chick"}),
        _FakeResp({"active_model": "garbage"}),
    ])
    poller.poll()
    result = poller.poll()
    assert result.reachable is True  # backend answered…
    assert result.changed is False   # …but with junk, so we hold
    assert result.mode == "chick"
    assert result.raw_active_model == "garbage"


def test_non_object_response_keeps_last_mode():
    poller = _poller([
        _FakeResp({"active_model": "incubation"}),
        _FakeResp(["not", "an", "object"]),
    ])
    poller.poll()
    result = poller.poll()
    assert result.reachable is False
    assert result.mode == "incubation"
