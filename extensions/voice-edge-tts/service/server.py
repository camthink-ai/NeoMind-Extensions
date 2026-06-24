"""voice-edge-tts HTTP Service (NDJSON adapter for sherpa-onnx ZipVoice).

Thin FastAPI wrapper around `sherpa_onnx.OfflineTts` running the ZipVoice
distill int8 zh-en-emilia model. Exposes the **same `/tts/stream` NDJSON
contract** as moss-tts-nano / cosyvoice-3, so voice-assistant can switch
backends by changing one env var.

Endpoints (filled in across tasks A4-A5):
* POST /tts         — full synthesis, returns WAV bytes (with X-* headers).
* POST /tts/stream  — streaming synthesis, NDJSON: one line per PCM chunk.
* GET  /voices      — list registered voice IDs.
* GET  /health      — liveness probe.

Output is 24 kHz mono int16 LE PCM. voice-assistant's _tts_to_browser_pcm
already resamples arbitrary sample_rate / channels.
"""
from __future__ import annotations

import argparse
import base64
import io
import logging
import os
import wave
from pathlib import Path
from typing import Optional

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

logger = logging.getLogger("voice-edge-tts")

# ---------------------------------------------------------------------------
# Globals
# ---------------------------------------------------------------------------
app = FastAPI(title="Voice Edge TTS Service")

# Lazy-loaded on startup (Task A4).
tts = None  # sherpa_onnx.OfflineTts
model_sample_rate: int = 24000  # ZipVoice outputs 24kHz mono
available_voices: list[str] = []

# ---------------------------------------------------------------------------
# Model constants
# ---------------------------------------------------------------------------
MODEL_NAME = "sherpa-onnx-zipvoice-distill-int8-zh-en-emilia"
VOCODER_NAME = "vocos_24khz.onnx"


def _model_dir() -> str:
    """Resolve and ensure the ZipVoice model dir exists; auto-download if missing."""
    base = os.environ.get(
        "VOICE_EDGE_TTS_MODEL_DIR",
        str(Path.home() / ".cache" / "sherpa-onnx"),
    )
    d = Path(base) / MODEL_NAME
    if not (d / "encoder.int8.onnx").exists():
        d.mkdir(parents=True, exist_ok=True)
        _download_model(d)
    vocoder = Path(base) / VOCODER_NAME
    if not vocoder.exists():
        _download_vocoder(vocoder)
    return str(d)


def _download_model(dest: Path) -> None:
    import tarfile
    import urllib.request

    url = (f"https://github.com/k2-fsa/sherpa-onnx/releases/download/"
           f"tts-models/{MODEL_NAME}.tar.bz2")
    tmp = dest.parent / f"{MODEL_NAME}.tar.bz2"
    logger.info("Downloading ZipVoice model from %s → %s", url, tmp)
    urllib.request.urlretrieve(url, tmp)
    logger.info("Extracting %s", tmp)
    with tarfile.open(tmp, "r:bz2") as t:
        t.extractall(dest.parent)
    tmp.unlink(missing_ok=True)
    if not (dest / "encoder.int8.onnx").exists():
        raise RuntimeError(f"encoder.int8.onnx missing after extract at {dest}")


def _download_vocoder(dest: Path) -> None:
    import urllib.request

    url = ("https://github.com/k2-fsa/sherpa-onnx/releases/download/"
           "vocoder-models/vocos_24khz.onnx")
    logger.info("Downloading vocoder → %s", dest)
    urllib.request.urlretrieve(url, dest)


# ---------------------------------------------------------------------------
# Request model — mirrors moss-tts-nano/cosyvoice-3 TTSRequest for drop-in
# compatibility. Fields not used by ZipVoice are accepted and ignored.
# ---------------------------------------------------------------------------
class TTSRequest(BaseModel):
    text: str
    voice: Optional[str] = "中文女"
    prompt_audio_path: Optional[str] = None
    prompt_text: Optional[str] = None
    # Parity fields accepted but ignored by ZipVoice:
    sample_mode: str = "greedy"
    max_new_frames: int = 375
    voice_clone_max_text_tokens: int = 75
    seed: Optional[int] = None
    audio_temperature: float = 0.8
    audio_top_p: float = 0.95
    audio_top_k: int = 25
    audio_repetition_penalty: float = 1.2
    response_format: str = "wav"


