//! NeoMind Face Recognition Extension
//!
//! This extension provides real-time face recognition with device binding,
//! face registration, and identity matching using SCRFD for detection and
//! ArcFace for feature extraction.
//!
//! # Features
//! - Bind/unbind devices with image data sources for automatic face detection
//! - Register faces with names for identity recognition
//! - Event-driven face recognition on device data updates
//! - Store recognition results as virtual metrics on the device
//!
//! # Event Handling
//! This extension uses the SDK's built-in event handling mechanism:
//! - `event_subscriptions()` declares which events to subscribe to
//! - `handle_event()` is called by the system when events are received

pub mod alignment;
pub mod database;
pub mod detector;
pub mod drawing;
pub mod recognizer;

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
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use base64::Engine;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use crate::database::FaceDatabase;
use crate::detector::{FaceDetect, ScrfdDetector};
use crate::recognizer::{FaceExtract, ArcFaceRecognizer};

// ============================================================================
// Types
// ============================================================================

/// A single facial landmark point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Landmark {
    pub x: f64,
    pub y: f64,
}

/// Bounding box for a detected face
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landmarks: Option<Vec<Landmark>>,
}

/// Result for a single detected face
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceResult {
    pub face_box: FaceBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face_id: Option<String>,
}

/// Device binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceBinding {
    pub device_id: String,
    pub metric_name: String,
    pub active: bool,
    pub created_at: i64,
}

/// Statistics for a device binding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingStats {
    pub total_inferences: u64,
    pub total_recognized: u64,
    pub total_unknown: u64,
}

/// Extension configuration for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceRecConfig {
    pub confidence_threshold: f64,
    pub recognition_threshold: f64,
    pub max_faces: usize,
    pub auto_detect: bool,
    #[serde(default)]
    pub bindings: Vec<DeviceBinding>,
}

impl Default for FaceRecConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.5,
            recognition_threshold: 0.45,
            max_faces: 10,
            auto_detect: true,
            bindings: Vec::new(),
        }
    }
}

// ============================================================================
// Extension Struct
// ============================================================================

pub struct FaceRecognition {
    /// SCRFD face detector (lazy loading)
    detector: Mutex<Box<dyn FaceDetect + Send>>,
    /// ArcFace face feature extractor (lazy loading)
    recognizer: Mutex<Box<dyn FaceExtract + Send>>,
    /// Device bindings: device_id -> binding
    bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,
    /// Binding statistics: device_id -> stats
    binding_stats: Arc<RwLock<HashMap<String, BindingStats>>>,
    /// Face database
    face_db: Arc<RwLock<FaceDatabase>>,
    /// Extension configuration
    config: Arc<RwLock<FaceRecConfig>>,
    /// Global statistics
    total_inferences: Arc<AtomicU64>,
    total_recognized: Arc<AtomicU64>,
    total_unknown: Arc<AtomicU64>,
}

