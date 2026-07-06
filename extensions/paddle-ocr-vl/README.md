# paddle-ocr-vl

Bridges NeoMind with **PaddleOCR-VL 1.6** for high-accuracy document parsing:
text recognition, table extraction, and full Markdown reconstruction.

## Architecture

This extension is an HTTP client (sync `ureq`). The actual PaddleOCR-VL model
runs in a separate Python service. This keeps the extension cross-platform and
lightweight (~MB binary), while the heavy VLM inference happens on a GPU
server.

```
[NeoMind device]              [PaddleOCR-VL service]
  paddle-ocr-vl ext ──HTTP──▶  FastAPI + PaddleOCRVL
                               (Linux + NVIDIA GPU recommended)
```

## Backend setup (`server/`)

### 1. Install PaddlePaddle + PaddleOCR

```bash
cd extensions/paddle-ocr-vl/server

# Create a clean venv (PaddlePaddle has narrow dep constraints)
python -m venv .venv_paddleocr
source .venv_paddleocr/bin/activate

# CPU (macOS / x86 Linux)
pip install paddlepaddle==3.2.1

# OR NVIDIA GPU (CUDA 12.6)
pip install paddlepaddle-gpu==3.2.1 \
  -i https://www.paddlepaddle.org.cn/packages/stable/cu126/

# Then the rest
pip install -r requirements.txt
```

### 2. (Optional) Pre-download model weights

PaddleOCR-VL auto-downloads ~1-2 GB on first inference. Pre-warm the cache:

```bash
./download_models.sh
```

Models cache to `~/.paddlex/`.

### 3. Run the server

```bash
python3 server.py
# → http://0.0.0.0:8000
# Override host/port/device via env:
#   HOST=127.0.0.1 PORT=9000 PADDLE_DEVICE=gpu python3 server.py
```

### 4. (For quick UI testing) Run the mock server

If you don't have a GPU/CPU beefy enough for the full VLM, run the mock
backend — it returns canned responses so you can verify the wiring:

```bash
python3 mock_server.py
```

## Extension commands

| Command       | Endpoint | Returns                          |
|---------------|----------|----------------------------------|
| `recognize`   | `/ocr`   | `text_blocks` + `full_text`      |
| `recognize_table` | `/table` | `html`                       |
| `extract_keys`| `/kie`   | `fields` (best-effort)           |
| `health`      | `/health`| status / model_loaded            |

### `text_blocks` shape (consumed by ne101_camera's `ocr_text_blocks` mode)

```json
{
  "text_blocks": [
    { "text": "Hello", "confidence": 0.97,
      "bbox": { "x": 0.05, "y": 0.20, "width": 0.50, "height": 0.60 } }
  ],
  "full_text": "Hello\nWorld",
  "processing_time_ms": 420,
  "language": "ch"
}
```

`bbox` is normalized to `[0, 1]`.

## Configuration parameters

| Parameter                       | Default                  | Description                          |
|---------------------------------|--------------------------|--------------------------------------|
| `endpoint`                      | `http://127.0.0.1:8000`  | Base URL of the service              |
| `language`                      | `ch`                     | Language hint (ch/en/japan/...)      |
| `use_doc_orientation_classify`  | `false`                  | Auto-rotate before OCR               |
| `use_doc_unwarping`             | `false`                  | Dewarp curved documents              |
| `timeout_ms`                    | `30000`                  | HTTP timeout (1s–120s)               |

## Cross-platform support

The **extension** compiles cleanly on all 6 NeoMind targets (darwin/linux/
windows × amd64/arm64/x86) — pure Rust + `ureq`, no native ML deps.

The **backend service** requires PaddlePaddle:
- ✅ Linux x86_64 (CUDA) — production recommended
- ✅ Linux ARM64 (Jetson, with JetPack)
- ✅ Windows (CUDA)
- ⚠️ macOS Apple Silicon — CPU only, VLM is slow
- ⚠️ macOS Intel — CPU only, not practical

For pure-edge deployments without a GPU server, prefer `ocr-device-inference`
(SVTR local ONNX, ~tens of MB, CPU-friendly).

## Files

```
extensions/paddle-ocr-vl/
├── Cargo.toml
├── src/lib.rs                    # Rust extension (HTTP client)
├── frontend/
│   ├── frontend.json             # Component registration
│   └── src/PaddleOcrCard.tsx     # Minimal tester card
├── server/
│   ├── server.py                 # Real PaddleOCR-VL FastAPI service
│   ├── mock_server.py            # Canned-response mock for testing
│   ├── download_models.sh        # Pre-warm model cache
│   └── requirements.txt
└── README.md                     # This file
```

## Build & test

```bash
# Build + install
./build.sh --dev --single paddle-ocr-vl

# Release package
./build.sh --release 2.7.7 --single paddle-ocr-vl

# Rust unit tests
cargo test -p paddle-ocr-vl --lib
```

## License

Apache-2.0