# ---------------------------------------------------------------------------
# Endpoints (skeleton — /health only for now; A4-A5 add the rest)
# ---------------------------------------------------------------------------
@app.get("/health")
def health():
    return {
        "status": "ok" if tts is not None else "loading",
        "sample_rate": model_sample_rate,
        "voices": available_voices,
    }


# ---------------------------------------------------------------------------
# PCM / WAV helpers
# ---------------------------------------------------------------------------
def _wav_bytes(samples_f32, sample_rate: int) -> bytes:
    """float32 [-1,1] → int16 LE mono WAV bytes."""
    import numpy as np
    pcm = np.clip(np.asarray(samples_f32, dtype=np.float32).reshape(-1), -1.0, 1.0)
    pcm = (pcm * 32767.0).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm.tobytes())
    return buf.getvalue()


def _pcm_int16_le_bytes(samples_f32) -> bytes:
    """float32 [-1,1] → int16 LE bytes (no WAV header)."""
    import numpy as np
    pcm = np.clip(np.asarray(samples_f32, dtype=np.float32).reshape(-1), -1.0, 1.0)
    return (pcm * 32767.0).astype("<i2").tobytes()


def _resolve_prompt(req: "TTSRequest") -> tuple[str, str]:
    """Return (prompt_text, prompt_wav_path) — explicit or default."""
    if req.prompt_audio_path:
        return req.prompt_text or "", req.prompt_audio_path
    if req.voice == "中文女" and _default_prompt_wav:
        return _default_prompt_text, _default_prompt_wav
    if _default_prompt_wav:
        return _default_prompt_text, _default_prompt_wav
    raise HTTPException(500, "no voice available; pass prompt_audio_path")


def _generate(text: str, prompt_text: str, prompt_wav_path: str):
    """Synthesize one utterance. Returns GeneratedAudio (.samples, .sample_rate)."""
    return _synthesize_one(text, prompt_text, prompt_wav_path)


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------
@app.post("/tts")
def tts_full(req: TTSRequest):
    import time
    from fastapi.responses import Response
    if tts is None:
        raise HTTPException(503, "runtime not loaded")
    try:
        t0 = time.perf_counter()
        prompt_text, prompt_wav = _resolve_prompt(req)
        audio = _generate(req.text, prompt_text, prompt_wav)
        elapsed = time.perf_counter() - t0
        sr = int(audio.sample_rate)
        wav = _wav_bytes(audio.samples, sr)
        return Response(
            content=wav,
            media_type="audio/wav",
            headers={
                "X-Sample-Rate": str(sr),
                "X-Elapsed-Seconds": f"{elapsed:.4f}",
                "X-Duration-Seconds": f"{len(audio.samples)/sr:.4f}",
                "X-Channels": "1",
            },
        )
    except HTTPException:
        raise
    except Exception as e:
        logger.exception("synthesize failed")
        raise HTTPException(500, str(e))


@app.post("/tts/stream")
def tts_stream(req: TTSRequest):
    """Stream PCM chunks as NDJSON.

    Each line: {"seq": int, "data": "<base64 int16 le>",
                "sample_rate": 24000, "channels": 1, "is_pause": bool}

    ZipVoice returns a single complete waveform per generate() call (no true
    streaming). We emit it as one NDJSON line — voice-assistant tolerates
    this (same as CosyVoice 3 behavior).
    """
    import json
    import queue
    import threading
    from fastapi.responses import StreamingResponse

    if tts is None:
        raise HTTPException(503, "runtime not loaded")
    sr = int(model_sample_rate)
    prompt_text, prompt_wav = _resolve_prompt(req)

    def gen():
        seq = 0
        q: "queue.Queue" = queue.Queue(maxsize=4)

        def _worker():
            try:
                audio = _generate(req.text, prompt_text, prompt_wav)
                q.put(audio.samples)
            except Exception as exc:
                logger.exception("stream synthesis failed")
                q.put({"error": str(exc)})
            finally:
                q.put(None)

        threading.Thread(target=_worker, daemon=True, name="zipvoice-stream").start()

        while True:
            item = q.get()
            if item is None:
                break
            if isinstance(item, dict) and "error" in item:
                yield json.dumps(item, ensure_ascii=False) + "\n"
                break
            pcm = _pcm_int16_le_bytes(item)
            yield json.dumps(
                {
                    "seq": seq,
                    "data": base64.b64encode(pcm).decode(),
                    "sample_rate": sr,
                    "channels": 1,
                    "is_pause": False,
                },
                ensure_ascii=False,
            ) + "\n"
            seq += 1

    return StreamingResponse(gen(), media_type="application/x-ndjson")


