# voice-edge-tts

Cross-platform CPU TTS extension for NeoMind. Wraps `sherpa-onnx` ZipVoice
(zero-shot, ZH+EN) behind the same NDJSON `/tts/stream` contract as
`moss-tts-nano` and `cosyvoice-3`, so `voice-assistant` can switch backends
by changing one env var.

## Why

|                  | moss-tts-nano | cosyvoice-3      | **voice-edge-tts** |
|------------------|---------------|------------------|--------------------|
| Mac CPU RTF      | good          | 2.5x (unusable)  | **<0.5**           |
| Linux ARM CPU    | ✅            | ❌ CUDA-only      | ✅                 |
| ZH quality       | Fair          | Excellent        | Good               |
| Clone            | Yes (MOSS)    | Yes (zero-shot)  | Yes (zero-shot)    |
| Footprint        | ~200MB        | ~1GB             | ~150MB             |

## Quickstart

```bash
cd extensions/voice-edge-tts/service
pip install -r requirements.txt
./start.sh
# First start downloads ~150MB ZipVoice model to ~/.cache/sherpa-onnx
curl http://127.0.0.1:9386/health
# {"status":"ok","sample_rate":24000,"voices":["中文女"]}
```

## Endpoints

- `POST /tts` → full WAV bytes (with `X-Sample-Rate`, `X-Elapsed-Seconds`, `X-Duration-Seconds`, `X-Channels` headers)
- `POST /tts/stream` → NDJSON, one line per PCM chunk:
  ```json
  {"seq": 0, "data": "<base64 int16 LE>", "sample_rate": 24000, "channels": 1, "is_pause": false}
  ```
- `GET /voices` → `{"voices": ["中文女"]}`
- `GET /health` → `{"status": "ok", "sample_rate": 24000, "voices": [...]}`

## Switch voice-assistant to use it

In `extensions/voice-assistant/service/start.sh`:
```bash
export VOICE_ASSISTANT_TTS_URL=http://127.0.0.1:9386
```

## Environment

| Var                       | Default                  | Purpose                          |
|---------------------------|--------------------------|----------------------------------|
| `VOICE_EDGE_TTS_HOST`     | 127.0.0.1                | Bind host                        |
| `VOICE_EDGE_TTS_PORT`     | 9386                     | Bind port                        |
| `VOICE_EDGE_TTS_CPU_THREADS` | 2                     | sherpa-onnx inference threads    |
| `VOICE_EDGE_TTS_MODEL_DIR` | ~/.cache/sherpa-onnx     | Model cache root                 |

## Bundle a custom voice

Replace `service/assets/default_prompt.wav` and `service/assets/default_prompt.txt`.
The transcript MUST match the audio for ZipVoice zero-shot to work.
Recommended: 5-10s clean 16kHz mono clip of the target voice.

## Docker (production)

```bash
# From repo root
docker build -t voice-edge-tts:latest -f extensions/voice-edge-tts/service/Dockerfile .
docker run -p 9386:9386 voice-edge-tts:latest
```

## Tests

```bash
cd extensions/voice-edge-tts/service
python -m pytest test_server.py -v
# 5 tests: health, voices, NDJSON shape, empty-text handling, WAV headers
```
