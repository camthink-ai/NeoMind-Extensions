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
import logging
import os
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
