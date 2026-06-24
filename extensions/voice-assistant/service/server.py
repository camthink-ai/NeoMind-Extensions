"""
Voice Assistant Orchestrator (PoC)
==================================

WebSocket server that owns the full voice pipeline:

    browser PCM → energy VAD → sensevoice-asr HTTP → echo reply →
    moss-tts-nano /tts/stream HTTP → PCM chunks back

The Rust extension (`voice-assistant`) is a thin proxy: it forwards browser
PCM to this service via WS, and pushes any PCM we send back to the browser.

Protocol (extension ↔ this service)
------------------------------------
Outgoing (extension → here), all over the same WS:

* Text frame ``{"type":"start","session_id":"...","sample_rate":16000,
  "channels":1,"format":"pcm_int16_le"}`` — sent once on session start.
* Text frame ``{"type":"ping"}`` — health probe from the extension's
  `check_health`.
* Binary frame — raw int16 LE PCM, 16kHz mono.

Incoming (here → extension):

* Binary frame — raw int16 LE PCM to play on the browser.
* Text frame ``{"type":"transcript","text":"..."}`` — final ASR result.
* Text frame ``{"type":"asr_start"|"asr_end"|"tts_start"|"tts_end",
  ...metrics}`` — phase markers for latency measurement.
* Text frame ``{"type":"stop"}`` — utterance pipeline finished.
* Text frame ``{"type":"error","message":"..."}`` — failure.

Latency measurements emitted per utterance:
    asr_ms, tts_first_chunk_ms, tts_total_ms, total_ms
"""
from __future__ import annotations

import argparse
import asyncio
import base64
import io
import json
import logging
import os
import time
import wave
from pathlib import Path
from typing import Optional

import numpy as np

logger = logging.getLogger("voice-assistant")

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
ASR_URL = os.environ.get("SENSEVOICE_ASR_URL", "http://127.0.0.1:9383")
# Default TTS backend is now CosyVoice 3 (port 9385). moss-tts-nano on
# port 9382 is kept as fallback — set VOICE_ASSISTANT_TTS_URL or
# MOSS_TTS_URL to override.
TTS_URL = (
    os.environ.get("VOICE_ASSISTANT_TTS_URL")
    or os.environ.get("MOSS_TTS_URL")
    or "http://127.0.0.1:9385"
)
TTS_VOICE = os.environ.get("VOICE_ASSISTANT_VOICE", "中文女")

# VAD tuning — energy-based, PoC only.
VAD_SILENCE_MS = int(os.environ.get("VOICE_ASSISTANT_VAD_SILENCE_MS", "500"))
VAD_MIN_SPEECH_MS = int(os.environ.get("VOICE_ASSISTANT_VAD_MIN_SPEECH_MS", "300"))
VAD_ENERGY_THRESHOLD = float(os.environ.get("VOICE_ASSISTANT_VAD_ENERGY", "0.015"))

# VAD backend selection.
#   VOICE_ASSISTANT_VAD_BACKEND=energy (default, PoC) — simple RMS threshold
#   VOICE_ASSISTANT_VAD_BACKEND=fsmn                  — FunASR FSMN neural VAD
#   VOICE_ASSISTANT_VAD_BACKEND=silero                — Silero VAD via sherpa-onnx
VAD_BACKEND = os.environ.get("VOICE_ASSISTANT_VAD_BACKEND", "energy").lower()
FSMN_VAD_MODEL_ID = os.environ.get(
    "FSMN_VAD_MODEL_ID",
    "iic/speech_fsmn_vad_zh-cn-16k-common-onnx",
)
FSMN_VAD_CACHE_DIR = os.environ.get("FSMN_VAD_CACHE_DIR", "/tmp/funasr_models")
# Dynamic silence schedule for FSMN VAD (accumulated_speech_ms → max_end_sil_ms).
# Tuned for voice-assistant turn-taking — more aggressive than FunASR's default
# (which starts at 2000ms, far too long for quick replies). Short utterances
# get tight cutoff (600ms ≈ the energy VAD default), longer ones get progressively
# more tolerance to avoid mid-sentence truncation.
FSMN_VAD_SCHEDULE = [
    (3000, 600),
    (8000, 800),
    (15000, 1000),
    (10 ** 9, 1500),
]

# Silero VAD via sherpa-onnx — preferred neural backend.
# License: MIT. ~1.7MB. <1ms/frame on CPU. Cross-platform (Mac + Linux ARM identical).
SILERO_VAD_MODEL_PATH = os.environ.get(
    "SILERO_VAD_MODEL_PATH",
    str(Path.home() / ".cache" / "sherpa-onnx" / "silero_vad.onnx"),
)
SILERO_VAD_THRESHOLD = float(os.environ.get("SILERO_VAD_THRESHOLD", "0.5"))
SILERO_VAD_MIN_SPEECH_MS = int(os.environ.get("SILERO_VAD_MIN_SPEECH_MS", "250"))
SILERO_VAD_SILENCE_MS = int(os.environ.get("SILERO_VAD_SILENCE_MS", "500"))

SAMPLE_RATE = 16000

# AEC echo window — interrupt-aware half-duplex protection.
# When TTS audio is being played to the browser, microphone input will
# leak some of that audio back. Without AEC, VAD picks up the leakage as
# "user speech" and triggers phantom transcripts (and worse, barge-in).
# Strategy: while TTS is active (and for a tail period after the last PCM
# chunk), raise the VAD silence/energy threshold so short TTS-leak bursts
# are ignored. Sustained real speech (a user actually interrupting) still
# triggers barge-in because it overruns the suppression window.
#
#   VOICE_ASSISTANT_AEC_MODE=off          disable suppression entirely
#   VOICE_ASSISTANT_AEC_MODE=echo_window  (default) enable this strategy
AEC_MODE = os.environ.get("VOICE_ASSISTANT_AEC_MODE", "echo_window").lower()
AEC_SILENCE_BOOST_MS = int(os.environ.get("VOICE_ASSISTANT_AEC_SILENCE_BOOST_MS", "800"))
AEC_ENERGY_BOOST = float(os.environ.get("VOICE_ASSISTANT_AEC_ENERGY_BOOST", "0.020"))
AEC_TAIL_MS = int(os.environ.get("VOICE_ASSISTANT_AEC_TAIL_MS", "400"))

