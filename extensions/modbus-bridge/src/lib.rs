//! NeoMind Modbus Bridge Extension
//!
//! Connects to Modbus TCP/RTU devices (PLCs, power meters, sensors, etc.)
//! and exposes register data as metrics for the NeoMind platform.
//!
//! Features:
//! - Modbus TCP and RTU (serial) support
//! - Automatic background polling of configured registers
//! - On-demand register read/write operations
//! - Per-device metrics export
//! - Device template registration via CapabilityContext
//!
//! # Architecture Note
//!
//! This extension uses **sync Modbus client** (`tokio-modbus` sync features)
//! to avoid Tokio runtime compatibility issues when loaded as a dynamic library
//! (.dylib/.so/.dll). Background polling runs on dedicated std threads.
//! Uses `parking_lot::RwLock` instead of `tokio::sync::RwLock` for simpler
//! sync access patterns (no .await needed).

mod types;
mod register_map;
mod device;

use neomind_extension_sdk::{
    async_trait, json, CapabilityContext, Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use device::ModbusDevice;
use types::DeviceConfig;

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct ModbusBridgeExtension {
    devices: RwLock<HashMap<String, ModbusDevice>>,
    total_commands: AtomicI64,
    /// 0 = template not registered yet, 1 = registered
    template_registered: AtomicI64,
}

impl ModbusBridgeExtension {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
            total_commands: AtomicI64::new(0),
            template_registered: AtomicI64::new(0),
        }
    }
}

