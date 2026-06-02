//! NeoMind ONVIF Bridge Extension
//!
//! Discovers and manages IP cameras via the ONVIF protocol.
//! Exposes camera information, RTSP streams, snapshots, and PTZ control
//! as devices and metrics for the NeoMind platform.

#![allow(dead_code)] // Public API types used by extension consumers
//!
//! Features:
//! - WS-Discovery for automatic camera detection on the local network
//! - Manual camera addition by IP/URL
//! - RTSP stream URI retrieval per media profile
//! - Snapshot URI retrieval
//! - Full PTZ control: relative/absolute move, stop, home, presets
//! - Per-device metrics export via device_metrics_write capability
//! - Device template registration via CapabilityContext
//!
//! # Architecture Note
//!
//! This extension uses **sync HTTP client** (ureq) for SOAP requests
//! to avoid Tokio runtime compatibility issues when loaded as a dynamic library
//! (.dylib/.so/.dll). Uses `parking_lot::RwLock` instead of `tokio::sync::RwLock`
//! for simpler sync access patterns (no .await needed).

mod types;
mod discovery;
mod soap_client;
mod ptz;

use neomind_extension_sdk::{
    async_trait, json, CapabilityContext, Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use types::OnvifDevice;

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct OnvifBridgeExtension {
    devices: RwLock<HashMap<String, OnvifDevice>>,
    total_commands: AtomicI64,
    /// 0 = template not registered yet, 1 = registered
    template_registered: AtomicI64,
}

impl OnvifBridgeExtension {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
            total_commands: AtomicI64::new(0),
            template_registered: AtomicI64::new(0),
        }
    }
}