impl FaceRecognition {
    pub fn new() -> Self {
        Self {
            detector: Mutex::new(Box::new(ScrfdDetector::new())),
            recognizer: Mutex::new(Box::new(ArcFaceRecognizer::new())),
            bindings: Arc::new(RwLock::new(HashMap::new())),
            binding_stats: Arc::new(RwLock::new(HashMap::new())),
            face_db: Arc::new(RwLock::new(FaceDatabase::new(0.45, 10))),
            config: Arc::new(RwLock::new(FaceRecConfig::default())),
            total_inferences: Arc::new(AtomicU64::new(0)),
            total_recognized: Arc::new(AtomicU64::new(0)),
            total_unknown: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Invoke a host capability synchronously via the capability context.
    fn invoke_capability_sync(
        &self,
        capability_name: &str,
        params: &serde_json::Value,
    ) -> serde_json::Value {
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

    /// Extract image data from a value, supporting nested paths and data URIs.
    pub fn extract_image_from_value<'a>(
        &self,
        value: Option<&'a serde_json::Value>,
        nested_path: Option<&str>,
    ) -> Option<String> {
        let v = value?;

        // Navigate through nested path if provided
        let target_value = if let Some(path) = nested_path {
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

    /// Get extension status including model and database info.
    pub fn get_status(&self) -> serde_json::Value {
        // Trigger lazy loading by calling detect/extract with empty data
        // to get accurate model status
        #[cfg(not(target_arch = "wasm32"))]
        {
            // We probe the models by attempting to use them (they will fail on empty input
            // but that's fine for status checking). The ensure_loaded happens internally.
            let _ = self.detector.lock().detect(&[], 0.5);
            let _ = self.recognizer.lock().extract(&[]);
        }

        let face_db = self.face_db.read();
        let config = self.config.read();

        json!({
            "total_bindings": self.bindings.read().len(),
            "total_inferences": self.total_inferences.load(Ordering::SeqCst),
            "total_recognized": self.total_recognized.load(Ordering::SeqCst),
            "total_unknown": self.total_unknown.load(Ordering::SeqCst),
            "registered_faces": face_db.len(),
            "config": *config,
        })
    }

    /// Get the current face database path based on NEOMIND_EXTENSION_DIR.
    fn get_faces_db_path() -> Option<std::path::PathBuf> {
        std::env::var("NEOMIND_EXTENSION_DIR")
            .ok()
            .map(|dir| std::path::PathBuf::from(dir).join("faces.json"))
    }

    /// Get the current config file path.
    fn get_config_path() -> Option<std::path::PathBuf> {
        std::env::var("NEOMIND_EXTENSION_DIR")
            .ok()
            .map(|dir| std::path::PathBuf::from(dir).join("config.json"))
    }

    /// Save face database to faces.json.
    fn save_face_database(&self) {
        if let Some(path) = Self::get_faces_db_path() {
            let db = self.face_db.read();
            if let Err(e) = db.save_to_file(&path) {
                tracing::warn!(
                    "[FaceRecognition] Failed to save face database: {}",
                    e
                );
            } else {
                tracing::debug!(
                    "[FaceRecognition] Face database saved to {}",
                    path.display()
                );
            }
        }
    }

    /// Persist configuration to config.json.
    fn persist_config(&self) {
        if let Some(path) = Self::get_config_path() {
            let config = self.config.read();
            let mut config_to_save = config.clone();
            config_to_save.bindings = self.bindings.read().values().cloned().collect();

            match serde_json::to_string_pretty(&config_to_save) {
                Ok(json_str) => {
                    if let Err(e) = std::fs::write(&path, json_str) {
                        tracing::warn!(
                            "[FaceRecognition] Failed to persist config: {}",
                            e
                        );
                    } else {
                        tracing::debug!(
                            "[FaceRecognition] Config persisted to {}",
                            path.display()
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "[FaceRecognition] Failed to serialize config: {}",
                        e
                    );
                }
            }
        }
    }

    /// Load configuration from config.json file.
    fn load_config_from_file(&self) -> Option<FaceRecConfig> {
        // Try current directory first (extension runner sets cwd)
        let config_path = std::path::PathBuf::from("config.json");
        if config_path.exists() {
            if let Ok(json_str) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<FaceRecConfig>(&json_str) {
                    tracing::info!(
                        "[FaceRecognition] Loaded config from file with {} bindings",
                        config.bindings.len()
                    );
                    return Some(config);
                }
            }
        }

        // Try NEOMIND_EXTENSION_DIR as fallback
        if let Some(path) = Self::get_config_path() {
            if path.exists() {
                if let Ok(json_str) = std::fs::read_to_string(&path) {
                    if let Ok(config) = serde_json::from_str::<FaceRecConfig>(&json_str) {
                        tracing::info!(
                            "[FaceRecognition] Loaded config from {} with {} bindings",
                            path.display(),
                            config.bindings.len()
                        );
                        return Some(config);
                    }
                }
            }
        }

        None
    }

    /// Load persisted bindings from a config into the runtime state.
    fn restore_bindings(&self, config: &FaceRecConfig) {
        for binding in &config.bindings {
            self.bindings
                .write()
                .insert(binding.device_id.clone(), binding.clone());
            self.binding_stats.write().insert(
                binding.device_id.clone(),
                BindingStats {
                    total_inferences: 0,
                    total_recognized: 0,
                    total_unknown: 0,
                },
            );
            tracing::info!(
                "[FaceRecognition] Restored binding for device: {}",
                binding.device_id
            );
        }
    }

    /// Write recognition results as virtual metrics on the device.
    fn write_recognition_results(
        &self,
        device_id: &str,
        face_count: usize,
        face_names: &[String],
        annotated_image_b64: &str,
        avg_confidence: f64,
        timestamp: i64,
    ) {
        // Write face count
        let params = json!({
            "device_id": device_id,
            "metric": "face_recognition.face_count",
            "value": face_count,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write face_count metric");
        }

        // Write face names
        let names_str = face_names.join(",");
        let params = json!({
            "device_id": device_id,
            "metric": "face_recognition.face_names",
            "value": names_str,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write face_names metric");
        }

        // Write annotated image
        let data_uri = format!("data:image/jpeg;base64,{}", annotated_image_b64);
        let params = json!({
            "device_id": device_id,
            "metric": "face_recognition.annotated_image",
            "value": data_uri,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write annotated_image metric");
        }

        // Write confidence
        let params = json!({
            "device_id": device_id,
            "metric": "face_recognition.confidence",
            "value": avg_confidence,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write confidence metric");
        }
    }

    /// Run the full face recognition pipeline on image data.
    /// Returns a tuple of (face_results, annotated_image_base64).
    #[cfg(not(target_arch = "wasm32"))]
    fn run_recognition_pipeline(
        &self,
        image_data: &[u8],
    ) -> Result<(Vec<FaceResult>, String)> {
        let config = self.config.read();
        let confidence = config.confidence_threshold as f32;

        // Step 1: Detect faces
        let faces = {
            let mut detector = self.detector.lock();
            detector
                .detect(image_data, confidence)
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Detection failed: {}", e)))?
        };

        if faces.is_empty() {
            return Ok((Vec::new(), String::new()));
        }

        // Step 2: For each face, align, extract features, and match
        let img = image::load_from_memory(image_data)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to load image: {}", e)))?;

        let mut results = Vec::with_capacity(faces.len());
        let db = self.face_db.read();

        for face_box in &faces {
            // Align the face
            let aligned = crate::alignment::align_face(&img, face_box);

            // Encode aligned face to JPEG bytes for the recognizer
            let mut aligned_bytes = Vec::new();
            let aligned_rgb = aligned.to_rgb8();
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut aligned_bytes,
                95,
            );
            encoder
                .encode(
                    aligned_rgb.as_raw(),
                    aligned_rgb.width(),
                    aligned_rgb.height(),
                    image::ColorType::Rgb8.into(),
                )
                .map_err(|e| {
                    ExtensionError::ExecutionFailed(format!(
                        "Failed to encode aligned face: {}",
                        e
                    ))
                })?;

            // Extract features
            let feature = {
                let mut recognizer = self.recognizer.lock();
                recognizer.extract(&aligned_bytes).map_err(|e| {
                    ExtensionError::ExecutionFailed(format!("Feature extraction failed: {}", e))
                })?
            };

            // Match against database
            let match_result = db.match_face(&feature);

            let face_result = match match_result {
                Some(m) => FaceResult {
                    face_box: face_box.clone(),
                    name: Some(m.name.clone()),
                    similarity: Some(m.similarity),
                    face_id: Some(m.face_id),
                },
                None => FaceResult {
                    face_box: face_box.clone(),
                    name: None,
                    similarity: None,
                    face_id: None,
                },
            };
            results.push(face_result);
        }

        // Step 3: Draw recognition results on the image
        let annotated_b64 = if !results.is_empty() {
            drawing::draw_recognition_results(image_data, &results)
                .map_err(|e| {
                    ExtensionError::ExecutionFailed(format!("Drawing failed: {}", e))
                })?
        } else {
            String::new()
        };

        Ok((results, annotated_b64))
    }
}

impl Default for FaceRecognition {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for FaceRecognition {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "face-recognition",
                "Face Recognition",
                "0.1.0",
            )
            .with_description("Real-time face recognition with device binding, face registration, and identity matching")
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
                name: "total_recognized".to_string(),
                display_name: "Total Recognized".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_unknown".to_string(),
                display_name: "Total Unknown".to_string(),
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
            // 1. bind_device
            ExtensionCommand {
                name: "bind_device".to_string(),
                display_name: "Bind Device".to_string(),
                description: "Bind a device for automatic face recognition on image updates".to_string(),
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
                        name: "metric_name".to_string(),
                        display_name: "Metric Name".to_string(),
                        description: "Name of the image data source metric".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: Some(ParamMetricValue::String("image".to_string())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![json!({"device_id": "camera-01", "metric_name": "image"})],
                parameter_groups: Vec::new(),
            },
            // 2. unbind_device
            ExtensionCommand {
                name: "unbind_device".to_string(),
                display_name: "Unbind Device".to_string(),
                description: "Unbind a device from automatic face recognition".to_string(),
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
            // 3. toggle_binding
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
            // 4. get_bindings
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
            // 5. register_face
            ExtensionCommand {
                name: "register_face".to_string(),
                display_name: "Register Face".to_string(),
                description: "Register a face with a name for identity recognition".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "name".to_string(),
                        display_name: "Name".to_string(),
                        description: "Name to associate with the face".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "image".to_string(),
                        display_name: "Image".to_string(),
                        description: "Base64 encoded image containing the face".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![json!({"name": "John", "image": "base64_encoded_image_data"})],
                parameter_groups: Vec::new(),
            },
            // 6. delete_face
            ExtensionCommand {
                name: "delete_face".to_string(),
                display_name: "Delete Face".to_string(),
                description: "Delete a registered face by ID".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "face_id".to_string(),
                        display_name: "Face ID".to_string(),
                        description: "ID of the face to delete".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![json!({"face_id": "uuid-of-face"})],
                parameter_groups: Vec::new(),
            },
            // 7. list_faces
            ExtensionCommand {
                name: "list_faces".to_string(),
                display_name: "List Faces".to_string(),
                description: "List all registered faces".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            // 8. get_status
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
            // 9. configure
            ExtensionCommand {
                name: "configure".to_string(),
                display_name: "Configure".to_string(),
                description: "Update extension configuration".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "config".to_string(),
                        display_name: "Configuration".to_string(),
                        description: "Configuration object to apply".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![json!({"config": {"recognition_threshold": 0.5, "max_faces": 20}})],
                parameter_groups: Vec::new(),
            },
            // 10. get_config
            ExtensionCommand {
                name: "get_config".to_string(),
                display_name: "Get Config".to_string(),
                description: "Get current extension configuration".to_string(),
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
                name: "total_recognized".to_string(),
                value: ParamMetricValue::Integer(self.total_recognized.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_unknown".to_string(),
                value: ParamMetricValue::Integer(self.total_unknown.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
        ])
    }

    fn event_subscriptions(&self) -> &[&str] {
        &["DeviceMetric"]
    }

    fn handle_event(
        &self,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<()> {
        if event_type != "DeviceMetric" {
            return Ok(());
        }

        tracing::info!("[FaceRecognition] handle_event called: event_type={}", event_type);

        // Extract event data from the standardized format
        let inner_payload = payload.get("payload").unwrap_or(payload);

        let device_id = inner_payload
            .get("device_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let metric = inner_payload
            .get("metric")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let value = inner_payload.get("value");

        tracing::info!(
            "[FaceRecognition] Processing event: device={}, metric={}",
            device_id,
            metric
        );

        // Check if this device is bound
        let binding = self.bindings.read().get(device_id).cloned();
        if let Some(binding) = binding {
            if !binding.active {
                tracing::debug!(
                    "[FaceRecognition] Binding inactive for device: {}",
                    device_id
                );
                return Ok(());
            }

            // Check if metric matches the binding's metric name
            let (top_level_metric, nested_path) =
                if binding.metric_name.contains('.') {
                    let parts: Vec<&str> = binding.metric_name.splitn(2, '.').collect();
                    (parts[0].to_string(), Some(parts[1].to_string()))
                } else {
                    (binding.metric_name.clone(), None)
                };

            let metric_matches = metric == binding.metric_name || metric == top_level_metric;
            let nested_path = if metric == binding.metric_name {
                None
            } else {
                nested_path
            };

            if !metric_matches {
                return Ok(());
            }

            // Extract image data from the value
            let image_b64 = self.extract_image_from_value(value, nested_path.as_deref());

            if let Some(image_data_b64) = image_b64 {
                match base64::engine::general_purpose::STANDARD.decode(&image_data_b64) {
                    Ok(image_data) => {
                        tracing::info!(
                            "[FaceRecognition] Processing image for device {}: {} bytes",
                            device_id,
                            image_data.len()
                        );

                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            match self.run_recognition_pipeline(&image_data) {
                                Ok((results, annotated_b64)) => {
                                    let face_count = results.len();
                                    let recognized_count = results
                                        .iter()
                                        .filter(|r| r.name.is_some())
                                        .count();
                                    let unknown_count = face_count - recognized_count;

                                    // Update global counters
                                    self.total_inferences.fetch_add(1, Ordering::SeqCst);
                                    self.total_recognized
                                        .fetch_add(recognized_count as u64, Ordering::SeqCst);
                                    self.total_unknown
                                        .fetch_add(unknown_count as u64, Ordering::SeqCst);

                                    // Update binding stats
                                    if let Some(stats) =
                                        self.binding_stats.write().get_mut(device_id)
                                    {
                                        stats.total_inferences += 1;
                                        stats.total_recognized += recognized_count as u64;
                                        stats.total_unknown += unknown_count as u64;
                                    }

                                    // Build face names list
                                    let face_names: Vec<String> = results
                                        .iter()
                                        .map(|r| {
                                            r.name
                                                .clone()
                                                .unwrap_or_else(|| "Unknown".to_string())
                                        })
                                        .collect();

                                    // Calculate average confidence
                                    let avg_confidence = if face_count > 0 {
                                        results
                                            .iter()
                                            .map(|r| {
                                                r.similarity
                                                    .unwrap_or(r.face_box.confidence)
                                            })
                                            .sum::<f64>()
                                            / face_count as f64
                                    } else {
                                        0.0
                                    };

                                    let timestamp = chrono::Utc::now().timestamp();

                                    // Write virtual metrics
                                    if !annotated_b64.is_empty() {
                                        self.write_recognition_results(
                                            device_id,
                                            face_count,
                                            &face_names,
                                            &annotated_b64,
                                            avg_confidence,
                                            timestamp,
                                        );
                                    }

                                    tracing::info!(
                                        "[FaceRecognition] Device {}: {} faces detected, {} recognized, {} unknown",
                                        device_id,
                                        face_count,
                                        recognized_count,
                                        unknown_count
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "[FaceRecognition] Pipeline failed for device {}: {}",
                                        device_id,
                                        e
                                    );
                                }
                            }
                        }

                        #[cfg(target_arch = "wasm32")]
                        {
                            tracing::warn!(
                                "[FaceRecognition] Image processing not supported in WASM"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[FaceRecognition] Base64 decode failed: device={}, error={}",
                            device_id,
                            e
                        );
                    }
                }
            }
        }

        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            // 1. bind_device
            "bind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid device_id".to_string(),
                        )
                    })?;

                if device_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "device_id cannot be empty",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                // Check if already bound
                if self.bindings.read().contains_key(device_id) {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Device {} is already bound", device_id),
                        "error_code": "DEVICE_ALREADY_BOUND"
                    }));
                }

                let metric_name = args
                    .get("metric_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image")
                    .to_string();

                let binding = DeviceBinding {
                    device_id: device_id.to_string(),
                    metric_name,
                    active: true,
                    created_at: chrono::Utc::now().timestamp(),
                };

                self.bindings
                    .write()
                    .insert(device_id.to_string(), binding.clone());
                self.binding_stats.write().insert(
                    device_id.to_string(),
                    BindingStats {
                        total_inferences: 0,
                        total_recognized: 0,
                        total_unknown: 0,
                    },
                );

                self.persist_config();

                tracing::info!(
                    "[FaceRecognition] Device bound: {} (metric: {})",
                    device_id,
                    binding.metric_name
                );

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "message": format!("Device {} bound successfully", device_id)
                }))
            }

            // 2. unbind_device
            "unbind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid device_id".to_string(),
                        )
                    })?;

