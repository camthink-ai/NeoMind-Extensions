//! NeoMind BACnet Bridge Extension
//!
//! Connects to BACnet/IP building automation devices (sensors, actuators, controllers)
//! and exposes object data as metrics for the NeoMind platform.

#![allow(dead_code)] // Public API types and protocol constants for future use
//!
//! Features:
//! - BACnet/IP device discovery via Who-Is/I-Am
//! - ReadProperty / ReadPropertyMultiple for sensor data
//! - WriteProperty for control commands
//! - SubscribeCOV for Change-of-Value notifications
//! - Background polling of configured objects
//! - Per-device metrics export via CapabilityContext
//!
//! # Architecture Note
//!
//! This extension uses **std::net::UdpSocket** for BACnet/IP communication
//! (BACnet/IP runs over UDP port 47808). Hand-written APDU encoding/decoding
//! (no external BACnet crate). Background listener and polling run on
//! dedicated std threads. Uses `parking_lot::RwLock` for synchronous access.

mod apdu;
mod bacnet_client;
mod bacnet_device;
mod types;

use neomind_extension_sdk::{
    async_trait, json, CapabilityContext, Extension, ExtensionMetadata, ExtensionError,
    ExtensionMetricValue, MetricDescriptor, ExtensionCommand, MetricDataType,
    ParameterDefinition, ParamMetricValue, Result,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;

use bacnet_client::{BacnetClient, start_listener, parse_ip_port};
use bacnet_device::BacnetDeviceManager;
use types::*;

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct BacnetBridgeExtension {
    /// Shared device map — accessible from both extension commands and background listener.
    /// Wrapped in Arc<RwLock> so the listener thread can update discovered devices.
    devices: Arc<RwLock<HashMap<u32, BacnetDevice>>>,
    device_managers: RwLock<HashMap<u32, BacnetDeviceManager>>,
    /// Shared COV subscriptions — accessible from listener for incoming notifications.
    cov_subscriptions: Arc<RwLock<HashMap<u32, CovSubscription>>>,
    total_commands: AtomicI64,
    /// 0 = template not registered yet, 1 = registered
    template_registered: AtomicI64,
    /// Listener thread handle
    listener_handle: RwLock<Option<std::thread::JoinHandle<()>>>,
    /// Listener running flag
    listener_running: Arc<AtomicBool>,
    /// Extension configuration
    config: RwLock<BacnetConfig>,
    /// Monotonic subscriber ID counter (avoids timestamp collisions)
    subscriber_id_counter: AtomicU32,
}

impl BacnetBridgeExtension {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            device_managers: RwLock::new(HashMap::new()),
            cov_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            total_commands: AtomicI64::new(0),
            template_registered: AtomicI64::new(0),
            listener_handle: RwLock::new(None),
            listener_running: Arc::new(AtomicBool::new(false)),
            config: RwLock::new(BacnetConfig::default()),
            subscriber_id_counter: AtomicU32::new(1),
        }
    }

    /// Ensure the background listener thread is running
    fn ensure_listener(&self) {
        if self.listener_running.compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed).is_err() {
            return; // Already running or being started
        }

        let config = self.config.read();
        let bind_addr = config.bind_address.clone();
        let bind_port = config.bind_port;
        drop(config);

        // Clone the Arc references so listener writes to the SAME maps as the extension
        let devices = self.devices.clone();
        let covs = self.cov_subscriptions.clone();
        let running = self.listener_running.clone();

        let handle = start_listener(bind_addr, bind_port, devices, covs, running.clone());

        match handle {
            Ok(h) => {
                let mut lh = self.listener_handle.write();
                *lh = Some(h);
                eprintln!("[bacnet-bridge] Background listener started");
            }
            Err(e) => {
                self.listener_running.store(false, Ordering::SeqCst);
                eprintln!("[bacnet-bridge] Failed to start listener: {}", e);
            }
        }
    }
}

