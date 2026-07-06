# OCR Device Inference

Automatic OCR text recognition bound to device image streams using PP-OCRv4 (DB + SVTR) models, with bounding box drawing and real-time metric output.

## Features

- Bind devices with image data sources for automatic OCR inference on every frame update
- Support for Chinese and English text recognition via language switch
- Draw text bounding boxes on annotated images
- ROI (Region of Interest) polygon filtering -- only recognize text in specified regions
- One-shot image recognition via base64-encoded input
- Real-time metrics: text blocks, confidence scores, inference count
- Auto-detect inference device: CoreML on macOS, CUDA on Linux, CPU fallback

## Installation

```bash
# Build the extension
./build.sh --single ocr-device-inference

# Or build all extensions
./build.sh

# Dev build with auto-install to NeoMind
./build.sh --dev
```

## Commands

| Command | Description | Key Parameters |
|---------|-------------|----------------|
| `bind_device` | Bind a device for automatic OCR on image updates | `device_id`, `device_name`, `image_metric`, `draw_boxes`, `language` |
| `unbind_device` | Unbind a device from OCR inference | `device_id` |
| `toggle_binding` | Enable or disable an existing binding | `device_id`, `active` |
| `get_bindings` | List all current device bindings | -- |
| `recognize_image` | Perform OCR on a base64-encoded image | `image`, `language` |
| `get_status` | Get extension status and statistics | -- |
| `update_roi` | Set ROI polygon regions for a device binding | `device_id`, `roi_regions`, `roi_overlap_threshold` |
| `configure` | Load persisted configuration | -- |

## Metrics

| Metric | Type | Unit | Description |
|--------|------|------|-------------|
| `bound_devices` | Integer | count | Number of currently bound devices |
| `total_inferences` | Integer | count | Total OCR inferences performed |
| `total_text_blocks` | Integer | count | Total text blocks detected |
| `total_errors` | Integer | count | Total inference errors |
| `virtual.ocr.text` | String | json | Latest recognized text blocks (JSON) |
| `virtual.ocr.full_text` | String | text | Concatenated full text |
| `virtual.ocr.count` | Integer | count | Latest text block count |
| `virtual.ocr.confidence` | Float | score | Average confidence (0.0 - 1.0) |

## Frontend Component

**OcrDeviceCard** -- Widget for uploading images for OCR text recognition or managing device bindings. Supports bounding box visualization and recognition result preview. Configurable with `drawBoxes` and `showPreview` options.

## Requirements

- **ONNX Runtime** -- Required for model inference
- **PP-OCRv4 Models** -- Download models before building:

```bash
cd extensions/ocr-device-inference
./download_models.sh
```

Models bundled: `det_mv3_db.onnx` (text detection), `rec_svtr.onnx` (Chinese recognition), `rec_en.onnx` (English recognition), `vocab.txt` (character dictionary), `en_dict.txt` (English dictionary)

## License

Apache-2.0
