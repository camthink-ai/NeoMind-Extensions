"""
CosyVoice 3 HTTP Service (NDJSON adapter)
=========================================

Thin FastAPI wrapper around `FunAudioLLM/Fun-CosyVoice3-0.5B-2512` that
exposes the **same `/tts/stream` NDJSON contract** as the moss-tts-nano
service. voice-assistant can switch backends by changing one env var.

Endpoints
---------
* `POST /tts`         — full synthesis, returns WAV bytes (with X-* headers).
* `POST /tts/stream`  — streaming synthesis, returns NDJSON: one line per PCM
                        chunk, each line is `{"seq", "data", "sample_rate",
                        "channels", "is_pause"}` where `data` is base64
                        int16 LE.
* `GET  /voices`      — list registered voice IDs.
* `GET  /health`      — liveness probe.

Voice model
-----------
CosyVoice3-0.5B is **zero-shot only** — it has no SFT voice bank. So:
* If `prompt_audio_path` is provided → use it as the zero-shot reference.
* Otherwise → fall back to the bundled CosyVoice repo prompt
  (`asset/zero_shot_prompt.wav`) pre-registered at startup as the default
  voice "中文女" via `add_zero_shot_spk`.
* The `voice` parameter is matched against registered zero-shot spk IDs
  (populated at startup). Unknown voice → default.

Setup
-----
1. `pip install -r requirements.txt` + CosyVoice repo on PYTHONPATH
   (handled by start.sh).
2. `./start.sh` — first run downloads ~8GB model from ModelScope.

Notes
-----
* Output is **24 kHz mono**. voice-assistant's `_tts_to_browser_pcm` already
  resamples arbitrary sample_rate / channels, so no caller-side change is
  needed.
* Inference runs in a worker thread (CosyVoice's generator is sync). The
  HTTP generator drains a bounded queue for backpressure.
* Apple Silicon: `PYTORCH_ENABLE_MPS_FALLBACK=1` (set in start.sh) makes
  unsupported MPS ops fall back to CPU. CUDA devices use CUDA natively.
"""
from __future__ import annotations

import argparse
import base64
import io
import json
import logging
import os
import queue
import threading
import time
import wave
from pathlib import Path
from typing import Optional

import numpy as np

logger = logging.getLogger("cosyvoice-3")

from fastapi import FastAPI, HTTPException  # noqa: E402
from fastapi.responses import Response, StreamingResponse, JSONResponse  # noqa: E402
from pydantic import BaseModel  # noqa: E402

# ---------------------------------------------------------------------------
# Globals
# ---------------------------------------------------------------------------
app = FastAPI(title="CosyVoice 3 Service")

# Lazy-loaded on startup.
cosyvoice = None
model_sample_rate: int = 24000  # CosyVoice 3 default; confirmed after load
available_voices: list[str] = []


# ---------------------------------------------------------------------------
# Request model — mirrors moss-tts-nano TTSRequest for drop-in compatibility.
# ---------------------------------------------------------------------------
class TTSRequest(BaseModel):
    text: str
    voice: Optional[str] = "中文女"
    prompt_audio_path: Optional[str] = None
    prompt_text: Optional[str] = None
    sample_mode: str = "greedy"  # accepted for parity; CosyVoice ignores
    max_new_frames: int = 375    # accepted for parity; CosyVoice ignores
    voice_clone_max_text_tokens: int = 75  # accepted for parity; ignored
    seed: Optional[int] = None
    audio_temperature: float = 0.8  # accepted for parity; ignored
    audio_top_p: float = 0.95       # accepted for parity; ignored
    audio_top_k: int = 25           # accepted for parity; ignored
    audio_repetition_penalty: float = 1.2  # accepted for parity; ignored
    response_format: str = "wav"   # wav | base64


def _upcast_bf16(model, torch) -> None:
    """Walk all submodules and convert BFloat16 parameters/tensors to
    float32 in-place. Required on backends without BF16 autocast (MPS,
    CPU) because Qwen2 LLM weights ship as BF16 but CosyVoice's
    inference_*.path uses float32 activations.

    CosyVoice3's wrapper layout:
      CosyVoice3 → .model (CosyVoice3Model, NOT nn.Module)
                   ├─ .llm  (CosyVoice3LM, nn.Module, 290 BF16 params)
                   ├─ .flow (nn.Module)
                   └─ .hift (nn.Module)
    We recurse through any attribute that exposes `modules()` and upcast
    every BF16 param/buffer we find.
    """
    visited = set()
    stack = [model]
    upcasted = 0
    while stack:
        obj = stack.pop()
        if id(obj) in visited:
            continue
        visited.add(id(obj))
        if hasattr(obj, "modules") and callable(getattr(obj, "modules", None)):
            try:
                for sub in obj.modules():
                    for name, p in list(getattr(sub, "_parameters", {}).items()):
                        if p is not None and p.dtype == torch.bfloat16:
                            with torch.no_grad():
                                sub._parameters[name] = torch.nn.Parameter(
                                    p.to(torch.float32), requires_grad=False,
                                )
                            upcasted += 1
                    for name, b in list(getattr(sub, "_buffers", {}).items()):
                        if b is not None and b.dtype == torch.bfloat16:
                            with torch.no_grad():
                                sub._buffers[name] = b.to(torch.float32)
                            upcasted += 1
            except Exception:
                pass
        # Recurse into child attributes (wrapper layers).
        for attr in ("model", "llm", "flow", "hift", "frontend"):
            child = getattr(obj, attr, None)
            if child is not None and id(child) not in visited:
                stack.append(child)
    if upcasted:
        logger.info("Upcasted %d BF16 tensors to float32 for non-CUDA backend", upcasted)
    else:
        logger.warning("_upcast_bf16: no BF16 tensors found; dtype errors may persist")


