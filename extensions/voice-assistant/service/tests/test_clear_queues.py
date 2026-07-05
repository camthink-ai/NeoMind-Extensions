"""sentence_q drain tests — barge-in must drop buffered sentences so the
TTS consumer doesn't wake up to enqueue more playback after the browser
has already been told to silence audio."""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock

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


def _pipeline_with_blocked_consumer():
    """Build a pipeline whose TTS consumer parks forever on the first
    sentence, so the producer fills sentence_q up to its bounded maxsize
    and any extra sentences stay buffered."""
    vad = MagicMock()
    vad.threshold = 0.5
    asr = AsyncMock()
    asr.transcribe = AsyncMock(return_value="你好")
    llm = AsyncMock()
    llm.cancel = AsyncMock()

    park = asyncio.Event()

    async def llm_stream(text, session_id):
        # Producer emits a sentence; consumer parks so the queue is non-empty.
        yield LlmEvent(type="Content", text="第一句。")
        # Wait for the test to drain + fire barge-in.
        try:
            await asyncio.wait_for(park.wait(), timeout=1.0)
        except asyncio.TimeoutError:
            pass
        yield LlmEvent(type="end")

    llm.stream = llm_stream

    tts = AsyncMock()

    async def tts_stream(text, voice):
        # Park on first chunk so the consumer stays alive mid-sentence.
        await asyncio.sleep(2.0)
        yield TtsChunk(pcm_int16=b"\x00\x00" * 10, sample_rate=24000, is_final=False)

    tts.stream = tts_stream
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)
    return pipeline, park


@pytest.mark.asyncio
async def test_clear_queues_drains_sentence_q():
    """_clear_queues drains whatever's currently buffered in sentence_q."""
    vad = MagicMock()
    vad.threshold = 0.5
    asr = AsyncMock()
    asr.transcribe = AsyncMock(return_value="你好")
    llm = AsyncMock()
    llm.cancel = AsyncMock()
    llm.stream = AsyncMock()  # not used in this test
    tts = AsyncMock()
    tts.stream = AsyncMock()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    # Simulate a sentence_q populated mid-turn.
    q: asyncio.Queue = asyncio.Queue(maxsize=4)
    pipeline._sentence_q = q
    await q.put("第一句。")
    await q.put("第二句。")
    assert not q.empty()

    await pipeline._clear_queues()

    assert q.empty(), "_clear_queues must drain sentence_q on barge-in"


@pytest.mark.asyncio
async def test_clear_queues_no_sentence_q_is_noop():
    """_clear_queues is a no-op when _sentence_q is None (before run_turn)."""
    vad = MagicMock()
    asr = AsyncMock()
    llm = AsyncMock()
    tts = AsyncMock()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)
    pipeline._sentence_q = None  # explicit
    # Must not raise.
    await pipeline._clear_queues()


@pytest.mark.asyncio
async def test_clear_queues_after_run_turn_does_not_crash():
    """After a turn completes, _sentence_q may still hold the trailing
    sentinel; draining it must not raise."""
    vad = MagicMock()
    vad.threshold = 0.5
    asr = AsyncMock()
    asr.transcribe = AsyncMock(return_value="你好")

    async def llm_stream(text, session_id):
        yield LlmEvent(type="Content", text="一句话。")
        yield LlmEvent(type="end")

    llm = AsyncMock()
    llm.stream = llm_stream
    llm.cancel = AsyncMock()
    tts = AsyncMock()

    async def tts_stream(text, voice):
        yield TtsChunk(pcm_int16=b"\x00\x00" * 10, sample_rate=24000, is_final=False)

    tts.stream = tts_stream
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr, llm, tts, on_tts_pcm=on_tts_pcm)

    await pipeline.run_turn(_make_segment())
    # After the turn, _sentence_q may be empty or still hold the sentinel;
    # either way, draining again must not raise.
    await pipeline._clear_queues()
