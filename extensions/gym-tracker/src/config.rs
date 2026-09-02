// config.rs
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    // The host always sends `Init { config: {} }` at load and only delivers the
    // real config afterwards via ConfigUpdate, so every field needs a default —
    // `configure({})` must succeed for a cold load. An empty `device.host`
    // means "not provisioned yet": the extension idles (no ingest thread) until
    // a real config arrives and configure() runs again.
    #[serde(default)]
    pub device: DeviceCfg,
    #[serde(default)]
    pub device_id: String,
    pub rtsp_url: Option<String>,
    #[serde(default)]
    pub ingest: IngestCfg,
    #[serde(default)]
    pub identity: IdentityCfg,
    #[serde(default)]
    pub roi: RoiCfg,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}
fn default_data_dir() -> String {
    std::env::var("NEOMIND_EXTENSION_DATA_DIR").unwrap_or_else(|_| ".".into())
}
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeviceCfg { pub host: String, #[serde(default)] pub port: Option<u16>, pub username: String, pub password: String, #[serde(default)] pub tls_insecure: bool }
// TLS verification defaults to ON; self-signed deployments
// opt in explicitly via tls_insecure: true in config.json
#[derive(Debug, Clone, Deserialize)]
pub struct IngestCfg { pub topic: String, pub publish_hz: u32, pub track_ttl_sec: u32, pub reconnect_backoff_sec: Vec<u64> }
#[derive(Debug, Clone, Deserialize)]
pub struct IdentityCfg {
    pub match_threshold: f32,
    pub auto_capture_unknown: bool,
    pub unknown_prefix: String,
    /// A track whose nearest member is FARTHER than this auto-enrolls as a
    /// new (unnamed) member. Deliberately above match_threshold: the gap
    /// between the two absorbs embedding drift for already-known people
    /// (same-person re-embeds were measured 0-352, unknown persons 684+).
    #[serde(default = "default_auto_capture_distance")]
    pub auto_capture_distance: f32,
}
fn default_auto_capture_distance() -> f32 { 600.0 }
#[derive(Debug, Clone, Deserialize)]
pub struct RoiCfg { pub dwell_debounce_sec: u32, pub hysteresis: bool }

impl Default for IngestCfg { fn default() -> Self { Self { topic: "gym/track".into(), publish_hz: 8, track_ttl_sec: 30, reconnect_backoff_sec: vec![1,2,5,10,30] } } }
impl Default for IdentityCfg {
    fn default() -> Self {
        Self {
            // Max L2 distance for a member match, in the osnet uint8-quantized
            // embedding space. 40.0 sits well below the ~52 distance measured
            // between unrelated probes; tune live with the exposed distances.
            match_threshold: 40.0,
            auto_capture_unknown: true,
            unknown_prefix: "未知会员".into(),
            auto_capture_distance: 600.0,
        }
    }
}
impl Default for RoiCfg { fn default() -> Self { Self { dwell_debounce_sec: 3, hysteresis: true } } }

impl Config {
    pub fn parse(raw: &str) -> Result<Self, serde_json::Error> {
        if raw.trim().is_empty() { return Err(serde::de::Error::custom("empty config")); }
        serde_json::from_str(raw)
    }
    /// True once a device host has been supplied — gates the ingest thread and
    /// device login so an unprovisioned cold load stays inert.
    pub fn provisioned(&self) -> bool { !self.device.host.trim().is_empty() }
    // NE503 this firmware: HTTPS 443 (self-signed), WS wss, RTSP on :8554. See "NE503 device reality" in the plan.
    pub fn ws_url(&self) -> String { format!("wss://{}/api/v1/events/stream", self.device.host) }
    pub fn rest_base(&self) -> String { format!("https://{}", self.device.host) }
    pub fn rtsp_url(&self, stream: &str) -> String { format!("rtsp://{}:8554/{}", self.device.host, stream) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_full() {
        let s = r#"{"device":{"host":"192.168.93.200","username":"admin","password":"password","tls_insecure":true},"device_id":"ne503-001","ingest":{"topic":"gym/track","publish_hz":8,"track_ttl_sec":30,"reconnect_backoff_sec":[1,2,5,10,30]},"identity":{"match_threshold":0.1,"auto_capture_unknown":true,"unknown_prefix":"未知会员"},"roi":{"dwell_debounce_sec":3,"hysteresis":true},"data_dir":"/tmp/gym"}"#;
        let c = Config::parse(s).unwrap();
        assert_eq!(c.device.host, "192.168.93.200");
        assert_eq!(c.ws_url(), "wss://192.168.93.200/api/v1/events/stream");
        assert_eq!(c.rest_base(), "https://192.168.93.200");
        assert_eq!(c.rtsp_url("sub"), "rtsp://192.168.93.200:8554/sub");
        assert!(c.device.tls_insecure);
    }
    #[test]
    fn rejects_empty() { assert!(Config::parse("").is_err()); }
    #[test]
    fn cold_load_empty_config() {
        // The host's Init handshake: `configure({})` must not fail.
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.provisioned());
        assert_eq!(c.ingest.topic, "gym/track");
    }
}
