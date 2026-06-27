"""Qwen3-ASR 0.6B 8-bit MLX HTTP Service.

Drop-in replacement for sensevoice-asr with the same `/asr` contract:
  POST /asr          Body: {audio_path | audio_base64, language, use_itn}
                      → {text, language, elapsed_seconds, duration_seconds, rtf}
  POST /asr/stream   Same body; emits NDJSON with a single final line
                      (Qwen3-ASR transcribe() is offline/full-utterance).
  GET  /health
  GET  /languages

Qwen3-ASR natively supports 99 languages incl. zh, en, ja, ko, yue.
First run downloads ~700MB to ~/.cache/huggingface/hub/.
"""
from __future__ import annotations

import argparse
import base64
import io
import logging
import os
import time
from pathlib import Path
from typing import Optional

import numpy as np
from pydantic import BaseModel
from fastapi import FastAPI, HTTPException
from fastapi.responses import JSONResponse, StreamingResponse

logger = logging.getLogger("qwen3-asr")

MODEL_REPO = os.environ.get("QWEN3_ASR_MODEL", "mlx-community/Qwen3-ASR-0.6B-8bit")
SAMPLE_RATE_TARGET = 16000
SUPPORTED_LANGS = ["auto", "zh", "en", "ja", "ko", "yue"]

app = FastAPI(title="Qwen3-ASR MLX Service")

# Globals populated at startup
MODEL = None  # mlx_qwen3_asr.Qwen3ASRModel
MODEL_CONFIG = None


class AsrRequest(BaseModel):
    audio_path: Optional[str] = None
    audio_base64: Optional[str] = None  # WAV bytes as base64
    language: str = "auto"  # auto | zh | en | ja | ko | yue
    use_itn: bool = True   # accepted for contract compat; Qwen3 does ITN by default


class ChunkRequest(AsrRequest):
    chunk_sec: float = 2.0  # ignored, kept for API symmetry


# ---------------------------------------------------------------------------
# Audio decode helpers (mirror sensevoice-asr)
# ---------------------------------------------------------------------------
def _read_wav_bytes(wav_bytes: bytes) -> tuple[np.ndarray, int]:
    """Decode WAV bytes via soundfile (handles PCM + IEEE float + mulaw)."""
    import soundfile as sf
    data, sr = sf.read(io.BytesIO(wav_bytes), dtype="float32", always_2d=False)
    if data.ndim > 1:
        data = data.mean(axis=1)
    return data.astype(np.float32), int(sr)


def _read_audio_file(path: str) -> tuple[np.ndarray, int]:
    """Read any audio file via soundfile → (float32 mono, sample_rate)."""
    import soundfile as sf
    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if data.ndim > 1:
        data = data.mean(axis=1)
    return data.astype(np.float32), int(sr)


def _resample_linear(data: np.ndarray, sr_in: int, sr_out: int = SAMPLE_RATE_TARGET) -> np.ndarray:
    if sr_in == sr_out:
        return data.astype(np.float32, copy=False)
    n_out = int(round(len(data) * sr_out / sr_in))
    idx = np.linspace(0, len(data) - 1, n_out)
    return np.interp(idx, np.arange(len(data)), data).astype(np.float32)


def _decode_audio(req) -> np.ndarray:
    if req.audio_path:
        data, sr = _read_audio_file(req.audio_path)
    elif req.audio_base64:
        data, sr = _read_wav_bytes(base64.b64decode(req.audio_base64))
    else:
        raise HTTPException(400, "must provide `audio_path` or `audio_base64`")
    return _resample_linear(data, sr, SAMPLE_RATE_TARGET)


# ---------------------------------------------------------------------------
# Transcription
# ---------------------------------------------------------------------------
def _transcribe(audio: np.ndarray, language: str) -> dict:
    """Run Qwen3-ASR on full utterance; return dict with text + timings."""
    from mlx_qwen3_asr import transcribe
    lang_arg = None if language == "auto" else language
    t0 = time.perf_counter()
    result = transcribe(audio, model=MODEL, language=lang_arg)
    elapsed = time.perf_counter() - t0
    dur = len(audio) / SAMPLE_RATE_TARGET
    return {
        "text": result.text.strip(),
        "language": result.language or language,
        "elapsed_seconds": elapsed,
        "duration_seconds": dur,
        "rtf": elapsed / dur if dur > 0 else 0.0,
    }


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------
@app.get("/health")
def health():
    return {"status": "ok" if MODEL is not None else "loading",
            "model": MODEL_REPO}


