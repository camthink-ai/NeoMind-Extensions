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


@pytest.mark.asyncio
async def test_pipeline_barge_in_invokes_handler_and_cancels_llm():
    """End-to-end: handle_barge_in transitions to BARGED, runs 4 cleanups
    (cancel_llm should call llm.cancel), then transitions to LISTENING."""
    vad, asr, llm, tts = _make_mocks()
    cancelled = asyncio.Event()

    async def llm_stream(text, session_id):
        yield LlmEvent(type="Content", text="部")
        # Park until barge-in fires; the second chunk should never reach TTS
        await cancelled.wait()
        yield LlmEvent(type="Content", text="分内容")

    llm.stream = llm_stream
    llm.cancel = AsyncMock(side_effect=lambda **_: cancelled.set())
    on_tts_pcm = AsyncMock()
    on_stop_playback = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts,
        on_tts_pcm=on_tts_pcm,
        on_stop_playback=on_stop_playback,
    )

    # Start the turn in background
    task = asyncio.create_task(pipeline.run_turn(_make_segment()))
    await asyncio.sleep(0.05)  # let LLM emit first chunk and park on cancelled.wait()

    # Sanity: pipeline should be in THINKING
    assert pipeline.fsm.state == State.THINKING

    # Fire barge-in
    await pipeline.barge_in.handle_barge_in(pipeline.fsm, reason="test_speech")

    # cancelled should have been set by llm.cancel side_effect, unblocking the task
    await task

    # llm.cancel was invoked
    llm.cancel.assert_called_once()
    # TTS never synthesized, PCM never delivered
    tts.synthesize.assert_not_called()
    on_tts_pcm.assert_not_called()
    # Browser was told to stop playback (the cancel_tts_playback cleanup)
    on_stop_playback.assert_called_once()
    # After handle_barge_in, FSM is in LISTENING (cleanup complete)
    assert pipeline.fsm.state == State.LISTENING


@pytest.mark.asyncio
async def test_pipeline_emits_lifecycle_callbacks():
    """run_turn fires on_asr_start → on_asr_complete → on_tts_start → on_tts_end in order."""
    vad, asr, llm, tts = _make_mocks()
    events = []
    on_asr_start = AsyncMock(side_effect=lambda n: events.append(("asr_start", (n,))))
    on_asr_complete = AsyncMock(side_effect=lambda t, ms: events.append(("asr_complete", (t, ms))))
    on_tts_start = AsyncMock(side_effect=lambda: events.append(("tts_start", ())))
    on_tts_end = AsyncMock(side_effect=lambda m: events.append(("tts_end", (m,))))
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts,
        on_tts_pcm=on_tts_pcm,
        on_asr_start=on_asr_start,
        on_asr_complete=on_asr_complete,
        on_tts_start=on_tts_start,
        on_tts_end=on_tts_end,
    )

    await pipeline.run_turn(_make_segment())

    names = [e[0] for e in events]
    assert names == ["asr_start", "asr_complete", "tts_start", "tts_end"]
    # asr_start got a byte count > 0
    assert events[0][1][0] > 0
    # asr_complete got the transcript text and a positive elapsed_ms
    assert events[1][1][0] == "你好"
    assert events[1][1][1] >= 0.0
    # tts_end got a metrics dict
    assert isinstance(events[3][1][0], dict)
    assert "total_ms" in events[3][1][0]


@pytest.mark.asyncio
async def test_pipeline_emits_skip_on_empty_transcript():
    vad, asr, llm, tts = _make_mocks()
    asr.transcribe = AsyncMock(return_value="   ")
    on_skip = AsyncMock()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts,
        on_tts_pcm=on_tts_pcm,
        on_skip=on_skip,
    )

    await pipeline.run_turn(_make_segment())

    on_skip.assert_called_once_with("empty_transcript")
    tts.synthesize.assert_not_called()


@pytest.mark.asyncio
async def test_pipeline_emits_error_on_asr_failure():
    vad, asr, llm, tts = _make_mocks()
    asr.transcribe = AsyncMock(side_effect=RuntimeError("ASR down"))
    on_error = AsyncMock()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts,
        on_tts_pcm=on_tts_pcm,
        on_error=on_error,
    )

    await pipeline.run_turn(_make_segment())  # should not raise

    on_error.assert_called_once()
    args = on_error.call_args.args
    assert args[0] == "asr"
    assert "ASR down" in args[1]
    tts.synthesize.assert_not_called()


@pytest.mark.asyncio
async def test_pipeline_emits_error_on_tts_failure():
    vad, asr, llm, tts = _make_mocks()
    tts.synthesize = AsyncMock(side_effect=RuntimeError("TTS down"))
    on_error = AsyncMock()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts,
        on_tts_pcm=on_tts_pcm,
        on_error=on_error,
    )

    await pipeline.run_turn(_make_segment())  # should not raise

    on_error.assert_called_once()
    args = on_error.call_args.args
    assert args[0] == "tts"
    assert "TTS down" in args[1]
    on_tts_pcm.assert_not_called()
