//! NeoMind OPC-UA Bridge Extension
//!
//! Connects to OPC-UA servers for industrial data acquisition and control.
//!
//! # Features
//!
//! - Connect to OPC-UA servers (with security mode and authentication)
//! - Browse the server address space
//! - Read and write node values
//! - Subscribe to data change notifications
//! - Per-node metrics export via NeoMind device model
//!
//! # Architecture
//!
//! Uses a background tokio runtime on a dedicated `std::thread` for async
//! OPC-UA operations. Commands are bridged via mpsc channels:
//!
//! ```text
//! execute_command() → channel → async runtime → channel → result
//! ```
//!
//! Node cache uses `parking_lot::RwLock` for synchronous `produce_metrics()`
//! access without requiring `.await`.

mod types;
mod node_cache;
mod opcua_client;

use neomind_extension_sdk::{
    async_trait, json, CapabilityContext, Extension, ExtensionMetadata, ExtensionError,
    ExtensionMetricValue, MetricDescriptor, ExtensionCommand, MetricDataType,
    ParameterDefinition, ParamMetricValue, Result,
};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

use node_cache::NodeCache;
use opcua_client::OpcUaClientManager;
use types::OpcUaConfig;

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct OpcUaBridgeExtension {
    client: RwLock<Option<OpcUaClientManager>>,
    cache: Arc<NodeCache>,
    connected: Arc<AtomicBool>,
    total_commands: AtomicI64,
    /// 0 = template not registered yet, 1 = registered
    template_registered: AtomicI64,
    client_starting: AtomicBool,
}

impl OpcUaBridgeExtension {
    pub fn new() -> Self {
        Self {
            client: RwLock::new(None),
            cache: Arc::new(NodeCache::new()),
            connected: Arc::new(AtomicBool::new(false)),
            total_commands: AtomicI64::new(0),
            template_registered: AtomicI64::new(0),
            client_starting: AtomicBool::new(false),
        }
    }
}

