"""
Voice Assistant Orchestrator
============================

WebSocket server that drives the full voice pipeline via Profile-backed
backends and the VoicePipeline orchestrator:

    browser PCM → Silero/FSMN/energy VAD → ASR → LLM → TTS → PCM back

Backend selection is driven by ``profiles/<name>.yaml`` (default: default.yaml).
Env overrides (VOICE_ASSISTANT_PROFILE, VOICE_ASSISTANT_VAD_BACKEND,
VOICE_ASSISTANT_TTS_URL, VOICE_ASSISTANT_VOICE) are applied in profile.py.

The Rust extension (``voice-assistant``) is a thin WS proxy: it forwards
browser PCM to this service and pushes any PCM we send back to the browser.

Protocol (extension <-> this service)
-------------------------------------
Outgoing (extension -> here):

* ``{"type":"start",...}`` — session start (once).
* ``{"type":"ping"}`` — health probe.
* ``{"type":"stop"}`` — client-initiated stop (barge-in).
* Binary frame — raw int16 LE PCM, 16kHz mono.

Incoming (here -> extension):

* ``{"type":"asr_start","bytes":N}`` — before ASR.
* ``{"type":"transcript","text":"...","language":"auto","elapsed_ms":N}``
* ``{"type":"skip","reason":"empty_transcript"}`` — empty ASR result.
* ``{"type":"tts_start",...}`` — before TTS.
* ``{"type":"tts_end",...metrics}`` — after TTS.
* ``{"type":"stop"}`` — turn finished.
* ``{"type":"barge_in"}`` — interrupt (user speech during TTS).
* ``{"type":"error","phase":"...","message":"..."}``
* ``{"type":"pong"}`` / ``{"type":"ready",...}`` — handshake.
* Binary frame — PCM to play.
"""
from __future__ import annotations

import argparse
import asyncio
import json
import logging
import os
import time
from pathlib import Path
from typing import Optional

import numpy as np
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.responses import JSONResponse

logger = logging.getLogger("voice-assistant")

# ---------------------------------------------------------------------------
# Profile + backend construction at startup
# ---------------------------------------------------------------------------
from profile import load_profile
from backends import make_vad, make_asr, make_llm, make_tts
from orchestrator import VoicePipeline, State
from telemetry import Telemetry

_profile = load_profile(os.environ.get("VOICE_ASSISTANT_PROFILE"))
# VAD/ASR/TTS backends are stateless or thread-safe across sessions — share them.
# LLM backend is built PER-SESSION inside ws_handler because NeoMindWSClient
# stores live WS connection state as instance attributes (_active_ws,
# _llm_completed); sharing would cross-corrupt concurrent sessions.
_vad_backend = make_vad(_profile)
_asr_backend = make_asr(_profile)
_tts_backend = make_tts(_profile)
_telemetry = Telemetry()

# Backward-compat globals — exposed at module level so VoiceSession (which
# still reads these directly) and the WS ``ready`` frame can reference them.
ASR_URL = _profile.asr_config.get("url", "http://127.0.0.1:9383")
TTS_URL = _profile.tts_config.get("url", "http://127.0.0.1:9386")
TTS_VOICE = _profile.tts_config.get("voice", "中文女")
VAD_BACKEND = _profile.vad_backend_type

# ---------------------------------------------------------------------------
# VAD tuning — read from env (VoiceSession still reads these directly).
# Refactoring VAD config into Profile is out of scope for this task.
# ---------------------------------------------------------------------------
VAD_SILENCE_MS = int(os.environ.get("VOICE_ASSISTANT_VAD_SILENCE_MS", "500"))
VAD_MIN_SPEECH_MS = int(os.environ.get("VOICE_ASSISTANT_VAD_MIN_SPEECH_MS", "300"))
VAD_ENERGY_THRESHOLD = float(os.environ.get("VOICE_ASSISTANT_VAD_ENERGY", "0.015"))

