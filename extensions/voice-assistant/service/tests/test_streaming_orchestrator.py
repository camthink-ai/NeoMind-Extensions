"""Streaming-ASR orchestrator integration tests.

Verifies that VoicePipeline._run_asr() picks the streaming branch when
the backend supports stream() and the profile enables it, fires
on_asr_partial per non-final PartialTranscript, and falls back to
batched transcribe() otherwise.
"""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock

import pytest

from orchestrator import VoicePipeline, State
from backends.asr import PartialTranscript
from contracts import VadSegment


def _make_segment() -> VadSegment:
    return VadSegment(
        samples=[0.0] * 16000,
        sample_rate=16000,
        start_ms=0,
        end_ms=1000,
    )


def _base_pipeline(*, streaming_flag: bool, asr_obj) -> VoicePipeline:
    vad = MagicMock()
    vad.threshold = 0.5
    llm = AsyncMock()
    llm.cancel = AsyncMock()
    tts = AsyncMock()
    on_tts_pcm = AsyncMock()
    pipeline = VoicePipeline(vad, asr_obj, llm, tts, on_tts_pcm=on_tts_pcm)
    pipeline._streaming_asr = streaming_flag
    return pipeline


@pytest.mark.asyncio
async def test_streaming_branch_fires_on_asr_partial():
    """When backend.stream() exists and _streaming_asr is True, _run_asr
    iterates partials and invokes on_asr_partial for each."""

    captured_partials: list[str] = []

    async def on_asr_partial(text: str) -> None:
        captured_partials.append(text)

    class _StreamASR:
        streaming = True

        async def stream(self, samples, sr):
            yield PartialTranscript(text="你", is_final=False)
            yield PartialTranscript(text="你好", is_final=False)
            yield PartialTranscript(text="你好世界", is_final=True, confidence=1.0)

        async def transcribe(self, samples, sr):
            assert False, "transcribe() must not be called when stream() is used"

    pipeline = _base_pipeline(streaming_flag=True, asr_obj=_StreamASR())
    pipeline.on_asr_partial = on_asr_partial

    text = await pipeline._run_asr([0.0] * 100, 16000)

    assert text == "你好世界"
    # Final partial must not fire on_asr_partial (only non-final ones do).
    assert captured_partials == ["你", "你好"]


@pytest.mark.asyncio
async def test_streaming_branch_falls_back_to_transcribe_when_disabled():
    """When _streaming_asr is False, _run_asr uses transcribe() even if
    the backend has a stream() method."""

    class _StreamASR:
        streaming = False

        async def stream(self, samples, sr):
            yield PartialTranscript(text="nope", is_final=True)
            assert False, "stream() must not run when streaming flag is False"

        async def transcribe(self, samples, sr):
            return "你好"

    pipeline = _base_pipeline(streaming_flag=False, asr_obj=_StreamASR())
    pipeline.on_asr_partial = AsyncMock()

    text = await pipeline._run_asr([0.0] * 100, 16000)
    assert text == "你好"
    pipeline.on_asr_partial.assert_not_called()


@pytest.mark.asyncio
async def test_streaming_branch_absent_when_backend_lacks_stream():
    """Backends without a stream() method always go through transcribe()."""

    class _BatchedASR:
        async def transcribe(self, samples, sr):
            return "你好"

    pipeline = _base_pipeline(streaming_flag=True, asr_obj=_BatchedASR())
    pipeline.on_asr_partial = AsyncMock()

    text = await pipeline._run_asr([0.0] * 100, 16000)
    assert text == "你好"
    pipeline.on_asr_partial.assert_not_called()


@pytest.mark.asyncio
async def test_streaming_aborts_on_barge_in():
    """Barge-in mid-stream must short-circuit _run_asr and return whatever
    has been accumulated so far."""

    class _StreamASR:
        streaming = True

        async def stream(self, samples, sr):
            yield PartialTranscript(text="你好", is_final=False)
            # Simulate barge-in fired by another task.
            pipeline.fsm._state = State.BARGED
            # Subsequent partials must be dropped by the orchestrator loop.
            yield PartialTranscript(text="后续不该到", is_final=False)
            yield PartialTranscript(text="后续不该到完整", is_final=True)

    pipeline = _base_pipeline(streaming_flag=True, asr_obj=_StreamASR())
    pipeline.on_asr_partial = AsyncMock()

    text = await pipeline._run_asr([0.0] * 100, 16000)
    # First partial fired, then BARGED tripped before the next iteration.
    assert text == "你好"
    # Only the pre-barge partial reached the callback.
    pipeline.on_asr_partial.assert_called_once_with("你好")


@pytest.mark.asyncio
async def test_on_asr_partial_exception_does_not_abort():
    """A flaky on_asr_partial callback must not abort the transcription."""

    class _StreamASR:
        streaming = True

        async def stream(self, samples, sr):
            yield PartialTranscript(text="你", is_final=False)
            yield PartialTranscript(text="你好", is_final=False)
            yield PartialTranscript(text="你好世界", is_final=True, confidence=1.0)

    pipeline = _base_pipeline(streaming_flag=True, asr_obj=_StreamASR())

    async def on_asr_partial(text):
        if text == "你":
            raise RuntimeError("WS send failed")

    pipeline.on_asr_partial = on_asr_partial

    text = await pipeline._run_asr([0.0] * 100, 16000)
    assert text == "你好世界"
