//! NeoMind Home Assistant Bridge Extension
//!
//! Bridges Home Assistant into NeoMind, importing 3000+ HA entity integrations
//! as devices. Supports real-time state updates via WebSocket and command
//! execution (turn on/off lights, switches, climate, locks, etc.).
//!
//! # Architecture
//!
//! Uses sync HTTP client (ureq) to avoid Tokio runtime issues in dynamic libraries.
//! parking_lot::RwLock is used for the shared entity map (non-async, no .await needed).
//! Device template and device registration use CapabilityContext.invoke_capability().

mod types;
mod rest_client;
mod ws_client;

use neomind_extension_sdk::{
    async_trait, json,
    CapabilityContext, Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;

use types::{HaConfig, HaEntity, entity_matches_patterns};

// ============================================================================
// Extension Struct
// ============================================================================

pub struct HomeAssistantBridgeExtension {
    rest_client: RwLock<Option<rest_client::HaRestClient>>,
    entities: Arc<RwLock<HashMap<String, HaEntity>>>,
    running: Arc<AtomicBool>,
    config: RwLock<Option<HaConfig>>,
    total_commands: AtomicI64,
    connected: Arc<AtomicBool>,
    /// 0 = template not registered, 1 = registered
    template_registered: AtomicI64,
    /// Timestamp (epoch seconds) of last full REST resync
    last_full_sync: AtomicI64,
    /// Set of entity IDs already registered with NeoMind (avoids repeated device_register calls)
    registered_entities: RwLock<HashSet<String>>,
    /// Shared REST client for WS reconnection resync
    ws_rest_client: Arc<RwLock<Option<rest_client::HaRestClient>>>,
    /// Handle for the WebSocket background task (to abort old one on reconnect)
    ws_task_handle: RwLock<Option<JoinHandle<()>>>,
}

impl HomeAssistantBridgeExtension {
    pub fn new() -> Self {
        Self {
            rest_client: RwLock::new(None),
            entities: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
            config: RwLock::new(None),
            total_commands: AtomicI64::new(0),
            connected: Arc::new(AtomicBool::new(false)),
            template_registered: AtomicI64::new(0),
            last_full_sync: AtomicI64::new(0),
            registered_entities: RwLock::new(HashSet::new()),
            ws_rest_client: Arc::new(RwLock::new(None)),
            ws_task_handle: RwLock::new(None),
        }
    }
}

impl Default for HomeAssistantBridgeExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for HomeAssistantBridgeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "homeassistant-bridge",
                "Home Assistant Bridge",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description(
                "Bridge Home Assistant into NeoMind — import 3000+ HA entity integrations as devices",
            )
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "haUrl".to_string(),
                    display_name: "Home Assistant URL".to_string(),
                    description: "URL of your Home Assistant instance (e.g. http://192.168.1.10:8123)".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "token".to_string(),
                    display_name: "Long-Lived Access Token".to_string(),
                    description: "Home Assistant long-lived access token (generated in HA Profile > Security)".to_string(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "domains".to_string(),
                    display_name: "Entity Domains".to_string(),
                    description: "Comma-separated list of HA domains to import (e.g. sensor,light,switch)".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("sensor,light,switch,climate,lock".to_string())),
                    min: None,
                    max: None,
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "syncInterval".to_string(),
                    display_name: "Sync Interval (seconds)".to_string(),
                    description: "How often to sync full state from HA REST API".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(30)),
                    min: Some(5.0),
                    max: Some(3600.0),
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "entityPatterns".to_string(),
                    display_name: "Entity Patterns".to_string(),
                    description: "Comma-separated glob patterns to filter entities (e.g. sensor.living_room*,light.*)".to_string(),
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
                name: "ha.connection".to_string(),
                display_name: "HA Connection Status".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: Some(0.0),
                max: Some(1.0),
                required: false,
            },
            MetricDescriptor {
                name: "ha.entities_count".to_string(),
                display_name: "HA Entities Count".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "ha.total_commands".to_string(),
                display_name: "HA Total Commands".to_string(),
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
                name: "connect".to_string(),
                display_name: "Connect to HA".to_string(),
                description: "Connect to a Home Assistant instance and sync entities".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "haUrl".to_string(),
                        display_name: "HA URL".to_string(),
                        description: "Home Assistant URL".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "token".to_string(),
                        display_name: "Access Token".to_string(),
                        description: "Long-lived access token".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "domains".to_string(),
                        display_name: "Domains".to_string(),
                        description: "Comma-separated domains to import".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String("sensor,light,switch,climate,lock".to_string())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "syncInterval".to_string(),
                        display_name: "Sync Interval".to_string(),
                        description: "Full sync interval in seconds".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(30)),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "entityPatterns".to_string(),
                        display_name: "Entity Patterns".to_string(),
                        description: "Comma-separated glob patterns to filter entities".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({
                    "haUrl": "http://192.168.1.10:8123",
                    "token": "your-long-lived-access-token",
                    "domains": "sensor,light,switch",
                    "syncInterval": 30
                })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "disconnect".to_string(),
                display_name: "Disconnect from HA".to_string(),
                description: "Disconnect from Home Assistant and clear entity cache".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_entities".to_string(),
                display_name: "List Entities".to_string(),
                description: "List all imported HA entities, optionally filtered by domain".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "domain".to_string(),
                        display_name: "Domain Filter".to_string(),
                        description: "Filter by domain (e.g. sensor, light)".to_string(),
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
                    json!({}),
                    json!({ "domain": "sensor" }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_state".to_string(),
                display_name: "Get Entity State".to_string(),
                description: "Get the current state of a specific HA entity".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "entityId".to_string(),
                        display_name: "Entity ID".to_string(),
                        description: "Full entity ID (e.g. sensor.temperature)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "entityId": "sensor.living_room_temperature" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "call_service".to_string(),
                display_name: "Call Service".to_string(),
                description: "Call a Home Assistant service (e.g. turn_on, turn_off, toggle)".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "domain".to_string(),
                        display_name: "Service Domain".to_string(),
                        description: "Domain of the service (e.g. light, switch, climate)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "service".to_string(),
                        display_name: "Service Name".to_string(),
                        description: "Service to call (e.g. turn_on, turn_off, toggle)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "entityId".to_string(),
                        display_name: "Entity ID".to_string(),
                        description: "Target entity ID".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "data".to_string(),
                        display_name: "Service Data".to_string(),
                        description: "Additional service data as JSON object".to_string(),
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
                    json!({ "domain": "light", "service": "turn_on", "entityId": "light.living_room" }),
                    json!({ "domain": "switch", "service": "toggle", "entityId": "switch.kitchen" }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "set_filters".to_string(),
                display_name: "Set Domain Filters".to_string(),
                description: "Update the domain filters and re-sync entities".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "domains".to_string(),
                        display_name: "Domains".to_string(),
                        description: "Comma-separated list of domains to import".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "domains": "sensor,light,switch" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".to_string(),
                display_name: "Get Status".to_string(),
                description: "Get current connection status and entity statistics".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_areas".to_string(),
                display_name: "Get Areas".to_string(),
                description: "List all areas configured in Home Assistant".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "refresh".to_string(),
                display_name: "Refresh Entities".to_string(),
                description: "Manually trigger a full entity resync from Home Assistant (no reconnect needed)".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "configure".to_string(),
                display_name: "Configure".to_string(),
                description: "Apply configuration changes".to_string(),
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
            "list_entities" => self.cmd_list_entities(args).await,
            "get_state" => self.cmd_get_state(args).await,
            "call_service" => self.cmd_call_service(args).await,
            "set_filters" => self.cmd_set_filters(args).await,
            "get_status" => self.cmd_get_status().await,
            "get_areas" => self.cmd_get_areas().await,
            "refresh" => self.cmd_refresh().await,
            "configure" => Ok(json!({"status": "ok", "message": "Configuration saved. Run 'connect' command to establish connection and sync entities."})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::new();

        // Auto-sync: register template and periodically resync via REST
        let configured = self.config.read().is_some();
        if configured {
            let now_secs = chrono::Utc::now().timestamp();
            let sync_interval = self.config.read().as_ref().map(|c| c.sync_interval as i64).unwrap_or(30);
            let should_sync = self.template_registered.load(Ordering::SeqCst) == 0
                || (now_secs - self.last_full_sync.load(Ordering::SeqCst)) >= sync_interval;

            if should_sync {
                if let Err(e) = self.auto_sync() {
                    eprintln!("[ha-bridge] Auto-sync failed: {}", e);
                }
            }
        }

        // Connection status
        metrics.push(ExtensionMetricValue {
            name: "ha.connection".to_string(),
            value: ParamMetricValue::Integer(if self.connected.load(Ordering::SeqCst) {
                1
            } else {
                0
            }),
            timestamp: now,
        });

        // Entity count
        let entity_count = self.entities.read().len();
        metrics.push(ExtensionMetricValue {
            name: "ha.entities_count".to_string(),
            value: ParamMetricValue::Integer(entity_count as i64),
            timestamp: now,
        });

        // Total commands
        metrics.push(ExtensionMetricValue {
            name: "ha.total_commands".to_string(),
            value: ParamMetricValue::Integer(self.total_commands.load(Ordering::SeqCst)),
            timestamp: now,
        });

        // Per-entity metrics: ha.{entity_id}.value, ha.{entity_id}.battery
        let entities = self.entities.read();
        for (entity_id, entity) in entities.iter() {
            // Sanitize entity_id for metric name: replace dots and hyphens with underscores
            let safe_id = entity_id.replace(['.', '-'], "_");

            // Numeric value metric (if available)
            if let Some(val) = entity.value {
                metrics.push(ExtensionMetricValue {
                    name: format!("ha.{}.value", safe_id),
                    value: ParamMetricValue::Float(val),
                    timestamp: now,
                });
            }

            // Battery metric (if available)
            if let Some(battery) = entity.battery {
                metrics.push(ExtensionMetricValue {
                    name: format!("ha.{}.battery", safe_id),
                    value: ParamMetricValue::Integer(battery as i64),
                    timestamp: now,
                });
            }
        }

        Ok(metrics)
    }

    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        // Store config for later use, but do not connect automatically
        if config.get("haUrl").and_then(|v| v.as_str()).is_some()
            && config.get("token").and_then(|v| v.as_str()).is_some()
        {
            let domains_str = config
                .get("domains")
                .and_then(|v| v.as_str())
                .unwrap_or("sensor,light,switch,climate,lock");
            let domains: Vec<String> = domains_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let sync_interval = config
                .get("syncInterval")
                .and_then(|v| v.as_u64())
                .unwrap_or(30);

            let patterns_str = config
                .get("entityPatterns")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let entity_patterns: Vec<String> = patterns_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let ha_config = HaConfig {
                ha_url: config
                    .get("haUrl")
                    .and_then(|v| v.as_str())
                    .unwrap()
                    .to_string(),
                token: config
                    .get("token")
                    .and_then(|v| v.as_str())
                    .unwrap()
                    .to_string(),
                domains,
                sync_interval,
                entity_patterns,
            };

            *self.config.write() = Some(ha_config);
            // Reset template_registered so auto_sync runs with new config
            self.template_registered.store(0, Ordering::SeqCst);
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Auto-Sync (called from produce_metrics, synchronous context)
// ============================================================================

impl HomeAssistantBridgeExtension {
    /// Auto-sync: register device template (once) and fetch & register devices.
    /// Called from produce_metrics() in a synchronous context.
    fn auto_sync(&self) -> Result<()> {
        let ctx = CapabilityContext::default();

        // Register device template once using compare_exchange for thread safety
        if self.template_registered.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            let template_json = json!({
                "device_type": "ha_entity",
                "name": "Home Assistant Entity",
                "description": "Entity synced from Home Assistant",
                "categories": ["smart-home", "home-assistant"],
                "metrics": [
                    { "name": "state", "display_name": "State", "data_type": "String" },
                    { "name": "friendly_name", "display_name": "Friendly Name", "data_type": "String" },
                    { "name": "domain", "display_name": "Domain", "data_type": "String" },
                    { "name": "area", "display_name": "Area", "data_type": "String" },
                    { "name": "last_changed", "display_name": "Last Changed", "data_type": "String" },
                    { "name": "value", "display_name": "Numeric Value", "data_type": "Float" },
                    { "name": "unit", "display_name": "Unit", "data_type": "String" },
                    { "name": "battery", "display_name": "Battery", "data_type": "Integer" }
                ]
            });
            let result = ctx.invoke_capability("device_template_register", &template_json);
            if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                eprintln!("[ha-bridge] Template registered");
                // Already set to 1 by compare_exchange above
            } else {
                let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                eprintln!("[ha-bridge] Template registration failed: {}", err);
                // Reset to 0 so it will retry next time
                self.template_registered.store(0, Ordering::SeqCst);
                return Err(ExtensionError::ExecutionFailed(format!("Template registration failed: {}", err)));
            }
        }

        // Periodic REST resync: fetch all entities via REST and update the map + device metrics
        let (domains, entity_patterns) = {
            let cfg = self.config.read();
            match cfg.as_ref() {
                Some(c) => (c.domains.clone(), c.entity_patterns.clone()),
                None => return Ok(()),
            }
        };

        let client_guard = self.rest_client.read();
        if let Some(ref client) = *client_guard {
            match client.get_all_states(&domains) {
                Ok(fetched) => {
                    let now = chrono::Utc::now().timestamp_millis();
                    let mut ent_map = self.entities.write();
                    let mut reg_set = self.registered_entities.write();
                    let mut registered = 0i64;

                    for entity in &fetched {
                        // Apply entity_patterns filter
                        if !entity_matches_patterns(&entity.entity_id, &entity_patterns) {
                            continue;
                        }

                        ent_map.insert(entity.entity_id.clone(), entity.clone());

                        // Register as a NeoMind device (only if not already registered)
                        let safe_id = entity.entity_id.replace(['.', '-'], "_");
                        let neo_device_id = format!("ha_{}", safe_id);

                        if !reg_set.contains(&entity.entity_id) {
                            let device_json = json!({
                                "device_id": neo_device_id,
                                "name": entity.name,
                                "device_type": "ha_entity",
                            });
                            let _ = ctx.invoke_capability("device_register", &device_json);
                            reg_set.insert(entity.entity_id.clone());
                        }

                        // Write entity state metrics to the NeoMind device
                        let _ = ctx.invoke_capability("device_metrics_write", &json!({
                            "device_id": neo_device_id,
                            "metric": "state",
                            "value": entity.state,
                            "timestamp": now,
                        }));

                        let _ = ctx.invoke_capability("device_metrics_write", &json!({
                            "device_id": neo_device_id,
                            "metric": "friendly_name",
                            "value": entity.name,
                            "timestamp": now,
                        }));

                        let _ = ctx.invoke_capability("device_metrics_write", &json!({
                            "device_id": neo_device_id,
                            "metric": "domain",
                            "value": entity.domain,
                            "timestamp": now,
                        }));

                        let _ = ctx.invoke_capability("device_metrics_write", &json!({
                            "device_id": neo_device_id,
                            "metric": "last_changed",
                            "value": entity.last_changed,
                            "timestamp": now,
                        }));

                        if let Some(val) = entity.value {
                            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                                "device_id": neo_device_id,
                                "metric": "value",
                                "value": val,
                                "timestamp": now,
                            }));
                        }

                        if let Some(ref unit) = entity.unit {
                            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                                "device_id": neo_device_id,
                                "metric": "unit",
                                "value": unit,
                                "timestamp": now,
                            }));
                        }

                        if let Some(battery) = entity.battery {
                            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                                "device_id": neo_device_id,
                                "metric": "battery",
                                "value": battery,
                                "timestamp": now,
                            }));
                        }

                        registered += 1;
                    }

                    self.last_full_sync.store(chrono::Utc::now().timestamp(), Ordering::SeqCst);
                    eprintln!("[ha-bridge] REST resync completed, {} entities synced", registered);
                }
                Err(e) => {
                    eprintln!("[ha-bridge] REST resync failed: {}", e);
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Command Implementations
// ============================================================================

impl HomeAssistantBridgeExtension {
    /// Connect to Home Assistant: test connection, fetch initial state,
    /// and spawn WebSocket listener for real-time updates.
    async fn cmd_connect(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        // Disconnect first if already connected
        if self.connected.load(Ordering::SeqCst) {
            self.cmd_disconnect().await?;
        }

        let ha_url = args
            .get("haUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'haUrl' parameter".to_string())
            })?
            .to_string();

        let token = args
            .get("token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'token' parameter".to_string())
            })?
            .to_string();

        let domains_str = args
            .get("domains")
            .and_then(|v| v.as_str())
            .unwrap_or("sensor,light,switch,climate,lock");
        let domains: Vec<String> = domains_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let sync_interval = args
            .get("syncInterval")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let patterns_str = args
            .get("entityPatterns")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let entity_patterns: Vec<String> = patterns_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let ha_config = HaConfig {
            ha_url: ha_url.clone(),
            token: token.clone(),
            domains: domains.clone(),
            sync_interval,
            entity_patterns: entity_patterns.clone(),
        };

        // 1. Create REST client and test connection
        let client = rest_client::HaRestClient::new(&ha_config);
        let msg = client
            .test_connection()
            .map_err(ExtensionError::ExecutionFailed)?;

        // 2. Fetch all states and populate entity map
        let mut entities = client
            .get_all_states(&domains)
            .map_err(ExtensionError::ExecutionFailed)?;

        // Apply entity_patterns filter
        if !entity_patterns.is_empty() {
            entities.retain(|e| entity_matches_patterns(&e.entity_id, &entity_patterns));
        }

        let count = entities.len();
        let mut ent_map = HashMap::new();
        for e in entities {
            ent_map.insert(e.entity_id.clone(), e);
        }

        {
            let mut ents = self.entities.write();
            *ents = ent_map;
        }

        // 3. Store config and client
        *self.config.write() = Some(ha_config);
        *self.rest_client.write() = Some(client);
        // Reset template flag so auto_sync registers template with new connection
        self.template_registered.store(0, Ordering::SeqCst);
        self.last_full_sync.store(chrono::Utc::now().timestamp(), Ordering::SeqCst);

        // 4. Spawn WebSocket read loop on the host Tokio runtime
        self.running.store(true, Ordering::SeqCst);
        self.connected.store(true, Ordering::SeqCst);

        let ws_running = self.running.clone();
        let ws_entities = self.entities.clone();
        let ws_connected = self.connected.clone();
        let ws_url = ha_url.clone();
        let ws_token = token.clone();
        let ws_domains = domains.clone();
        let ws_rest_client = self.ws_rest_client.clone();

        // Copy REST client for WS reconnection resync
        {
            let rest_guard = self.rest_client.read();
            *ws_rest_client.write() = rest_guard.clone();
        }

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                // Abort any previous WebSocket task and store new one atomically
                let mut task_guard = self.ws_task_handle.write();
                if let Some(old_handle) = task_guard.take() {
                    old_handle.abort();
                }

                let task = handle.spawn(async move {
                    ws_client::run_ws_loop(ws_url, ws_token, ws_domains, ws_entities, ws_rest_client, ws_running, ws_connected)
                        .await;
                });
                *task_guard = Some(task);
            }
            Err(_) => {
                // No Tokio runtime available - WebSocket will not run,
                // but REST-based operation is still functional
            }
        }

        Ok(json!({
            "success": true,
            "message": msg,
            "entities_synced": count,
            "domains": domains,
        }))
    }

    /// Disconnect from Home Assistant and clear state.
    async fn cmd_disconnect(&self) -> Result<serde_json::Value> {
        self.running.store(false, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);

        // Abort the WebSocket task
        if let Some(handle) = self.ws_task_handle.write().take() {
            handle.abort();
        }

        // Give the WS loop a moment to notice the flag
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        {
            let mut ents = self.entities.write();
            ents.clear();
        }
        self.registered_entities.write().clear();

        *self.rest_client.write() = None;
        *self.ws_rest_client.write() = None;
        *self.config.write() = None;
        self.template_registered.store(0, Ordering::SeqCst);
        self.last_full_sync.store(0, Ordering::SeqCst);

        Ok(json!({
            "success": true,
            "message": "Disconnected from Home Assistant"
        }))
    }

    /// List all imported entities, optionally filtered by domain.
    async fn cmd_list_entities(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let domain_filter = args.get("domain").and_then(|v| v.as_str());

        let entities = self.entities.read();
        let mut result: Vec<&HaEntity> = Vec::new();

        for entity in entities.values() {
            if let Some(filter) = domain_filter {
                if entity.domain != filter {
                    continue;
                }
            }
            result.push(entity);
        }

        // Sort by entity_id for consistent output
        result.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));

        Ok(json!({
            "success": true,
            "count": result.len(),
            "entities": result,
        }))
    }

    /// Get the current state of a specific entity.
    /// Tries the local cache first, then falls back to a REST API call.
    async fn cmd_get_state(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let entity_id = args
            .get("entityId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'entityId' parameter".to_string())
            })?;

        // Try cache first
        {
            let entities = self.entities.read();
            if let Some(entity) = entities.get(entity_id) {
                return Ok(json!({
                    "success": true,
                    "source": "cache",
                    "entity": entity,
                }));
            }
        }

        // Fall back to REST API
        let entity_result = {
            let client_guard = self.rest_client.read();
            (*client_guard).as_ref().map(|client| client.get_state(entity_id))
        }; // client_guard dropped here

        match entity_result {
            Some(Ok(entity)) => {
                let mut entities = self.entities.write();
                entities.insert(entity_id.to_string(), entity.clone());

                Ok(json!({
                    "success": true,
                    "source": "api",
                    "entity": entity,
                }))
            }
            Some(Err(e)) => Err(ExtensionError::ExecutionFailed(e)),
            None => Err(ExtensionError::ExecutionFailed(
                "Not connected to Home Assistant".to_string(),
            )),
        }
    }

    /// Call a Home Assistant service.
    async fn cmd_call_service(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let domain = args
            .get("domain")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'domain' parameter".to_string())
            })?;

        let service = args
            .get("service")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'service' parameter".to_string())
            })?;

        let entity_id = args
            .get("entityId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'entityId' parameter".to_string())
            })?;

        // Build service data payload
        let mut data = serde_json::Map::new();
        data.insert(
            "entity_id".to_string(),
            serde_json::Value::String(entity_id.to_string()),
        );

        // Merge additional data if provided
        if let Some(extra) = args.get("data") {
            if let Some(obj) = extra.as_object() {
                for (k, v) in obj {
                    data.insert(k.clone(), v.clone());
                }
            }
        }

        let service_result = {
            let client_guard = self.rest_client.read();
            (*client_guard).as_ref().map(|client| client.call_service(domain, service, &serde_json::Value::Object(data)))
        }; // client_guard dropped here

        match service_result {
            Some(Ok(result)) => Ok(json!({
                "success": true,
                "domain": domain,
                "service": service,
                "entityId": entity_id,
                "result": result,
            })),
            Some(Err(e)) => Err(ExtensionError::ExecutionFailed(e)),
            None => Err(ExtensionError::ExecutionFailed(
                "Not connected to Home Assistant".to_string(),
            )),
        }
    }

    /// Update domain filters and re-sync entities from HA.
    async fn cmd_set_filters(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let domains_str = args
            .get("domains")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'domains' parameter".to_string())
            })?;

        let new_domains: Vec<String> = domains_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // Update config
        {
            let mut config_guard = self.config.write();
            if let Some(ref mut config) = *config_guard {
                config.domains = new_domains.clone();
            } else {
                return Err(ExtensionError::ExecutionFailed(
                    "Not connected to Home Assistant".to_string(),
                ));
            }
        }

        // Re-sync entities with new domain filter
        let entity_patterns = self.config.read().as_ref().map(|c| c.entity_patterns.clone()).unwrap_or_default();

        let fetch_result = {
            let client_guard = self.rest_client.read();
            (*client_guard).as_ref().map(|client| client.get_all_states(&new_domains))
        }; // client_guard dropped here

        match fetch_result {
            Some(Ok(mut entities)) => {
                // Apply entity_patterns filter
                if !entity_patterns.is_empty() {
                    entities.retain(|e| entity_matches_patterns(&e.entity_id, &entity_patterns));
                }

                let count = entities.len();
                let mut ent_map = HashMap::new();
                for e in entities {
                    ent_map.insert(e.entity_id.clone(), e);
                }

                {
                    let mut ents = self.entities.write();
                    *ents = ent_map;
                }

                Ok(json!({
                    "success": true,
                    "domains": new_domains,
                    "entities_synced": count,
                }))
            }
            Some(Err(e)) => Err(ExtensionError::ExecutionFailed(e)),
            None => Err(ExtensionError::ExecutionFailed(
                "Not connected to Home Assistant".to_string(),
            )),
        }
    }

    /// Manually trigger a full entity resync (without reconnecting).
    async fn cmd_refresh(&self) -> Result<serde_json::Value> {
        let config = self.config.read();
        let ha_config = config.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not configured. Run 'connect' or 'configure' first.".to_string())
        })?;

        let client = rest_client::HaRestClient::new(ha_config);
        let mut entities = client
            .get_all_states(&ha_config.domains)
            .map_err(ExtensionError::ExecutionFailed)?;

        if !ha_config.entity_patterns.is_empty() {
            entities.retain(|e| entity_matches_patterns(&e.entity_id, &ha_config.entity_patterns));
        }

        let count = entities.len();
        let mut entity_map = self.entities.write();
        entity_map.clear();
        for entity in entities {
            entity_map.insert(entity.entity_id.clone(), entity);
        }

        // Force re-sync on next produce_metrics cycle
        self.last_full_sync.store(0, Ordering::SeqCst);

        Ok(json!({
            "success": true,
            "entity_count": count,
            "message": format!("Refreshed {} entities from Home Assistant", count)
        }))
    }

    /// Get current connection status and entity statistics.
    async fn cmd_get_status(&self) -> Result<serde_json::Value> {
        let is_connected = self.connected.load(Ordering::SeqCst);
        let entities = self.entities.read();

        // Count by domain
        let mut domain_counts: HashMap<String, usize> = HashMap::new();
        for entity in entities.values() {
            *domain_counts.entry(entity.domain.clone()).or_insert(0) += 1;
        }

        let config_info = self
            .config
            .read()
            .as_ref()
            .map(|c| {
                json!({
                    "haUrl": c.ha_url,
                    "domains": c.domains,
                    "syncInterval": c.sync_interval,
                    "entityPatterns": c.entity_patterns,
                })
            })
            .unwrap_or(json!(null));

        Ok(json!({
            "connected": is_connected,
            "totalEntities": entities.len(),
            "domainCounts": domain_counts,
            "totalCommands": self.total_commands.load(Ordering::SeqCst),
            "config": config_info,
        }))
    }

    /// Get all areas from Home Assistant.
    ///
    /// NOTE: Areas are not available via the REST API in standard HA installs.
    /// This command will return an informative error when the REST endpoint
    /// is not available (which is the common case).
    async fn cmd_get_areas(&self) -> Result<serde_json::Value> {
        let areas_result = {
            let client_guard = self.rest_client.read();
            (*client_guard).as_ref().map(|client| client.get_areas())
        };

        match areas_result {
            Some(Ok(areas)) => Ok(json!({
                "success": true,
                "areas": areas,
            })),
            Some(Err(e)) => Ok(json!({
                "success": false,
                "error": e,
                "hint": "Home Assistant areas are not available via REST API. \
                         Use the Home Assistant UI to manage areas instead."
            })),
            None => Err(ExtensionError::ExecutionFailed(
                "Not connected to Home Assistant".to_string(),
            )),
        }
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

neomind_extension_sdk::neomind_export!(HomeAssistantBridgeExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = HomeAssistantBridgeExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "homeassistant-bridge");
        assert_eq!(meta.name, "Home Assistant Bridge");
        assert!(meta.description.is_some());
    }

    #[test]
    fn test_extension_commands() {
        let ext = HomeAssistantBridgeExtension::new();
        let commands = ext.commands();
        assert!(commands.len() >= 9, "expected at least 9 commands, got {}", commands.len());
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"connect"));
        assert!(names.contains(&"disconnect"));
        assert!(names.contains(&"list_entities"));
        assert!(names.contains(&"get_state"));
        assert!(names.contains(&"call_service"));
        assert!(names.contains(&"set_filters"));
        assert!(names.contains(&"get_status"));
        assert!(names.contains(&"get_areas"));
        assert!(names.contains(&"configure"));
    }

    #[test]
    fn test_extension_metrics() {
        let ext = HomeAssistantBridgeExtension::new();
        let metrics = ext.metrics();
        assert!(metrics.len() >= 3);
        let names: Vec<&str> = metrics.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"ha.connection"));
        assert!(names.contains(&"ha.entities_count"));
        assert!(names.contains(&"ha.total_commands"));
    }

    #[test]
    fn test_produce_metrics_disconnected() {
        let ext = HomeAssistantBridgeExtension::new();
        let metrics = ext.produce_metrics().unwrap();
        // Should have at least the 3 base metrics
        assert!(metrics.len() >= 3);

        let conn_metric = metrics.iter().find(|m| m.name == "ha.connection").unwrap();
        if let ParamMetricValue::Integer(v) = conn_metric.value {
            assert_eq!(v, 0); // not connected
        } else {
            panic!("Expected Integer for connection metric");
        }

        let count_metric = metrics
            .iter()
            .find(|m| m.name == "ha.entities_count")
            .unwrap();
        if let ParamMetricValue::Integer(v) = count_metric.value {
            assert_eq!(v, 0); // no entities
        } else {
            panic!("Expected Integer for entities count metric");
        }
    }

    #[test]
    fn test_default() {
        let ext = HomeAssistantBridgeExtension::default();
        assert!(!ext.connected.load(Ordering::SeqCst));
        assert!(!ext.running.load(Ordering::SeqCst));
        assert_eq!(ext.total_commands.load(Ordering::SeqCst), 0);
        assert_eq!(ext.template_registered.load(Ordering::SeqCst), 0);
    }


    #[test]
    fn test_entity_matches_patterns_empty() {
        // Empty patterns = accept all
        assert!(entity_matches_patterns("sensor.temperature", &[]));
        assert!(entity_matches_patterns("light.living_room", &[]));
    }

    #[test]
    fn test_entity_matches_patterns_substring() {
        let patterns: Vec<String> = vec!["living_room".to_string(), "kitchen".to_string()];
        assert!(entity_matches_patterns("sensor.living_room_temperature", &patterns));
        assert!(entity_matches_patterns("light.kitchen", &patterns));
        assert!(!entity_matches_patterns("sensor.bedroom", &patterns));
    }

    #[test]
    fn test_entity_matches_patterns_glob() {
        let patterns: Vec<String> = vec!["sensor.living_*".to_string()];
        assert!(entity_matches_patterns("sensor.living_room_temperature", &patterns));
        assert!(entity_matches_patterns("sensor.living_room_humidity", &patterns));
        assert!(!entity_matches_patterns("sensor.bedroom_temperature", &patterns));
    }

    #[test]
    fn test_entity_matches_patterns_wildcard_only() {
        let patterns: Vec<String> = vec!["*".to_string()];
        assert!(entity_matches_patterns("sensor.temperature", &patterns));
        assert!(entity_matches_patterns("light.living_room", &patterns));
    }

    #[test]
    fn test_entity_matches_patterns_case_insensitive() {
        let patterns: Vec<String> = vec!["Living_Room".to_string()];
        assert!(entity_matches_patterns("sensor.living_room_temperature", &patterns));
    }

    #[test]
    fn test_ha_state_response_to_entity() {
        let state = types::HaStateResponse {
            entity_id: "sensor.temperature".to_string(),
            state: "23.5".to_string(),
            attributes: serde_json::json!({
                "friendly_name": "Living Room Temperature",
                "unit_of_measurement": "\u{00b0}C",
                "battery": 85,
            }),
            last_changed: "2025-01-01T00:00:00+00:00".to_string(),
        };

        let entity = state.to_entity();
        assert_eq!(entity.entity_id, "sensor.temperature");
        assert_eq!(entity.domain, "sensor");
        assert_eq!(entity.name, "Living Room Temperature");
        assert_eq!(entity.state, "23.5");
        assert!((entity.value.unwrap() - 23.5).abs() < f64::EPSILON);
        assert_eq!(entity.unit.as_deref(), Some("\u{00b0}C"));
        assert_eq!(entity.battery, Some(85));
    }

    #[test]
    fn test_ha_state_response_unavailable() {
        let state = types::HaStateResponse {
            entity_id: "light.living_room".to_string(),
            state: "unavailable".to_string(),
            attributes: serde_json::json!({
                "friendly_name": "Living Room Light",
            }),
            last_changed: "2025-01-01T00:00:00+00:00".to_string(),
        };

        let entity = state.to_entity();
        assert_eq!(entity.domain, "light");
        assert!(entity.value.is_none());
        assert!(entity.unit.is_none());
        assert!(entity.battery.is_none());
    }

    #[test]
    fn test_ha_state_response_battery_level_field() {
        let state = types::HaStateResponse {
            entity_id: "sensor.outside_temp".to_string(),
            state: "15.0".to_string(),
            attributes: serde_json::json!({
                "battery_level": 60,
            }),
            last_changed: "2025-01-01T00:00:00+00:00".to_string(),
        };

        let entity = state.to_entity();
        assert_eq!(entity.battery, Some(60));
    }

    #[test]
    fn test_ha_config_defaults() {
        let config = serde_json::from_str::<types::HaConfig>(
            r#"{"ha_url":"http://hassio.local:8123","token":"test123"}"#,
        )
        .unwrap();

        assert_eq!(config.ha_url, "http://hassio.local:8123");
        assert_eq!(config.token, "test123");
        assert_eq!(config.domains, vec!["sensor", "light", "switch", "climate", "lock"]);
        assert_eq!(config.sync_interval, 30);
        assert!(config.entity_patterns.is_empty());
    }

    #[test]
    fn test_ha_config_with_entity_patterns() {
        let config = serde_json::from_str::<types::HaConfig>(
            r#"{"ha_url":"http://hassio.local:8123","token":"test123","entity_patterns":["sensor.living_*","light.*"]}"#,
        )
        .unwrap();

        assert_eq!(config.entity_patterns, vec!["sensor.living_*", "light.*"]);
    }
}
