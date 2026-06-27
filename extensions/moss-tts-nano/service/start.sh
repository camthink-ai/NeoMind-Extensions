#!/usr/bin/env bash
# Start the MOSS-TTS-Nano Python service.
#
# Prerequisites:
#   - Python 3.10+ (3.12 recommended)
#   - MOSS-TTS-Nano repo cloned and installed in editable mode:
#       git clone https://github.com/OpenMOSS/MOSS-TTS-Nano.git
#       cd MOSS-TTS-Nano
#       pip install -r requirements.txt
#       pip install -e .
#     (If WeTextProcessing / pynini fail to install, use conda:
#        conda install -c conda-forge pynini=2.1.6.post1 -y
#        pip install git+https://github.com/WhizZest/WeTextProcessing.git
#      )
set -euo pipefail

# Where is the MOSS-TTS-Nano repo?
export MOSS_TTS_NANO_REPO="${MOSS_TTS_NANO_REPO:-$HOME/MOSS-TTS-Nano}"
if [ ! -d "$MOSS_TTS_NANO_REPO" ]; then
  echo "ERROR: MOSS-TTS-Nano repo not found at $MOSS_TTS_NANO_REPO" >&2
  echo "Set MOSS_TTS_NANO_REPO=/path/to/MOSS-TTS-Nano or clone it there." >&2
  exit 1
fi

# Where to store downloaded ONNX weights (auto-downloads on first run).
export MOSS_TTS_MODEL_DIR="${MOSS_TTS_MODEL_DIR:-$MOSS_TTS_NANO_REPO/models}"

# CPU threads for ONNX Runtime.
export MOSS_TTS_CPU_THREADS="${MOSS_TTS_CPU_THREADS:-4}"

# Host/port for the HTTP service (the Rust extension connects here by default).
HOST="${MOSS_TTS_HOST:-127.0.0.1}"
PORT="${MOSS_TTS_PORT:-9382}"

exec python "$(dirname "$0")/server.py" --host "$HOST" --port "$PORT"
