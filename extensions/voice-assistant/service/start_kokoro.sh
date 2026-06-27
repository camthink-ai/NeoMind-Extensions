#!/usr/bin/env bash
# Kokoro TTS service (mlx-audio) — Apple Silicon only.
# NDJSON /tts/stream contract identical to moss-tts-nano on port 9385.
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"

export KOKORO_HOST="${KOKORO_HOST:-127.0.0.1}"
export KOKORO_PORT="${KOKORO_PORT:-9385}"
export KOKORO_MODEL="${KOKORO_MODEL:-prince-canuma/Kokoro-82M}"
export KOKORO_VOICE="${KOKORO_VOICE:-zf_xiaoxiao}"
export KOKORO_LANG_CODE="${KOKORO_LANG_CODE:-zh}"

exec python "$DIR/tts_kokoro.py" --host "$KOKORO_HOST" --port "$KOKORO_PORT"