# ---------------------------------------------------------------------------
# PCM helpers
# ---------------------------------------------------------------------------
def _wav_bytes(waveform: np.ndarray, sample_rate: int) -> bytes:
    """float32 [-1, 1] → int16 LE PCM WAV bytes (mono or stereo)."""
    wf = np.asarray(waveform, dtype=np.float32)
    stereo = wf.ndim == 2
    n_channels = wf.shape[1] if stereo else 1
    interleaved = wf.reshape(-1) if stereo else wf
    pcm = np.clip(interleaved, -1.0, 1.0)
    pcm = (pcm * 32767.0).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(n_channels)
        w.setsampwidth(2)
        w.setframerate(sample_rate)
        w.writeframes(pcm.tobytes())
    return buf.getvalue()


def _pcm_int16_le_bytes(waveform: np.ndarray) -> bytes:
    """float32 [-1, 1] → int16 LE bytes (no WAV header)."""
    wf = np.asarray(waveform, dtype=np.float32)
    if wf.ndim == 2:
        wf = wf.reshape(-1)
    pcm = np.clip(wf, -1.0, 1.0)
    return (pcm * 32767.0).astype("<i2").tobytes()


def _load_prompt_wav(path: str) -> np.ndarray:
    """Load any audio file as 16kHz mono float32 numpy array (CosyVoice API)."""
    import torchaudio

    wav, sr = torchaudio.load(path)
    # CosyVoice expects mono; average channels if stereo.
    if wav.dim() == 2 and wav.shape[0] > 1:
        wav = wav.mean(dim=0, keepdim=True)
    if sr != 16000:
        wav = torchaudio.functional.resample(wav, sr, 16000)
    # Squeeze leading channel dim → 1D float32 numpy.
    return wav.squeeze(0).numpy().astype(np.float32)


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------
@app.get("/health")
def health():
    return {
        "status": "ok" if cosyvoice is not None else "loading",
        "sample_rate": model_sample_rate,
        "voices": available_voices,
    }


@app.get("/voices")
def list_voices():
    if cosyvoice is None:
        raise HTTPException(503, "runtime not loaded")
    return {"voices": available_voices}


