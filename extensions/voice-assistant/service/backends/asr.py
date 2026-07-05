"""ASR backend implementations."""
from __future__ import annotations

import asyncio
import base64
import io
import json
import logging
import os
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import AsyncIterator

import httpx
import numpy as np

logger = logging.getLogger("voice-assistant.asr")


@dataclass
class PartialTranscript:
    """One streamed ASR chunk from a streaming backend.

    ``text`` is the *accumulated* transcript so far (not just the delta),
    so the UI can replace its subtitle with each new partial. ``is_final``
    marks the terminal chunk.
    """
    text: str
    is_final: bool = False
    confidence: float = 0.0


# Module-level cache for the heavy sherpa_onnx import (~700MB with ONNX
# Runtime natives). Lazy so HTTP-only users and unit tests don't pay the
# cost. Resolved via _get_sherpa() so tests can monkeypatch sys.modules.
_sherpa = None


def _get_sherpa():
    """Lazy sherpa_onnx import, cached on first call."""
    global _sherpa
    if _sherpa is None:
        import sherpa_onnx  # noqa: F401  (heavy, optional)
        _sherpa = sherpa_onnx
    return _sherpa


def _pcm_to_wav(pcm_int16: bytes, sample_rate: int, channels: int = 1) -> bytes:
    """Wrap raw int16 LE PCM in a WAV header."""
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm_int16)
    return buf.getvalue()


class SenseVoiceHTTPASR:
    """SenseVoice-Small ASR via HTTP (sensevoice-asr service on port 9383).

    Implements the ASRBackend Protocol from contracts.py.
    """

    def __init__(self, url: str, language: str = "auto", timeout: float = 30.0):
        self.url = url
        self.language = language
        self.timeout = timeout

    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str:
        """Transcribe a complete audio segment. Returns the recognized text.

        Request shape matches the sensevoice-asr service:
        POST /asr with JSON {"audio_base64": <b64 wav>, "language": ..., "use_itn": true}.
        """
        pcm_int16 = (np.clip(np.asarray(pcm_float32, dtype=np.float32), -1.0, 1.0)
                     * 32767).astype("<i2")
        wav_bytes = _pcm_to_wav(pcm_int16.tobytes(), sample_rate)
        b64 = base64.b64encode(wav_bytes).decode()
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            resp = await client.post(
                f"{self.url}/asr",
                json={
                    "audio_base64": b64,
                    "language": self.language,
                    "use_itn": True,
                },
            )
            resp.raise_for_status()
            return (resp.json().get("text") or "").strip()


# ---------------------------------------------------------------------------
# In-process SenseVoice backend (all-in-one mode)
# ---------------------------------------------------------------------------
# Calls sherpa_onnx.OfflineRecognizer directly — no HTTP boundary, no extra
# service/venv. Trade-off: adds ~700MB to the venv (ONNX Runtime natives).
# Implements the ASRBackend Protocol from contracts.py.

SENSEVOICE_MODEL_NAME = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"


def _download_sensevoice(dest: Path) -> None:
    """Download + extract the SenseVoice INT8 model tar.bz2 into dest.

    Idempotent: caller already checked model.int8.onnx is missing.
    """
    import tarfile
    import urllib.request

    url = (
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/"
        "asr-models/" + SENSEVOICE_MODEL_NAME + ".tar.bz2"
    )
    tmp_tar = dest.parent / f"{SENSEVOICE_MODEL_NAME}.tar.bz2"
    logger.info(
        "Downloading SenseVoice model (~230MB) from %s -> %s ...", url, tmp_tar,
    )
    urllib.request.urlretrieve(url, tmp_tar)
    logger.info("Extracting %s -> %s", tmp_tar, dest.parent)
    with tarfile.open(tmp_tar, "r:bz2") as t:
        t.extractall(dest.parent)
    tmp_tar.unlink(missing_ok=True)
    if not (dest / "model.int8.onnx").exists():
        raise RuntimeError(
            f"SenseVoice model.int8.onnx missing after extract at {dest}"
        )


class SenseVoiceInprocASR:
    """In-process SenseVoice-Small ASR. No HTTP boundary.

    Loads the OfflineRecognizer eagerly at construction (~0.5s) so first
    request doesn't pay. Auto-downloads model on first construction if
    missing. Implements ASRBackend Protocol.

    `transcribe()` wraps the sync decode in `asyncio.to_thread()` so the
    orchestrator's event loop stays responsive (decode is 100ms–1s on CPU;
    ONNX Runtime releases the GIL during inference).
    """

    def __init__(
        self,
        language: str = "auto",
        num_threads: int = 2,
        model_dir: str | None = None,
    ):
        sherpa = _get_sherpa()
        self.language = language
        base = model_dir or os.environ.get(
            "SENSEVOICE_ASR_MODEL_DIR",
            str(Path.home() / ".cache" / "sherpa-onnx"),
        )
        d = Path(base) / SENSEVOICE_MODEL_NAME
        if not (d / "model.int8.onnx").exists():
            d.mkdir(parents=True, exist_ok=True)
            _download_sensevoice(d)
        logger.info(
            "Loading SenseVoice recognizer (model=%s, threads=%d)", d, num_threads,
        )
        self._recognizer = sherpa.OfflineRecognizer.from_sense_voice(
            model=str(d / "model.int8.onnx"),
            tokens=str(d / "tokens.txt"),
            use_itn=True,
            num_threads=num_threads,
        )
        logger.info("SenseVoice recognizer ready")

    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str:
        """Transcribe a complete utterance. Linear-resamples to 16kHz first
        if needed (matching sensevoice-asr/service logic).
        """
        audio = self._resample(pcm_float32, sample_rate)
        return await asyncio.to_thread(self._decode_sync, audio)

    def _decode_sync(self, audio: np.ndarray) -> str:
        s = self._recognizer.create_stream()
        s.accept_waveform(16000, audio)
        self._recognizer.decode_stream(s)
        return (s.result.text or "").strip()

    @staticmethod
    def _resample(pcm: list[float], sr_in: int) -> np.ndarray:
        """Linear resample to 16kHz float32 (good enough for ASR)."""
        data = np.asarray(pcm, dtype=np.float32).reshape(-1)
        if sr_in == 16000:
            return data
        n_out = int(round(len(data) * 16000 / sr_in))
        idx = np.linspace(0, len(data) - 1, n_out)
        return np.interp(idx, np.arange(len(data)), data).astype(np.float32)


