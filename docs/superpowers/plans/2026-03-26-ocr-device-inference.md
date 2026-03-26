# OCR Device Inference Extension - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create an OCR device inference extension that binds devices for automatic text recognition or allows manual image upload for testing.

**Architecture:** DB + SVTR pipeline from usls library for text detection and recognition. Follows yolo-device-inference patterns with device binding, event handling, and virtual metrics output.

**Tech Stack:** Rust (neomind-extension-sdk, usls, ort), React 18 + TypeScript + Vite (UMD bundle)

---

## File Structure

```
extensions/ocr-device-inference/
├── Cargo.toml                          # Extension dependencies
├── src/
│   └── lib.rs                          # Main extension implementation
├── frontend/
│   ├── frontend.json                   # Component definitions
│   ├── package.json                    # NPM dependencies
│   ├── vite.config.ts                  # Vite build config
│   ├── tsconfig.json                   # TypeScript config
│   ├── src/
│   │   ├── index.tsx                   # Component exports
│   │   └── OcrDeviceCard.tsx           # Main React component
│   └── dist/                           # Build output
├── models/                             # OCR models (downloaded separately)
│   ├── det_mv3_db.onnx                 # DB detection model
│   └── rec_svtr.onnx                   # SVTR recognition model
├── tests/
│   └── test_ocr.rs                     # Unit tests
├── metadata.json                       # Auto-generated
└── README.md                           # Documentation
```

---

## Task 1: Project Setup

**Files:**
- Create: `extensions/ocr-device-inference/Cargo.toml`
- Create: `extensions/ocr-device-inference/src/lib.rs` (skeleton)

- [ ] **Step 1: Create extension directory and Cargo.toml**

```bash
mkdir -p extensions/ocr-device-inference/src
mkdir -p extensions/ocr-device-inference/models
mkdir -p extensions/ocr-device-inference/tests
```

- [ ] **Step 2: Write Cargo.toml**

```toml
[package]
name = "ocr-device-inference"
version = "1.0.0"
edition = "2021"
authors = ["NeoMind Team"]
license = "MIT"
description = "OCR device inference extension with automatic text recognition on data source updates"

[lib]
name = "neomind_extension_ocr_device_inference"
crate-type = ["cdylib", "rlib"]

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

[features]
default = []

[dev-dependencies]
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "test-util"] }
```

- [ ] **Step 3: Add extension to workspace**

Edit `Cargo.toml` at workspace root to add the new extension to the members list:

```toml
members = [
    # ... existing extensions ...
    "extensions/ocr-device-inference",
]
```

- [ ] **Step 4: Create lib.rs skeleton**

```rust
//! OCR Device Inference Extension
//!
//! This extension provides automatic OCR (Optical Character Recognition) inference
//! on device image data sources using DB + SVTR pipeline.

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError,
    MetricDescriptor, MetricDataType, Result,
};
use serde_json::json;

pub struct OcrDeviceInference;

impl OcrDeviceInference {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OcrDeviceInference {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for OcrDeviceInference {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "ocr-device-inference",
                "OCR识别",
                "1.0.0"
            )
            .with_description("Automatic OCR inference on device image data sources")
            .with_author("NeoMind Team")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![]
    }

    fn commands(&self) -> Vec<neomind_extension_sdk::ExtensionCommand> {
        vec![]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        Err(ExtensionError::CommandNotFound(command.to_string()))
    }
}

neomind_extension_sdk::neomind_export!(OcrDeviceInference);
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 6: Commit**

```bash
git add extensions/ocr-device-inference/ Cargo.toml
git commit -m "feat(ocr): initialize ocr-device-inference extension skeleton"
```

---

## Task 2: Data Structures

**Files:**
- Modify: `extensions/ocr-device-inference/src/lib.rs`

- [ ] **Step 1: Add data structures after imports**

```rust
// ============================================================================
// Types
// ============================================================================

