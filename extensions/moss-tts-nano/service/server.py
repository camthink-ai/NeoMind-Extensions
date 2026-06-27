"""
MOSS-TTS-Nano ONNX HTTP Service
================================

Thin FastAPI wrapper around `OnnxTtsRuntime` from the MOSS-TTS-Nano
ONNX CPU backend. Exposes two endpoints used by the Rust extension:

* `POST /tts`         — full synthesis, returns WAV bytes (with X-* headers).
* `POST /tts/stream`  — streaming synthesis, returns NDJSON: one line per PCM
                        chunk, each line is `{"seq", "data", "sample_rate",
                        "channels", "is_pause"}` where `data` is base64 int16 LE.

Plus:
* `GET  /voices`      — list built-in voice presets.
* `GET  /health`      — liveness probe.

Setup
-----
1. `git clone https://github.com/OpenMOSS/MOSS-TTS-Nano.git`
2. `cd MOSS-TTS-Nano && pip install -r requirements.txt && pip install -e .`
3. Set `MOSS_TTS_NANO_REPO=/path/to/MOSS-TTS-Nano` (so we can import
   `onnx_tts_runtime`, `text_normalization_pipeline`).
4. Run this server:
   ```
   python server.py --host 127.0.0.1 --port 9382
   ```
   The first run downloads ONNX weights into `./models/` (~200 MB).

Notes
-----
* Output is **48 kHz stereo**. PCM chunks in `/tts/stream` are int16 LE,
  interleaved L,R,L,R,...
* `WeTextProcessingManager` is optional — the server falls back to raw text
  normalization if it cannot start (e.g. pynini missing on minimal Linux).
"""
from __future__ import annotations

import argparse
import base64
import io
import logging
import os
import queue
import sys
import threading
import time
import wave
from pathlib import Path
from typing import Optional

import numpy as np

logger = logging.getLogger("moss-tts")

# ---------------------------------------------------------------------------
# Make the MOSS-TTS-Nano repo importable (onnx_tts_runtime, etc.)
# ---------------------------------------------------------------------------
_REPO_ENV = "MOSS_TTS_NANO_REPO"
_repo_dir = os.environ.get(_REPO_ENV)
if _repo_dir and Path(_repo_dir).is_dir():
    sys.path.insert(0, _repo_dir)
elif not _repo_dir:
    # Default layout: sibling directory of this service folder.
    _guess = Path(__file__).resolve().parents[3] / "MOSS-TTS-Nano"
    if _guess.is_dir():
        sys.path.insert(0, str(_guess))
        logger.warning(
            "%s not set; guessed %s. Set the env var explicitly if this is wrong.",
            _REPO_ENV, _guess,
        )

from fastapi import FastAPI, HTTPException  # noqa: E402
from fastapi.responses import Response, StreamingResponse, JSONResponse  # noqa: E402
from pydantic import BaseModel  # noqa: E402

try:
    from onnx_tts_runtime import OnnxTtsRuntime  # type: ignore
except ImportError as e:  # pragma: no cover
    raise SystemExit(
        f"Cannot import onnx_tts_runtime: {e}. "
        f"Make sure MOSS-TTS-Nano repo is on PYTHONPATH (env {_REPO_ENV})."
    )

# Streaming helpers from the upstream runtime. These are the same primitives
# the upstream `app_onnx.py::synthesize_stream()` uses; we reuse them rather
# than re-implementing the adaptive batching policy.
try:
    from ort_cpu_runtime import (  # type: ignore
        _resolve_stream_decode_frame_budget,
        _normalize_sample_mode,
    )
    from onnx_tts_runtime import _merge_audio_channels  # type: ignore
except ImportError as e:  # pragma: no cover
    raise SystemExit(
        f"Cannot import streaming helpers from MOSS-TTS-Nano runtime: {e}. "
        f"Update the MOSS-TTS-Nano repo (env {_REPO_ENV})."
    )

try:
    from text_normalization_pipeline import WeTextProcessingManager  # type: ignore
    _HAVE_WETEXT = True
except Exception:  # pragma: no cover
    _HAVE_WETEXT = False
    WeTextProcessingManager = None  # type: ignore

# ---------------------------------------------------------------------------
# Globals
# ---------------------------------------------------------------------------
app = FastAPI(title="MOSS-TTS-Nano Service")

runtime: Optional[OnnxTtsRuntime] = None
normalizer = None


