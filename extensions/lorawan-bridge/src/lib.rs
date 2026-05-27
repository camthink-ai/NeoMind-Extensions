//! NeoMind LoRaWAN Bridge Extension
//!
//! Connects to a LoRaWAN Network Server (ChirpStack or The Things Network) via
//! MQTT, auto-discovers devices, decodes sensor payloads, and exposes metrics
//! for the NeoMind platform.
//!
//! # Architecture
//!
//! - MQTT uplink listener (async via `rumqttc::AsyncClient` + background task)
//! - Cayenne LPP and custom binary payload decoders
//! - Sync HTTP downlink via `ureq` (avoids Tokio runtime conflicts in cdylib)
//! - Extension SDK FFI exports via `neomind_export!`

mod decoders;
mod ns_client;
mod types;

use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};

use ns_client::NsClient;
use types::{CustomDecoderField, DecoderType, LoRaDevice, NsConfig};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

// ============================================================================
// Extension Struct
// ============================================================================

pub struct LorawanBridgeExtension {
    ns_client: std::sync::RwLock<Option<NsClient>>,
    devices: Arc<tokio::sync::RwLock<HashMap<String, LoRaDevice>>>,
    total_commands: AtomicI64,
    connected: AtomicBool,
}

impl LorawanBridgeExtension {
    pub fn new() -> Self {
        Self {
            ns_client: std::sync::RwLock::new(None),
            devices: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            total_commands: AtomicI64::new(0),
            connected: AtomicBool::new(false),
        }
    }

    // -----------------------------------------------------------------------
    // Command handlers
    // -----------------------------------------------------------------------

    async fn cmd_connect(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let config: NsConfig = serde_json::from_value(args.clone())
            .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid NS config: {}", e)))?;

        // If already connected, disconnect first.
        if self.connected.load(Ordering::SeqCst) {
            self.cmd_disconnect_inner()?;
        }

        let client = NsClient::connect(config.clone(), self.devices.clone())
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        *self.ns_client.write().unwrap() = Some(client);
        self.connected.store(true, Ordering::SeqCst);

        Ok(json!({
            "success": true,
            "message": format!("Connected to {:?} at {}", config.ns_type, config.broker_url),
            "auto_discover": config.auto_discover,
        }))
    }

    async fn cmd_disconnect(&self) -> Result<serde_json::Value> {
        self.cmd_disconnect_inner()?;
        Ok(json!({
            "success": true,
            "message": "Disconnected from Network Server",
        }))
    }

    fn cmd_disconnect_inner(&self) -> Result<()> {
        self.connected.store(false, Ordering::SeqCst);
        let mut guard = self.ns_client.write().unwrap();
        if let Some(client) = guard.take() {
            // try_disconnect is non-blocking; if it fails the background task
            // will terminate on its own via the running flag.
            let rt = tokio::runtime::Handle::try_current();
            if let Ok(handle) = rt {
                handle.block_on(async {
                    let _ = client.disconnect().await;
                });
            }
        }
        Ok(())
    }

    async fn cmd_list_devices(&self) -> Result<serde_json::Value> {
        let map = self.devices.read().await;
        let devices: Vec<&LoRaDevice> = map.values().collect();
        Ok(json!({
            "success": true,
            "count": devices.len(),
            "devices": devices,
        }))
    }

    async fn cmd_get_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let dev_eui = args
            .get("dev_eui")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'dev_eui' parameter".to_string()))?;

        let map = self.devices.read().await;
        let device = map
            .get(dev_eui)
            .ok_or_else(|| ExtensionError::ExecutionFailed(format!("Device {} not found", dev_eui)))?;

