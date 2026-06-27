# cosyvoice-3

Streaming TTS extension wrapping [Fun-CosyVoice3-0.5B-2512](https://www.modelscope.cn/models/FunAudioLLM/Fun-CosyVoice3-0.5B-2512)
(0.5B parameters, 24 kHz mono). Targets ~150 ms first-chunk latency and
<500 ms per-sentence synthesis — roughly 10× faster wall-clock than
moss-tts-nano for typical 30-character utterances.

This extension is the **drop-in replacement** for `moss-tts-nano` in the
voice-assistant pipeline. The Python service exposes the same
`/tts/stream` NDJSON contract, so `voice-assistant` switches backends by
changing one env var (`MOSS_TTS_URL=http://127.0.0.1:9385`).

| Command         | Description                                                              | Use case                                            |
|-----------------|--------------------------------------------------------------------------|-----------------------------------------------------|
| `speak`         | Stream PCM from the Python service and play directly on the host audio. | Edge devices, kiosks, Agent voice replies. No UI.   |
| `synthesize`    | Call the service, return full WAV as base64 (no playback).              | UI cards, recording, forwarding, post-processing.   |
| `stop_speaking` | Stop current playback immediately.                                       | Interrupt a long utterance.                         |
| `list_voices`   | List voice presets from the service.                                     | Voice pickers.                                      |
| `health`        | Ping the Python service.                                                 | Monitoring.                                         |

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  NeoMind host process                                         │
│   └─ cosyvoice-3 (Rust cdylib, isolated subprocess)           │
│        │  ureq HTTP (sync, wrapped in spawn_blocking)          │
│        ▼                                                       │
│      ┌────────────────────────────────────────────┐           │
│      │ Python FastAPI service  (separate process) │           │
│      │   · AutoModel('Fun-CosyVoice3-0.5B-2512')  │           │
│      │   · /tts         → full WAV                 │           │
│      │   · /tts/stream  → NDJSON PCM chunks        │           │
│      │   · /voices /health                         │           │
│      └────────────────────────────────────────────┘           │
│   ▲                                                           │
│   │ rodio (cpal)                                              │
│   ▼                                                           │
│  Host audio device (only used by `speak`)                     │
└──────────────────────────────────────────────────────────────┘
```

### NDJSON contract (identical to moss-tts-nano)

```
POST /tts/stream
Body: {"text": "...", "voice": "...", ...}
Response (one line per PCM chunk):
  {"seq": 0, "data": "<base64 int16 LE PCM>",
   "sample_rate": 24000, "channels": 1, "is_pause": false}
  ...
```

## Setup

### 1. Python service

Prerequisites: Python 3.10+ (3.12 recommended).

```bash
cd extensions/cosyvoice-3/service
pip install -r requirements.txt
./start.sh
```

First run downloads ~2GB model from ModelScope into
`~/.cache/modelscope/` (5-10 min depending on bandwidth). Subsequent
starts load from cache in ~5-10 s.

Smoke test:

```bash
curl http://127.0.0.1:9385/health
curl -X POST http://127.0.0.1:9385/tts/stream \
  -H 'Content-Type: application/json' \
  -d '{"text":"你好，这是一个测试","voice":"中文女"}'
```

### 2. Build the Rust extension

```bash
# Dev build + auto-install to ~/.neomind/extensions/
./build.sh --dev --single cosyvoice-3

# Or just compile:
cargo build --release -p cosyvoice-3
```

### 3. Wire up voice-assistant

```bash
# Either edit voice-assistant/service/start.sh or set at runtime:
export MOSS_TTS_URL=http://127.0.0.1:9385
export VOICE_ASSISTANT_VOICE=中文女
```

`voice-assistant/service/server.py` also accepts `VOICE_ASSISTANT_TTS_URL`
which takes precedence over `MOSS_TTS_URL`.

### 4. Docker (Linux / CUDA — Jetson Orin, x86 servers)

```bash
docker build -t neomind/cosyvoice-3-service extensions/cosyvoice-3/service
docker run --rm --gpus all -p 9385:9385 \
  -v "$HOME/.cache/modelscope:/root/.cache/modelscope" \
  neomind/cosyvoice-3-service
```

On macOS dev, skip Docker — run `./start.sh` directly with PyTorch + MPS.

## Usage

### From a NeoMind Agent / rule

```jsonc
// Speak directly on the host device:
{
  "extension": "cosyvoice-3",
  "command": "speak",
  "args": {
    "text": "你好，欢迎使用 NeoMind。",
    "voice": "中文女",
    "blocking": true
  }
}

// Zero-shot voice cloning (mimics the prompt speaker):
{
  "extension": "cosyvoice-3",
  "command": "speak",
  "args": {
    "text": "用我的声音说话。",
    "prompt_audio_path": "/path/to/reference.wav",
    "prompt_text": "这段话的内容是参考音频的转写文本。"
  }
}

// Stop an in-progress utterance:
{ "extension": "cosyvoice-3", "command": "stop_speaking" }
```

### Environment variables

| Variable                 | Default                                       | Description                                  |
|--------------------------|-----------------------------------------------|----------------------------------------------|
| `COSYVOICE_SERVICE_URL`  | `http://127.0.0.1:9385`                       | Python service base URL.                     |
| `COSYVOICE_VOICE`        | `中文女`                                       | Default voice preset.                        |
| `COSYVOICE_MODEL_DIR`    | `FunAudioLLM/Fun-CosyVoice3-0.5B-2512`        | ModelScope ID or local path.                 |
| `COSYVOICE_HOST`         | `127.0.0.1`                                   | Service bind host.                           |
| `COSYVOICE_PORT`         | `9385`                                        | Service bind port.                           |
| `PYTORCH_ENABLE_MPS_FALLBACK` | `1`                                       | Auto-fall back to CPU on unsupported MPS ops. |

## Metrics

| Name                     | Type    | Description                                  |
|--------------------------|---------|----------------------------------------------|
| `service_ok`             | bool    | Python service reachable at last call.       |
| `total_requests`         | int     | Lifetime TTS request count.                  |
| `last_latency_ms`        | float   | Wall-clock latency of last synthesis (ms).   |
| `last_audio_duration_ms` | float   | Duration of last generated audio (ms).       |
| `rtf`                    | float   | Real-time factor = latency / duration.       |

## Platform support

| Platform | Backend             | Notes                                  |
|----------|---------------------|----------------------------------------|
| macOS    | CoreAudio / MPS     | Apple Silicon: MPS with CPU fallback.  |
| Linux    | ALSA / CUDA         | GPU servers, Jetson Orin.              |
| Windows  | WASAPI              | Works out of the box.                  |

## Performance targets

| Metric                          | moss-tts-nano (measured) | CosyVoice 3 (target) |
|---------------------------------|--------------------------|----------------------|
| First-chunk latency             | 235-279 ms               | <200 ms              |
| Per-sentence synthesis (30 字)   | 3-8 s                    | <500 ms              |
| 6-sentence reply total          | 35-55 s                  | <5 s                 |

## Limitations / TODO

- **Phase 1 only** (this release): sentence-level invocation, identical
  pipeline shape to moss-tts-nano. CosyVoice 3's native bi-streaming
  (token-in / audio-out) is Phase 2 — it requires rewriting voice-assistant
  to a token-level queue.
- **Single-request-at-a-time** on `/tts/stream`: the model is a shared
  global resource. Concurrent streaming requests would queue. Adequate for
  one-user voice assistant.
- No `StreamCapability` integration yet (same as moss-tts-nano).