FSMN_VAD_MODEL_ID = os.environ.get(
    "FSMN_VAD_MODEL_ID",
    "iic/speech_fsmn_vad_zh-cn-16k-common-onnx",
)
FSMN_VAD_CACHE_DIR = os.environ.get("FSMN_VAD_CACHE_DIR", "/tmp/funasr_models")
# Dynamic silence schedule for FSMN VAD (accumulated_speech_ms -> max_end_sil_ms).
FSMN_VAD_SCHEDULE = [
    (3000, 600),
    (8000, 800),
    (15000, 1000),
    (10 ** 9, 1500),
]

SILERO_VAD_MODEL_PATH = os.environ.get(
    "SILERO_VAD_MODEL_PATH",
    str(Path.home() / ".cache" / "sherpa-onnx" / "silero_vad.onnx"),
)
SILERO_VAD_THRESHOLD = float(os.environ.get("SILERO_VAD_THRESHOLD", "0.5"))
SILERO_VAD_MIN_SPEECH_MS = int(os.environ.get("SILERO_VAD_MIN_SPEECH_MS", "250"))
SILERO_VAD_SILENCE_MS = int(os.environ.get("SILERO_VAD_SILENCE_MS", "500"))

SAMPLE_RATE = 16000

# AEC echo window — interrupt-aware half-duplex protection.
AEC_MODE = os.environ.get("VOICE_ASSISTANT_AEC_MODE", "echo_window").lower()
AEC_SILENCE_BOOST_MS = int(os.environ.get("VOICE_ASSISTANT_AEC_SILENCE_BOOST_MS", "800"))
AEC_ENERGY_BOOST = float(os.environ.get("VOICE_ASSISTANT_AEC_ENERGY_BOOST", "0.020"))
AEC_TAIL_MS = int(os.environ.get("VOICE_ASSISTANT_AEC_TAIL_MS", "400"))


# ---------------------------------------------------------------------------
# FastAPI app — serves HTTP /measure and WebSocket /ws on the same port.
# ---------------------------------------------------------------------------
app = FastAPI(title="Voice Assistant Orchestrator")


@app.post("/measure")
async def measure(req: dict | None = None):
    """Return aggregated latency stats from telemetry.

    Body is ignored (the ``n`` field in the plan is informational; we report
    all accumulated observations). Response shape::

        {
          "turn_count": int,
          "barge_in_count": int,
          "target_ms": int,
          "target_met": bool,
          "<kpi_name>": {"p50": float, "p95": float, "min": float, "max": float},
          ...
        }
    """
    snap = _telemetry.snapshot()
    first_audio_p50 = snap.get("first_audio_out_ms", {}).get("p50")
    target_met = (
        first_audio_p50 is not None
        and first_audio_p50 <= _profile.latency_target_ms
    )
    return {
        "turn_count": _telemetry.turn_count,
        "barge_in_count": _telemetry.barge_in_count,
        "target_ms": _profile.latency_target_ms,
        "target_met": target_met,
        **snap,
    }


# ---------------------------------------------------------------------------
# PCM resampler for browser playback
# ---------------------------------------------------------------------------
def _tts_to_browser_pcm(pcm_int16: bytes, sr_in: int, ch_in: int) -> bytes:
    """Convert TTS PCM (int16 LE interleaved) -> 16kHz mono int16 LE."""
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
# FSMN-VAD singleton (loaded once at startup if backend=fsmn)
# ---------------------------------------------------------------------------
_FSMN_VAD_SINGLETON = None
if VAD_BACKEND == "fsmn":
    try:
        from funasr_onnx import Fsmn_vad_online

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
# Silero VAD config singleton (loaded once at startup if backend=silero)
# ---------------------------------------------------------------------------
from backends.vad import _ensure_silero_config as _load_silero_config

_SILERO_VAD_CONFIG = None
if VAD_BACKEND == "silero":
    _SILERO_VAD_CONFIG = _load_silero_config()
    if _SILERO_VAD_CONFIG is None:
        VAD_BACKEND = "energy"


