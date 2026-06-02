use serde::{Deserialize, Serialize};

/// ONVIF extension configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnvifConfig {
    pub discovery_timeout_ms: u64,
    pub default_username: Option<String>,
    pub default_password: Option<String>,
}

impl Default for OnvifConfig {
    fn default() -> Self {
        Self {
            discovery_timeout_ms: 5000,
            default_username: None,
            default_password: None,
        }
    }
}

/// A discovered or manually-added ONVIF device (camera)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnvifDevice {
    pub device_id: String,
    pub name: String,
    pub device_url: String,
    pub hardware_id: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub firmware_version: Option<String>,
    pub serial_number: Option<String>,
    pub scopes: Vec<String>,
    pub profiles: Vec<OnvifProfile>,
    pub ptz_supported: bool,
    pub username: Option<String>,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    pub connected: bool,
    pub last_seen_ms: i64,
}

/// Media profile (stream configuration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnvifProfile {
    pub token: String,
    pub name: String,
    pub video_source_token: Option<String>,
    pub video_encoder: Option<VideoEncoderConfig>,
    pub stream_uri: Option<String>,
    pub snapshot_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEncoderConfig {
    pub encoding: String,
    pub width: u32,
    pub height: u32,
    pub framerate: f64,
    pub bitrate: u32,
}

/// PTZ movement parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtzParams {
    pub pan: f64,
    pub tilt: f64,
    pub zoom: f64,
    pub speed: Option<f64>,
}

/// WS-Discovery probe match result
#[derive(Debug, Clone)]
pub struct DiscoveryMatch {
    pub endpoint: String,
    pub types: Vec<String>,
    pub scopes: Vec<String>,
    pub xaddrs: Vec<String>,
}
