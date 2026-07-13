# LocateAnything

Visual grounding AI skill for NeoMind — object detection, phrase grounding, OCR, GUI element localization, and pointing via the LocateAnything-3B model.

## Features

- Object detection by category (`detect`)
- Open-vocabulary phrase grounding — locate objects from a natural-language description (`ground`)
- Text detection and localization / OCR (`detect_text`)
- GUI element grounding on screenshots, returning a box or point (`ground_gui`)
- Pointing to a specific described object (`point`)
- Three generation modes: `fast` (MTP, fastest), `slow` (NTP, most stable), `hybrid` (fast with slow fallback)
- Client-side area-ratio filtering and Non-Maximum Suppression on detection responses
- Per-command override of NMS / area-ratio parameters

## Installation

```bash
# Build from repository root
./build.sh --single locate-anything-v2

# Or build with Cargo directly
cargo build --release -p locate-anything-v2
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `check_status` | Check service health and model load | None |
| `detect` | Detect specified object categories in an image | `image_base64` (string, required), `categories` (string, required, comma-separated, e.g. `person,car,bicycle`) |
| `ground` | Locate objects matching a natural-language description | `image_base64` (string, required), `phrase` (string, required), `mode` (string, optional, `single` / `multi`, default `multi`) |
| `detect_text` | Detect and localize all text in an image | `image_base64` (string, required) |
| `ground_gui` | Locate a UI element in a screenshot | `image_base64` (string, required), `phrase` (string, required), `output_type` (string, optional, `box` / `point`, default `box`) |
| `point` | Point to a specific object in an image | `image_base64` (string, required), `phrase` (string, required) |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `service_status` | Service Status | Integer | - | - |
| `total_requests` | Total Requests | Integer | count | - |
| `last_inference_time` | Last Inference Time | Float | ms | - |

## Configuration Parameters

| Parameter | Type | Default | Range | Description |
|-----------|------|---------|-------|-------------|
| `service_url` | String | `http://127.0.0.1:9380` | — | LocateAnything Python service URL |
| `generation_mode` | String | `slow` | `hybrid` / `fast` / `slow` | `fast` = fastest (MTP), `slow` = most stable (NTP), `hybrid` = fast with slow fallback |
| `max_new_tokens` | Integer | `2048` | 128–8192 | Max tokens generated per inference |
| `nms_iou_threshold` | Float | `0.7` | 0.0–1.0 | NMS IoU threshold (lower = more aggressive filtering, 1.0 = no filtering) |
| `min_area_ratio` | Float | `0.0005` | 0.0–0.5 | Min box area as fraction of image area |
| `max_area_ratio` | Float | `0.98` | 0.1–1.0 | Max box area as fraction of image area |

The service URL can also be set via the `LOCATE_ANYTHING_SERVICE_URL` environment variable (the config parameter takes precedence at runtime).

`nms_iou_threshold`, `min_area_ratio`, and `max_area_ratio` can be overridden per command via args; otherwise the configured defaults apply. Area filtering and NMS run on `detect`, `ground`, and `ground_gui` responses; `detect_text` and `point` are returned as-is.

## Requirements

- A running **LocateAnything Python service** (HTTP) that loads the `nvidia/LocateAnything-3B` model
- Reachable at the configured `service_url` (default `http://127.0.0.1:9380`)
- Must expose `/health` plus `/detect`, `/ground`, `/detect_text`, `/ground_gui`, `/point` and return JSON with a `success` field

## License

Apache-2.0
