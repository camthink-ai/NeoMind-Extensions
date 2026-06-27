# moss-tts-nano

Voice-cloning TTS extension wrapping the
[MOSS-TTS-Nano](https://github.com/OpenMOSS/MOSS-TTS-Nano) ONNX CPU backend
(0.1B parameters, 48 kHz stereo, 20+ languages). Designed for the NeoMind
Edge AI Platform.

The extension exposes two primary commands:

| Command         | Description                                                              | Use case                                            |
|-----------------|--------------------------------------------------------------------------|-----------------------------------------------------|
| `speak`         | Stream PCM from the Python service and play directly on the host audio. | Edge devices, kiosks, Agent voice replies. No UI.   |
| `synthesize`    | Call the service, return full WAV as base64 (no playback).              | UI cards, recording, forwarding, post-processing.   |
| `stop_speaking` | Stop current playback immediately.                                       | Interrupt a long utterance.                         |
| `list_voices`   | List built-in voice presets.                                             | Voice pickers.                                      |
| `health`        | Ping the Python service.                                                 | Monitoring.                                         |

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  NeoMind host process                                         │
│   └─ moss-tts-nano (Rust cdylib, isolated subprocess)         │
│        │  ureq HTTP (sync, wrapped in spawn_blocking)          │
│        ▼                                                       │
│      ┌────────────────────────────────────────────┐           │
│      │ Python FastAPI service  (separate process) │           │
│      │   · OnnxTtsRuntime (MOSS-TTS-Nano-100M-ONNX)│          │
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

The Rust extension and the Python service run as **separate processes**.
All HTTP calls are blocking (`ureq` v3) and wrapped in
`tokio::task::spawn_blocking` so they never stall the async executor.
Audio playback happens on a dedicated audio thread (see `audio_thread` in
`src/lib.rs`) that owns the `rodio::OutputStream` for the process
lifetime.

## Setup

### 1. Python service

Prerequisites: Python 3.10+ (3.12 recommended). Conda recommended for
pynini/WeTextProcessing.

```bash
# Clone the upstream MOSS-TTS-Nano repo and install it in editable mode.
git clone https://github.com/OpenMOSS/MOSS-TTS-Nano.git ~/MOSS-TTS-Nano
cd ~/MOSS-TTS-Nano

# If using conda (recommended for pynini):
conda install -c conda-forge pynini=2.1.6.post1 -y
pip install git+https://github.com/WhizZest/WeTextProcessing.git

pip install -r requirements.txt
pip install -e .
```

Then start the service:

```bash
cd extensions/moss-tts-nano/service
MOSS_TTS_NANO_REPO=~/MOSS-TTS-Nano ./start.sh
```

The first run downloads ~200 MB of ONNX weights into
`$MOSS_TTS_NANO_REPO/models/`.

Smoke test:

```bash
curl http://127.0.0.1:9382/health
curl -X POST http://127.0.0.1:9382/tts \
  -H 'Content-Type: application/json' \
  -d '{"text":"你好","voice":"Junhao"}' \
  --output a.wav
```

### 2. Build the extension

```bash
# Dev build + auto-install to ~/.neomind/extensions/
./build.sh --dev --single moss-tts-nano

# Or just compile:
cargo build --release -p moss-tts-nano
```

### 3. Docker

```bash
docker build -t neomind/moss-tts-nano-service extensions/moss-tts-nano/service
docker run --rm -p 9382:9382 \
  -v "$HOME/MOSS-TTS-Nano/models:/models" \
  -e MOSS_TTS_MODEL_DIR=/models \
  neomind/moss-tts-nano-service
```

## Usage

### From a NeoMind Agent / rule

```jsonc
// Speak directly on the host device (blocks until playback finishes):
{
  "extension": "moss-tts-nano",
  "command": "speak",
  "args": {
    "text": "你好，欢迎使用 NeoMind。",
    "voice": "Junhao",
    "blocking": true
  }
}

// Background playback (return immediately):
{
  "extension": "moss-tts-nano",
  "command": "speak",
  "args": { "text": "报警：温度过高", "blocking": false }
}

// Stop an in-progress utterance:
{ "extension": "moss-tts-nano", "command": "stop_speaking" }

// Voice cloning with a reference audio file:
{
  "extension": "moss-tts-nano",
  "command": "speak",
  "args": {
    "text": "用我的声音说话。",
    "prompt_audio_path": "/path/to/reference.wav"
  }
}

// Get WAV bytes for UI playback or recording:
{
  "extension": "moss-tts-nano",
  "command": "synthesize",
  "args": { "text": "保存这一段" }
}
// → { "audio_base64": "...", "format": "wav",
//     "sample_rate": 48000, "duration_ms": 1834, "size_bytes": 351232 }
```

### Environment variables

| Variable                 | Default                            | Description                                |
|--------------------------|------------------------------------|--------------------------------------------|
| `MOSS_TTS_SERVICE_URL`   | `http://127.0.0.1:9382`            | Python service base URL.                   |
| `MOSS_TTS_VOICE`         | `Junhao`                           | Default voice preset.                      |
| `MOSS_TTS_NANO_REPO`     | `~/MOSS-TTS-Nano`                  | Path to the MOSS-TTS-Nano repo (Python).   |
| `MOSS_TTS_MODEL_DIR`     | `$MOSS_TTS_NANO_REPO/models`       | ONNX weights dir.                          |
| `MOSS_TTS_CPU_THREADS`   | `4`                                | ONNX Runtime intra-op thread count.        |

## Metrics

| Name                     | Type    | Description                                  |
|--------------------------|---------|----------------------------------------------|
| `service_ok`             | bool    | Python service reachable at last call.       |
| `total_requests`         | int     | Lifetime TTS request count.                  |
| `last_latency_ms`        | float   | Wall-clock latency of last synthesis (ms).   |
| `last_audio_duration_ms` | float   | Duration of last generated audio (ms).       |
| `rtf`                    | float   | Real-time factor = latency / duration.       |

## Platform support

Audio playback via `rodio`:

| Platform | Backend             | Notes                                  |
|----------|---------------------|----------------------------------------|
| macOS    | CoreAudio           | Works out of the box.                  |
| Linux    | ALSA (libasound2)   | Install `libasound2-dev` at build time and `libasound2` at runtime. |
| Windows  | WASAPI              | Works out of the box.                  |

On platforms without a `rodio` backend (e.g. wasm), `speak` returns an
error from the audio thread; `synthesize` still works for callers that
want the raw WAV bytes.

## Limitations / TODO

- `/tts/stream` now uses true per-frame streaming (queue+thread, adaptive
  batching 1→2→4→8 frames). First-byte latency on CPU is ~70ms for short
  replies, suitable for real-time conversation.
- **Single-request-at-a-time** on `/tts/stream`: the worker mutates the
  global `runtime.manifest["generation_defaults"]` per request. Concurrent
  streaming requests would clobber each other. Fine for one-user voice
  assistant; document if extending to multi-tenant.
- No `StreamCapability` integration yet. If needed (for UI streaming via
  NeoMind's WebSocket `extension-stream` client), add a push-mode
  capability with `StreamDataType::Audio { format: "pcm_int16_le",
  sample_rate: 48000, channels: 2 }` and bridge the NDJSON stream into
  `PushOutputMessage` events.
- Single global audio thread serializes all `speak` calls. Adequate for
  TTS; not designed for mixing multiple parallel utterances.