impl Default for OpcUaBridgeExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for OpcUaBridgeExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "opcua-bridge",
                "OPC-UA Bridge",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description(
                "OPC-UA bridge extension — connect industrial servers, browse nodes, subscribe to data changes",
            )
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "sessionTimeout".to_string(),
                    display_name: "Session Timeout".to_string(),
                    description: "OPC-UA session timeout in milliseconds".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(30000)),
                    min: Some(1000.0),
                    max: Some(300000.0),
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "autoReconnect".to_string(),
                    display_name: "Auto Reconnect".to_string(),
                    description: "Automatically reconnect on connection loss".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("true".to_string())),
                    min: None,
                    max: None,
                    options: vec!["true".to_string(), "false".to_string()],
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
                name: "connected".to_string(),
                display_name: "Connected".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "nodes_count".to_string(),
                display_name: "Cached Nodes".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "subscriptions_count".to_string(),
                display_name: "Active Subscriptions".to_string(),
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
                display_name: "Connect".to_string(),
                description: "Connect to an OPC-UA server".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "server_url".to_string(),
                        display_name: "Server URL".to_string(),
                        description: "OPC-UA server endpoint URL (e.g. opc.tcp://localhost:4840)"
                            .to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "security_mode".to_string(),
                        display_name: "Security Mode".to_string(),
                        description: "Security mode (none, sign, sign_and_encrypt)".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec![
                                "none".to_string(),
                                "sign".to_string(),
                                "sign_and_encrypt".to_string(),
                            ],
                        },
                        required: false,
                        default_value: Some(ParamMetricValue::String("none".to_string())),
                        min: None,
                        max: None,
                        options: vec![
                            "none".to_string(),
                            "sign".to_string(),
                            "sign_and_encrypt".to_string(),
                        ],
                    },
                    ParameterDefinition {
                        name: "username".to_string(),
                        display_name: "Username".to_string(),
                        description: "Username for authentication (optional)".to_string(),
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
                        description: "Password for authentication (optional)".to_string(),
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
                        "server_url": "opc.tcp://localhost:4840",
                        "security_mode": "none"
                    }),
                    json!({
                        "server_url": "opc.tcp://192.168.1.100:4840",
                        "security_mode": "sign_and_encrypt",
                        "username": "admin",
                        "password": "secret"
                    }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "disconnect".to_string(),
                display_name: "Disconnect".to_string(),
                description: "Disconnect from the OPC-UA server".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "browse".to_string(),
                display_name: "Browse".to_string(),
                description: "Browse the OPC-UA address space from a given node".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "node_id".to_string(),
                        display_name: "Node ID".to_string(),
                        description:
                            "Starting node ID (omit or use \"i=84\" for root)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String("i=84".to_string())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "max_depth".to_string(),
                        display_name: "Max Depth".to_string(),
                        description: "Maximum browse depth (default: 1)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(1)),
                        min: Some(1.0),
                        max: Some(10.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({}),
                    json!({ "node_id": "i=85", "max_depth": 2 }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "read".to_string(),
                display_name: "Read Nodes".to_string(),
                description: "Read current values from one or more OPC-UA nodes".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "node_ids".to_string(),
                        display_name: "Node IDs".to_string(),
                        description: "Array of node IDs to read".to_string(),
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
                    json!({ "node_ids": ["i=2258", "i=2259"] }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "write".to_string(),
                display_name: "Write Node".to_string(),
                description: "Write a value to an OPC-UA node".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "node_id".to_string(),
                        display_name: "Node ID".to_string(),
                        description: "Target node ID".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "value".to_string(),
                        display_name: "Value".to_string(),
                        description: "Value to write (number, string, boolean, etc.)"
                            .to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "data_type".to_string(),
                        display_name: "Data Type".to_string(),
                        description: "Expected data type (optional, e.g. \"Float\", \"Int32\")"
                            .to_string(),
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
                    json!({ "node_id": "ns=2;s=Temperature", "value": 42.5 }),
                    json!({ "node_id": "ns=2;s=PumpSpeed", "value": 1500, "data_type": "Int32" }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "subscribe".to_string(),
                display_name: "Subscribe".to_string(),
                description: "Subscribe to data change notifications for one or more nodes"
                    .to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "node_ids".to_string(),
                        display_name: "Node IDs".to_string(),
                        description: "Array of node IDs to monitor".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "interval_ms".to_string(),
                        display_name: "Sampling Interval (ms)".to_string(),
                        description: "Sampling interval in milliseconds (default: 1000)"
                            .to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(1000)),
                        min: Some(50.0),
                        max: Some(60000.0),
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({
                        "node_ids": ["ns=2;s=Temperature", "ns=2;s=Pressure"],
                        "interval_ms": 500
                    }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "unsubscribe".to_string(),
                display_name: "Unsubscribe".to_string(),
                description: "Unsubscribe from data change notifications".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "node_ids".to_string(),
                        display_name: "Node IDs".to_string(),
                        description: "Array of node IDs to stop monitoring".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "node_ids": ["ns=2;s=Temperature"] })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_subscriptions".to_string(),
                display_name: "List Subscriptions".to_string(),
                description: "List all active subscriptions".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_nodes".to_string(),
                display_name: "List Nodes".to_string(),
                description: "List all cached nodes".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_node".to_string(),
                display_name: "Get Node".to_string(),
                description: "Get details of a specific cached node".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "node_id".to_string(),
                        display_name: "Node ID".to_string(),
                        description: "Node ID to look up".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "node_id": "ns=2;s=Temperature" })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".to_string(),
                display_name: "Get Status".to_string(),
                description: "Get current connection and cache status".to_string(),
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
            "connect" => self.cmd_connect(args),
            "disconnect" => self.cmd_disconnect(),
            "browse" => self.cmd_browse(args),
            "read" => self.cmd_read(args),
            "write" => self.cmd_write(args),
            "subscribe" => self.cmd_subscribe(args),
            "unsubscribe" => self.cmd_unsubscribe(args),
            "list_subscriptions" => self.cmd_list_subscriptions(),
            "list_nodes" => self.cmd_list_nodes(),
            "get_node" => self.cmd_get_node(args),
            "get_status" => self.cmd_get_status(),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::new();

        // Auto-register device template (with 30s cooldown on failure)
        let reg = self.template_registered.load(Ordering::SeqCst);
        if reg == 0 || (reg > 1 && chrono::Utc::now().timestamp_millis() - reg > 30_000) {
            self.register_template();
        }

        // Extension-level metrics
        metrics.push(ExtensionMetricValue {
            name: "total_commands".to_string(),
            value: ParamMetricValue::Integer(self.total_commands.load(Ordering::SeqCst)),
            timestamp: now,
        });

        let conn_flag = if self.connected.load(Ordering::SeqCst) {
            1
        } else {
            0
        };
        metrics.push(ExtensionMetricValue {
            name: "connected".to_string(),
            value: ParamMetricValue::Integer(conn_flag),
            timestamp: now,
        });

        metrics.push(ExtensionMetricValue {
            name: "nodes_count".to_string(),
            value: ParamMetricValue::Integer(self.cache.node_count() as i64),
            timestamp: now,
        });

        metrics.push(ExtensionMetricValue {
            name: "subscriptions_count".to_string(),
            value: ParamMetricValue::Integer(self.cache.subscription_count() as i64),
            timestamp: now,
        });

        // Per-node metrics from cache
        let nodes = self.cache.get_all_nodes();
        let ctx = CapabilityContext::default();

        for node in &nodes {
            // Build a sanitized metric key from node_id (replace special chars)
            let safe_id = node
                .node_id
                .replace('=', "_")
                .replace(';', "_")
                .replace(':', "_")
                .replace(' ', "_");

            // Extension-level metric per node value
            if let Some(ref val) = node.value {
                let float_val = match val {
                    serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
                    _ => 0.0,
                };
                metrics.push(ExtensionMetricValue {
                    name: format!("opcua.node.{}.value", safe_id),
                    value: ParamMetricValue::Float(float_val),
                    timestamp: now,
                });
            }

            // Per-device metrics via CapabilityContext
            let device_id = format!("opcua-{}", safe_id);

            let _ = ctx.invoke_capability(
                "device_metrics_write",
                &json!({
                    "device_id": device_id,
                    "metric": "value",
                    "value": node.value,
                    "timestamp": now,
                }),
            );

            let _ = ctx.invoke_capability(
                "device_metrics_write",
                &json!({
                    "device_id": device_id,
                    "metric": "quality",
                    "value": node.quality,
                    "timestamp": now,
                }),
            );

            let _ = ctx.invoke_capability(
                "device_metrics_write",
                &json!({
                    "device_id": device_id,
                    "metric": "source_timestamp",
                    "value": node.source_timestamp,
                    "timestamp": now,
                }),
            );
        }

        Ok(metrics)
    }

    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> {
        // Extension-level configuration is accepted silently.
        // Connection configuration is done via the connect command.
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Template & Device Registration
// ============================================================================

impl OpcUaBridgeExtension {
    /// Register the "opcua_server" and "opcua_node" device templates with NeoMind.
    /// Called once from produce_metrics() when template_registered == 0.
    fn register_template(&self) {
        let ctx = CapabilityContext::default();

        // Register the server template
        let server_template = json!({
            "device_type": "opcua_server",
            "name": "OPC-UA Server",
            "description": "OPC-UA industrial server connection",
            "categories": ["industrial", "opcua"],
            "metrics": [
                { "name": "connected", "display_name": "Connection Status", "data_type": "Integer" },
                { "name": "nodes_count", "display_name": "Cached Nodes", "data_type": "Integer" },
                { "name": "subscriptions_count", "display_name": "Active Subscriptions", "data_type": "Integer" }
            ],
            "commands": [
                {
                    "name": "browse",
                    "display_name": "Browse",
                    "description": "Browse address space",
                    "parameters": [
                        { "name": "node_id", "display_name": "Node ID", "data_type": "String", "required": false }
                    ]
                },
                {
                    "name": "read",
                    "display_name": "Read",
                    "description": "Read node values",
                    "parameters": [
                        { "name": "node_ids", "display_name": "Node IDs", "data_type": "String", "required": true }
                    ]
                },
                {
                    "name": "write",
                    "display_name": "Write",
                    "description": "Write node value",
                    "parameters": [
                        { "name": "node_id", "display_name": "Node ID", "data_type": "String", "required": true },
                        { "name": "value", "display_name": "Value", "data_type": "String", "required": true }
                    ]
                }
            ]
        });

        let result = ctx.invoke_capability("device_template_register", &server_template);
        let server_ok = result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if server_ok {
            eprintln!("[opcua-bridge] Server template registered");
        } else {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            eprintln!(
                "[opcua-bridge] Server template registration failed: {} (will retry)",
                err
            );
        }

        // Register the node template
        let node_template = json!({
            "device_type": "opcua_node",
            "name": "OPC-UA Node",
            "description": "OPC-UA server address space node with value, quality, and timestamp",
            "categories": ["industrial", "opcua"],
            "metrics": [
                { "name": "value", "display_name": "Node Value", "data_type": "Float" },
                { "name": "quality", "display_name": "Quality", "data_type": "String" },
                { "name": "source_timestamp", "display_name": "Source Timestamp", "data_type": "Integer" }
            ],
            "commands": [
                {
                    "name": "read",
                    "display_name": "Read",
                    "description": "Read node value",
                    "parameters": []
                },
                {
                    "name": "write",
                    "display_name": "Write",
                    "description": "Write node value",
                    "parameters": [
                        { "name": "value", "display_name": "Value", "data_type": "String", "required": true }
                    ]
                }
            ]
        });

        let result = ctx.invoke_capability("device_template_register", &node_template);
        let node_ok = result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if node_ok {
            eprintln!("[opcua-bridge] Node template registered");
        } else {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            eprintln!(
                "[opcua-bridge] Node template registration failed: {} (will retry)",
                err
            );
        }

        // Only mark registered when BOTH templates succeed
        if server_ok && node_ok {
            self.template_registered.store(1, Ordering::SeqCst);
        } else if self.template_registered.load(Ordering::SeqCst) != 1 {
            // Store current timestamp as failure marker for cooldown
            self.template_registered.store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        }
    }

    /// Register an OPC-UA node as a NeoMind device instance.
    fn register_node_device(&self, node_id: &str, display_name: &str) {
        let ctx = CapabilityContext::default();

        let safe_id = node_id
            .replace('=', "_")
            .replace(';', "_")
            .replace(':', "_")
            .replace(' ', "_");

        let device_id = format!("opcua-{}", safe_id);

        let device_json = json!({
            "device_id": device_id,
            "name": display_name,
            "device_type": "opcua_node",
        });

        let result = ctx.invoke_capability("device_register", &device_json);
        if result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            eprintln!("[opcua-bridge] Node device '{}' registered", device_id);
        } else {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            eprintln!(
                "[opcua-bridge] Node device '{}' registration skipped: {}",
                device_id, err
            );
        }
    }
}

