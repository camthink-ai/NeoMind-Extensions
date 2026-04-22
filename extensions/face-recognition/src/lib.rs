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
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

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
}

impl Default for FaceRecConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.5,
            recognition_threshold: 0.45,
            max_faces: 10,
            auto_detect: true,
        }
    }
}

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct FaceRecognition {
    /// Device bindings: device_id -> binding
    bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,
    /// Binding statistics: device_id -> stats
    binding_stats: Arc<RwLock<HashMap<String, BindingStats>>>,

    /// Global statistics
    total_inferences: Arc<AtomicU64>,
    total_faces_detected: Arc<AtomicU64>,
    total_faces_recognized: Arc<AtomicU64>,
    total_errors: Arc<AtomicU64>,

    /// Configuration
    config: Mutex<FaceRecConfig>,
}

impl FaceRecognition {
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(RwLock::new(HashMap::new())),
            binding_stats: Arc::new(RwLock::new(HashMap::new())),
            total_inferences: Arc::new(AtomicU64::new(0)),
            total_faces_detected: Arc::new(AtomicU64::new(0)),
            total_faces_recognized: Arc::new(AtomicU64::new(0)),
            total_errors: Arc::new(AtomicU64::new(0)),
            config: Mutex::new(FaceRecConfig::default()),
        }
    }

    /// Get extension status
    pub fn get_status(&self) -> serde_json::Value {
        json!({
            "model_loaded": false,
            "total_bindings": self.bindings.read().len(),
            "total_inferences": self.total_inferences.load(Ordering::SeqCst),
            "total_faces_detected": self.total_faces_detected.load(Ordering::SeqCst),
            "total_faces_recognized": self.total_faces_recognized.load(Ordering::SeqCst),
            "total_errors": self.total_errors.load(Ordering::SeqCst),
            "config": *self.config.lock(),
        })
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
                name: "total_faces_detected".to_string(),
                display_name: "Total Faces Detected".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_faces_recognized".to_string(),
                display_name: "Total Faces Recognized".to_string(),
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
                        required: false,
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
            ExtensionCommand {
                name: "recognize".to_string(),
                display_name: "Recognize Faces".to_string(),
                description: "Recognize faces in an image".to_string(),
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
                name: "total_faces_detected".to_string(),
                value: ParamMetricValue::Integer(self.total_faces_detected.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_faces_recognized".to_string(),
                value: ParamMetricValue::Integer(self.total_faces_recognized.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_errors".to_string(),
                value: ParamMetricValue::Integer(self.total_errors.load(Ordering::SeqCst) as i64),
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
        _payload: &serde_json::Value,
    ) -> Result<()> {
        // Stub: only process DeviceMetric events
        if event_type != "DeviceMetric" {
            return Ok(());
        }
        // TODO: Implement event-driven face recognition in subsequent tasks
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "bind_device" => {
                let device_id = args.get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".to_string()))?;
                let metric_name = args.get("metric_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image").to_string();

                let binding = DeviceBinding {
                    device_id: device_id.to_string(),
                    metric_name,
                    active: true,
                    created_at: chrono::Utc::now().timestamp(),
                };

                self.bindings.write().insert(device_id.to_string(), binding.clone());
                self.binding_stats.write().insert(device_id.to_string(), BindingStats {
                    total_inferences: 0,
                    total_recognized: 0,
                    total_unknown: 0,
                });

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

                self.bindings.write().remove(device_id);
                self.binding_stats.write().remove(device_id);

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "message": format!("Device {} unbound successfully", device_id)
                }))
            }

            "get_bindings" => {
                let bindings: Vec<DeviceBinding> = self.bindings.read().values().cloned().collect();
                Ok(json!({
                    "success": true,
                    "bindings": bindings
                }))
            }

            "register_face" => {
                // Stub: face registration will be implemented in subsequent tasks
                Err(ExtensionError::NotSupported("Face registration not yet implemented".to_string()))
            }

            "recognize" => {
                // Stub: face recognition will be implemented in subsequent tasks
                Err(ExtensionError::NotSupported("Face recognition not yet implemented".to_string()))
            }

            "get_status" => {
                Ok(self.get_status())
            }

            "configure" => {
                // Stub: configuration loading will be implemented in subsequent tasks
                Ok(json!({"success": true, "message": "Configuration applied (stub)"}))
            }

            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn execute_command_sync(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "get_status" => Ok(self.get_status()),
            _ => Err(ExtensionError::NotSupported("Not supported in WASM".to_string())),
        }
    }

    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> {
        // Stub: configuration will be implemented in subsequent tasks
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
