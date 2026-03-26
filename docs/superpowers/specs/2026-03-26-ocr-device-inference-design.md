# OCR Device Inference Extension - Design Document

## Overview

**Extension Name:** `ocr-device-inference`
**Display Name:** OCR识别
**Description:** OCR device inference extension with automatic text recognition on data source updates

## Requirements

- Support device binding for automatic OCR inference on image updates
- Support manual image upload for testing
- Multi-language text recognition (Chinese, English, Japanese, Korean, etc.)
- Output results as virtual metrics on devices

## Technical Approach

### OCR Pipeline

Use **DB + SVTR** pipeline from usls library:
- **DB (Differentiable Binarization)**: Text detection model
- **SVTR (Scene Text Recognition)**: Text recognition model

This is the standard OCR workflow that handles arbitrary images with text in various positions.

### Model Files

| Model | File | Size | Purpose |
|-------|------|------|---------|
| DB | `det_mv3_db.onnx` | ~20MB | Text region detection |
| SVTR | `rec_svtr.onnx` | ~40MB | Text recognition |

Total: ~60MB (balanced approach)

## Virtual Metrics

| Metric Name | Type | Description |
|-------------|------|-------------|
| `virtual.ocr.text` | JSON Array | Text blocks with text, confidence, bbox |
| `virtual.ocr.full_text` | String | Merged text (newline separated) |
| `virtual.ocr.count` | Integer | Number of detected text blocks |
| `virtual.ocr.confidence` | Float | Average confidence score |
| `virtual.ocr.inference_time_ms` | Integer | Inference time in milliseconds |
| `virtual.ocr.annotated_image` | String | Base64 encoded annotated image |

## Data Structures

```rust
/// Device binding configuration
pub struct DeviceBinding {
    pub device_id: String,
    pub device_name: Option<String>,
    pub image_metric: String,
    pub result_metric_prefix: String,
    pub draw_boxes: bool,
    pub active: bool,
}

/// Single text block recognition result
pub struct TextBlock {
    pub text: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
    pub language: Option<String>,
}

/// OCR inference result
pub struct OcrResult {
    pub device_id: String,
    pub text_blocks: Vec<TextBlock>,
    pub full_text: String,
    pub total_blocks: usize,
    pub avg_confidence: f32,
    pub inference_time_ms: u64,
    pub image_width: u32,
    pub image_height: u32,
    pub timestamp: i64,
    pub annotated_image_base64: Option<String>,
}

/// Binding status
pub struct BindingStatus {
    pub binding: DeviceBinding,
    pub last_inference: Option<i64>,
    pub total_inferences: u64,
    pub total_text_blocks: u64,
    pub last_error: Option<String>,
    pub last_image: Option<String>,
    pub last_text_blocks: Option<Vec<TextBlock>>,
    pub last_annotated_image: Option<String>,
}
```

## OCR Engine

```rust
/// OCR Pipeline Engine - encapsulates DB detection + SVTR recognition
pub struct OcrEngine {
    detector: Option<Runtime<DB>>,
    recognizer: Option<Runtime<SVTR>>,
    load_error: Option<String>,
}

impl OcrEngine {
    pub fn new() -> Self;
    pub fn init(&mut self, models_dir: &Path) -> Result<()>;
    pub fn recognize(&mut self, image_data: &[u8]) -> Result<OcrResult>;
}
```

**Key Features:**
- Lazy loading: Models loaded on first use to avoid OOM at startup
- Single responsibility: Only handles OCR inference, no device binding logic
- Model reuse: Models kept in memory across sessions

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `bind_device` | Bind device for automatic OCR | device_id, image_metric, draw_boxes |
| `unbind_device` | Unbind device | device_id |
| `get_bindings` | Get all bindings and status | - |
| `toggle_binding` | Toggle binding active state | device_id, active |
| `recognize_image` | Manual OCR test | image (base64) |
| `get_status` | Get extension status | - |
| `get_config` | Get current config | - |
| `configure` | Load persisted config | - |

## Main Extension Structure