impl Default for ModbusBridgeExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for ModbusBridgeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "modbus-bridge",
                "Modbus Bridge",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description("Modbus TCP/RTU bridge extension — connect PLCs, power meters, sensors, and industrial devices")
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "defaultPollInterval".to_string(),
                    display_name: "Default Poll Interval".to_string(),
                    description: "Default polling interval in milliseconds".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(5000)),
                    min: Some(100.0),
                    max: Some(60000.0),
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "defaultTimeout".to_string(),
                    display_name: "Default Timeout".to_string(),
                    description: "Default connection timeout in milliseconds".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(3000)),
                    min: Some(100.0),
                    max: Some(30000.0),
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
                name: "total_poll_errors".to_string(),
                display_name: "Total Poll Errors".to_string(),
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
                name: "add_device".to_string(),
                display_name: "Add Device".to_string(),
                description: "Add a Modbus device and start polling".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device".to_string(),
                        display_name: "Device Config".to_string(),
                        description: "Device configuration JSON (register addresses are 0-based, e.g. use 0 for holding register 40001)".to_string(),
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
                            "device_id": "plc-001",
                            "name": "Workshop PLC",
                            "mode": "tcp",
                            "ip": "192.168.1.100",
                            "port": 502,
                            "slave_id": 1,
                            "poll_interval_ms": 5000,
                            "registers": [
                                { "address": 0, "name": "temperature", "register_type": "holding", "type": "float32", "scale": 0.01, "unit": "\u{00b0}C" },
                                { "address": 2, "name": "humidity", "register_type": "holding", "type": "uint16", "unit": "%" },
                                { "address": 3, "name": "voltage", "register_type": "holding", "type": "uint16", "scale": 0.1, "unit": "V" }
                            ]
                        }
                    }),
                    json!({
                        "device": {
                            "device_id": "plc-002",
                            "name": "Energy Meter (Little-Endian)",
                            "mode": "tcp",
                            "ip": "192.168.1.101",
                            "port": 502,
                            "slave_id": 2,
                            "poll_interval_ms": 3000,
                            "registers": [
                                { "address": 0, "name": "power", "register_type": "holding", "type": "float32", "word_order": "little", "scale": 0.001, "unit": "kW" }
                            ]
                        }
                    }),
                    json!({
                        "device": {
                            "device_id": "sensor-rtu-001",
                            "name": "RTU Sensor",
                            "mode": "rtu",
                            "serial_port": "/dev/ttyUSB0",
                            "baud_rate": 9600,
                            "slave_id": 3,
                            "poll_interval_ms": 10000,
                            "registers": [
                                { "address": 0, "name": "temp", "register_type": "input", "type": "int16", "scale": 0.1, "unit": "\u{00b0}C" }
                            ]
                        }
                    }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "remove_device".to_string(),
                display_name: "Remove Device".to_string(),
                description: "Remove a Modbus device and stop polling".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device to remove".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_devices".to_string(),
                display_name: "List Devices".to_string(),
                description: "List all configured Modbus devices and their status".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_device_data".to_string(),
                display_name: "Get Device Data".to_string(),
                description: "Get current register values for a device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "read_registers".to_string(),
                display_name: "Read Registers".to_string(),
                description: "Read registers from a device on-demand".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "address".to_string(),
                        display_name: "Start Address".to_string(),
                        description: "Starting register address (0-based, e.g. 0 = first register)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "count".to_string(),
                        display_name: "Register Count".to_string(),
                        description: "Number of registers to read".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: Some(ParamMetricValue::Integer(1)),
                        min: Some(1.0),
                        max: Some(125.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "register_type".to_string(),
                        display_name: "Register Type".to_string(),
                        description: "Register type to read (default: holding)".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["holding".to_string(), "input".to_string()],
                        },
                        required: false,
                        default_value: Some(ParamMetricValue::String("holding".to_string())),
                        min: None,
                        max: None,
                        options: vec!["holding".to_string(), "input".to_string()],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "device_id": "plc-001", "address": 0, "count": 10 }),
                    json!({ "device_id": "plc-001", "address": 0, "count": 5, "register_type": "input" }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "write_register".to_string(),
                display_name: "Write Register".to_string(),
                description: "Write a single holding register on a device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "address".to_string(),
                        display_name: "Register Address".to_string(),
                        description: "Register address to write".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "value".to_string(),
                        display_name: "Value".to_string(),
                        description: "Value to write (0-65535)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001", "address": 100, "value": 1 })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "write_registers".to_string(),
                display_name: "Write Registers".to_string(),
                description: "Write multiple holding registers on a device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "address".to_string(),
                        display_name: "Start Address".to_string(),
                        description: "Starting register address (0-based, e.g. 0 = first register)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "values".to_string(),
                        display_name: "Values".to_string(),
                        description: "Array of values to write (0-65535)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001", "address": 100, "values": [1, 2, 3] })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "write_coil".to_string(),
                display_name: "Write Coil".to_string(),
                description: "Write a single coil on a device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "address".to_string(),
                        display_name: "Coil Address".to_string(),
                        description: "Coil address to write".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "value".to_string(),
                        display_name: "Value".to_string(),
                        description: "Coil value (true/false)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec!["true".to_string(), "false".to_string()],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001", "address": 0, "value": "true" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "write_coils".to_string(),
                display_name: "Write Coils".to_string(),
                description: "Write multiple coils on a device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "address".to_string(),
                        display_name: "Start Address".to_string(),
                        description: "Starting coil address".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(0.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "values".to_string(),
                        display_name: "Values".to_string(),
                        description: "Array of boolean values (e.g. [true,false,true])".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001", "address": 0, "values": [true, false, true] })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "update_polling".to_string(),
                display_name: "Update Polling".to_string(),
                description: "Update the polling interval for a device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "interval_ms".to_string(),
                        display_name: "Poll Interval (ms)".to_string(),
                        description: "New polling interval in milliseconds".to_string(),
                        param_type: MetricDataType::Integer,
                        required: true,
                        default_value: None,
                        min: Some(100.0),
                        max: Some(60000.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001", "interval_ms": 10000 })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "set_register_map".to_string(),
                display_name: "Set Register Map".to_string(),
                description: "Replace the register map for a device (restarts polling)".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "registers".to_string(),
                        display_name: "Registers".to_string(),
                        description: "New register configuration array".to_string(),
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
                        "device_id": "plc-001",
                        "registers": [
                            { "address": 0, "name": "voltage", "type": "uint16", "scale": 0.1, "unit": "V" }
                        ]
                    }),
                ],
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
            "add_device" => self.cmd_add_device(args),
            "remove_device" => self.cmd_remove_device(args),
            "list_devices" => self.cmd_list_devices(),
            "get_device_data" => self.cmd_get_device_data(args),
            "read_registers" => self.cmd_read_registers(args),
            "write_register" => self.cmd_write_register(args),
            "write_registers" => self.cmd_write_registers(args),
            "write_coil" => self.cmd_write_coil(args),
            "write_coils" => self.cmd_write_coils(args),
            "update_polling" => self.cmd_update_polling(args),
            "set_register_map" => self.cmd_set_register_map(args),
            "configure" => Ok(json!({"status": "ok"})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::new();

        // Auto-register device template once (register_template manages the flag)
        if self.template_registered.load(Ordering::SeqCst) == 0 {
            self.register_template();
        }

        metrics.push(ExtensionMetricValue {
            name: "total_commands".to_string(),
            value: ParamMetricValue::Integer(self.total_commands.load(Ordering::SeqCst)),
            timestamp: now,
        });

        let devices = self.devices.read();
        let mut connected_count = 0i64;
        let mut total_errors = 0i64;

        for (_id, device) in devices.iter() {
            let state = device.get_state();
            if state.connected {
                connected_count += 1;
            }
            total_errors += state.poll_errors as i64;

            // Per-register metrics
            for rv in &state.register_values {
                metrics.push(ExtensionMetricValue {
                    name: format!("modbus.{}.{}", state.config.device_id, rv.name),
                    value: ParamMetricValue::Float(rv.value),
                    timestamp: now,
                });
            }

            // Per-device status metrics
            metrics.push(ExtensionMetricValue {
                name: format!("modbus.{}.connected", state.config.device_id),
                value: ParamMetricValue::Integer(if state.connected { 1 } else { 0 }),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: format!("modbus.{}.poll_errors", state.config.device_id),
                value: ParamMetricValue::Integer(state.poll_errors as i64),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: format!("modbus.{}.last_poll_ms", state.config.device_id),
                value: ParamMetricValue::Integer(state.last_poll_ms as i64),
                timestamp: now,
            });

            // Write per-device metrics via device_metrics_write capability
            let ctx = CapabilityContext::default();
            let device_id = &state.config.device_id;

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": device_id,
                "metric": "connected",
                "value": if state.connected { "true" } else { "false" },
                "timestamp": now,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": device_id,
                "metric": "poll_errors",
                "value": state.poll_errors,
                "timestamp": now,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": device_id,
                "metric": "last_poll_ms",
                "value": state.last_poll_ms,
                "timestamp": now,
            }));

            // Write per-register values as device metrics
            for rv in &state.register_values {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": device_id,
                    "metric": rv.name,
                    "value": rv.value,
                    "timestamp": now,
                }));
            }
        }

        metrics.push(ExtensionMetricValue {
            name: "connected_devices".to_string(),
            value: ParamMetricValue::Integer(connected_count),
            timestamp: now,
        });
        metrics.push(ExtensionMetricValue {
            name: "total_poll_errors".to_string(),
            value: ParamMetricValue::Integer(total_errors),
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

impl ModbusBridgeExtension {
    /// Register the "modbus_device" device template with NeoMind.
    /// Called once from produce_metrics() when template_registered == 0.
    fn register_template(&self) {
        let ctx = CapabilityContext::default();

        let template_json = json!({
            "device_type": "modbus_device",
            "name": "Modbus Device",
            "description": "Modbus TCP/RTU industrial device (PLC, power meter, sensor)",
            "categories": ["industrial", "modbus"],
            "metrics": [
                { "name": "connected", "display_name": "Connection Status", "data_type": "String" },
                { "name": "poll_errors", "display_name": "Poll Errors", "data_type": "Integer" },
                { "name": "last_poll_ms", "display_name": "Last Poll Duration", "data_type": "Integer", "unit": "ms" }
            ],
            "commands": [
                {
                    "name": "read_registers",
                    "display_name": "Read Registers",
                    "description": "Read holding registers",
                    "parameters": [
                        { "name": "address", "display_name": "Start Address", "data_type": "Integer", "required": true },
                        { "name": "count", "display_name": "Count", "data_type": "Integer", "required": true }
                    ]
                },
                {
                    "name": "write_register",
                    "display_name": "Write Register",
                    "description": "Write single holding register",
                    "parameters": [
                        { "name": "address", "display_name": "Address", "data_type": "Integer", "required": true },
                        { "name": "value", "display_name": "Value", "data_type": "Integer", "required": true }
                    ]
                }
            ]
        });

        let result = ctx.invoke_capability("device_template_register", &template_json);
        if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            eprintln!("[modbus-bridge] Device template registered");
            self.template_registered.store(1, Ordering::SeqCst);
        } else {
            let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
            eprintln!("[modbus-bridge] Template registration failed: {} (will retry)", err);
            self.template_registered.store(0, Ordering::SeqCst);
        }
    }

    /// Register a device instance with NeoMind via device_register capability.
    fn register_device(&self, device_id: &str, name: &str) {
        let ctx = CapabilityContext::default();

        let device_json = json!({
            "device_id": device_id,
            "name": name,
            "device_type": "modbus_device",
        });

        let result = ctx.invoke_capability("device_register", &device_json);
        if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            eprintln!("[modbus-bridge] Device '{}' registered", device_id);
        } else {
            let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
            eprintln!("[modbus-bridge] Device '{}' registration skipped: {}", device_id, err);
        }
    }
}

