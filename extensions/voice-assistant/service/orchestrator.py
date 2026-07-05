"""Voice orchestrator: explicit FSM + pipeline + barge-in.

Phase 1 — this task implements ONLY the StateMachine class. Pipeline and
barge-in logic are added in subsequent tasks.
"""
from __future__ import annotations

import asyncio
import logging
import os
from dataclasses import dataclass, field
from enum import Enum
from typing import Callable, Awaitable

logger = logging.getLogger("voice-assistant.orchestrator")


# Common ASR hallucination outputs on ambient noise ( SenseVoice on room
# background, fan noise, keyboard, etc.). Most are short English words even
# when language=zh, because SenseVoice falls back to its English training data
# when it can't find clear phonetic content.
_NOISE_TOKENS = {
    "", ".", ",", "!", "?", "。", "，", "！", "？",
    "yeah", "yes", "oh", "ah", "wow", "hey",
    "嗯", "啊", "哦", "唉", "哈", "嗨",
    "ok", "okay",
}
_NOISE_MAX_LEN = 4  # anything <= 4 chars (after strip+punct removal) is suspect


def _is_noise_transcript(text: str) -> bool:
    """Heuristic: detect ASR hallucinations on ambient noise.

    Triggers if the transcript is a single short token after stripping
    whitespace and trailing punctuation. Catches "Yeah." / "." / "Oh." /
    "我." (single CJK char) etc. Legit short utterances like "你好" / "开灯"
    (2+ chars) pass through.
    """
    if not text:
        return True
    # Strip leading/trailing whitespace + common punctuation
    cleaned = text.strip().strip(".,!?。,！？~·…")
    if not cleaned:
        return True
    if cleaned.lower() in _NOISE_TOKENS:
        return True
    # Single-character transcript (any language) is almost always hallucination.
    if len(cleaned) <= 1:
        return True
    # Short (≤4 chars) non-CJK string = likely English hallucination.
    if len(cleaned) <= _NOISE_MAX_LEN and not any('\u4e00' <= c <= '\u9fff' for c in cleaned):
        return True
    return False


class State(str, Enum):
    IDLE = "idle"
    LISTENING = "listening"
    THINKING = "thinking"
    SPEAKING = "speaking"
    BARGED = "barged"


