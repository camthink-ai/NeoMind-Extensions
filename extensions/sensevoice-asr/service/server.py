"""
SenseVoice ASR ONNX HTTP Service
=================================

Thin FastAPI wrapper around `sherpa_onnx.OfflineRecognizer` running the
SenseVoice-Small INT8 ONNX model. Exposes endpoints used by the Rust
extension:

* `POST /asr`          — full transcription, returns JSON with text + timings.
* `POST /asr/stream`   — chunked (pseudo-streaming) transcription. Emits
                         NDJSON: `{"seq", "partial", "final"}`. The final
                         line has `final=true` with the full text.
                         (SenseVoice is offline; we aggregate chunks and
                         emit one final result at the end. True streaming
                         would require a Streaming Zipformer / Paraformer
                         streaming model.)
* `GET  /languages`    — list supported language hints.
* `GET  /health`       — liveness probe.

Setup
-----
1. `pip install -r requirements.txt`
2. Run:
   ```
   python server.py --host 127.0.0.1 --port 9383
   ```
   The first run downloads ~230 MB of ONNX weights into
   `$SENSEVOICE_ASR_MODEL_DIR` (default: `~/.cache/sherpa-onnx`).

Notes
-----
* Input audio must be **16 kHz mono**. The service resamples other rates
  on the fly via linear interpolation (good enough for ASR; for production
  consider a proper resampler).
* Accepted input formats in `/asr`:
    - `audio_path` (str)  — local wav/mp3/m4a/flac file path (preferred)
    - `audio_base64` (str) — raw WAV bytes (incl. header) as base64
* SenseVoice supports: zh, en, ja, ko, yue, auto. The `language` hint
  improves accuracy but `auto` works well for mixed content.
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

logger = logging.getLogger("sensevoice-asr")

# ---------------------------------------------------------------------------
# Globals & config
# ---------------------------------------------------------------------------
MODEL_NAME = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"
MODEL_SUBDIR = MODEL_NAME

app = None  # type: Optional[FastAPI]
runtime = None  # sherpa_onnx.OfflineRecognizer
sample_rate_target = 16000

SUPPORTED_LANGS = ["auto", "zh", "en", "ja", "ko", "yue"]


# ---------------------------------------------------------------------------
# Request models — MUST be at module level for FastAPI's pydantic introspection.
# If defined inside a function, FastAPI fails to treat the param as a body and
# instead resolves it as a query parameter, causing 422 errors.
# ---------------------------------------------------------------------------
class AsrRequest(BaseModel):
    audio_path: Optional[str] = None
    audio_base64: Optional[str] = None  # WAV bytes as base64
    language: str = "auto"  # auto | zh | en | ja | ko | yue
    use_itn: bool = True


class ChunkRequest(AsrRequest):
    # ignored for SenseVoice (offline) — kept for API symmetry with a
    # future streaming backend.
    chunk_sec: float = 2.0


def _model_dir() -> str:
    """Resolve and ensure the model directory exists; auto-download if missing."""
    base = os.environ.get("SENSEVOICE_ASR_MODEL_DIR", str(Path.home() / ".cache" / "sherpa-onnx"))
    d = Path(base) / MODEL_SUBDIR
    if not d.exists():
        d.mkdir(parents=True, exist_ok=True)
        _download_model(d)
    return str(d)


def _download_model(dest: Path) -> None:
    """Download + extract the SenseVoice INT8 model from the sherpa-onnx mirror."""
    import tarfile
    import urllib.request

    url = (
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/"
        "asr-models/" + MODEL_NAME + ".tar.bz2"
    )
    tmp_tar = dest.parent / f"{MODEL_NAME}.tar.bz2"
    logger.info("Downloading SenseVoice model from %s → %s", url, tmp_tar)
    urllib.request.urlretrieve(url, tmp_tar)
    logger.info("Extracting %s → %s", tmp_tar, dest.parent)
    with tarfile.open(tmp_tar, "r:bz2") as t:
        t.extractall(dest.parent)
    tmp_tar.unlink(missing_ok=True)
    # The tarball extracts to <parent>/<MODEL_NAME>/. Verify.
    if not (dest / "model.int8.onnx").exists():
        raise RuntimeError(
            f"Model files missing after extract at {dest}/model.int8.onnx"
        )


# ---------------------------------------------------------------------------
# FastAPI app
# ---------------------------------------------------------------------------
def _build_app():
    app = FastAPI(title="SenseVoice ASR Service")
    return app


# ---------------------------------------------------------------------------
# Audio loading helpers
# ---------------------------------------------------------------------------
def _read_wav_bytes(b: bytes) -> tuple[np.ndarray, int]:
    """Parse WAV bytes → (float32 mono, sample_rate)."""
    import wave
    with wave.open(io.BytesIO(b), "rb") as w:
        sr = w.getframerate()
        ch = w.getnchannels()
        sw = w.getsampwidth()
        n = w.getnframes()
        raw = w.readframes(n)
    if sw != 2:
        raise ValueError(f"only 16-bit PCM WAV supported (got sampwidth={sw})")
    arr = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    if ch > 1:
        arr = arr.reshape(-1, ch).mean(axis=1)
    return arr, sr


def _read_audio_file(path: str) -> tuple[np.ndarray, int]:
    """Read any audio file via soundfile → (float32 mono, sample_rate)."""
    import soundfile as sf
    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if data.ndim > 1:
        data = data.mean(axis=1)
    return data.astype(np.float32), int(sr)


def _resample_linear(data: np.ndarray, sr_in: int, sr_out: int = 16000) -> np.ndarray:
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
    return _resample_linear(data, sr, sample_rate_target)


# ---------------------------------------------------------------------------
# Recognizer helpers
# ---------------------------------------------------------------------------
def _recognize(audio: np.ndarray, language: str, use_itn: bool) -> dict:
    """Run SenseVoice on a full utterance. Returns dict with text + timings."""
    assert runtime is not None, "runtime not initialized"
    # SenseVoice language codes:
    #   ""  → auto
    #   "zh"/"en"/"ja"/"ko"/"yue"
    lang_map = {"auto": "", "zh": "zh", "en": "en", "ja": "ja", "ko": "ko", "yue": "yue"}
    lang_code = lang_map.get(language, "")
    stream = runtime.create_stream()
    stream.accept_waveform(sample_rate_target, audio)
    t0 = time.perf_counter()
    runtime.decode_stream(stream)
    elapsed = time.perf_counter() - t0
    text = stream.result.text.strip()
    dur = len(audio) / sample_rate_target
    return {
        "text": text,
        "language": language,
        "elapsed_seconds": elapsed,
        "duration_seconds": dur,
        "rtf": elapsed / dur if dur > 0 else 0.0,
    }


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------
def register_routes(app):
    import json

    @app.get("/health")
    def health():
        return {"status": "ok" if runtime is not None else "loading"}

    @app.get("/languages")
    def languages():
        return {"languages": SUPPORTED_LANGS}

    @app.post("/asr")
    def asr(req: AsrRequest):
        if runtime is None:
            raise HTTPException(503, "runtime not loaded")
        if req.language not in SUPPORTED_LANGS:
            raise HTTPException(400, f"unsupported language: {req.language}")
        try:
            audio = _decode_audio(req)
            if len(audio) == 0:
                raise HTTPException(400, "empty audio")
            result = _recognize(audio, req.language, req.use_itn)
            return JSONResponse(
                {
                    "text": result["text"],
                    "language": result["language"],
                    "elapsed_seconds": result["elapsed_seconds"],
                    "duration_seconds": result["duration_seconds"],
                    "rtf": result["rtf"],
                },
                headers={
                    "X-Elapsed-Seconds": f"{result['elapsed_seconds']:.4f}",
                    "X-Duration-Seconds": f"{result['duration_seconds']:.4f}",
                    "X-RTF": f"{result['rtf']:.4f}",
                },
            )
        except HTTPException:
            raise
        except Exception as e:
            logger.exception("asr failed")
            raise HTTPException(500, str(e))

    @app.post("/asr/stream")
    def asr_stream(req: ChunkRequest):
        """Pseudo-streaming endpoint for API symmetry. Emits NDJSON:

            {"seq":0,"partial":""}
            ...
            {"seq":N,"final":true,"text":"...","elapsed_seconds":...}

        SenseVoice is offline, so we just emit the final result once. This
        keeps the extension protocol ready for a future streaming backend
        (Streaming Zipformer / Paraformer-streaming) without breaking API.
        """
        if runtime is None:
            raise HTTPException(503, "runtime not loaded")
        if req.language not in SUPPORTED_LANGS:
            raise HTTPException(400, f"unsupported language: {req.language}")

        def gen():
            try:
                audio = _decode_audio(req)
                if len(audio) == 0:
                    yield json.dumps({"error": "empty audio"}, ensure_ascii=False) + "\n"
                    return
                yield json.dumps({"seq": 0, "partial": ""}, ensure_ascii=False) + "\n"
                result = _recognize(audio, req.language, req.use_itn)
                yield json.dumps(
                    {
                        "seq": 1,
                        "final": True,
                        "text": result["text"],
                        "elapsed_seconds": result["elapsed_seconds"],
                        "duration_seconds": result["duration_seconds"],
                    },
                    ensure_ascii=False,
                ) + "\n"
            except Exception as e:
                logger.exception("stream asr failed")
                yield json.dumps({"error": str(e)}, ensure_ascii=False) + "\n"

        return StreamingResponse(gen(), media_type="application/x-ndjson")


# ---------------------------------------------------------------------------
# Lifecycle
# ---------------------------------------------------------------------------
def _startup():
    global runtime
    import sherpa_onnx

    model_dir = _model_dir()
    threads = int(os.environ.get("SENSEVOICE_ASR_CPU_THREADS", "2"))
    model_path = str(Path(model_dir) / "model.int8.onnx")
    tokens_path = str(Path(model_dir) / "tokens.txt")
    if not Path(model_path).exists():
        raise RuntimeError(f"model file missing: {model_path}")

    runtime = sherpa_onnx.OfflineRecognizer.from_sense_voice(
        model=model_path,
        tokens=tokens_path,
        use_itn=True,
        num_threads=threads,
    )
    logger.info(
        "SenseVoice loaded (model=%s, threads=%d)", model_path, threads,
    )

    # Warmup: a short silent utterance primes ONNX session graph.
    try:
        s = runtime.create_stream()
        s.accept_waveform(sample_rate_target, np.zeros(sample_rate_target, dtype=np.float32))
        runtime.decode_stream(s)
        logger.info("Warmup complete")
    except Exception as e:
        logger.warning("warmup failed (non-fatal): %s", e)


def main():
    parser = argparse.ArgumentParser(description="SenseVoice ASR ONNX HTTP service")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9383)
    parser.add_argument("--cpu-threads", type=int, default=2)
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    os.environ.setdefault("SENSEVOICE_ASR_CPU_THREADS", str(args.cpu_threads))

    global app
    app = _build_app()
    register_routes(app)

    # Startup hook: load model before uvicorn starts serving.
    _startup()

    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
