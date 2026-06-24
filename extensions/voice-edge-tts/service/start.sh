#!/usr/bin/env bash
# voice-edge-tts launcher — sherpa-onnx ZipVoice HTTP service.
#
# Cross-platform: runs identically on macOS (dev) and Linux ARM64 (prod).
# First start downloads ~150MB ZipVoice model to ~/.cache/sherpa-onnx.
set -euo pipefail

HOST="${VOICE_EDGE_TTS_HOST:-127.0.0.1}"
PORT="${VOICE_EDGE_TTS_PORT:-9386}"

# Apple Silicon: harmless on Linux. sherpa-onnx is pure CPU/ONNX so no
# MPS-specific env is actually needed (unlike CosyVoice PyTorch).
export PYTORCH_ENABLE_MPS_FALLBACK="${PYTORCH_ENABLE_MPS_FALLBACK:-1}"

DIR="$(cd "$(dirname "$0")" && pwd)"
exec python "$DIR/server.py" --host "$HOST" --port "$PORT"