// ============================================================================
// Command Handlers
// ============================================================================

impl ModbusBridgeExtension {
    /// Extract a u16 from args, validating it's within 0-65535 range.
    fn extract_u16(args: &serde_json::Value, name: &str) -> Result<u16> {
        let raw = args
            .get(name)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments(format!("Missing '{}' parameter", name))
            })?;
        if raw > 65535 {
            return Err(ExtensionError::InvalidArguments(format!(
                "'{}' must be 0-65535, got {}",
                name, raw
            )));
        }
        Ok(raw as u16)
    }

    fn cmd_add_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_value = args
            .get("device")
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'device' parameter".to_string()))?;

        let config: DeviceConfig = serde_json::from_value(device_value.clone())
            .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid device config: {}", e)))?;

        let device_id = config.device_id.clone();
        let device_name = config.name.clone().unwrap_or_else(|| device_id.clone());

        // Stop and remove any existing device with the same ID
        {
            let mut devices = self.devices.write();
            if let Some(mut old) = devices.remove(&device_id) {
                old.stop();
            }
        }

        // Create and start the new device
        let mut device = ModbusDevice::new(config);
        device
            .start()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to start device: {}", e)))?;

        {
            let mut devices = self.devices.write();
            devices.insert(device_id.clone(), device);
        }

        // Register device with NeoMind platform
        self.register_device(&device_id, &device_name);

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "message": "Device added and polling started"
        }))
    }

    fn cmd_remove_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        // Remove from map under write lock (quick), then stop outside the lock
        // to avoid blocking other operations while waiting for the polling thread.
        let mut device = {
            let mut devices = self.devices.write();
            devices.remove(device_id).ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
            })?
        };

        // Stop polling thread outside the lock (may block during TCP timeout)
        device.stop();

        // Unregister from NeoMind
        let ctx = CapabilityContext::default();
        let _ = ctx.invoke_capability("device_unregister", &json!({
            "device_id": device_id,
        }));

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "message": "Device removed and polling stopped"
        }))
    }

    fn cmd_list_devices(&self) -> Result<serde_json::Value> {
        let devices = self.devices.read();
        let mut device_list = Vec::new();

        for (id, device) in devices.iter() {
            let state = device.get_state();
            device_list.push(json!({
                "device_id": id,
                "name": state.config.name,
                "mode": state.config.mode,
                "connected": state.connected,
                "register_count": state.config.registers.len(),
                "poll_interval_ms": state.config.poll_interval_ms,
                "poll_errors": state.poll_errors,
                "last_poll_ms": state.last_poll_ms,
            }));
        }

        Ok(json!({
            "success": true,
            "count": device_list.len(),
            "devices": device_list
        }))
    }

    fn cmd_get_device_data(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
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

        let state = device.get_state();
        let register_values: Vec<serde_json::Value> = state
            .register_values
            .iter()
            .map(|rv| {
                json!({
                    "name": rv.name,
                    "value": rv.value,
                    "unit": rv.unit,
                    "raw": rv.raw,
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "connected": state.connected,
            "poll_errors": state.poll_errors,
            "last_poll_ms": state.last_poll_ms,
            "registers": register_values
        }))
    }

    fn cmd_read_registers(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = Self::extract_u16(args, "address")?;

        let count = args
            .get("count")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'count' parameter".to_string())
            })? as u16;

        // Modbus protocol limits: max 125 registers per read request
        if count == 0 {
            return Err(ExtensionError::InvalidArguments(
                "Count must be at least 1".to_string(),
            ));
        }
        if count > 125 {
            return Err(ExtensionError::InvalidArguments(format!(
                "Count {} exceeds Modbus read limit of 125 registers. Split into multiple reads.",
                count
            )));
        }

        let reg_type = args
            .get("register_type")
            .and_then(|v| v.as_str())
            .unwrap_or("holding");

        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        let words = match reg_type {
            "input" => device
                .read_input_registers(address, count)
                .map_err(ExtensionError::ExecutionFailed)?,
            _ => device
                .read_registers(address, count)
                .map_err(ExtensionError::ExecutionFailed)?,
        };

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "register_type": reg_type,
            "count": count,
            "data": words
        }))
    }

    fn cmd_write_register(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = Self::extract_u16(args, "address")?;
        let value = Self::extract_u16(args, "value")?;

        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        device
            .write_register(address, value)
            .map_err(ExtensionError::ExecutionFailed)?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "value": value,
            "message": "Register written successfully"
        }))
    }

    fn cmd_write_registers(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = Self::extract_u16(args, "address")?;

        let values: std::result::Result<Vec<u16>, ExtensionError> = args
            .get("values")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'values' parameter (expected array)".to_string())
            })?
            .iter()
            .map(|v| {
                v.as_u64()
                    .map(|n| n as u16)
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(format!(
                            "Invalid register value: {:?} (expected integer 0-65535)",
                            v
                        ))
                    })
            })
            .collect();
        let values = values?;

        if values.is_empty() {
            return Err(ExtensionError::InvalidArguments(
                "Values array must not be empty".to_string(),
            ));
        }
        // Modbus protocol limit: max 123 registers per write request
        if values.len() > 123 {
            return Err(ExtensionError::InvalidArguments(format!(
                "Cannot write {} registers at once (Modbus limit is 123). Split into multiple writes.",
                values.len()
            )));
        }
        // Validate address + count doesn't overflow Modbus address space
        if (address as u32) + (values.len() as u32) > 65536 {
            return Err(ExtensionError::InvalidArguments(format!(
                "Address {} + count {} overflows Modbus address space (max 65535)",
                address, values.len()
            )));
        }

        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        device
            .write_registers(address, &values)
            .map_err(ExtensionError::ExecutionFailed)?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "count": values.len(),
            "message": "Registers written successfully"
        }))
    }

    fn cmd_write_coil(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = Self::extract_u16(args, "address")?;

        let value_str = args
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'value' parameter".to_string())
            })?;
        let value = value_str
            .parse::<bool>()
            .map_err(|_| {
                ExtensionError::InvalidArguments(
                    "Invalid coil value, expected 'true' or 'false'".to_string(),
                )
            })?;

        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        device
            .write_coil(address, value)
            .map_err(ExtensionError::ExecutionFailed)?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "value": value,
            "message": "Coil written successfully"
        }))
    }

    fn cmd_write_coils(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = Self::extract_u16(args, "address")?;

        let values: std::result::Result<Vec<bool>, ExtensionError> = args
            .get("values")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'values' parameter (expected boolean array)".to_string())
            })?
            .iter()
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(format!(
                            "Invalid coil value: {:?} (expected true/false)",
                            v
                        ))
                    })
            })
            .collect();
        let values = values?;

        if values.is_empty() {
            return Err(ExtensionError::InvalidArguments(
                "Values array must not be empty".to_string(),
            ));
        }
        // Modbus protocol limit: max 1968 coils per write request
        if values.len() > 1968 {
            return Err(ExtensionError::InvalidArguments(format!(
                "Cannot write {} coils at once (Modbus limit is 1968). Split into multiple writes.",
                values.len()
            )));
        }
        // Validate address + count doesn't overflow Modbus address space
        if (address as u32) + (values.len() as u32) > 65536 {
            return Err(ExtensionError::InvalidArguments(format!(
                "Address {} + count {} overflows Modbus address space (max 65535)",
                address, values.len()
            )));
        }

        let devices = self.devices.read();
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        device
            .write_coils(address, &values)
            .map_err(ExtensionError::ExecutionFailed)?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "count": values.len(),
            "message": "Coils written successfully"
        }))
    }

    fn cmd_update_polling(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let interval_ms = args
            .get("interval_ms")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'interval_ms' parameter".to_string())
            })?;

        let mut devices = self.devices.write();
        let device = devices.get_mut(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        device.update_poll_interval(interval_ms);

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "poll_interval_ms": interval_ms,
            "message": "Poll interval updated"
        }))
    }

    fn cmd_set_register_map(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let registers_value = args
            .get("registers")
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'registers' parameter".to_string())
            })?;

        let new_registers: Vec<types::RegisterConfig> =
            serde_json::from_value(registers_value.clone()).map_err(|e| {
                ExtensionError::InvalidArguments(format!("Invalid registers config: {}", e))
            })?;

        let register_count = new_registers.len();

        // Remove the old device, stopping its polling thread
        let old_config = {
            let mut devices = self.devices.write();
            if let Some(mut old) = devices.remove(device_id) {
                let state = old.get_state();
                let mut cfg = state.config;
                old.stop();
                cfg.registers = new_registers;
                Some(cfg)
            } else {
                None
            }
        };

        let config = match old_config {
            Some(c) => c,
            None => {
                return Err(ExtensionError::ExecutionFailed(format!(
                    "Device not found: {}",
                    device_id
                )))
            }
        };

        // The registers were already replaced above; create and start fresh
        let mut device = ModbusDevice::new(config);
        device
            .start()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to restart device: {}", e)))?;

        {
            let mut devices = self.devices.write();
            devices.insert(device_id.to_string(), device);
        }

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "register_count": register_count,
            "message": "Register map updated and polling restarted"
        }))
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