/// Device binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceBinding {
    pub device_id: String,
    pub device_name: Option<String>,
    pub image_metric: String,
    pub result_metric_prefix: String,
    pub draw_boxes: bool,
    pub active: bool,
}

/// Bounding box
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Single text block recognition result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
}

/// OCR inference result
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Extension configuration for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    pub draw_boxes_by_default: bool,
    pub bindings: Vec<DeviceBinding>,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            draw_boxes_by_default: true,
            bindings: Vec::new(),
        }
    }
}
```

- [ ] **Step 2: Add necessary imports**

Add at top of file:
```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use base64::Engine;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add extensions/ocr-device-inference/src/lib.rs
git commit -m "feat(ocr): add data structures for OCR extension"
```

---

## Task 3: OcrEngine Implementation

**Files:**
- Modify: `extensions/ocr-device-inference/src/lib.rs`

- [ ] **Step 1: Add usls imports (native only)**

```rust
#[cfg(not(target_arch = "wasm32"))]
use usls::{models::{DB, SVTR}, Config, DataLoader, Model, ORTConfig};
```

- [ ] **Step 2: Add OcrEngine struct**

```rust
// ============================================================================
// OCR Engine
// ============================================================================

/// OCR Pipeline Engine - encapsulates DB detection + SVTR recognition
#[cfg(not(target_arch = "wasm32"))]
pub struct OcrEngine {
    /// DB text detection model
    detector: Option<Runtime<DB>>,
    /// SVTR text recognition model
    recognizer: Option<Runtime<SVTR>>,
    /// Load error message
    load_error: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl OcrEngine {
    pub fn new() -> Self {
        tracing::info!("[OcrDeviceInference] OCR models will be loaded on first use (lazy loading)");
        Self {
            detector: None,
            recognizer: None,
            load_error: None,
        }
    }

    /// Initialize models (lazy loading)
    pub fn init(&mut self, models_dir: &std::path::Path) -> Result<()> {
        if self.detector.is_some() && self.recognizer.is_some() {
            return Ok(());
        }

        // Load DB detection model
        let det_path = models_dir.join("det_mv3_db.onnx");
        if !det_path.exists() {
            return Err(ExtensionError::LoadFailed(
                format!("DB model not found: {:?}", det_path)
            ));
        }

        let det_config = Config::db()
            .with_model(ORTConfig::default().with_file(&det_path))
            .commit()
            .map_err(|e| ExtensionError::LoadFailed(format!("DB config failed: {:?}", e)))?;

        self.detector = Some(
            DB::new(det_config)
                .map_err(|e| ExtensionError::LoadFailed(format!("DB model load failed: {:?}", e)))?
        );

        // Load SVTR recognition model
        let rec_path = models_dir.join("rec_svtr.onnx");
        if !rec_path.exists() {
            return Err(ExtensionError::LoadFailed(
                format!("SVTR model not found: {:?}", rec_path)
            ));
        }

        let rec_config = Config::svtr()
            .with_model(ORTConfig::default().with_file(&rec_path))
            .commit()
            .map_err(|e| ExtensionError::LoadFailed(format!("SVTR config failed: {:?}", e)))?;

        self.recognizer = Some(
            SVTR::new(rec_config)
                .map_err(|e| ExtensionError::LoadFailed(format!("SVTR model load failed: {:?}", e)))?
        );

        self.load_error = None;
        tracing::info!("[OcrDeviceInference] OCR models loaded successfully");
        Ok(())
    }

    /// Perform OCR on image data
    pub fn recognize(&mut self, image_data: &[u8], device_id: &str) -> Result<OcrResult> {
        let start = std::time::Instant::now();

        // Create temp file for image
        let temp_path = std::env::temp_dir()
            .join(format!("ocr_inference_{}.jpg", uuid::Uuid::new_v4()));
        std::fs::write(&temp_path, image_data)
            .map_err(|e| ExtensionError::ExecutionFailed(
                format!("Failed to write temp image: {}", e)
            ))?;

        // Load image
        let dl = DataLoader::new(&temp_path)
            .map_err(|e| ExtensionError::ExecutionFailed(
                format!("Failed to load image: {:?}", e)
            ))?;

        let xs = dl.try_read()
            .map_err(|e| ExtensionError::ExecutionFailed(
                format!("Failed to read image: {:?}", e)
            ))?;

        let (img_width, img_height) = if !xs.is_empty() {
            let img = &xs[0];
            (img.width(), img.height())
        } else {
            (0, 0)
        };

        // Run text detection
        let detector = self.detector.as_mut()
            .ok_or_else(|| ExtensionError::ExecutionFailed("Detector not loaded".to_string()))?;
        let det_outputs = detector.run(&xs)
            .map_err(|e| ExtensionError::ExecutionFailed(
                format!("Detection failed: {:?}", e)
            ))?;

        // Run text recognition
        let recognizer = self.recognizer.as_mut()
            .ok_or_else(|| ExtensionError::ExecutionFailed("Recognizer not loaded".to_string()))?;
        let rec_outputs = recognizer.run(&xs)
            .map_err(|e| ExtensionError::ExecutionFailed(
                format!("Recognition failed: {:?}", e)
            ))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        // Parse results
        let text_blocks = self.parse_outputs(&det_outputs, &rec_outputs)?;

        let inference_time = start.elapsed().as_millis() as u64;
        let timestamp = chrono::Utc::now().timestamp();

        // Calculate average confidence
        let avg_confidence = if text_blocks.is_empty() {
            0.0
        } else {
            text_blocks.iter().map(|t| t.confidence).sum::<f32>() / text_blocks.len() as f32
        };

        // Build full text
        let full_text = text_blocks.iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(OcrResult {
            device_id: device_id.to_string(),
            text_blocks,
            full_text,
            total_blocks: text_blocks.len(),
            avg_confidence,
            inference_time_ms: inference_time,
            image_width: img_width,
            image_height: img_height,
            timestamp,
            annotated_image_base64: None,
        })
    }

    /// Parse model outputs into text blocks
    fn parse_outputs(
        &self,
        det_outputs: &[usls::Tensor],
        rec_outputs: &[usls::Tensor],
    ) -> Result<Vec<TextBlock>> {
        let mut text_blocks = Vec::new();

        // Extract text from recognition output
        // Note: Actual implementation depends on usls output format
        for y in rec_outputs.iter() {
            // Get text content and confidence
            if let Some(text) = y.text() {
                let confidence = y.confidence().unwrap_or(0.0);
                text_blocks.push(TextBlock {
                    text: text.to_string(),
                    confidence,
                    bbox: BoundingBox {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    },
                });
            }
        }

        Ok(text_blocks)
    }

    /// Check if models are loaded
    pub fn is_loaded(&self) -> bool {
        self.detector.is_some() && self.recognizer.is_some()
    }

    /// Get load error if any
    pub fn get_load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add extensions/ocr-device-inference/src/lib.rs
git commit -m "feat(ocr): implement OcrEngine with DB+SVTR pipeline"
```

---

## Task 4: Main Extension Structure

**Files:**
- Modify: `extensions/ocr-device-inference/src/lib.rs`

- [ ] **Step 1: Update OcrDeviceInference struct**

```rust
// ============================================================================
// Extension Implementation
// ============================================================================

pub struct OcrDeviceInference {
    /// OCR engine (native only)
    #[cfg(not(target_arch = "wasm32"))]
    ocr_engine: Mutex<OcrEngine>,
    #[cfg(not(target_arch = "wasm32"))]
    model_load_error: Mutex<Option<String>>,

    /// Device bindings
    bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,
    /// Binding status
    binding_stats: Arc<RwLock<HashMap<String, BindingStatus>>>,

    /// Global statistics
    total_inferences: Arc<AtomicU64>,
    total_text_blocks: Arc<AtomicU64>,
    total_errors: Arc<AtomicU64>,

    /// Configuration
    draw_boxes_by_default: Mutex<bool>,
}

impl OcrDeviceInference {
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        tracing::info!("[OcrDeviceInference] Extension created, models will load on first use");

        Self {
            #[cfg(not(target_arch = "wasm32"))]
            ocr_engine: Mutex::new(OcrEngine::new()),
            #[cfg(not(target_arch = "wasm32"))]
            model_load_error: Mutex::new(None),

            bindings: Arc::new(RwLock::new(HashMap::new())),
            binding_stats: Arc::new(RwLock::new(HashMap::new())),
            total_inferences: Arc::new(AtomicU64::new(0)),
            total_text_blocks: Arc::new(AtomicU64::new(0)),
            total_errors: Arc::new(AtomicU64::new(0)),
            draw_boxes_by_default: Mutex::new(true),
        }
    }

    /// Find models directory
    fn find_models_dir(&self) -> Result<std::path::PathBuf> {
        // Try NEOMIND_EXTENSION_DIR first
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            let path = std::path::PathBuf::from(&ext_dir).join("models");
            if path.exists() {
                return Ok(path);
            }
        }

        // Fallback to current directory
        if let Ok(cwd) = std::env::current_dir() {
            let path = cwd.join("models");
            if path.exists() {
                return Ok(path);
            }
        }

        Err(ExtensionError::LoadFailed(
            "Models directory not found. Set NEOMIND_EXTENSION_DIR or ensure models/ exists".to_string()
        ))
    }