# ---------------------------------------------------------------------------
# Session state
# ---------------------------------------------------------------------------
class VoiceSession:
    """Per-connection state + VAD (Silero / FSMN / energy)."""

    def __init__(self, ws, session_id: str):
        self.ws = ws
        self.session_id = session_id
        # Telemetry
        self.bytes_in = 0
        self.bytes_out = 0
        # ---- Backend-specific VAD state ----
        self._fsmn_vad = _FSMN_VAD_SINGLETON
        # Energy VAD state
        self.in_speech = False
        self.speech_audio: list[np.ndarray] = []
        self.silence_frames = 0
        self.speech_frames = 0
        # FSMN VAD state
        self._fsmn_cache: list = []
        self._fsmn_pre_idx_ms: int = 0
        self._fsmn_speech_started: bool = False
        self._fsmn_collected: list[np.ndarray] = []
        self._fsmn_lookback: list[np.ndarray] = []
        self._fsmn_speech_ms: int = 0
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
        self.tts_active: bool = False
        self.tts_last_chunk_ts: float = 0.0

    def _aec_active_now(self) -> bool:
        """True if TTS echo suppression should currently be applied."""
        if not self.tts_active:
            return False
        if AEC_MODE != "echo_window":
            return False
        elapsed_ms = (time.perf_counter() - self.tts_last_chunk_ts) * 1000.0
        return elapsed_ms < AEC_TAIL_MS

    async def send_json(self, obj: dict) -> None:
        try:
            await self.ws.send_text(json.dumps(obj, ensure_ascii=False))
        except Exception as e:
            logger.warning("send_json failed: %s", e)

    async def send_binary(self, data: bytes) -> None:
        try:
            await self.ws.send_bytes(data)
        except Exception as e:
            logger.warning("send_binary failed: %s", e)

    # ---- VAD ingestion ----

    def feed_pcm(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """Feed int16 samples. Returns complete utterance PCM (int16 LE bytes)
        when VAD detects speech-end, else None."""
        if self._silero_vad is not None:
            return self._feed_pcm_silero(samples_int16)
        if self._fsmn_vad is not None:
            return self._feed_pcm_fsmn(samples_int16)
        return self._feed_pcm_energy(samples_int16)

    def _feed_pcm_energy(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """Energy-based VAD with AEC echo-window suppression."""
        f = samples_int16.astype(np.float32) / 32768.0
        frame_len = int(SAMPLE_RATE * 0.030)
        n_frames = len(f) // frame_len
        for i in range(n_frames):
            fr = f[i * frame_len:(i + 1) * frame_len]
            rms = float(np.sqrt(np.mean(fr * fr)))
            effective_threshold = VAD_ENERGY_THRESHOLD
            if self._aec_active_now() and not self.in_speech:
                effective_threshold = VAD_ENERGY_THRESHOLD + AEC_ENERGY_BOOST
            is_speech = rms > effective_threshold
            if is_speech:
                if not self.in_speech:
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
                    self.speech_audio.append(fr * 0.0)
                    if self.silence_frames >= VAD_SILENCE_MS // 30:
                        audio = np.concatenate(self.speech_audio) if self.speech_audio else np.zeros(0, dtype=np.float32)
                        pcm_bytes = (audio * 32767.0).astype("<i2").tobytes()
                        self._reset_vad()
                        return pcm_bytes
                else:
                    self.speech_frames = 0
        return None

    def _feed_pcm_fsmn(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """FSMN neural VAD with streaming protocol and AEC suppression."""
        audio_f32 = samples_int16.astype(np.float32) / 32768.0
        chunk_ms = int(len(audio_f32) / SAMPLE_RATE * 1000)

        for thresh_speech_ms, sil_ms in FSMN_VAD_SCHEDULE:
            if self._fsmn_speech_ms <= thresh_speech_ms:
                if self._aec_active_now():
                    sil_ms = sil_ms + AEC_SILENCE_BOOST_MS
                self._fsmn_vad.max_end_sil = sil_ms
                break

        param = {"is_final": False, "in_cache": self._fsmn_cache}
        try:
            segments = self._fsmn_vad(audio_f32, param_dict=param)
        except Exception as e:
            logger.warning("FSMN VAD inference failed: %s", e)
            self._fsmn_pre_idx_ms += chunk_ms
            return None
        self._fsmn_cache = param["in_cache"]
        self._fsmn_pre_idx_ms += chunk_ms

        self._fsmn_lookback.append(audio_f32)
        lookback_samples = int(SAMPLE_RATE * 0.5)
        total = sum(len(x) for x in self._fsmn_lookback)
        while total > lookback_samples and len(self._fsmn_lookback) > 1:
            total -= len(self._fsmn_lookback[0])
            self._fsmn_lookback.pop(0)

        for seg_list in segments:
            for start_ms, end_ms in seg_list:
                if start_ms != -1 and end_ms == -1:
                    if not self._fsmn_speech_started:
                        self._fsmn_speech_started = True
                        self._fsmn_collected = list(self._fsmn_lookback)
                        self._fsmn_speech_ms = 0
                elif start_ms == -1 and end_ms != -1:
                    if self._fsmn_speech_started:
                        self._fsmn_collected.append(audio_f32)
                        return self._finalize_fsmn()
                elif start_ms != -1 and end_ms != -1:
                    if not self._fsmn_speech_started:
                        self._fsmn_speech_started = True
                        self._fsmn_collected = list(self._fsmn_lookback)
                    self._fsmn_collected.append(audio_f32)
                    return self._finalize_fsmn()

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
        self._fsmn_speech_started = False
        self._fsmn_collected = []
        self._fsmn_speech_ms = 0
        return pcm_bytes

    def _feed_pcm_silero(self, samples_int16: np.ndarray) -> Optional[np.ndarray]:
        """Silero neural VAD via sherpa-onnx with AEC echo-window suppression."""
        if self._silero_vad is None:
            return self._feed_pcm_energy(samples_int16)

        if self._aec_active_now():
            return None

        audio_f32 = (samples_int16.astype(np.float32) / 32768.0).tolist()
        self._silero_vad.accept_waveform(audio_f32)

        while not self._silero_vad.empty():
            segment = self._silero_vad.front
            samples = np.asarray(segment.samples, dtype=np.float32)
            self._silero_vad.pop()
            if samples.size == 0:
                continue
            pcm_bytes = (samples * 32767.0).astype("<i2").tobytes()
            return pcm_bytes
        return None

    def _reset_vad(self):
        """Reset energy VAD state."""
        self.in_speech = False
        self.speech_audio = []
        self.silence_frames = 0
        self.speech_frames = 0


# ---------------------------------------------------------------------------
# Turn execution
# ---------------------------------------------------------------------------
async def run_pipeline_for_segment(
    sess: VoiceSession,
    pipeline: VoicePipeline,
    pcm_int16: bytes,
) -> None:
    """Run one voice turn via the Profile-backed VoicePipeline."""
    from contracts import VadSegment

    samples_f32 = (
        np.frombuffer(pcm_int16, dtype=np.int16).astype(np.float32) / 32768.0
    )
    segment = VadSegment(
        samples=samples_f32.tolist(),
        sample_rate=SAMPLE_RATE,
        start_ms=0,
        end_ms=0,
    )
    try:
        await pipeline.run_turn(segment)
    except Exception as e:
        logger.exception("run_turn failed")
        await sess.send_json(
            {"type": "error", "phase": "pipeline", "message": str(e)}
        )
        return

    # Terminal event: stop marks turn complete (unless barge-in already
    # sent a terminal event). If state is LISTENING, the barge_in handler
    # already emitted the barge_in frame.
    if pipeline.fsm.state == State.IDLE:
        await sess.send_json({"type": "stop"})


# ---------------------------------------------------------------------------
# WebSocket handler
# ---------------------------------------------------------------------------
@app.websocket("/ws")
async def ws_handler(websocket: WebSocket):
    """Main per-connection loop: parse messages, drive VAD, delegate each
    turn to VoicePipeline.run_turn()."""
    await websocket.accept()

    # session_id is passed as a query parameter (?session_id=xxx) — the
    # browser extension's WS URL already includes this.
    session_id = websocket.query_params.get("session_id", "anon")

    sess = VoiceSession(websocket, session_id)
    logger.info("voice session connected: %s", session_id)

    # -- Per-session callbacks (closures over sess) --
    async def on_tts_pcm(pcm_bytes: bytes, sample_rate: int) -> None:
        out = _tts_to_browser_pcm(pcm_bytes, sample_rate, 1)
        if not out:
            return
        sess.bytes_out += len(out)
        await sess.send_binary(out)
        sess.tts_active = True
        sess.tts_last_chunk_ts = time.perf_counter()

    async def on_stop_playback() -> None:
        sess.tts_active = False
        await sess.send_json({"type": "barge_in"})

    async def on_asr_start(byte_count: int) -> None:
        await sess.send_json({"type": "asr_start", "bytes": byte_count})

    async def on_asr_complete(transcript: str, elapsed_ms: float) -> None:
        await sess.send_json({
            "type": "transcript",
            "text": transcript,
            "language": "auto",
            "elapsed_ms": elapsed_ms,
        })

    async def on_skip(reason: str) -> None:
        await sess.send_json({"type": "skip", "reason": reason})

    async def on_tts_start() -> None:
        await sess.send_json({
            "type": "tts_start",
            "text": "(voice reply)",
            "mode": "full_synthesize",
        })

    async def on_tts_end(metrics: dict) -> None:
        await sess.send_json({"type": "tts_end", **metrics})
        sess.tts_active = False  # AEC tail still applies via _aec_active_now

    async def on_error(phase: str, message: str) -> None:
        await sess.send_json(
            {"type": "error", "phase": phase, "message": message}
        )

    # Per-session LLM backend — NeoMindWSClient holds live WS state as instance
    # attributes; a single shared instance would cross-corrupt concurrent
    # sessions. Other backends are stateless and shared.
    llm_backend = make_llm(_profile)

    pipeline = VoicePipeline(
        _vad_backend,
        _asr_backend,
        llm_backend,
        _tts_backend,
        on_tts_pcm=on_tts_pcm,
        on_stop_playback=on_stop_playback,
        on_asr_start=on_asr_start,
        on_asr_complete=on_asr_complete,
        on_skip=on_skip,
        on_tts_start=on_tts_start,
        on_tts_end=on_tts_end,
        on_error=on_error,
        telemetry=_telemetry,
        voice=TTS_VOICE,
    )

    current_pipeline_task: Optional[asyncio.Task] = None

    try:
        async for message in websocket:
            if isinstance(message, bytes):
                sess.bytes_in += len(message)
                samples = np.frombuffer(message, dtype=np.int16)
                if samples.size == 0:
                    continue
                pcm_complete = sess.feed_pcm(samples)
                if pcm_complete is None:
                    continue
                # Barge-in: if a turn is in progress, fire handle_barge_in
                # before starting a new turn.
                if pipeline.fsm.state in (State.THINKING, State.SPEAKING):
                    await pipeline.barge_in.handle_barge_in(
                        pipeline.fsm, reason="user_speech"
                    )
                    if (
                        current_pipeline_task is not None
                        and not current_pipeline_task.done()
                    ):
                        current_pipeline_task.cancel()
                        try:
                            await current_pipeline_task
                        except (asyncio.CancelledError, Exception):
                            pass
                # Start the new turn
                current_pipeline_task = asyncio.create_task(
                    run_pipeline_for_segment(sess, pipeline, pcm_complete)
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
                    await pipeline.barge_in.handle_barge_in(
                        pipeline.fsm, reason="client_stop"
                    )
                    if (
                        current_pipeline_task is not None
                        and not current_pipeline_task.done()
                    ):
                        current_pipeline_task.cancel()
    except WebSocketDisconnect:
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
# Main entry
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(description="Voice Assistant orchestrator")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9384)
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    logger.info(
        "voice orchestrator starting: http://%s:%d  "
        "(profile=%s, VAD=%s, ASR=%s, LLM=%s, TTS=%s, voice=%s)",
        args.host, args.port,
        _profile.name,
        VAD_BACKEND,
        ASR_URL,
        _profile.llm_config.get("type", "unknown"),
        TTS_URL,
        TTS_VOICE,
    )

    import uvicorn
    uvicorn.run(
        app,
        host=args.host,
        port=args.port,
        log_level="info",
        # Keep WebSocket pings alive (matches old websockets.serve defaults).
        ws_ping_interval=20,
        ws_ping_timeout=60,
        # No max message size cap — mic PCM frames can be large.
        ws_max_size=None,
    )


if __name__ == "__main__":
    main()
