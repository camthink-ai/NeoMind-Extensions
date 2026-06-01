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
//! - Device template registration and per-device metrics via CapabilityContext
//! - Extension SDK FFI exports via `neomind_export!`

mod decoders;
mod ns_client;
mod types;

use neomind_extension_sdk::{
    async_trait, json,
    capabilities::CapabilityContext,
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};

use ns_client::NsClient;
use types::{CustomDecoderField, DecoderType, LoRaDevice, NsConfig};

use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

// ============================================================================
// Extension Struct
// ============================================================================

pub struct LorawanBridgeExtension {
    ns_client: std::sync::RwLock<Option<NsClient>>,
    devices: Arc<RwLock<HashMap<String, LoRaDevice>>>,
    registered_devices: RwLock<HashSet<String>>,
    total_commands: AtomicI64,
    connected: AtomicBool,
    template_registered: AtomicI64, // 0 = not registered, 1 = registered
}

impl LorawanBridgeExtension {
    pub fn new() -> Self {
        Self {
            ns_client: std::sync::RwLock::new(None),
            devices: Arc::new(RwLock::new(HashMap::new())),
            registered_devices: RwLock::new(HashSet::new()),
            total_commands: AtomicI64::new(0),
            connected: AtomicBool::new(false),
            template_registered: AtomicI64::new(0),
        }
    }

    // -----------------------------------------------------------------------
    // Device template & registration (CapabilityContext)
    // -----------------------------------------------------------------------

