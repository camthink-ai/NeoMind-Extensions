"""Verify all Protocol interfaces are importable and structurally sound."""
from __future__ import annotations

import inspect
from contracts import (
    VADBackend, ASRBackend, LLMClient, TTSBackend, AECBackend,
    VadSegment, PartialTranscript, LlmEvent, TtsChunk,
)


def test_vad_protocol_methods():
    assert hasattr(VADBackend, "feed")
    assert hasattr(VADBackend, "flush")
    assert "sample_rate" in {
        name for name, _ in inspect.getmembers(VADBackend)
    }


def test_asr_protocol_methods():
    assert hasattr(ASRBackend, "transcribe")
    assert hasattr(ASRBackend, "stream")


def test_llm_protocol_methods():
    assert hasattr(LLMClient, "stream")
    assert hasattr(LLMClient, "cancel")


def test_tts_protocol_methods():
    assert hasattr(TTSBackend, "synthesize")
    assert hasattr(TTSBackend, "stream")


def test_llm_event_supports_progress():
    """LlmEvent must carry progress field per spec (Progress events)."""
    evt = LlmEvent(type="Progress", progress=0.5)
    assert evt.progress == 0.5
    assert evt.text is None


def test_llm_event_supports_tool_name():
    evt = LlmEvent(type="ToolCallStart", tool_name="weather")
    assert evt.tool_name == "weather"


def test_vad_segment_fields():
    seg = VadSegment(samples=[0.1, 0.2], sample_rate=16000, start_ms=0, end_ms=100)
    assert seg.sample_rate == 16000
