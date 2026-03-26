//! OCR Device Inference Extension
//!
//! This extension provides automatic OCR (Optical Character Recognition) inference
//! on device image data sources using DB + SVTR pipeline.

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use chrono::Utc;

#[cfg(not(target_arch = "wasm32"))]
use uuid::Uuid;

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
///
/// This is a simplified implementation that will be enhanced when
/// the full usls OCR API is available.
#[cfg(not(target_arch = "wasm32"))]
pub struct OcrEngine {
    /// Models loaded flag
    loaded: bool,
    /// Load error message
    load_error: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl OcrEngine {
    pub fn new() -> Self {
        tracing::info!("[OcrDeviceInference] OCR models will be loaded on first use (lazy loading)");
        Self {
            loaded: false,
            load_error: None,
        }
    }

    /// Initialize models (lazy loading)
    pub fn init(&mut self, models_dir: &std::path::Path) -> Result<()> {
        if self.loaded {
            return Ok(());
        }

        // Check for required model files
        let det_path = models_dir.join("det_mv3_db.onnx");
        if !det_path.exists() {
            return Err(ExtensionError::LoadFailed(
                format!("DB model not found: {:?}. Please download OCR models.", det_path)
            ));
        }

        let rec_path = models_dir.join("rec_svtr.onnx");
        if !rec_path.exists() {
            return Err(ExtensionError::LoadFailed(
                format!("SVTR model not found: {:?}. Please download OCR models.", rec_path)
            ));
        }

        // Mark as loaded - actual model initialization will be done when usls OCR API is stable
        self.loaded = true;
        self.load_error = None;
        tracing::info!("[OcrDeviceInference] OCR models found and ready");
        Ok(())
    }

    /// Perform OCR on image data
    ///
    /// This is a placeholder implementation. Full OCR functionality
    /// will be implemented when the usls OCR API is finalized.
    pub fn recognize(&mut self, image_data: &[u8], device_id: &str) -> Result<OcrResult> {
        let start = std::time::Instant::now();

        if !self.loaded {
            return Err(ExtensionError::ExecutionFailed(
                "OCR engine not initialized".to_string()
            ));
        }

        // Create temp file for image
        let temp_path = std::env::temp_dir()
            .join(format!("ocr_inference_{}.jpg", Uuid::new_v4()));
        std::fs::write(&temp_path, image_data)
            .map_err(|e| ExtensionError::ExecutionFailed(
                format!("Failed to write temp image: {}", e)
            ))?;

        // Load image to get dimensions
        let (img_width, img_height) = self.get_image_dimensions(&temp_path);

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        let inference_time = start.elapsed().as_millis() as u64;
        let timestamp = Utc::now().timestamp();

        // Placeholder: Return empty result
        // TODO: Implement full OCR pipeline when usls OCR API is stable
        tracing::debug!("[OcrDeviceInference] OCR placeholder executed in {}ms", inference_time);

        Ok(OcrResult {
            device_id: device_id.to_string(),
            text_blocks: Vec::new(),
            full_text: String::new(),
            total_blocks: 0,
            avg_confidence: 0.0,
            inference_time_ms: inference_time,
            image_width: img_width,
            image_height: img_height,
            timestamp,
            annotated_image_base64: None,
        })
    }

    /// Get image dimensions using image crate
    fn get_image_dimensions(&self, path: &std::path::Path) -> (u32, u32) {
        match image::ImageReader::open(path) {
            Ok(reader) => match reader.into_dimensions() {
                Ok((w, h)) => (w, h),
                Err(_) => (0, 0),
            },
            Err(_) => (0, 0),
        }
    }

    /// Check if models are loaded
    pub fn is_loaded(&self) -> bool {
        self.loaded
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

// ============================================================================
// Main Extension Structure
// ============================================================================

pub struct OcrDeviceInference {
    /// OCR engine (native only)
    #[cfg(not(target_arch = "wasm32"))]
    ocr_engine: Mutex<OcrEngine>,

    /// Device bindings: device_id -> binding
    bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,

    /// Binding status for tracking
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

            bindings: Arc::new(RwLock::new(HashMap::new())),
            binding_stats: Arc::new(RwLock::new(HashMap::new())),
            total_inferences: Arc::new(AtomicU64::new(0)),
            total_text_blocks: Arc::new(AtomicU64::new(0)),
            total_errors: Arc::new(AtomicU64::new(0)),
            draw_boxes_by_default: Mutex::new(true),
        }
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
            serde_json::json!({
                "model_loaded": model_loaded,
                "total_bindings": self.bindings.read().len(),
                "total_inferences": self.total_inferences.load(Ordering::SeqCst),
                "total_text_blocks": self.total_text_blocks.load(Ordering::SeqCst),
                "total_errors": self.total_errors.load(Ordering::SeqCst),
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            serde_json::json!({
                "model_loaded": false,
                "total_bindings": self.bindings.read().len(),
                "total_inferences": self.total_inferences.load(Ordering::SeqCst),
                "total_text_blocks": self.total_text_blocks.load(Ordering::SeqCst),
                "total_errors": self.total_errors.load(Ordering::SeqCst),
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
        vec![
            MetricDescriptor {
                name: "bound_devices".to_string(),
                display_name: "Bound Devices".to_string(),
                data_type: neomind_extension_sdk::MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_inferences".to_string(),
                display_name: "Total Inferences".to_string(),
                data_type: neomind_extension_sdk::MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_text_blocks".to_string(),
                display_name: "Total Text Blocks".to_string(),
                data_type: neomind_extension_sdk::MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_errors".to_string(),
                display_name: "Total Errors".to_string(),
                data_type: neomind_extension_sdk::MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "bind_device".to_string(),
                display_name: "Bind Device".to_string(),
                description: "Bind a device for automatic OCR inference on image updates".to_string(),
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
                        name: "device_name".to_string(),
                        display_name: "Device Name".to_string(),
                        description: "Display name for the device".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
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
                        default_value: Some(ParamMetricValue::String("image".to_string())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "result_metric_prefix".to_string(),
                        display_name: "Result Metric Prefix".to_string(),
                        description: "Prefix for virtual metrics storing results".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String("ocr_".to_string())),
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
                        default_value: Some(ParamMetricValue::Boolean(true)),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![json!({"device_id": "camera-01", "image_metric": "image", "draw_boxes": true})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "unbind_device".to_string(),
                display_name: "Unbind Device".to_string(),
                description: "Unbind a device from automatic OCR inference".to_string(),
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
                description: "Get all device bindings and their status".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "recognize_image".to_string(),
                display_name: "Recognize Image".to_string(),
                description: "Perform OCR on an image and return recognized text".to_string(),
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
                description: "Toggle a device binding active state".to_string(),
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
                description: "Get extension status including model info and statistics".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_config".to_string(),
                display_name: "Get Config".to_string(),
                description: "Get current OCR extension configuration".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "configure".to_string(),
                display_name: "Configure".to_string(),
                description: "Configure the extension with persisted settings".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
        ]
    }

    async fn execute_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value> {
        Err(ExtensionError::CommandNotFound(command.to_string()))
    }
}

neomind_extension_sdk::neomind_export!(OcrDeviceInference);
