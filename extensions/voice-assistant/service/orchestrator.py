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
    # THINKING allowed from IDLE: VoicePipeline.run_turn starts a fresh turn
    # from IDLE (between turns) without going through LISTENING, which is only
    # entered during VAD detection in server.py's ws_handler.
    State.IDLE: {State.LISTENING, State.THINKING},
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


class VoicePipeline:
    """Runs one voice turn: VAD segment -> ASR -> LLM -> TTS.

    Uses StateMachine for transitions. Backends are Protocol instances —
    the pipeline knows nothing about concrete types (see contracts.py).

    NOTE: server.py's run_pipeline_for_segment still uses the old function-
    based backends with different contracts (old asr_transcribe returns a
    dict, old llm_stream yields sentence strings, old tts_stream uses
    streaming with producer/consumer parallel TTS). Wiring VoicePipeline
    into server.py is deferred to Task 14c once Profile-backed backend
    instances are constructed.
    """

    def __init__(
        self,
        vad,
        asr,
        llm,
        tts,
        on_tts_pcm,
        on_stop_playback=None,
        telemetry=None,
        voice: str = "中文女",
    ):
        self.vad = vad
        self.asr = asr
        self.llm = llm
        self.tts = tts
        self.on_tts_pcm = on_tts_pcm  # async callable(pcm_bytes, sample_rate)
        self.on_stop_playback = on_stop_playback  # async callable() -> None
        self.telemetry = telemetry
        self.voice = voice
        self.fsm = StateMachine()
        # AEC threshold hack: raise VAD threshold during TTS playback to avoid
        # the assistant's own voice triggering barge-in. Restored after.
        self._original_vad_threshold = getattr(vad, "threshold", None)
        self._pcm_queue: asyncio.Queue | None = None  # set when streaming TTS queue is in use
        # Barge-in handler — wires the 4 cleanup actions to pipeline-owned state
        self.barge_in = BargeInHandler(
            cancel_tts_playback=self._cancel_tts_playback,
            cancel_llm_request=self._cancel_llm,
            clear_pending_queues=self._clear_queues,
            drain_asr_buffer=self._drain_asr,
        )

    async def run_turn(self, segment) -> None:
        """Process one VAD segment end-to-end.

        Transitions: IDLE -> THINKING -> SPEAKING -> IDLE.
        Returns early (state left at BARGED) if barge-in fires mid-turn,
        or returns to IDLE if the transcript is empty.

        Args:
            segment: contracts.VadSegment with samples + sample_rate.
        """
        import time

        # IDLE -> THINKING (a fresh turn starts from IDLE between turns).
        await self.fsm.async_transition(State.THINKING)
        turn_start = time.perf_counter()
        asr_start = time.perf_counter()

        # ---- ASR ----
        transcript = await self.asr.transcribe(
            segment.samples, segment.sample_rate
        )
        asr_ms = (time.perf_counter() - asr_start) * 1000
        if self.telemetry:
            self.telemetry.observe("asr_complete_ms", asr_ms)
        if self.fsm.state == State.BARGED:
            if self.telemetry:
                self.telemetry.increment_barge_ins()
            return  # barge-in fired during ASR

        # Empty transcript -> back to IDLE without invoking LLM/TTS.
        if not transcript.strip():
            await self.fsm.async_transition(State.IDLE)
            return

        # ---- LLM streaming ----
        llm_start = time.perf_counter()
        full_text = ""
        first_token_observed = False
        async for evt in self.llm.stream(transcript, session_id=str(id(self))):
            # Observe TTFB on first non-empty Content token.
            if (
                not first_token_observed
                and evt.type == "Content"
                and getattr(evt, "text", None)
            ):
                first_token_observed = True
                if self.telemetry:
                    self.telemetry.observe(
                        "llm_ttfb_ms",
                        (time.perf_counter() - llm_start) * 1000,
                    )
            if evt.type == "Content" and getattr(evt, "text", None):
                full_text += evt.text
            # Barge-in check after each event. During LLM streaming the
            # pipeline is in THINKING; any other state (BARGED, or LISTENING
            # after the BargeInHandler already completed cleanup) means the
            # turn was interrupted.
            if self.fsm.state != State.THINKING:
                if self.telemetry:
                    self.telemetry.increment_barge_ins()
                return

        # No content produced -> back to IDLE.
        if not full_text.strip():
            await self.fsm.async_transition(State.IDLE)
            return

        # ---- TTS ----
        # THINKING -> SPEAKING
        await self.fsm.async_transition(State.SPEAKING)
        # Raise VAD threshold during playback to suppress self-trigger.
        if self._original_vad_threshold is not None:
            self.vad.threshold = self._original_vad_threshold + 0.2

        tts_start = time.perf_counter()
        try:
            pcm = await self.tts.synthesize(full_text, self.voice)
        finally:
            # Always restore VAD threshold, even if synthesize raised.
            if self._original_vad_threshold is not None:
                self.vad.threshold = self._original_vad_threshold

        tts_ms = (time.perf_counter() - tts_start) * 1000
        if self.telemetry:
            self.telemetry.observe("tts_first_chunk_ms", tts_ms)

        # Re-check barge-in right before delivering PCM (may have fired
        # during TTS synthesis).
        if self.fsm.state == State.BARGED:
            if self.telemetry:
                self.telemetry.increment_barge_ins()
            return

        # Deliver PCM to player. ZipVoice outputs 24kHz int16 LE.
        await self.on_tts_pcm(pcm, 24000)

        total_ms = (time.perf_counter() - turn_start) * 1000
        if self.telemetry:
            self.telemetry.observe("first_audio_out_ms", total_ms)
            self.telemetry.observe("full_turn_ms", total_ms)
            self.telemetry.increment_turns()

        # SPEAKING -> IDLE
        await self.fsm.async_transition(State.IDLE)

    async def _cancel_tts_playback(self) -> None:
        """Notify the browser to stop audio playback (WS control frame)."""
        if self.on_stop_playback is not None:
            await self.on_stop_playback()

    async def _cancel_llm(self) -> None:
        """Cancel the in-flight LLM stream."""
        await self.llm.cancel(session_id=str(id(self)))

    async def _clear_queues(self) -> None:
        """Drop any buffered PCM chunks not yet sent to the browser."""
        if self._pcm_queue is None:
            return
        while not self._pcm_queue.empty():
            try:
                self._pcm_queue.get_nowait()
            except asyncio.QueueEmpty:
                break

    async def _drain_asr(self) -> None:
        """ASR is stateless at the Protocol level; nothing to drain.

        Orchestrator-level reset happens via the FSM transition to LISTENING.
        """
        pass
