"""Backend interface contracts — single source of truth.

All backends implement these Protocols. The orchestrator depends only on
these interfaces, never on concrete classes.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import AsyncIterator, Protocol

import numpy as np


@dataclass
class VadSegment:
    """One complete speech segment from VAD."""
    samples: list[float]      # 16kHz mono float32
    sample_rate: int
    start_ms: int
    end_ms: int


@dataclass
class PartialTranscript:
    text: str
    is_final: bool
    confidence: float


@dataclass
class LlmEvent:
    # Type values match NeoMind WS event casing (verified in NeoMind source):
    # crates/neomind-api/src/handlers/sessions.rs serializes End as lowercase "end",
    # others are PascalCase. Error casing varies — handle both.
    type: str  # "Content" | "Thinking" | "ToolCallStart" | "ToolCallEnd" | "Progress" | "end" | "Error" | "error"
    text: str | None = None
    tool_name: str | None = None
    progress: float | None = None


@dataclass
class TtsChunk:
    pcm_int16: bytes
    sample_rate: int
    is_final: bool


class VADBackend(Protocol):
    """Voice Activity Detection backend."""
    def feed(self, pcm_int16: bytes) -> list[VadSegment]:
        """Feed 32ms audio chunk. Return any completed speech segments."""
        ...
    def flush(self) -> list[VadSegment]:
        """Return any in-progress segment at end of stream."""
        ...
    @property
    def sample_rate(self) -> int:
        """Expected input sample rate (typically 16000)."""
        ...


class ASRBackend(Protocol):
    """Automatic Speech Recognition backend."""
    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str:
        """Batch: transcribe a complete audio segment."""
        ...
    async def stream(
        self, pcm_iterator: AsyncIterator[list[float]]
    ) -> AsyncIterator[PartialTranscript]:
        """Streaming: continuously emit partials. Phase 2 path."""
        raise NotImplementedError


class LLMClient(Protocol):
    """LLM streaming client. Orchestrator filters Content events to TTS."""
    async def stream(self, user_text: str, session_id: str) -> AsyncIterator[LlmEvent]:
        ...
    async def cancel(self, session_id: str) -> None:
        """Best-effort cancel. Must be idempotent."""
        ...


class TTSBackend(Protocol):
    """Text-to-Speech backend."""
    async def synthesize(self, text: str, voice: str) -> bytes:
        """Batch: synthesize full utterance. Returns int16 LE PCM."""
        ...
    async def stream(self, text: str, voice: str) -> AsyncIterator[TtsChunk]:
        """Streaming: chunked PCM. Phase 2 path."""
        raise NotImplementedError


class AECBackend(Protocol):
    """Acoustic Echo Cancellation backend.

    Receives mic PCM (post-browser) and the corresponding reference
    PCM slice (what the server pushed to the speaker `delay_ms` ago).
    Returns cleaned mic PCM with echo component removed.

    All PCM is 16kHz mono int16 LE numpy array.
    """
    def init(self, sample_rate: int) -> bool:
        """Initialize for the given sample rate. Return True on success,
        False to trigger caller-side fallback to NoopAECBackend."""
        ...

    def process_capture(self, mic_pcm: "np.ndarray", reference_pcm: "np.ndarray") -> "np.ndarray":
        """Subtract echo from mic. Same length & dtype as input."""
        ...

    def close(self) -> None:
        """Release native resources. Idempotent."""
        ...
