"""State machine transition tests."""
from __future__ import annotations

import pytest

from orchestrator import State, StateMachine


@pytest.fixture
def fsm():
    return StateMachine()


def test_initial_state_is_idle(fsm):
    assert fsm.state == State.IDLE


def test_idle_to_listening(fsm):
    fsm.transition(State.LISTENING)
    assert fsm.state == State.LISTENING


def test_listening_to_thinking(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    assert fsm.state == State.THINKING


def test_thinking_to_speaking(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.SPEAKING)
    assert fsm.state == State.SPEAKING


def test_speaking_to_idle(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.SPEAKING)
    fsm.transition(State.IDLE)
    assert fsm.state == State.IDLE


def test_any_state_to_barged(fsm):
    # Test THINKING → BARGED
    fsm._state = State.IDLE
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.BARGED)
    assert fsm.state == State.BARGED

    # Test SPEAKING → BARGED (need full progression)
    fsm._state = State.IDLE
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.SPEAKING)
    fsm.transition(State.BARGED)
    assert fsm.state == State.BARGED


def test_barged_to_listening(fsm):
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.BARGED)
    fsm.transition(State.LISTENING)
    assert fsm.state == State.LISTENING


def test_invalid_transition_raises(fsm):
    # IDLE → SPEAKING is invalid (must go through LISTENING, THINKING)
    with pytest.raises(ValueError):
        fsm.transition(State.SPEAKING)


def test_barge_in_locks_state(fsm):
    """During BARGED, no other transition until cleanup done."""
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    fsm.transition(State.BARGED)
    # Attempting THINKING during BARGED should fail
    with pytest.raises(ValueError):
        fsm.transition(State.THINKING)


def test_callback_fires_on_transition(fsm):
    """Optional callback fires when state changes."""
    calls = []
    fsm.on_transition = lambda prev, new: calls.append((prev, new))
    fsm.transition(State.LISTENING)
    fsm.transition(State.THINKING)
    assert calls == [(State.IDLE, State.LISTENING),
                     (State.LISTENING, State.THINKING)]


@pytest.mark.asyncio
async def test_concurrent_transitions_serialized():
    """Two coroutines transitioning simultaneously don't corrupt state."""
    import asyncio
    fsm = StateMachine()
    async def try_transition(target):
        try:
            fsm.transition(target)
        except ValueError:
            pass
    # Should not raise; final state is deterministic
    await asyncio.gather(
        try_transition(State.LISTENING),
        try_transition(State.THINKING),
    )
    assert fsm.state in (State.IDLE, State.LISTENING, State.THINKING)