impl Default for BacnetBridgeExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for BacnetBridgeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "bacnet-bridge",
                "BACnet Bridge",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description(
                "BACnet/IP bridge — discover building automation devices, read sensors, write controls",
            )
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "bindAddress".to_string(),
                    display_name: "Bind Address".to_string(),
                    description: "Local IP address to bind for BACnet/IP communication".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("0.0.0.0".to_string())),
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "bindPort".to_string(),
                    display_name: "Bind Port".to_string(),
                    description: "UDP port for BACnet/IP (default: 47808)".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(47808)),
                    min: Some(1.0),
                    max: Some(65535.0),
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "defaultTimeoutMs".to_string(),
                    display_name: "Default Timeout (ms)".to_string(),
                    description: "Default timeout for BACnet requests in milliseconds".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(3000)),
                    min: Some(100.0),
                    max: Some(30000.0),
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "pollIntervalMs".to_string(),
                    display_name: "Poll Interval (ms)".to_string(),
                    description: "Default polling interval for subscribed objects in milliseconds".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(10000)),
                    min: Some(1000.0),
                    max: Some(60000.0),
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
            MetricDescriptor {
                name: "cov_subscriptions".to_string(),
                display_name: "COV Subscriptions".to_string(),
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
                display_name: "Discover Devices".to_string(),
                description: "Send Who-Is broadcast to discover BACnet devices on the network".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "low_id".to_string(),
                        display_name: "Low Device ID".to_string(),
                        description: "Low end of device ID range (default: 0)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(0)),
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "high_id".to_string(),
                        display_name: "High Device ID".to_string(),
                        description: "High end of device ID range (default: 4194303)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(4194303)),
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "timeout_ms".to_string(),
                        display_name: "Timeout (ms)".to_string(),
                        description: "How long to wait for I-Am responses (default: 3000)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(3000)),
                        min: Some(500.0),
                        max: Some(30000.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({}),
                    json!({ "low_id": 0, "high_id": 4194303, "timeout_ms": 5000 }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "read_property".to_string(),
                display_name: "Read Property".to_string(),
                description: "Read a property from a BACnet object".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "BACnet device instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "object_type".to_string(),
                        display_name: "Object Type".to_string(),
                        description: "BACnet object type".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec![
                                "analog_input".to_string(),
                                "analog_output".to_string(),
                                "analog_value".to_string(),
                                "binary_input".to_string(),
                                "binary_output".to_string(),
                                "binary_value".to_string(),
                                "multi_state_input".to_string(),
                                "multi_state_output".to_string(),
                                "multi_state_value".to_string(),
                            ],
                        },
                        required: true,
                        default_value: Some(ParamMetricValue::String("analog_input".to_string())),
                        min: None,
                        max: None,
                        options: vec![
                            "analog_input".to_string(),
                            "analog_output".to_string(),
                            "analog_value".to_string(),
                            "binary_input".to_string(),
                            "binary_output".to_string(),
                            "binary_value".to_string(),
                            "multi_state_input".to_string(),
                            "multi_state_output".to_string(),
                            "multi_state_value".to_string(),
                        ],
                    },
                    ParameterDefinition {
                        name: "instance".to_string(),
                        display_name: "Object Instance".to_string(),
                        description: "Object instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "property_id".to_string(),
                        display_name: "Property ID".to_string(),
                        description: "Property identifier (85=present_value, 77=object_name, 28=description, 117=units)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(85)),
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "device_id": 100, "object_type": "analog_input", "instance": 1, "property_id": 85 }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "read_property_multiple".to_string(),
                display_name: "Read Property Multiple".to_string(),
                description: "Read multiple properties from a BACnet device in one request".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "BACnet device instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "objects".to_string(),
                        display_name: "Objects".to_string(),
                        description: "JSON array of {object_type, instance, properties: [property_id, ...]}".to_string(),
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
                        "device_id": 100,
                        "objects": [
                            { "object_type": "analog_input", "instance": 1, "properties": [85, 77] },
                            { "object_type": "analog_input", "instance": 2, "properties": [85, 77] }
                        ]
                    }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "write_property".to_string(),
                display_name: "Write Property".to_string(),
                description: "Write a value to a BACnet object property".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "BACnet device instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "object_type".to_string(),
                        display_name: "Object Type".to_string(),
                        description: "BACnet object type".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec![
                                "analog_output".to_string(),
                                "analog_value".to_string(),
                                "binary_output".to_string(),
                                "binary_value".to_string(),
                                "multi_state_output".to_string(),
                                "multi_state_value".to_string(),
                            ],
                        },
                        required: true,
                        default_value: Some(ParamMetricValue::String("analog_output".to_string())),
                        min: None,
                        max: None,
                        options: vec![
                            "analog_output".to_string(),
                            "analog_value".to_string(),
                            "binary_output".to_string(),
                            "binary_value".to_string(),
                            "multi_state_output".to_string(),
                            "multi_state_value".to_string(),
                        ],
                    },
                    ParameterDefinition {
                        name: "instance".to_string(),
                        display_name: "Object Instance".to_string(),
                        description: "Object instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "property_id".to_string(),
                        display_name: "Property ID".to_string(),
                        description: "Property identifier (default: 85 = present_value)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(85)),
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "value".to_string(),
                        display_name: "Value".to_string(),
                        description: "Value to write (number, boolean, or string)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "priority".to_string(),
                        display_name: "Priority".to_string(),
                        description: "Write priority (1-16, default: 8). Lower number = higher priority.".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(8)),
                        min: Some(1.0),
                        max: Some(16.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "device_id": 100, "object_type": "analog_output", "instance": 1, "value": "22.5" }),
                    json!({ "device_id": 100, "object_type": "binary_output", "instance": 1, "value": "true", "priority": 5 }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "subscribe_cov".to_string(),
                display_name: "Subscribe COV".to_string(),
                description: "Subscribe to Change-of-Value notifications for a BACnet object".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "BACnet device instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "object_type".to_string(),
                        display_name: "Object Type".to_string(),
                        description: "BACnet object type".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec![
                                "analog_input".to_string(),
                                "analog_output".to_string(),
                                "analog_value".to_string(),
                                "binary_input".to_string(),
                                "binary_output".to_string(),
                                "binary_value".to_string(),
                                "multi_state_input".to_string(),
                                "multi_state_output".to_string(),
                                "multi_state_value".to_string(),
                            ],
                        },
                        required: true,
                        default_value: Some(ParamMetricValue::String("analog_input".to_string())),
                        min: None,
                        max: None,
                        options: vec![
                            "analog_input".to_string(),
                            "analog_output".to_string(),
                            "analog_value".to_string(),
                            "binary_input".to_string(),
                            "binary_output".to_string(),
                            "binary_value".to_string(),
                            "multi_state_input".to_string(),
                            "multi_state_output".to_string(),
                            "multi_state_value".to_string(),
                        ],
                    },
                    ParameterDefinition {
                        name: "instance".to_string(),
                        display_name: "Object Instance".to_string(),
                        description: "Object instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "lifetime".to_string(),
                        display_name: "Lifetime (seconds)".to_string(),
                        description: "Subscription lifetime in seconds (0 = indefinite)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(0)),
                        min: Some(0.0),
                        max: Some(86400.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "confirmed".to_string(),
                        display_name: "Confirmed".to_string(),
                        description: "Request confirmed COV notifications (default: true)".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["true".to_string(), "false".to_string()],
                        },
                        required: false,
                        default_value: Some(ParamMetricValue::String("true".to_string())),
                        min: None,
                        max: None,
                        options: vec!["true".to_string(), "false".to_string()],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "device_id": 100, "object_type": "analog_input", "instance": 1, "lifetime": 3600 }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "unsubscribe_cov".to_string(),
                display_name: "Unsubscribe COV".to_string(),
                description: "Cancel a COV subscription".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "subscriber_id".to_string(),
                        display_name: "Subscriber ID".to_string(),
                        description: "The subscriber process ID returned by subscribe_cov".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "subscriber_id": 1 })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "add_device".to_string(),
                display_name: "Add Device".to_string(),
                description: "Manually add a BACnet device and start polling its objects".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device".to_string(),
                        display_name: "Device Config".to_string(),
                        description: "Device configuration JSON".to_string(),
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
                            "device_id": 100,
                            "ip": "192.168.1.100",
                            "port": 47808,
                            "name": "HVAC Controller",
                            "poll_interval_ms": 5000,
                            "objects": [
                                { "object_type": "analog_input", "instance": 1, "name": "Temperature", "units": "degC" },
                                { "object_type": "analog_input", "instance": 2, "name": "Humidity", "units": "%" },
                                { "object_type": "binary_output", "instance": 1, "name": "Fan Status" }
                            ]
                        }
                    }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "remove_device".to_string(),
                display_name: "Remove Device".to_string(),
                description: "Remove a BACnet device and stop polling".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "BACnet device instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": 100 })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_devices".to_string(),
                display_name: "List Devices".to_string(),
                description: "List all discovered and configured BACnet devices".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_device".to_string(),
                display_name: "Get Device".to_string(),
                description: "Get detailed information about a BACnet device including its objects".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "BACnet device instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": 100 })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_objects".to_string(),
                display_name: "List Objects".to_string(),
                description: "List all objects for a BACnet device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "BACnet device instance number".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(4194303.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": 100 })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".to_string(),
                display_name: "Get Status".to_string(),
                description: "Get extension status including connected devices and COV subscriptions".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
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

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.total_commands.fetch_add(1, Ordering::SeqCst);

        match command {
            "discover" => self.cmd_discover(args),
            "read_property" => self.cmd_read_property(args),
            "read_property_multiple" => self.cmd_read_property_multiple(args),
            "write_property" => self.cmd_write_property(args),
            "subscribe_cov" => self.cmd_subscribe_cov(args),
            "unsubscribe_cov" => self.cmd_unsubscribe_cov(args),
            "add_device" => self.cmd_add_device(args),
            "remove_device" => self.cmd_remove_device(args),
            "list_devices" => self.cmd_list_devices(),
            "get_device" => self.cmd_get_device(args),
            "list_objects" => self.cmd_list_objects(args),
            "get_status" => self.cmd_get_status(),
            "configure" => Ok(json!({"status": "ok"})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::new();

        // Auto-register device template once
        if self.template_registered.load(Ordering::SeqCst) == 0 {
            self.register_template();
        }

        // Extension-level metrics
        metrics.push(ExtensionMetricValue {
            name: "total_commands".to_string(),
            value: ParamMetricValue::Integer(self.total_commands.load(Ordering::SeqCst)),
            timestamp: now,
        });

        // Clone device data snapshot to avoid holding read lock during invoke_capability
        let device_snapshot: Vec<_> = {
            let devices = self.devices.read();
            devices.iter().map(|(_, d)| d.clone()).collect()
        };
        let mut connected_count = 0i64;

        for device in &device_snapshot {
            if device.connected {
                connected_count += 1;
            }

            let device_key = format!("bacnet_{}", device.device_id);

            // Per-object metrics
            for obj in &device.objects {
                if let Some(ref pv) = obj.present_value {
                    let metric_name = format!(
                        "{}.{}_{}_{}",
                        device_key,
                        obj.object_type.name(),
                        obj.instance,
                        "present_value"
                    );
                    if let Some(fv) = pv.as_f64() {
                        metrics.push(ExtensionMetricValue {
                            name: metric_name,
                            value: ParamMetricValue::Float(fv),
                            timestamp: now,
                        });
                    }
                }
            }

            // Per-device status metrics
            metrics.push(ExtensionMetricValue {
                name: format!("{}.connected", device_key),
                value: ParamMetricValue::Integer(if device.connected { 1 } else { 0 }),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: format!("{}.objects_count", device_key),
                value: ParamMetricValue::Integer(device.objects.len() as i64),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: format!("{}.last_seen", device_key),
                value: ParamMetricValue::Integer(device.last_seen_ms),
                timestamp: now,
            });

            // Write per-device metrics via device_metrics_write capability
            let ctx = CapabilityContext::default();
            let did = format!("bacnet_{}", device.device_id);

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": did,
                "metric": "connected",
                "value": if device.connected { "true" } else { "false" },
                "timestamp": now,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": did,
                "metric": "objects_count",
                "value": device.objects.len(),
                "timestamp": now,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": did,
                "metric": "last_seen",
                "value": device.last_seen_ms,
                "timestamp": now,
            }));

            // Write per-object present values as device metrics
            for obj in &device.objects {
                if let Some(ref pv) = obj.present_value {
                    let metric_name = format!("{}_{}", obj.object_type.name(), obj.instance);
                    if let Some(fv) = pv.as_f64() {
                        let _ = ctx.invoke_capability("device_metrics_write", &json!({
                            "device_id": did,
                            "metric": metric_name,
                            "value": fv,
                            "timestamp": now,
                        }));
                    }
                }
            }
        }

        metrics.push(ExtensionMetricValue {
            name: "connected_devices".to_string(),
            value: ParamMetricValue::Integer(connected_count),
            timestamp: now,
        });

        let cov_count = self.cov_subscriptions.read().len() as i64;
        metrics.push(ExtensionMetricValue {
            name: "cov_subscriptions".to_string(),
            value: ParamMetricValue::Integer(cov_count),
            timestamp: now,
        });

        Ok(metrics)
    }

    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        if let Ok(bacnet_config) = serde_json::from_value::<BacnetConfig>(config.clone()) {
            let mut cfg = self.config.write();
            *cfg = bacnet_config;
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Template & Device Registration
// ============================================================================

impl BacnetBridgeExtension {
    /// Register the "bacnet_device" device template with NeoMind.
    fn register_template(&self) {
        let ctx = CapabilityContext::default();

        let template_json = json!({
            "device_type": "bacnet_device",
            "name": "BACnet Device",
            "description": "BACnet/IP building automation device (sensor, actuator, controller)",
            "categories": ["building-automation", "bacnet"],
            "metrics": [
                { "name": "connected", "display_name": "Connection Status", "data_type": "String" },
                { "name": "objects_count", "display_name": "Objects Count", "data_type": "Integer" },
                { "name": "last_seen", "display_name": "Last Seen", "data_type": "Integer", "unit": "ms" }
            ],
            "commands": [
                {
                    "name": "read_property",
                    "display_name": "Read Property",
                    "description": "Read a property from a BACnet object",
                    "parameters": [
                        { "name": "object_type", "display_name": "Object Type", "data_type": "String", "required": true },
                        { "name": "instance", "display_name": "Instance", "data_type": "Integer", "required": true },
                        { "name": "property_id", "display_name": "Property ID", "data_type": "Integer", "required": false }
                    ]
                },
                {
                    "name": "write_property",
                    "display_name": "Write Property",
                    "description": "Write a value to a BACnet object property",
                    "parameters": [
                        { "name": "object_type", "display_name": "Object Type", "data_type": "String", "required": true },
                        { "name": "instance", "display_name": "Instance", "data_type": "Integer", "required": true },
                        { "name": "value", "display_name": "Value", "data_type": "String", "required": true }
                    ]
                }
            ]
        });

        let result = ctx.invoke_capability("device_template_register", &template_json);
        if result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            eprintln!("[bacnet-bridge] Device template registered");
            self.template_registered.store(1, Ordering::SeqCst);
        } else {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            eprintln!(
                "[bacnet-bridge] Template registration failed: {} (will retry)",
                err
            );
            self.template_registered.store(0, Ordering::SeqCst);
        }
    }

    /// Register a device instance with NeoMind via device_register capability.
    fn register_device_instance(&self, device_id: u32, name: &str) {
        let ctx = CapabilityContext::default();

        let device_json = json!({
            "device_id": format!("bacnet_{}", device_id),
            "name": name,
            "device_type": "bacnet_device",
        });

        let result = ctx.invoke_capability("device_register", &device_json);
        if result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            eprintln!("[bacnet-bridge] Device '{}' registered", device_id);
        } else {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            eprintln!(
                "[bacnet-bridge] Device '{}' registration skipped: {}",
                device_id, err
            );
        }
    }
}

