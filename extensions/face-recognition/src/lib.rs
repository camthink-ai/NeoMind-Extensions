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
pub mod commands;
pub mod config;
pub mod database;
pub mod detector;
pub mod drawing;
pub mod onnx_utils;
pub mod pipeline;
pub mod recognizer;
pub mod types;

// Re-export public types for backward compatibility
pub use types::{
    BindingStats, DeviceBinding, FaceBox, FaceRecConfig, FaceResult, Landmark,
};

use async_trait::async_trait;
use neomind_extension_sdk::capabilities::CapabilityContext;
use neomind_extension_sdk::prelude::*;
use neomind_extension_sdk::{
    Extension, ExtensionCommand, ExtensionMetadata, ExtensionMetricValue,
    MetricDataType, MetricDescriptor, ParameterDefinition, ParamMetricValue, Result,
};
use parking_lot::{Mutex, RwLock};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::database::FaceDatabase;
use crate::detector::{FaceDetect, ScrfdDetector};
use crate::recognizer::{ArcFaceRecognizer, FaceExtract};

// ============================================================================
// Extension Struct
// ============================================================================

pub struct FaceRecognition {
    /// SCRFD face detector (lazy loading)
    pub(crate) detector: Mutex<Box<dyn FaceDetect + Send>>,
    /// ArcFace face feature extractor (lazy loading)
    pub(crate) recognizer: Mutex<Box<dyn FaceExtract + Send>>,
    /// Device bindings: device_id -> binding
    pub(crate) bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,
    /// Binding statistics: device_id -> stats
    pub(crate) binding_stats: Arc<RwLock<HashMap<String, BindingStats>>>,
    /// Face database
    pub(crate) face_db: Arc<RwLock<FaceDatabase>>,
    /// Extension configuration
    pub(crate) config: Arc<RwLock<FaceRecConfig>>,
    /// Global statistics
    pub(crate) total_inferences: Arc<AtomicU64>,
    pub(crate) total_recognized: Arc<AtomicU64>,
    pub(crate) total_unknown: Arc<AtomicU64>,
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

    /// Create a new instance with custom detector and recognizer implementations.
    ///
    /// This is intended for testing, allowing mock implementations to be injected
    /// instead of the real ONNX-based models.
    pub fn with_models(
        detector: Box<dyn FaceDetect + Send>,
        recognizer: Box<dyn FaceExtract + Send>,
    ) -> Self {
        Self {
            detector: Mutex::new(detector),
            recognizer: Mutex::new(recognizer),
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
    pub(crate) fn invoke_capability_sync(
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

    /// Get extension status including model and database info.
    pub fn get_status(&self) -> serde_json::Value {
        // Trigger lazy loading by calling detect/extract with empty data
        // to get accurate model status
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = self.detector.lock().detect(&[], 0.5);
            let _ = self.recognizer.lock().extract(&[]);
        }

        let face_db = self.face_db.read();
        let config = self.config.read();

        let detector_loaded = self.detector.lock().as_any().downcast_ref::<crate::detector::ScrfdDetector>()
            .map(|d| d.is_loaded())
            .unwrap_or(false);
        let recognizer_loaded = self.recognizer.lock().as_any().downcast_ref::<crate::recognizer::ArcFaceRecognizer>()
            .map(|r| r.is_loaded())
            .unwrap_or(false);

        json!({
            "total_bindings": self.bindings.read().len(),
            "total_inferences": self.total_inferences.load(Ordering::SeqCst),
            "total_recognized": self.total_recognized.load(Ordering::SeqCst),
            "total_unknown": self.total_unknown.load(Ordering::SeqCst),
            "registered_faces": face_db.len(),
            "model_loaded": detector_loaded && recognizer_loaded,
            "config": *config,
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
                match base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    &image_data_b64,
                ) {
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
                                        stats.last_image = if annotated_b64.is_empty() { None } else { Some(format!("data:image/jpeg;base64,{}", annotated_b64)) };
                                        stats.last_faces = if results.is_empty() { None } else { Some(results.clone()) };
                                        stats.last_error = None;
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

                                    let timestamp = chrono::Utc::now().timestamp_millis();

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
                                    if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
                                        stats.last_error = Some(e.to_string());
                                    }
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
        self.execute_command_impl(command, args).await
    }

    #[cfg(target_arch = "wasm32")]
    fn execute_command_sync(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.execute_command_sync_impl(command, args)
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

            // Sync with FaceDatabase
            self.face_db.write().set_threshold(file_config.recognition_threshold);
            self.face_db.write().set_max_faces(file_config.max_faces);

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

                // Sync with FaceDatabase
                self.face_db.write().set_threshold(parsed_config.recognition_threshold);
                self.face_db.write().set_max_faces(parsed_config.max_faces);

                self.restore_bindings(&parsed_config);
            }
        }

        // Apply individual config settings (these override persisted values)
        if let Some(confidence) = config.get("confidence_threshold").and_then(|v| v.as_f64()) {
            self.config.write().confidence_threshold = confidence;
        }
        if let Some(threshold) = config.get("recognition_threshold").and_then(|v| v.as_f64()) {
            self.config.write().recognition_threshold = threshold;
            self.face_db.write().set_threshold(threshold);
        }
        if let Some(max_faces) = config.get("max_faces").and_then(|v| v.as_u64()) {
            let max_faces_usize = max_faces as usize;
            self.config.write().max_faces = max_faces_usize;
            self.face_db.write().set_max_faces(max_faces_usize);
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

        // Load face database from faces.json
        // Note: This MUST be in configure() because the SDK's neomind_export! macro
        // does NOT export a start() FFI function for isolated extensions.
        // The start() method is never called by the extension runner.
        let config = self.config.read();
        let (threshold, max_faces) = (config.recognition_threshold, config.max_faces);
        drop(config);

        *self.face_db.write() = FaceDatabase::new(threshold, max_faces);

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
        } else {
            tracing::warn!(
                "[FaceRecognition] NEOMIND_EXTENSION_DIR not set, cannot load face database"
            );
        }

        tracing::info!(
            "[FaceRecognition] Extension configured: {} bindings, {} faces registered",
            self.bindings.read().len(),
            self.face_db.read().len()
        );

        Ok(())
    }

    fn start(&mut self) -> Result<()> {
        tracing::info!("[FaceRecognition] start called (no-op, loading done in configure)");
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
            last_image: None,
            last_faces: None,
            last_error: None,
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
