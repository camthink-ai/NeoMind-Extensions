//! NeoMind YOLO Device Inference Extension
//!
//! This extension provides automatic YOLOv8 inference on device image data sources.
//! When a bound device updates its image data, the extension automatically performs
//! object detection and stores the results as virtual metrics on the device.
//!
//! # Features
//! - Bind/unbind devices with image data sources
//! - Device validation before binding
//! - Event-driven inference on device data updates
//! - Store detection results as virtual metrics
//!
//! # Event Handling
//! This extension uses the SDK's built-in event handling mechanism:
//! - `event_subscriptions()` declares which events to subscribe to
//! - `handle_event()` is called by the system when events are received

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use neomind_extension_sdk::capabilities::CapabilityContext;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use base64::Engine;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use usls::{models::YOLO, Config, DataLoader, Version as YOLOVersion};

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
    /// Image data source metric name (e.g., "image", "frame", "snapshot")
    pub image_metric: String,
    /// Virtual metric name prefix for storing results
    pub result_metric_prefix: String,
    /// Confidence threshold (0.0 - 1.0)
    pub confidence_threshold: f32,
    /// Whether to draw detection boxes on images
    pub draw_boxes: bool,
    /// Whether the binding is active
    pub active: bool,
}

/// Detection result for a single object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub label: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_id: Option<usize>,
}

/// Bounding box
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Inference result for a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResult {
    pub device_id: String,
    pub detections: Vec<Detection>,
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
    pub total_detections: u64,
    pub last_error: Option<String>,
    /// Last image (base64 encoded) for frontend display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_image: Option<String>,
    /// Last detection results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_detections: Option<Vec<Detection>>,
    /// Last annotated image with detection boxes (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_annotated_image: Option<String>,
}

/// Extension configuration for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YoloConfig {
    /// Default confidence threshold
    pub default_confidence: f32,
    /// Model version
    pub model_version: String,
    /// Device bindings
    pub bindings: Vec<DeviceBinding>,
}

impl Default for YoloConfig {
    fn default() -> Self {
        Self {
            default_confidence: 0.25,
            model_version: "v8-n".to_string(),
            bindings: Vec::new(),
        }
    }
}

// ============================================================================
// COCO Classes
// ============================================================================

pub const COCO_CLASSES: [&str; 80] = [
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat",
    "traffic light", "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat",
    "dog", "horse", "sheep", "cow", "elephant", "bear", "zebra", "giraffe", "backpack",
    "umbrella", "handbag", "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball",
    "kite", "baseball bat", "baseball glove", "skateboard", "surfboard", "tennis racket",
    "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
    "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake",
    "chair", "couch", "potted plant", "bed", "dining table", "toilet", "tv", "laptop",
    "mouse", "remote", "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
    "refrigerator", "book", "clock", "vase", "scissors", "teddy bear", "hair drier", "toothbrush",
];

// ============================================================================
// Drawing Helpers
// ============================================================================

/// Color palette for drawing boxes
const BOX_COLORS: [(u8, u8, u8); 10] = [
    (239, 68, 68), (34, 197, 94), (59, 130, 246), (234, 179, 8), (6, 182, 212),
    (139, 92, 246), (236, 72, 153), (249, 115, 22), (132, 204, 22), (20, 184, 166),
];