// ============================================================================
// Command Handlers
// ============================================================================

impl BacnetBridgeExtension {
    fn parse_object_type(name: &str) -> Result<BacnetObjectType> {
        match name {
            "analog_input" => Ok(BacnetObjectType::AnalogInput),
            "analog_output" => Ok(BacnetObjectType::AnalogOutput),
            "analog_value" => Ok(BacnetObjectType::AnalogValue),
            "binary_input" => Ok(BacnetObjectType::BinaryInput),
            "binary_output" => Ok(BacnetObjectType::BinaryOutput),
            "binary_value" => Ok(BacnetObjectType::BinaryValue),
            "multi_state_input" => Ok(BacnetObjectType::MultiStateInput),
            "multi_state_output" => Ok(BacnetObjectType::MultiStateOutput),
            "multi_state_value" => Ok(BacnetObjectType::MultiStateValue),
            "device" => Ok(BacnetObjectType::Device),
            _ => Err(ExtensionError::InvalidArguments(format!(
                "Unknown object type: '{}'. Valid types: analog_input, analog_output, analog_value, binary_input, binary_output, binary_value, multi_state_input, multi_state_output, multi_state_value",
                name
            ))),
        }
    }

    fn get_device_addr(&self, device_id: u32) -> Result<(String, u16)> {
        let devices = self.devices.read();
        let device = devices.get(&device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!(
                "Device not found: {}. Use discover or add_device first.",
                device_id
            ))
        })?;
        Ok((device.ip_address.clone(), device.port))
    }

    fn create_client(&self) -> std::result::Result<BacnetClient, ExtensionError> {
        let config = self.config.read();
        BacnetClient::new(&config.bind_address, config.bind_port, config.default_timeout_ms)
            .map_err(ExtensionError::ExecutionFailed)
    }

    // ---- discover ----

    fn cmd_discover(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let low = args
            .get("low_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let high = args
            .get("high_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(4194303) as u32;
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(3000);

        if low > high {
            return Err(ExtensionError::InvalidArguments(
                "low_id must be <= high_id".to_string(),
            ));
        }

        let client = self.create_client()?;

        // Send Who-Is and collect I-Am responses
        let who_is_msg = apdu::build_who_is(low, high);
        let responses = client.send_and_collect_responses(&who_is_msg, "255.255.255.255:47808", timeout_ms);

        let mut discovered = Vec::new();
        let mut devices = self.devices.write();

        for (data, addr_str) in &responses {
            if devices.len() >= 500 {
                eprintln!("[bacnet-bridge] Warning: maximum device limit (500) reached, ignoring remaining responses");
                break;
            }

            if let Some(apdu::ApduResponse::IAm {
                device_id,
                max_apdu,
                segmentation,
                vendor_id,
            }) = apdu::parse_response(data)
            {
                let ip_port = parse_ip_port(addr_str);

                if let Some(device) = devices.get_mut(&device_id) {
                    device.connected = true;
                    device.last_seen_ms = chrono::Utc::now().timestamp_millis();
                    device.max_apdu = Some(max_apdu);
                    device.vendor_id = Some(vendor_id);
                    device.ip_address = ip_port.0.clone();
                    device.port = ip_port.1;
                } else {
                    devices.insert(
                        device_id,
                        BacnetDevice {
                            device_id,
                            ip_address: ip_port.0.clone(),
                            port: ip_port.1,
                            name: None,
                            vendor_id: Some(vendor_id),
                            vendor_name: None,
                            model: None,
                            firmware: None,
                            description: None,
                            max_apdu: Some(max_apdu),
                            segmentation: Some(format!("{}", segmentation)),
                            objects: Vec::new(),
                            connected: true,
                            last_seen_ms: chrono::Utc::now().timestamp_millis(),
                        },
                    );
                }

                discovered.push(json!({
                    "device_id": device_id,
                    "ip": ip_port.0,
                    "port": ip_port.1,
                    "vendor_id": vendor_id,
                    "max_apdu": max_apdu,
                }));
            }
        }

        Ok(json!({
            "success": true,
            "count": discovered.len(),
            "devices": discovered,
        }))
    }

    // ---- read_property ----

    fn cmd_read_property(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })? as u32;

        let object_type_str = args
            .get("object_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'object_type' parameter".to_string())
            })?;
        let object_type = Self::parse_object_type(object_type_str)?;

        let instance = args
            .get("instance")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'instance' parameter".to_string())
            })? as u32;

        let property_id = args
            .get("property_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(85) as u8;

        let (ip, port) = self.get_device_addr(device_id)?;
        let target = format!("{}:{}", ip, port);

        let client = self.create_client()?;
        let msg = apdu::build_read_property(device_id, object_type, instance, property_id);

        let (response, _) = client
            .send_and_receive(&target, &msg)
            .map_err(ExtensionError::ExecutionFailed)?;

        match apdu::parse_response(&response) {
            Some(apdu::ApduResponse::ReadPropertyAck {
                object_type: ot,
                instance: inst,
                property_id: pid,
                value,
            }) => {
                // Cache the value if it's present_value
                if pid == apdu::PROPERTY_PRESENT_VALUE {
                    let mut devices = self.devices.write();
                    if let Some(device) = devices.get_mut(&device_id) {
                        for obj in &mut device.objects {
                            if obj.object_type == ot && obj.instance == inst {
                                obj.present_value = Some(value.clone());
                            }
                        }
                    }
                }

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "object_type": ot.name(),
                    "instance": inst,
                    "property_id": pid,
                    "value": value.to_json_value(),
                }))
            }
            Some(apdu::ApduResponse::Error {
                invoke_id: _,
                error_class,
                error_code,
            }) => Err(ExtensionError::ExecutionFailed(format!(
                "BACnet error: class={}, code={}",
                error_class, error_code
            ))),
            _ => Err(ExtensionError::ExecutionFailed(
                "Failed to parse ReadProperty response".to_string(),
            )),
        }
    }

    // ---- read_property_multiple ----

    fn cmd_read_property_multiple(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })? as u32;

        let objects_value = args
            .get("objects")
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'objects' parameter".to_string())
            })?;

        let objects_arr = objects_value
            .as_array()
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("'objects' must be an array".to_string())
            })?;

        let mut reads: Vec<(BacnetObjectType, u32, Vec<u8>)> = Vec::new();
        for obj_val in objects_arr {
            let ot_str = obj_val
                .get("object_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ExtensionError::InvalidArguments(
                        "Each object must have 'object_type'".to_string(),
                    )
                })?;
            let ot = Self::parse_object_type(ot_str)?;
            let inst = obj_val
                .get("instance")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    ExtensionError::InvalidArguments(
                        "Each object must have 'instance'".to_string(),
                    )
                })? as u32;
            let props: Vec<u8> = obj_val
                .get("properties")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u8))
                        .collect()
                })
                .unwrap_or_else(|| vec![apdu::PROPERTY_PRESENT_VALUE]);

            reads.push((ot, inst, props));
        }

        let (ip, port) = self.get_device_addr(device_id)?;
        let target = format!("{}:{}", ip, port);

        let client = self.create_client()?;
        let msg = apdu::build_read_property_multiple(device_id, &reads);

        let (response, _) = client
            .send_and_receive(&target, &msg)
            .map_err(ExtensionError::ExecutionFailed)?;

        match apdu::parse_response(&response) {
            Some(apdu::ApduResponse::ReadPropertyMultipleAck { values }) => {
                let results: Vec<serde_json::Value> = values
                    .iter()
                    .map(|(ot, inst, pid, val)| {
                        json!({
                            "object_type": ot.name(),
                            "instance": inst,
                            "property_id": pid,
                            "value": val.to_json_value(),
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "count": results.len(),
                    "values": results,
                }))
            }
            Some(apdu::ApduResponse::Error {
                error_class, error_code, ..
            }) => Err(ExtensionError::ExecutionFailed(format!(
                "BACnet error: class={}, code={}",
                error_class, error_code
            ))),
            _ => Err(ExtensionError::ExecutionFailed(
                "Failed to parse ReadPropertyMultiple response".to_string(),
            )),
        }
    }

    // ---- write_property ----

    fn cmd_write_property(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })? as u32;

        let object_type_str = args
            .get("object_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'object_type' parameter".to_string())
            })?;
        let object_type = Self::parse_object_type(object_type_str)?;

        let instance = args
            .get("instance")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'instance' parameter".to_string())
            })? as u32;

        let property_id = args
            .get("property_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(85) as u8;

        let value_str = args
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'value' parameter".to_string())
            })?;

        let mut value = parse_value_from_str(value_str)?;

        // Per ASHRAE 135-2020, Analog Output/Value Present_Value is REAL (tag 4).
        // Coerce integer strings to Real for analog object types.
        if matches!(
            object_type,
            BacnetObjectType::AnalogOutput | BacnetObjectType::AnalogValue
        ) {
            value = match value {
                BacnetValue::Unsigned(v) => BacnetValue::Real(v as f64),
                BacnetValue::Integer(v) => BacnetValue::Real(v as f64),
                other => other,
            };
        }

        let priority = args
            .get("priority")
            .and_then(|v| v.as_u64())
            .map(|p| p as u8);

        let (ip, port) = self.get_device_addr(device_id)?;
        let target = format!("{}:{}", ip, port);

        let client = self.create_client()?;
        let msg = apdu::build_write_property(
            device_id,
            object_type,
            instance,
            property_id,
            &value,
            priority,
        );

        let (response, _) = client
            .send_and_receive(&target, &msg)
            .map_err(ExtensionError::ExecutionFailed)?;

        match apdu::parse_response(&response) {
            Some(apdu::ApduResponse::SimpleAck { .. }) => {
                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "object_type": object_type.name(),
                    "instance": instance,
                    "property_id": property_id,
                    "value": value.to_json_value(),
                    "message": "Property written successfully"
                }))
            }
            Some(apdu::ApduResponse::Error {
                error_class, error_code, ..
            }) => Err(ExtensionError::ExecutionFailed(format!(
                "BACnet error: class={}, code={}",
                error_class, error_code
            ))),
            _ => Err(ExtensionError::ExecutionFailed(
                "Unexpected response to WriteProperty".to_string(),
            )),
        }
    }

    // ---- subscribe_cov ----

    fn cmd_subscribe_cov(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })? as u32;

        let object_type_str = args
            .get("object_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'object_type' parameter".to_string())
            })?;
        let object_type = Self::parse_object_type(object_type_str)?;

        let instance = args
            .get("instance")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'instance' parameter".to_string())
            })? as u32;

        let lifetime = args
            .get("lifetime")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let confirmed = args
            .get("confirmed")
            .and_then(|v| v.as_bool())
            .or_else(|| args.get("confirmed").and_then(|v| v.as_str()).map(|s| s == "true"))
            .unwrap_or(true);

        let subscriber_id = self.subscriber_id_counter.fetch_add(1, Ordering::SeqCst);

        let (ip, port) = self.get_device_addr(device_id)?;
        let target = format!("{}:{}", ip, port);

        let client = self.create_client()?;
        let msg = apdu::build_subscribe_cov(
            subscriber_id,
            device_id,
            object_type,
            instance,
            lifetime,
            confirmed,
        );

        let (response, _) = client
            .send_and_receive(&target, &msg)
            .map_err(ExtensionError::ExecutionFailed)?;

        match apdu::parse_response(&response) {
            Some(apdu::ApduResponse::SimpleAck { .. }) => {
                let subscription = CovSubscription {
                    subscriber_id,
                    device_id,
                    object_type,
                    instance,
                    lifetime,
                    confirmed,
                    active: true,
                    last_update_ms: chrono::Utc::now().timestamp_millis(),
                };

                self.cov_subscriptions
                    .write()
                    .insert(subscriber_id, subscription);

                Ok(json!({
                    "success": true,
                    "subscriber_id": subscriber_id,
                    "device_id": device_id,
                    "object_type": object_type.name(),
                    "instance": instance,
                    "lifetime": lifetime,
                    "confirmed": confirmed,
                    "message": "COV subscription active"
                }))
            }
            Some(apdu::ApduResponse::Error {
                error_class, error_code, ..
            }) => Err(ExtensionError::ExecutionFailed(format!(
                "BACnet error: class={}, code={}",
                error_class, error_code
            ))),
            _ => Err(ExtensionError::ExecutionFailed(
                "Unexpected response to SubscribeCOV".to_string(),
            )),
        }
    }

    // ---- unsubscribe_cov ----

    fn cmd_unsubscribe_cov(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let subscriber_id = args
            .get("subscriber_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'subscriber_id' parameter".to_string())
            })? as u32;

        let sub = self
            .cov_subscriptions
            .write()
            .remove(&subscriber_id)
            .ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!(
                    "COV subscription not found: {}",
                    subscriber_id
                ))
            })?;

        // Send unsubscribe (lifetime=0 cancels subscription)
        let (ip, port) = self.get_device_addr(sub.device_id).unwrap_or(("0.0.0.0".to_string(), 47808));
        let target = format!("{}:{}", ip, port);

        if let Ok(client) = self.create_client() {
            let msg = apdu::build_subscribe_cov(
                subscriber_id,
                sub.device_id,
                sub.object_type,
                sub.instance,
                0, // lifetime=0 means cancel
                false,
            );
            let _ = client.send_and_receive(&target, &msg);
        }

        Ok(json!({
            "success": true,
            "subscriber_id": subscriber_id,
            "message": "COV subscription cancelled"
        }))
    }

    // ---- add_device ----

    fn cmd_add_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_value = args.get("device").ok_or_else(|| {
            ExtensionError::InvalidArguments("Missing 'device' parameter".to_string())
        })?;

        let device_id = device_value
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments(
                    "Device config must have 'device_id'".to_string(),
                )
            })? as u32;

        let ip = device_value
            .get("ip")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Device config must have 'ip'".to_string())
            })?
            .to_string();

        let port = device_value
            .get("port")
            .and_then(|v| v.as_u64())
            .unwrap_or(47808) as u16;

        let name = device_value
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let poll_interval_ms = device_value
            .get("poll_interval_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(10000);

        // Parse objects
        let objects: Vec<BacnetObject> = device_value
            .get("objects")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|obj_val| {
                        let ot_str = obj_val.get("object_type")?.as_str()?;
                        let ot = Self::parse_object_type(ot_str).ok()?;
                        let inst = obj_val.get("instance")?.as_u64()? as u32;
                        Some(BacnetObject {
                            object_type: ot,
                            instance: inst,
                            name: obj_val.get("name").and_then(|v| v.as_str()).map(String::from),
                            description: obj_val
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            present_value: None,
                            units: obj_val.get("units").and_then(|v| v.as_str()).map(String::from),
                            cov_subscribed: false,
                            cov_lifetime: None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Stop existing device manager if any
        {
            let mut managers = self.device_managers.write();
            if let Some(mut old) = managers.remove(&device_id) {
                old.stop();
            }
        }

        // Insert/update device state
        {
            let mut devices = self.devices.write();
            devices.insert(
                device_id,
                BacnetDevice {
                    device_id,
                    ip_address: ip.clone(),
                    port,
                    name: name.clone(),
                    vendor_id: None,
                    vendor_name: None,
                    model: None,
                    firmware: None,
                    description: None,
                    max_apdu: None,
                    segmentation: None,
                    objects,
                    connected: false,
                    last_seen_ms: 0,
                },
            );
        }

        // Start polling
        let _config = self.config.read();

        // Create a new manager with a snapshot of the device for polling
        // Share the actual device map so polling thread writes are visible to produce_metrics
        let devices_shared = self.devices.clone();

        let cfg = self.config.read();
        let mut manager = BacnetDeviceManager::new(device_id, ip.clone(), port, poll_interval_ms);
        manager
            .start(
                devices_shared,
                cfg.bind_address.clone(),
                cfg.bind_port,
                cfg.default_timeout_ms,
            )
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to start polling: {}", e)))?;

        self.device_managers.write().insert(device_id, manager);

        // Register device with NeoMind platform
        let device_name = name
            .clone()
            .unwrap_or_else(|| format!("BACnet Device {}", device_id));
        self.register_device_instance(device_id, &device_name);

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "ip": ip,
            "port": port,
            "message": "Device added and polling started"
        }))
    }

    // ---- remove_device ----

    fn cmd_remove_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })? as u32;

        // Stop and remove device manager
        {
            let mut managers = self.device_managers.write();
            if let Some(mut old) = managers.remove(&device_id) {
                old.stop();
            }
        }

        // Remove device from state
        {
            let mut devices = self.devices.write();
            devices
                .remove(&device_id)
                .ok_or_else(|| {
                    ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
                })?;
        }

        // Remove COV subscriptions for this device
        {
            let mut covs = self.cov_subscriptions.write();
            covs.retain(|_, sub| sub.device_id != device_id);
        }

        // Unregister from NeoMind
        let ctx = CapabilityContext::default();
        let _ = ctx.invoke_capability("device_unregister", &json!({
            "device_id": format!("bacnet_{}", device_id),
        }));

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "message": "Device removed and polling stopped"
        }))
    }

    // ---- list_devices ----

    fn cmd_list_devices(&self) -> Result<serde_json::Value> {
        let devices = self.devices.read();
        let device_list: Vec<serde_json::Value> = devices
            .iter()
            .map(|(id, device)| {
                json!({
                    "device_id": id,
                    "ip": device.ip_address,
                    "port": device.port,
                    "name": device.name,
                    "vendor_id": device.vendor_id,
                    "connected": device.connected,
                    "objects_count": device.objects.len(),
                    "last_seen_ms": device.last_seen_ms,
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "count": device_list.len(),
            "devices": device_list,
        }))
    }

    // ---- get_device ----

    fn cmd_get_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })? as u32;

        let devices = self.devices.read();
        let device = devices.get(&device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        Ok(json!({
            "success": true,
            "device_id": device.device_id,
            "ip": device.ip_address,
            "port": device.port,
            "name": device.name,
            "vendor_id": device.vendor_id,
            "vendor_name": device.vendor_name,
            "model": device.model,
            "firmware": device.firmware,
            "description": device.description,
            "max_apdu": device.max_apdu,
            "segmentation": device.segmentation,
            "connected": device.connected,
            "last_seen_ms": device.last_seen_ms,
            "objects_count": device.objects.len(),
        }))
    }

    // ---- list_objects ----

    fn cmd_list_objects(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })? as u32;

        let devices = self.devices.read();
        let device = devices.get(&device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        let objects: Vec<serde_json::Value> = device
            .objects
            .iter()
            .map(|obj| {
                json!({
                    "object_type": obj.object_type.name(),
                    "instance": obj.instance,
                    "name": obj.name,
                    "description": obj.description,
                    "present_value": obj.present_value.as_ref().map(|v| v.to_json_value()),
                    "units": obj.units,
                    "cov_subscribed": obj.cov_subscribed,
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "count": objects.len(),
            "objects": objects,
        }))
    }

    // ---- get_status ----

    fn cmd_get_status(&self) -> Result<serde_json::Value> {
        let devices = self.devices.read();
        let connected = devices.values().filter(|d| d.connected).count();
        let total = devices.len();
        drop(devices);

        let covs = self.cov_subscriptions.read();
        let active_covs = covs.values().filter(|s| s.active).count();
        let total_covs = covs.len();
        drop(covs);

        let config = self.config.read();

        Ok(json!({
            "success": true,
            "status": {
                "total_devices": total,
                "connected_devices": connected,
                "total_cov_subscriptions": total_covs,
                "active_cov_subscriptions": active_covs,
                "total_commands": self.total_commands.load(Ordering::SeqCst),
                "bind_address": config.bind_address,
                "bind_port": config.bind_port,
                "default_timeout_ms": config.default_timeout_ms,
                "poll_interval_ms": config.poll_interval_ms,
                "listener_running": self.listener_running.load(Ordering::SeqCst),
            }
        }))
    }
}

