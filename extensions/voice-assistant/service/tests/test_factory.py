"""Factory creates correct backend type from Profile."""
from __future__ import annotations

import pytest

from backends import make_vad, make_asr, make_llm, make_tts
from backends.vad import SileroVAD, EnergyVAD
from backends.asr import SenseVoiceHTTPASR
from backends.llm import OllamaHTTPClient, NeoMindWSClient, FakeLLMClient
from backends.tts import ZipVoiceHTTP
from profile import Profile


def _profile(**overrides):
    base = dict(
        name="test", vad_backend_type="silero",
        vad_config={"threshold": 0.5, "min_speech_ms": 250, "silence_ms": 500},
        asr_config={"type": "sensevoice_http", "url": "http://m"},
        llm_config={"type": "ollama_http", "url": "http://m", "model": "qwen"},
        tts_config={"type": "zipvoice_http", "url": "http://m", "voice": "v"},
        aec_config=None, barge_in_mode="full",
        latency_target_ms=1000, cpu_threads=4,
        barge_in_ack=False, ack_words=["好的"],
        stage_filler_words={"thinking": ["让我想想"]},
        greeting_text="",
    )
    base.update(overrides)
    return Profile(**base)


def test_make_vad_silero():
    vad = make_vad(_profile(vad_backend_type="silero"))
    assert isinstance(vad, SileroVAD)


def test_make_vad_energy():
    vad = make_vad(_profile(vad_backend_type="energy"))
    assert isinstance(vad, EnergyVAD)


def test_make_asr():
    asr = make_asr(_profile())
    assert isinstance(asr, SenseVoiceHTTPASR)


def test_make_llm_ollama():
    llm = make_llm(_profile(llm_config={"type": "ollama_http", "url": "x", "model": "q"}))
    assert isinstance(llm, OllamaHTTPClient)


def test_make_llm_neomind():
    llm = make_llm(_profile(llm_config={"type": "neomind_ws", "url": "ws://x"}))
    assert isinstance(llm, NeoMindWSClient)


def test_make_llm_fake():
    llm = make_llm(_profile(llm_config={"type": "fake"}))
    assert isinstance(llm, FakeLLMClient)


def test_make_tts():
    tts = make_tts(_profile())
    assert isinstance(tts, ZipVoiceHTTP)


def test_make_vad_unknown_raises():
    with pytest.raises(ValueError):
        make_vad(_profile(vad_backend_type="nonexistent"))