                if device_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "device_id cannot be empty",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                self.bindings.write().remove(device_id);
                self.binding_stats.write().remove(device_id);

                self.persist_config();

                tracing::info!("[FaceRecognition] Device unbound: {}", device_id);

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "message": format!("Device {} unbound successfully", device_id)
                }))
            }

            // 3. toggle_binding
            "toggle_binding" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid device_id".to_string(),
                        )
                    })?;

                let active = args
                    .get("active")
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid active parameter".to_string(),
                        )
                    })?;

                if device_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "device_id cannot be empty",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                let mut bindings = self.bindings.write();
                if let Some(binding) = bindings.get_mut(device_id) {
                    binding.active = active;
                    drop(bindings); // Release write lock before persist

                    self.persist_config();

                    tracing::info!(
                        "[FaceRecognition] Device {} binding toggled to active={}",
                        device_id,
                        active
                    );

                    Ok(json!({
                        "success": true,
                        "device_id": device_id,
                        "active": active
                    }))
                } else {
                    Ok(json!({
                        "success": false,
                        "error": format!("Device {} not bound", device_id),
                        "error_code": "DEVICE_NOT_FOUND"
                    }))
                }
            }

            // 4. get_bindings
            "get_bindings" => {
                let bindings: Vec<serde_json::Value> = self
                    .bindings
                    .read()
                    .iter()
                    .map(|(id, b)| {
                        let stats = self
                            .binding_stats
                            .read()
                            .get(id)
                            .cloned()
                            .unwrap_or(BindingStats {
                                total_inferences: 0,
                                total_recognized: 0,
                                total_unknown: 0,
                            });
                        json!({
                            "device_id": b.device_id,
                            "metric_name": b.metric_name,
                            "active": b.active,
                            "created_at": b.created_at,
                            "stats": stats,
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "bindings": bindings
                }))
            }

            // 5. register_face
            "register_face" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if name.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "name is required and cannot be empty",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                let image_b64 = args
                    .get("image")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if image_b64.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "image is required",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                // Handle data URI prefix
                let image_b64_clean = if image_b64.starts_with("data:") {
                    if let Some(comma_pos) = image_b64.find(',') {
                        &image_b64[comma_pos + 1..]
                    } else {
                        image_b64
                    }
                } else {
                    image_b64
                };

                // Decode base64 image
                let image_data =
                    match base64::engine::general_purpose::STANDARD.decode(image_b64_clean) {
                        Ok(data) => data,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Invalid base64 image: {}", e),
                                "error_code": "INVALID_ARGUMENTS"
                            }));
                        }
                    };

                // Check size limit (10MB)
                if image_data.len() > 10 * 1024 * 1024 {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Image too large: {} bytes (max 10MB)", image_data.len()),
                        "error_code": "IMAGE_TOO_LARGE"
                    }));
                }

                // Run detection to find faces
                let config = self.config.read();
                let confidence = config.confidence_threshold as f32;

                let faces = {
                    let mut detector = self.detector.lock();
                    match detector.detect(&image_data, confidence) {
                        Ok(f) => f,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Face detection failed: {}", e),
                                "error_code": "MODEL_NOT_LOADED"
                            }));
                        }
                    }
                };

                // Validate face count
                if faces.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "No face detected in the image",
                        "error_code": "NO_FACE_DETECTED"
                    }));
                }

                if faces.len() > 1 {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Multiple faces ({}) detected, please provide an image with exactly one face", faces.len()),
                        "error_code": "MULTIPLE_FACES"
                    }));
                }

                let face_box = &faces[0];

                // Align the face
                let img = match image::load_from_memory(&image_data) {
                    Ok(i) => i,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to load image: {}", e),
                            "error_code": "INVALID_ARGUMENTS"
                        }));
                    }
                };

                let aligned = crate::alignment::align_face(&img, face_box);

                // Generate thumbnail: encode aligned 112x112 face as JPEG -> base64 -> data URI
                let thumbnail = {
                    let mut jpeg_bytes = Vec::new();
                    let aligned_rgb = aligned.to_rgb8();
                    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                        &mut jpeg_bytes,
                        90,
                    );
                    if let Err(e) = encoder.encode(
                        aligned_rgb.as_raw(),
                        aligned_rgb.width(),
                        aligned_rgb.height(),
                        image::ColorType::Rgb8.into(),
                    ) {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to encode thumbnail: {}", e),
                            "error_code": "INVALID_ARGUMENTS"
                        }));
                    }
                    format!(
                        "data:image/jpeg;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes)
                    )
                };

                // Encode aligned face to JPEG for feature extraction
                let mut aligned_bytes = Vec::new();
                let aligned_rgb = aligned.to_rgb8();
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut aligned_bytes,
                    95,
                );
                if let Err(e) = encoder.encode(
                    aligned_rgb.as_raw(),
                    aligned_rgb.width(),
                    aligned_rgb.height(),
                    image::ColorType::Rgb8.into(),
                ) {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Failed to encode aligned face: {}", e),
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                // Extract features
                let feature = {
                    let mut recognizer = self.recognizer.lock();
                    match recognizer.extract(&aligned_bytes) {
                        Ok(f) => f,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Feature extraction failed: {}", e),
                                "error_code": "MODEL_NOT_LOADED"
                            }));
                        }
                    }
                };

                // Check for duplicate name
                {
                    let db = self.face_db.read();
                    // We need to check by trying to find the name in the list
                    let existing = db.list_faces();
                    if existing.iter().any(|e| e.name == name) {
                        return Ok(json!({
                            "success": false,
                            "error": format!("A face with name '{}' is already registered", name),
                            "error_code": "DUPLICATE_NAME"
                        }));
                    }
                }

                // Check max faces limit
                {
                    let db = self.face_db.read();
                    if db.len() >= config.max_faces {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Maximum face limit of {} reached", config.max_faces),
                            "error_code": "MAX_FACES_EXCEEDED"
                        }));
                    }
                }

                // Register in database
                let entry = {
                    let mut db = self.face_db.write();
                    match db.register(name, feature, &thumbnail) {
                        Ok(entry) => entry,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Failed to register face: {}", e),
                                "error_code": "INVALID_ARGUMENTS"
                            }));
                        }
                    }
                };

                // Save database to file
                self.save_face_database();

                tracing::info!(
                    "[FaceRecognition] Face registered: {} (id: {})",
                    entry.name,
                    entry.id
                );

                Ok(json!({
                    "success": true,
                    "face_id": entry.id,
                    "name": entry.name,
                    "registered_at": entry.registered_at,
                    "message": format!("Face '{}' registered successfully", name)
                }))
            }

            // 6. delete_face
            "delete_face" => {
                let face_id = args
                    .get("face_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if face_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "face_id is required",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                let mut db = self.face_db.write();
                match db.delete(face_id) {
                    Ok(()) => {
                        drop(db); // Release lock before saving
                        self.save_face_database();

                        tracing::info!(
                            "[FaceRecognition] Face deleted: {}",
                            face_id
                        );

                        Ok(json!({
                            "success": true,
                            "face_id": face_id,
                            "message": "Face deleted successfully"
                        }))
                    }
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": format!("Face not found: {}", e),
                        "error_code": "FACE_NOT_FOUND"
                    })),
                }
            }

            // 7. list_faces
            "list_faces" => {
                let db = self.face_db.read();
                let faces = db.list_faces();
                Ok(json!({
                    "success": true,
                    "faces": faces,
                    "count": faces.len()
                }))
            }

            // 8. get_status
            "get_status" => Ok(self.get_status()),

            // 9. configure
            "configure" => {
                // Accept config either as a "config" parameter or directly in args
                let config_value = args.get("config").unwrap_or(args);

                if let Some(confidence) =
                    config_value.get("confidence_threshold").and_then(|v| v.as_f64())
                {
                    self.config.write().confidence_threshold = confidence;
                }
                if let Some(threshold) = config_value
                    .get("recognition_threshold")
                    .and_then(|v| v.as_f64())
                {
                    self.config.write().recognition_threshold = threshold;
                }
                if let Some(max_faces) =
                    config_value.get("max_faces").and_then(|v| v.as_u64())
                {
                    self.config.write().max_faces = max_faces as usize;
                }
                if let Some(auto_detect) =
                    config_value.get("auto_detect").and_then(|v| v.as_bool())
                {
                    self.config.write().auto_detect = auto_detect;
                }

                self.persist_config();

                tracing::info!("[FaceRecognition] Configuration updated");

                Ok(json!({
                    "success": true,
                    "message": "Configuration updated",
                    "config": *self.config.read()
                }))
            }

            // 10. get_config
            "get_config" => {
                let config = self.config.read();
                Ok(json!({
                    "success": true,
                    "config": *config
                }))
            }

            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn execute_command_sync(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "get_status" => Ok(self.get_status()),
            "get_config" => {
                let config = self.config.read();
                Ok(json!({
                    "success": true,
                    "config": *config
                }))
            }
            "list_faces" => {
                let db = self.face_db.read();
                let faces = db.list_faces();
                Ok(json!({
                    "success": true,
                    "faces": faces,
                    "count": faces.len()
                }))
            }
            "get_bindings" => {
                let bindings: Vec<serde_json::Value> = self
                    .bindings
                    .read()
                    .iter()
                    .map(|(_, b)| json!(b))
                    .collect();
                Ok(json!({
                    "success": true,
                    "bindings": bindings
                }))
            }
            "bind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("Missing device_id".to_string())
                    })?;
                let metric_name = args
                    .get("metric_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image")
                    .to_string();

                let binding = DeviceBinding {
                    device_id: device_id.to_string(),
                    metric_name,
                    active: true,
                    created_at: chrono::Utc::now().timestamp(),
                };
                self.bindings
                    .write()
                    .insert(device_id.to_string(), binding);
                self.binding_stats.write().insert(
                    device_id.to_string(),
                    BindingStats {
                        total_inferences: 0,
                        total_recognized: 0,
                        total_unknown: 0,
                    },
                );
                Ok(json!({"success": true, "device_id": device_id}))
            }
            "unbind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("Missing device_id".to_string())
                    })?;
                self.bindings.write().remove(device_id);
                self.binding_stats.write().remove(device_id);
                Ok(json!({"success": true, "device_id": device_id}))
            }
            "toggle_binding" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("Missing device_id".to_string())
                    })?;
                let active = args.get("active").and_then(|v| v.as_bool());
                if let Some(binding) = self.bindings.write().get_mut(device_id) {
                    binding.active = active.unwrap_or(!binding.active);
                    Ok(json!({"success": true, "device_id": device_id, "active": binding.active}))
                } else {
                    Err(ExtensionError::NotFound(format!(
                        "Device {} not bound",
                        device_id
                    )))
                }
            }
            "configure" => Ok(json!({"success": true, "message": "Configuration applied (WASM)"})),
            "register_face" => Err(ExtensionError::NotSupported(
                "Face registration not supported in WASM".to_string(),
            )),
            "delete_face" => Err(ExtensionError::NotSupported(
                "Face deletion not supported in WASM".to_string(),
            )),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        tracing::info!("[FaceRecognition] configure called");

        // First, try to load from file (for isolated extensions)
        if let Some(file_config) = self.load_config_from_file() {
            tracing::info!(
                "[FaceRecognition] Loading persisted configuration from file with {} bindings",
                file_config.bindings.len()
            );

            // Apply config values
            let mut cfg = self.config.write();
            cfg.confidence_threshold = file_config.confidence_threshold;
            cfg.recognition_threshold = file_config.recognition_threshold;
            cfg.max_faces = file_config.max_faces;
            cfg.auto_detect = file_config.auto_detect;
            drop(cfg);

            // Restore persisted bindings
            self.restore_bindings(&file_config);
        }

        // Then, load from system config if provided (overrides file config)
        if let Some(face_config) = config.get("face_recognition_config") {
            if let Ok(parsed_config) =
                serde_json::from_value::<FaceRecConfig>(face_config.clone())
            {
                tracing::info!(
                    "[FaceRecognition] Loading system configuration with {} bindings",
                    parsed_config.bindings.len()
                );

                let mut cfg = self.config.write();
                cfg.confidence_threshold = parsed_config.confidence_threshold;
                cfg.recognition_threshold = parsed_config.recognition_threshold;
                cfg.max_faces = parsed_config.max_faces;
                cfg.auto_detect = parsed_config.auto_detect;
                drop(cfg);

                self.restore_bindings(&parsed_config);
            }
        }

        // Apply individual config settings (these override persisted values)
        if let Some(confidence) = config.get("confidence_threshold").and_then(|v| v.as_f64()) {
            self.config.write().confidence_threshold = confidence;
        }
        if let Some(threshold) = config.get("recognition_threshold").and_then(|v| v.as_f64()) {
            self.config.write().recognition_threshold = threshold;
        }
        if let Some(max_faces) = config.get("max_faces").and_then(|v| v.as_u64()) {
            self.config.write().max_faces = max_faces as usize;
        }
        if let Some(auto_detect) = config.get("auto_detect").and_then(|v| v.as_bool()) {
            self.config.write().auto_detect = auto_detect;
        }

        tracing::info!(
            "[FaceRecognition] Configuration applied: threshold={}, recognition={}, max_faces={}",
            self.config.read().confidence_threshold,
            self.config.read().recognition_threshold,
            self.config.read().max_faces
        );

        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        tracing::info!("[FaceRecognition] start called");

        // Load face database from faces.json
        // First create the database with correct thresholds
        let config = self.config.read();
        *self.face_db.write() = FaceDatabase::new(
            config.recognition_threshold,
            config.max_faces,
        );
        drop(config);

        // Then load from file (this replaces the empty db with persisted data)
        if let Some(path) = Self::get_faces_db_path() {
            if path.exists() {
                match FaceDatabase::load_from_file(&path) {
                    Ok(db) => {
                        tracing::info!(
                            "[FaceRecognition] Loaded face database with {} entries",
                            db.len()
                        );
                        *self.face_db.write() = db;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[FaceRecognition] Failed to load face database: {}. Starting empty.",
                            e
                        );
                    }
                }
            } else {
                tracing::info!(
                    "[FaceRecognition] No faces.json found. Starting with empty database."
                );
            }
        }

        // Models load lazily on first inference
        tracing::info!(
            "[FaceRecognition] Models will be loaded on first inference (lazy loading)"
        );

        tracing::info!(
            "[FaceRecognition] Extension started: {} bindings, {} faces registered",
            self.bindings.read().len(),
            self.face_db.read().len()
        );

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// FFI Export
// ============================================================================

