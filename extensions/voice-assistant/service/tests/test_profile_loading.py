"""Profile YAML loading + env var override."""
from __future__ import annotations

import os

import pytest

from profile import load_profile, Profile


def test_default_profile_loads():
    prof = load_profile(None)
    assert prof.name == "default"
    assert prof.vad_backend_type == "silero"
    assert prof.asr_config["type"] == "sensevoice_http"
    assert prof.llm_config["type"] == "neomind_ws"
    assert prof.tts_config["type"] == "zipvoice_http"
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


def test_env_override_tts_url(monkeypatch):
    monkeypatch.setenv("VOICE_ASSISTANT_TTS_URL", "http://example:9999")
    prof = load_profile(None)
    assert prof.tts_config["url"] == "http://example:9999"


def test_latency_target_informational():
    prof = load_profile(None)
    # Field exists but is informational only
    assert prof.latency_target_ms > 0