# ---------------------------------------------------------------------------
# Qwen3-ASR via llama.cpp (llama-server OpenAI-compatible API)
# ---------------------------------------------------------------------------
# llama.cpp ships Qwen3-ASR support behind its /v1/chat/completions endpoint.
# Audio is sent as a base64-encoded WAV data URL inside the message content;
# the model emits the transcript as text. With stream:true the response is an
# SSE stream of token deltas — gives the UI live subtitle feedback after VAD
# endpoint fires (real input-streaming during speech needs an OnlineRecognizer
# and is out of scope here).

def _build_chat_messages(b64_wav: str, mime: str = "audio/wav") -> list[dict]:
    """Build OpenAI-style messages for an audio-only ASR turn.

    llama.cpp accepts audio as a multipart content list with an ``image_url``
    part carrying a data URL. The trailing text instructs the model to
    transcribe rather than answer.
    """
    return [{
        "role": "user",
        "content": [
            {
                "type": "image_url",
                "image_url": {"url": f"data:{mime};base64,{b64_wav}"},
            },
            {
                "type": "text",
                "text": "Please transcribe the audio verbatim.",
            },
        ],
    }]


class Qwen3LlamaCppASR:
    """Qwen3-ASR served by llama.cpp's llama-server (OpenAI-compatible API).

    Configuration:
        url:        Base URL of llama-server (default http://127.0.0.1:8080).
        model:      Model identifier known to the server (e.g. GGUF filename).
        language:   Hint language code; "auto" lets the model decide.
        timeout:    HTTP timeout for the full transcription (non-streaming).

    ``transcribe()`` does one POST with stream:false; ``stream()`` does the
    same with stream:true and yields PartialTranscript per token delta.
    """

    def __init__(
        self,
        url: str = "http://127.0.0.1:8080",
        model: str = "qwen3-asr",
        language: str = "auto",
        timeout: float = 30.0,
        streaming: bool = False,
    ):
        self.url = url.rstrip("/")
        self.model = model
        self.language = language
        self.timeout = timeout
        # Exposed so the orchestrator can decide whether to use the streaming
        # branch at all (it also checks hasattr(stream)).
        self.streaming = bool(streaming)

    def _endpoint(self) -> str:
        return f"{self.url}/v1/chat/completions"

    def _payload(self, b64_wav: str, stream: bool) -> dict:
        # temperature=0 keeps transcription deterministic; max_tokens is
        # generous because long utterances can run several hundred tokens.
        return {
            "model": self.model,
            "messages": _build_chat_messages(b64_wav),
            "temperature": 0.0,
            "max_tokens": 1024,
            "stream": stream,
        }

    def _encode_audio(self, pcm_float32: list[float], sample_rate: int) -> str:
        pcm_int16 = (np.clip(np.asarray(pcm_float32, dtype=np.float32), -1.0, 1.0)
                     * 32767).astype("<i2")
        wav_bytes = _pcm_to_wav(pcm_int16.tobytes(), sample_rate)
        return base64.b64encode(wav_bytes).decode()

    async def transcribe(self, pcm_float32: list[float], sample_rate: int) -> str:
        """One-shot transcription. Returns the recognized text."""
        b64 = self._encode_audio(pcm_float32, sample_rate)
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            resp = await client.post(
                self._endpoint(),
                json=self._payload(b64, stream=False),
            )
            resp.raise_for_status()
            data = resp.json()
        choices = data.get("choices") or []
        if not choices:
            return ""
        msg = choices[0].get("message") or {}
        return (msg.get("content") or "").strip()

    async def stream(
        self, pcm_float32: list[float], sample_rate: int
    ) -> AsyncIterator[PartialTranscript]:
        """Token-streaming transcription. Yields accumulated-text partials.

        The model still processes the whole utterance up front (audio encoder
        is not incremental) but emits the decoded transcript token-by-token,
        which lowers perceived TTFT and lets the UI show a live subtitle.
        """
        b64 = self._encode_audio(pcm_float32, sample_rate)
        accumulated = ""
        # Stream timeout uses the same value; long-utterance ASR can take a
        # while so we don't shrink it.
        async with httpx.AsyncClient(timeout=self.timeout) as client:
            async with client.stream(
                "POST",
                self._endpoint(),
                json=self._payload(b64, stream=True),
            ) as resp:
                resp.raise_for_status()
                async for raw in resp.aiter_lines():
                    if not raw:
                        continue
                    line = raw.strip()
                    if not line.startswith("data:"):
                        continue
                    payload = line[len("data:"):].strip()
                    if payload == "[DONE]":
                        break
                    try:
                        evt = json.loads(payload)
                    except json.JSONDecodeError:
                        continue
                    choices = evt.get("choices") or []
                    if not choices:
                        continue
                    delta = (choices[0].get("delta") or {})
                    token = delta.get("content")
                    if token:
                        accumulated += token
                        yield PartialTranscript(text=accumulated, is_final=False)
        # Terminal marker so the orchestrator can break on is_final.
        yield PartialTranscript(text=accumulated, is_final=True, confidence=1.0)