neomind_extension_sdk::neomind_export!(ModbusBridgeExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = ModbusBridgeExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "modbus-bridge");
        assert_eq!(meta.name, "Modbus Bridge");
        assert!(meta.description.is_some());
    }

    #[test]
    fn test_extension_metrics() {
        let ext = ModbusBridgeExtension::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 3);
        assert!(metrics.iter().any(|m| m.name == "total_commands"));
        assert!(metrics.iter().any(|m| m.name == "connected_devices"));
        assert!(metrics.iter().any(|m| m.name == "total_poll_errors"));
    }

    #[test]
    fn test_extension_commands() {
        let ext = ModbusBridgeExtension::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 12);

        let command_names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(command_names.contains(&"add_device"));
        assert!(command_names.contains(&"remove_device"));
        assert!(command_names.contains(&"list_devices"));
        assert!(command_names.contains(&"get_device_data"));
        assert!(command_names.contains(&"read_registers"));
        assert!(command_names.contains(&"write_register"));
        assert!(command_names.contains(&"write_registers"));
        assert!(command_names.contains(&"write_coil"));
        assert!(command_names.contains(&"write_coils"));
        assert!(command_names.contains(&"update_polling"));
        assert!(command_names.contains(&"set_register_map"));
        assert!(command_names.contains(&"configure"));
    }

    #[test]
    fn test_produce_metrics_no_devices() {
        let ext = ModbusBridgeExtension::new();
        let metrics = ext.produce_metrics().unwrap();
        // Should have total_commands + connected_devices + total_poll_errors = 3
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
        let mut ext = ModbusBridgeExtension::new();
        let result = ext.configure(&json!({})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_devices_empty() {
        let ext = ModbusBridgeExtension::new();
        let result = ext.execute_command("list_devices", &json!({})).await.unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["count"], 0);
    }

    #[tokio::test]
    async fn test_remove_nonexistent_device() {
        let ext = ModbusBridgeExtension::new();
        let result = ext.execute_command("remove_device", &json!({"device_id": "no-such-device"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_device_data_nonexistent() {
        let ext = ModbusBridgeExtension::new();
        let result = ext.execute_command("get_device_data", &json!({"device_id": "no-such-device"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = ModbusBridgeExtension::new();
        let result = ext.execute_command("nonexistent", &json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_device_config_deserialization() {
        let config_json = json!({
            "device_id": "test-plc",
            "name": "Test PLC",
            "mode": "tcp",
            "ip": "192.168.1.100",
            "port": 502,
            "slave_id": 1,
            "poll_interval_ms": 3000,
            "timeout_ms": 2000,
            "registers": [
                { "address": 0, "name": "temp", "type": "float32", "scale": 0.01, "unit": "°C" },
                { "address": 2, "name": "status", "type": "uint16", "unit": "" }
            ]
        });

        let config: DeviceConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.device_id, "test-plc");
        assert_eq!(config.name, Some("Test PLC".to_string()));
        assert_eq!(config.registers.len(), 2);
        assert_eq!(config.poll_interval_ms, 3000);
    }
}
