#!/usr/bin/env bash
# Start the Voice Assistant orchestrator service.
#
# Prerequisites:
#   - sensevoice-asr service running on port 9383
#   - voice-edge-tts service running on port 9386  (default TTS backend)
#     cosyvoice-3 on port 9385 is supported as opt-in fallback
#     (set VOICE_ASSISTANT_TTS_URL=http://127.0.0.1:9385).
#   - pip install -r requirements.txt
set -euo pipefail

# Default stack: voice-edge-tts (sherpa-onnx ZipVoice) on port 9386 + Silero VAD.
# Override individual pieces by exporting the relevant env var before running.
#   VOICE_ASSISTANT_TTS_URL       — point at a different TTS (e.g. cosyvoice-3 at :9385)
#   VOICE_ASSISTANT_VAD_BACKEND   — energy | fsmn | silero
#   VOICE_ASSISTANT_VOICE         — voice ID the TTS service recognizes
export VOICE_ASSISTANT_VAD_BACKEND="${VOICE_ASSISTANT_VAD_BACKEND:-silero}"
export VOICE_ASSISTANT_TTS_URL="${VOICE_ASSISTANT_TTS_URL:-http://127.0.0.1:9386}"
export VOICE_ASSISTANT_VOICE="${VOICE_ASSISTANT_VOICE:-中文女}"

HOST="${VOICE_ASSISTANT_HOST:-127.0.0.1}"
PORT="${VOICE_ASSISTANT_PORT:-9384}"

exec python "$(dirname "$0")/server.py" --host "$HOST" --port "$PORT"
