use serde::{Deserialize, Serialize};

/// OPC-UA extension configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcUaConfig {
    pub server_url: String,
    pub security_mode: SecurityMode,
    pub username: Option<String>,
    pub password: Option<String>,
    pub session_timeout_ms: u64,
    pub auto_reconnect: bool,
}

impl Default for OpcUaConfig {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            security_mode: SecurityMode::None,
            username: None,
            password: None,
            session_timeout_ms: 30000,
            auto_reconnect: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityMode {
    None,
    Sign,
    SignAndEncrypt,
}

/// Cached OPC-UA node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcUaNode {
    pub node_id: String,
    pub browse_name: String,
    pub display_name: String,
    pub node_class: String,
    pub data_type: Option<String>,
    pub value: Option<serde_json::Value>,
    pub quality: Option<String>,
    pub source_timestamp: Option<i64>,
    pub children: Vec<String>,
}

/// Subscription information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionInfo {
    pub subscription_id: String,
    pub node_ids: Vec<String>,
    pub interval_ms: u64,
    pub active: bool,
}

/// Internal command message for async bridge.
///
/// Uses `std::sync::mpsc::Sender` for replies because callers are async
/// functions inside an existing tokio runtime, and
/// `tokio::sync::oneshot::blocking_recv()` would panic.
pub enum CommandMsg {
    Connect {
        config: OpcUaConfig,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    Disconnect {
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    Browse {
        node_id: Option<String>,
        max_depth: Option<u32>,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    Read {
        node_ids: Vec<String>,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    Write {
        node_id: String,
        value: serde_json::Value,
        data_type: Option<String>,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    Subscribe {
        node_ids: Vec<String>,
        interval_ms: Option<u64>,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    Unsubscribe {
        node_ids: Vec<String>,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    ListSubscriptions {
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    #[allow(dead_code)]
    Shutdown {
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
}