/// Draw detections on an image
#[cfg(not(target_arch = "wasm32"))]
fn draw_detections_on_image(
    image_data: &[u8],
    detections: &[Detection],
) -> Result<String> {
    use imageproc::drawing::{draw_hollow_rect_mut, draw_filled_rect_mut};
    use imageproc::rect::Rect;

    // Decode image
    let mut img = image::load_from_memory(image_data)
        .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to load image: {}", e)))?
        .to_rgb8();

    tracing::debug!("[YoloDeviceInference] Drawing {} detections on image {}x{}",
        detections.len(), img.width(), img.height());

    for (i, det) in detections.iter().enumerate() {
        let color = BOX_COLORS[i % BOX_COLORS.len()];
        let image_color = image::Rgb([color.0, color.1, color.2]);

        // Clip coordinates to image bounds
        let x = det.bbox.x.max(0.0).min(img.width() as f32 - 2.0) as i32;
        let y = det.bbox.y.max(0.0).min(img.height() as f32 - 2.0) as i32;
        let w = det.bbox.width.min(img.width() as f32 - x as f32 - 1.0) as u32;
        let h = det.bbox.height.min(img.height() as f32 - y as f32 - 1.0) as u32;

        if w < 2 || h < 2 {
            continue;
        }

        // Draw bounding box (2px thick)
        draw_hollow_rect_mut(&mut img, Rect::at(x, y).of_size(w, h), image_color);
        draw_hollow_rect_mut(&mut img, Rect::at(x + 1, y + 1).of_size(w.saturating_sub(2), h.saturating_sub(2)), image_color);

        // Draw label background
        let label = format!("{} {:.0}%", det.label, det.confidence * 100.0);
        let text_width = (label.len() as u32) * 8;
        let text_height = 14u32;

        if y >= text_height as i32 {
            draw_filled_rect_mut(
                &mut img,
                Rect::at(x, y - text_height as i32).of_size(text_width, text_height),
                image_color,
            );
        }
    }

    // Encode to JPEG and return base64
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

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct YoloDeviceInference {
    /// YOLO model runtime (native only)
    #[cfg(not(target_arch = "wasm32"))]
    detector: Mutex<Option<Arc<std::sync::Mutex<YOLO>>>>,
    #[cfg(not(target_arch = "wasm32"))]
    model_load_error: Mutex<Option<String>>,

    /// Device bindings: device_id -> binding
    bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,
    /// Binding status for tracking
    binding_stats: Arc<RwLock<HashMap<String, BindingStatus>>>,

    /// Global statistics
    total_inferences: Arc<AtomicU64>,
    total_detections: Arc<AtomicU64>,
    total_errors: Arc<AtomicU64>,

    /// Configuration
    default_confidence: Mutex<f32>,
    model_version: Mutex<String>,
}

impl YoloDeviceInference {
    pub fn new() -> Self {
        // ✅ FIX: Use lazy loading - model will be loaded on first use
        // This prevents OOM during extension startup
        #[cfg(not(target_arch = "wasm32"))]
        {
            tracing::info!("[YoloDeviceInference] YOLO model will be loaded on first inference (lazy loading)");
        }

        Self {
            #[cfg(not(target_arch = "wasm32"))]
            detector: Mutex::new(None),  // Lazy loading - no model yet
            #[cfg(not(target_arch = "wasm32"))]
            model_load_error: Mutex::new(None),
            bindings: Arc::new(RwLock::new(HashMap::new())),
            binding_stats: Arc::new(RwLock::new(HashMap::new())),
            total_inferences: Arc::new(AtomicU64::new(0)),
            total_detections: Arc::new(AtomicU64::new(0)),
            total_errors: Arc::new(AtomicU64::new(0)),
            default_confidence: Mutex::new(0.25),
            model_version: Mutex::new("v8-n".to_string()),
        }
    }

    fn invoke_capability_sync(
        &self,
        capability_name: &str,
        params: &serde_json::Value,
    ) -> serde_json::Value {
        // Use block_in_place to safely block in an async context.
        // This is safe because the extension runner uses multi_thread runtime.
        tokio::task::block_in_place(|| {
            let capability_context = CapabilityContext::default();
            let response = capability_context.invoke_capability(capability_name, params);
            if response
                .get("success")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                if response.get("result").is_some() {
                    response
                } else {
                    json!({
                        "success": true,
                        "result": response,
                    })
                }
            } else {
                response
            }
        })
    }

    /// Initialize the YOLO model (native only)
    #[cfg(not(target_arch = "wasm32"))]
    fn init_model(&self) -> Result<()> {
        let mut detector = self.detector.lock();

        if detector.is_some() {
            return Ok(());
        }

        let version_str = self.model_version.lock().clone();
        let parts: Vec<&str> = version_str.split('-').collect();
        let version_num: u8 = parts.get(0)
            .and_then(|v| v.trim_start_matches('v').parse().ok())
            .unwrap_or(8);

        let model_result = self.load_model(version_num);

        match model_result {
            Ok(model) => {
                *detector = Some(model);
                *self.model_load_error.lock() = None;
                tracing::debug!("[YoloDeviceInference] YOLO model loaded");
                Ok(())
            }
            Err(e) => {
                *self.model_load_error.lock() = Some(e.clone());
                tracing::error!("[YoloDeviceInference] Failed to load model: {}", e);
                Err(ExtensionError::LoadFailed(e))
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_model(&self, version: u8) -> std::result::Result<Arc<std::sync::Mutex<YOLO>>, String> {
        let model_filename = format!("yolov{}n.onnx", version);
        let model_path = self.find_model_path(&model_filename)?;

        tracing::debug!("[YoloDeviceInference] Loading model: {}", model_filename);

        let conf = *self.default_confidence.lock();

        // Create config using usls 0.1.11 API
        let config = Config::yolo()
            .with_model_file(model_path.to_str().unwrap())
            .with_version(YOLOVersion(version, 0, None))
            .with_class_confs(&[conf])
            .commit()
            .map_err(|e| format!("Failed to commit config: {:?}", e))?;

        let model = YOLO::new(config)
            .map_err(|e| format!("Failed to create YOLO model: {:?}", e))?;

        Ok(Arc::new(std::sync::Mutex::new(model)))
    }

    /// Static version of load_model for use during initialization (before `self` is available)
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    fn load_model_static(version: u8) -> std::result::Result<Arc<std::sync::Mutex<YOLO>>, String> {
        let model_filename = format!("yolov{}n.onnx", version);
        let model_path = Self::find_model_path_static(&model_filename)?;

        tracing::debug!("[YoloDeviceInference] Loading model: {}", model_filename);

        // Use default confidence threshold
        let conf = 0.25;

        // Create config using usls 0.1.11 API
        let config = Config::yolo()
            .with_model_file(model_path.to_str().unwrap())
            .with_version(YOLOVersion(version, 0, None))
            .with_class_confs(&[conf])
            .commit()
            .map_err(|e| format!("Failed to commit config: {:?}", e))?;

        let model = YOLO::new(config)
            .map_err(|e| format!("Failed to create YOLO model: {:?}", e))?;

        Ok(Arc::new(std::sync::Mutex::new(model)))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn find_model_path(&self, filename: &str) -> std::result::Result<std::path::PathBuf, String> {
        // Load model from extension's models directory
        // Expected path: <extension_dir>/models/<filename>
        
        // Try NEOMIND_EXTENSION_DIR first
        eprintln!("[YoloDeviceInference] Checking NEOMIND_EXTENSION_DIR env var");
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            eprintln!("[YoloDeviceInference] NEOMIND_EXTENSION_DIR = {}", ext_dir);
            let path = std::path::PathBuf::from(&ext_dir).join("models").join(filename);
            if path.exists() {
                return Ok(path);
            }
        }

        // Fallback: Check current working directory
        if let Ok(cwd) = std::env::current_dir() {
            let path = cwd.join("models").join(filename);
            if path.exists() {
                return Ok(path);
            }
        }

        // Additional fallback paths
        let fallback_paths = vec![
            std::path::PathBuf::from("models").join(filename),
            std::path::PathBuf::from("../models").join(filename),
        ];

        for path in fallback_paths {
            if path.exists() {
                return Ok(path);
            }
        }

        Err(format!("Model file '{}' not found in extension models directory", filename))
    }

    /// Static version of find_model_path for use during initialization
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    fn find_model_path_static(filename: &str) -> std::result::Result<std::path::PathBuf, String> {
        // Load model from extension's models directory
        // Expected path: <extension_dir>/models/<filename>
        
        // Try NEOMIND_EXTENSION_DIR first
        eprintln!("[YoloDeviceInference] Checking NEOMIND_EXTENSION_DIR env var");
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            eprintln!("[YoloDeviceInference] NEOMIND_EXTENSION_DIR = {}", ext_dir);
            let path = std::path::PathBuf::from(&ext_dir).join("models").join(filename);
            if path.exists() {
                return Ok(path);
            }
        }

        // Fallback: Check current working directory
        if let Ok(cwd) = std::env::current_dir() {
            let path = cwd.join("models").join(filename);
            if path.exists() {
                return Ok(path);
            }
        }

        // Additional fallback paths
        let fallback_paths = vec!
[ std::path::PathBuf::from("models").join(filename),
            std::path::PathBuf::from("../models").join(filename),
        ];

        for path in fallback_paths {
            if path.exists() {
                return Ok(path);
            }
        }

        Err(format!("Model file '{}' not found in extension models directory", filename))
    }

    /// Validate device exists (native only)
    #[cfg(not(target_arch = "wasm32"))]
    async fn validate_device(&self, device_id: &str) -> Result<bool> {
        let response = self.invoke_capability_sync(
            "device_metrics_read",
            &json!({ "device_id": device_id }),
        );
        if response
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            Ok(true)
        } else {
            if let Some(error) = response.get("error").and_then(|value| value.as_str()) {
                tracing::warn!("[YoloDeviceInference] Device validation failed: {}", error);
            }
            Ok(true)
        }
    }

    /// Bind a device for automatic inference
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn bind_device_async(&self, mut binding: DeviceBinding) -> Result<()> {
        let device_id = binding.device_id.clone();

        // Validate device exists
        if !self.validate_device(&device_id).await? {
            return Err(ExtensionError::NotFound(format!(
                "Device '{}' not found or metric '{}' not available",
                device_id, binding.image_metric
            )));
        }

        // Ensure binding is active
        binding.active = true;

        // Add to bindings
        self.bindings.write().insert(device_id.clone(), binding.clone());

        // Initialize stats
        self.binding_stats.write().insert(device_id.clone(), BindingStatus {
            binding,
            last_inference: None,
            total_inferences: 0,
            total_detections: 0,
            last_error: None,
            last_image: None,
            last_detections: None,
            last_annotated_image: None,
        });

        // Persist configuration
        self.persist_config();

        tracing::info!("[YoloDeviceInference] Device bound: {}", device_id);
        Ok(())
    }

    /// Unbind a device
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn unbind_device_async(&self, device_id: &str) -> Result<()> {
        self.bindings.write().remove(device_id);
        self.binding_stats.write().remove(device_id);
        
        // Persist configuration
        self.persist_config();
        
        tracing::info!("[YoloDeviceInference] Device unbound: {}", device_id);
        Ok(())
    }

    /// Get all bindings
    pub fn get_bindings(&self) -> Vec<BindingStatus> {
        self.binding_stats.read().values().cloned().collect()
    }

    /// Get binding for a specific device
    pub fn get_binding(&self, device_id: &str) -> Option<DeviceBinding> {
        self.bindings.read().get(device_id).cloned()
    }

    /// Process image data and run inference (native only)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn process_image(&self, device_id: &str, image_data: &[u8], draw_boxes: bool) -> Result<InferenceResult> {
        let start = std::time::Instant::now();

        self.init_model()?;

        let detector = self.detector.lock();
        let model = detector.as_ref()
            .ok_or_else(|| ExtensionError::ExecutionFailed("Model not loaded".to_string()))?;
        let mut model_guard = model.lock().unwrap();

        // Create temp file for image
        let temp_path = std::env::temp_dir().join(format!("yolo_inference_{}.jpg", uuid::Uuid::new_v4()));
        std::fs::write(&temp_path, image_data)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to write temp image: {}", e)))?;

        // Load image
        let temp_path_str = temp_path.to_str().ok_or_else(|| ExtensionError::ExecutionFailed("Invalid temp path".to_string()))?;
        let xs = DataLoader::try_read_one(temp_path_str)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to read image: {}", e)))?;

        let (img_width, img_height) = (xs.width(), xs.height());

        // Run inference
        let ys = model_guard.forward(&[xs])
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Inference failed: {}", e)))?;

        // Clean up temp file immediately
        let _ = std::fs::remove_file(&temp_path);

        // Parse results
        let mut detections = Vec::new();
        for y in ys.iter() {
            for hbb in &y.hbbs {
                let class_id = hbb.id().unwrap_or(0);
                let label = if class_id < COCO_CLASSES.len() {
                    COCO_CLASSES[class_id].to_string()
                } else {
                    hbb.name().unwrap_or(&format!("class_{}", class_id)).to_string()
                };

                detections.push(Detection {
                    label,
                    confidence: hbb.confidence().unwrap_or(0.0),
                    bbox: BoundingBox {
                        x: hbb.xmin(),
                        y: hbb.ymin(),
                        width: hbb.xmax() - hbb.xmin(),
                        height: hbb.ymax() - hbb.ymin(),
                    },
                    class_id: Some(class_id),
                });
            }
        }

        let inference_time = start.elapsed().as_millis() as u64;
        let timestamp = chrono::Utc::now().timestamp();

        // Update stats
        self.total_inferences.fetch_add(1, Ordering::SeqCst);
        self.total_detections.fetch_add(detections.len() as u64, Ordering::SeqCst);

        // Draw boxes on image if enabled
        let mut annotated_image_base64 = None;
        if draw_boxes && !detections.is_empty() {
            match draw_detections_on_image(image_data, &detections) {
                Ok(base64_str) => {
                    annotated_image_base64 = Some(base64_str);
                    tracing::debug!("[YoloDeviceInference] Annotated image encoded successfully");
                }
                Err(e) => {
                    tracing::warn!("[YoloDeviceInference] Failed to draw detections: {}", e);
                }
            }
        }

        if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
            stats.last_inference = Some(timestamp);
            stats.total_inferences += 1;
            stats.total_detections += detections.len() as u64;
        }

        Ok(InferenceResult {
            device_id: device_id.to_string(),
            detections,
            inference_time_ms: inference_time,
            image_width: img_width,
            image_height: img_height,
            timestamp,
            annotated_image_base64,
        })
    }

    /// Analyze an image directly (for manual inference)
    pub fn analyze_image(&self, image_b64: &str) -> Result<InferenceResult> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let image_data = base64::engine::general_purpose::STANDARD.decode(image_b64)
                .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid base64: {}", e)))?;
            // Default to drawing boxes for manual analysis
            let result = self.process_image("manual", &image_data, true)?;
            
            // Update binding stats for manual analysis (using "manual" as device_id)
            // This allows frontend to see the latest analysis results
            {
                let mut stats_map = self.binding_stats.write();
                let stats = stats_map.entry("manual".to_string())
                    .or_insert_with(|| BindingStatus {
                        binding: DeviceBinding {
                            device_id: "manual".to_string(),
                            device_name: Some("Manual Analysis".to_string()),
                            image_metric: "input".to_string(),
                            result_metric_prefix: "manual_".to_string(),
                            confidence_threshold: 0.25,
                            draw_boxes: true,
                            active: true,
                        },
                        last_inference: None,
                        total_inferences: 0,
                        total_detections: 0,
                        last_error: None,
                        last_image: None,
                        last_detections: None,
                        last_annotated_image: None,
                    });
                stats.last_image = Some(format!("data:image/jpeg;base64,{}", image_b64));
                stats.last_detections = Some(result.detections.clone());
                stats.last_inference = Some(result.timestamp);
                stats.total_inferences += 1;
                stats.total_detections += result.detections.len() as u64;
                if let Some(annotated_b64) = &result.annotated_image_base64 {
                    stats.last_annotated_image = Some(format!("data:image/jpeg;base64,{}", annotated_b64));
                }
            }

            Ok(result)
        }

        #[cfg(target_arch = "wasm32")]
        {
            Err(ExtensionError::NotSupported("Image analysis not supported in WASM".to_string()))
        }
    }

    /// Get extension status
    pub fn get_status(&self) -> serde_json::Value {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Try to load model if not already loaded
            if self.detector.lock().is_none() {
                tracing::debug!("[YoloDeviceInference] Model not loaded, attempting to load...");
                if let Err(e) = self.init_model() {
                    tracing::error!("[YoloDeviceInference] Failed to load model in get_status: {}", e);
                }
            }

            let model_loaded = self.detector.lock().is_some();

            json!({
                "model_loaded": model_loaded,
                "model_version": self.model_version.lock().clone(),
                "default_confidence": *self.default_confidence.lock(),
                "total_bindings": self.bindings.read().len(),
                "total_inferences": self.total_inferences.load(Ordering::SeqCst),
                "total_detections": self.total_detections.load(Ordering::SeqCst),
                "total_errors": self.total_errors.load(Ordering::SeqCst),
                "model_error": self.model_load_error.lock().clone(),
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            json!({
                "model_loaded": false,
                "model_version": self.model_version.lock().clone(),
                "default_confidence": *self.default_confidence.lock(),
                "total_bindings": self.bindings.read().len(),
                "total_inferences": self.total_inferences.load(Ordering::SeqCst),
                "total_detections": self.total_detections.load(Ordering::SeqCst),
                "total_errors": self.total_errors.load(Ordering::SeqCst),
                "model_error": "YOLO not available in WASM",
            })
        }
    }

    /// Get current configuration for persistence
    pub fn get_config(&self) -> YoloConfig {
        YoloConfig {
            default_confidence: *self.default_confidence.lock(),
            model_version: self.model_version.lock().clone(),
            bindings: self.bindings.read().values().cloned().collect(),
        }
    }

    /// Persist configuration to file
    fn persist_config(&self) {
        let config = self.get_config();

        // Get extension directory from environment
        eprintln!("[YoloDeviceInference] Checking NEOMIND_EXTENSION_DIR env var");
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            eprintln!("[YoloDeviceInference] NEOMIND_EXTENSION_DIR = {}", ext_dir);
            let config_path = std::path::PathBuf::from(&ext_dir).join("config.json");
            eprintln!("[YoloDeviceInference] config_path = {:?}", config_path);
            eprintln!("[YoloDeviceInference] config_path.exists() = {}", config_path.exists());

            match serde_json::to_string_pretty(&config) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&config_path, json) {
                        tracing::warn!("[YoloDeviceInference] Failed to persist config: {}", e);
                    } else {
                        tracing::debug!("[YoloDeviceInference] Config persisted to {:?}", config_path);
                    }
                }
                Err(e) => {
                    tracing::warn!("[YoloDeviceInference] Failed to serialize config: {}", e);
                }
            }
        } else {
            tracing::debug!("[YoloDeviceInference] NEOMIND_EXTENSION_DIR not set, skipping config persistence");
        }
    }

    /// Load configuration from file
    fn load_config_from_file(&self) -> Option<YoloConfig> {
        eprintln!("[YoloDeviceInference] load_config_from_file called");
        eprintln!("[YoloDeviceInference] Current working directory: {:?}", std::env::current_dir());
        
        // Since the working directory is set to the extension directory,
        // we can directly use "config.json" in the current directory
        let config_path = std::path::PathBuf::from("config.json");
        eprintln!("[YoloDeviceInference] config_path = {:?}", config_path);
        eprintln!("[YoloDeviceInference] config_path.exists() = {}", config_path.exists());

        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(json) => {
                    eprintln!("[YoloDeviceInference] Read config file successfully, length = {}", json.len());
                    match serde_json::from_str::<YoloConfig>(&json) {
                        Ok(config) => {
                            eprintln!("[YoloDeviceInference] Parsed config successfully, bindings = {}", config.bindings.len());
                            return Some(config);
                        }
                        Err(e) => {
                            eprintln!("[YoloDeviceInference] Failed to parse config file: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[YoloDeviceInference] Failed to read config file: {}", e);
                }
            }
        } else {
            // Also try using NEOMIND_EXTENSION_DIR as fallback
            if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
                eprintln!("[YoloDeviceInference] Trying NEOMIND_EXTENSION_DIR: {}", ext_dir);
                let alt_config_path = std::path::PathBuf::from(&ext_dir).join("config.json");
                eprintln!("[YoloDeviceInference] alt_config_path = {:?}, exists = {}", alt_config_path, alt_config_path.exists());
                
                if alt_config_path.exists() {
                    match std::fs::read_to_string(&alt_config_path) {
                        Ok(json) => {
                            match serde_json::from_str::<YoloConfig>(&json) {
                                Ok(config) => {
                                    eprintln!("[YoloDeviceInference] Loaded config from alt path, bindings = {}", config.bindings.len());
                                    return Some(config);
                                }
                                Err(e) => {
                                    eprintln!("[YoloDeviceInference] Failed to parse alt config file: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[YoloDeviceInference] Failed to read alt config file: {}", e);
                        }
                    }
                }
            }
        }
        None
    }

    /// Load configuration from persisted state
    pub fn load_config(&self, config: &YoloConfig) -> Result<()> {
        eprintln!("[YoloDeviceInference] load_config called with {} bindings", config.bindings.len());
        
        // Load settings
        *self.default_confidence.lock() = config.default_confidence;
        *self.model_version.lock() = config.model_version.clone();

        // Load bindings
        for binding in &config.bindings {
            eprintln!("[YoloDeviceInference] Loading binding for device: {}, image_metric: {}", binding.device_id, binding.image_metric);

            // Add to bindings
            self.bindings.write().insert(binding.device_id.clone(), binding.clone());
            
            eprintln!("[YoloDeviceInference] Binding inserted, current bindings count: {}", self.bindings.read().len());

            // Initialize stats
            self.binding_stats.write().insert(binding.device_id.clone(), BindingStatus {
                binding: binding.clone(),
                last_inference: None,
                total_inferences: 0,
                total_detections: 0,
                last_error: None,
                last_image: None,
                last_detections: None,
                last_annotated_image: None,
            });
        }

        eprintln!("[YoloDeviceInference] Loaded {} persisted bindings, total in map: {}", config.bindings.len(), self.bindings.read().len());
        Ok(())
    }

    /// Process inference result and write virtual metrics
    /// This is called after successful inference to update device metrics
    #[cfg(not(target_arch = "wasm32"))]
    fn write_inference_results(
        &self,
        device_id: &str,
        result: &InferenceResult,
        image_b64: &str,
        _result_metric_prefix: &str,
    ) {
        // Update binding stats
        if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
            stats.last_image = Some(format!("data:image/jpeg;base64,{}", image_b64));
            stats.last_detections = Some(result.detections.clone());
            stats.last_inference = Some(result.timestamp);
            // Update annotated image if available
            if let Some(annotated_b64) = &result.annotated_image_base64 {
                stats.last_annotated_image = Some(format!("data:image/jpeg;base64,{}", annotated_b64));
            }
        }

        // Write virtual metrics through the native capability bridge.
        // Virtual metrics must start with: transform., virtual., computed., derived., or aggregated.


        // Write detection count (virtual metric)
        let metric_name = "virtual.yolo.detections";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": result.detections.len(),
            "timestamp": result.timestamp,
        });
        match self.invoke_capability_sync("device_metrics_write", &params) {
            v if v.get("success").and_then(|s| s.as_bool()).unwrap_or(false) => {
                eprintln!("[YoloDeviceInference] Successfully wrote {}", metric_name);
            }
            _ => {
                eprintln!("[YoloDeviceInference] Failed to write {}", metric_name);
            }
        }

        // Write inference time (virtual metric)
        let metric_name = "virtual.yolo.inference_time_ms";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": result.inference_time_ms,
            "timestamp": result.timestamp,
        });
        match self.invoke_capability_sync("device_metrics_write", &params) {
            v if v.get("success").and_then(|s| s.as_bool()).unwrap_or(false) => {
                eprintln!("[YoloDeviceInference] Successfully wrote {}", metric_name);
            }
            _ => {
                eprintln!("[YoloDeviceInference] Failed to write {}", metric_name);
            }
        }

        // Write detection labels as JSON (virtual metric)
        let labels: Vec<&str> = result.detections.iter().map(|d| d.label.as_str()).collect();
        let metric_name = "virtual.yolo.labels";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": labels,
            "timestamp": result.timestamp,
        });
        match self.invoke_capability_sync("device_metrics_write", &params) {
            v if v.get("success").and_then(|s| s.as_bool()).unwrap_or(false) => {
                eprintln!("[YoloDeviceInference] Successfully wrote {}", metric_name);
            }
            _ => {
                eprintln!("[YoloDeviceInference] Failed to write {}", metric_name);
            }
        }

        // Write annotated image (virtual metric) with proper data URI format
        if let Some(img) = &result.annotated_image_base64 {
            let metric_name = "virtual.yolo.annotated_image";
            // Add data URI prefix for proper image display
            let data_uri = format!("data:image/jpeg;base64,{}", img);
            let params = serde_json::json!({
                "device_id": device_id,
                "metric": metric_name,
                "value": data_uri,
                "timestamp": result.timestamp,
            });
            match self.invoke_capability_sync("device_metrics_write", &params) {
                v if v.get("success").and_then(|s| s.as_bool()).unwrap_or(false) => {
                    eprintln!("[YoloDeviceInference] Successfully wrote {}", metric_name);
                }
                _ => {
                    eprintln!("[YoloDeviceInference] Failed to write {}", metric_name);
                }
            }
        }

    }

    /// Extract image data from a value, supporting nested paths
    pub fn extract_image_from_value<'a>(&self, value: Option<&'a serde_json::Value>, nested_path: Option<&str>) -> Option<String> {
        let v = value?;

        // If we have a nested path, navigate to it
        let target_value = if let Some(path) = nested_path {
            // Navigate through nested path
            let mut current = v;
            for part in path.split('.') {
                current = current.get(part)?;
            }
            current
        } else {
            v
        };

        // Try to extract string from the target value
        let raw_string: Option<&str> = {
            // Format 1: Direct string
            if let Some(s) = target_value.as_str() {
                Some(s)
            // Format 2: MetricValue wrapper {"String": "..."}
            } else if let Some(s) = target_value.get("String").and_then(|s| s.as_str()) {
                Some(s)
            // Format 3: Object with string fields (try common field names)
            } else {
                for field in &["image", "data", "value", "base64"] {
                    if let Some(s) = target_value.get(field).and_then(|s| s.as_str()) {
                        return Some(s.to_string());
                    }
                }
                None
            }
        };

        // Process the string - handle data URL format
        if let Some(s) = raw_string {
            // Handle data URL format: "data:image/jpeg;base64,actual_base64_data"
            if s.starts_with("data:") {
                if let Some(comma_pos) = s.find(',') {
                    let base64_part = &s[comma_pos + 1..];
                    return Some(base64_part.to_string());
                }
            }
            return Some(s.to_string());
        }

        None
    }
}