impl Default for OnvifBridgeExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for OnvifBridgeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "onvif-bridge",
                "ONVIF Bridge",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description("ONVIF camera bridge — discover IP cameras, get RTSP streams, PTZ control")
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "discoveryTimeoutMs".to_string(),
                    display_name: "Discovery Timeout (ms)".to_string(),
                    description: "WS-Discovery probe timeout in milliseconds".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(5000)),
                    min: Some(1000.0),
                    max: Some(30000.0),
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "defaultUsername".to_string(),
                    display_name: "Default Username".to_string(),
                    description: "Default ONVIF username for discovered devices".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "defaultPassword".to_string(),
                    display_name: "Default Password".to_string(),
                    description: "Default ONVIF password for discovered devices".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
            ])
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "total_commands".to_string(),
                display_name: "Total Commands".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "connected_devices".to_string(),
                display_name: "Connected Devices".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "discover".to_string(),
                display_name: "Discover Cameras".to_string(),
                description: "Discover ONVIF cameras on the local network via WS-Discovery".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "timeout_ms".to_string(),
                        display_name: "Timeout (ms)".to_string(),
                        description: "Discovery timeout in milliseconds (default: 5000)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(5000)),
                        min: Some(1000.0),
                        max: Some(30000.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "timeout_ms": 5000 }),
                    json!({}),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "add_device".to_string(),
                display_name: "Add Camera".to_string(),
                description: "Manually add an ONVIF camera by URL/IP".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device".to_string(),
                        display_name: "Device Config".to_string(),
                        description: "ONVIF device configuration JSON".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({
                        "device": {
                            "device_id": "cam-001",
                            "name": "Front Door Camera",
                            "device_url": "http://192.168.1.100:80",
                            "username": "admin",
                            "password": "password123"
                        }
                    }),
                    json!({
                        "device": {
                            "device_id": "cam-002",
                            "name": "Parking Lot Camera",
                            "device_url": "http://192.168.1.101/onvif/device_service",
                            "username": "admin",
                            "password": "password123"
                        }
                    }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "remove_device".to_string(),
                display_name: "Remove Camera".to_string(),
                description: "Remove an ONVIF camera".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera to remove".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_devices".to_string(),
                display_name: "List Cameras".to_string(),
                description: "List all configured ONVIF cameras and their status".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_device".to_string(),
                display_name: "Get Camera Details".to_string(),
                description: "Get detailed information about a specific ONVIF camera".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_stream_uri".to_string(),
                display_name: "Get Stream URI".to_string(),
                description: "Get the RTSP stream URI for a camera profile".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "stream_type".to_string(),
                        display_name: "Stream Type".to_string(),
                        description: "Stream type: RTP-Unicast or RTP-Multicast".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["RTP-Unicast".to_string(), "RTP-Multicast".to_string()],
                        },
                        required: false,
                        default_value: Some(ParamMetricValue::String("RTP-Unicast".to_string())),
                        min: None,
                        max: None,
                        options: vec!["RTP-Unicast".to_string(), "RTP-Multicast".to_string()],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "device_id": "cam-001" }),
                    json!({ "device_id": "cam-001", "profile_token": "profile_1", "stream_type": "RTP-Unicast" }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_snapshot".to_string(),
                display_name: "Get Snapshot URI".to_string(),
                description: "Get the snapshot URI for a camera profile".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "ptz_move".to_string(),
                display_name: "PTZ Relative Move".to_string(),
                description: "Move camera PTZ by relative offset".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "pan".to_string(),
                        display_name: "Pan".to_string(),
                        description: "Pan offset (-1.0 to 1.0)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(0.0)),
                        min: Some(-1.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "tilt".to_string(),
                        display_name: "Tilt".to_string(),
                        description: "Tilt offset (-1.0 to 1.0)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(0.0)),
                        min: Some(-1.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "zoom".to_string(),
                        display_name: "Zoom".to_string(),
                        description: "Zoom offset (-1.0 to 1.0)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(0.0)),
                        min: Some(-1.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "speed".to_string(),
                        display_name: "Speed".to_string(),
                        description: "Movement speed (0.0 to 1.0, default 0.5)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(0.5)),
                        min: Some(0.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "device_id": "cam-001", "pan": 0.5, "tilt": 0.0, "zoom": 0.0, "speed": 0.5 }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "ptz_absolute".to_string(),
                display_name: "PTZ Absolute Move".to_string(),
                description: "Move camera PTZ to absolute position".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "pan".to_string(),
                        display_name: "Pan".to_string(),
                        description: "Pan position (-1.0 to 1.0)".to_string(),
                        param_type: MetricDataType::Float,
                        required: true,
                        default_value: None,
                        min: Some(-1.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "tilt".to_string(),
                        display_name: "Tilt".to_string(),
                        description: "Tilt position (-1.0 to 1.0)".to_string(),
                        param_type: MetricDataType::Float,
                        required: true,
                        default_value: None,
                        min: Some(-1.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "zoom".to_string(),
                        display_name: "Zoom".to_string(),
                        description: "Zoom position (0.0 to 1.0)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(0.0)),
                        min: Some(0.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "speed".to_string(),
                        display_name: "Speed".to_string(),
                        description: "Movement speed (0.0 to 1.0, default 0.5)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(0.5)),
                        min: Some(0.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "device_id": "cam-001", "pan": 0.0, "tilt": 0.0, "zoom": 0.0 }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "ptz_stop".to_string(),
                display_name: "PTZ Stop".to_string(),
                description: "Stop current PTZ movement".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "ptz_home".to_string(),
                display_name: "PTZ Go Home".to_string(),
                description: "Move camera to home position".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_presets".to_string(),
                display_name: "List PTZ Presets".to_string(),
                description: "List PTZ presets for a camera profile".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "goto_preset".to_string(),
                display_name: "Go To PTZ Preset".to_string(),
                description: "Move camera to a saved PTZ preset".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "preset_token".to_string(),
                        display_name: "Preset Token".to_string(),
                        description: "Token of the preset to go to".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001", "preset_token": "1" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".to_string(),
                display_name: "Get PTZ Status".to_string(),
                description: "Get the current PTZ status of a camera".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the camera".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "profile_token".to_string(),
                        display_name: "Profile Token".to_string(),
                        description: "Media profile token (uses first profile if not specified)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "cam-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "configure".to_string(),
                display_name: "Configure".to_string(),
                description: "Apply extension-level configuration".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        self.total_commands.fetch_add(1, Ordering::SeqCst);

        match command {
            "discover" => self.cmd_discover(args),
            "add_device" => self.cmd_add_device(args),
            "remove_device" => self.cmd_remove_device(args),
            "list_devices" => self.cmd_list_devices(),
            "get_device" => self.cmd_get_device(args),
            "get_stream_uri" => self.cmd_get_stream_uri(args),
            "get_snapshot" => self.cmd_get_snapshot(args),
            "ptz_move" => self.cmd_ptz_move(args),
            "ptz_absolute" => self.cmd_ptz_absolute(args),
            "ptz_stop" => self.cmd_ptz_stop(args),
            "ptz_home" => self.cmd_ptz_home(args),
            "list_presets" => self.cmd_list_presets(args),
            "goto_preset" => self.cmd_goto_preset(args),
            "get_status" => self.cmd_get_status(args),
            "configure" => Ok(json!({"status": "ok"})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::new();

        // Auto-register device template once
        self.register_template();

        metrics.push(ExtensionMetricValue {
            name: "total_commands".to_string(),
            value: ParamMetricValue::Integer(self.total_commands.load(Ordering::SeqCst)),
            timestamp: now,
        });

        // Clone device data snapshot to avoid holding read lock during invoke_capability
        let device_snapshot: Vec<_> = {
            let devices = self.devices.read();
            devices.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()
        };
        let mut connected_count = 0i64;

        for (id, device) in &device_snapshot {
            if device.connected {
                connected_count += 1;
            }

            // Per-device metrics
            metrics.push(ExtensionMetricValue {
                name: format!("onvif.{}.connected", id),
                value: ParamMetricValue::Integer(if device.connected { 1 } else { 0 }),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: format!("onvif.{}.profile_count", id),
                value: ParamMetricValue::Integer(device.profiles.len() as i64),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: format!("onvif.{}.ptz_supported", id),
                value: ParamMetricValue::Integer(if device.ptz_supported { 1 } else { 0 }),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: format!("onvif.{}.last_seen_ms", id),
                value: ParamMetricValue::Integer(device.last_seen_ms),
                timestamp: now,
            });

            // Write per-device metrics via device_metrics_write capability
            let ctx = CapabilityContext::default();
            let device_id = &device.device_id;

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": device_id,
                "metric": "connected",
                "value": if device.connected { "true" } else { "false" },
                "timestamp": now,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": device_id,
                "metric": "profile_count",
                "value": device.profiles.len(),
                "timestamp": now,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": device_id,
                "metric": "ptz_supported",
                "value": if device.ptz_supported { "true" } else { "false" },
                "timestamp": now,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": device_id,
                "metric": "last_seen_ms",
                "value": device.last_seen_ms,
                "timestamp": now,
            }));

            // Write stream/snapshot URIs for first profile if available
            if let Some(first_profile) = device.profiles.first() {
                if let Some(ref uri) = first_profile.stream_uri {
                    let _ = ctx.invoke_capability("device_metrics_write", &json!({
                        "device_id": device_id,
                        "metric": "stream_uri",
                        "value": uri,
                        "timestamp": now,
                    }));
                }
                if let Some(ref uri) = first_profile.snapshot_uri {
                    let _ = ctx.invoke_capability("device_metrics_write", &json!({
                        "device_id": device_id,
                        "metric": "snapshot_uri",
                        "value": uri,
                        "timestamp": now,
                    }));
                }
            }
        }

        metrics.push(ExtensionMetricValue {
            name: "connected_devices".to_string(),
            value: ParamMetricValue::Integer(connected_count),
            timestamp: now,
        });

        Ok(metrics)
    }

    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> {
        // Extension-level configuration is accepted silently.
        // Per-device configuration is done via the add_device command.
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Template & Device Registration
// ============================================================================

impl OnvifBridgeExtension {
    /// Register the "onvif_camera" device template with NeoMind.
    /// Called once from produce_metrics() when template_registered == 0.
    fn register_template(&self) {
        // Use compare_exchange to prevent concurrent registration
        if self.template_registered.compare_exchange(0, 1, Ordering::SeqCst, Ordering::Relaxed).is_err() {
            return; // Already being registered by another thread
        }

        let ctx = CapabilityContext::default();

        let template_json = json!({
            "device_type": "onvif_camera",
            "name": "ONVIF Camera",
            "description": "IP camera discovered or added via ONVIF protocol",
            "categories": ["camera", "onvif", "video"],
            "metrics": [
                { "name": "connected", "display_name": "Connection Status", "data_type": "String" },
                { "name": "profile_count", "display_name": "Profile Count", "data_type": "Integer" },
                { "name": "ptz_supported", "display_name": "PTZ Supported", "data_type": "String" },
                { "name": "last_seen_ms", "display_name": "Last Seen", "data_type": "Integer", "unit": "ms" },
                { "name": "stream_uri", "display_name": "Stream URI", "data_type": "String" },
                { "name": "snapshot_uri", "display_name": "Snapshot URI", "data_type": "String" }
            ],
            "commands": [
                {
                    "name": "get_stream_uri",
                    "display_name": "Get Stream URI",
                    "description": "Get RTSP stream URI",
                    "parameters": [
                        { "name": "profile_token", "display_name": "Profile Token", "data_type": "String", "required": false }
                    ]
                },
                {
                    "name": "ptz_move",
                    "display_name": "PTZ Move",
                    "description": "Relative PTZ move",
                    "parameters": [
                        { "name": "pan", "display_name": "Pan", "data_type": "Float", "required": false },
                        { "name": "tilt", "display_name": "Tilt", "data_type": "Float", "required": false },
                        { "name": "zoom", "display_name": "Zoom", "data_type": "Float", "required": false }
                    ]
                },
                {
                    "name": "ptz_stop",
                    "display_name": "PTZ Stop",
                    "description": "Stop PTZ movement",
                    "parameters": []
                }
            ]
        });

        let result = ctx.invoke_capability("device_template_register", &template_json);
        if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            eprintln!("[onvif-bridge] Device template registered");
            // Already set to 1 by compare_exchange above
        } else {
            let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
            eprintln!("[onvif-bridge] Template registration failed: {} (will retry)", err);
            self.template_registered.store(0, Ordering::SeqCst);
        }
    }

    /// Register a device instance with NeoMind via device_register capability.
    fn register_device(&self, device_id: &str, name: &str) {
        let ctx = CapabilityContext::default();

        let device_json = json!({
            "device_id": device_id,
            "name": name,
            "device_type": "onvif_camera",
        });

        let result = ctx.invoke_capability("device_register", &device_json);
        if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            eprintln!("[onvif-bridge] Device '{}' registered", device_id);
        } else {
            let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
            eprintln!("[onvif-bridge] Device '{}' registration skipped: {}", device_id, err);
        }
    }
}

// ============================================================================
// Command Handlers
// ============================================================================

impl OnvifBridgeExtension {
    /// Helper: get a profile token from args, defaulting to the first profile.
    fn resolve_profile_token(&self, device_id: &str, args: &serde_json::Value) -> Result<String> {
        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        if let Some(token) = args.get("profile_token").and_then(|v| v.as_str()) {
            Ok(token.to_string())
        } else if let Some(first_profile) = device.profiles.first() {
            Ok(first_profile.token.clone())
        } else {
            Err(ExtensionError::ExecutionFailed(
                "No profiles available for this device".to_string(),
            ))
        }
    }

    fn cmd_discover(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(5000);

        let discovered = discovery::discover_devices(timeout_ms)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Discovery failed: {}", e)))?;

        let results: Vec<serde_json::Value> = discovered
            .iter()
            .map(|m| {
                json!({
                    "endpoint": m.endpoint,
                    "scopes": m.scopes,
                    "xaddrs": m.xaddrs,
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "count": results.len(),
            "devices": results
        }))
    }

    fn cmd_add_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_value = args
            .get("device")
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'device' parameter".to_string()))?;

        let device_id = device_value
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'device_id' in device config".to_string()))?
            .to_string();

        // Validate device ID format
        if device_id.is_empty() || device_id.len() > 64
            || !device_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ExtensionError::InvalidArguments(
                "Invalid device_id: must be 1-64 chars, alphanumeric/hyphen/underscore only".to_string(),
            ));
        }

        let name = device_value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&device_id)
            .to_string();

        let device_url = device_value
            .get("device_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'device_url' in device config".to_string()))?
            .to_string();

        let username = device_value.get("username").and_then(|v| v.as_str()).map(|s| s.to_string());
        let password = device_value.get("password").and_then(|v| v.as_str()).map(|s| s.to_string());

        let mut device = OnvifDevice {
            device_id: device_id.clone(),
            name: name.clone(),
            device_url,
            hardware_id: None,
            manufacturer: None,
            model: None,
            firmware_version: None,
            serial_number: None,
            scopes: Vec::new(),
            profiles: Vec::new(),
            ptz_supported: false,
            username,
            password,
            connected: false,
            last_seen_ms: chrono::Utc::now().timestamp_millis(),
        };

        // Try to enrich with device info
        match soap_client::get_device_info(&device) {
            Ok(info) => {
                device.manufacturer = info.get("manufacturer").and_then(|v| v.as_str()).map(|s| s.to_string());
                device.model = info.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
                device.firmware_version = info.get("firmware_version").and_then(|v| v.as_str()).map(|s| s.to_string());
                device.serial_number = info.get("serial_number").and_then(|v| v.as_str()).map(|s| s.to_string());
                device.hardware_id = info.get("hardware_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                device.connected = true;
                device.last_seen_ms = chrono::Utc::now().timestamp_millis();
                eprintln!("[onvif-bridge] Device info retrieved for '{}'", device_id);
            }
            Err(e) => {
                eprintln!("[onvif-bridge] Warning: Could not get device info for '{}': {}", device_id, e);
                // Still add the device even if info retrieval fails
            }
        }

        // Try to get media profiles
        match soap_client::get_profiles(&device) {
            Ok(profiles) => {
                eprintln!("[onvif-bridge] Found {} profiles for '{}'", profiles.len(), device_id);
                device.profiles = profiles;
            }
            Err(e) => {
                eprintln!("[onvif-bridge] Warning: Could not get profiles for '{}': {}", device_id, e);
            }
        }

        // Try to get stream URIs for each profile
        let mut enriched_profiles = device.profiles.clone();
        for profile in &mut enriched_profiles {
            if let Ok(uri) = soap_client::get_stream_uri(&device, &profile.token, "RTP-Unicast") {
                profile.stream_uri = Some(uri);
            }
            if let Ok(uri) = soap_client::get_snapshot_uri(&device, &profile.token) {
                profile.snapshot_uri = Some(uri);
            }
        }
        device.profiles = enriched_profiles;

        // Check PTZ support
        device.ptz_supported = soap_client::is_ptz_supported(&device);

        // Store the device
        {
            let mut devices = self.devices.write();
            devices.insert(device_id.clone(), device);
        }

        // Register device with NeoMind platform
        self.register_device(&device_id, &name);

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "message": "Camera added successfully"
        }))
    }

    fn cmd_remove_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        {
            let mut devices = self.devices.write();
            devices.remove(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?;
        }

        // Unregister from NeoMind
        let ctx = CapabilityContext::default();
        let _ = ctx.invoke_capability("device_unregister", &json!({
            "device_id": device_id,
        }));

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "message": "Camera removed"
        }))
    }

    fn cmd_list_devices(&self) -> Result<serde_json::Value> {
        let devices = self.devices.read();
        let mut device_list = Vec::new();

        for (id, device) in devices.iter() {
            let profile_summaries: Vec<serde_json::Value> = device
                .profiles
                .iter()
                .map(|p| {
                    json!({
                        "token": p.token,
                        "name": p.name,
                        "has_stream_uri": p.stream_uri.is_some(),
                        "has_snapshot_uri": p.snapshot_uri.is_some(),
                    })
                })
                .collect();

            device_list.push(json!({
                "device_id": id,
                "name": device.name,
                "device_url": device.device_url,
                "manufacturer": device.manufacturer,
                "model": device.model,
                "connected": device.connected,
                "ptz_supported": device.ptz_supported,
                "profile_count": device.profiles.len(),
                "profiles": profile_summaries,
                "last_seen_ms": device.last_seen_ms,
            }));
        }

        Ok(json!({
            "success": true,
            "count": device_list.len(),
            "devices": device_list
        }))
    }

    fn cmd_get_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        Ok(json!({
            "success": true,
            "device": device
        }))
    }

    fn cmd_get_stream_uri(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let stream_type = args
            .get("stream_type")
            .and_then(|v| v.as_str())
            .unwrap_or("RTP-Unicast");

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        let uri = soap_client::get_stream_uri(&device, &profile_token, stream_type)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to get stream URI: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "stream_type": stream_type,
            "uri": uri
        }))
    }

    fn cmd_get_snapshot(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        let uri = soap_client::get_snapshot_uri(&device, &profile_token)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to get snapshot URI: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "uri": uri
        }))
    }

    fn cmd_ptz_move(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let pan = args.get("pan").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let tilt = args.get("tilt").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let zoom = args.get("zoom").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let speed = args.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.5);

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        if !device.ptz_supported {
            return Err(ExtensionError::ExecutionFailed(
                "PTZ is not supported on this device".to_string(),
            ));
        }

        ptz::ptz_relative_move(&device, &profile_token, pan, tilt, zoom, speed)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("PTZ move failed: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "pan": pan,
            "tilt": tilt,
            "zoom": zoom,
            "speed": speed,
            "message": "PTZ relative move executed"
        }))
    }

    fn cmd_ptz_absolute(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let pan = args.get("pan").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let tilt = args.get("tilt").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let zoom = args.get("zoom").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let speed = args.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.5);

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        if !device.ptz_supported {
            return Err(ExtensionError::ExecutionFailed(
                "PTZ is not supported on this device".to_string(),
            ));
        }

        ptz::ptz_absolute_move(&device, &profile_token, pan, tilt, zoom, speed)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("PTZ absolute move failed: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "pan": pan,
            "tilt": tilt,
            "zoom": zoom,
            "speed": speed,
            "message": "PTZ absolute move executed"
        }))
    }

    fn cmd_ptz_stop(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        if !device.ptz_supported {
            return Err(ExtensionError::ExecutionFailed(
                "PTZ is not supported on this device".to_string(),
            ));
        }

        ptz::ptz_stop(&device, &profile_token)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("PTZ stop failed: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "message": "PTZ stopped"
        }))
    }

    fn cmd_ptz_home(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        if !device.ptz_supported {
            return Err(ExtensionError::ExecutionFailed(
                "PTZ is not supported on this device".to_string(),
            ));
        }

        ptz::ptz_go_home(&device, &profile_token)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("PTZ go home failed: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "message": "PTZ moved to home position"
        }))
    }

    fn cmd_list_presets(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        let presets = ptz::list_presets(&device, &profile_token)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("List presets failed: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "count": presets.len(),
            "presets": presets
        }))
    }

    fn cmd_goto_preset(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let preset_token = args
            .get("preset_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'preset_token' parameter".to_string())
            })?;

        let profile_token = self.resolve_profile_token(device_id, args)?;

        let device = {
            let devices = self.devices.read();
            devices.get(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?.clone()
        };

        ptz::goto_preset(&device, &profile_token, preset_token)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Goto preset failed: {}", e)))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "profile_token": profile_token,
            "preset_token": preset_token,
            "message": "Moved to preset"
        }))
    }

    fn cmd_get_status(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        // Return current device state
        Ok(json!({
            "success": true,
            "device_id": device_id,
            "name": device.name,
            "connected": device.connected,
            "device_url": device.device_url,
            "manufacturer": device.manufacturer,
            "model": device.model,
            "firmware_version": device.firmware_version,
            "serial_number": device.serial_number,
            "hardware_id": device.hardware_id,
            "ptz_supported": device.ptz_supported,
            "profile_count": device.profiles.len(),
            "profiles": device.profiles,
            "last_seen_ms": device.last_seen_ms,
        }))
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

neomind_extension_sdk::neomind_export!(OnvifBridgeExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use types::OnvifDevice;

    #[test]
    fn test_extension_metadata() {
        let ext = OnvifBridgeExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "onvif-bridge");
        assert_eq!(meta.name, "ONVIF Bridge");
    }

    #[test]
    fn test_extension_commands() {
        let ext = OnvifBridgeExtension::new();
        let commands = ext.commands();
        let command_names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(command_names.contains(&"discover"));
        assert!(command_names.contains(&"add_device"));
        assert!(command_names.contains(&"remove_device"));
        assert!(command_names.contains(&"list_devices"));
        assert!(command_names.contains(&"get_device"));
        assert!(command_names.contains(&"get_stream_uri"));
        assert!(command_names.contains(&"get_snapshot"));
        assert!(command_names.contains(&"ptz_move"));
        assert!(command_names.contains(&"ptz_absolute"));
        assert!(command_names.contains(&"ptz_stop"));
        assert!(command_names.contains(&"ptz_home"));
        assert!(command_names.contains(&"list_presets"));
        assert!(command_names.contains(&"goto_preset"));
        assert!(command_names.contains(&"get_status"));
    }

    #[test]
    fn test_list_devices_empty() {
        let ext = OnvifBridgeExtension::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(ext.execute_command("list_devices", &json!({}))).unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["count"], 0);
    }

    #[test]
    fn test_remove_nonexistent_device() {
        let ext = OnvifBridgeExtension::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(ext.execute_command("remove_device", &json!({ "device_id": "nonexistent" })));
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_command() {
        let ext = OnvifBridgeExtension::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(ext.execute_command("nonexistent", &json!({})));
        assert!(result.is_err());
    }

    #[test]
    fn test_metrics() {
        let ext = OnvifBridgeExtension::new();
        let metrics = ext.metrics();
        assert!(metrics.iter().any(|m| m.name == "total_commands"));
        assert!(metrics.iter().any(|m| m.name == "connected_devices"));
    }

    #[test]
    fn test_add_and_list_device() {
        let ext = OnvifBridgeExtension::new();

        // Directly insert a device into the internal map to avoid network calls
        {
            let mut devices = ext.devices.write();
            devices.insert("cam-001".to_string(), OnvifDevice {
                device_id: "cam-001".to_string(),
                name: "Test Camera".to_string(),
                device_url: "http://192.168.1.100:80/onvif/device_service".to_string(),
                username: Some("admin".to_string()),
                password: Some("pass123".to_string()),
                manufacturer: None,
                model: None,
                firmware_version: None,
                serial_number: None,
                hardware_id: None,
                profiles: Vec::new(),
                ptz_supported: false,
                connected: false,
                last_seen_ms: 0,
                scopes: Vec::new(),
            });
        }

        let rt = tokio::runtime::Runtime::new().unwrap();

        // List devices — should have 1
        let result = rt.block_on(ext.execute_command("list_devices", &json!({}))).unwrap();
        assert_eq!(result["count"], 1);

        // Get device
        let result = rt.block_on(ext.execute_command("get_device", &json!({ "device_id": "cam-001" }))).unwrap();
        assert_eq!(result["device"]["device_id"], "cam-001");
        assert_eq!(result["device"]["device_url"], "http://192.168.1.100:80/onvif/device_service");

        // Remove device
        let result = rt.block_on(ext.execute_command("remove_device", &json!({ "device_id": "cam-001" }))).unwrap();
        assert_eq!(result["success"], true);

        // List should be empty
        let result = rt.block_on(ext.execute_command("list_devices", &json!({}))).unwrap();
        assert_eq!(result["count"], 0);
    }

    #[test]
    fn test_produce_metrics_with_device() {
        let ext = OnvifBridgeExtension::new();

        // Directly insert to avoid network calls
        {
            let mut devices = ext.devices.write();
            devices.insert("cam-002".to_string(), OnvifDevice {
                device_id: "cam-002".to_string(),
                name: "Camera 2".to_string(),
                device_url: "http://10.0.0.1/onvif/device_service".to_string(),
                username: None,
                password: None,
                manufacturer: None,
                model: None,
                firmware_version: None,
                serial_number: None,
                hardware_id: None,
                profiles: Vec::new(),
                ptz_supported: false,
                connected: true,
                last_seen_ms: chrono::Utc::now().timestamp_millis(),
                scopes: Vec::new(),
            });
        }

        let metrics = ext.produce_metrics().unwrap();
        let conn_metric = metrics.iter().find(|m| m.name == "connected_devices").unwrap();
        if let ParamMetricValue::Integer(v) = &conn_metric.value {
            assert_eq!(*v, 1);
        } else {
            panic!("expected Integer metric");
        }
    }
}