# ---------------------------------------------------------------------------
# Request model
# ---------------------------------------------------------------------------
class TTSRequest(BaseModel):
    text: str
    voice: Optional[str] = "Junhao"
    prompt_audio_path: Optional[str] = None
    sample_mode: str = "fixed"  # greedy / fixed / full
    max_new_frames: int = 375
    voice_clone_max_text_tokens: int = 75
    seed: Optional[int] = None
    audio_temperature: float = 0.8
    audio_top_p: float = 0.95
    audio_top_k: int = 25
    audio_repetition_penalty: float = 1.2
    response_format: str = "wav"   # wav | base64


# ---------------------------------------------------------------------------
# PCM helpers
# ---------------------------------------------------------------------------
def _wav_bytes(waveform: np.ndarray, sample_rate: int) -> bytes:
    """float32 [-1, 1] → int16 LE PCM WAV bytes (stereo or mono)."""
    wf = np.asarray(waveform, dtype=np.float32)
    stereo = wf.ndim == 2
    n_channels = wf.shape[1] if stereo else 1
    if stereo:
        # interleaved: L,R,L,R,...
        interleaved = wf.reshape(-1)
    else:
        interleaved = wf
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


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------
@app.get("/health")
def health():
    return {"status": "ok" if runtime is not None else "loading",
            "wetext": _HAVE_WETEXT}


@app.get("/voices")
def list_voices():
    if runtime is None:
        raise HTTPException(503, "runtime not loaded")
    try:
        voices = [v["voice"] for v in runtime.list_builtin_voices()]
    except Exception as e:  # pragma: no cover
        raise HTTPException(500, f"list_builtin_voices failed: {e}")
    return {"voices": voices}


@app.post("/tts")
def tts(req: TTSRequest):
    if runtime is None:
        raise HTTPException(503, "runtime not loaded")
    try:
        t0 = time.perf_counter()
        result = runtime.synthesize(
            text=req.text,
            voice=req.voice,
            prompt_audio_path=req.prompt_audio_path,
            sample_mode=req.sample_mode,
            do_sample=(req.sample_mode != "greedy"),
            streaming=False,
            max_new_frames=req.max_new_frames,
            voice_clone_max_text_tokens=req.voice_clone_max_text_tokens,
            enable_wetext=_HAVE_WETEXT,
            enable_normalize_tts_text=True,
            seed=req.seed,
        )
        wf = np.asarray(result["waveform"], dtype=np.float32)
        sr = int(result["sample_rate"])
        elapsed = time.perf_counter() - t0
        audio_bytes = _wav_bytes(wf, sr)

        if req.response_format == "base64":
            return JSONResponse({
                "audio": base64.b64encode(audio_bytes).decode(),
                "sample_rate": sr,
                "elapsed_seconds": elapsed,
                "duration_seconds": len(wf) / sr,
            })

        # Default: return raw WAV with metadata in headers.
        return Response(
            content=audio_bytes,
            media_type="audio/wav",
            headers={
                "X-Sample-Rate": str(sr),
                "X-Elapsed-Seconds": f"{elapsed:.4f}",
                "X-Duration-Seconds": f"{len(wf) / sr:.4f}",
                "X-Channels": str(wf.ndim + 1 if wf.ndim == 1 else wf.shape[1]),
            },
        )
    except HTTPException:
        raise
    except Exception as e:
        logger.exception("synthesize failed")
        raise HTTPException(500, str(e))