        Ok(json!({
            "success": true,
            "device": device,
        }))
    }

    fn cmd_send_downlink(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let dev_eui = args
            .get("dev_eui")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'dev_eui'".to_string()))?;
        let payload_hex = args
            .get("payload_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'payload_hex'".to_string()))?;
        let f_port = args
            .get("f_port")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u8;
        let confirmed = args
            .get("confirmed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let guard = self.ns_client.read().unwrap();
        let client = guard
            .as_ref()
            .ok_or_else(|| ExtensionError::ExecutionFailed("Not connected to a Network Server".to_string()))?;

        let response = client
            .send_downlink(dev_eui, payload_hex, f_port, confirmed)
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        Ok(json!({
            "success": true,
            "response": response,
        }))
    }

    async fn cmd_set_decoder(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let dev_eui = args
            .get("dev_eui")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'dev_eui'".to_string()))?;

        let decoder_type_str = args
            .get("decoder_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'decoder_type'".to_string()))?;

        let decoder_type = match decoder_type_str {
            "cayenne" => DecoderType::Cayenne,
            "custom" => DecoderType::Custom,
            _ => {
                return Err(ExtensionError::InvalidArguments(
                    "decoder_type must be 'cayenne' or 'custom'".to_string(),
                ))
            }
        };

        let custom_decoder: Option<Vec<CustomDecoderField>> = if decoder_type_str == "custom" {
            let fields = args.get("custom_decoder").ok_or_else(|| {
                ExtensionError::InvalidArguments(
                    "Missing 'custom_decoder' fields definition".to_string(),
                )
            })?;
            Some(
                serde_json::from_value(fields.clone())
                    .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid custom_decoder: {}", e)))?,
            )
        } else {
            None
        };

        let mut map = self.devices.write().await;
        let device = map
            .get_mut(dev_eui)
            .ok_or_else(|| ExtensionError::ExecutionFailed(format!("Device {} not found", dev_eui)))?;

        device.decoder_type = decoder_type;
        device.custom_decoder = custom_decoder;

        Ok(json!({
            "success": true,
            "message": format!("Decoder set to {} for device {}", decoder_type_str, dev_eui),
        }))
    }

    async fn cmd_get_status(&self) -> Result<serde_json::Value> {
        let is_connected = self.connected.load(Ordering::SeqCst);
        let device_count = self.devices.read().await.len();
        let total_cmds = self.total_commands.load(Ordering::SeqCst);

        let ns_info = {
            let guard = self.ns_client.read().unwrap();
            guard.as_ref().map(|c| {
                json!({
                    "ns_type": c.config().ns_type,
                    "broker_url": c.config().broker_url,
                    "application_id": c.config().application_id,
                    "auto_discover": c.config().auto_discover,
                })
            })
        };

        Ok(json!({
            "success": true,
            "connected": is_connected,
            "device_count": device_count,
            "total_commands": total_cmds,
            "ns_info": ns_info,
        }))
    }

    async fn cmd_configure(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let _ = args;
        Ok(json!({
            "success": true,
            "message": "Configuration acknowledged. Use 'connect' command to apply new NS settings.",
        }))
    }
}

