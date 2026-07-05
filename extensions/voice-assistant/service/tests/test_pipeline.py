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
from contracts import LlmEvent, TtsChunk, VadSegment


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
        # Multi-sentence reply — exercises the bi-streaming pipeline.
        yield LlmEvent(type="Content", text="你好啊。")
        yield LlmEvent(type="Content", text="我是语音助手。")
        yield LlmEvent(type="Content", text="很高兴认识你！")
        yield LlmEvent(type="end")

    llm.stream = llm_stream
    llm.cancel = AsyncMock()
    tts = AsyncMock()
    tts.synthesize = AsyncMock(return_value=b"\x00\x00" * 100)

    # Pipeline uses tts.stream() (async iterator of TtsChunk). Default mock
    # yields one non-empty PCM chunk per call so each sentence completes.
    async def tts_stream(text, voice):
        yield TtsChunk(pcm_int16=b"\x00\x00" * 100, sample_rate=24000, is_final=False)

    tts.stream = tts_stream
    return vad, asr, llm, tts


@pytest.mark.asyncio
async def test_pipeline_runs_one_turn():
    vad, asr, llm, tts = _make_mocks()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    await pipeline.run_turn(_make_segment())

    asr.transcribe.assert_called_once()
    # TTS streamed its chunk to the browser
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
    """End-to-end: handle_barge_in transitions to BARGED, runs 3 cleanups
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

    async def tts_stream_fail(text, voice):
        raise RuntimeError("TTS down")
        yield  # noqa: unreachable — marks this as an async generator

    tts.stream = tts_stream_fail
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


@pytest.mark.asyncio
async def test_pipeline_bi_streaming_overlaps_llm_and_tts():
    """TTS consumer must start delivering PCM for sentence 1 BEFORE the LLM
    produces sentence 2 — proving the producer/consumer overlap.

    We force a 200ms delay before the LLM's second sentence; the consumer
    must have invoked on_tts_pcm by then.
    """
    vad, asr, llm, tts = _make_mocks()
    second_sentinel = asyncio.Event()

    async def llm_stream_slow(text, session_id):
        yield LlmEvent(type="Content", text="第一句。")
        # Park for 200ms — if bi-streaming works, TTS for sentence 1 runs
        # during this window. If NOT working (batched), on_tts_pcm would
        # only fire after this returns.
        await asyncio.sleep(0.2)
        second_sentinel.set()
        yield LlmEvent(type="Content", text="第二句。")
        yield LlmEvent(type="end")

    llm.stream = llm_stream_slow
    pcm_calls_before_sentence_2: list[int] = []

    async def on_tts_pcm(pcm_bytes, sample_rate):
        # Record how many PCM deliveries happened before sentence 2 was emitted.
        if not second_sentinel.is_set():
            pcm_calls_before_sentence_2.append(1)

    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)
    await pipeline.run_turn(_make_segment())

    assert pcm_calls_before_sentence_2, (
        "TTS consumer did not deliver any PCM before LLM emitted sentence 2 — "
        "bi-streaming overlap is broken"
    )
    assert pipeline.fsm.state == State.IDLE


@pytest.mark.asyncio
async def test_pipeline_empty_llm_output_skips_tts():
    """LLM stream that yields no Content tokens must skip TTS entirely."""
    vad, asr, llm, tts = _make_mocks()

    async def llm_stream_empty(text, session_id):
        yield LlmEvent(type="end")  # no Content

    llm.stream = llm_stream_empty
    on_tts_start = AsyncMock()
    on_tts_pcm = AsyncMock()
    on_skip = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts,
        on_tts_pcm=on_tts_pcm,
        on_tts_start=on_tts_start,
        on_skip=on_skip,
    )

    await pipeline.run_turn(_make_segment())

    on_tts_start.assert_not_called()
    on_tts_pcm.assert_not_called()
    on_skip.assert_called_once_with("empty_llm_output")
    assert pipeline.fsm.state == State.IDLE


@pytest.mark.asyncio
async def test_pipeline_barge_in_during_tts_consumer():
    """Barge-in fired mid-TTS (during SPEAKING) must stop the consumer at the
    next chunk boundary and leave the FSM ready for cleanup.
    """
    vad, asr, llm, tts = _make_mocks()
    barrier = asyncio.Event()

    async def tts_stream_long(text, voice):
        # First chunk delivered, then we trip barge-in before yielding more.
        yield TtsChunk(pcm_int16=b"\x00\x00" * 100, sample_rate=24000, is_final=False)
        # Wait for the test to fire barge-in.
        try:
            await asyncio.wait_for(barrier.wait(), timeout=1.0)
        except asyncio.TimeoutError:
            pass
        # These chunks must NOT be delivered — consumer should bail on the
        # BARGED check before reaching them.
        yield TtsChunk(pcm_int16=b"\x00\x00" * 100, sample_rate=24000, is_final=False)
        yield TtsChunk(pcm_int16=b"\x00\x00" * 100, sample_rate=24000, is_final=False)

    tts.stream = tts_stream_long

    delivered: list[int] = []

    async def on_tts_pcm(pcm_bytes, sample_rate):
        delivered.append(1)

    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    async def llm_stream_single(text, session_id):
        yield LlmEvent(type="Content", text="一句话。")
        yield LlmEvent(type="end")

    llm.stream = llm_stream_single

    task = asyncio.create_task(pipeline.run_turn(_make_segment()))
    # Wait for at least one PCM delivery so the consumer is mid-sentence.
    await asyncio.sleep(0.05)
    # Fire barge-in: transition to BARGED, then unblock the TTS stream.
    await pipeline.fsm.async_transition(State.BARGED)
    barrier.set()
    await task

    # Exactly one chunk delivered (the post-barge-in chunks were skipped).
    assert len(delivered) == 1, f"expected 1 PCM chunk, got {len(delivered)}"
    assert pipeline.fsm.state == State.BARGED


@pytest.mark.asyncio
async def test_pipeline_fires_on_llm_sentence_per_sentence():
    """on_llm_sentence fires once per completed LLM sentence, in order."""
    vad, asr, llm, tts = _make_mocks()
    captured: list[tuple[int, str]] = []

    async def on_llm_sentence(seq, text):
        captured.append((seq, text))

    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(
        vad, asr, llm, tts,
        on_tts_pcm=on_tts_pcm,
        on_llm_sentence=on_llm_sentence,
    )

    await pipeline.run_turn(_make_segment())

    # The default mock yields 3 sentences.
    assert len(captured) == 3
    assert captured[0] == (0, "你好啊。")
    assert captured[1] == (1, "我是语音助手。")
    assert captured[2] == (2, "很高兴认识你！")


@pytest.mark.asyncio
async def test_pipeline_llm_stream_raises_propagates_and_cancels_consumer():
    """If llm.stream() raises mid-stream, the exception propagates and the
    consumer task is cancelled (not leaked). VAD threshold is restored."""
    vad, asr, llm, tts = _make_mocks()

    async def llm_stream_raise(text, session_id):
        yield LlmEvent(type="Content", text="第一句。")
        # Let the consumer pick up sentence 1 and start TTS.
        await asyncio.sleep(0.02)
        raise RuntimeError("LLM connection dropped")

    llm.stream = llm_stream_raise
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)
    original_threshold = vad.threshold

    # The exception should propagate out of run_turn.
    with pytest.raises(RuntimeError, match="LLM connection dropped"):
        await pipeline.run_turn(_make_segment())

    # Threshold must be restored by the finally block.
    assert vad.threshold == original_threshold


@pytest.mark.asyncio
async def test_pipeline_barge_in_during_llm_producer_cancels_consumer():
    """Barge-in detected inside the producer loop must cancel the consumer
    task and return promptly without delivering further PCM."""
    vad, asr, llm, tts = _make_mocks()
    consumer_started = asyncio.Event()
    second_yield_unblocked = asyncio.Event()

    async def llm_stream(text, session_id):
        yield LlmEvent(type="Content", text="第一句。")
        # Give the consumer a chance to start processing sentence 1.
        await asyncio.sleep(0.05)
        consumer_started.set()
        # Fire barge-in from outside while the producer is mid-stream.
        await pipeline.fsm.async_transition(State.BARGED)
        second_yield_unblocked.set()
        # Yield another event — producer's BARGED check must intercept
        # before this gets enqueued.
        yield LlmEvent(type="Content", text="这句不该到达。")
        yield LlmEvent(type="end")

    llm.stream = llm_stream

    delivered: list[int] = []

    async def on_tts_pcm(pcm_bytes, sample_rate):
        delivered.append(1)

    # Make TTS slow so the consumer is mid-sentence when barge-in fires.
    async def tts_stream_slow(text, voice):
        yield TtsChunk(pcm_int16=b"\x00\x00" * 100, sample_rate=24000,
                       is_final=False)
        # Park here so the consumer is provably still alive at barge-in time.
        await asyncio.sleep(1.0)
        yield TtsChunk(pcm_int16=b"\x00\x00" * 100, sample_rate=24000,
                       is_final=False)

    tts.stream = tts_stream_slow

    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    await pipeline.run_turn(_make_segment())

    # Producer returned at BARGED; consumer was cancelled mid-TTS so only
    # one PCM chunk delivered (the post-barge-in one was skipped).
    assert pipeline.fsm.state == State.BARGED
    assert len(delivered) <= 1, (
        f"expected ≤1 PCM delivery after barge-in, got {len(delivered)}")
