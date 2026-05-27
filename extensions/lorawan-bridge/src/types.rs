use serde::{Deserialize, Serialize};

/// Supported Network Server types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NsType {
    Chirpstack,
    Ttn,
}

/// Network Server connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NsConfig {
    pub ns_type: NsType,
    pub broker_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub application_id: String,
    pub tenant_id: Option<String>,
    pub ns_api_url: Option<String>,
    #[serde(default = "default_decoder")]
    pub default_decoder: DecoderType,
    #[serde(default = "default_true")]
    pub auto_discover: bool,
}

fn default_decoder() -> DecoderType {
    DecoderType::Cayenne
}

fn default_true() -> bool {
    true
}

/// Payload decoder type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoderType {
    Cayenne,
    Custom,
}

/// A single decoded sensor field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedField {
    pub name: String,
    pub value: f64,
    pub unit: String,
}

/// Field descriptor for custom binary decoder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomDecoderField {
    pub offset: usize,
    pub length: usize,
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: CustomDataType,
    #[serde(default)]
    pub scale: f64,
    #[serde(default)]
    pub unit: String,
}

/// Data types supported by the custom binary decoder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomDataType {
    Uint8,
    Uint16,
    Int16,
    Uint32,
    Int32,
}

/// A LoRa device tracked by the bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoRaDevice {
    pub dev_eui: String,
    pub fields: Vec<DecodedField>,
    pub rssi: i32,
    pub snr: f64,
    pub battery: Option<u8>,
    pub f_cnt: u32,
    pub last_seen: i64,
    pub decoder_type: DecoderType,
    pub custom_decoder: Option<Vec<CustomDecoderField>>,
}