neomind_extension_sdk::neomind_export!(FaceRecognition);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_new_creates_clean_state() {
        let ext = FaceRecognition::new();
        assert_eq!(ext.bindings.read().len(), 0);
        assert_eq!(ext.total_inferences.load(Ordering::SeqCst), 0);
        assert_eq!(ext.total_recognized.load(Ordering::SeqCst), 0);
        assert_eq!(ext.total_unknown.load(Ordering::SeqCst), 0);
        assert_eq!(ext.face_db.read().len(), 0);
    }

    #[test]
    fn test_default_creates_equivalent() {
        let ext = FaceRecognition::default();
        assert_eq!(ext.bindings.read().len(), 0);
    }

    #[test]
    fn test_metadata_returns_consistent_id() {
        let ext = FaceRecognition::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "face-recognition");
        assert_eq!(meta.name, "Face Recognition");
        assert_eq!(meta.version, "0.1.0");
    }

    #[test]
    fn test_metadata_is_static() {
        let ext1 = FaceRecognition::new();
        let ext2 = FaceRecognition::new();
        // Should return the same static reference
        assert!(std::ptr::eq(ext1.metadata(), ext2.metadata()));
    }

    #[test]
    fn test_metrics_returns_four_descriptors() {
        let ext = FaceRecognition::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 4);

        let names: Vec<&str> = metrics.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"bound_devices"));
        assert!(names.contains(&"total_inferences"));
        assert!(names.contains(&"total_recognized"));
        assert!(names.contains(&"total_unknown"));
    }

    #[test]
    fn test_commands_returns_ten_commands() {
        let ext = FaceRecognition::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 10);

        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"bind_device"));
        assert!(names.contains(&"unbind_device"));
        assert!(names.contains(&"toggle_binding"));
        assert!(names.contains(&"get_bindings"));
        assert!(names.contains(&"register_face"));
        assert!(names.contains(&"delete_face"));
        assert!(names.contains(&"list_faces"));
        assert!(names.contains(&"get_status"));
        assert!(names.contains(&"configure"));
        assert!(names.contains(&"get_config"));
    }

    #[test]
    fn test_produce_metrics_returns_four_values() {
        let ext = FaceRecognition::new();
        let values = ext.produce_metrics().unwrap();
        assert_eq!(values.len(), 4);

        let names: Vec<&str> = values.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"bound_devices"));
        assert!(names.contains(&"total_inferences"));
        assert!(names.contains(&"total_recognized"));
        assert!(names.contains(&"total_unknown"));
    }

    #[test]
    fn test_event_subscriptions_returns_device_metric() {
        let ext = FaceRecognition::new();
        let subs = ext.event_subscriptions();
        assert_eq!(subs, &["DeviceMetric"]);
    }

    #[test]
    fn test_handle_event_ignores_non_device_metric() {
        let ext = FaceRecognition::new();
        let result = ext.handle_event("OtherEvent", &json!({}));
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_image_from_value_handles_data_uri() {
        let ext = FaceRecognition::new();
        let value = json!("data:image/jpeg;base64,aGVsbG8=");
        let result = ext.extract_image_from_value(Some(&value), None);
        assert_eq!(result, Some("aGVsbG8=".to_string()));
    }

    #[test]
    fn test_extract_image_from_value_handles_plain_base64() {
        let ext = FaceRecognition::new();
        let value = json!("aGVsbG8=");
        let result = ext.extract_image_from_value(Some(&value), None);
        assert_eq!(result, Some("aGVsbG8=".to_string()));
    }

    #[test]
    fn test_extract_image_from_value_handles_metric_value_wrapper() {
        let ext = FaceRecognition::new();
        let value = json!({"String": "aGVsbG8="});
        let result = ext.extract_image_from_value(Some(&value), None);
        assert_eq!(result, Some("aGVsbG8=".to_string()));
    }

    #[test]
    fn test_extract_image_from_value_handles_nested_path() {
        let ext = FaceRecognition::new();
        let value = json!({"image": {"data": "aGVsbG8="}});
        let result = ext.extract_image_from_value(Some(&value), Some("image.data"));
        assert_eq!(result, Some("aGVsbG8=".to_string()));
    }

    #[test]
    fn test_extract_image_from_value_returns_none_for_none() {
        let ext = FaceRecognition::new();
        let result = ext.extract_image_from_value(None, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_image_from_value_handles_object_with_data_field() {
        let ext = FaceRecognition::new();
        let value = json!({"data": "aGVsbG8="});
        let result = ext.extract_image_from_value(Some(&value), None);
        assert_eq!(result, Some("aGVsbG8=".to_string()));
    }

    #[test]
    fn test_get_status_returns_valid_json() {
        let ext = FaceRecognition::new();
        let status = ext.get_status();
        assert!(status.get("total_bindings").is_some());
        assert!(status.get("total_inferences").is_some());
        assert!(status.get("total_recognized").is_some());
        assert!(status.get("total_unknown").is_some());
        assert!(status.get("registered_faces").is_some());
        assert!(status.get("config").is_some());
    }

    #[test]
    fn test_face_rec_config_default() {
        let config = FaceRecConfig::default();
        assert_eq!(config.confidence_threshold, 0.5);
        assert_eq!(config.recognition_threshold, 0.45);
        assert_eq!(config.max_faces, 10);
        assert!(config.auto_detect);
        assert!(config.bindings.is_empty());
    }

    #[test]
    fn test_device_binding_serialization() {
        let binding = DeviceBinding {
            device_id: "cam-01".to_string(),
            metric_name: "image".to_string(),
            active: true,
            created_at: 1700000000,
        };
        let json = serde_json::to_string(&binding).unwrap();
        let deserialized: DeviceBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.device_id, "cam-01");
        assert_eq!(deserialized.metric_name, "image");
        assert!(deserialized.active);
    }

    #[test]
    fn test_binding_stats_serialization() {
        let stats = BindingStats {
            total_inferences: 10,
            total_recognized: 5,
            total_unknown: 5,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: BindingStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_inferences, 10);
        assert_eq!(deserialized.total_recognized, 5);
    }

    #[test]
    fn test_face_box_serialization() {
        let face_box = FaceBox {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 120.0,
            confidence: 0.95,
            landmarks: Some(vec![
                Landmark { x: 30.0, y: 50.0 },
                Landmark { x: 70.0, y: 48.0 },
            ]),
        };
        let json = serde_json::to_string(&face_box).unwrap();
        let deserialized: FaceBox = serde_json::from_str(&json).unwrap();
        assert!((deserialized.x - 10.0).abs() < 1e-6);
        assert!((deserialized.confidence - 0.95).abs() < 1e-6);
        assert_eq!(deserialized.landmarks.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_face_result_serialization() {
        let result = FaceResult {
            face_box: FaceBox {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 120.0,
                confidence: 0.95,
                landmarks: None,
            },
            name: Some("Alice".to_string()),
            similarity: Some(0.92),
            face_id: Some("uuid-123".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: FaceResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name.unwrap(), "Alice");
    }

    #[test]
    fn test_face_result_unknown_face() {
        let result = FaceResult {
            face_box: FaceBox {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 120.0,
                confidence: 0.95,
                landmarks: None,
            },
            name: None,
            similarity: None,
            face_id: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("name"));
        assert!(!json.contains("similarity"));
        assert!(!json.contains("face_id"));
    }
}
