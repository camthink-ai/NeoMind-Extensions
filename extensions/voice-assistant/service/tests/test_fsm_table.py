"""FSM table reconciliation tests.

Validates that BARGED is reachable from every interruptible source state
purely via _VALID_TRANSITIONS lookup (no runtime special-casing), and
that the table is internally consistent.
"""
from __future__ import annotations

import pytest

from orchestrator import State, StateMachine, _VALID_TRANSITIONS


def test_barged_reachable_from_all_interruptible_states():
    """Every state that can be barged into must list BARGED in its row."""
    # IDLE, LISTENING, THINKING, SPEAKING all need BARGED reachability.
    # Only BARGED itself is excluded (BARGED -> BARGED is a self-transition,
    # already a no-op).
    interruptible = {State.IDLE, State.LISTENING, State.THINKING, State.SPEAKING}
    for src in interruptible:
        allowed = _VALID_TRANSITIONS.get(src, set())
        assert State.BARGED in allowed, (
            f"BARGED must be reachable from {src.value} via the table alone; "
            f"got allowed={sorted(s.value for s in allowed)}"
        )


def test_barged_only_transitions_to_listening():
    """Cleanup contract: BARGED -> LISTENING only."""
    assert _VALID_TRANSITIONS[State.BARGED] == {State.LISTENING}


@pytest.mark.parametrize("src", [State.IDLE, State.LISTENING, State.THINKING, State.SPEAKING])
def test_barged_transition_no_special_case(src):
    """transition() must accept src -> BARGED without any if-target==BARGED
    safety net. Confirms the table is AUTHORITATIVE (regression guard
    against re-introducing the old special-case)."""
    fsm = StateMachine()
    fsm._state = src
    fsm.transition(State.BARGED)
    assert fsm.state == State.BARGED


def test_idle_to_barged_direct_transition():
    """IDLE -> BARGED must work (previously special-cased)."""
    fsm = StateMachine()
    fsm.transition(State.BARGED)
    assert fsm.state == State.BARGED


def test_invalid_transition_still_raises():
    """Table-driven: SPEAKING from IDLE is not in the table -> ValueError."""
    fsm = StateMachine()
    with pytest.raises(ValueError):
        fsm.transition(State.SPEAKING)


def test_barged_to_thinking_still_invalid():
    """BARGED -> THINKING is not in the table; must raise (no special case)."""
    fsm = StateMachine()
    fsm._state = State.BARGED
    with pytest.raises(ValueError):
        fsm.transition(State.THINKING)
