//! NeoMind Home Assistant Bridge Extension
//!
//! Bridges Home Assistant into NeoMind, importing 3000+ HA entity integrations
//! as devices. Supports real-time state updates via WebSocket and command
//! execution (turn on/off lights, switches, climate, locks, etc.).

mod types;
mod rest_client;
mod ws_client;

use neomind_extension_sdk::{
    async_trait, json,
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use types::{HaConfig, HaEntity};

// ============================================================================
// Extension Struct
// ============================================================================

pub struct HomeAssistantBridgeExtension {
    rest_client: std::sync::RwLock<Option<rest_client::HaRestClient>>,
    entities: Arc<RwLock<HashMap<String, HaEntity>>>,
    running: Arc<AtomicBool>,
    config: std::sync::RwLock<Option<HaConfig>>,
    total_commands: AtomicI64,
    connected: Arc<AtomicBool>,
}

impl HomeAssistantBridgeExtension {
    pub fn new() -> Self {
        Self {
            rest_client: std::sync::RwLock::new(None),
            entities: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
            config: std::sync::RwLock::new(None),
            total_commands: AtomicI64::new(0),
            connected: Arc::new(AtomicBool::new(false)),
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
            "configure" => Ok(json!({"status": "ok"})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::new();

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

        // Entity count - use try_read to avoid blocking sync context
        let entity_count = self.entities.try_read().map(|e| e.len()).unwrap_or(0);
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
        if let Ok(entities) = self.entities.try_read() {
            for (entity_id, entity) in entities.iter() {
                // Sanitize entity_id for metric name: replace dots and hyphens with underscores
                let safe_id = entity_id.replace('.', "_").replace('-', "_");

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
            };

            *self.config.write().unwrap() = Some(ha_config);
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

        let ha_config = HaConfig {
            ha_url: ha_url.clone(),
            token: token.clone(),
            domains: domains.clone(),
            sync_interval,
        };

        // 1. Create REST client and test connection
        let client = rest_client::HaRestClient::new(&ha_config);
        let msg = client
            .test_connection()
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        // 2. Fetch all states and populate entity map
        let entities = client
            .get_all_states(&domains)
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        let count = entities.len();
        let mut ent_map = HashMap::new();
        for e in entities {
            ent_map.insert(e.entity_id.clone(), e);
        }

        {
            let mut ents = self.entities.write().await;
            *ents = ent_map;
        }

        // 3. Store config and client
        *self.config.write().unwrap() = Some(ha_config);
        *self.rest_client.write().unwrap() = Some(client);

        // 4. Spawn WebSocket read loop on the host Tokio runtime
        self.running.store(true, Ordering::SeqCst);
        self.connected.store(true, Ordering::SeqCst);

        let ws_running = self.running.clone();
        let ws_entities = self.entities.clone();
        let ws_connected = self.connected.clone();
        let ws_url = ha_url.clone();
        let ws_token = token.clone();

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    ws_client::run_ws_loop(ws_url, ws_token, ws_entities, ws_running, ws_connected)
                        .await;
                });
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

        // Give the WS loop a moment to notice the flag
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        {
            let mut ents = self.entities.write().await;
            ents.clear();
        }

        *self.rest_client.write().unwrap() = None;
        *self.config.write().unwrap() = None;

        Ok(json!({
            "success": true,
            "message": "Disconnected from Home Assistant"
        }))
    }

    /// List all imported entities, optionally filtered by domain.
    async fn cmd_list_entities(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let domain_filter = args.get("domain").and_then(|v| v.as_str());

        let entities = self.entities.read().await;
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
            let entities = self.entities.read().await;
            if let Some(entity) = entities.get(entity_id) {
                return Ok(json!({
                    "success": true,
                    "source": "cache",
                    "entity": entity,
                }));
            }
        }

        // Fall back to REST API
        // Clone the client data out of the std::sync::RwLock before any await,
        // so the guard is not held across an await point.
        let entity_result = {
            let client_guard = self.rest_client.read().unwrap();
            match *client_guard {
                Some(ref client) => Some(client.get_state(entity_id)),
                None => None,
            }
        }; // client_guard dropped here

        match entity_result {
            Some(Ok(entity)) => {
                let mut entities = self.entities.write().await;
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
            let client_guard = self.rest_client.read().unwrap();
            match *client_guard {
                Some(ref client) => {
                    Some(client.call_service(domain, service, &serde_json::Value::Object(data)))
                }
                None => None,
            }
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
            let mut config_guard = self.config.write().unwrap();
            if let Some(ref mut config) = *config_guard {
                config.domains = new_domains.clone();
            } else {
                return Err(ExtensionError::ExecutionFailed(
                    "Not connected to Home Assistant".to_string(),
                ));
            }
        }

        // Re-sync entities with new domain filter
        // Extract the data from std::sync::RwLock before any await.
        let fetch_result = {
            let client_guard = self.rest_client.read().unwrap();
            match *client_guard {
                Some(ref client) => Some(client.get_all_states(&new_domains)),
                None => None,
            }
        }; // client_guard dropped here

        match fetch_result {
            Some(Ok(entities)) => {
                let count = entities.len();
                let mut ent_map = HashMap::new();
                for e in entities {
                    ent_map.insert(e.entity_id.clone(), e);
                }

                {
                    let mut ents = self.entities.write().await;
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

    /// Get current connection status and entity statistics.
    async fn cmd_get_status(&self) -> Result<serde_json::Value> {
        let is_connected = self.connected.load(Ordering::SeqCst);
        let entities = self.entities.read().await;

        // Count by domain
        let mut domain_counts: HashMap<String, usize> = HashMap::new();
        for entity in entities.values() {
            *domain_counts.entry(entity.domain.clone()).or_insert(0) += 1;
        }

        let config_info = self
            .config
            .read()
            .unwrap()
            .as_ref()
            .map(|c| {
                json!({
                    "haUrl": c.ha_url,
                    "domains": c.domains,
                    "syncInterval": c.sync_interval,
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
    use types::domain_has_commands;

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
        assert!(commands.len() >= 8);
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"connect"));
        assert!(names.contains(&"disconnect"));
        assert!(names.contains(&"list_entities"));
        assert!(names.contains(&"get_state"));
        assert!(names.contains(&"call_service"));
        assert!(names.contains(&"set_filters"));
        assert!(names.contains(&"get_status"));
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
    }

    #[test]
    fn test_types_domain_has_commands() {
        assert!(domain_has_commands("light"));
        assert!(domain_has_commands("switch"));
        assert!(domain_has_commands("climate"));
        assert!(domain_has_commands("lock"));
        assert!(domain_has_commands("cover"));
        assert!(domain_has_commands("fan"));
        assert!(domain_has_commands("media_player"));
        assert!(!domain_has_commands("sensor"));
        assert!(!domain_has_commands("binary_sensor"));
        assert!(!domain_has_commands("weather"));
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
    }
}