# ---------------------------------------------------------------------------
# HTTP clients
# ---------------------------------------------------------------------------
import httpx


async def asr_transcribe(pcm_int16: bytes, language: str = "auto") -> dict:
    """POST /asr on sensevoice-asr service. Returns parsed JSON."""
    wav_bytes = _pcm_to_wav(pcm_int16, SAMPLE_RATE, channels=1)
    b64 = base64.b64encode(wav_bytes).decode()
    async with httpx.AsyncClient(timeout=30.0) as client:
        r = await client.post(
            f"{ASR_URL}/asr",
            json={"audio_base64": b64, "language": language, "use_itn": True},
        )
        r.raise_for_status()
        return r.json()


async def tts_stream(pcm_consumer, text: str, voice: str = TTS_VOICE,
                     max_new_frames: Optional[int] = None):
    """POST /tts/stream on moss-tts-nano service.

    For each NDJSON line carrying PCM, calls
    `await pcm_consumer(bytes, sample_rate, channels)`.

    `max_new_frames` caps the per-request generation budget (1 frame ≈ 80ms
    of audio). Pass a small value (e.g. 30 = 2.4s) for short sentences so
    TTS doesn't waste compute on trailing silence. None uses the runtime
    default (375 = 30s).

    Returns dict with first_chunk_ms / total_ms.
    """
    t0 = time.perf_counter()
    first_ms: Optional[float] = None
    n_chunks = 0
    total_bytes = 0
    sample_rate = 48000
    channels = 2
    body = {
        "text": text,
        "voice": voice,
        "sample_mode": "greedy",
        "response_format": "wav",
    }
    if max_new_frames is not None:
        body["max_new_frames"] = int(max_new_frames)
    async with httpx.AsyncClient(timeout=60.0) as client:
        async with client.stream(
            "POST",
            f"{TTS_URL}/tts/stream",
            json=body,
        ) as r:
            r.raise_for_status()
            async for line in r.aiter_lines():
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if "error" in obj:
                    raise RuntimeError(f"tts: {obj['error']}")
                if "data" not in obj:
                    continue
                if first_ms is None:
                    first_ms = (time.perf_counter() - t0) * 1000.0
                pcm = base64.b64decode(obj["data"])
                sample_rate = int(obj.get("sample_rate", sample_rate))
                channels = int(obj.get("channels", channels))
                await pcm_consumer(pcm, sample_rate, channels)
                n_chunks += 1
                total_bytes += len(pcm)
    total_ms = (time.perf_counter() - t0) * 1000.0
    return {
        "first_chunk_ms": first_ms if first_ms is not None else total_ms,
        "total_ms": total_ms,
        "n_chunks": n_chunks,
        "total_bytes": total_bytes,
        "sample_rate": sample_rate,
        "channels": channels,
    }


def _pcm_to_wav(pcm_int16: bytes, sample_rate: int, channels: int = 1) -> bytes:
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm_int16)
    return buf.getvalue()