    /// Register the "lorawan_device" template with NeoMind and register all
    /// discovered devices. Called from produce_metrics (sync context).
    fn register_devices_if_needed(&self) {
        let ctx = CapabilityContext::default();

        // Register template once using compare_exchange for thread safety
        if self.template_registered.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            // Already registered by another thread
            // Continue to device registration below
        } else {
            let template_json = json!({
                "device_type": "lorawan_device",
                "name": "LoRaWAN Sensor Device",
                "description": "Auto-discovered LoRaWAN sensor device from ChirpStack/TTN",
                "categories": ["sensor", "lorawan", "iot"],
                "metrics": [
                    { "name": "rssi", "display_name": "RSSI", "data_type": "Integer", "unit": "dBm", "description": "Received Signal Strength Indicator" },
                    { "name": "snr", "display_name": "SNR", "data_type": "Float", "unit": "dB", "description": "Signal-to-Noise Ratio" },
                    { "name": "f_cnt", "display_name": "Frame Counter", "data_type": "Integer", "description": "LoRaWAN uplink frame counter" },
                    { "name": "battery", "display_name": "Battery", "data_type": "Integer", "unit": "%", "min": 0, "max": 100 },
                    { "name": "last_seen", "display_name": "Last Seen", "data_type": "Integer", "description": "Last uplink timestamp (ms)" }
                ],
                "commands": []
            });
            let result = ctx.invoke_capability("device_template_register", &template_json);
            if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                eprintln!("[lorawan-bridge] Device template registered");
                // Already set to 1 by compare_exchange above
            } else {
                let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                eprintln!("[lorawan-bridge] Template registration failed: {}", err);
                // Reset to 0 so it will retry next time
                self.template_registered.store(0, Ordering::SeqCst);
                return;
            }
        }

        // Register each discovered device and write per-device metrics
        let devices = self.devices.read();
        let mut registered = self.registered_devices.write();
        for (dev_eui, device) in devices.iter() {
            let neo_device_id = format!("lorawan-{}", dev_eui);

            // Only register device if not already registered
            if !registered.contains(dev_eui) {
                let device_json = json!({
                    "device_id": neo_device_id,
                    "name": format!("LoRa Device {}", dev_eui),
                    "device_type": "lorawan_device",
                });
                let _ = ctx.invoke_capability("device_register", &device_json);
                registered.insert(dev_eui.clone());
            }

            // Always write per-device metrics (values may have changed)
            let now_ms = chrono::Utc::now().timestamp_millis();

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": neo_device_id,
                "metric": "rssi",
                "value": device.rssi,
                "timestamp": now_ms,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": neo_device_id,
                "metric": "snr",
                "value": device.snr,
                "timestamp": now_ms,
            }));

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": neo_device_id,
                "metric": "f_cnt",
                "value": device.f_cnt,
                "timestamp": now_ms,
            }));

            if let Some(batt) = device.battery {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id,
                    "metric": "battery",
                    "value": batt,
                    "timestamp": now_ms,
                }));
            }

            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": neo_device_id,
                "metric": "last_seen",
                "value": device.last_seen,
                "timestamp": now_ms,
            }));

            // Write each decoded sensor field as a device metric
            for field in &device.fields {
                let safe_name = field.name.replace([' ', '.'], "_");
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id,
                    "metric": safe_name,
                    "value": field.value,
                    "timestamp": now_ms,
                }));
            }
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
            .map_err(ExtensionError::ExecutionFailed)?;

        *self
            .ns_client
            .write()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Lock poisoned: {}", e)))? = Some(client);
        self.connected.store(true, Ordering::SeqCst);

        // Reset template registration so devices get re-registered
        self.template_registered.store(0, Ordering::SeqCst);

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
        self.registered_devices.write().clear();
        let mut guard = self
            .ns_client
            .write()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
        if let Some(client) = guard.take() {
            // Use handle.spawn() instead of block_on() to avoid panicking
            // inside an async context. The disconnect will complete in the
            // background; the running flag already stops the event loop.
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = client.disconnect().await;
                });
            }
        }
        Ok(())
    }

    fn cmd_list_devices(&self) -> Result<serde_json::Value> {
        let map = self.devices.read();
        let devices: Vec<&LoRaDevice> = map.values().collect();
        Ok(json!({
            "success": true,
            "count": devices.len(),
            "devices": devices,
        }))
    }

    fn cmd_get_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let dev_eui = args
            .get("dev_eui")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'dev_eui' parameter".to_string()))?;

        let map = self.devices.read();
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

        // LoRaWAN spec: FPort 0 is reserved for MAC commands,
        // 224+ reserved for future use / test protocols.
        // Only FPort 1-223 is valid for application downlinks.
        if f_port == 0 || f_port > 223 {
            return Err(ExtensionError::InvalidArguments(format!(
                "fPort must be 1-223 for application data, got {} (0=MAC commands, 224+=reserved)",
                f_port
            )));
        }
        let confirmed = args
            .get("confirmed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // For TTN, device_id differs from dev_eui. Accept it from args or
        // look it up from discovered devices.
        let device_id = args
            .get("device_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| {
                // If the device was auto-discovered, its dev_eui field IS
                // the TTN device_id (we store TTN device_id in dev_eui).
                let map = self.devices.read();
                map.get(dev_eui).map(|d| d.dev_eui.clone())
            });

        let guard = self
            .ns_client
            .read()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Lock poisoned: {}", e)))?;
        let client = guard
            .as_ref()
            .ok_or_else(|| ExtensionError::ExecutionFailed("Not connected to a Network Server".to_string()))?;

        let response = client
            .send_downlink(dev_eui, device_id.as_deref(), payload_hex, f_port, confirmed)
            .map_err(ExtensionError::ExecutionFailed)?;

        Ok(json!({
            "success": true,
            "response": response,
        }))
    }

    fn cmd_set_decoder(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
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

        let mut map = self.devices.write();
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

    fn cmd_remove_device(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let dev_eui = args
            .get("dev_eui")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'dev_eui'".to_string()))?;

        let mut map = self.devices.write();
        map.remove(dev_eui)
            .ok_or_else(|| ExtensionError::ExecutionFailed(format!("Device {} not found", dev_eui)))?;

        // Unregister from NeoMind
        let neo_device_id = format!("lorawan-{}", dev_eui);
        let ctx = CapabilityContext::default();
        let _ = ctx.invoke_capability("device_unregister", &json!({
            "device_id": neo_device_id,
        }));

        // Remove from registered set
        self.registered_devices.write().remove(dev_eui);

        Ok(json!({
            "success": true,
            "message": format!("Device {} removed", dev_eui),
        }))
    }

    fn cmd_get_status(&self) -> Result<serde_json::Value> {
        let is_connected = self.connected.load(Ordering::SeqCst);
        let device_count = self.devices.read().len();
        let total_cmds = self.total_commands.load(Ordering::SeqCst);

        let ns_info = {
            let guard = match self.ns_client.read() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("[lorawan-bridge] Lock poisoned in get_status: {}", e);
                    return Ok(json!({
                        "success": true,
                        "connected": is_connected,
                        "device_count": device_count,
                        "total_commands": total_cmds,
                        "ns_info": null,
                    }));
                }
            };
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

    fn cmd_configure(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
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
                        options: vec!["chirpstack".to_string(), "chirpstack_v4".to_string(), "ttn".to_string()],
                    },
                    required: true,
                    default_value: Some(ParamMetricValue::String("chirpstack".to_string())),
                    min: None,
                    max: None,
                    options: vec!["chirpstack".to_string(), "chirpstack_v4".to_string(), "ttn".to_string()],
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
                        description: "Network Server type (chirpstack, chirpstack_v4, or ttn)".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["chirpstack".to_string(), "chirpstack_v4".to_string(), "ttn".to_string()],
                        },
                        required: true,
                        default_value: Some(ParamMetricValue::String("chirpstack".to_string())),
                        min: None,
                        max: None,
                        options: vec!["chirpstack".to_string(), "chirpstack_v4".to_string(), "ttn".to_string()],
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
                    ParameterDefinition {
                        name: "tenant_id".to_string(),
                        display_name: "Tenant ID".to_string(),
                        description: "TTN tenant ID (defaults to 'ttn' if not set)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "ns_api_url".to_string(),
                        display_name: "NS API URL".to_string(),
                        description: "NS REST/gRPC API URL for downlink (required for send_downlink, e.g. http://chirpstack:8080)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({
                        "ns_type": "chirpstack",
                        "broker_url": "tcp://localhost:1883",
                        "application_id": "1",
                        "auto_discover": true
                    }),
                    json!({
                        "ns_type": "ttn",
                        "broker_url": "mqtts://eu1.cloud.thethings.network:8883",
                        "username": "myapp@tenant1",
                        "password": "NNSXS.XXX",
                        "application_id": "myapp",
                        "tenant_id": "tenant1",
                        "ns_api_url": "https://eu1.cloud.thethings.network",
                        "auto_discover": true
                    }),
                ],
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
                        name: "device_id".to_string(),
                        display_name: "Device ID (TTN)".to_string(),
                        description: "TTN device ID (only needed for TTN if different from dev_eui)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
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
                        description: "Decoder type: cayenne (Cayenne LPP) or custom (binary field mapping)".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["cayenne".to_string(), "custom".to_string()],
                        },
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec!["cayenne".to_string(), "custom".to_string()],
                    },
                    ParameterDefinition {
                        name: "custom_decoder".to_string(),
                        display_name: "Custom Decoder Fields".to_string(),
                        description: "Field definitions for custom decoder. Each field: {offset, length, name, type (uint8/uint16/int16/uint32/int32), scale, unit}".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({
                        "dev_eui": "0102030405060708",
                        "decoder_type": "cayenne"
                    }),
                    json!({
                        "dev_eui": "0102030405060708",
                        "decoder_type": "custom",
                        "custom_decoder": [
                            {"offset": 0, "length": 2, "name": "temperature", "type": "uint16", "scale": 0.1, "unit": "\u{00b0}C"},
                            {"offset": 2, "length": 1, "name": "humidity", "type": "uint8", "scale": 1.0, "unit": "%"}
                        ]
                    }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "remove_device".to_string(),
                display_name: "Remove Device".to_string(),
                description: "Remove a discovered LoRa device".to_string(),
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
            "list_devices" => self.cmd_list_devices(),
            "get_device" => self.cmd_get_device(args),
            "send_downlink" => self.cmd_send_downlink(args),
            "set_decoder" => self.cmd_set_decoder(args),
            "remove_device" => self.cmd_remove_device(args),
            "get_status" => self.cmd_get_status(),
            "configure" => self.cmd_configure(args),
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

        // Register devices and write per-device metrics via CapabilityContext
        // when connected and devices exist
        if self.connected.load(Ordering::SeqCst) {
            let device_count = self.devices.read().len();
            if device_count > 0 {
                self.register_devices_if_needed();
            }
        }

        // Extension-level metrics from device data (for backward compatibility)
        let map = self.devices.read();
        metrics.push(ExtensionMetricValue {
            name: "device_count".to_string(),
            value: ParamMetricValue::Integer(map.len() as i64),
            timestamp: now,
        });

        for (dev_eui, device) in map.iter() {
            // Per-device decoded fields.
            for field in &device.fields {
                let safe_name = field.name.replace([' ', '.'], "_");
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
        assert_eq!(commands.len(), 9);
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

    #[test]
    fn test_get_status_disconnected() {
        let ext = LorawanBridgeExtension::new();
        let result = ext.cmd_get_status().unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["connected"], false);
        assert_eq!(result["device_count"], 0);
    }

    #[test]
    fn test_list_devices_empty() {
        let ext = LorawanBridgeExtension::new();
        let result = ext.cmd_list_devices().unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["count"], 0);
    }

    #[test]
    fn test_get_device_not_found() {
        let ext = LorawanBridgeExtension::new();
        let result = ext.cmd_get_device(&json!({ "dev_eui": "nonexistent" }));
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

    #[test]
    fn test_template_registered_initial() {
        let ext = LorawanBridgeExtension::new();
        assert_eq!(ext.template_registered.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_set_and_get_device() {
        let ext = LorawanBridgeExtension::new();

        // Manually insert a device
        let device = LoRaDevice {
            dev_eui: "0102030405060708".to_string(),
            fields: vec![],
            rssi: -57,
            snr: 8.2,
            battery: Some(85),
            f_cnt: 42,
            f_port: 2,
            last_seen: 1700000000000,
            decoder_type: DecoderType::Cayenne,
            custom_decoder: None,
        };
        ext.devices.write().insert("0102030405060708".to_string(), device);

        let result = ext.cmd_get_device(&json!({ "dev_eui": "0102030405060708" })).unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["device"]["dev_eui"], "0102030405060708");
        assert_eq!(result["device"]["rssi"], -57);
        assert_eq!(result["device"]["battery"], 85);

        let list = ext.cmd_list_devices().unwrap();
        assert_eq!(list["count"], 1);
    }

    #[test]
    fn test_set_decoder_on_device() {
        let ext = LorawanBridgeExtension::new();

        let device = LoRaDevice {
            dev_eui: "ABCDEF1234567890".to_string(),
            fields: vec![],
            rssi: 0,
            snr: 0.0,
            battery: None,
            f_cnt: 0,
            f_port: 0,
            last_seen: 0,
            decoder_type: DecoderType::Cayenne,
            custom_decoder: None,
        };
        ext.devices.write().insert("ABCDEF1234567890".to_string(), device);

        let result = ext.cmd_set_decoder(&json!({
            "dev_eui": "ABCDEF1234567890",
            "decoder_type": "custom",
            "custom_decoder": [
                { "offset": 0, "length": 2, "name": "temp", "type": "uint16", "scale": 0.1, "unit": "C" }
            ]
        })).unwrap();

        assert_eq!(result["success"], true);

        let dev = ext.devices.read().get("ABCDEF1234567890").cloned().unwrap();
        assert!(matches!(dev.decoder_type, DecoderType::Custom));
        assert!(dev.custom_decoder.is_some());
        assert_eq!(dev.custom_decoder.unwrap().len(), 1);
    }
}