@app.post("/tts/stream")
def tts_stream(req: TTSRequest):
    """Stream PCM chunks as NDJSON with TRUE per-frame streaming.

    Each emitted line::

        {"seq": int, "data": "<base64 int16 le>",
         "sample_rate": 48000, "channels": 2, "is_pause": bool}

    On error, emits a final line ``{"error": "..."}`` then stops.

    Architecture (mirrors upstream ``app_onnx.py::synthesize_stream``):

    * A worker thread runs ``generate_audio_frames()`` with an ``on_frame``
      callback. The model is autoregressive — each forward pass yields
      exactly one codec frame (≈80ms of audio at 48kHz).
    * ``on_frame`` appends to ``pending`` and calls ``_decode_pending(False)``.
    * ``_decode_pending`` consults ``_resolve_stream_decode_frame_budget``
      which returns an adaptive batch size (1 → 2 → 4 → 8 frames) based on
      how far ahead of realtime we are. At startup it returns 1, so the
      very first generated frame is decoded and emitted immediately —
      this is what gives ~150-300ms first-byte instead of ~14s.
    * Decoded waveforms are pushed onto a bounded ``queue.Queue(maxsize=128)``
      which gives backpressure if the client is slow.
    * The HTTP generator drains the queue and emits NDJSON lines.

    ``req.max_new_frames`` is now honored (previously parsed but ignored),
    so callers can cap output length per request.

    Single-request-at-a-time: the worker mutates
    ``runtime.manifest["generation_defaults"]`` per request. Concurrent
    streaming requests would clobber each other. FastAPI runs sync
    endpoints in a threadpool — for the voice assistant use case (one
    user, one utterance at a time) this is fine.
    """
    if runtime is None:
        raise HTTPException(503, "runtime not loaded")

    # Snapshot request fields — pydantic may reuse the model object.
    text = req.text
    voice = req.voice
    prompt_audio_path = req.prompt_audio_path
    sample_mode = req.sample_mode
    max_new_frames = int(req.max_new_frames)
    voice_clone_max_text_tokens = int(req.voice_clone_max_text_tokens)
    do_sample = sample_mode != "greedy"
    seed = req.seed
    audio_temperature = req.audio_temperature
    audio_top_p = req.audio_top_p
    audio_top_k = req.audio_top_k
    audio_repetition_penalty = req.audio_repetition_penalty

    def gen():
        import json
        seq = 0
        event_queue: "queue.Queue[Optional[dict]]" = queue.Queue(maxsize=128)

        def _worker() -> None:
            try:
                # Apply per-request generation options by mutating the
                # runtime manifest. This is how the upstream reference
                # implementation does it.
                generation_defaults = runtime.manifest["generation_defaults"]
                resolved_mode = _normalize_sample_mode(sample_mode, raw_do_sample=do_sample)
                generation_defaults["max_new_frames"] = max_new_frames
                generation_defaults["sample_mode"] = resolved_mode
                generation_defaults["do_sample"] = resolved_mode != "greedy"
                generation_defaults["audio_temperature"] = float(audio_temperature)
                generation_defaults["audio_top_p"] = float(audio_top_p)
                generation_defaults["audio_top_k"] = int(audio_top_k)
                generation_defaults["audio_repetition_penalty"] = float(audio_repetition_penalty)
                if seed is not None:
                    runtime.rng = np.random.default_rng(int(seed))

                prompt_codes = runtime.resolve_prompt_audio_codes(
                    voice=voice, prompt_audio_path=prompt_audio_path,
                )
                text_chunks = runtime.split_voice_clone_text(
                    text, max_tokens=voice_clone_max_text_tokens,
                )
                sample_rate = int(runtime.codec_meta["codec_config"]["sample_rate"])
                channels = int(runtime.codec_meta["codec_config"]["channels"])

                emitted_samples_total = 0
                first_audio_emitted_at_perf: Optional[float] = None

                def _emit_waveform(waveform: np.ndarray, *, is_pause: bool) -> None:
                    nonlocal emitted_samples_total, first_audio_emitted_at_perf
                    audio_length = int(waveform.shape[0])
                    if first_audio_emitted_at_perf is None and not is_pause:
                        first_audio_emitted_at_perf = time.perf_counter()
                    emitted_samples_total += audio_length
                    event_queue.put({
                        "waveform": np.asarray(waveform, dtype=np.float32),
                        "sample_rate": sample_rate,
                        "channels": channels,
                        "is_pause": bool(is_pause),
                    })

                def _decode_pending(pending: list[list[int]], force: bool) -> None:
                    # Reads `emitted_samples_total` / `first_audio_emitted_at_perf`
                    # from enclosing scope; only `_emit_waveform` mutates them.
                    pending_count = len(pending)
                    if pending_count <= 0:
                        return
                    budget = _resolve_stream_decode_frame_budget(
                        emitted_samples_total,
                        sample_rate,
                        first_audio_emitted_at_perf,
                    )
                    if not force and pending_count < max(1, budget):
                        return
                    frame_budget = pending_count if force else min(pending_count, max(1, budget))
                    frame_chunk = pending[:frame_budget]
                    del pending[:frame_budget]
                    decoded = runtime.codec_streaming_session.run_frames(frame_chunk)
                    if decoded is None:
                        return
                    audio, audio_length = decoded
                    if audio_length <= 0:
                        return
                    waveform = _merge_audio_channels(
                        [audio[0, c, :audio_length] for c in range(audio.shape[1])]
                    )
                    _emit_waveform(waveform, is_pause=False)

                for chunk_idx, chunk_text in enumerate(text_chunks):
                    text_token_ids = runtime.encode_text(chunk_text)
                    rows = runtime.build_voice_clone_request_rows(prompt_codes, text_token_ids)
                    pending: list[list[int]] = []
                    runtime.codec_streaming_session.reset()

                    def _on_frame(_g, _i, frame, _pending=pending):
                        # Default-arg binds `pending` at definition time so
                        # each loop iteration's _on_frame closes over the
                        # correct list.
                        _pending.append(list(frame))
                        _decode_pending(_pending, False)

                    try:
                        runtime.generate_audio_frames(rows, on_frame=_on_frame)
                        _decode_pending(pending, True)  # flush any stragglers
                    finally:
                        runtime.codec_streaming_session.reset()

                    # Inter-chunk pause.
                    if chunk_idx < len(text_chunks) - 1:
                        pause_secs = runtime.estimate_voice_clone_inter_chunk_pause_seconds(chunk_text)
                        pause_samples = max(0, int(round(sample_rate * pause_secs)))
                        if pause_samples > 0:
                            silence = (
                                np.zeros((pause_samples, channels), dtype=np.float32)
                                if channels > 1
                                else np.zeros(pause_samples, dtype=np.float32)
                            )
                            _emit_waveform(silence, is_pause=True)
            except Exception as exc:
                logger.exception("stream synthesis failed")
                event_queue.put({"type": "error", "error": str(exc)})
            finally:
                event_queue.put(None)  # sentinel

        worker = threading.Thread(target=_worker, name="moss-tts-stream", daemon=True)
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
        # The daemon thread will keep running until it tries to put() into the
        # full queue (backpressure) or finishes naturally; either way it
        # can't crash the process.
        worker.join(timeout=2.0)

    return StreamingResponse(gen(), media_type="application/x-ndjson")