@app.post("/tts")
def tts(req: TTSRequest):
    if cosyvoice is None:
        raise HTTPException(503, "runtime not loaded")
    try:
        t0 = time.perf_counter()
        pcm_chunks: list[np.ndarray] = []
        for chunk in _run_inference(req):
            arr = np.asarray(chunk, dtype=np.float32).reshape(-1)
            pcm_chunks.append(arr)
        if not pcm_chunks:
            raise HTTPException(500, "inference produced no audio")
        wf = np.concatenate(pcm_chunks) if len(pcm_chunks) > 1 else pcm_chunks[0]
        sr = int(model_sample_rate)
        elapsed = time.perf_counter() - t0
        audio_bytes = _wav_bytes(wf, sr)

        if req.response_format == "base64":
            return JSONResponse({
                "audio": base64.b64encode(audio_bytes).decode(),
                "sample_rate": sr,
                "elapsed_seconds": elapsed,
                "duration_seconds": float(len(wf) / sr),
            })

        return Response(
            content=audio_bytes,
            media_type="audio/wav",
            headers={
                "X-Sample-Rate": str(sr),
                "X-Elapsed-Seconds": f"{elapsed:.4f}",
                "X-Duration-Seconds": f"{len(wf) / sr:.4f}",
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

    Each emitted line::

        {"seq": int, "data": "<base64 int16 le>",
         "sample_rate": 24000, "channels": 1, "is_pause": bool}

    On error, emits a final line ``{"error": "..."}`` then stops.

    The CosyVoice generator is synchronous, so we run inference in a worker
    thread that pushes chunks onto a bounded queue. The HTTP generator
    drains the queue. For the voice-assistant use case (single user, one
    utterance at a time) this is sufficient; concurrent streaming requests
    would serialize on the model lock anyway.
    """
    if cosyvoice is None:
        raise HTTPException(503, "runtime not loaded")

    sr = int(model_sample_rate)
    channels = 1

    def gen():
        seq = 0
        event_queue: "queue.Queue[Optional[dict]]" = queue.Queue(maxsize=128)

        def _worker() -> None:
            try:
                for chunk in _run_inference(req):
                    arr = np.asarray(chunk, dtype=np.float32).reshape(-1)
                    event_queue.put({
                        "waveform": arr,
                        "sample_rate": sr,
                        "channels": channels,
                        "is_pause": False,
                    })
            except Exception as exc:
                logger.exception("stream synthesis failed")
                event_queue.put({"type": "error", "error": str(exc)})
            finally:
                event_queue.put(None)  # sentinel

        worker = threading.Thread(target=_worker, name="cosyvoice-stream", daemon=True)
        worker.start()

        while True:
            item = event_queue.get()
            if item is None:
                break
            if "waveform" in item:
                pcm = _pcm_int16_le_bytes(item["waveform"])
                yield json.dumps({
                    "seq": seq,
                    "data": base64.b64encode(pcm).decode(),
                    "sample_rate": item["sample_rate"],
                    "channels": item["channels"],
                    "is_pause": item["is_pause"],
                }, ensure_ascii=False) + "\n"
                seq += 1
            elif item.get("type") == "error":
                yield json.dumps({"error": item["error"]}, ensure_ascii=False) + "\n"

        # Don't leak the worker thread if the client disconnected mid-stream.
        worker.join(timeout=2.0)

    return StreamingResponse(gen(), media_type="application/x-ndjson")


# ---------------------------------------------------------------------------
# Inference dispatcher
# ---------------------------------------------------------------------------
ENDOFPROMPT = "<|endofprompt|>"


def _ensure_endofprompt(text: str, prompt_text: str) -> str:
    """CosyVoice3 LLM requires `<|endofprompt|>` (token 151646) to be
    present in the concatenated text+prompt_text, otherwise it asserts out.
    The text_frontend doesn't add it for v3, so append to text if missing.
    """
    if ENDOFPROMPT in text or (prompt_text and ENDOFPROMPT in prompt_text):
        return text
    return text + ENDOFPROMPT


def _run_inference(req: TTSRequest):
    """Yield float32 numpy waveforms (one per CosyVoice chunk).

    CosyVoice3-0.5B is zero-shot only, so:
    * prompt_audio_path provided → clone that speaker.
    * Otherwise → use the pre-registered default voice
      (`voice` selects among zero-shot spk IDs added at startup).
    """
    if req.prompt_audio_path:
        prompt_16k = _load_prompt_wav(req.prompt_audio_path)
        prompt_text = req.prompt_text or ""
        text = _ensure_endofprompt(req.text, prompt_text)
        return cosyvoice.inference_zero_shot(
            text, prompt_text, prompt_16k,
        )

    voice = req.voice or "中文女"
    if voice in available_voices:
        # Pre-registered zero-shot spk ID. prompt_text/prompt_wav are
        # ignored when zero_shot_spk_id is set; the marker must go in text.
        text = _ensure_endofprompt(req.text, "")
        return cosyvoice.inference_zero_shot(
            text, "", "", zero_shot_spk_id=voice,
        )
    # Unknown voice — fall back to default.
    if available_voices:
        text = _ensure_endofprompt(req.text, "")
        return cosyvoice.inference_zero_shot(
            text, "", "", zero_shot_spk_id=available_voices[0],
        )
    raise HTTPException(500, "no voice available; pass prompt_audio_path")


# CosyVoice's generators yield dicts like
# `{"tts_speech": <float32 tensor [C, T]>, ...}`. Normalize to 1D numpy.
def _normalize_chunk(chunk):
    if isinstance(chunk, dict) and "tts_speech" in chunk:
        arr = chunk["tts_speech"]
        # torch tensor → numpy
        try:
            arr = arr.detach().cpu().numpy()
        except AttributeError:
            arr = np.asarray(arr)
        # collapse leading channel dim
        if arr.ndim >= 2:
            arr = arr.reshape(-1)
        return arr.astype(np.float32, copy=False)
    arr = np.asarray(chunk, dtype=np.float32)
    if arr.ndim >= 2:
        arr = arr.reshape(-1)
    return arr


# Monkey-patch: wrap CosyVoice generators so each yielded item is a flat
# numpy array. This keeps `_run_inference` and the stream/tts code simple.
def _wrap_generator(gen):
    while True:
        try:
            chunk = next(gen)
        except StopIteration:
            return
        yield _normalize_chunk(chunk)


# Patch inference_* methods to return normalized generators. Done at startup
# after model load — see `_startup`.
def _patch_inference_methods():
    for name in ("inference_sft", "inference_zero_shot",
                 "inference_cross_lingual", "inference_instruct",
                 "inference_instruct2"):
        original = getattr(cosyvoice, name, None)
        if original is None or getattr(original, "_cosywrap", False):
            continue

        def _make(fn):
            def _wrapped(*args, **kwargs):
                return _wrap_generator(fn(*args, **kwargs))
            _wrapped._cosywrap = True
            return _wrapped

        setattr(cosyvoice, name, _make(original))


# ---------------------------------------------------------------------------
# Lifecycle
# ---------------------------------------------------------------------------
@app.on_event("startup")
def _startup():
    global cosyvoice, model_sample_rate, available_voices
    from cosyvoice.cli.cosyvoice import AutoModel  # type: ignore

    model_dir = os.environ.get(
        "COSYVOICE_MODEL_DIR",
        "FunAudioLLM/Fun-CosyVoice3-0.5B-2512",
    )
    logger.info("Loading CosyVoice model: %s", model_dir)
    t0 = time.perf_counter()
    # fp16=True uses torch.cuda.amp.autocast which is CUDA-only. On Apple
    # Silicon MPS it's a no-op, so we'd still hit "mat1 and mat2 must have
    # the same dtype" because Qwen2 LLM weights are BFloat16 while runtime
    # activations are float32. Fix: load with fp16=False then upcast any
    # BFloat16 params to float32. This is slower than CUDA autocast but
    # works on every backend (MPS, CPU, CUDA). On CUDA production we can
    # flip this back to fp16=True for speed.
    cosyvoice = AutoModel(model_dir=model_dir, fp16=False)

    import torch  # local import keeps top-level dep surface small
    _upcast_bf16(cosyvoice, torch)
    elapsed = time.perf_counter() - t0
    logger.info("CosyVoice model loaded in %.1fs", elapsed)

    # Normalize inference outputs.
    _patch_inference_methods()

    # Sample rate: CosyVoice 3 = 24000, v2 = 22050. Read from model if exposed.
    sr = getattr(cosyvoice, "sample_rate", None)
    if isinstance(sr, (int, float)):
        model_sample_rate = int(sr)
    else:
        model_sample_rate = 24000
        logger.warning(
            "cosyvoice.sample_rate not found; assuming %d Hz",
            model_sample_rate,
        )

    # CosyVoice3-0.5B is zero-shot only (no SFT voice bank). Pre-register
    # the bundled CosyVoice repo prompt as the default voice "中文女" so
    # callers can use `voice="中文女"` without supplying prompt audio.
    repo_dir = os.environ.get("COSYVOICE_REPO", "")
    if not repo_dir:
        # start.sh sets COSYVOICE_REPO; fall back to ~/CosyVoice for manual runs.
        repo_dir = str(Path.home() / "CosyVoice")
    default_prompt_wav = Path(repo_dir) / "asset" / "zero_shot_prompt.wav"
    # Default prompt transcript — same as CosyVoice example.py.
    default_prompt_text = "希望你以后能够做的比我还好呦。"

    available_voices = []
    if default_prompt_wav.is_file():
        try:
            ok = cosyvoice.add_zero_shot_spk(
                default_prompt_text,
                str(default_prompt_wav),
                "中文女",
            )
            if ok:
                available_voices = ["中文女"]
                cosyvoice.save_spkinfo()
                logger.info("Registered default voice '中文女' from %s",
                            default_prompt_wav)
            else:
                logger.warning("add_zero_shot_spk returned False for '中文女'")
        except Exception as e:  # pragma: no cover
            logger.warning("Failed to register default voice: %s", e)
    else:
        logger.warning(
            "Bundled prompt not found: %s — voice param will be ignored; "
            "callers must pass prompt_audio_path",
            default_prompt_wav,
        )

    # Warmup (optional, makes the first real request much faster).
    try:
        if available_voices:
            warmup_text = "你好" + ENDOFPROMPT
            for _ in cosyvoice.inference_zero_shot(
                warmup_text, "", "", zero_shot_spk_id=available_voices[0],
            ):
                break
            logger.info("CosyVoice warmup complete")
    except Exception as e:  # pragma: no cover
        logger.warning("warmup failed (non-fatal): %s", e)


def main():
    parser = argparse.ArgumentParser(description="CosyVoice 3 HTTP service")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9385)
    parser.add_argument(
        "--model-dir", default=None,
        help="ModelScope ID or local path to CosyVoice 3 model. "
             "Defaults to env COSYVOICE_MODEL_DIR or "
             "'FunAudioLLM/Fun-CosyVoice3-0.5B-2512'.",
    )
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    if args.model_dir:
        os.environ["COSYVOICE_MODEL_DIR"] = args.model_dir

    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
