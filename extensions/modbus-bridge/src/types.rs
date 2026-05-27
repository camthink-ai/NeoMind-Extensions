use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterConfig {
    pub address: u16,
    #[serde(default = "default_count")]
    pub count: u16,
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: RegisterDataType,
    #[serde(default)]
    pub scale: f64,
    #[serde(default)]
    pub unit: String,
}

fn default_count() -> u16 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterDataType {
    Uint16,
    Int16,
    Uint32,
    Int32,
    Float32,
    Bool,
}

impl RegisterDataType {
    #[allow(dead_code)]
    pub fn register_count(&self) -> u16 {
        match self {
            Self::Uint16 | Self::Int16 | Self::Bool => 1,
            Self::Uint32 | Self::Int32 | Self::Float32 => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterValue {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub raw: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionMode {
    Tcp,
    Rtu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub device_id: String,
    pub name: Option<String>,
    pub mode: ConnectionMode,
    pub ip: Option<String>,
    pub port: Option<u16>,
    pub serial_port: Option<String>,
    pub baud_rate: Option<u32>,
    pub slave_id: u8,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    pub registers: Vec<RegisterConfig>,
}

fn default_poll_interval() -> u64 {
    5000
}

fn default_timeout() -> u64 {
    3000
}

#[derive(Debug, Clone)]
pub struct DeviceState {
    pub config: DeviceConfig,
    pub register_values: Vec<RegisterValue>,
    pub connected: bool,
    pub poll_errors: u64,
    pub last_poll_ms: u64,
}