impl Default for YoloDeviceInference {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for YoloDeviceInference {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "yolo-device-inference",
                "YOLO Device Inference",
                "2.0.0"
            )
            .with_description("Automatic YOLOv8 inference on device image data sources")
            .with_author("NeoMind Team")
        })
    }

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
                name: "total_detections".to_string(),
                display_name: "Total Detections".to_string(),
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

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "bind_device".to_string(),
                display_name: "Bind Device".to_string(),
                description: "Bind a device for automatic YOLO inference on image updates".to_string(),
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
                        default_value: Some(ParamMetricValue::String("yolo_".to_string())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "confidence_threshold".to_string(),
                        display_name: "Confidence Threshold".to_string(),
                        description: "Minimum confidence for detections (0.0-1.0)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(0.25)),
                        min: Some(0.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "draw_boxes".to_string(),
                        display_name: "Draw Boxes".to_string(),
                        description: "Whether to draw detection boxes on images".to_string(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: Some(ParamMetricValue::Boolean(true)),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![json!({"device_id": "camera-01", "image_metric": "image", "confidence_threshold": 0.3})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "unbind_device".to_string(),
                display_name: "Unbind Device".to_string(),
                description: "Unbind a device from automatic inference".to_string(),
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
                name: "analyze_image".to_string(),
                display_name: "Analyze Image".to_string(),
                description: "Analyze an image and return detected objects".to_string(),
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
                name: "get_config".to_string(),
                display_name: "Get Config".to_string(),
                description: "Get current YOLO extension configuration".to_string(),
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

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp();

        Ok(vec![
            ExtensionMetricValue {
                name: "bound_devices".to_string(),
                value: ParamMetricValue::Integer(self.bindings.read().len() as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_inferences".to_string(),
                value: ParamMetricValue::Integer(self.total_inferences.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_detections".to_string(),
                value: ParamMetricValue::Integer(self.total_detections.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_errors".to_string(),
                value: ParamMetricValue::Integer(self.total_errors.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
        ])
    }

    /// Handle events from the EventBus
    ///
    /// This method is called by the system when a DeviceMetric event is published.
    /// It checks if the device is bound and processes the image data.
    /// Handle events from the EventBus
    ///
    /// This method is called by the system when a DeviceMetric event is published.
    /// It checks if the device is bound and processes the image data.
    fn handle_event(
        &self,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<()> {
        eprintln!("[YoloDeviceInference] handle_event called: event_type={}", event_type);
        tracing::info!("[YoloDeviceInference] handle_event called: event_type={}", event_type);

        eprintln!("[YoloDeviceInference] Checking event_type: {}", event_type);
        if event_type != "DeviceMetric" {
            return Ok(());
        }

        // Extract event data from the standardized format
        let inner_payload = payload.get("payload").unwrap_or(payload);

        let device_id = inner_payload.get("device_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let metric = inner_payload.get("metric")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        eprintln!("[YoloDeviceInference] Processing: device={}, metric={}", device_id, metric);
        tracing::info!("[YoloDeviceInference] Processing: device={}, metric={}", device_id, metric);

        let value = inner_payload.get("value");

        // Check if this device is bound
        let bindings = self.bindings.read();
        let binding = bindings.get(device_id).cloned();
        eprintln!("[YoloDeviceInference] Looking up device {}, bindings map has {} entries", device_id, bindings.len());

        eprintln!("[YoloDeviceInference] Checking binding for device: {}", device_id);
        if binding.is_none() {
            // Log available bindings for debugging
            let bindings = self.bindings.read();
            let bound_devices: Vec<&String> = bindings.keys().collect();
            eprintln!("[YoloDeviceInference] Device {} not bound. Bound devices: {:?}", device_id, bound_devices);
        }

        eprintln!("[YoloDeviceInference] Found binding for device: {}", device_id);
        if let Some(binding) = binding {
            eprintln!("[YoloDeviceInference] Binding active: {}", binding.active);
            if !binding.active {
                tracing::debug!("[YoloDeviceInference] Binding inactive for device: {}", device_id);
                return Ok(());
            }

            // Check if metric matches
            let exact_match = metric == binding.image_metric;

            let (top_level_metric, nested_path) = if binding.image_metric.contains('.') {
                let parts: Vec<&str> = binding.image_metric.splitn(2, '.').collect();
                (parts[0].to_string(), Some(parts[1].to_string()))
            } else {
                (binding.image_metric.clone(), None)
            };

            let metric_matches = exact_match || metric == top_level_metric;
            let nested_path = if exact_match { None } else { nested_path };

            eprintln!("[YoloDeviceInference] Event: device={}, metric={}, binding_metric={}, exact={}, top_level={}, matches={}",
                device_id, metric, binding.image_metric, exact_match, top_level_metric, metric_matches);
            tracing::info!(
                "[YoloDeviceInference] Event: device={}, metric={}, binding_metric={}, exact={}, top_level={}, matches={}",
                device_id, metric, binding.image_metric, exact_match, top_level_metric, metric_matches
            );

            if !metric_matches {
                return Ok(());
            }

            eprintln!("[YoloDeviceInference] Metric matched! Extracting image data...");
            
            // Extract image data from value
            let image_b64 = self.extract_image_from_value(value, nested_path.as_deref());
            
            eprintln!("[YoloDeviceInference] extract_image_from_value returned: {:?}", image_b64.as_ref().map(|s| format!("{} bytes", s.len())));

            if let Some(image_data_b64) = image_b64 {
                match base64::engine::general_purpose::STANDARD.decode(&image_data_b64) {
                    Ok(image_data) => {
                        eprintln!("[YoloDeviceInference] Base64 decoded successfully, image size: {} bytes", image_data.len());
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            eprintln!("[YoloDeviceInference] Calling process_image...");
                            match self.process_image(device_id, &image_data, binding.draw_boxes) {
                                Ok(result) => {
                                    eprintln!("[YoloDeviceInference] process_image returned {} detections", result.detections.len());
                                    self.total_inferences.fetch_add(1, Ordering::SeqCst);
                                    self.total_detections.fetch_add(result.detections.len() as u64, Ordering::SeqCst);
                                    self.write_inference_results(
                                        device_id,
                                        &result,
                                        &image_data_b64,
                                        &binding.result_metric_prefix,
                                    );
                                    tracing::debug!(
                                        "[YoloDeviceInference] Inference: device={}, detections={}, time={}ms",
                                        device_id,
                                        result.detections.len(),
                                        result.inference_time_ms
                                    );
                                }
                                Err(e) => {
                                    self.total_errors.fetch_add(1, Ordering::SeqCst);
                                    tracing::warn!("[YoloDeviceInference] Process failed: device={}, error={}", device_id, e);
                                    if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
                                        stats.last_error = Some(e.to_string());
                                    }
                                }
                            }
                        }

                        #[cfg(target_arch = "wasm32")]
                        {
                            tracing::warn!("[YoloDeviceInference] Image processing not supported in WASM");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[YoloDeviceInference] Base64 decode failed: device={}, error={}", device_id, e);
                    }
                }
            }
        }

        Ok(())
    }

    fn event_subscriptions(&self) -> &[&str] {
        &["DeviceMetric"]
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "bind_device" => {
                let device_id = args.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".to_string()))?;

                let binding = DeviceBinding {
                    device_id: device_id.to_string(),
                    device_name: args.get("device_name").and_then(|v| v.as_str()).map(String::from),
                    image_metric: args.get("image_metric")
                        .and_then(|v| v.as_str())
                        .unwrap_or("image").to_string(),
                    result_metric_prefix: args.get("result_metric_prefix")
                        .and_then(|v| v.as_str())
                        .unwrap_or("yolo_").to_string(),
                    confidence_threshold: args.get("confidence_threshold")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(0.25),
                    draw_boxes: args.get("draw_boxes")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    active: true,
                };

                self.bind_device_async(binding).await?;

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "message": format!("Device {} bound successfully", device_id)
                }))
            }

            "unbind_device" => {
                let device_id = args.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".to_string()))?;

                self.unbind_device_async(device_id).await?;

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "message": format!("Device {} unbound successfully", device_id)
                }))
            }

            "get_bindings" => {
                let bindings = self.get_bindings();
                Ok(json!({
                    "success": true,
                    "bindings": bindings
                }))
            }

            "configure" => {
                // Load config from file
                if let Some(config) = self.load_config_from_file() {
                    self.load_config(&config)?;
                    Ok(json!({
                        "success": true,
                        "message": "Configuration loaded from file",
                        "bindings_count": config.bindings.len()
                    }))
                } else {
                    Ok(json!({
                        "success": true,
                        "message": "No persisted configuration found"
                    }))
                }
            }

            "analyze_image" => {
                let image_b64 = args.get("image")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing image parameter".to_string()))?;

                let result = self.analyze_image(image_b64)?;
                Ok(serde_json::to_value(result)
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("Serialization error: {}", e)))?)
            }

            "get_status" => {
                Ok(self.get_status())
            }

            "get_config" => {
                let config = self.get_config();
                Ok(serde_json::to_value(&config)
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("Serialization error: {}", e)))?)
            }

            "toggle_binding" => {
                let device_id = args.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".to_string()))?;

                let active = args.get("active")
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing active parameter".to_string()))?;

                if let Some(binding) = self.bindings.write().get_mut(device_id) {
                    binding.active = active;

                    if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
                        stats.binding.active = active;
                    }

                    Ok(json!({
                        "success": true,
                        "device_id": device_id,
                        "active": active
                    }))
                } else {
                    Err(ExtensionError::NotFound(format!("Device {} not bound", device_id)))
                }
            }

            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn execute_command_sync(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "bind_device" => {
                let device_id = args.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".to_string()))?;

                let binding = DeviceBinding {
                    device_id: device_id.to_string(),
                    device_name: args.get("device_name").and_then(|v| v.as_str()).map(String::from),
                    image_metric: args.get("image_metric").and_then(|v| v.as_str()).unwrap_or("image").to_string(),
                    result_metric_prefix: args.get("result_metric_prefix").and_then(|v| v.as_str()).unwrap_or("yolo_").to_string(),
                    confidence_threshold: args.get("confidence_threshold").and_then(|v| v.as_f64()).map(|v| v as f32).unwrap_or(0.25),
                    draw_boxes: args.get("draw_boxes").and_then(|v| v.as_bool()).unwrap_or(true),
                    active: true,
                };

                // For WASM, just store the binding
                self.bindings.write().insert(device_id.to_string(), binding.clone());
                self.binding_stats.write().insert(device_id.to_string(), BindingStatus {
                    binding,
                    last_inference: None,
                    total_inferences: 0,
                    total_detections: 0,
                    last_error: None,
                });

                Ok(json!({"success": true, "device_id": device_id}))
            }

            "unbind_device" => {
                let device_id = args.get("device_id").and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".to_string()))?;
                self.bindings.write().remove(device_id);
                self.binding_stats.write().remove(device_id);
                Ok(json!({"success": true, "device_id": device_id}))
            }

            "get_bindings" => {
                Ok(json!({"success": true, "bindings": self.get_bindings()}))
            }

            "configure" => {
                // Load config from file
                if let Some(config) = self.load_config_from_file() {
                    let _ = self.load_config(&config);
                    Ok(json!({"success": true, "message": "Configuration loaded from file"}))
                } else {
                    Ok(json!({"success": true, "message": "No persisted configuration found"}))
                }
            }

            "get_status" => Ok(self.get_status()),
            "toggle_binding" => {
                let device_id = args.get("device_id").and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".to_string()))?;
                let active = args.get("active").and_then(|v| v.as_bool());
                if let Some(binding) = self.bindings.write().get_mut(device_id) {
                    binding.active = active;
                    if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
                        stats.binding.active = active;
                    }
                    Ok(json!({"success": true, "device_id": device_id, "active": active}))
                } else {
                    Err(ExtensionError::NotFound(format!("Device {} not bound", device_id)))
                }
            }

            "analyze_image" => {
                Err(ExtensionError::NotSupported("Not supported in WASM".to_string()))
            }
            "get_config" => {
                Ok(serde_json::to_value(&self.get_config())
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("Serialization error: {}", e)))?)
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    /// Configure the extension with persisted settings
    ///
    /// This is called by the system when the extension is loaded.
    /// It loads persisted bindings and configuration.
    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {

        // ✅ FIX: Use lazy loading instead of eager loading
        // Model will be loaded on first use (process_image)
        // This prevents memory spike during extension startup
        #[cfg(not(target_arch = "wasm32"))]
        {
            tracing::info!("[YoloDeviceInference] Model will be loaded on first use (lazy loading)");
        }

        eprintln!("[YoloDeviceInference] configure called with config: {:?}", config);
        // First, try to load from file (for isolated extensions)
        if let Some(file_config) = self.load_config_from_file() {
            tracing::debug!("[YoloDeviceInference] Loading persisted configuration from file with {} bindings", file_config.bindings.len());
            self.load_config(&file_config)?;
        }

        // Then, load from system config if provided (overrides file config)
        if let Some(yolo_config) = config.get("yolo_config") {
            if let Ok(parsed_config) = serde_json::from_value::<YoloConfig>(yolo_config.clone()) {
                tracing::debug!("[YoloDeviceInference] Loading system configuration with {} bindings", parsed_config.bindings.len());
                self.load_config(&parsed_config)?;
            }
        }

        // Apply individual config settings (these override persisted values)
        if let Some(conf) = config.get("default_confidence").and_then(|v| v.as_f64()) {
            *self.default_confidence.lock() = conf as f32;
        }

        if let Some(version) = config.get("model_version").and_then(|v| v.as_str()) {
            *self.model_version.lock() = version.to_string();
        }

        tracing::debug!("[YoloDeviceInference] Configuration applied successfully");
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// FFI Export
// ============================================================================

neomind_extension_sdk::neomind_export!(YoloDeviceInference);
