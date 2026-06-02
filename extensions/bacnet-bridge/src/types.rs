use serde::{Deserialize, Serialize};

/// BACnet extension configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacnetConfig {
    pub bind_address: String,
    pub bind_port: u16,
    pub default_timeout_ms: u64,
    pub poll_interval_ms: u64,
}

impl Default for BacnetConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            bind_port: 47808,
            default_timeout_ms: 3000,
            poll_interval_ms: 10000,
        }
    }
}

/// BACnet object type identifiers
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BacnetObjectType {
    AnalogInput = 0,
    AnalogOutput = 1,
    AnalogValue = 2,
    BinaryInput = 3,
    BinaryOutput = 4,
    BinaryValue = 5,
    MultiStateInput = 13,
    MultiStateOutput = 14,
    MultiStateValue = 19,
    Device = 8,
}

impl BacnetObjectType {
    pub fn from_code(code: u16) -> Option<Self> {
        match code {
            0 => Some(Self::AnalogInput),
            1 => Some(Self::AnalogOutput),
            2 => Some(Self::AnalogValue),
            3 => Some(Self::BinaryInput),
            4 => Some(Self::BinaryOutput),
            5 => Some(Self::BinaryValue),
            13 => Some(Self::MultiStateInput),
            14 => Some(Self::MultiStateOutput),
            19 => Some(Self::MultiStateValue),
            8 => Some(Self::Device),
            _ => None,
        }
    }

    pub fn code(&self) -> u16 {
        *self as u16
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::AnalogInput => "analog_input",
            Self::AnalogOutput => "analog_output",
            Self::AnalogValue => "analog_value",
            Self::BinaryInput => "binary_input",
            Self::BinaryOutput => "binary_output",
            Self::BinaryValue => "binary_value",
            Self::MultiStateInput => "multi_state_input",
            Self::MultiStateOutput => "multi_state_output",
            Self::MultiStateValue => "multi_state_value",
            Self::Device => "device",
        }
    }
}

/// A discovered BACnet device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacnetDevice {
    pub device_id: u32,
    pub ip_address: String,
    pub port: u16,
    pub name: Option<String>,
    pub vendor_id: Option<u32>,
    pub vendor_name: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub description: Option<String>,
    pub max_apdu: Option<u32>,
    pub segmentation: Option<String>,
    pub objects: Vec<BacnetObject>,
    pub connected: bool,
    pub last_seen_ms: i64,
}

/// A BACnet object within a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacnetObject {
    pub object_type: BacnetObjectType,
    pub instance: u32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub present_value: Option<BacnetValue>,
    pub units: Option<String>,
    pub cov_subscribed: bool,
    pub cov_lifetime: Option<u32>,
}

/// BACnet data value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BacnetValue {
    Real(f64),
    Integer(i32),
    Unsigned(u32),
    Boolean(bool),
    String(String),
    Null,
}

impl BacnetValue {
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            BacnetValue::Real(v) => serde_json::json!(v),
            BacnetValue::Integer(v) => serde_json::json!(v),
            BacnetValue::Unsigned(v) => serde_json::json!(v),
            BacnetValue::Boolean(v) => serde_json::json!(v),
            BacnetValue::String(v) => serde_json::json!(v),
            BacnetValue::Null => serde_json::Value::Null,
        }
    }

    /// Try to convert to f64 for metric output
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            BacnetValue::Real(v) => Some(*v),
            BacnetValue::Integer(v) => Some(*v as f64),
            BacnetValue::Unsigned(v) => Some(*v as f64),
            BacnetValue::Boolean(v) => Some(if *v { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
}

/// COV subscription info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CovSubscription {
    pub subscriber_id: u32,
    pub device_id: u32,
    pub object_type: BacnetObjectType,
    pub instance: u32,
    pub lifetime: u32,
    pub confirmed: bool,
    pub active: bool,
    pub last_update_ms: i64,
}
