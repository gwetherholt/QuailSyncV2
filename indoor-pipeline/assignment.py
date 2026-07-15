"""Polls the backend for the indoor camera's assignment / active model.

The pipeline is *assignment-aware*: which YOLO model it runs is decided by the
backend, not by config. On startup and every ``poll_seconds`` the loop calls
:meth:`AssignmentPoller.poll`, which does ``GET
{backend_url}/api/cameras/{camera_id}/assignment`` and reads the response's
``active_model`` field (``"incubation"`` or ``"chick"``; the backend derives it
from the assignment via ``active_model_for`` in quailsync-common).

Resilience contract (matches the sibling pipelines' settings clients):

* Before the first successful poll the mode is the configured ``default_mode``.
* A poll that reaches the backend and returns a recognized model updates the
  mode; :attr:`AssignmentResult.changed` flags whether it differs from before.
* If the backend is unreachable (or returns junk), the *last-known* mode is kept
  and ``reachable=False`` — the loop keeps running the current model and retries
  next poll. A model-not-found never comes from here; it comes from loading.

``requests`` is imported lazily and the HTTP session is injectable, so this
module imports cheaply and unit-tests without the network.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass

try:
    from . import config as config_module
except ImportError:  # plain-script / bare-name import (tests)
    import config as config_module

logger = logging.getLogger("indoorpipeline.assignment")


@dataclass(frozen=True)
class AssignmentResult:
    """Outcome of one poll cycle."""

    mode: str  # resolved model key currently in effect: "incubation" | "chick"
    changed: bool  # did the mode differ from the previous poll's mode
    reachable: bool  # was the backend reached (and returned a usable model)
    raw_active_model: str | None  # the raw ``active_model`` the backend returned


class AssignmentPoller:
    """Tracks the camera's active model, refreshing it from the backend."""

    def __init__(
        self,
        backend_url: str,
        camera_id: str,
        default_mode: str,
        *,
        session=None,
        timeout: float = 10.0,
    ):
        self.url = f"{backend_url.rstrip('/')}/api/cameras/{camera_id}/assignment"
        self.camera_id = camera_id
        self.session = session
        self.timeout = timeout
        # Resolve the configured default to a canonical model key. Config
        # validation guarantees this is not None.
        resolved = config_module.resolve_mode(default_mode)
        if resolved is None:  # pragma: no cover - guarded by config validation
            raise ValueError(f"invalid default_mode: {default_mode!r}")
        self.mode: str = resolved
        # True until the first successful backend read, so the first successful
        # poll doesn't spuriously report ``changed`` just because it matched the
        # default.
        self._never_fetched = True

    def _session_obj(self):
        if self.session is not None:
            return self.session
        import requests  # lazy: keeps the module importable without requests

        return requests

    def poll(self) -> AssignmentResult:
        """Fetch the assignment and update :attr:`mode`. Never raises.

        Returns an :class:`AssignmentResult`. On any error the last-known mode is
        kept and ``reachable=False``.
        """
        previous = self.mode
        try:
            resp = self._session_obj().get(self.url, timeout=self.timeout)
            resp.raise_for_status()
            data = resp.json()
            if not isinstance(data, dict):
                raise ValueError("assignment response was not a JSON object")
            raw_active_model = data.get("active_model")
        except Exception as exc:  # noqa: BLE001 — a poll failure must never crash the loop
            logger.warning(
                "Assignment poll failed (%s) — keeping current mode %r", exc, previous
            )
            return AssignmentResult(mode=previous, changed=False, reachable=False, raw_active_model=None)

        resolved = config_module.resolve_mode(raw_active_model if isinstance(raw_active_model, str) else None)
        if resolved is None:
            logger.warning(
                "Backend returned unrecognized active_model %r — keeping current mode %r",
                raw_active_model,
                previous,
            )
            return AssignmentResult(
                mode=previous, changed=False, reachable=True, raw_active_model=raw_active_model
            )

        # ``changed`` is relative to the last mode we were running, but the very
        # first successful fetch is never a "change" even if it matches default.
        changed = (resolved != previous) and not self._never_fetched
        self._never_fetched = False
        self.mode = resolved
        if changed:
            logger.info("Assignment changed: active_model %r -> %r", previous, resolved)
        return AssignmentResult(
            mode=resolved, changed=changed, reachable=True, raw_active_model=raw_active_model
        )
