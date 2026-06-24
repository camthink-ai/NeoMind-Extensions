"""Voice orchestrator: explicit FSM + pipeline + barge-in.

Phase 1 — this task implements ONLY the StateMachine class. Pipeline and
barge-in logic are added in subsequent tasks.
"""
from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
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


@dataclass
class BargeInHandler:
    """Coordinates the 4 parallel cleanup actions on barge-in.

    Each action is an async callable; failures are logged but don't halt others.
    Uses a lock to ensure idempotency for concurrent barge-in triggers.
    """
    cancel_tts_playback: Callable[[], Awaitable[None]]
    cancel_llm_request: Callable[[], Awaitable[None]]
    clear_pending_queues: Callable[[], Awaitable[None]]
    drain_asr_buffer: Callable[[], Awaitable[None]]
    _lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    _in_progress: bool = field(default=False)

    async def handle_barge_in(self, fsm: StateMachine, reason: str) -> None:
        """Transition to BARGED, run 4 cleanups, transition to LISTENING.

        Idempotent: if a barge-in is already in progress, return immediately.
        """
        async with self._lock:
            if self._in_progress:
                logger.debug("barge-in already in progress, skipping (reason=%s)", reason)
                return
            if fsm.state == State.IDLE or fsm.state == State.LISTENING:
                logger.debug("barge-in in state %s, nothing to cancel", fsm.state.value)
                return
            self._in_progress = True

        logger.info("BARGE-IN triggered (reason=%s, from=%s)", reason, fsm.state.value)
        await fsm.async_transition(State.BARGED)

        cleanup_tasks = [
            self.cancel_tts_playback(),
            self.cancel_llm_request(),
            self.clear_pending_queues(),
            self.drain_asr_buffer(),
        ]
        results = await asyncio.gather(*cleanup_tasks, return_exceptions=True)
        for i, r in enumerate(results):
            if isinstance(r, Exception):
                logger.warning("barge-in cleanup task %d failed: %s", i, r)

        await fsm.async_transition(State.LISTENING)
        async with self._lock:
            self._in_progress = False
