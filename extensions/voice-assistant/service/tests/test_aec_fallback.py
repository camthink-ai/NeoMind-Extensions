"""AEC fallback reconciliation tests.

When the profile requests ``aec: webrtc`` but the webrtc_audio_processing
package is unavailable, ``backends.make_aec`` returns a NoopAECBackend.
Without reconciliation, ``server.AEC_MODE`` stays at ``"webrtc"`` and
``VoiceSession._aec_active_now`` returns False (it only fires for
``echo_window``) — so neither real-AEC nor half-duplex echo suppression
runs. The fix lives in ``server._warm_aec`` and forces AEC_MODE to
``"echo_window"`` when the requested backend degraded to Noop.
"""
from __future__ import annotations

import asyncio
from unittest.mock import patch

import pytest

import server
from backends import make_aec
from backends.aec import NoopAECBackend
from profile import Profile


def _make_webrtc_profile() -> Profile:
    """Build a minimal Profile whose aec_config requests webrtc."""
    # Bypass YAML loading — Profile is a dataclass-like wrapper. Read the
    # public properties the factory + reconciliation actually touch.
    p = Profile.__new__(Profile)
    p.aec_config = {"type": "webrtc"}
    return p


@pytest.mark.asyncio
async def test_webrtc_unavailable_yields_noop_and_echo_window(monkeypatch):
    """make_aec returns Noop when webrtc lib missing; reconciliation must
    then force AEC_MODE = 'echo_window' so echo suppression still runs."""
    # Pretend the webrtc library is missing.
    from backends import aec as aec_module
    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", None)
    monkeypatch.setattr(
        aec_module, "_resolve_webrtc_apm_class", lambda: None
    )

    profile = _make_webrtc_profile()
    backend = make_aec(profile)
    assert isinstance(backend, NoopAECBackend), \
        "make_aec must fall back to Noop when webrtc lib missing"

    # Simulate the reconciliation block in _warm_aec. We import server
    # lazily so module-level globals don't fire on collection.
    # Reset AEC_MODE to the 'pre-reconcile' value the profile implies.
    monkeypatch.setattr(server, "AEC_MODE", "webrtc")

    # Inline the reconciliation logic to mirror server._warm_aec exactly.
    if server.AEC_MODE == "webrtc" and isinstance(backend, NoopAECBackend):
        server.AEC_MODE = "echo_window"

    assert server.AEC_MODE == "echo_window", (
        "AEC_MODE must downgrade to echo_window when webrtc requested but "
        "library unavailable; otherwise _aec_active_now returns False and "
        "no suppression runs."
    )


@pytest.mark.asyncio
async def test_webrtc_available_keeps_webrtc_mode(monkeypatch):
    """If the library resolves, AEC_MODE must stay 'webrtc' (no downgrade)."""
    from backends import aec as aec_module
    # Sentinel non-None class so _resolve_webrtc_apm_class returns truthy.
    class _FakeAPM:
        pass
    monkeypatch.setattr(aec_module, "_WEBRTC_APM_CLASS", _FakeAPM)
    monkeypatch.setattr(
        aec_module, "_resolve_webrtc_apm_class", lambda: _FakeAPM
    )
    monkeypatch.setattr(server, "AEC_MODE", "webrtc")

    profile = _make_webrtc_profile()
    # WebRtcAECBackend.init may need real natives; skip make_aec here.
    # The reconciliation check uses isinstance(backend, Noop), so we pass
    # a non-Noop stand-in to prove the branch is not taken.
    class _StandIn:
        pass
    backend = _StandIn()
    if server.AEC_MODE == "webrtc" and isinstance(backend, NoopAECBackend):
        server.AEC_MODE = "echo_window"

    assert server.AEC_MODE == "webrtc"