    /// Get all bindings
    pub fn get_bindings(&self) -> Vec<BindingStatus> {
        self.binding_stats.read().values().cloned().collect()
    }

    /// Get extension status
    pub fn get_status(&self) -> serde_json::Value {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let model_loaded = self.ocr_engine.lock().is_loaded();
            json!({
                "model_loaded": model_loaded,
                "total_bindings": self.bindings.read().len(),
                "total_inferences": self.total_inferences.load(Ordering::SeqCst),
                "total_text_blocks": self.total_text_blocks.load(Ordering::SeqCst),
                "total_errors": self.total_errors.load(Ordering::SeqCst),
                "model_error": self.model_load_error.lock().clone(),
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            json!({
                "model_loaded": false,
                "total_bindings": self.bindings.read().len(),
                "total_inferences": self.total_inferences.load(Ordering::SeqCst),
                "total_text_blocks": self.total_text_blocks.load(Ordering::SeqCst),
                "total_errors": self.total_errors.load(Ordering::SeqCst),
                "model_error": "OCR not available in WASM",
            })
        }
    }

    /// Get current config for persistence
    pub fn get_config(&self) -> OcrConfig {
        OcrConfig {
            draw_boxes_by_default: *self.draw_boxes_by_default.lock(),
            bindings: self.bindings.read().values().cloned().collect(),
        }
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add extensions/ocr-device-inference/src/lib.rs
git commit -m "feat(ocr): add OcrDeviceInference main structure"
```

---

## Task 5: Drawing Helpers

**Files:**
- Modify: `extensions/ocr-device-inference/src/lib.rs`

- [ ] **Step 1: Add drawing helper function**

```rust
// ============================================================================
// Drawing Helpers
// ============================================================================

/// Color palette for drawing text boxes
const TEXT_BOX_COLORS: [(u8, u8, u8); 8] = [
    (239, 68, 68),   // red
    (34, 197, 94),   // green
    (59, 130, 246),  // blue
    (234, 179, 8),   // yellow
    (139, 92, 246),  // purple
    (6, 182, 212),   // cyan
    (236, 72, 153),  // pink
    (249, 115, 22),  // orange
];

/// Draw text boxes on an image
#[cfg(not(target_arch = "wasm32"))]
fn draw_text_boxes_on_image(
    image_data: &[u8],
    text_blocks: &[TextBlock],
) -> Result<String> {
    use imageproc::drawing::{draw_hollow_rect_mut, draw_filled_rect_mut};
    use imageproc::rect::Rect;

    // Decode image
    let mut img = image::load_from_memory(image_data)
        .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to load image: {}", e)))?
        .to_rgb8();

    tracing::debug!("[OcrDeviceInference] Drawing {} text boxes on image {}x{}",
        text_blocks.len(), img.width(), img.height());

    for (i, block) in text_blocks.iter().enumerate() {
        let color = TEXT_BOX_COLORS[i % TEXT_BOX_COLORS.len()];
        let image_color = image::Rgb([color.0, color.1, color.2]);

        // Clip coordinates
        let x = block.bbox.x.max(0.0).min(img.width() as f32 - 2.0) as i32;
        let y = block.bbox.y.max(0.0).min(img.height() as f32 - 2.0) as i32;
        let w = block.bbox.width.min(img.width() as f32 - x as f32 - 1.0) as u32;
        let h = block.bbox.height.min(img.height() as f32 - y as f32 - 1.0) as u32;

        if w < 2 || h < 2 {
            continue;
        }

        // Draw bounding box (2px thick)
        draw_hollow_rect_mut(&mut img, Rect::at(x, y).of_size(w, h), image_color);
        draw_hollow_rect_mut(&mut img, Rect::at(x + 1, y + 1).of_size(w.saturating_sub(2), h.saturating_sub(2)), image_color);

        // Draw text label background
        let label = format!("{} {:.0}%", block.text, block.confidence * 100.0);
        // Truncate long labels
        let label: String = label.chars().take(30).collect();
        let text_width = (label.len() as u32) * 8;
        let text_height = 14u32;

        let label_y = if y >= text_height as i32 {
            y - text_height as i32
        } else {
            y + h as i32
        };

        draw_filled_rect_mut(
            &mut img,
            Rect::at(x, label_y).of_size(text_width, text_height),
            image_color,
        );
    }

    // Encode to JPEG
    let mut jpeg_data = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_data);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 85);
    encoder.encode(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ColorType::Rgb8.into(),
    )
    .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to encode JPEG: {}", e)))?;

    Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_data))
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add extensions/ocr-device-inference/src/lib.rs
git commit -m "feat(ocr): add drawing helpers for text box annotation"
```

---

## Task 6: Commands Implementation

**Files:**
- Modify: `extensions/ocr-device-inference/src/lib.rs`

- [ ] **Step 1: Implement commands() method**

Update the `commands()` method in the Extension trait implementation:

```rust
fn commands(&self) -> Vec<neomind_extension_sdk::ExtensionCommand> {
    use neomind_extension_sdk::{ExtensionCommand, ParameterDefinition};

    vec![
        ExtensionCommand {
            name: "bind_device".to_string(),
            display_name: "Bind Device".to_string(),
            description: "Bind a device for automatic OCR inference".to_string(),
            payload_template: String::new(),
            parameters: vec![
                ParameterDefinition {
                    name: "device_id".to_string(),
                    display_name: "Device ID".to_string(),
                    description: "ID of the device to bind".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "image_metric".to_string(),
                    display_name: "Image Metric".to_string(),
                    description: "Name of the image data source metric".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(neomind_extension_sdk::ParamMetricValue::String("image".to_string())),
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "draw_boxes".to_string(),
                    display_name: "Draw Boxes".to_string(),
                    description: "Whether to draw text boxes on images".to_string(),
                    param_type: MetricDataType::Boolean,
                    required: false,
                    default_value: Some(neomind_extension_sdk::ParamMetricValue::Boolean(true)),
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
            ],
            fixed_values: HashMap::new(),
            samples: vec![json!({"device_id": "camera-01", "image_metric": "image"})],
            parameter_groups: Vec::new(),
        },
        ExtensionCommand {
            name: "unbind_device".to_string(),
            display_name: "Unbind Device".to_string(),
            description: "Unbind a device".to_string(),
            payload_template: String::new(),
            parameters: vec![
                ParameterDefinition {
                    name: "device_id".to_string(),
                    display_name: "Device ID".to_string(),
                    description: "ID of the device to unbind".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
            ],
            fixed_values: HashMap::new(),
            samples: vec![json!({"device_id": "camera-01"})],
            parameter_groups: Vec::new(),
        },
        ExtensionCommand {
            name: "get_bindings".to_string(),
            display_name: "Get Bindings".to_string(),
            description: "Get all device bindings".to_string(),
            payload_template: String::new(),
            parameters: vec![],
            fixed_values: HashMap::new(),
            samples: vec![],
            parameter_groups: Vec::new(),
        },
        ExtensionCommand {
            name: "recognize_image".to_string(),
            display_name: "Recognize Image".to_string(),
            description: "Manually upload image for OCR testing".to_string(),
            payload_template: String::new(),
            parameters: vec![
                ParameterDefinition {
                    name: "image".to_string(),
                    display_name: "Image".to_string(),
                    description: "Base64 encoded image data".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
            ],
            fixed_values: HashMap::new(),
            samples: vec![json!({"image": "base64_encoded_image_data"})],
            parameter_groups: Vec::new(),
        },
        ExtensionCommand {
            name: "toggle_binding".to_string(),
            display_name: "Toggle Binding".to_string(),
            description: "Toggle binding active state".to_string(),
            payload_template: String::new(),
            parameters: vec![
                ParameterDefinition {
                    name: "device_id".to_string(),
                    display_name: "Device ID".to_string(),
                    description: "ID of the bound device".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "active".to_string(),
                    display_name: "Active".to_string(),
                    description: "Whether the binding should be active".to_string(),
                    param_type: MetricDataType::Boolean,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
            ],
            fixed_values: HashMap::new(),
            samples: vec![json!({"device_id": "camera-01", "active": false})],
            parameter_groups: Vec::new(),
        },
        ExtensionCommand {
            name: "get_status".to_string(),
            display_name: "Get Status".to_string(),
            description: "Get extension status".to_string(),
            payload_template: String::new(),
            parameters: vec![],
            fixed_values: HashMap::new(),
            samples: vec![],
            parameter_groups: Vec::new(),
        },
        ExtensionCommand {
            name: "get_config".to_string(),
            display_name: "Get Config".to_string(),
            description: "Get current config".to_string(),
            payload_template: String::new(),
            parameters: vec![],
            fixed_values: HashMap::new(),
            samples: vec![],
            parameter_groups: Vec::new(),
        },
        ExtensionCommand {
            name: "configure".to_string(),
            display_name: "Configure".to_string(),
            description: "Load persisted configuration".to_string(),
            payload_template: String::new(),
            parameters: vec![],
            fixed_values: HashMap::new(),
            samples: vec![],
            parameter_groups: Vec::new(),
        },
    ]
}
```

- [ ] **Step 2: Implement metrics() method**

```rust
fn metrics(&self) -> Vec<MetricDescriptor> {
    vec![
        MetricDescriptor {
            name: "bound_devices".to_string(),
            display_name: "Bound Devices".to_string(),
            data_type: MetricDataType::Integer,
            unit: "count".to_string(),
            min: Some(0.0),
            max: None,
            required: false,
        },
        MetricDescriptor {
            name: "total_inferences".to_string(),
            display_name: "Total Inferences".to_string(),
            data_type: MetricDataType::Integer,
            unit: "count".to_string(),
            min: Some(0.0),
            max: None,
            required: false,
        },
        MetricDescriptor {
            name: "total_text_blocks".to_string(),
            display_name: "Total Text Blocks".to_string(),
            data_type: MetricDataType::Integer,
            unit: "count".to_string(),
            min: Some(0.0),
            max: None,
            required: false,
        },
        MetricDescriptor {
            name: "total_errors".to_string(),
            display_name: "Total Errors".to_string(),
            data_type: MetricDataType::Integer,
            unit: "count".to_string(),
            min: Some(0.0),
            max: None,
            required: false,
        },
    ]
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add extensions/ocr-device-inference/src/lib.rs
git commit -m "feat(ocr): add commands and metrics definitions"
```

---

## Task 7: execute_command Implementation

**Files:**
- Modify: `extensions/ocr-device-inference/src/lib.rs`

- [ ] **Step 1: Implement execute_command**

This is a large implementation. Add the full command handling logic following the yolo-device-inference pattern. Key commands:
- `bind_device`: Add device binding
- `unbind_device`: Remove device binding
- `get_bindings`: Return all bindings
- `recognize_image`: Manual OCR test
- `toggle_binding`: Toggle active state
- `get_status`: Return extension status
- `get_config`: Return current config
- `configure`: Load persisted config

Reference: `extensions/yolo-device-inference/src/lib.rs` lines 1330-1550

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add extensions/ocr-device-inference/src/lib.rs
git commit -m "feat(ocr): implement execute_command for all commands"
```

---

## Task 8: Event Handling

**Files:**
- Modify: `extensions/ocr-device-inference/src/lib.rs`

- [ ] **Step 1: Implement event_subscriptions and handle_event**

```rust
fn event_subscriptions(&self) -> &[&str] {
    &["DeviceMetric"]
}

fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
    if event_type != "DeviceMetric" {
        return Ok(());
    }

    // Extract device info
    let inner_payload = payload.get("payload").unwrap_or(payload);
    let device_id = inner_payload.get("device_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let metric = inner_payload.get("metric")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let value = inner_payload.get("value");

    // Check if device is bound
    let binding = self.bindings.read().get(device_id).cloned();
    if let Some(binding) = binding {
        if !binding.active {
            return Ok(());
        }

        // Check if metric matches
        if metric != binding.image_metric {
            return Ok(());
        }

        // Extract image data
        let image_b64 = self.extract_image_from_value(value, None);
        if let Some(image_data_b64) = image_b64 {
            match base64::engine::general_purpose::STANDARD.decode(&image_data_b64) {
                Ok(image_data) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        match self.process_image(&image_data, device_id, binding.draw_boxes) {
                            Ok(result) => {
                                self.write_virtual_metrics(device_id, &result);
                            }
                            Err(e) => {
                                self.total_errors.fetch_add(1, Ordering::SeqCst);
                                if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
                                    stats.last_error = Some(e.to_string());
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("[OcrDeviceInference] Base64 decode failed: {}", e);
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Add helper methods**

Add `extract_image_from_value`, `process_image`, and `write_virtual_metrics` methods.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p ocr-device-inference`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add extensions/ocr-device-inference/src/lib.rs
git commit -m "feat(ocr): implement event handling for device metrics"
```

---

## Task 9: Frontend Setup

**Files:**
- Create: `extensions/ocr-device-inference/frontend/package.json`
- Create: `extensions/ocr-device-inference/frontend/tsconfig.json`
- Create: `extensions/ocr-device-inference/frontend/vite.config.ts`
- Create: `extensions/ocr-device-inference/frontend/frontend.json`

- [ ] **Step 1: Create frontend directory structure**

```bash
mkdir -p extensions/ocr-device-inference/frontend/src
```

- [ ] **Step 2: Create package.json**

```json
{
  "name": "@neomind/ocr-device-inference-frontend",
  "version": "1.0.0",
  "description": "OCR Device Inference frontend component",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "react": "^18.2.0",
    "react-dom": "^18.2.0"
  },
  "devDependencies": {
    "@types/react": "^18.2.0",
    "@types/react-dom": "^18.2.0",
    "@vitejs/plugin-react": "^4.2.0",
    "typescript": "^5.3.0",
    "vite": "^5.0.0"
  },
  "peerDependencies": {
    "react": ">=18.0.0",
    "react-dom": ">=18.0.0"
  }
}
```

- [ ] **Step 3: Create tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

- [ ] **Step 4: Create tsconfig.node.json**

```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 5: Create vite.config.ts**

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  define: {
    'process.env.NODE_ENV': JSON.stringify('production')
  },
  build: {
    lib: {
      entry: 'src/index.tsx',
      name: 'OcrDeviceInferenceComponents',
      fileName: 'ocr-device-inference-components',
      formats: ['umd']
    },
    rollupOptions: {
      external: ['react', 'react-dom'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM'
        }
      }
    },
    outDir: 'dist',
    emptyOutDir: true
  }
})
```

- [ ] **Step 6: Create frontend.json**

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
  ],
  "i18n": {
    "defaultLanguage": "zh",
    "supportedLanguages": ["zh", "en"]
  },
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

- [ ] **Step 7: Commit**

```bash
git add extensions/ocr-device-inference/frontend/
git commit -m "feat(ocr): add frontend build configuration"
```

---

## Task 10: Frontend React Component

**Files:**
- Create: `extensions/ocr-device-inference/frontend/src/index.tsx`
- Create: `extensions/ocr-device-inference/frontend/src/OcrDeviceCard.tsx`

- [ ] **Step 1: Create index.tsx**

```tsx
export { OcrDeviceCard } from './OcrDeviceCard'
export default { OcrDeviceCard }
```

- [ ] **Step 2: Create OcrDeviceCard.tsx**

Implement the full React component with:
- Dual tabs: Manual Test / Device Bindings
- Image upload (drag & drop or click)
- OCR result display
- Text block list with confidence
- Annotated image preview
- Device binding management

Reference: `extensions/yolo-device-inference/frontend/src/index.tsx` for patterns

- [ ] **Step 3: Install dependencies and build**

```bash
cd extensions/ocr-device-inference/frontend
npm install
npm run build
```

Expected: `dist/ocr-device-inference-components.umd.cjs` created

- [ ] **Step 4: Commit**

```bash
git add extensions/ocr-device-inference/frontend/
git commit -m "feat(ocr): add OcrDeviceCard React component"
```

---

## Task 11: Tests and Documentation

**Files:**
- Create: `extensions/ocr-device-inference/tests/test_ocr.rs`
- Create: `extensions/ocr-device-inference/README.md`

- [ ] **Step 1: Create basic unit tests**

- [ ] **Step 2: Create README.md** with usage instructions

- [ ] **Step 3: Run tests**

```bash
cargo test -p ocr-device-inference
```

- [ ] **Step 4: Commit**

```bash
git add extensions/ocr-device-inference/
git commit -m "feat(ocr): add tests and documentation"
```

---

## Task 12: Build and Verify

- [ ] **Step 1: Build extension**

```bash
cargo build --release -p ocr-device-inference
```

- [ ] **Step 2: Generate metadata.json**

```bash
./scripts/update-versions.sh 2.4.0
```

- [ ] **Step 3: Build .nep package**

```bash
./build.sh --single ocr-device-inference
```

- [ ] **Step 4: Final commit**

```bash
git add .
git commit -m "feat(ocr): complete ocr-device-inference extension v1.0.0"
```

---

## Summary

| Task | Description | Est. Time |
|------|-------------|-----------|
| 1 | Project Setup | 15 min |
| 2 | Data Structures | 10 min |
| 3 | OcrEngine | 30 min |
| 4 | Main Extension Structure | 20 min |
| 5 | Drawing Helpers | 15 min |
| 6 | Commands | 20 min |
| 7 | execute_command | 30 min |
| 8 | Event Handling | 25 min |
| 9 | Frontend Setup | 10 min |
| 10 | React Component | 45 min |
| 11 | Tests & Docs | 20 min |
| 12 | Build & Verify | 15 min |

**Total: ~4 hours**