# ---------------------------------------------------------------------------
# Lifecycle
# ---------------------------------------------------------------------------
@app.on_event("startup")
def _startup():
    global runtime, normalizer
    if _HAVE_WETEXT:
        try:
            normalizer = WeTextProcessingManager()
            normalizer.start()
        except Exception as e:  # pragma: no cover
            logger.warning("WeTextProcessingManager start failed: %s", e)

    model_dir = os.environ.get("MOSS_TTS_MODEL_DIR")
    threads = int(os.environ.get("MOSS_TTS_CPU_THREADS", "4"))
    runtime = OnnxTtsRuntime(
        model_dir=model_dir,
        thread_count=threads,
        max_new_frames=375,
    )
    # Warmup (optional, but makes the first real request much faster).
    # NOTE: Do NOT pass `max_new_frames` here — `synthesize()` writes it
    # permanently into `runtime.manifest["generation_defaults"]["max_new_frames"]`,
    # which would cap all subsequent streaming synthesis to that many frames
    # (truncating long text to ~1s of audio). Let the constructor value (375)
    # survive.
    try:
        first_voice = runtime.list_builtin_voices()[0]["voice"]
        runtime.synthesize(
            text="warmup",
            voice=first_voice,
            sample_mode="fixed",
            voice_clone_max_text_tokens=75,
        )
        logger.info("MOSS-TTS-Nano warmup complete")
    except Exception as e:  # pragma: no cover
        logger.warning("warmup failed (non-fatal): %s", e)


def main():
    parser = argparse.ArgumentParser(description="MOSS-TTS-Nano ONNX HTTP service")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9382)
    parser.add_argument("--model-dir", default=None,
                        help="browser_onnx model directory. If omitted, "
                             "OnnxTtsRuntime auto-downloads to ./models.")
    parser.add_argument("--cpu-threads", type=int, default=4)
    args = parser.parse_args()

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        level=logging.INFO,
    )

    if args.model_dir:
        os.environ["MOSS_TTS_MODEL_DIR"] = args.model_dir
    os.environ.setdefault("MOSS_TTS_CPU_THREADS", str(args.cpu_threads))

    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port, log_level="info")


if __name__ == "__main__":
    main()