# Valid transitions: from_state → set of allowed target states. The table
# is AUTHORITATIVE — no runtime special-casing. BARGED is listed in every
# interruptible source state so barge-in is always admissible by table
# lookup alone.
_VALID_TRANSITIONS: dict[State, set[State]] = {
    # THINKING allowed from IDLE: VoicePipeline.run_turn starts a fresh turn
    # from IDLE (between turns) without going through LISTENING, which is only
    # entered during VAD detection in server.py's ws_handler.
    State.IDLE: {State.LISTENING, State.THINKING, State.BARGED},
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
            if target not in _VALID_TRANSITIONS.get(self._state, set()):
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
    """Coordinates the 3 parallel cleanup actions on barge-in.

    Each action is an async callable; failures are logged but don't halt others.
    Uses a lock to ensure idempotency for concurrent barge-in triggers.

    ASR-drain was previously a 4th action but the orchestrator has no ASR
    state — server.py's VoiceSession owns the VAD ring buffer and resets
    via its own FSM→LISTENING path. Kept as 3 actions.

    Optional `play_ack` runs AFTER cleanup (so browser has cleared its
    playback queue) but BEFORE transitioning to LISTENING — used for
    ChatGPT-style backchannel acknowledgment ("好的" / "嗯哼").
    """
    cancel_tts_playback: Callable[[], Awaitable[None]]
    cancel_llm_request: Callable[[], Awaitable[None]]
    clear_pending_queues: Callable[[], Awaitable[None]]
    play_ack: Callable[[], Awaitable[None]] | None = None
    _lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    _in_progress: bool = field(default=False)

    async def handle_barge_in(self, fsm: StateMachine, reason: str) -> None:
        """Transition to BARGED, run 3 cleanups, play ack, transition to LISTENING.

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
        ]
        results = await asyncio.gather(*cleanup_tasks, return_exceptions=True)
        for i, r in enumerate(results):
            if isinstance(r, Exception):
                logger.warning("barge-in cleanup task %d failed: %s", i, r)

        # Backchannel ack — runs after cleanup so browser playback queue is
        # already cleared by the `barge_in` control frame.
        if self.play_ack is not None:
            try:
                await self.play_ack()
            except Exception as e:
                logger.warning("ack playback failed: %s", e)

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
        on_asr_start=None,
        on_asr_complete=None,
        on_skip=None,
        on_tts_start=None,
        on_tts_end=None,
        on_error=None,
        play_ack=None,
        on_thinking_start=None,
        on_tool_call=None,
        on_llm_sentence=None,
        on_asr_partial=None,
    ):
        self.vad = vad
        self.asr = asr
        self.llm = llm
        self.tts = tts
        self.on_tts_pcm = on_tts_pcm  # async callable(pcm_bytes, sample_rate)
        self.on_stop_playback = on_stop_playback  # async callable() -> None
        self.telemetry = telemetry
        self.voice = voice
        self.on_asr_start = on_asr_start  # async callable(bytes_count: int)
        self.on_asr_complete = on_asr_complete  # async callable(transcript: str, elapsed_ms: float)
        self.on_skip = on_skip  # async callable(reason: str)
        self.on_tts_start = on_tts_start  # async callable()
        self.on_tts_end = on_tts_end  # async callable(metrics: dict)
        self.on_error = on_error  # async callable(phase: str, message: str)
        # Stage feedback — fire voice fillers so user knows what's happening.
        # on_thinking_start: no-arg async, fires after ASR transcript confirmed.
        # on_tool_call: async(tool_name: str | None), fires on LLM ToolCallStart.
        self.on_thinking_start = on_thinking_start
        self.on_tool_call = on_tool_call
        # Phase 2: progressive subtitle callback — async(seq: int, text: str).
        # Fires once per completed LLM sentence, in order.
        self.on_llm_sentence = on_llm_sentence
        # Streaming-ASR partial-transcript callback — async(text: str). Fires
        # per partial chunk from backends that implement stream(). None for
        # backends that only do batched transcribe().
        self.on_asr_partial = on_asr_partial
        # Whether to use streaming ASR when the backend supports it. Toggled
        # by profile config (streaming: true under backends.asr).
        self._streaming_asr = bool(
            os.environ.get("VOICE_ASSISTANT_ASR_STREAMING", "")
        ) or getattr(getattr(asr, "streaming", None), "__bool__", lambda: False)()
        self.fsm = StateMachine()
        # AEC threshold hack: raise VAD threshold during TTS playback to avoid
        # the assistant's own voice triggering barge-in. Restored after.
        self._original_vad_threshold = getattr(vad, "threshold", None)
        # Per-turn sentence queue, lifted to instance scope so barge-in can
        # drain it. Reassigned fresh at the top of each run_turn.
        self._sentence_q: asyncio.Queue | None = None
        # Barge-in handler — wires the 3 cleanup actions to pipeline-owned state.
        # play_ack (optional) runs AFTER cleanup, BEFORE transition to LISTENING.
        self.barge_in = BargeInHandler(
            cancel_tts_playback=self._cancel_tts_playback,
            cancel_llm_request=self._cancel_llm,
            clear_pending_queues=self._clear_queues,
            play_ack=play_ack,
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
        # Raise VAD threshold during THINKING too. Without this, the bare
        # threshold + AEC fallback (echo_window half-duplex) leaks false
        # positives from prior TTS playback residue into the THINKING
        # window → premature barge-in kills the slow-but-real LLM
        # response (qwen3.5 with tools has ~2.5s first-token latency).
        # Same bump as SPEAKING below (line ~440) so THINKING and
        # SPEAKING share one "user can't interrupt for a moment" contract.
        if self._original_vad_threshold is not None:
            mult = float(os.environ.get(
                "VOICE_ASSISTANT_THINKING_VAD_MULT",
                os.environ.get("VOICE_ASSISTANT_SPEAKING_VAD_MULT", "30")))
            floor = float(os.environ.get(
                "VOICE_ASSISTANT_THINKING_VAD_FLOOR", "0.4"))
            self.vad.threshold = max(
                self._original_vad_threshold * mult, floor)
        turn_start = time.perf_counter()

        # Compute PCM byte count for the segment (int16 = 2 bytes per sample).
        pcm_byte_count = len(segment.samples) * 2

        if self.on_asr_start is not None:
            await self.on_asr_start(pcm_byte_count)

        asr_start = time.perf_counter()

        # ---- ASR ----
        try:
            transcript = await self._run_asr(segment.samples, segment.sample_rate)
        except Exception as exc:
            logger.exception("ASR failed in run_turn")
            if self.on_error is not None:
                await self.on_error("asr", str(exc))
            # Best-effort return to a clean state
            if self.fsm.state == State.THINKING:
                await self.fsm.async_transition(State.IDLE)
            return

        asr_ms = (time.perf_counter() - asr_start) * 1000
        if self.telemetry:
            self.telemetry.observe("asr_complete_ms", asr_ms)

        if self.on_asr_complete is not None:
            await self.on_asr_complete(transcript, asr_ms)

        logger.info(
            "ASR transcript: %r (len=%d, state=%s, asr_ms=%.1f, "
            "pcm_bytes=%d, sample_rate=%d, duration_ms=%.0f)",
            transcript, len(transcript), self.fsm.state, asr_ms,
            pcm_byte_count, segment.sample_rate,
            pcm_byte_count / 2.0 / segment.sample_rate * 1000.0,
        )

        if self.fsm.state == State.BARGED:
            if self.telemetry:
                self.telemetry.increment_barge_ins()
            return  # barge-in fired during ASR

        # Empty transcript -> back to IDLE without invoking LLM/TTS.
        if not transcript.strip():
            if self.on_skip is not None:
                await self.on_skip("empty_transcript")
            await self.fsm.async_transition(State.IDLE)
            return

        # Noise filter — ASR (esp. SenseVoice) often hallucinates short
        # English tokens on ambient noise. Reject transcripts that are too
        # short or match common hallucination patterns.
        if _is_noise_transcript(transcript):
            logger.info("rejecting noise-like transcript: %r", transcript)
            if self.on_skip is not None:
                await self.on_skip("noise_transcript")
            await self.fsm.async_transition(State.IDLE)
            return

        # ---- LLM + TTS bi-streaming ----
        # Producer (LLM reader) and consumer (sentence-level TTS player) run
        # as two concurrent tasks wired through a bounded asyncio.Queue.
        # As soon as the first complete sentence is produced by the LLM, the
        # consumer starts TTS for it — overlapping LLM generation of later
        # sentences with TTS playback of earlier ones. This collapses the
        # N-sentence first-audio latency from "sum(LLM) + sum(TTS)" down to
        # "first-sentence LLM + first-sentence TTS first-chunk".
        #
        # Fire "thinking" stage filler so the user gets immediate voice
        # feedback before the (potentially slow) LLM produces first token.
        if self.on_thinking_start is not None:
            try:
                await self.on_thinking_start()
            except Exception as e:
                logger.warning("on_thinking_start failed: %s", e)

        # Function-local import: sentence_buffer has no heavy deps so this
        # is effectively free after first import (module cached). Kept local
        # to minimize orchestrator's top-level surface and match the
        # pattern of other turn-scoped helpers.
        from sentence_buffer import SentenceBuffer

        llm_start = time.perf_counter()
        # tts_start is captured BEFORE the first sentence is enqueued, so it
        # represents the moment TTS work logically begins (not when the
        # consumer picks it up). This keeps tts_first_chunk_ms comparable to
        # the Phase 1 single-stream measurement.
        tts_start = time.perf_counter()
        # Lifted to instance scope so _clear_queues (barge-in) can drain it.
        # Fresh queue per turn; old reference is replaced atomically.
        sentence_q: asyncio.Queue[str | None] = asyncio.Queue(maxsize=4)
        self._sentence_q = sentence_q
        buf = SentenceBuffer()
        first_token_observed = False
        llm_first_sentence_observed = False
        tts_first_ms: float = 0.0  # set by tts_consumer; read by on_tts_end
        # State shared between producer + consumer. Closed over by both.
        state = {
            "first_chunk_delivered": False,
            "tts_first_ms": 0.0,
            "llm_produced_any": False,
        }

        async def _fire_sentence(seq: int, s: str) -> None:
            """Fire-and-forget wrapper: log callback failures instead of
            letting them surface as 'Task exception was never retrieved'."""
            try:
                await self.on_llm_sentence(seq, s)
            except Exception as e:
                logger.warning("on_llm_sentence failed: %s", e)

        async def tts_consumer() -> None:
            """Pop sentences from the queue; for each, run TTS stream → PCM.

            The THINKING→SPEAKING transition, VAD threshold raise, and
            on_tts_start callback fire ONCE — right before the first
            sentence's first chunk is delivered. Barge-in is checked
            between sentences and between chunks.

            ``on_llm_sentence`` is dispatched fire-and-forget so a slow WS
            send can't stall TTS first-chunk for the next sentence. ``seq``
            indexes EMITTED subtitle frames — on barge-in, emitted count
            may exceed played count (last sentence's TTS was skipped).
            """
            nonlocal tts_first_ms
            seq = 0
            while True:
                s = await sentence_q.get()
                if s is None:
                    return
                # Progressive subtitle callback — fires per completed sentence.
                # create_task on the same loop preserves emission order; the
                # browser may see a tight burst but never out-of-order frames.
                if self.on_llm_sentence is not None:
                    asyncio.create_task(_fire_sentence(seq, s))
                seq += 1
                # Barge-in check before starting a new sentence's TTS.
                if self.fsm.state == State.BARGED:
                    return
                # First-sentence setup: FSM → SPEAKING, raise VAD threshold,
                # fire on_tts_start. Done here (not at producer) so the
                # SPEAKING window opens just before the first audible PCM.
                if not state["first_chunk_delivered"]:
                    await self.fsm.async_transition(State.SPEAKING)
                    if self._original_vad_threshold is not None:
                        mult = float(os.environ.get(
                            "VOICE_ASSISTANT_SPEAKING_VAD_MULT", "30"))
                        floor = float(os.environ.get(
                            "VOICE_ASSISTANT_SPEAKING_VAD_FLOOR", "0.4"))
                        self.vad.threshold = max(
                            self._original_vad_threshold * mult, floor)
                    if self.on_tts_start is not None:
                        await self.on_tts_start()
                try:
                    async for chunk in self.tts.stream(s, self.voice):
                        if self.fsm.state == State.BARGED:
                            return
                        await self.on_tts_pcm(chunk.pcm_int16, chunk.sample_rate)
                        if not state["first_chunk_delivered"]:
                            state["first_chunk_delivered"] = True
                            tts_first_ms = (time.perf_counter() - tts_start) * 1000
                            if self.telemetry:
                                self.telemetry.observe(
                                    "tts_first_chunk_ms", tts_first_ms)
                                self.telemetry.observe(
                                    "first_audio_out_ms",
                                    (time.perf_counter() - turn_start) * 1000)
                except Exception as exc:
                    logger.exception("TTS failed in tts_consumer")
                    if self.on_error is not None:
                        await self.on_error("tts", str(exc))
                    return

        consumer_task = asyncio.create_task(tts_consumer())
        try:
            # ---- Producer: LLM stream → sentence queue ----
            async for evt in self.llm.stream(transcript, session_id=str(id(self))):
                # Observe TTFB on first non-empty Content token.
                if (
                    not first_token_observed
                    and evt.type == "Content"
                    and getattr(evt, "text", None)
                ):
                    first_token_observed = True
                    state["llm_produced_any"] = True
                    if self.telemetry:
                        self.telemetry.observe(
                            "llm_ttfb_ms",
                            (time.perf_counter() - llm_start) * 1000,
                        )
                if evt.type == "Content" and getattr(evt, "text", None):
                    logger.info("LLM Content token: %r", evt.text)
                    for sentence in buf.feed(evt.text):
                        logger.info("TTS sentence queued: %r", sentence)
                        if not llm_first_sentence_observed:
                            llm_first_sentence_observed = True
                            if self.telemetry:
                                self.telemetry.observe(
                                    "llm_first_sentence_ms",
                                    (time.perf_counter() - llm_start) * 1000,
                                )
                        # Pre-check barge-in before parking on put. If the
                        # consumer was cancelled mid-stream the queue may be
                        # full; a blocked put() would only be released by the
                        # outer current_pipeline_task.cancel() (which server
                        # .py's ws_handler always fires on barge-in). The
                        # pre-check avoids the park entirely when we already
                        # know we're barged.
                        if self.fsm.state == State.BARGED:
                            if self.telemetry:
                                self.telemetry.increment_barge_ins()
                            consumer_task.cancel()
                            return
                        # Bounded put: yields backpressure if LLM outpaces
                        # TTS (rare; Kokoro is faster than qwen3 LLM).
                        await sentence_q.put(sentence)
                # Tool-call stage filler — fire once on first ToolCallStart.
                if evt.type == "ToolCallStart" and self.on_tool_call is not None:
                    try:
                        await self.on_tool_call(getattr(evt, "tool_name", None))
                    except Exception as e:
                        logger.warning("on_tool_call failed: %s", e)
                    self.on_tool_call = None  # fire-once
                # Barge-in check after each LLM event. With bi-streaming the
                # consumer legitimately moves THINKING→SPEAKING mid-stream
                # (first sentence started playing) — that's normal, not an
                # interrupt. Only BARGED means the user broke in.
                if self.fsm.state == State.BARGED:
                    if self.telemetry:
                        self.telemetry.increment_barge_ins()
                    consumer_task.cancel()
                    return
            # Flush any residual tail as the final sentence.
            tail = buf.flush()
            if tail:
                if not llm_first_sentence_observed:
                    llm_first_sentence_observed = True
                    if self.telemetry:
                        self.telemetry.observe(
                            "llm_first_sentence_ms",
                            (time.perf_counter() - llm_start) * 1000,
                        )
                await sentence_q.put(tail)
            await sentence_q.put(None)  # sentinel → consumer exits
            await consumer_task
        except BaseException:
            consumer_task.cancel()
            raise
        finally:
            # Restore VAD threshold after the SPEAKING window closes.
            if self._original_vad_threshold is not None:
                self.vad.threshold = self._original_vad_threshold

        # No LLM content produced at all → back to IDLE without TTS.
        if not state["llm_produced_any"]:
            if self.on_skip is not None:
                await self.on_skip("empty_llm_output")
            await self.fsm.async_transition(State.IDLE)
            return

        # TTS produced no audio (empty reply from model) → skip.
        # If barge-in fired mid-consumer, leave the state to BargeInHandler.
        if not state["first_chunk_delivered"] and self.fsm.state != State.BARGED:
            if self.on_skip is not None:
                await self.on_skip("empty_tts_output")
            await self.fsm.async_transition(State.IDLE)
            return

        total_ms = (time.perf_counter() - turn_start) * 1000
        if self.telemetry:
            self.telemetry.observe("full_turn_ms", total_ms)
            self.telemetry.increment_turns()

        if self.on_tts_end is not None:
            await self.on_tts_end({
                "total_ms": total_ms,
                "tts_first_chunk_ms": tts_first_ms,
                "asr_ms": asr_ms,
            })

        # SPEAKING -> IDLE. Skip if barge-in fired mid-TTS — the
        # BargeInHandler owns the BARGED → LISTENING cleanup transition.
        if self.fsm.state != State.BARGED:
            await self.fsm.async_transition(State.IDLE)

    async def _cancel_tts_playback(self) -> None:
        """Notify the browser to stop audio playback (WS control frame)."""
        if self.on_stop_playback is not None:
            await self.on_stop_playback()

    async def _cancel_llm(self) -> None:
        """Cancel the in-flight LLM stream."""
        await self.llm.cancel(session_id=str(id(self)))

    async def _run_asr(self, samples, sample_rate: int) -> str:
        """Run ASR, preferring streaming when the backend + profile enable it.

        Streaming fires ``on_asr_partial`` per partial transcript so the UI
        can show a live subtitle while the model finishes decoding. If the
        backend has no ``stream()`` method, or the profile disabled
        streaming, falls back to one-shot ``transcribe()``.

        Aborts early (returns whatever has been accumulated so far) on
        barge-in mid-transcription.

        Note: we check ``type(self.asr)`` rather than the instance to avoid
        ``AsyncMock`` (used heavily in tests) auto-creating a ``stream``
        attribute that pretends to be a streaming backend.
        """
        # Inspect the class so AsyncMock (which auto-creates per-instance
        # attributes) doesn't accidentally enable the streaming branch.
        has_stream = (
            hasattr(type(self.asr), "stream")
            and callable(getattr(type(self.asr), "stream", None))
        )
        if has_stream and self._streaming_asr:
            transcript = ""
            async for pt in self.asr.stream(samples, sample_rate):
                if self.fsm.state == State.BARGED:
                    return transcript
                transcript = pt.text
                if not pt.is_final and self.on_asr_partial is not None:
                    try:
                        await self.on_asr_partial(pt.text)
                    except Exception as e:
                        logger.warning("on_asr_partial failed: %s", e)
                if pt.is_final:
                    break
            return transcript
        return await self.asr.transcribe(samples, sample_rate)

    async def _clear_queues(self) -> None:
        """Drop any buffered sentences not yet consumed by the TTS consumer.

        The bi-streaming pipeline parks the producer on ``sentence_q.put``
        when the LLM outpaces TTS; on barge-in we want those buffered
        sentences discarded so the consumer doesn't wake up to enqueue more
        TTS work after we've already told the browser to silence playback.
        """
        q = self._sentence_q
        if q is None:
            return
        while not q.empty():
            try:
                q.get_nowait()
            except asyncio.QueueEmpty:
                break
