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
//!
//! # Architecture Note
//!
//! This extension uses **sync Modbus client** (`tokio-modbus` sync features)
//! to avoid Tokio runtime compatibility issues when loaded as a dynamic library
//! (.dylib/.so/.dll). Background polling runs on dedicated std threads.

mod types;
mod register_map;
mod device;

use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::RwLock;

use device::ModbusDevice;
use types::DeviceConfig;

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct ModbusBridgeExtension {
    devices: RwLock<HashMap<String, ModbusDevice>>,
    total_commands: AtomicI64,
}

impl ModbusBridgeExtension {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
            total_commands: AtomicI64::new(0),
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
                            "device_id": "plc-001",
                            "name": "Workshop PLC",
                            "mode": "tcp",
                            "ip": "192.168.1.100",
                            "port": 502,
                            "slave_id": 1,
                            "poll_interval_ms": 5000,
                            "registers": [
                                { "address": 0, "name": "temperature", "type": "float32", "scale": 0.01, "unit": "°C" },
                                { "address": 2, "name": "humidity", "type": "uint16", "unit": "%" }
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
                description: "Read holding registers from a device on-demand".to_string(),
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
                        description: "Starting register address".to_string(),
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
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "device_id": "plc-001", "address": 0, "count": 10 })],
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
            "add_device" => self.cmd_add_device(args).await,
            "remove_device" => self.cmd_remove_device(args).await,
            "list_devices" => self.cmd_list_devices().await,
            "get_device_data" => self.cmd_get_device_data(args).await,
            "read_registers" => self.cmd_read_registers(args).await,
            "write_register" => self.cmd_write_register(args).await,
            "write_coil" => self.cmd_write_coil(args).await,
            "update_polling" => self.cmd_update_polling(args).await,
            "set_register_map" => self.cmd_set_register_map(args).await,
            "configure" => Ok(json!({"status": "ok"})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::new();

        metrics.push(ExtensionMetricValue {
            name: "total_commands".to_string(),
            value: ParamMetricValue::Integer(self.total_commands.load(Ordering::SeqCst)),
            timestamp: now,
        });

        // Access devices synchronously — produce_metrics is called from the
        // metrics thread and must not block. We use try_read to avoid deadlocks;
        // if the lock is contended we just skip device metrics this cycle.
        if let Ok(devices) = self.devices.try_read() {
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
        }

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
// Command Handlers
// ============================================================================

impl ModbusBridgeExtension {
    async fn cmd_add_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_value = args
            .get("device")
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'device' parameter".to_string()))?;

        let config: DeviceConfig = serde_json::from_value(device_value.clone())
            .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid device config: {}", e)))?;

        let device_id = config.device_id.clone();

        // Stop and remove any existing device with the same ID
        {
            let mut devices = self.devices.write().await;
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
            let mut devices = self.devices.write().await;
            devices.insert(device_id.clone(), device);
        }

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "message": "Device added and polling started"
        }))
    }

    async fn cmd_remove_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let mut devices = self.devices.write().await;
        if let Some(mut device) = devices.remove(device_id) {
            device.stop();
            Ok(json!({
                "success": true,
                "device_id": device_id,
                "message": "Device removed and polling stopped"
            }))
        } else {
            Err(ExtensionError::ExecutionFailed(format!(
                "Device not found: {}",
                device_id
            )))
        }
    }

    async fn cmd_list_devices(&self) -> Result<serde_json::Value> {
        let devices = self.devices.read().await;
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

    async fn cmd_get_device_data(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let devices = self.devices.read().await;
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

    async fn cmd_read_registers(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = args
            .get("address")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'address' parameter".to_string())
            })? as u16;

        let count = args
            .get("count")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'count' parameter".to_string())
            })? as u16;

        let devices = self.devices.read().await;
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        let words = device
            .read_registers(address, count)
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "count": count,
            "data": words
        }))
    }

    async fn cmd_write_register(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = args
            .get("address")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'address' parameter".to_string())
            })? as u16;

        let value = args
            .get("value")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'value' parameter".to_string())
            })? as u16;

        let devices = self.devices.read().await;
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        device
            .write_register(address, value)
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "value": value,
            "message": "Register written successfully"
        }))
    }

    async fn cmd_write_coil(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'device_id' parameter".to_string())
            })?;

        let address = args
            .get("address")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'address' parameter".to_string())
            })? as u16;

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

        let devices = self.devices.read().await;
        let device = devices.get(device_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Device not found: {}", device_id))
        })?;

        device
            .write_coil(address, value)
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "address": address,
            "value": value,
            "message": "Coil written successfully"
        }))
    }

    async fn cmd_update_polling(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
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

        let mut devices = self.devices.write().await;
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

    async fn cmd_set_register_map(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
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
            let mut devices = self.devices.write().await;
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
            let mut devices = self.devices.write().await;
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
        assert_eq!(commands.len(), 10);

        let command_names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(command_names.contains(&"add_device"));
        assert!(command_names.contains(&"remove_device"));
        assert!(command_names.contains(&"list_devices"));
        assert!(command_names.contains(&"get_device_data"));
        assert!(command_names.contains(&"read_registers"));
        assert!(command_names.contains(&"write_register"));
        assert!(command_names.contains(&"write_coil"));
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
