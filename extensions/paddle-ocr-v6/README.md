# paddle-ocr-v6

PP-OCRv6 native ONNX inference extension for the NeoMind Edge AI Platform.

## What it does

Exposes pure OCR inference commands — no device binding, no stream wiring.
Upper-layer NeoMind flows (devices, dashboards, AI agents) call this
extension's `recognize` command with image bytes + ROI and get back
recognized text + bounding boxes.

## Commands

| Command       | Description                                                  |
|---------------|--------------------------------------------------------------|
| `recognize`   | Run PP-OCRv6 detection + recognition on an image.            |
| `switch_tier` | Switch model tier: `tiny` / `small` / `medium` / `auto`.     |
| `health`      | Return load status, current tier, models dir, last error.    |

### `recognize` parameters

| Parameter       | Type   | Required | Notes                                   |
|-----------------|--------|----------|-----------------------------------------|
| `image_base64`  | string | no*      | Base64-encoded image bytes (PNG/JPEG).  |
| `image_url`     | string | no*      | HTTP URL to fetch image from.           |

\* One of `image_base64` / `image_url` is required.

### `recognize` response

```json
{
  "text_blocks": [
    {
      "text": "recognized text",
      "confidence": 0.97,
      "bbox": { "x": 0.10, "y": 0.20, "width": 0.30, "height": 0.05 },
      "polygon": [[0.10, 0.20], [0.40, 0.20], [0.40, 0.25], [0.10, 0.25]]
    }
  ],
  "full_text": "recognized text",
  "total_blocks": 1,
  "avg_confidence": 0.97,
  "processing_time_ms": 42,
  "image_width": 1920,
  "image_height": 1080,
  "tier": "tiny"
}
```

All bbox / polygon coordinates are **normalized to `[0, 1]`** relative to
the source image dimensions, so overlays render correctly without knowing
pixel dimensions.

## Tiers

| Tier    | Size    | Notes                                              |
|---------|---------|----------------------------------------------------|
| `tiny`  | ~6 MB   | Ships inside the .nep. No Japanese; fast CPU.      |
| `small` | ~18 MB  | Lazy-downloaded on first switch. CoreML/CUDA rec.  |
| `medium`| ~132 MB | Lazy-downloaded. Highest accuracy. CUDA + ≥16GB.   |
| `auto`  | —       | Pick based on host: CUDA+RAM→medium, GPU→small, else tiny. |

## Models

Tiny tier is bundled in `models/`. Small/medium are lazy-downloaded
from HuggingFace `PaddlePaddle/PP-OCRv6_<tier>_det_onnx` on first
`switch_tier` call.

To pre-cache other tiers at build time:

```bash
./download_models.sh small    # or medium / all
```

## PP-OCRv6 vs v5 preprocessing

These overrides are encoded in `src/preset.rs` and pinned by unit tests:

- `swap_rgb=true` — v6 trained on BGR; usls default forces RGB.
- `normalize=false` — v6 rec expects raw `[0,255]` pixels (v5 wanted `[0,1]`).
- `unclip_ratio=1.4` — v6 YAML value (usls default is 1.5).
- `box_thresh` — 0.40 tiny / 0.45 others.

## Architecture

```
OcrEngine
├── detector:  usls::models::DB       (text region detection)
├── recognizer: usls::models::SVTR    (single multilingual recognizer)
├── tier:      Tier                   (current loaded tier)
└── downloader: Downloader            (lazy HF fetch)

PaddleOcrV6Extension
└── engine: Arc<RwLock<OcrEngine>>    (single engine, tier-switch reloads)
```

This extension does NOT bind to NeoMind devices — that's the upper
layer's job. It only exposes pure inference commands.

## Development

```bash
cargo test -p paddle-ocr-v6 --lib     # 24 unit tests
cargo build --release -p paddle-ocr-v6
./build.sh --single paddle-ocr-v6     # produce .nep
```
