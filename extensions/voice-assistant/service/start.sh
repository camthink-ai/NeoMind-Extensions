#!/usr/bin/env bash
# Start the Voice Assistant orchestrator service.
#
# Default (all-in-one) mode: SenseVoice + ZipVoice run in-process via
# sherpa_onnx. Only NeoMind LLM at ws://127.0.0.1:9375 must be reachable.
# First run downloads ~400MB of models to ~/.cache/sherpa-onnx/.
#
#   pip install -r requirements.txt
#   export NEOMIND_TOKEN=nmk_xxx
#   ./start.sh
#
# Profile override: VOICE_ASSISTANT_PROFILE=edge-arm|noisy-env|headset|...
#   Default is neomind-capability (token-free ChatStream LLM via the host
#   capability path). Set VOICE_ASSISTANT_PROFILE=default to fall back to
#   the in-proc SenseVoice + ZipVoice stack with NeoMind cloud LLM.
# VAD override:     VOICE_ASSISTANT_VAD_BACKEND=silero|energy
set -euo pipefail

export VOICE_ASSISTANT_VAD_BACKEND="${VOICE_ASSISTANT_VAD_BACKEND:-silero}"
export VOICE_ASSISTANT_PROFILE="${VOICE_ASSISTANT_PROFILE:-neomind-capability}"

HOST="${VOICE_ASSISTANT_HOST:-127.0.0.1}"
PORT="${VOICE_ASSISTANT_PORT:-9384}"

exec python "$(dirname "$0")/server.py" --host "$HOST" --port "$PORT"
