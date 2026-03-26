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
