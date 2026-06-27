#!/usr/bin/env bash
# Qwen3-ASR service (mlx-community/Qwen3-ASR-0.6B-8bit) — Apple Silicon only.
# /asr contract identical to sensevoice-asr on port 9383.
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"

export QWEN3_ASR_HOST="${QWEN3_ASR_HOST:-127.0.0.1}"
export QWEN3_ASR_PORT="${QWEN3_ASR_PORT:-9383}"
export QWEN3_ASR_MODEL="${QWEN3_ASR_MODEL:-mlx-community/Qwen3-ASR-0.6B-8bit}"

exec python "$DIR/asr_qwen3.py" --host "$QWEN3_ASR_HOST" --port "$QWEN3_ASR_PORT"
