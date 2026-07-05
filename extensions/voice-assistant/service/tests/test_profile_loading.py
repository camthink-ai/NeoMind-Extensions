"""Profile YAML loading + env var override."""
from __future__ import annotations

import os

import pytest

from profile import load_profile, Profile


def test_default_profile_loads():
    prof = load_profile(None)
    assert prof.name == "default"
    assert prof.vad_backend_type == "silero"
    assert prof.asr_config["type"] == "sensevoice_inproc"
    assert prof.llm_config["type"] == "neomind_ws"
    assert prof.tts_config["type"] == "zipvoice_inproc"
    assert prof.barge_in_mode == "full"


def test_named_profile_loads():
    prof = load_profile("headset")
    assert prof.vad_backend_type == "energy"
    assert prof.aec_config is None  # headset: no echo path


def test_edge_arm_profile_loads():
    """Verify edge-arm profile with Ollama LLM (non-NeoMind path)."""
    prof = load_profile("edge-arm")
    assert prof.cpu_threads == 6
    assert prof.llm_config["type"] == "ollama_http"
    assert prof.llm_config["model"] == "qwen3:1.7b"


def test_noisy_env_profile_loads():
    """Verify noisy-env profile has elevated VAD threshold."""
    prof = load_profile("noisy-env")
    assert prof.vad_config["threshold"] == 0.7
    assert prof.vad_config["min_speech_ms"] == 500


def test_env_override_vad(monkeypatch):
    monkeypatch.setenv("VOICE_ASSISTANT_VAD_BACKEND", "energy")
    prof = load_profile(None)
    assert prof.vad_backend_type == "energy"


def test_latency_target_informational():
    prof = load_profile(None)
    # Field exists but is informational only
    assert prof.latency_target_ms > 0


def test_aec_config_none_for_default_none():
    """Profile.from_dict with aec=none -> aec_config is None (backward compat)."""
    from profile import Profile
    prof = Profile.from_dict({
        "acoustic": {"aec": "none"},
        "backends": {},
    })
    assert prof.aec_config is None


def test_aec_config_full_dict_for_echo_window():
    """aec=echo_window -> aec_config is the new dict shape with keep_echo_window=True."""
    from profile import Profile
    prof = Profile.from_dict({
        "acoustic": {"aec": "echo_window"},
        "backends": {},
    })
    assert prof.aec_config == {
        "type": "echo_window",
        "reference_delay_ms": 200,
        "ref_buffer_seconds": 3.0,
        "keep_echo_window": True,
    }


def test_aec_config_full_dict_for_webrtc():
    """aec=webrtc -> aec_config dict with keep_echo_window=False by default."""
    from profile import Profile
    prof = Profile.from_dict({
        "acoustic": {"aec": "webrtc"},
        "backends": {},
    })
    assert prof.aec_config == {
        "type": "webrtc",
        "reference_delay_ms": 200,
        "ref_buffer_seconds": 3.0,
        "keep_echo_window": False,
    }


def test_aec_config_respects_yaml_overrides():
    """User-supplied aec_reference_delay_ms etc. override defaults."""
    from profile import Profile
    prof = Profile.from_dict({
        "acoustic": {
            "aec": "webrtc",
            "aec_reference_delay_ms": 350,
            "aec_ref_buffer_seconds": 5.0,
            "aec_keep_echo_window": True,
        },
        "backends": {},
    })
    assert prof.aec_config["reference_delay_ms"] == 350
    assert prof.aec_config["ref_buffer_seconds"] == 5.0
    assert prof.aec_config["keep_echo_window"] is True
