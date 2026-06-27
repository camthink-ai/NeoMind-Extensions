#!/usr/bin/env bash
# Start the CosyVoice 3 Python service.
#
# Prerequisites:
#   - Python 3.10+ (3.12 recommended)
#   - pip install -r requirements.txt
#   - First run downloads ~2GB model from ModelScope into ~/.cache/modelscope/
#
# Environment:
#   COSYVOICE_MODEL_DIR  ModelScope ID or local path (default FunAudioLLM/Fun-CosyVoice3-0.5B-2512)
#   COSYVOICE_HOST        HTTP bind host (default 127.0.0.1)
#   COSYVOICE_PORT        HTTP bind port (default 9385)
#   PYTORCH_ENABLE_MPS_FALLBACK  set to 1 for Apple Silicon (auto-set below)
set -euo pipefail

export COSYVOICE_MODEL_DIR="${COSYVOICE_MODEL_DIR:-FunAudioLLM/Fun-CosyVoice3-0.5B-2512}"

# CosyVoice repo provides `cosyvoice.cli.cosyvoice.AutoModel`. Look it up
# under $COSYVOICE_REPO (default ~/CosyVoice). Add to PYTHONPATH without
# clobbering what's already there. Matcha-TTS (submodule) must also be on
# PYTHONPATH because CosyVoice's flow_matching imports `matcha.*`.
export COSYVOICE_REPO="${COSYVOICE_REPO:-$HOME/CosyVoice}"
if [ -d "$COSYVOICE_REPO" ]; then
  MATCHA_DIR="$COSYVOICE_REPO/third_party/Matcha-TTS"
  export PYTHONPATH="${COSYVOICE_REPO}${MATCHA_DIR:+:$MATCHA_DIR}${PYTHONPATH:+:$PYTHONPATH}"
else
  echo "ERROR: CosyVoice repo not found at $COSYVOICE_REPO" >&2
  echo "Clone it: git clone https://github.com/FunAudioLLM/CosyVoice.git \"\$COSYVOICE_REPO\"" >&2
  echo "Then init submodule: git -C \"\$COSYVOICE_REPO\" submodule update --init --depth 1" >&2
  exit 1
fi

# Apple Silicon: fall back to CPU for ops the MPS backend doesn't support.
# Harmless on Linux/Windows (env is just ignored by torch).
export PYTORCH_ENABLE_MPS_FALLBACK="${PYTORCH_ENABLE_MPS_FALLBACK:-1}"

HOST="${COSYVOICE_HOST:-127.0.0.1}"
PORT="${COSYVOICE_PORT:-9385}"

exec python "$(dirname "$0")/server.py" --host "$HOST" --port "$PORT" \
    --model-dir "$COSYVOICE_MODEL_DIR"
