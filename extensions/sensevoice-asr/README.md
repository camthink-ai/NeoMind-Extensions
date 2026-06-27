# sensevoice-asr

Multilingual speech-to-text extension wrapping
[SenseVoice-Small](https://github.com/FunAudioLLM/SenseVoice) via the
`sherpa-onnx` ONNX CPU backend (234M params INT8, 5 languages: zh, en,
ja, ko, yue). Designed for the NeoMind Edge AI Platform.

The extension exposes four commands:

| Command          | Description                                                       | Use case                                            |
|------------------|-------------------------------------------------------------------|-----------------------------------------------------|
| `transcribe`     | Transcribe audio from a file path or base64 WAV; returns text.    | Voice input for AI agents, transcription UIs.       |
| `transcribe_file`| Convenience: transcribe a local file by path only.                | Pre-recorded audio pipelines.                       |
| `health`         | Ping the Python service.                                          | Monitoring.                                         |
| `languages`      | List supported language hints.                                    | UI dropdowns.                                       |

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  NeoMind host process                                         │
│   └─ sensevoice-asr (Rust cdylib)                             │
│        │  ureq HTTP (sync, wrapped in spawn_blocking)         │
│        ▼                                                       │
│      ┌────────────────────────────────────────────┐           │
│      │ Python FastAPI service  (separate process) │           │
│      │   · sherpa_onnx.OfflineRecognizer           │          │
│      │   · POST /asr         → full JSON           │           │
│      │   · POST /asr/stream  → NDJSON pseudo-stream│           │
│      │   · GET  /languages /health                 │           │
│      └────────────────────────────────────────────┘           │
└──────────────────────────────────────────────────────────────┘
```

The Rust extension and the Python service run as **separate processes**.
All HTTP calls are blocking (`ureq` v3) and wrapped in
`tokio::task::spawn_blocking` so they never stall the async executor.

## Performance (Apple Silicon M2, 2 threads, INT8)

From the benchmark run on this machine:

| Metric              | Value             |
|---------------------|-------------------|
| Load time           | ~0.46 s           |
| RAM delta           | +740 MB           |
| Avg RTF             | 0.017 (1/60× RT)  |
| Avg CER (Mandarin)  | 0.034 – 0.182     |

SenseVoice is offline (batch), so latency ≈ `RTF × audio_duration`.
For a 10 s clip, expect ~0.2 s decode time.

## Setup

### 1. Python service

```bash
cd extensions/sensevoice-asr/service
pip install -r requirements.txt
./start.sh
```

The first run downloads ~230 MB of ONNX weights into
`~/.cache/sherpa-onnx/`.

Smoke test:

```bash
curl http://127.0.0.1:9383/health
curl -X POST http://127.0.0.1:9383/asr \
  -H 'Content-Type: application/json' \
  -d '{"audio_path":"/tmp/test.wav","language":"auto"}'
```

### 2. Build the extension

```bash
# Dev build + auto-install to ~/.neomind/extensions/
./build.sh --dev --single sensevoice-asr

# Or just compile:
cargo build --release -p sensevoice-asr
```

## Usage

### From a NeoMind Agent / rule

```jsonc
// Transcribe a local file:
{
  "extension": "sensevoice-asr",
  "command": "transcribe",
  "args": {
    "audio_path": "/tmp/recording.wav",
    "language": "auto"
  }
}
// → { "text": "你好，今天天气怎么样？", "language": "auto",
//     "elapsed_seconds": 0.21, "duration_seconds": 12.5, "rtf": 0.017 }

// Transcribe base64 WAV bytes (e.g. from a browser mic recording):
{
  "extension": "sensevoice-asr",
  "command": "transcribe",
  "args": {
    "audio_base64": "<base64-encoded WAV>",
    "language": "zh"
  }
}
```

### Environment variables

| Variable                      | Default                              | Description                              |
|-------------------------------|--------------------------------------|------------------------------------------|
| `SENSEVOICE_ASR_SERVICE_URL`  | `http://127.0.0.1:9383`              | Python service base URL.                 |
| `SENSEVOICE_ASR_LANGUAGE`     | `auto`                               | Default language hint.                   |
| `SENSEVOICE_ASR_MODEL_DIR`    | `~/.cache/sherpa-onnx`               | ONNX weights dir.                        |
| `SENSEVOICE_ASR_CPU_THREADS`  | `2`                                  | ONNX Runtime intra-op thread count.      |

## Metrics

| Name                     | Type    | Description                                  |
|--------------------------|---------|----------------------------------------------|
| `service_ok`             | bool    | Python service reachable at last call.       |
| `total_requests`         | int     | Lifetime ASR request count.                  |
| `last_latency_ms`        | float   | Wall-clock latency of last decode (ms).      |
| `last_audio_duration_ms` | float   | Duration of last decoded audio (ms).         |
| `rtf`                    | float   | Real-time factor = latency / duration.       |

## Limitations / TODO

- **Offline model**: SenseVoice is batch-only. `/asr/stream` emits a
  single final result rather than incremental partials. If true
  streaming is needed, swap the backend to Streaming Zipformer or
  Paraformer-streaming — the HTTP API is already shaped for it.
- **No microphone capture in this extension**: pair with a UI component
  (browser `getUserMedia`) or a future `audio-capture` extension to feed
  mic input via `audio_base64`.
- **No automatic LLM integration yet**: the Voice Assistant pipeline
  (ASR → Agent → TTS) is wired at the orchestration layer, not here.
