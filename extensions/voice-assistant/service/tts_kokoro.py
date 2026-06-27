"""Kokoro MLX TTS Service — NDJSON /tts/stream adapter for voice-assistant.

Mirrors the moss-tts-nano HTTP contract:
  POST /tts/stream
    Body: {"text": "...", "voice": "..."}
    Response (NDJSON, one line per PCM chunk):
      {"seq": 0, "data": "<base64 int16 LE PCM>",
       "sample_rate": 24000, "channels": 1, "is_pause": false}
  POST /tts
    Body: same; returns WAV bytes via X-* headers
  GET  /health
  GET  /voices

Output is **24 kHz mono int16 LE** (vs moss-tts-nano's 48 kHz stereo).
voice-assistant's `_to_mono` + resampler handle the difference.

Notes
-----
* mlx-audio 0.4.4 has a SineGen broadcast bug; a local patch is required
  (see patches/mlx-audio-istftnet.patch). The patch is idempotent.
* Kokoro is not a streaming model — generate() yields one chunk per text
  segment (split by \\n+). Each utterance = one NDJSON line. First-byte
  latency ≈ 100-200ms.
"""
from __future__ import annotations

import argparse
import base64
import io
import json
import logging
import os
import sys
import time
import wave
from pathlib import Path
from typing import Optional

import numpy as np

logger = logging.getLogger("kokoro-tts")

from fastapi import FastAPI, HTTPException
from fastapi.responses import Response, StreamingResponse, JSONResponse
from pydantic import BaseModel

# ---------------------------------------------------------------------------
# Patch mlx-audio istftnet if not already patched (idempotent)
# ---------------------------------------------------------------------------
def _apply_istftnet_patch() -> None:
    """Apply local SineGen length-match guard if missing (issue #803, PR #788)."""
    try:
        import mlx_audio.tts.models.kokoro.istftnet as mod
    except ImportError:
        return
    src = Path(mod.__file__)
    text = src.read_text()
    marker = "Length-match guard"
    if marker in text:
        return
    needle = (
        "        # Generate UV signal\n"
        "        uv = self._f02uv(f0)\n"
        "\n"
        "        # Generate noise\n"
    )
    replacement = (
        "        # Generate UV signal\n"
        "        uv = self._f02uv(f0)\n"
        "\n"
        "        # Length-match guard: _f02sine's internal phase upsampling can yield a\n"
        "        # time dimension one hop longer than uv for certain f0 lengths, which\n"
        "        # makes the noise broadcast below fail with a [broadcast_shapes] error\n"
        "        # on a significant fraction of real inputs. Truncate both to the common\n"
        "        # length (a <=1-hop trim of the excitation signal, inaudible).\n"
        "        seq_len = min(sine_waves.shape[1], uv.shape[1])\n"
        "        sine_waves = sine_waves[:, :seq_len, :]\n"
        "        uv = uv[:, :seq_len, :]\n"
        "\n"
        "        # Generate noise\n"
    )
    if needle not in text:
        logger.warning("istftnet.py needle not found — skipping patch")
        return
    src.write_text(text.replace(needle, replacement))
    logger.info("Applied istftnet SineGen length-match patch")


_apply_istftnet_patch()

from mlx_audio.tts import load_model  # noqa: E402

# ---------------------------------------------------------------------------
# Globals
# ---------------------------------------------------------------------------
app = FastAPI(title="Kokoro MLX TTS Service")

MODEL: Optional[object] = None
SAMPLE_RATE: int = 24000
DEFAULT_VOICE = os.environ.get("KOKORO_VOICE", "zf_xiaoxiao")
LANG_CODE = os.environ.get("KOKORO_LANG_CODE", "zh")
MODEL_ID = os.environ.get("KOKORO_MODEL", "prince-canuma/Kokoro-82M")


# ---------------------------------------------------------------------------
# Request
# ---------------------------------------------------------------------------
class TTSRequest(BaseModel):
    text: str
    voice: Optional[str] = None
    # Accepted for moss-tts-nano contract compatibility; ignored.
    prompt_audio_path: Optional[str] = None
    sample_mode: str = "fixed"
    max_new_frames: int = 375
    voice_clone_max_text_tokens: int = 75
    seed: Optional[int] = None
    audio_temperature: float = 0.8
    audio_top_p: float = 0.95
    audio_top_k: int = 25
    audio_repetition_penalty: float = 1.2
    response_format: str = "wav"


# ---------------------------------------------------------------------------
# PCM helpers
# ---------------------------------------------------------------------------
def _wav_bytes(pcm_int16: bytes, sample_rate: int, channels: int = 1) -> bytes:
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm_int16)
    return buf.getvalue()


def _to_int16_le(audio: np.ndarray) -> bytes:
    """float32 [-1, 1] → int16 LE bytes (mono)."""
    pcm = np.clip(np.asarray(audio, dtype=np.float32).reshape(-1), -1.0, 1.0)
    return (pcm * 32767.0).astype("<i2").tobytes()


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------
@app.get("/health")
def health():
    return {"status": "ok" if MODEL is not None else "loading",
            "voice": DEFAULT_VOICE, "lang": LANG_CODE}


