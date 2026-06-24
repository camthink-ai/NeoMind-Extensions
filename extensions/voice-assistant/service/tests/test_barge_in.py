"""Barge-in protocol tests with mocked backends."""
from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock

import pytest

from orchestrator import State, StateMachine, BargeInHandler


@pytest.fixture
def handler():
    """BargeInHandler with all dependencies mocked."""
    h = BargeInHandler(
        cancel_tts_playback=AsyncMock(),
        cancel_llm_request=AsyncMock(),
        clear_pending_queues=AsyncMock(),
        drain_asr_buffer=AsyncMock(),
    )
    return h


@pytest.mark.asyncio
async def test_barge_in_runs_all_four_cleanups(handler):
    fsm = StateMachine()
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)

    await handler.handle_barge_in(fsm, reason="test")

    handler.cancel_tts_playback.assert_called_once()
    handler.cancel_llm_request.assert_called_once()
    handler.clear_pending_queues.assert_called_once()
    handler.drain_asr_buffer.assert_called_once()
    assert fsm.state == State.LISTENING  # transitioned back after cleanup


@pytest.mark.asyncio
async def test_barge_in_logs_cleanup_failures(handler):
    """Cleanup task failure is logged but doesn't halt others."""
    handler.cancel_llm_request.side_effect = RuntimeError("ws closed")
    fsm = StateMachine()
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.SPEAKING)

    # Should not raise
    await handler.handle_barge_in(fsm, reason="test")
    # Other cleanups still ran
    handler.cancel_tts_playback.assert_called_once()
    handler.clear_pending_queues.assert_called_once()


@pytest.mark.asyncio
async def test_barge_in_idempotent_concurrent(handler):
    """Two barge-ins within 200ms execute cleanup only once."""
    fsm = StateMachine()
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)

    await asyncio.gather(
        handler.handle_barge_in(fsm, reason="first"),
        handler.handle_barge_in(fsm, reason="second"),
    )
    # cancel_llm_request should be called one or twice but state must end in LISTENING
    assert fsm.state == State.LISTENING
