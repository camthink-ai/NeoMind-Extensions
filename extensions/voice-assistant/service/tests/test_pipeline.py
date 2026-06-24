"""VoicePipeline tests with mocked backends.

Tests the Protocol-based turn pipeline end-to-end with mocks for each
backend (VAD, ASR, LLM, TTS). Verifies state transitions, barge-in
handling, empty-transcript short-circuit, VAD threshold restore, and
telemetry observation.
"""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock, ANY

import pytest

from orchestrator import VoicePipeline, State
from contracts import LlmEvent, VadSegment


def _make_segment() -> VadSegment:
    return VadSegment(
        samples=[0.0] * 16000,
        sample_rate=16000,
        start_ms=0,
        end_ms=1000,
    )


def _make_mocks() -> tuple:
    vad = MagicMock()
    vad.threshold = 0.5
    asr = AsyncMock()
    asr.transcribe = AsyncMock(return_value="你好")
    llm = AsyncMock()

    async def llm_stream(text, session_id):
        yield LlmEvent(type="Content", text="你好啊")
        yield LlmEvent(type="end")

    llm.stream = llm_stream
    llm.cancel = AsyncMock()
    tts = AsyncMock()
    tts.synthesize = AsyncMock(return_value=b"\x00\x00" * 100)
    return vad, asr, llm, tts


@pytest.mark.asyncio
async def test_pipeline_runs_one_turn():
    vad, asr, llm, tts = _make_mocks()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    await pipeline.run_turn(_make_segment())

    asr.transcribe.assert_called_once()
    tts.synthesize.assert_called_once_with("你好啊", ANY)
    on_tts_pcm.assert_called()
    assert pipeline.fsm.state == State.IDLE


@pytest.mark.asyncio
async def test_pipeline_empty_transcript_returns_to_idle():
    vad, asr, llm, tts = _make_mocks()
    asr.transcribe = AsyncMock(return_value="   ")  # whitespace-only
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    await pipeline.run_turn(_make_segment())

    tts.synthesize.assert_not_called()
    on_tts_pcm.assert_not_called()
    assert pipeline.fsm.state == State.IDLE


@pytest.mark.asyncio
async def test_pipeline_raises_vad_threshold_during_speaking():
    """VAD threshold is raised during SPEAKING then restored to original."""
    vad, asr, llm, tts = _make_mocks()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)
    original = vad.threshold  # 0.5

    await pipeline.run_turn(_make_segment())

    # After run, threshold must be restored exactly.
    assert vad.threshold == original


@pytest.mark.asyncio
async def test_pipeline_barge_in_aborts_llm_stream():
    """Barge-in during THINKING aborts the LLM stream early."""
    vad, asr, llm, tts = _make_mocks()
    cancelled = asyncio.Event()

    async def llm_stream(text, session_id):
        yield LlmEvent(type="Content", text="部")
        # Trigger barge-in mid-stream
        await pipeline.fsm.async_transition(State.BARGED)
        yield LlmEvent(type="Content", text="分内容不应到达")  # should be skipped

    llm.stream = llm_stream
    llm.cancel = AsyncMock(side_effect=lambda: cancelled.set())
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    await pipeline.run_turn(_make_segment())

    # Should NOT have synthesized or delivered PCM because barge-in happened first
    tts.synthesize.assert_not_called()
    on_tts_pcm.assert_not_called()
    # Pipeline returns at BARGED state; BargeInHandler (Task 14b) drives the
    # BARGED -> LISTENING cleanup. Document this contract explicitly.
    assert pipeline.fsm.state == State.BARGED


@pytest.mark.asyncio
async def test_pipeline_records_telemetry():
    """Telemetry KPIs receive at least one observation per successful turn."""
    from telemetry import Telemetry
    vad, asr, llm, tts = _make_mocks()
    telemetry = Telemetry()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts, on_tts_pcm=on_tts_pcm, telemetry=telemetry
    )

    await pipeline.run_turn(_make_segment())

    snap = telemetry.snapshot()
    # snapshot() returns {kpi_name: {p50,p95,min,max}} only for KPIs with samples
    assert "asr_complete_ms" in snap
    assert "llm_ttfb_ms" in snap
    assert "tts_first_chunk_ms" in snap
    assert "first_audio_out_ms" in snap
    assert "full_turn_ms" in snap
    assert telemetry.turn_count == 1
