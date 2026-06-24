"""Voice orchestrator: explicit FSM + pipeline + barge-in.

Phase 1 — this task implements ONLY the StateMachine class. Pipeline and
barge-in logic are added in subsequent tasks.
"""
from __future__ import annotations

import asyncio
import logging
from enum import Enum
from typing import Callable, Awaitable

logger = logging.getLogger("voice-assistant.orchestrator")


class State(str, Enum):
    IDLE = "idle"
    LISTENING = "listening"
    THINKING = "thinking"
    SPEAKING = "speaking"
    BARGED = "barged"


# Valid transitions: from_state → set of allowed target states
_VALID_TRANSITIONS: dict[State, set[State]] = {
    State.IDLE: {State.LISTENING},
    State.LISTENING: {State.THINKING, State.BARGED, State.IDLE},
    State.THINKING: {State.SPEAKING, State.BARGED, State.IDLE},
    State.SPEAKING: {State.IDLE, State.BARGED},
    State.BARGED: {State.LISTENING},  # only after cleanup
}


class StateMachine:
    """Explicit FSM with asyncio-safe transitions.

    All transitions are serialized via a lock. Invalid transitions raise
    ValueError. An optional callback fires on every successful transition.
    """

    def __init__(self):
        self._state = State.IDLE
        self._lock = asyncio.Lock()
        self.on_transition: Callable[[State, State], Awaitable[None] | None] | None = None

    @property
    def state(self) -> State:
        return self._state

    def transition(self, target: State) -> None:
        """Synchronous transition (must be called from async context with lock held).

        For async-safe transitions, use `async_transition`.
        """
        # Self-transition is a no-op (idempotency for concurrent barge-ins)
        if target == self._state:
            return
        if target not in _VALID_TRANSITIONS.get(self._state, set()):
            if target != State.BARGED:  # BARGED is special — allowed from almost anywhere
                raise ValueError(
                    f"Invalid transition {self._state.value} → {target.value}"
                )
        prev = self._state
        self._state = target
        logger.debug("FSM: %s → %s", prev.value, target.value)
        if self.on_transition:
            result = self.on_transition(prev, target)
            if asyncio.iscoroutine(result):
                # Best-effort: schedule but don't block
                asyncio.create_task(result)

    async def async_transition(self, target: State) -> None:
        """Async-safe transition with lock."""
        async with self._lock:
            # Self-transition is a no-op (idempotency for concurrent barge-ins)
            if target == self._state:
                return
            # BARGED is always allowed as an interrupt
            if target == State.BARGED:
                pass
            elif target not in _VALID_TRANSITIONS.get(self._state, set()):
                raise ValueError(
                    f"Invalid transition {self._state.value} → {target.value}"
                )
            prev = self._state
            self._state = target
            logger.info("FSM: %s → %s", prev.value, target.value)
            if self.on_transition:
                result = self.on_transition(prev, target)
                if asyncio.iscoroutine(result):
                    await result