```rust
pub struct OcrDeviceInference {
    ocr_engine: Mutex<OcrEngine>,
    bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,
    binding_stats: Arc<RwLock<HashMap<String, BindingStatus>>>,
    total_inferences: Arc<AtomicU64>,
    total_text_blocks: Arc<AtomicU64>,
    total_errors: Arc<AtomicU64>,
    draw_boxes_by_default: Mutex<bool>,
}

impl OcrDeviceInference {
    pub fn new() -> Self;
    pub async fn bind_device(&self, binding: DeviceBinding) -> Result<()>;
    pub async fn unbind_device(&self, device_id: &str) -> Result<()>;
    pub fn recognize_image(&self, image_b64: &str, draw_boxes: bool) -> Result<OcrResult>;
    fn handle_device_event(&self, device_id: &str, image_b64: &str, draw_boxes: bool);
    fn write_virtual_metrics(&self, device_id: &str, result: &OcrResult);
}
```

## Event Handling

Subscribe to `DeviceMetric` events. When a bound device updates its image metric:
1. Extract base64 image data
2. Run OCR inference
3. Write results to virtual metrics
4. Update binding statistics

## Frontend Component

**Component:** `OcrDeviceCard`
**Display Name:** OCR识别

**Features:**
- Dual tabs: Manual Test / Device Bindings
- Image drag-and-drop or click to upload
- Real-time display of recognition results and annotated image
- Copy recognized text

**frontend.json:**
```json
{
  "id": "ocr-device-inference",
  "version": "1.0.0",
  "entrypoint": "ocr-device-inference-components.umd.cjs",
  "components": [
    {
      "name": "OcrDeviceCard",
      "type": "widget",
      "displayName": "OCR识别",
      "description": "绑定设备进行自动 OCR 识别，或手动上传图像测试",
      "icon": "file-text",
      "defaultSize": { "width": 450, "height": 500 },
      "minSize": { "width": 350, "height": 400 },
      "maxSize": { "width": 600, "height": 700 },
      "configSchema": {
        "drawBoxes": {
          "type": "boolean",
          "default": true,
          "description": "在图像上绘制文本框"
        },
        "showPreview": {
          "type": "boolean",
          "default": true,
          "description": "显示识别结果预览"
        }
      }
    }
  ]
}
```

## File Structure

```
extensions/ocr-device-inference/
├── Cargo.toml
├── src/
│   └── lib.rs
├── frontend/
│   ├── frontend.json
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── src/
│   │   ├── index.tsx
│   │   └── OcrDeviceCard.tsx
│   └── dist/
│       └── ocr-device-inference-components.umd.cjs
├── models/
│   ├── det_mv3_db.onnx
│   └── rec_svtr.onnx
├── tests/
│   └── test_ocr.rs
├── metadata.json
└── README.md
```

## Dependencies

```toml
[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
chrono = "0.4"
base64 = "0.22"
image = "0.25"
imageproc = "0.24"
rusttype = "0.9"
parking_lot = "0.12"
tracing = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time"] }
uuid = { version = "1.0", features = ["v4"] }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ort = { version = "2.0.0-rc.11", features = ["half"] }
usls = { version = "0.2.0-alpha.3", default-features = false, features = ["vision", "ort-download-binaries"] }
```

## Implementation Notes

1. **Lazy Loading**: Models loaded on first inference to prevent OOM during extension startup
2. **Model Path**: Uses `NEOMIND_EXTENSION_DIR` environment variable to locate models directory
3. **Config Persistence**: Saves bindings to `config.json` in extension directory
4. **Drawing**: Uses `imageproc` crate to draw text boxes on images
5. **FFI Export**: Uses `neomind_export!` macro from SDK

## Comparison with yolo-device-inference

| Aspect | yolo-device-inference | ocr-device-inference |
|--------|----------------------|---------------------|
| Models | Single (YOLO) | Dual (DB + SVTR) |
| Detection | Objects | Text regions |
| Output | Detection labels | Text + confidence |
| Drawing | Bounding boxes | Text boxes + labels |
| Commands | Similar | Similar + recognize_image |