@app.get("/voices")
def list_voices():
    """Return Kokoro's available voice packs (*.safetensors in voices/)."""
    try:
        snapshot_dir = Path(MODEL._model_dir)  # type: ignore[attr-defined]
    except Exception:
        # Fallback: scan HF cache for the prince-canuma/Kokoro-82M repo.
        snapshot_dir = Path.home() / ".cache/huggingface/hub/models--prince-canuma--Kokoro-82M/snapshots"
        snaps = list(snapshot_dir.glob("*")) if snapshot_dir.exists() else []
        snapshot_dir = snaps[0] if snaps else None
    voices: list[str] = []
    if snapshot_dir and snapshot_dir.is_dir():
        voices_dir = snapshot_dir / "voices"
        if voices_dir.is_dir():
            voices = sorted(p.stem for p in voices_dir.glob("*.safetensors"))
    return {"voices": voices}


@app.post("/tts")
def tts(req: TTSRequest):
    if MODEL is None:
        raise HTTPException(503, "model not loaded")
    t0 = time.perf_counter()
    voice = req.voice or DEFAULT_VOICE
    pcm_chunks: list[bytes] = []
    try:
        for result in MODEL.generate(req.text, voice=voice, lang_code=LANG_CODE):
            audio = result.audio
            arr = np.asarray(audio)[0] if audio.ndim == 2 else np.asarray(audio)
            pcm_chunks.append(_to_int16_le(arr))
    except Exception as e:
        logger.exception("kokoro /tts failed")
        raise HTTPException(500, str(e))
    pcm = b"".join(pcm_chunks)
    elapsed = time.perf_counter() - t0
    audio_bytes = _wav_bytes(pcm, SAMPLE_RATE, 1)

    if req.response_format == "base64":
        return JSONResponse({
            "audio": base64.b64encode(audio_bytes).decode(),
            "sample_rate": SAMPLE_RATE,
            "elapsed_seconds": elapsed,
            "duration_seconds": len(pcm) // 2 / SAMPLE_RATE,
        })

    return Response(
        content=audio_bytes,
        media_type="audio/wav",
        headers={
            "X-Sample-Rate": str(SAMPLE_RATE),
            "X-Elapsed-Seconds": f"{elapsed:.4f}",
            "X-Duration-Seconds": f"{len(pcm) // 2 / SAMPLE_RATE:.4f}",
            "X-Channels": "1",
        },
    )


@app.post("/tts/stream")
def tts_stream(req: TTSRequest):
    """Stream PCM chunks as NDJSON. One line per Kokoro segment (≈ one utterance).

    Each line:
      {"seq": int, "data": "<base64 int16 LE>", "sample_rate": 24000,
       "channels": 1, "is_pause": false}
    """
    if MODEL is None:
        raise HTTPException(503, "model not loaded")

    text = req.text
    voice = req.voice or DEFAULT_VOICE

    def gen():
        seq = 0
        try:
            for result in MODEL.generate(text, voice=voice, lang_code=LANG_CODE):
                audio = result.audio
                arr = np.asarray(audio)[0] if audio.ndim == 2 else np.asarray(audio)
                pcm = _to_int16_le(arr)
                yield json.dumps({
                    "seq": seq,
                    "data": base64.b64encode(pcm).decode(),
                    "sample_rate": SAMPLE_RATE,
                    "channels": 1,
                    "is_pause": False,
                }, ensure_ascii=False) + "\n"
                seq += 1
        except Exception as exc:
            logger.exception("kokoro /tts/stream failed")
            yield json.dumps({"error": str(exc)}, ensure_ascii=False) + "\n"

    return StreamingResponse(gen(), media_type="application/x-ndjson")


# ---------------------------------------------------------------------------
# Lifecycle
# ---------------------------------------------------------------------------
@app.on_event("startup")
def _startup():
    global MODEL, SAMPLE_RATE
    logger.info("loading mlx-audio Kokoro: %s", MODEL_ID)
    t0 = time.perf_counter()
    MODEL = load_model(MODEL_ID)
    SAMPLE_RATE = int(MODEL.sample_rate)
    logger.info("model loaded in %.1fs, sample_rate=%d", time.perf_counter() - t0, SAMPLE_RATE)
    # Warm-up (first call always slower)
    try:
        for _ in MODEL.generate("预热", voice=DEFAULT_VOICE, lang_code=LANG_CODE):
            pass
        logger.info("warmup complete")
    except Exception as e:
        logger.warning("warmup failed (non-fatal): %s", e)


def main():
    parser = argparse.ArgumentParser(description="Kokoro MLX TTS Service")
    parser.add_argument("--host", default=os.environ.get("KOKORO_HOST", "127.0.0.1"))
    parser.add_argument("--port", type=int, default=int(os.environ.get("KOKORO_PORT", "9385")))
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