@app.get("/voices")
def list_voices():
    if tts is None:
        raise HTTPException(503, "runtime not loaded")
    return {"voices": available_voices}


# ---------------------------------------------------------------------------
# Default voice registration (zero-shot reference audio)
# ---------------------------------------------------------------------------
_default_prompt_wav: Optional[str] = None
_default_prompt_text: Optional[str] = None


@app.on_event("startup")
def _startup():
    global tts, model_sample_rate, available_voices
    import sherpa_onnx

    model_root = _model_dir()
    base = Path(model_root).parent
    vocoder = str(base / VOCODER_NAME)
    threads = int(os.environ.get("VOICE_EDGE_TTS_CPU_THREADS", "2"))

    cfg = sherpa_onnx.OfflineTtsConfig(
        model=sherpa_onnx.OfflineTtsModelConfig(
            zipvoice=sherpa_onnx.OfflineTtsZipvoiceModelConfig(
                encoder=f"{model_root}/encoder.int8.onnx",
                decoder=f"{model_root}/decoder.int8.onnx",
                vocoder=vocoder,
                tokens=f"{model_root}/tokens.txt",
                lexicon=f"{model_root}/lexicon.txt",
                data_dir=f"{model_root}/espeak-ng-data",
            ),
            num_threads=threads,
            debug=False,
            provider="cpu",
        ),
        max_num_sentences=2,
    )
    if not cfg.validate():
        raise RuntimeError("sherpa-onnx ZipVoice config invalid; check paths")
    tts = sherpa_onnx.OfflineTts(cfg)
    logger.info("ZipVoice loaded (threads=%d)", threads)

    _register_default_voice()
    _warmup()


@app.on_event("shutdown")
def _shutdown():
    """Release model + reject new requests on SIGTERM."""
    global tts
    logger.info("voice-edge-tts shutting down")
    tts = None


def _register_default_voice():
    """Pre-register a default zero-shot voice so callers can use voice='中文女'."""
    global available_voices, _default_prompt_wav, _default_prompt_text
    assets = Path(__file__).parent / "assets"
    wav = assets / "default_prompt.wav"
    txt = assets / "default_prompt.txt"
    if not wav.is_file() or not txt.is_file():
        logger.warning(
            "default_prompt assets missing at %s; voice='中文女' will require prompt_audio_path",
            assets,
        )
        return
    _default_prompt_wav = str(wav)
    _default_prompt_text = txt.read_text(encoding="utf-8").strip()
    available_voices = ["中文女"]
    logger.info("Registered default voice '中文女' (prompt: %s)", _default_prompt_text[:30])


def _load_prompt(path: str):
    """Load any audio as 16kHz mono float32 list (ZipVoice expects 16kHz prompt).

    Returns (samples_list, sample_rate_int).
    """
    import soundfile as sf
    import numpy as np

    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if data.ndim > 1:
        data = data.mean(axis=1)
    if sr != 16000:
        n = int(len(data) * 16000 / sr)
        idx = np.linspace(0, len(data) - 1, n)
        data = np.interp(idx, np.arange(len(data)), data).astype(np.float32)
        sr = 16000
    return data.tolist(), int(sr)


def _synthesize_one(text: str, prompt_text: str, prompt_wav_path: str):
    """Run one generate() call. Returns GeneratedAudio."""
    samples, sr = _load_prompt(prompt_wav_path)
    # VERIFIED API: generate(text, prompt_text, prompt_samples, sample_rate, ...)
    return tts.generate(text, prompt_text, samples, sr)


def _warmup():
    try:
        if available_voices and _default_prompt_wav:
            _synthesize_one("你好", _default_prompt_text, _default_prompt_wav)
            logger.info("Warmup complete")
    except Exception as e:
        logger.warning("warmup failed (non-fatal): %s", e)


def main():
    parser = argparse.ArgumentParser(description="voice-edge-tts HTTP service")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9386)
    parser.add_argument(
        "--model-dir",
        default=None,
        help="ModelScope ID or local path. Defaults to env "
             "VOICE_EDGE_TTS_MODEL_DIR or auto-download.",
    )
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    if args.model_dir:
        os.environ["VOICE_EDGE_TTS_MODEL_DIR"] = args.model_dir

    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