def _tts_to_browser_pcm(pcm_int16: bytes, sr_in: int, ch_in: int) -> bytes:
    """Convert moss-tts PCM (int16 LE interleaved) → 16kHz mono int16 LE."""
    if len(pcm_int16) < ch_in * 2:
        return b""
    arr = np.frombuffer(pcm_int16, dtype=np.int16).astype(np.float32) / 32768.0
    if ch_in > 1:
        arr = arr.reshape(-1, ch_in).mean(axis=1)
    if sr_in != SAMPLE_RATE:
        n_out = int(round(len(arr) * SAMPLE_RATE / sr_in))
        if n_out < 1:
            return b""
        idx = np.linspace(0, len(arr) - 1, n_out)
        arr = np.interp(idx, np.arange(len(arr)), arr).astype(np.float32)
    return (np.clip(arr, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()


# ---------------------------------------------------------------------------
# Simulated streaming LLM (PoC stand-in for NeoMind Agent invoke_stream)
# ---------------------------------------------------------------------------
# Real implementation will replace fake_llm_stream() with:
#     async for token_chunk in agent.invoke_stream(text, ...):
#         ...
# and feed the same sentence-chunked fan-out below.
LLM_CHARS_PER_SEC = float(os.environ.get("VOICE_ASSISTANT_LLM_CPS", "30"))

# LLM backend selection.
#   VOICE_ASSISTANT_LLM=fake   (default) — deterministic char-pacing simulator
#   VOICE_ASSISTANT_LLM=ollama            — local Ollama HTTP /api/chat stream
LLM_BACKEND = os.environ.get("VOICE_ASSISTANT_LLM", "fake").lower()
OLLAMA_URL = os.environ.get("OLLAMA_URL", "http://127.0.0.1:11434")
OLLAMA_MODEL = os.environ.get("OLLAMA_MODEL", "qwen3.5:0.8b-mlx")
# Minimum sentence length before we yield it. Prevents per-word splitting
# for English (ollama streams token-by-token, often breaking on spaces).
LLM_MIN_SENTENCE_CHARS = int(os.environ.get("VOICE_ASSISTANT_LLM_MIN_SENTENCE", "6"))
# System prompt sent to the LLM. Override for language/style changes.
LLM_SYSTEM_PROMPT = os.environ.get(
    "VOICE_ASSISTANT_LLM_SYSTEM_PROMPT",
    "你是 NeoMind 语音助手，请用简短的口语化中文回答用户，每次回答不超过 3 句话。",
)
# Approximate audio seconds per character of synthesized speech. Chinese ≈
# 0.12-0.18s/char, English ≈ 0.05-0.08s/char. Conservative middle:
TTS_SECS_PER_CHAR_ZH = 0.15
TTS_SECS_PER_CHAR_EN = 0.06
# One moss-tts frame ≈ 80ms of audio (48kHz / 12.5Hz codec frame rate).
TTS_FRAME_SECS = 0.08

# LLM↔TTS parallel overlap.
# When enabled, the LLM sentence generator runs concurrently with TTS
# synthesis: while moss-tts is rendering sentence N, the LLM is already
# producing sentence N+1. This eliminates the ~500-700ms gap between
# consecutive TTS sentences (the time the LLM waits while TTS blocks
# the loop).
#
# TTS itself is still strictly serial — moss-tts has global state and
# cannot handle concurrent requests — but the LLM is free to race ahead
# and fill a small bounded queue so the next sentence is ready the
# instant the previous TTS stream finishes.
#
#   VOICE_ASSISTANT_PARALLEL_TTS=1  enable (default)
#   VOICE_ASSISTANT_PARALLEL_TTS=0  fall back to legacy serial loop
PARALLEL_TTS = os.environ.get("VOICE_ASSISTANT_PARALLEL_TTS", "1") not in ("0", "", "false", "False")
TTS_QUEUE_SIZE = int(os.environ.get("VOICE_ASSISTANT_TTS_QUEUE_SIZE", "4"))


def _build_fake_reply(user_text: str) -> str:
    """Construct a multi-sentence reply that exercises long-text streaming."""
    has_zh = any('\u4e00' <= c <= '\u9fff' for c in user_text)
    if has_zh:
        return (
            f"好的，我听到你说的是「{user_text}」。"
            f"让我帮你分析一下。"
            f"首先，这是一个语音助手的实时性测试。"
            f"我们正在验证边说边播的体验。"
            f"通过把回复按句切分，每说完一句就立刻合成音频，"
            f"用户可以更快听到回应。"
            f"你可以感受一下当前的流畅度。"
        )
    return (
        f"Sure, I heard you say '{user_text}'. "
        f"Let me analyze that for you. "
        f"First, this is a real-time voice assistant test. "
        f"We are validating the streaming experience. "
        f"By splitting the reply into sentences and synthesizing each one as soon as it's ready, "
        f"you can hear the response much sooner. "
        f"Feel free to test the fluency of the current setup."
    )


async def fake_llm_stream(user_text: str):
    """Async generator yielding reply sentences as if produced by a
    token-streaming LLM. Each char takes ``1/LLM_CHARS_PER_SEC`` seconds.

    Uses the same ``_split_streaming_buffer`` logic as the real LLM
    backends so sentence boundaries stay consistent across modes.
    Real LLM streaming plugs in here with the same yield contract.
    """
    reply = _build_fake_reply(user_text)
    delay = 1.0 / max(1.0, LLM_CHARS_PER_SEC)
    buf = ""
    for i, ch in enumerate(reply):
        await asyncio.sleep(delay)
        buf += ch
        # Drain any complete sentences currently in buf.
        consumed = 0
        for frag in _split_streaming_buffer(buf):
            yield frag
            consumed += len(frag)
        if consumed:
            buf = _consume_prefix(buf)
    # Flush trailing text without a terminator.
    for frag in _split_streaming_buffer(buf, force=True):
        yield frag


# Sentence splitting for streaming LLM output. Chinese and English need
# different strategies because Chinese characters carry more semantic
# weight per glyph (one char ≈ one syllable), while English needs whole
# words/phrases to form a coherent speech unit.
#
# Hard terminators (。？！；\n) always yield — these mark complete thoughts.
# Chinese soft breaks (，、) yield after ``LLM_MIN_SENTENCE_CHARS`` (default 6).
# English terminators (.!?) yield after ``ENGLISH_MIN_SENTENCE`` (default 20).
# English commas DO NOT yield by default — they produce choppy TTS output
# like "Sure," → "I" → "heard". Set VOICE_ASSISTANT_ENGLISH_USE_COMMAS=1
# to restore the legacy comma-splitting behavior.
_SENTENCE_TERMINATORS = set("。？！；\n")
_ZH_SOFT_BREAKS = set("，、")
_EN_HARD_TERMINATORS = set(".!?")
_EN_COMMAS = set(",")
_ENGLISH_MIN_SENTENCE = int(os.environ.get("VOICE_ASSISTANT_ENGLISH_MIN_SENTENCE", "20"))
_ENGLISH_USE_COMMAS = os.environ.get("VOICE_ASSISTANT_ENGLISH_USE_COMMAS", "0") not in ("0", "", "false", "False")


def _is_chinese(s: str) -> bool:
    """True if any CJK Unified Ideograph is present."""
    return any('\u4e00' <= c <= '\u9fff' for c in s)


def _split_streaming_buffer(buf: str, force: bool = False):
    """Yield complete sentences from a streaming buffer.

    A sentence is yielded when:
      * a hard terminator (。？！；\\n) is seen — yields immediately
        regardless of length; or
      * (Chinese text) a soft break (，、) is seen AND the accumulated
        fragment is at least ``LLM_MIN_SENTENCE_CHARS`` long; or
      * (English text) a terminator (.!?) is seen AND the accumulated
        fragment is at least ``ENGLISH_MIN_SENTENCE`` long; or
      * (English legacy mode, ``VOICE_ASSISTANT_ENGLISH_USE_COMMAS=1``)
        a comma (,) is seen AND the fragment is at least
        ``ENGLISH_MIN_SENTENCE`` long; or
      * ``force=True`` — flush whatever is left as a final sentence.

    Returns the unconsumed tail (caller is responsible for re-calling
    with the returned tail on next token arrival; or use
    ``_consume_prefix`` for the same effect).
    """
    zh = _is_chinese(buf)
    start = 0
    for i, ch in enumerate(buf):
        split = (ch in _SENTENCE_TERMINATORS) or \
                (zh and ch in _ZH_SOFT_BREAKS and (i - start) >= LLM_MIN_SENTENCE_CHARS) or \
                ((not zh) and ch in _EN_HARD_TERMINATORS and (i - start) >= _ENGLISH_MIN_SENTENCE) or \
                ((not zh) and _ENGLISH_USE_COMMAS and ch in _EN_COMMAS and (i - start) >= _ENGLISH_MIN_SENTENCE)
        if split:
            frag = buf[start:i + 1].strip()
            if frag:
                yield frag
            start = i + 1
    remaining = buf[start:]
    if force and remaining.strip():
        yield remaining.strip()
        remaining = ""
    return remaining


async def ollama_llm_stream(user_text: str):
    """Async generator yielding reply sentences from a local Ollama model.

    Streams tokens from Ollama's ``/api/chat`` (NDJSON, ``stream=true``),
    buffers them, and emits sentences using the same split logic as
    ``fake_llm_stream`` but with stricter English handling (soft breaks
    require ``LLM_MIN_SENTENCE_CHARS`` accumulation).

    Requires ``VOICE_ASSISTANT_LLM=ollama`` and Ollama running at
    ``OLLAMA_URL`` (default http://127.0.0.1:11434).
    """
    import httpx
    payload = {
        "model": OLLAMA_MODEL,
        "messages": [
            {"role": "system", "content": LLM_SYSTEM_PROMPT},
            {"role": "user", "content": user_text},
        ],
        "stream": True,
        "think": False,
        "options": {"num_predict": 120, "temperature": 0.6},
    }
    buf = ""
    try:
        async with httpx.AsyncClient(timeout=httpx.Timeout(30.0, connect=3.0)) as client:
            async with client.stream("POST", f"{OLLAMA_URL}/api/chat",
                                     json=payload,
                                     headers={"Content-Type": "application/json"}) as r:
                r.raise_for_status()
                async for line in r.aiter_lines():
                    if not line:
                        continue
                    try:
                        obj = json.loads(line)
                    except Exception:
                        continue
                    if obj.get("done"):
                        break
                    tok = obj.get("message", {}).get("content", "")
                    if not tok:
                        continue
                    buf += tok
                    # Drain all complete sentences currently in buf.
                    consumed = 0
                    for frag in _split_streaming_buffer(buf):
                        yield frag
                        consumed += len(frag)
                    if consumed:
                        # Remove the consumed prefix; the splitter consumed
                        # up to and including the last matched terminator.
                        # We need to find the actual byte offset. Simpler:
                        # rebuild buf by re-scanning.
                        # _split_streaming_buffer does not return offsets;
                        # recompute by re-splitting without force and
                        # measuring.
                        buf = _consume_prefix(buf)
    except httpx.HTTPError as e:
        # Yield an error sentence so the browser shows something.
        yield f"(LLM 错误: {e})"

    # Flush any trailing text without a terminator
    if buf.strip():
        yield buf.strip()


def _consume_prefix(buf: str) -> str:
    """Helper: run the splitter, return what's left after the last yielded
    sentence. Used by streaming LLM backends to trim the consumed prefix."""
    zh = _is_chinese(buf)
    start = 0
    for i, ch in enumerate(buf):
        split = (ch in _SENTENCE_TERMINATORS) or \
                (zh and ch in _ZH_SOFT_BREAKS and (i - start) >= LLM_MIN_SENTENCE_CHARS) or \
                ((not zh) and ch in _EN_HARD_TERMINATORS and (i - start) >= _ENGLISH_MIN_SENTENCE) or \
                ((not zh) and _ENGLISH_USE_COMMAS and ch in _EN_COMMAS and (i - start) >= _ENGLISH_MIN_SENTENCE)
        if split:
            start = i + 1
    return buf[start:]


async def llm_stream(user_text: str):
    """Backend selector. Yields sentences from either the fake simulator
    or a real Ollama model based on ``VOICE_ASSISTANT_LLM``."""
    if LLM_BACKEND == "ollama":
        async for s in ollama_llm_stream(user_text):
            yield s
    else:
        async for s in fake_llm_stream(user_text):
            yield s


def _estimate_max_frames(sentence: str, cap: int = 60) -> int:
    """Rough audio-length estimate → moss-tts frame count.

    Overshoot is harmless (just trailing silence); undershoot truncates.
    Keep it conservative.
    """
    has_zh = any('\u4e00' <= c <= '\u9fff' for c in sentence)
    per_char = TTS_SECS_PER_CHAR_ZH if has_zh else TTS_SECS_PER_CHAR_EN
    audio_secs = max(0.5, len(sentence) * per_char) + 1.0  # +1s slack
    return max(8, min(cap, int(round(audio_secs / TTS_FRAME_SECS))))


# ---------------------------------------------------------------------------
# FSMN-VAD singleton (loaded once at startup if backend=fsmn)
# ---------------------------------------------------------------------------
_FSMN_VAD_SINGLETON = None
if VAD_BACKEND == "fsmn":
    try:
        from funasr_onnx import Fsmn_vad_online

        # Resolve local cache path to avoid modelscope network check on every
        # startup. If not yet downloaded, fall back to modelscope ID (which
        # triggers snapshot_download internally).
        _fsmn_local_path = os.path.join(
            FSMN_VAD_CACHE_DIR, FSMN_VAD_MODEL_ID
        )
        _fsmn_model_arg = (
            _fsmn_local_path if os.path.isdir(_fsmn_local_path) else FSMN_VAD_MODEL_ID
        )
        _FSMN_VAD_SINGLETON = Fsmn_vad_online(
            model_dir=_fsmn_model_arg,
            cache_dir=FSMN_VAD_CACHE_DIR,
            quantize=True,
            max_end_sil=800,
        )
        logger.info("FSMN-VAD loaded: %s", _fsmn_model_arg)
    except Exception as e:
        logger.warning("FSMN-VAD load failed, falling back to energy: %s", e)
        _FSMN_VAD_SINGLETON = None
        VAD_BACKEND = "energy"


# ---------------------------------------------------------------------------
# Silero VAD config — loaded once at startup if backend=silero.
# sherpa-onnx VoiceActivityDetector is stateful (owns its ring buffer), so
# we cache the VadModelConfig here and each VoiceSession creates its own
# VoiceActivityDetector from it in __init__.
# ---------------------------------------------------------------------------
_SILERO_VAD_CONFIG = None
if VAD_BACKEND == "silero":
    try:
        import sherpa_onnx

        silero_path = Path(SILERO_VAD_MODEL_PATH)
        if not silero_path.is_file():
            silero_path.parent.mkdir(parents=True, exist_ok=True)
            import urllib.request
            url = "https://github.com/snakers4/silero-vad/raw/master/files/silero_vad.onnx"
            logger.info("Downloading Silero VAD model → %s", silero_path)
            urllib.request.urlretrieve(url, silero_path)

        _SILERO_VAD_CONFIG = sherpa_onnx.VadModelConfig()
        _SILERO_VAD_CONFIG.silero_vad.model = str(silero_path)
        _SILERO_VAD_CONFIG.silero_vad.threshold = SILERO_VAD_THRESHOLD
        _SILERO_VAD_CONFIG.silero_vad.min_silence_duration = SILERO_VAD_SILENCE_MS / 1000.0
        _SILERO_VAD_CONFIG.silero_vad.min_speech_duration = SILERO_VAD_MIN_SPEECH_MS / 1000.0
        _SILERO_VAD_CONFIG.sample_rate = SAMPLE_RATE
        _SILERO_VAD_CONFIG.provider = "cpu"
        if not _SILERO_VAD_CONFIG.validate():
            raise RuntimeError("Silero VAD config invalid")
        logger.info("Silero VAD config ready: %s", silero_path)
    except Exception as e:
        logger.warning("Silero VAD load failed, falling back to energy: %s", e)
        _SILERO_VAD_CONFIG = None
        VAD_BACKEND = "energy"


# ---------------------------------------------------------------------------
# Session state
# ---------------------------------------------------------------------------
class VoiceSession:
    """Per-connection state + VAD (Silero / FSMN / energy, picked by env + load state)."""

    def __init__(self, ws, session_id: str):
        self.ws = ws
        self.session_id = session_id
        # Telemetry
        self.bytes_in = 0
        self.bytes_out = 0
        # Barge-in: increments on each stop / explicit cancel; old in-flight
        # tasks check this and abort if it changed.
        self.epoch = 0
        # ---- Backend-specific VAD state ----
        self._fsmn_vad = _FSMN_VAD_SINGLETON  # None if energy backend or load failed
        # Energy VAD state
        self.in_speech = False
        self.speech_audio: list[np.ndarray] = []
        self.silence_frames = 0
        self.speech_frames = 0
        # FSMN VAD state
        self._fsmn_cache: list = []
        self._fsmn_pre_idx_ms: int = 0          # absolute timestamp accumulator
        self._fsmn_speech_started: bool = False  # FSMN emitted [start, -1]
        self._fsmn_collected: list[np.ndarray] = []  # audio accumulated during speech
        self._fsmn_lookback: list[np.ndarray] = []   # ring buffer for pre-speech audio
        self._fsmn_speech_ms: int = 0           # accumulated speech duration for dynamic scheduling
        # Silero VAD state
        self._silero_vad = None
        if VAD_BACKEND == "silero" and _SILERO_VAD_CONFIG is not None:
            try:
                import sherpa_onnx
                self._silero_vad = sherpa_onnx.VoiceActivityDetector(
                    _SILERO_VAD_CONFIG, buffer_size_in_seconds=30,
                )
            except Exception as e:
                logger.warning("per-session Silero VAD init failed: %s", e)
                self._silero_vad = None
        # ---- AEC echo-window state ----
        # TTS playback tracking — set True while TTS PCM is flowing to the
        # browser, with tts_last_chunk_ts marking the most recent PCM send.
        # _aec_active_now() returns True if we're within AEC_TAIL_MS of the
        # last chunk, which is the window where VAD should suppress.
        self.tts_active: bool = False
        self.tts_last_chunk_ts: float = 0.0

    def _aec_active_now(self) -> bool:
        """True if TTS echo suppression should currently be applied.

        Active while ``tts_active`` is True AND within ``AEC_TAIL_MS``
        of the last TTS PCM chunk. The tail lets suppression briefly
        outlive the final chunk so the mic doesn't immediately pick up
        reverb/late echo from the speaker.
        """
        if not self.tts_active:
            return False
        if AEC_MODE != "echo_window":
            return False
        elapsed_ms = (time.perf_counter() - self.tts_last_chunk_ts) * 1000.0
        return elapsed_ms < AEC_TAIL_MS

    async def send_json(self, obj: dict) -> None:
        try:
            await self.ws.send(json.dumps(obj, ensure_ascii=False))
        except Exception as e:
            logger.warning("send_json failed: %s", e)

    async def send_binary(self, data: bytes) -> None:
        try:
            await self.ws.send(data)
        except Exception as e:
            logger.warning("send_binary failed: %s", e)

    # ---- VAD ingestion ----

    def feed_pcm(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """Feed a chunk of int16 samples. Returns the complete utterance's
        PCM (int16 LE bytes) when VAD detects speech-end, else None.
        Dispatches to Silero / FSMN / energy backend based on env + load state.
        """
        if self._silero_vad is not None:
            return self._feed_pcm_silero(samples_int16)
        if self._fsmn_vad is not None:
            return self._feed_pcm_fsmn(samples_int16)
        return self._feed_pcm_energy(samples_int16)

    def _feed_pcm_energy(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """Energy-based VAD — original PoC path.

        Applies AEC echo-window suppression: while TTS is active, raise
        the effective energy threshold so short TTS-leak bursts don't
        register as speech start. A loud sustained voice (real user
        interrupting) still exceeds the boosted threshold.
        """
        # Normalize to float in [-1, 1] for RMS computation.
        f = samples_int16.astype(np.float32) / 32768.0
        # Split into 30ms frames for energy evaluation.
        frame_len = int(SAMPLE_RATE * 0.030)
        n_frames = len(f) // frame_len
        for i in range(n_frames):
            fr = f[i * frame_len:(i + 1) * frame_len]
            rms = float(np.sqrt(np.mean(fr * fr)))
            # AEC echo window: while TTS playback is active, demand a
            # higher RMS to flip into speech state.
            effective_threshold = VAD_ENERGY_THRESHOLD
            if self._aec_active_now() and not self.in_speech:
                effective_threshold = VAD_ENERGY_THRESHOLD + AEC_ENERGY_BOOST
            is_speech = rms > effective_threshold
            if is_speech:
                if not self.in_speech:
                    # Starting speech — only flip after enough frames accumulated.
                    self.speech_frames += 1
                    if self.speech_frames >= VAD_MIN_SPEECH_MS // 30:
                        self.in_speech = True
                        self.speech_audio = []
                        self.silence_frames = 0
                if self.in_speech:
                    self.speech_audio.append(fr)
                    self.silence_frames = 0
            else:
                if self.in_speech:
                    self.silence_frames += 1
                    # Keep accumulating silence audio too so the ASR gets
                    # the trailing 100ms for natural phrasing.
                    self.speech_audio.append(fr * 0.0)  # zero-filled
                    if self.silence_frames >= VAD_SILENCE_MS // 30:
                        # End of speech.
                        audio = np.concatenate(self.speech_audio) if self.speech_audio else np.zeros(0, dtype=np.float32)
                        pcm_bytes = (audio * 32767.0).astype("<i2").tobytes()
                        self._reset_vad()
                        return pcm_bytes
                else:
                    self.speech_frames = 0
        return None

    def _feed_pcm_fsmn(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """FSMN neural VAD. Uses streaming [start_ms, -1] / [-1, end_ms]
        protocol. Maintains a 500ms lookback ring so the first syllable
        (which arrived before VAD confirmed speech_start) is captured.
        Applies dynamic silence scheduling based on accumulated speech.
        """
        # int16 → float32 in [-1, 1]
        audio_f32 = samples_int16.astype(np.float32) / 32768.0
        chunk_ms = int(len(audio_f32) / SAMPLE_RATE * 1000)

        # Update dynamic silence threshold based on accumulated speech.
        for thresh_speech_ms, sil_ms in FSMN_VAD_SCHEDULE:
            if self._fsmn_speech_ms <= thresh_speech_ms:
                # AEC echo window: extend silence duration so TTS-leak
                # between syllables doesn't prematurely trigger end-of-speech.
                if self._aec_active_now():
                    sil_ms = sil_ms + AEC_SILENCE_BOOST_MS
                self._fsmn_vad.max_end_sil = sil_ms
                break

        # Run FSMN inference (mutates cache).
        param = {"is_final": False, "in_cache": self._fsmn_cache}
        try:
            segments = self._fsmn_vad(audio_f32, param_dict=param)
        except Exception as e:
            logger.warning("FSMN VAD inference failed: %s", e)
            self._fsmn_pre_idx_ms += chunk_ms
            return None
        self._fsmn_cache = param["in_cache"]
        self._fsmn_pre_idx_ms += chunk_ms

        # Maintain 500ms lookback ring (so we can recover pre-speech audio
        # when VAD confirms speech_start later).
        self._fsmn_lookback.append(audio_f32)
        lookback_samples = int(SAMPLE_RATE * 0.5)
        total = sum(len(x) for x in self._fsmn_lookback)
        while total > lookback_samples and len(self._fsmn_lookback) > 1:
            total -= len(self._fsmn_lookback[0])
            self._fsmn_lookback.pop(0)

        # Process emitted segments.
        for seg_list in segments:
            for start_ms, end_ms in seg_list:
                if start_ms != -1 and end_ms == -1:
                    # Speech start event.
                    if not self._fsmn_speech_started:
                        self._fsmn_speech_started = True
                        self._fsmn_collected = list(self._fsmn_lookback)  # recover lookback
                        self._fsmn_speech_ms = 0
                elif start_ms == -1 and end_ms != -1:
                    # Speech end event.
                    if self._fsmn_speech_started:
                        # Collect trailing audio too (everything up to now).
                        self._fsmn_collected.append(audio_f32)
                        return self._finalize_fsmn()
                elif start_ms != -1 and end_ms != -1:
                    # Single-shot [start, end] segment (rare in streaming).
                    if not self._fsmn_speech_started:
                        self._fsmn_speech_started = True
                        self._fsmn_collected = list(self._fsmn_lookback)
                    self._fsmn_collected.append(audio_f32)
                    return self._finalize_fsmn()

        # No boundary emitted this chunk — accumulate audio if in speech.
        if self._fsmn_speech_started:
            self._fsmn_collected.append(audio_f32)
            self._fsmn_speech_ms += chunk_ms

        return None

    def _finalize_fsmn(self) -> bytes:
        if not self._fsmn_collected:
            audio = np.zeros(0, dtype=np.float32)
        else:
            audio = np.concatenate(self._fsmn_collected)
        pcm_bytes = (audio * 32767.0).astype("<i2").tobytes()
        # Reset per-utterance FSMN state (cache persists across utterances).
        self._fsmn_speech_started = False
        self._fsmn_collected = []
        self._fsmn_speech_ms = 0
        return pcm_bytes

    def _feed_pcm_silero(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """Silero neural VAD via sherpa-onnx.

        Silero emits complete segments via accept_waveform + drain loop.
        Each SpeechSegment from .front contains the full PCM samples (float32
        in [-1, 1]) for one utterance. We convert to int16 LE bytes and hand
        to the ASR pipeline, exactly like the FSMN/energy paths.

        AEC echo window: while TTS playback is active, skip feeding the VAD
        entirely so TTS-leak doesn't get misclassified as user speech.
        """
        if self._silero_vad is None:
            return self._feed_pcm_energy(samples_int16)

        # AEC echo window: drop this chunk so the detector's internal buffer
        # doesn't accumulate speaker echo. Real user speech after the tail
        # resumes normal feeding.
        if self._aec_active_now():
            return None

        # int16 → float32 in [-1, 1]
        audio_f32 = (samples_int16.astype(np.float32) / 32768.0).tolist()
        self._silero_vad.accept_waveform(audio_f32)

        # Drain any completed segments.
        while not self._silero_vad.empty():
            segment = self._silero_vad.front
            self._silero_vad.pop()
            samples = np.asarray(segment.samples, dtype=np.float32)
            if samples.size == 0:
                continue
            pcm_bytes = (samples * 32767.0).astype("<i2").tobytes()
            return pcm_bytes
        return None

    def _reset_vad(self):
        """Reset only used by energy path."""
        self.in_speech = False
        self.speech_audio = []
        self.silence_frames = 0
        self.speech_frames = 0


# ---------------------------------------------------------------------------
# WebSocket handler
# ---------------------------------------------------------------------------
async def ws_handler(websocket):
    """Main per-connection loop. Parses incoming messages, drives VAD, runs
    the ASR→reply→TTS pipeline on each detected utterance."""
    import websockets  # for exception types

    session_id = "anon"
    path = getattr(websocket, "path", "") or ""
    if "?" in path:
        qs = path.split("?", 1)[1]
        for kv in qs.split("&"):
            if kv.startswith("session_id="):
                session_id = kv.split("=", 1)[1]

    sess = VoiceSession(websocket, session_id)
    logger.info("voice session connected: %s", session_id)

    # Only one pipeline at a time per session. If a new utterance arrives
    # while one is in-flight, we increment epoch (=barge-in) then start new.
    current_pipeline: Optional[asyncio.Task] = None

    try:
        async for message in websocket:
            if isinstance(message, bytes):
                sess.bytes_in += len(message)
                samples = np.frombuffer(message, dtype=np.int16)
                if samples.size == 0:
                    continue
                pcm_complete = sess.feed_pcm(samples)
                if pcm_complete is not None:
                    # Barge-in: cancel any running pipeline.
                    if current_pipeline is not None and not current_pipeline.done():
                        sess.epoch += 1
                        current_pipeline.cancel()
                        try:
                            await current_pipeline
                        except (asyncio.CancelledError, Exception):
                            pass
                        await sess.send_json({"type": "barge_in"})
                    current_pipeline = asyncio.create_task(
                        run_pipeline_for_segment(sess, pcm_complete)
                    )
            elif isinstance(message, str):
                try:
                    obj = json.loads(message)
                except json.JSONDecodeError:
                    continue
                mtype = obj.get("type")
                if mtype == "ping":
                    await sess.send_json({"type": "pong"})
                elif mtype == "start":
                    await sess.send_json({
                        "type": "ready",
                        "session_id": sess.session_id,
                        "asr_url": ASR_URL,
                        "tts_url": TTS_URL,
                        "voice": TTS_VOICE,
                        "vad_silence_ms": VAD_SILENCE_MS,
                        "vad_min_speech_ms": VAD_MIN_SPEECH_MS,
                        "vad_energy_threshold": VAD_ENERGY_THRESHOLD,
                    })
                elif mtype == "stop":
                    sess.epoch += 1
                    if current_pipeline is not None and not current_pipeline.done():
                        current_pipeline.cancel()
    except websockets.ConnectionClosed:
        pass
    except Exception as e:
        logger.exception("ws_handler crashed")
        try:
            await sess.send_json({"type": "error", "message": str(e)})
        except Exception:
            pass
    finally:
        logger.info(
            "voice session closed: %s (bytes_in=%d, bytes_out=%d)",
            session_id, sess.bytes_in, sess.bytes_out,
        )


# ---------------------------------------------------------------------------
# Pipeline
# ---------------------------------------------------------------------------
async def run_pipeline_for_segment(
    sess: VoiceSession,
    pcm_int16: bytes,
):
    """Run ASR → reply → TTS for one VAD-detected utterance."""
    my_epoch = sess.epoch
    t_total_start = time.perf_counter()

    # ASR phase
    await sess.send_json({"type": "asr_start", "bytes": len(pcm_int16)})
    t_asr_start = time.perf_counter()
    try:
        asr_resp = await asr_transcribe(pcm_int16)
    except Exception as e:
        await sess.send_json({"type": "error", "phase": "asr", "message": str(e)})
        return
    if sess.epoch != my_epoch:
        return  # barge-in
    asr_ms = (time.perf_counter() - t_asr_start) * 1000.0
    text = (asr_resp.get("text") or "").strip()
    await sess.send_json({
        "type": "transcript",
        "text": text,
        "language": asr_resp.get("language", "auto"),
        "elapsed_ms": asr_ms,
    })

    if not text:
        await sess.send_json({"type": "skip", "reason": "empty_transcript"})
        return

    # Reply phase: simulated LLM token-stream → sentence-chunked TTS fan-out.
    # Each LLM sentence is sent to TTS the instant it's ready, and the
    # browser playback queue stitches PCM chunks back-to-back. This gives
    # "first audio ≈ LLM first sentence time + TTS first-byte (~70ms)"
    # instead of "first audio ≈ LLM total reply time + TTS first-byte".
    t_reply_start = time.perf_counter()
    t_first_audio_to_browser: Optional[float] = None
    t_first_sentence_ready: Optional[float] = None
    total_tts_chunks = 0
    sentences_sent = 0
    first_tts_first_byte_ms: Optional[float] = None

    async def consume(pcm: bytes, sr: int, ch: int):
        nonlocal t_first_audio_to_browser, total_tts_chunks
        if sess.epoch != my_epoch:
            raise asyncio.CancelledError()
        out = _tts_to_browser_pcm(pcm, sr, ch)
        if not out:
            return
        if t_first_audio_to_browser is None:
            t_first_audio_to_browser = time.perf_counter()
        sess.bytes_out += len(out)
        await sess.send_binary(out)
        total_tts_chunks += 1
        # AEC echo window: mark TTS as actively playing so VAD suppresses
        # mic leakage. Refreshed on every PCM send; tail is applied in
        # VoiceSession._aec_active_now().
        sess.tts_active = True
        sess.tts_last_chunk_ts = time.perf_counter()

    await sess.send_json({
        "type": "tts_start",
        "text": "(streaming reply)",
        "reply_chars": -1,
        "mode": "sentence_streamed",
    })

    # Mark TTS active for the whole LLM-streaming phase; cleared in finally
    # below so suppression persists briefly past the last PCM chunk (tail).
    sess.tts_active = True
    sess.tts_last_chunk_ts = time.perf_counter()

    # Barge-in error sentinel — any of these sentinels short-circuits the
    # finally block into emitting the right terminal event once.
    pipeline_error: Optional[tuple] = None  # (phase, message, sentence_seq) or None
    pipeline_barge_in = False

    if PARALLEL_TTS:
        # Producer/consumer with a bounded queue. The LLM races ahead of
        # TTS, filling a small buffer so the next sentence is ready the
        # instant TTS finishes the previous one. TTS itself stays serial
        # (moss-tts has global state); only the LLM overlaps with it.
        q: asyncio.Queue = asyncio.Queue(maxsize=max(1, TTS_QUEUE_SIZE))

        async def producer():
            nonlocal pipeline_error
            try:
                async for sentence in llm_stream(text):
                    if sess.epoch != my_epoch:
                        return
                    await q.put(("sentence", sentence))
                await q.put(("eof", None))
            except asyncio.CancelledError:
                raise
            except Exception as e:
                pipeline_error = ("llm", str(e), sentences_sent)
                await q.put(("error", None))

        async def consumer():
            nonlocal sentences_sent, t_first_sentence_ready, first_tts_first_byte_ms
            nonlocal pipeline_error, pipeline_barge_in
            while True:
                if sess.epoch != my_epoch:
                    return
                try:
                    item = await asyncio.wait_for(q.get(), timeout=30.0)
                except asyncio.TimeoutError:
                    pipeline_error = ("tts", "LLM producer stalled >30s", sentences_sent)
                    return
                except asyncio.CancelledError:
                    raise
                kind, payload = item
                if kind == "eof":
                    return
                if kind == "error":
                    # pipeline_error already set by producer.
                    return
                sentence = payload
                sentences_sent += 1
                if t_first_sentence_ready is None:
                    t_first_sentence_ready = time.perf_counter()
                await sess.send_json({
                    "type": "reply_sentence",
                    "seq": sentences_sent,
                    "text": sentence,
                    "chars": len(sentence),
                })
                max_frames = _estimate_max_frames(sentence)
                try:
                    stats = await tts_stream(consume, sentence, voice=TTS_VOICE,
                                             max_new_frames=max_frames)
                except asyncio.CancelledError:
                    pipeline_barge_in = True
                    return
                except Exception as e:
                    pipeline_error = ("tts", str(e), sentences_sent)
                    return
                if first_tts_first_byte_ms is None:
                    first_tts_first_byte_ms = stats["first_chunk_ms"]

        try:
            await asyncio.gather(
                asyncio.create_task(producer()),
                asyncio.create_task(consumer()),
            )
        except asyncio.CancelledError:
            pipeline_barge_in = True
        finally:
            sess.tts_active = False
    else:
        # Legacy serial path: LLM → TTS, no overlap. Kept as a fallback
        # in case parallel mode surfaces unexpected issues.
        try:
            async for sentence in llm_stream(text):
                if sess.epoch != my_epoch:
                    return  # barge-in
                sentences_sent += 1
                if t_first_sentence_ready is None:
                    t_first_sentence_ready = time.perf_counter()
                await sess.send_json({
                    "type": "reply_sentence",
                    "seq": sentences_sent,
                    "text": sentence,
                    "chars": len(sentence),
                })
                # Cap per-sentence generation budget so we don't waste compute on
                # trailing silence for short replies.
                max_frames = _estimate_max_frames(sentence)
                try:
                    stats = await tts_stream(consume, sentence, voice=TTS_VOICE,
                                             max_new_frames=max_frames)
                except asyncio.CancelledError:
                    pipeline_barge_in = True
                    return
                except Exception as e:
                    pipeline_error = ("tts", str(e), sentences_sent)
                    return
                if first_tts_first_byte_ms is None:
                    first_tts_first_byte_ms = stats["first_chunk_ms"]
        finally:
            sess.tts_active = False

    # Emit the right terminal event for any early-exit cause. Normal end
    # (sentinel eof reached) falls through with no error/barge_in set.
    if pipeline_barge_in:
        await sess.send_json({"type": "barge_in", "phase": "tts"})
        return
    if pipeline_error is not None:
        phase, msg, seq = pipeline_error
        await sess.send_json({
            "type": "error",
            "phase": phase,
            "message": msg,
            "sentence_seq": seq,
        })
        return

    total_ms = (time.perf_counter() - t_total_start) * 1000.0
    first_audio_ms = (
        (t_first_audio_to_browser - t_reply_start) * 1000.0
        if t_first_audio_to_browser else -1.0
    )
    first_sentence_ms = (
        (t_first_sentence_ready - t_reply_start) * 1000.0
        if t_first_sentence_ready else -1.0
    )
    await sess.send_json({
        "type": "tts_end",
        "asr_ms": asr_ms,
        "llm_first_sentence_ms": first_sentence_ms,
        "tts_first_chunk_ms": first_tts_first_byte_ms if first_tts_first_byte_ms is not None else -1,
        "first_audio_to_browser_ms": first_audio_ms,
        "total_tts_chunks": total_tts_chunks,
        "sentences_sent": sentences_sent,
        "total_ms": total_ms,
        "bytes": len(pcm_int16),
    })
    await sess.send_json({"type": "stop"})


# ---------------------------------------------------------------------------
# Main entry
# ---------------------------------------------------------------------------
async def _main(host: str, port: int):
    import websockets

    logger.info(
        "voice orchestrator starting: ws://%s:%d/ws  (ASR=%s, TTS=%s, voice=%s, "
        "VAD silence=%dms, threshold=%.3f, LLM=%s%s)",
        host, port, ASR_URL, TTS_URL, TTS_VOICE,
        VAD_SILENCE_MS, VAD_ENERGY_THRESHOLD,
        LLM_BACKEND,
        f" model={OLLAMA_MODEL}" if LLM_BACKEND == "ollama" else "",
    )

    async with websockets.serve(
        ws_handler,
        host,
        port,
        max_size=None,
        ping_interval=20,
        ping_timeout=60,
    ):
        await asyncio.Future()  # run forever


def main():
    parser = argparse.ArgumentParser(description="Voice Assistant orchestrator (PoC)")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9384)
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    try:
        asyncio.run(_main(args.host, args.port))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