impl Default for LorawanBridgeExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for LorawanBridgeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "lorawan-bridge",
                "LoRaWAN Bridge",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description(
                "LoRaWAN bridge extension for NeoMind \u{2014} connect ChirpStack/TTN sensors with auto-discovery",
            )
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "ns_type".to_string(),
                    display_name: "Network Server Type".to_string(),
                    description: "Type of LoRaWAN Network Server".to_string(),
                    param_type: MetricDataType::Enum {
                        options: vec!["chirpstack".to_string(), "ttn".to_string()],
                    },
                    required: true,
                    default_value: Some(ParamMetricValue::String("chirpstack".to_string())),
                    min: None,
                    max: None,
                    options: vec!["chirpstack".to_string(), "ttn".to_string()],
                },
                ParameterDefinition {
                    name: "broker_url".to_string(),
                    display_name: "MQTT Broker URL".to_string(),
                    description: "MQTT broker address (e.g. tcp://host:1883)".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "application_id".to_string(),
                    display_name: "Application ID".to_string(),
                    description: "LoRaWAN Application ID".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "default_decoder".to_string(),
                    display_name: "Default Decoder".to_string(),
                    description: "Default payload decoder type".to_string(),
                    param_type: MetricDataType::Enum {
                        options: vec!["cayenne".to_string(), "custom".to_string()],
                    },
                    required: false,
                    default_value: Some(ParamMetricValue::String("cayenne".to_string())),
                    min: None,
                    max: None,
                    options: vec!["cayenne".to_string(), "custom".to_string()],
                },
            ])
        })
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "connect".to_string(),
                display_name: "Connect to NS".to_string(),
                description: "Connect to a LoRaWAN Network Server via MQTT".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "ns_type".to_string(),
                        display_name: "NS Type".to_string(),
                        description: "Network Server type (chirpstack or ttn)".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["chirpstack".to_string(), "ttn".to_string()],
                        },
                        required: true,
                        default_value: Some(ParamMetricValue::String("chirpstack".to_string())),
                        min: None,
                        max: None,
                        options: vec!["chirpstack".to_string(), "ttn".to_string()],
                    },
                    ParameterDefinition {
                        name: "broker_url".to_string(),
                        display_name: "Broker URL".to_string(),
                        description: "MQTT broker URL (e.g. tcp://host:1883)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "username".to_string(),
                        display_name: "Username".to_string(),
                        description: "MQTT username (optional)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "password".to_string(),
                        display_name: "Password".to_string(),
                        description: "MQTT password (optional)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "application_id".to_string(),
                        display_name: "Application ID".to_string(),
                        description: "LoRaWAN Application ID".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "auto_discover".to_string(),
                        display_name: "Auto Discover".to_string(),
                        description: "Automatically discover new devices".to_string(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: Some(ParamMetricValue::Boolean(true)),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({
                    "ns_type": "chirpstack",
                    "broker_url": "tcp://localhost:1883",
                    "application_id": "1",
                    "auto_discover": true
                })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "disconnect".to_string(),
                display_name: "Disconnect".to_string(),
                description: "Disconnect from the Network Server".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_devices".to_string(),
                display_name: "List Devices".to_string(),
                description: "List all discovered LoRa devices".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_device".to_string(),
                display_name: "Get Device".to_string(),
                description: "Get details for a specific LoRa device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "dev_eui".to_string(),
                        display_name: "Device EUI".to_string(),
                        description: "Device EUI (hex string)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "dev_eui": "0102030405060708" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "send_downlink".to_string(),
                display_name: "Send Downlink".to_string(),
                description: "Send a downlink message to a LoRa device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "dev_eui".to_string(),
                        display_name: "Device EUI".to_string(),
                        description: "Device EUI (hex string)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "payload_hex".to_string(),
                        display_name: "Payload (Hex)".to_string(),
                        description: "Downlink payload in hex encoding".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "f_port".to_string(),
                        display_name: "FPort".to_string(),
                        description: "LoRaWAN FPort (1-223)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(1)),
                        min: Some(1.0),
                        max: Some(223.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "confirmed".to_string(),
                        display_name: "Confirmed".to_string(),
                        description: "Send as confirmed downlink".to_string(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: Some(ParamMetricValue::Boolean(false)),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({
                    "dev_eui": "0102030405060708",
                    "payload_hex": "AA55",
                    "f_port": 1,
                    "confirmed": false
                })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "set_decoder".to_string(),
                display_name: "Set Decoder".to_string(),
                description: "Set the payload decoder for a specific device".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "dev_eui".to_string(),
                        display_name: "Device EUI".to_string(),
                        description: "Device EUI (hex string)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "decoder_type".to_string(),
                        display_name: "Decoder Type".to_string(),
                        description: "Decoder type: cayenne or custom".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["cayenne".to_string(), "custom".to_string()],
                        },
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec!["cayenne".to_string(), "custom".to_string()],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({
                    "dev_eui": "0102030405060708",
                    "decoder_type": "cayenne"
                })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".to_string(),
                display_name: "Get Status".to_string(),
                description: "Get the current bridge status".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "configure".to_string(),
                display_name: "Configure".to_string(),
                description: "Update extension configuration".to_string(),
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
            "connect" => self.cmd_connect(args).await,
            "disconnect" => self.cmd_disconnect().await,
            "list_devices" => self.cmd_list_devices().await,
            "get_device" => self.cmd_get_device(args).await,
            "send_downlink" => self.cmd_send_downlink(args),
            "set_decoder" => self.cmd_set_decoder(args).await,
            "get_status" => self.cmd_get_status().await,
            "configure" => self.cmd_configure(args).await,
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

        metrics.push(ExtensionMetricValue {
            name: "connected".to_string(),
            value: ParamMetricValue::Integer(if self.connected.load(Ordering::SeqCst) { 1 } else { 0 }),
            timestamp: now,
        });

        // Try to read devices without blocking. If we cannot acquire the lock
        // immediately, return what we have so far.
        match self.devices.try_read() {
            Ok(map) => {
                metrics.push(ExtensionMetricValue {
                    name: "device_count".to_string(),
                    value: ParamMetricValue::Integer(map.len() as i64),
                    timestamp: now,
                });

                for (dev_eui, device) in map.iter() {
                    // Per-device decoded fields.
                    for field in &device.fields {
                        let safe_name = field.name.replace(' ', "_").replace('.', "_");
                        metrics.push(ExtensionMetricValue {
                            name: format!("lorawan.{}.{}", dev_eui, safe_name),
                            value: ParamMetricValue::Float(field.value),
                            timestamp: now,
                        });
                    }

                    // RSSI per device.
                    metrics.push(ExtensionMetricValue {
                        name: format!("lorawan.{}.rssi", dev_eui),
                        value: ParamMetricValue::Integer(device.rssi as i64),
                        timestamp: now,
                    });

                    // SNR per device.
                    metrics.push(ExtensionMetricValue {
                        name: format!("lorawan.{}.snr", dev_eui),
                        value: ParamMetricValue::Float(device.snr),
                        timestamp: now,
                    });

                    // Frame counter.
                    metrics.push(ExtensionMetricValue {
                        name: format!("lorawan.{}.f_cnt", dev_eui),
                        value: ParamMetricValue::Integer(device.f_cnt as i64),
                        timestamp: now,
                    });

                    // Battery (if available).
                    if let Some(batt) = device.battery {
                        metrics.push(ExtensionMetricValue {
                            name: format!("lorawan.{}.battery", dev_eui),
                            value: ParamMetricValue::Integer(batt as i64),
                            timestamp: now,
                        });
                    }

                    // Last seen timestamp.
                    metrics.push(ExtensionMetricValue {
                        name: format!("lorawan.{}.last_seen", dev_eui),
                        value: ParamMetricValue::Integer(device.last_seen),
                        timestamp: now,
                    });
                }
            }
            Err(_) => {
                // Lock is contended -- skip device metrics this cycle.
                metrics.push(ExtensionMetricValue {
                    name: "device_count".to_string(),
                    value: ParamMetricValue::Integer(-1),
                    timestamp: now,
                });
            }
        }

        Ok(metrics)
    }

    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        // Configuration is applied through the "connect" command.
        // This method accepts the config silently for SDK compatibility.
        let _ = config;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

neomind_extension_sdk::neomind_export!(LorawanBridgeExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = LorawanBridgeExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "lorawan-bridge");
        assert_eq!(meta.name, "LoRaWAN Bridge");
    }

    #[test]
    fn test_extension_commands() {
        let ext = LorawanBridgeExtension::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 8);
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"connect"));
        assert!(names.contains(&"disconnect"));
        assert!(names.contains(&"list_devices"));
        assert!(names.contains(&"get_device"));
        assert!(names.contains(&"send_downlink"));
        assert!(names.contains(&"set_decoder"));
        assert!(names.contains(&"get_status"));
        assert!(names.contains(&"configure"));
    }

    #[test]
    fn test_produce_metrics_initial() {
        let ext = LorawanBridgeExtension::new();
        let metrics = ext.produce_metrics().unwrap();
        // Should have at least total_commands, connected, device_count.
        assert!(metrics.len() >= 3);
        let total_cmd = metrics.iter().find(|m| m.name == "total_commands").unwrap();
        match total_cmd.value {
            ParamMetricValue::Integer(v) => assert_eq!(v, 0),
            _ => panic!("Expected Integer"),
        }
    }

    #[tokio::test]
    async fn test_get_status_disconnected() {
        let ext = LorawanBridgeExtension::new();
        let result = ext.cmd_get_status().await.unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["connected"], false);
        assert_eq!(result["device_count"], 0);
    }

    #[tokio::test]
    async fn test_list_devices_empty() {
        let ext = LorawanBridgeExtension::new();
        let result = ext.cmd_list_devices().await.unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["count"], 0);
    }

    #[tokio::test]
    async fn test_get_device_not_found() {
        let ext = LorawanBridgeExtension::new();
        let result = ext.cmd_get_device(&json!({ "dev_eui": "nonexistent" })).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_broker_host() {
        assert_eq!(ns_client::parse_broker_host("tcp://broker.example.com:1883"), "broker.example.com");
        assert_eq!(ns_client::parse_broker_host("ssl://secure.example.com:8883"), "secure.example.com");
        assert_eq!(ns_client::parse_broker_host("mqtt://plain.example.com:1883"), "plain.example.com");
        assert_eq!(ns_client::parse_broker_host("broker.example.com:1883"), "broker.example.com");
    }

    #[test]
    fn test_parse_broker_port() {
        assert_eq!(ns_client::parse_broker_port("tcp://broker.example.com:1883"), 1883);
        assert_eq!(ns_client::parse_broker_port("ssl://broker.example.com:8883"), 8883);
        assert_eq!(ns_client::parse_broker_port("tcp://broker.example.com"), 1883);
        assert_eq!(ns_client::parse_broker_port("mqtts://broker.example.com"), 8883);
    }

    #[test]
    fn test_base64_to_bytes() {
        let bytes = ns_client::base64_to_bytes("AQID").unwrap();
        assert_eq!(bytes, vec![0x01, 0x02, 0x03]);

        let empty = ns_client::base64_to_bytes("").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_hex_to_base64() {
        let b64 = ns_client::hex_to_base64("010203").unwrap();
        assert_eq!(b64, "AQID");
    }

    #[test]
    fn test_default_extension() {
        let ext = LorawanBridgeExtension::default();
        assert_eq!(ext.metadata().id, "lorawan-bridge");
    }
}
