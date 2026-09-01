#!/bin/bash
# Start LocateAnything API Service
#
# Usage:
#   ./start.sh                    # Download model + start on port 9380
#   ./start.sh --port 9381        # Custom port
#   ./start.sh --model /path/to/model  # Use local model
#
# Environment variables:
#   EAGLE_EMBODIED_PATH  - Path to Eagle/Embodied directory
#   LOCATE_ANYTHING_MODEL - HuggingFace model ID or local path
#   LOCATE_ANYTHING_PORT  - Port number (default: 9380)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXT_DIR="$(dirname "$SCRIPT_DIR")"

# Default Embodied path
EMBODIED_PATH="${EAGLE_EMBODIED_PATH:-}"
if [ -z "$EMBODIED_PATH" ]; then
    # Try to find Eagle/Embodied relative to NeoMind-Extensions
    PARENT_DIR="$(dirname "$EXT_DIR")"
    if [ -d "$PARENT_DIR/Eagle/Embodied" ]; then
        EMBODIED_PATH="$PARENT_DIR/Eagle/Embodied"
    fi
fi

# Check if eagle conda env exists
if conda env list | grep -q "^eagle "; then
    PYTHON="conda run -n eagle python"
else
    PYTHON="python3"
fi

PORT="${LOCATE_ANYTHING_PORT:-9380}"
MODEL="${LOCATE_ANYTHING_MODEL:-nvidia/LocateAnything-3B}"

echo "=== LocateAnything Service ==="
echo "Embodied path: ${EMBODIED_PATH:-not found}"
echo "Model: $MODEL"
echo "Port: $PORT"
echo ""

export EAGLE_EMBODIED_PATH="$EMBODIED_PATH"
export LOCATE_ANYTHING_MODEL="$MODEL"
export PYTORCH_ENABLE_MPS_FALLBACK=1  # Required for MPS on macOS

# Add Embodied to PYTHONPATH so locateanything_worker can be found
if [ -n "$EMBODIED_PATH" ]; then
    export PYTHONPATH="$EMBODIED_PATH:${PYTHONPATH:-}"
fi

cd "$SCRIPT_DIR"
$PYTHON server.py --host 127.0.0.1 --port "$PORT" "$@"