/// Parse a string value into a BacnetValue for write operations
fn parse_value_from_str(s: &str) -> Result<BacnetValue> {
    // Try boolean first
    if s == "true" {
        return Ok(BacnetValue::Boolean(true));
    }
    if s == "false" {
        return Ok(BacnetValue::Boolean(false));
    }

    // Try integer (before float so "-5" becomes Integer, not Real)
    if let Ok(i) = s.parse::<i32>() {
        if i >= 0 {
            return Ok(BacnetValue::Unsigned(i as u32));
        }
        return Ok(BacnetValue::Integer(i));
    }

    // Try float
    if let Ok(f) = s.parse::<f64>() {
        return Ok(BacnetValue::Real(f));
    }

    // Default to string
    Ok(BacnetValue::String(s.to_string()))
}

// ============================================================================
// FFI Exports
// ============================================================================

neomind_extension_sdk::neomind_export!(BacnetBridgeExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = BacnetBridgeExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "bacnet-bridge");
        assert_eq!(meta.name, "BACnet Bridge");
        assert!(meta.description.is_some());
    }

    #[test]
    fn test_extension_metrics() {
        let ext = BacnetBridgeExtension::new();
        let metrics = ext.metrics();
        assert!(metrics.len() >= 3);
        assert!(metrics.iter().any(|m| m.name == "total_commands"));
        assert!(metrics.iter().any(|m| m.name == "connected_devices"));
        assert!(metrics.iter().any(|m| m.name == "cov_subscriptions"));
    }

    #[test]
    fn test_extension_commands() {
        let ext = BacnetBridgeExtension::new();
        let commands = ext.commands();
        assert!(commands.len() >= 10);

        let command_names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(command_names.contains(&"discover"));
        assert!(command_names.contains(&"read_property"));
        assert!(command_names.contains(&"read_property_multiple"));
        assert!(command_names.contains(&"write_property"));
        assert!(command_names.contains(&"subscribe_cov"));
        assert!(command_names.contains(&"unsubscribe_cov"));
        assert!(command_names.contains(&"add_device"));
        assert!(command_names.contains(&"remove_device"));
        assert!(command_names.contains(&"list_devices"));
        assert!(command_names.contains(&"get_device"));
        assert!(command_names.contains(&"list_objects"));
        assert!(command_names.contains(&"get_status"));
        assert!(command_names.contains(&"configure"));
    }

    #[test]
    fn test_produce_metrics_no_devices() {
        let ext = BacnetBridgeExtension::new();
        let metrics = ext.produce_metrics().unwrap();
        assert!(metrics.len() >= 3);

        let cmd_metric = metrics.iter().find(|m| m.name == "total_commands").unwrap();
        if let ParamMetricValue::Integer(v) = cmd_metric.value {
            assert_eq!(v, 0);
        } else {
            panic!("Expected Integer value for total_commands");
        }
    }

    #[tokio::test]
    async fn test_configure_command() {
        let mut ext = BacnetBridgeExtension::new();
        let result = ext.configure(&json!({})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_devices_empty() {
        let ext = BacnetBridgeExtension::new();
        let result = ext.execute_command("list_devices", &json!({})).await.unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["count"], 0);
    }

    #[tokio::test]
    async fn test_get_status() {
        let ext = BacnetBridgeExtension::new();
        let result = ext.execute_command("get_status", &json!({})).await.unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["status"]["total_devices"], 0);
    }

    #[tokio::test]
    async fn test_remove_nonexistent_device() {
        let ext = BacnetBridgeExtension::new();
        let result = ext
            .execute_command("remove_device", &json!({"device_id": 9999}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = BacnetBridgeExtension::new();
        let result = ext.execute_command("nonexistent", &json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_object_type() {
        assert_eq!(
            BacnetBridgeExtension::parse_object_type("analog_input").unwrap(),
            BacnetObjectType::AnalogInput
        );
        assert_eq!(
            BacnetBridgeExtension::parse_object_type("binary_output").unwrap(),
            BacnetObjectType::BinaryOutput
        );
        assert_eq!(
            BacnetBridgeExtension::parse_object_type("multi_state_value").unwrap(),
            BacnetObjectType::MultiStateValue
        );
        assert!(BacnetBridgeExtension::parse_object_type("invalid").is_err());
    }

    #[test]
    fn test_parse_value_from_str() {
        assert_eq!(parse_value_from_str("true").unwrap(), BacnetValue::Boolean(true));
        assert_eq!(parse_value_from_str("false").unwrap(), BacnetValue::Boolean(false));
        assert_eq!(parse_value_from_str("22.5").unwrap(), BacnetValue::Real(22.5));
        assert_eq!(parse_value_from_str("100").unwrap(), BacnetValue::Unsigned(100));
        assert_eq!(parse_value_from_str("-5").unwrap(), BacnetValue::Integer(-5));
        assert_eq!(
            parse_value_from_str("hello").unwrap(),
            BacnetValue::String("hello".to_string())
        );
    }

    #[test]
    fn test_bacnet_value_as_f64() {
        assert_eq!(BacnetValue::Real(23.5).as_f64(), Some(23.5));
        assert_eq!(BacnetValue::Integer(-10).as_f64(), Some(-10.0));
        assert_eq!(BacnetValue::Unsigned(42).as_f64(), Some(42.0));
        assert_eq!(BacnetValue::Boolean(true).as_f64(), Some(1.0));
        assert_eq!(BacnetValue::Boolean(false).as_f64(), Some(0.0));
        assert_eq!(BacnetValue::Null.as_f64(), None);
        assert_eq!(BacnetValue::String("x".into()).as_f64(), None);
    }
}
