#!/usr/bin/env bash
# Start the SenseVoice ASR Python service.
#
# Prerequisites:
#   - Python 3.10+
#   - pip install -r requirements.txt
#   - The SenseVoice INT8 ONNX model will auto-download to
#     $SENSEVOICE_ASR_MODEL_DIR on first run (~230 MB).
set -euo pipefail

# Where to store / find the ONNX model.
export SENSEVOICE_ASR_MODEL_DIR="${SENSEVOICE_ASR_MODEL_DIR:-$HOME/.cache/sherpa-onnx}"

# CPU threads for ONNX Runtime.
export SENSEVOICE_ASR_CPU_THREADS="${SENSEVOICE_ASR_CPU_THREADS:-2}"

# Host/port for the HTTP service (the Rust extension connects here by default).
HOST="${SENSEVOICE_ASR_HOST:-127.0.0.1}"
PORT="${SENSEVOICE_ASR_PORT:-9383}"

exec python "$(dirname "$0")/server.py" --host "$HOST" --port "$PORT"