@app.get("/languages")
def languages():
    return {"languages": SUPPORTED_LANGS}


@app.post("/asr")
def asr(req: AsrRequest):
    if MODEL is None:
        raise HTTPException(503, "runtime not loaded")
    if req.language not in SUPPORTED_LANGS:
        raise HTTPException(400, f"unsupported language: {req.language}")
    try:
        audio = _decode_audio(req)
        if len(audio) == 0:
            raise HTTPException(400, "empty audio")
        r = _transcribe(audio, req.language)
        return JSONResponse(
            {
                "text": r["text"],
                "language": r["language"],
                "elapsed_seconds": r["elapsed_seconds"],
                "duration_seconds": r["duration_seconds"],
                "rtf": r["rtf"],
            },
            headers={
                "X-Elapsed-Seconds": f"{r['elapsed_seconds']:.4f}",
                "X-Duration-Seconds": f"{r['duration_seconds']:.4f}",
                "X-RTF": f"{r['rtf']:.4f}",
            },
        )
    except HTTPException:
        raise
    except Exception as e:
        logger.exception("asr failed")
        raise HTTPException(500, str(e))


@app.post("/asr/stream")
def asr_stream(req: ChunkRequest):
    """Pseudo-streaming NDJSON endpoint (parity with sensevoice-asr).

    Qwen3-ASR is offline, so we emit one final line:
      {"seq":0,"partial":""}
      {"seq":1,"final":true,"text":"...","elapsed_seconds":...}
    """
    if MODEL is None:
        raise HTTPException(503, "runtime not loaded")
    if req.language not in SUPPORTED_LANGS:
        raise HTTPException(400, f"unsupported language: {req.language}")

    import json

    def gen():
        try:
            audio = _decode_audio(req)
            if len(audio) == 0:
                yield json.dumps({"seq": 0, "error": "empty audio"}) + "\n"
                return
            yield json.dumps({"seq": 0, "partial": ""}) + "\n"
            r = _transcribe(audio, req.language)
            yield json.dumps({
                "seq": 1,
                "final": True,
                "text": r["text"],
                "language": r["language"],
                "elapsed_seconds": r["elapsed_seconds"],
                "duration_seconds": r["duration_seconds"],
                "rtf": r["rtf"],
            }) + "\n"
        except Exception as exc:
            logger.exception("asr/stream failed")
            yield json.dumps({"seq": 0, "error": str(exc)}) + "\n"

    return StreamingResponse(gen(), media_type="application/x-ndjson")


# ---------------------------------------------------------------------------
# Lifecycle
# ---------------------------------------------------------------------------
@app.on_event("startup")
def _startup():
    global MODEL, MODEL_CONFIG
    logger.info("loading Qwen3-ASR: %s", MODEL_REPO)
    t0 = time.perf_counter()
    from mlx_qwen3_asr import load_model
    MODEL, MODEL_CONFIG = load_model(MODEL_REPO)
    logger.info("model loaded in %.1fs", time.perf_counter() - t0)
    # Warm-up (first call compiles Metal kernels — ~3-5s)
    try:
        warm = np.zeros(SAMPLE_RATE_TARGET, dtype=np.float32)
        from mlx_qwen3_asr import transcribe
        transcribe(warm, model=MODEL, language="zh")
        logger.info("warmup complete")
    except Exception as e:
        logger.warning("warmup failed (non-fatal): %s", e)


def main():
    parser = argparse.ArgumentParser(description="Qwen3-ASR MLX Service")
    parser.add_argument("--host", default=os.environ.get("QWEN3_ASR_HOST", "127.0.0.1"))
    parser.add_argument("--port", type=int, default=int(os.environ.get("QWEN3_ASR_PORT", "9383")))
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