// ============================================================================
// Command Handlers
// ============================================================================

impl OpcUaBridgeExtension {
    /// Ensure the background client manager is started, starting it if needed.
    fn ensure_client_started(&self) -> Result<()> {
        {
            let guard = self.client.read();
            if guard.is_some() {
                return Ok(());
            }
        }

        // Use compare_exchange to ensure only one thread initializes
        if self.client_starting.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            // Another thread is starting — spin-wait with timeout
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(10));
                {
                    let guard = self.client.read();
                    if guard.is_some() {
                        return Ok(());
                    }
                }
                if std::time::Instant::now() >= deadline {
                    return Err(ExtensionError::ExecutionFailed("Client initialization timeout".to_string()));
                }
            }
        }

        let result = (|| -> Result<()> {
            let mut guard = self.client.write();
            if guard.is_some() {
                return Ok(());
            }

            let mut manager = OpcUaClientManager::new(self.connected.clone());
            manager
                .start(self.cache.clone())
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to start client: {}", e)))?;
            *guard = Some(manager);
            Ok(())
        })();

        self.client_starting.store(false, Ordering::SeqCst);
        result
    }

    fn cmd_connect(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        self.ensure_client_started()?;

        let server_url = args
            .get("server_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'server_url' parameter".to_string())
            })?
            .to_string();

        let security_mode = args
            .get("security_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("none");

        let username = args
            .get("username")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let password = args
            .get("password")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let config = OpcUaConfig {
            server_url: server_url.clone(),
            security_mode: serde_json::from_value(serde_json::Value::String(
                security_mode.to_string(),
            ))
            .unwrap_or(types::SecurityMode::None),
            username,
            password,
            session_timeout_ms: 30000,
            auto_reconnect: true,
        };

        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Client not initialized".to_string())
        })?;

        let result = client
            .connect(config)
            .map_err(ExtensionError::ExecutionFailed)?;

        // Register server as a NeoMind device
        let ctx = CapabilityContext::default();
        let server_device_id = format!("opcua-server-{}", server_url.replace([':', '/', '.'], "_"));
        let _ = ctx.invoke_capability(
            "device_register",
            &json!({
                "device_id": server_device_id,
                "name": format!("OPC-UA Server ({})", server_url),
                "device_type": "opcua_server",
            }),
        );

        Ok(result)
    }

    fn cmd_disconnect(&self) -> Result<serde_json::Value> {
        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not connected".to_string())
        })?;

        client
            .disconnect()
            .map_err(ExtensionError::ExecutionFailed)
    }

    fn cmd_browse(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let node_id = args.get("node_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let max_depth = args
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not connected".to_string())
        })?;

        let result = client
            .browse(node_id, max_depth)
            .map_err(ExtensionError::ExecutionFailed)?;

        // Cache browsed nodes and register them as devices
        if let Some(nodes_arr) = result.get("nodes").and_then(|v| v.as_array()) {
            for node_val in nodes_arr {
                if let (Some(nid), Some(dn)) = (
                    node_val.get("node_id").and_then(|v| v.as_str()),
                    node_val.get("display_name").and_then(|v| v.as_str()),
                ) {
                    let node = types::OpcUaNode {
                        node_id: nid.to_string(),
                        browse_name: node_val
                            .get("browse_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        display_name: dn.to_string(),
                        node_class: node_val
                            .get("node_class")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string(),
                        data_type: node_val
                            .get("data_type")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        value: None,
                        quality: None,
                        source_timestamp: None,
                        children: Vec::new(),
                    };
                    self.cache.upsert_node(node);
                    self.register_node_device(nid, dn);
                }
            }
        }

        Ok(result)
    }

    /// Parse node_ids from args — accepts JSON array or comma-separated string
    fn parse_node_ids(args: &serde_json::Value) -> Result<Vec<String>> {
        let raw = args
            .get("node_ids")
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'node_ids' parameter".to_string()))?;

        let ids: Vec<String> = if raw.is_array() {
            raw.as_array().unwrap()
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        } else if let Some(s) = raw.as_str() {
            s.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            return Err(ExtensionError::InvalidArguments(
                "'node_ids' must be an array or comma-separated string".to_string(),
            ));
        };

        if ids.is_empty() {
            return Err(ExtensionError::InvalidArguments(
                "At least one node_id is required".to_string(),
            ));
        }
        Ok(ids)
    }

    fn cmd_read(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let node_ids = Self::parse_node_ids(args)?;

        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not connected".to_string())
        })?;

        client
            .read(node_ids)
            .map_err(ExtensionError::ExecutionFailed)
    }

    fn cmd_write(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let node_id = args
            .get("node_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'node_id' parameter".to_string())
            })?
            .to_string();

        let value = args
            .get("value")
            .cloned()
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'value' parameter".to_string())
            })?;

        let data_type = args
            .get("data_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not connected".to_string())
        })?;

        client
            .write(node_id, value, data_type)
            .map_err(ExtensionError::ExecutionFailed)
    }

    fn cmd_subscribe(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let node_ids = Self::parse_node_ids(args)?;
        let interval_ms = args.get("interval_ms").and_then(|v| v.as_u64());

        // Validate interval bounds
        if let Some(interval) = interval_ms {
            if interval < 50 || interval > 60000 {
                return Err(ExtensionError::InvalidArguments(
                    format!("interval_ms must be between 50 and 60000, got {}", interval)
                ));
            }
        }

        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not connected".to_string())
        })?;

        client
            .subscribe(node_ids, interval_ms)
            .map_err(ExtensionError::ExecutionFailed)
    }

    fn cmd_unsubscribe(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let node_ids = Self::parse_node_ids(args)?;

        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not connected".to_string())
        })?;

        client
            .unsubscribe(node_ids)
            .map_err(ExtensionError::ExecutionFailed)
    }

    fn cmd_list_subscriptions(&self) -> Result<serde_json::Value> {
        let guard = self.client.read();
        let client = guard.as_ref().ok_or_else(|| {
            ExtensionError::ExecutionFailed("Not connected".to_string())
        })?;

        client
            .list_subscriptions()
            .map_err(ExtensionError::ExecutionFailed)
    }

    fn cmd_list_nodes(&self) -> Result<serde_json::Value> {
        let nodes = self.cache.get_all_nodes();
        let nodes_json: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "node_id": n.node_id,
                    "browse_name": n.browse_name,
                    "display_name": n.display_name,
                    "node_class": n.node_class,
                    "data_type": n.data_type,
                    "value": n.value,
                    "quality": n.quality,
                    "source_timestamp": n.source_timestamp,
                    "children": n.children,
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "count": nodes_json.len(),
            "nodes": nodes_json,
        }))
    }

    fn cmd_get_node(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let node_id = args
            .get("node_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("Missing 'node_id' parameter".to_string())
            })?;

        let node = self.cache.get_node(node_id).ok_or_else(|| {
            ExtensionError::ExecutionFailed(format!("Node not found in cache: {}", node_id))
        })?;

        Ok(json!({
            "success": true,
            "node": {
                "node_id": node.node_id,
                "browse_name": node.browse_name,
                "display_name": node.display_name,
                "node_class": node.node_class,
                "data_type": node.data_type,
                "value": node.value,
                "quality": node.quality,
                "source_timestamp": node.source_timestamp,
                "children": node.children,
            },
        }))
    }

    fn cmd_get_status(&self) -> Result<serde_json::Value> {
        let connected = self.connected.load(Ordering::SeqCst);
        let nodes_count = self.cache.node_count();
        let subs_count = self.cache.subscription_count();
        let total_cmds = self.total_commands.load(Ordering::SeqCst);

        Ok(json!({
            "success": true,
            "connected": connected,
            "nodes_count": nodes_count,
            "subscriptions_count": subs_count,
            "total_commands": total_cmds,
        }))
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

neomind_extension_sdk::neomind_export!(OpcUaBridgeExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_extension() {
        let ext = OpcUaBridgeExtension::new();
        assert_eq!(ext.total_commands.load(Ordering::SeqCst), 0);
        assert!(!ext.connected.load(Ordering::SeqCst));
        assert_eq!(ext.cache.node_count(), 0);
        assert_eq!(ext.cache.subscription_count(), 0);
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = OpcUaBridgeExtension::new();
        let result = ext.execute_command("nonexistent", &json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_config_deserialization() {
        let config_json = json!({
            "server_url": "opc.tcp://localhost:4840",
            "security_mode": "none",
            "session_timeout_ms": 30000,
            "auto_reconnect": true,
        });

        let config: OpcUaConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.server_url, "opc.tcp://localhost:4840");
        assert!(config.username.is_none());
        assert!(config.password.is_none());
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_config_with_auth() {
        let config_json = json!({
            "server_url": "opc.tcp://192.168.1.100:4840",
            "security_mode": "sign_and_encrypt",
            "username": "admin",
            "password": "secret",
            "session_timeout_ms": 60000,
            "auto_reconnect": false,
        });

        let config: OpcUaConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.server_url, "opc.tcp://192.168.1.100:4840");
        assert_eq!(config.username.as_deref(), Some("admin"));
        assert_eq!(config.password.as_deref(), Some("secret"));
        assert!(!config.auto_reconnect);
    }

    #[tokio::test]
    async fn test_list_nodes_empty() {
        let ext = OpcUaBridgeExtension::new();
        let result = ext.cmd_list_nodes().unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["count"], 0);
    }

    #[tokio::test]
    async fn test_get_status_initial() {
        let ext = OpcUaBridgeExtension::new();
        let result = ext.cmd_get_status().unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["connected"], false);
        assert_eq!(result["nodes_count"], 0);
        assert_eq!(result["subscriptions_count"], 0);
        assert_eq!(result["total_commands"], 0);
    }

    #[tokio::test]
    async fn test_get_node_not_found() {
        let ext = OpcUaBridgeExtension::new();
        let result = ext.cmd_get_node(&json!({ "node_id": "i=9999" }));
        assert!(result.is_err());
    }

    #[test]
    fn test_node_cache_operations() {
        let cache = NodeCache::new();

        let node = types::OpcUaNode {
            node_id: "ns=2;s=Temperature".to_string(),
            browse_name: "Temperature".to_string(),
            display_name: "Temperature Sensor".to_string(),
            node_class: "Variable".to_string(),
            data_type: Some("Float".to_string()),
            value: Some(json!(42.5)),
            quality: Some("Good".to_string()),
            source_timestamp: Some(1700000000000i64),
            children: vec![],
        };

        cache.upsert_node(node.clone());
        assert_eq!(cache.node_count(), 1);

        let retrieved = cache.get_node("ns=2;s=Temperature").unwrap();
        assert_eq!(retrieved.display_name, "Temperature Sensor");
        assert_eq!(retrieved.value, Some(json!(42.5)));

        cache.update_node_value(
            "ns=2;s=Temperature",
            json!(43.0),
            Some("Good".to_string()),
            Some(1700000001000i64),
        );
        let updated = cache.get_node("ns=2;s=Temperature").unwrap();
        assert_eq!(updated.value, Some(json!(43.0)));

        cache.remove_node("ns=2;s=Temperature");
        assert_eq!(cache.node_count(), 0);
    }

    #[test]
    fn test_node_cache_subscriptions() {
        let cache = NodeCache::new();

        let sub = types::SubscriptionInfo {
            subscription_id: "sub-123".to_string(),
            node_ids: vec!["ns=2;s=Temp".to_string(), "ns=2;s=Pressure".to_string()],
            interval_ms: 500,
            active: true,
        };

        cache.upsert_subscription(sub);
        assert_eq!(cache.subscription_count(), 1);

        let subs = cache.get_all_subscriptions();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].subscription_id, "sub-123");

        cache.remove_subscription("sub-123");
        assert_eq!(cache.subscription_count(), 0);
    }
}
