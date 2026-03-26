//! OCR Device Inference Extension
//!
//! This extension provides automatic OCR (Optical Character Recognition) inference
//! on device image data sources using DB + SVTR pipeline.

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError,
    MetricDescriptor, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use base64::Engine;
use chrono::Utc;

#[cfg(not(target_arch = "wasm32"))]
use uuid::Uuid;

#[cfg(not(target_arch = "wasm32"))]
use usls::{models::{DB, SVTR}, Config, DataLoader, Model, ORTConfig};

// ============================================================================
// Types
// ============================================================================

/// Device binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceBinding {
    /// Device ID
    pub device_id: String,
    /// Device name (for display)
    pub device_name: Option<String>,
    /// Image data source metric name
    pub image_metric: String,
    /// Virtual metric name prefix for storing results
    pub result_metric_prefix: String,
    /// Whether to draw text boxes on images
    pub draw_boxes: bool,
    /// Whether the binding is active
    pub active: bool,
}

/// Bounding box for text region
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
    /// Recognized text content
    pub text: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Bounding box of text region
    pub bbox: BoundingBox,
}

/// OCR inference result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    /// Device ID
    pub device_id: String,
    /// Recognized text blocks
    pub text_blocks: Vec<TextBlock>,
    /// Full text merged with newlines
    pub full_text: String,
    /// Number of text blocks
    pub total_blocks: usize,
    /// Average confidence score
    pub avg_confidence: f32,
    /// Inference time in milliseconds
    pub inference_time_ms: u64,
    /// Original image width
    pub image_width: u32,
    /// Original image height
    pub image_height: u32,
    /// Unix timestamp
    pub timestamp: i64,
    /// Annotated image with text boxes (base64)
    pub annotated_image_base64: Option<String>,
}

/// Binding status for tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingStatus {
    /// The binding configuration
    pub binding: DeviceBinding,
    /// Last inference timestamp
    pub last_inference: Option<i64>,
    /// Total inference count
    pub total_inferences: u64,
    /// Total text blocks detected
    pub total_text_blocks: u64,
    /// Last error message
    pub last_error: Option<String>,
    /// Last processed image (base64 data URI)
    pub last_image: Option<String>,
    /// Last recognized text blocks
    pub last_text_blocks: Option<Vec<TextBlock>>,
    /// Last annotated image (base64 data URI)
    pub last_annotated_image: Option<String>,
}

/// Extension configuration for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    /// Draw text boxes by default
    pub draw_boxes_by_default: bool,
    /// Device bindings
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

impl Default for DeviceBinding {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            device_name: None,
            image_metric: "image".to_string(),
            result_metric_prefix: "ocr_".to_string(),
            draw_boxes: true,
            active: true,
        }
    }
}

// ============================================================================
// OCR Engine (Native Only)
// ============================================================================

/// OCR Pipeline Engine - encapsulates DB detection + SVTR recognition
#[cfg(not(target_arch = "wasm32"))]
pub struct OcrEngine {
    /// DB text detection model
    detector: Option<usls::Runtime<DB>>,
    /// SVTR text recognition model
    recognizer: Option<usls::Runtime<SVTR>>,
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

        let det_path_str = det_path.to_str()
            .ok_or_else(|| ExtensionError::LoadFailed("Invalid DB model path".to_string()))?;

        let det_config = Config::db()
            .with_model(ORTConfig::default().with_file(det_path_str))
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

        let rec_path_str = rec_path.to_str()
            .ok_or_else(|| ExtensionError::LoadFailed("Invalid SVTR model path".to_string()))?;

        let rec_config = Config::svtr()
            .with_model(ORTConfig::default().with_file(rec_path_str))
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
            .join(format!("ocr_inference_{}.jpg", Uuid::new_v4()));
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

        // Run text recognition on detected regions
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
        let timestamp = Utc::now().timestamp();

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

        let total_blocks = text_blocks.len();

        Ok(OcrResult {
            device_id: device_id.to_string(),
            text_blocks,
            full_text,
            total_blocks,
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
        _det_outputs: &[usls::Y],
        _rec_outputs: &[usls::Y],
    ) -> Result<Vec<TextBlock>> {
        let text_blocks = Vec::new();

        // Extract text from recognition output
        // Note: This is a simplified implementation
        // The actual usls library provides higher-level OCR pipeline functions
        // that handle both detection and recognition together
        // For now, we return a placeholder to allow compilation
        // This will be implemented in a follow-up task

        // TODO: Implement proper parsing of usls OCR pipeline outputs
        // The usls library has methods like:
        // - pipeline::ocr::detect_and_recognize() for full pipeline
        // - Individual model outputs need proper parsing

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
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

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

    async fn execute_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value> {
        Err(ExtensionError::CommandNotFound(command.to_string()))
    }
}

neomind_extension_sdk::neomind_export!(OcrDeviceInference);
