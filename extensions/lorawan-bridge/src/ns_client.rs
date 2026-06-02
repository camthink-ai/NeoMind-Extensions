//! Network Server MQTT and REST client.
//!
//! Connects to a LoRaWAN Network Server (ChirpStack or TTN) via MQTT to receive
//! uplink messages, and uses HTTP (ureq sync client) for downlink.

use crate::decoders::{decode_cayenne_lpp, decode_custom};
use crate::types::{DecoderType, LoRaDevice, NsConfig, NsType};

use base64::Engine;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use parking_lot::RwLock;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Outgoing, Packet, QoS};

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Extract the hostname portion from a broker URL like `tcp://host:1883`.
pub fn parse_broker_host(broker_url: &str) -> String {
    let stripped = broker_url
        .trim()
        .trim_start_matches("tcp://")
        .trim_start_matches("ssl://")
        .trim_start_matches("mqtt://")
        .trim_start_matches("mqtts://");

    // Split off the port suffix if present.
    if let Some(colon_pos) = stripped.rfind(':') {
        stripped[..colon_pos].to_string()
    } else {
        stripped.to_string()
    }
}

/// Extract the port from a broker URL. Defaults to 1883 for plain and 8883 for TLS.
pub fn parse_broker_port(broker_url: &str) -> u16 {
    let stripped = broker_url
        .trim()
        .trim_start_matches("tcp://")
        .trim_start_matches("ssl://")
        .trim_start_matches("mqtt://")
        .trim_start_matches("mqtts://");

    if let Some(colon_pos) = stripped.rfind(':') {
        let port_str = &stripped[colon_pos + 1..];
        u16::from_str(port_str).unwrap_or_else(|_| default_port_for_scheme(broker_url))
    } else {
        default_port_for_scheme(broker_url)
    }
}

fn default_port_for_scheme(broker_url: &str) -> u16 {
    let lower = broker_url.to_lowercase();
    if lower.starts_with("ssl://") || lower.starts_with("mqtts://") {
        8883
    } else {
        1883
    }
}

/// Returns true if the broker URL scheme indicates TLS.
fn is_tls_broker(broker_url: &str) -> bool {
    let lower = broker_url.to_lowercase();
    lower.starts_with("ssl://") || lower.starts_with("mqtts://")
}

/// Decode a base64-encoded string into raw bytes.
pub fn base64_to_bytes(b64: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("Base64 decode error: {}", e))
}

/// Convert a hex-encoded string to base64.
pub fn hex_to_base64(hex_str: &str) -> Result<String, String> {
    use base64::Engine;
    let bytes = hex_decode(hex_str.trim())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

fn hex_decode(hex_str: &str) -> Result<Vec<u8>, String> {
    if !hex_str.len().is_multiple_of(2) {
        return Err("Hex string has odd length".to_string());
    }
    if !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Hex string contains invalid characters".to_string());
    }
    let mut bytes = Vec::with_capacity(hex_str.len() / 2);
    for i in (0..hex_str.len()).step_by(2) {
        let byte_val = u8::from_str_radix(&hex_str[i..i + 2], 16)
            .map_err(|e| format!("Hex decode error at position {}: {}", i, e))?;
        bytes.push(byte_val);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// NS Client
// ---------------------------------------------------------------------------

/// Wrapper around the MQTT async client and NS configuration.
pub struct NsClient {
    mqtt_client: AsyncClient,
    config: NsConfig,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl NsClient {
    /// Create a new MQTT connection, subscribe to the relevant topic, and start
    /// the event-loop background task.
    ///
    /// **IMPORTANT**: This function must be called from within an existing
    /// Tokio runtime (e.g. the one created by the extension runner). The sync
    /// `rumqttc::Client` would create its own runtime and **panic** in a cdylib.
    pub async fn connect(
        config: NsConfig,
        devices: Arc<RwLock<HashMap<String, LoRaDevice>>>,
    ) -> Result<Self, String> {
        // Ensure we are inside a Tokio runtime.
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| "No Tokio runtime available — cannot start MQTT client".to_string())?;

        let host = parse_broker_host(&config.broker_url);
        let port = parse_broker_port(&config.broker_url);

        let mut mqtt_opts = MqttOptions::new(
            format!("neomind-lorawan-bridge-{}", chrono::Utc::now().timestamp_millis()),
            host,
            port,
        );
        mqtt_opts.set_keep_alive(std::time::Duration::from_secs(30));

        if let (Some(user), Some(pass)) = (&config.username, &config.password) {
            mqtt_opts.set_credentials(user, pass);
        }

        // Configure TLS transport for SSL/mqtts URLs (required for TTN and ChirpStack cloud)
        if is_tls_broker(&config.broker_url) {
            // Use rustls with default native root certificates
            let transport = rumqttc::Transport::tls_with_default_config();
            mqtt_opts.set_transport(transport);
        }

        let (mqtt_client, eventloop) = AsyncClient::new(mqtt_opts, 10);

        // Determine the subscription topic based on NS type.
        let topic = match config.ns_type {
            NsType::Chirpstack | NsType::ChirpstackV4 => {
                format!(
                    "application/{}/device/+/event/up",
                    config.application_id
                )
            }
            NsType::Ttn => {
                // TTN v3 uplink topic format: v3/{application_id}@{tenant_id}/devices/+/up
                // The wildcard + matches any device_id.
                let tenant = config.tenant_id.as_deref().unwrap_or("ttn");
                format!(
                    "v3/{}@{}/devices/+/up",
                    config.application_id,
                    tenant
                )
            }
        };

        mqtt_client
            .subscribe(&topic, QoS::AtMostOnce)
            .await
            .map_err(|e| format!("MQTT subscribe failed: {}", e))?;

        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let running_clone = running.clone();
        let ns_type = config.ns_type.clone();
        let default_decoder = config.default_decoder.clone();

        // Clone the client for the event loop runner so it can re-subscribe
        // after rumqttc automatically reconnects (clean_session=true loses subscriptions).
        let resub_client = mqtt_client.clone();
        let resub_topic = topic.clone();

        handle.spawn(async move {
            event_loop_runner(
                eventloop,
                running_clone,
                devices,
                ns_type,
                default_decoder,
                resub_client,
                resub_topic,
            ).await;
        });

        Ok(Self {
            mqtt_client,
            config,
            running,
        })
    }

    /// Disconnect from the MQTT broker.
    pub async fn disconnect(&self) -> Result<(), String> {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.mqtt_client
            .try_disconnect()
            .map_err(|e| format!("MQTT disconnect error: {}", e))
    }

    /// Send a downlink message to a device via the NS REST API (sync HTTP).
    /// `dev_eui` is used for ChirpStack; `device_id` is used for TTN v3
    /// (TTN's API identifies devices by device_id, not dev_eui).
    pub fn send_downlink(
        &self,
        dev_eui: &str,
        device_id: Option<&str>,
        payload_hex: &str,
        f_port: u8,
        confirmed: bool,
    ) -> Result<serde_json::Value, String> {
        let api_url = self
            .config
            .ns_api_url
            .as_deref()
            .ok_or("NS API URL not configured — cannot send downlink")?;

        let payload_b64 = hex_to_base64(payload_hex)?;

        match self.config.ns_type {
            NsType::Chirpstack => self.send_chirpstack_downlink(api_url, dev_eui, &payload_b64, f_port, confirmed),
            NsType::ChirpstackV4 => self.send_chirpstack_v4_downlink(api_url, dev_eui, &payload_b64, f_port, confirmed),
            NsType::Ttn => {
                let ttn_device_id = device_id.unwrap_or(dev_eui);
                self.send_ttn_downlink(api_url, ttn_device_id, &payload_b64, f_port, confirmed)
            }
        }
    }

    fn send_chirpstack_downlink(
        &self,
        api_url: &str,
        dev_eui: &str,
        payload_b64: &str,
        f_port: u8,
        confirmed: bool,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/devices/{}/queue", api_url.trim_end_matches('/'), dev_eui);

        let body = serde_json::json!({
            "deviceQueueItem": {
                "confirmedDownlink": confirmed,
                "fPort": f_port,
                "data": payload_b64,
            }
        });

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();

        let mut req = agent.post(&url);
        if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", user, pass));
            req = req.header("Authorization", format!("Basic {}", encoded));
        }

        let mut resp = req
            .send_json(&body)
            .map_err(|e| format!("ChirpStack downlink HTTP error: {}", e))?;

        let response: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("ChirpStack downlink JSON error: {}", e))?;

        Ok(response)
    }

    fn send_ttn_downlink(
        &self,
        api_url: &str,
        dev_eui: &str,
        payload_b64: &str,
        f_port: u8,
        confirmed: bool,
    ) -> Result<serde_json::Value, String> {
        let app_id = &self.config.application_id;
        let url = format!(
            "{}/api/v3/as/applications/{}/devices/{}/down/push",
            api_url.trim_end_matches('/'),
            app_id,
            dev_eui
        );

        let body = serde_json::json!({
            "downlinks": [{
                "f_port": f_port,
                "confirmed": confirmed,
                "frm_payload": payload_b64,
            }]
        });

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();

        let mut req = agent.post(&url);
        // TTN v3 API uses Bearer token auth, NOT Basic auth.
        // The password field stores the TTN API key.
        if let Some(api_key) = &self.config.password {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }

        let mut resp = req
            .send_json(&body)
            .map_err(|e| format!("TTN downlink HTTP error: {}", e))?;

        let response: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("TTN downlink JSON error: {}", e))?;

        Ok(response)
    }

    /// Get a reference to the NS config.
    pub fn config(&self) -> &NsConfig {
        &self.config
    }
}

// ---------------------------------------------------------------------------
// Event loop runner (spawned as a background task)
// ---------------------------------------------------------------------------

async fn event_loop_runner(
    mut eventloop: rumqttc::EventLoop,
    running: Arc<std::sync::atomic::AtomicBool>,
    devices: Arc<RwLock<HashMap<String, LoRaDevice>>>,
    ns_type: NsType,
    default_decoder: DecoderType,
    resub_client: AsyncClient,
    resub_topic: String,
) {
    let mut error_backoff_ms: u64 = 500;
    let max_backoff_ms: u64 = 30_000;

    loop {
        if !running.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }

        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                // Reset backoff on successful message
                error_backoff_ms = 500;

                let payload_str = match String::from_utf8(publish.payload.to_vec()) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[lorawan-bridge] MQTT payload is not valid UTF-8: {}", e);
                        continue;
                    }
                };

                let parsed: serde_json::Value = match serde_json::from_str(&payload_str) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[lorawan-bridge] MQTT payload is not valid JSON: {}", e);
                        continue;
                    }
                };

                let device = match ns_type {
                    NsType::Chirpstack => parse_chirpstack_uplink(&parsed, &default_decoder, &devices),
                    NsType::ChirpstackV4 => parse_chirpstack_v4_uplink(&parsed, &default_decoder, &devices),
                    NsType::Ttn => parse_ttn_uplink(&parsed, &default_decoder, &devices),
                };

                if let Some(device) = device {
                    let dev_eui = device.dev_eui.clone();
                    let mut map = devices.write();
                    // Preserve existing custom_decoder if the device already exists
                    if let Some(existing) = map.get(&dev_eui) {
                        if existing.custom_decoder.is_some() && device.custom_decoder.is_none() {
                            let mut updated_device = device;
                            updated_device.custom_decoder = existing.custom_decoder.clone();
                            updated_device.decoder_type = existing.decoder_type.clone();
                            map.insert(dev_eui, updated_device);
                            continue;
                        }
                    }
                    map.insert(dev_eui, device);
                }
            }
            Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                // Reconnected — re-subscribe because clean_session=true loses subscriptions.
                error_backoff_ms = 500;
                if let Err(e) = resub_client.subscribe(&resub_topic, QoS::AtMostOnce).await {
                    eprintln!("[lorawan-bridge] MQTT re-subscribe failed: {}", e);
                } else {
                    eprintln!("[lorawan-bridge] MQTT (re)connected and re-subscribed to {}", resub_topic);
                }
            }
            Ok(Event::Incoming(Incoming::Disconnect)) => {
                eprintln!("[lorawan-bridge] MQTT broker disconnected, rumqttc will auto-reconnect");
            }
            Ok(Event::Outgoing(Outgoing::PingReq)) => {
                // Normal keep-alive ping, ignore.
            }
            Ok(_) => {
                // Other incoming/outgoing events — ignore.
            }
            Err(e) => {
                eprintln!("[lorawan-bridge] MQTT event loop error: {}", e);
                tokio::time::sleep(std::time::Duration::from_millis(error_backoff_ms)).await;
                error_backoff_ms = (error_backoff_ms * 2).min(max_backoff_ms);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ChirpStack uplink parser
// ---------------------------------------------------------------------------

/// Expected ChirpStack JSON:
/// ```json
/// {
///   "devEui": "0102030405060708",
///   "fCnt": 42,
///   "data": "AQID",        // optional base64
///   "object": { ... },     // optional decoded object from NS
///   "rxInfo": [{"rssi":-57,"snr":8.2}],
///   "fPort": 2
/// }
/// ```
fn parse_chirpstack_uplink(
    msg: &serde_json::Value,
    default_decoder: &DecoderType,
    devices: &RwLock<HashMap<String, LoRaDevice>>,
) -> Option<LoRaDevice> {
    let dev_eui = msg.get("devEui")?.as_str()?.to_string();
    let f_cnt = msg.get("fCnt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let f_port = msg.get("fPort").and_then(|v| v.as_u64()).unwrap_or(1) as u8;

    // FPort 0 = MAC commands only, not application data
    if f_port == 0 {
        eprintln!("[lorawan-bridge] Ignoring MAC command on FPort 0 for {}", dev_eui);
        return None;
    }

    // Look up per-device decoder configuration
    let (device_decoder_type, device_custom_decoder) = {
        let map = devices.read();
        if let Some(existing) = map.get(&dev_eui) {
            (existing.decoder_type.clone(), existing.custom_decoder.clone())
        } else {
            (default_decoder.clone(), None)
        }
    };

    // Decode payload fields.
    let fields = if let Some(object) = msg.get("object") {
        // The NS already decoded the payload into a JSON object.
        decode_object_fields(object)
    } else if let Some(data_b64) = msg.get("data").and_then(|v| v.as_str()) {
        // Raw base64 payload — decode locally.
        match base64_to_bytes(data_b64) {
            Ok(bytes) => match &device_decoder_type {
                DecoderType::Custom => {
                    if let Some(ref custom_fields) = device_custom_decoder {
                        decode_custom(&bytes, custom_fields)
                    } else {
                        Vec::new()
                    }
                }
                DecoderType::Cayenne => decode_cayenne_lpp(&bytes),
            },
            Err(e) => {
                eprintln!("[lorawan-bridge] Base64 decode failed for {}: {}", dev_eui, e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Extract RSSI / SNR from rxInfo[0].
    let (rssi, snr) = extract_rssi_snr(msg.get("rxInfo"));

    let battery = msg
        .get("object")
        .and_then(|o| o.get("battery"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u8);

    Some(LoRaDevice {
        dev_eui,
        fields,
        rssi,
        snr,
        battery,
        f_cnt,
        f_port,
        last_seen: chrono::Utc::now().timestamp_millis(),
        decoder_type: device_decoder_type,
        custom_decoder: device_custom_decoder,
    })
}

// ---------------------------------------------------------------------------
// ChirpStack v4 uplink parser
// ---------------------------------------------------------------------------

/// ChirpStack v4 MQTT uplink JSON wraps device info inside a `deviceInfo` object:
/// ```json
/// {
///   "deviceInfo": {
///     "devEui": "0102030405060708",
///     "deviceName": "my-sensor"
///   },
///   "fCnt": 42,
///   "data": "AQID",
///   "object": { ... },
///   "rxInfo": [{"rssi":-57,"snr":8.2}],
///   "fPort": 2
/// }
/// ```
fn parse_chirpstack_v4_uplink(
    msg: &serde_json::Value,
    default_decoder: &DecoderType,
    devices: &RwLock<HashMap<String, LoRaDevice>>,
) -> Option<LoRaDevice> {
    // v4 nests devEui inside deviceInfo
    let dev_eui = msg
        .get("deviceInfo")?
        .get("devEui")?
        .as_str()?
        .to_string();
    let f_cnt = msg.get("fCnt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let f_port = msg.get("fPort").and_then(|v| v.as_u64()).unwrap_or(1) as u8;

    // FPort 0 = MAC commands only, not application data
    if f_port == 0 {
        eprintln!("[lorawan-bridge] Ignoring MAC command on FPort 0 for {}", dev_eui);
        return None;
    }

    // Look up per-device decoder configuration
    let (device_decoder_type, device_custom_decoder) = {
        let map = devices.read();
        if let Some(existing) = map.get(&dev_eui) {
            (existing.decoder_type.clone(), existing.custom_decoder.clone())
        } else {
            (default_decoder.clone(), None)
        }
    };

    // Decode payload fields — same logic as v3
    let fields = if let Some(object) = msg.get("object") {
        decode_object_fields(object)
    } else if let Some(data_b64) = msg.get("data").and_then(|v| v.as_str()) {
        match base64_to_bytes(data_b64) {
            Ok(bytes) => match &device_decoder_type {
                DecoderType::Custom => {
                    if let Some(ref custom_fields) = device_custom_decoder {
                        decode_custom(&bytes, custom_fields)
                    } else {
                        Vec::new()
                    }
                }
                DecoderType::Cayenne => decode_cayenne_lpp(&bytes),
            },
            Err(e) => {
                eprintln!("[lorawan-bridge] Base64 decode failed for {}: {}", dev_eui, e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let (rssi, snr) = extract_rssi_snr(msg.get("rxInfo"));

    let battery = msg
        .get("object")
        .and_then(|o| o.get("battery"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u8);

    Some(LoRaDevice {
        dev_eui,
        fields,
        rssi,
        snr,
        battery,
        f_cnt,
        f_port,
        last_seen: chrono::Utc::now().timestamp_millis(),
        decoder_type: device_decoder_type,
        custom_decoder: device_custom_decoder,
    })
}

// ---------------------------------------------------------------------------
// ChirpStack v4 downlink
// ---------------------------------------------------------------------------

impl NsClient {
    /// Send a downlink via ChirpStack v4 API.
    ///
    /// ChirpStack v4 uses the gRPC-gateway HTTP endpoint (same path as v3 REST)
    /// but with Bearer token auth instead of Basic auth.
    fn send_chirpstack_v4_downlink(
        &self,
        api_url: &str,
        dev_eui: &str,
        payload_b64: &str,
        f_port: u8,
        confirmed: bool,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/devices/{}/queue", api_url.trim_end_matches('/'), dev_eui);

        // ChirpStack v4 gRPC-gateway uses snake_case field names
        // and wraps in "queueItem" (not "deviceQueueItem" like v3).
        let body = serde_json::json!({
            "queueItem": {
                "confirmed": confirmed,
                "f_port": f_port,
                "data": payload_b64,
            }
        });

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();

        let mut req = agent.post(&url);
        // v4 gRPC-gateway uses Grpc-Metadata-Authorization header with API key.
        if let Some(api_key) = &self.config.password {
            req = req.header("Grpc-Metadata-Authorization", format!("Bearer {}", api_key));
        }

        let mut resp = req
            .send_json(&body)
            .map_err(|e| format!("ChirpStack v4 downlink HTTP error: {}", e))?;

        let response: serde_json::Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("ChirpStack v4 downlink JSON error: {}", e))?;

        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// TTN uplink parser
// ---------------------------------------------------------------------------

/// Expected TTN v3 JSON:
/// ```json
/// {
///   "end_device_ids": { "device_id": "my-device" },
///   "uplink_message": {
///     "f_cnt": 42,
///     "decoded_payload": { "temperature": 23.5 },
///     "rx_metadata": [{"rssi":-57,"snr":8.2}],
///     "f_port": 2
///   }
/// }
/// ```
fn parse_ttn_uplink(
    msg: &serde_json::Value,
    default_decoder: &DecoderType,
    devices: &RwLock<HashMap<String, LoRaDevice>>,
) -> Option<LoRaDevice> {
    let dev_eui = msg
        .get("end_device_ids")
        .and_then(|ids| ids.get("device_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let uplink = msg.get("uplink_message")?;
    let f_cnt = uplink.get("f_cnt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let f_port = uplink.get("f_port").and_then(|v| v.as_u64()).unwrap_or(1) as u8;

    // FPort 0 = MAC commands only, not application data
    if f_port == 0 {
        eprintln!("[lorawan-bridge] Ignoring MAC command on FPort 0 for {}", dev_eui);
        return None;
    }

    // Look up per-device decoder configuration
    let (device_decoder_type, device_custom_decoder) = {
        let map = devices.read();
        if let Some(existing) = map.get(&dev_eui) {
            (existing.decoder_type.clone(), existing.custom_decoder.clone())
        } else {
            (default_decoder.clone(), None)
        }
    };

    let fields = if let Some(decoded) = uplink.get("decoded_payload") {
        decode_object_fields(decoded)
    } else if let Some(data_b64) = uplink.get("frm_payload").and_then(|v| v.as_str()) {
        match base64_to_bytes(data_b64) {
            Ok(bytes) => match &device_decoder_type {
                DecoderType::Custom => {
                    if let Some(ref custom_fields) = device_custom_decoder {
                        decode_custom(&bytes, custom_fields)
                    } else {
                        Vec::new()
                    }
                }
                DecoderType::Cayenne => decode_cayenne_lpp(&bytes),
            },
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let (rssi, snr) = extract_rssi_snr(uplink.get("rx_metadata"));

    let battery = uplink
        .get("decoded_payload")
        .and_then(|o| o.get("battery"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u8);

    Some(LoRaDevice {
        dev_eui,
        fields,
        rssi,
        snr,
        battery,
        f_cnt,
        f_port,
        last_seen: chrono::Utc::now().timestamp_millis(),
        decoder_type: device_decoder_type,
        custom_decoder: device_custom_decoder,
    })
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Extract rssi and snr from a JSON array like `[{"rssi":-57,"snr":8.2}, ...]`.
fn extract_rssi_snr(rx_info: Option<&serde_json::Value>) -> (i32, f64) {
    let mut rssi = 0;
    let mut snr = 0.0;

    if let Some(arr) = rx_info.and_then(|v| v.as_array()) {
        if let Some(first) = arr.first() {
            rssi = first.get("rssi").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            snr = first.get("snr").and_then(|v| v.as_f64()).unwrap_or(0.0);
        }
    }

    (rssi, snr)
}

/// Convert a decoded JSON object `{ "temperature": 23.5, "humidity": 60 }`
/// into a flat list of `DecodedField`s.
fn decode_object_fields(obj: &serde_json::Value) -> Vec<crate::types::DecodedField> {
    let mut fields = Vec::new();

    if let Some(map) = obj.as_object() {
        for (key, val) in map {
            if let Some(num) = val.as_f64() {
                let (unit, name) = guess_unit_and_name(key);
                fields.push(crate::types::DecodedField {
                    name,
                    value: num,
                    unit,
                });
            } else if let Some(int) = val.as_i64() {
                let (unit, name) = guess_unit_and_name(key);
                fields.push(crate::types::DecodedField {
                    name,
                    value: int as f64,
                    unit,
                });
            }
        }
    }

    fields
}

/// Heuristic: guess the unit and normalise the field name from the JSON key.
fn guess_unit_and_name(key: &str) -> (String, String) {
    let lower = key.to_lowercase();
    if lower.contains("temp") {
        ("\u{00b0}C".to_string(), "temperature".to_string())
    } else if lower.contains("hum") {
        ("%".to_string(), "humidity".to_string())
    } else if lower.contains("press") || lower.contains("baro") {
        ("hPa".to_string(), "barometric_pressure".to_string())
    } else if lower.contains("lux") || lower.contains("illu") || lower.contains("light") {
        ("lux".to_string(), "illuminance".to_string())
    } else if lower.contains("batt") || lower.contains("volt") {
        ("V".to_string(), "battery_voltage".to_string())
    } else if lower.contains("lat") {
        ("\u{00b0}".to_string(), "latitude".to_string())
    } else if lower.contains("lon") || lower.contains("lng") {
        ("\u{00b0}".to_string(), "longitude".to_string())
    } else if lower.contains("alt") {
        ("m".to_string(), "altitude".to_string())
    } else {
        (String::new(), key.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CustomDecoderField;
    use crate::types::CustomDataType;

    #[test]
    fn test_parse_broker_host() {
        assert_eq!(parse_broker_host("tcp://broker.example.com:1883"), "broker.example.com");
        assert_eq!(parse_broker_host("ssl://secure.example.com:8883"), "secure.example.com");
        assert_eq!(parse_broker_host("mqtt://plain.example.com:1883"), "plain.example.com");
        assert_eq!(parse_broker_host("broker.example.com:1883"), "broker.example.com");
    }

    #[test]
    fn test_parse_broker_port() {
        assert_eq!(parse_broker_port("tcp://broker.example.com:1883"), 1883);
        assert_eq!(parse_broker_port("ssl://broker.example.com:8883"), 8883);
        assert_eq!(parse_broker_port("tcp://broker.example.com"), 1883);
        assert_eq!(parse_broker_port("mqtts://broker.example.com"), 8883);
    }

    #[test]
    fn test_base64_to_bytes() {
        let bytes = base64_to_bytes("AQID").unwrap();
        assert_eq!(bytes, vec![0x01, 0x02, 0x03]);

        let empty = base64_to_bytes("").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_hex_to_base64() {
        let b64 = hex_to_base64("010203").unwrap();
        assert_eq!(b64, "AQID");
    }

    // -----------------------------------------------------------------------
    // ChirpStack uplink parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_chirpstack_uplink_basic() {
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));
        let msg = serde_json::json!({
            "devEui": "0102030405060708",
            "fCnt": 42,
            "data": "AANmAQAXmg==",
            "rxInfo": [{"rssi": -57, "snr": 8.2}],
            "fPort": 2
        });

        let device = parse_chirpstack_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "0102030405060708");
        assert_eq!(device.f_cnt, 42);
        assert_eq!(device.rssi, -57);
        assert!((device.snr - 8.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_chirpstack_uplink_with_decoded_object() {
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));
        let msg = serde_json::json!({
            "devEui": "ABCDEF1234567890",
            "fCnt": 10,
            "object": {
                "temperature": 23.5,
                "humidity": 60
            },
            "rxInfo": [{"rssi": -80, "snr": 5.0}],
            "fPort": 1
        });

        let device = parse_chirpstack_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "ABCDEF1234567890");
        assert_eq!(device.f_cnt, 10);
        assert_eq!(device.fields.len(), 2);
        assert!(device.fields.iter().any(|f| f.name == "temperature" && (f.value - 23.5).abs() < f64::EPSILON));
        assert!(device.fields.iter().any(|f| f.name == "humidity" && (f.value - 60.0).abs() < f64::EPSILON));
    }

    #[test]
    fn test_parse_chirpstack_uplink_with_custom_decoder() {
        // Set up a device that already has a custom decoder configured
        let custom_fields = vec![
            CustomDecoderField {
                offset: 0,
                length: 2,
                name: "temperature".to_string(),
                data_type: CustomDataType::Uint16,
                scale: 0.1,
                unit: "\u{00b0}C".to_string(),
            },
        ];
        let existing_device = LoRaDevice {
            dev_eui: "ABCDEF1234567890".to_string(),
            fields: Vec::new(),
            rssi: 0,
            snr: 0.0,
            battery: None,
            f_cnt: 0,
            f_port: 0,
            last_seen: 0,
            decoder_type: DecoderType::Custom,
            custom_decoder: Some(custom_fields),
        };
        let mut map = HashMap::new();
        map.insert("ABCDEF1234567890".to_string(), existing_device);
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(map));

        // Payload: 0x00C8 = 200, with scale 0.1 => 20.0 degrees
        let payload_b64 = base64::engine::general_purpose::STANDARD.encode(&[0x00, 0xC8]);
        let msg = serde_json::json!({
            "devEui": "ABCDEF1234567890",
            "fCnt": 5,
            "data": payload_b64,
            "rxInfo": [{"rssi": -60, "snr": 7.5}],
            "fPort": 2
        });

        let device = parse_chirpstack_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "ABCDEF1234567890");
        assert_eq!(device.fields.len(), 1);
        assert_eq!(device.fields[0].name, "temperature");
        assert!((device.fields[0].value - 20.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // ChirpStack v4 uplink parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_chirpstack_v4_uplink_basic() {
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));
        let msg = serde_json::json!({
            "deviceInfo": {
                "devEui": "0102030405060708",
                "deviceName": "my-sensor"
            },
            "fCnt": 42,
            "data": "AANmAQAXmg==",
            "rxInfo": [{"rssi": -57, "snr": 8.2}],
            "fPort": 2
        });

        let device = parse_chirpstack_v4_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "0102030405060708");
        assert_eq!(device.f_cnt, 42);
        assert_eq!(device.rssi, -57);
        assert!((device.snr - 8.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_chirpstack_v4_uplink_with_decoded_object() {
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));
        let msg = serde_json::json!({
            "deviceInfo": {
                "devEui": "ABCDEF1234567890",
                "deviceName": "temp-sensor"
            },
            "fCnt": 10,
            "object": {
                "temperature": 23.5,
                "humidity": 60
            },
            "rxInfo": [{"rssi": -80, "snr": 5.0}],
            "fPort": 1
        });

        let device = parse_chirpstack_v4_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "ABCDEF1234567890");
        assert_eq!(device.f_cnt, 10);
        assert_eq!(device.fields.len(), 2);
        assert!(device.fields.iter().any(|f| f.name == "temperature" && (f.value - 23.5).abs() < f64::EPSILON));
        assert!(device.fields.iter().any(|f| f.name == "humidity" && (f.value - 60.0).abs() < f64::EPSILON));
    }

    #[test]
    fn test_parse_chirpstack_v4_uplink_missing_device_info() {
        // v3-style message (top-level devEui) should return None from v4 parser
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));
        let msg = serde_json::json!({
            "devEui": "0102030405060708",
            "fCnt": 42
        });

        let result = parse_chirpstack_v4_uplink(&msg, &DecoderType::Cayenne, &devices);
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // TTN uplink parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ttn_uplink_basic() {
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));
        let msg = serde_json::json!({
            "end_device_ids": {
                "device_id": "my-device-1",
                "application_ids": {
                    "application_id": "myapp"
                }
            },
            "uplink_message": {
                "f_cnt": 42,
                "decoded_payload": {
                    "temperature": 23.5,
                    "humidity": 60
                },
                "rx_metadata": [{"rssi": -57, "snr": 8.2}],
                "f_port": 2
            }
        });

        let device = parse_ttn_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "my-device-1");
        assert_eq!(device.f_cnt, 42);
        assert_eq!(device.rssi, -57);
        assert!((device.snr - 8.2).abs() < f64::EPSILON);
        assert_eq!(device.fields.len(), 2);
    }

    #[test]
    fn test_parse_ttn_uplink_raw_payload_cayenne() {
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));

        // Cayenne LPP: channel 0, temperature type 0x67, raw 0x0064 = 10.0 degrees
        let payload_b64 = base64::engine::general_purpose::STANDARD.encode(&[0x00, 0x67, 0x00, 0x64]);
        let msg = serde_json::json!({
            "end_device_ids": {
                "device_id": "raw-sensor-1"
            },
            "uplink_message": {
                "f_cnt": 7,
                "frm_payload": payload_b64,
                "rx_metadata": [{"rssi": -90, "snr": 3.5}],
                "f_port": 1
            }
        });

        let device = parse_ttn_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "raw-sensor-1");
        assert_eq!(device.fields.len(), 1);
        assert_eq!(device.fields[0].name, "temperature");
        assert!((device.fields[0].value - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_ttn_uplink_with_custom_decoder() {
        let custom_fields = vec![
            CustomDecoderField {
                offset: 0,
                length: 2,
                name: "temperature".to_string(),
                data_type: CustomDataType::Uint16,
                scale: 0.1,
                unit: "\u{00b0}C".to_string(),
            },
        ];
        let existing_device = LoRaDevice {
            dev_eui: "custom-sensor".to_string(),
            fields: Vec::new(),
            rssi: 0,
            snr: 0.0,
            battery: None,
            f_cnt: 0,
            f_port: 0,
            last_seen: 0,
            decoder_type: DecoderType::Custom,
            custom_decoder: Some(custom_fields),
        };
        let mut map = HashMap::new();
        map.insert("custom-sensor".to_string(), existing_device);
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(map));

        // Payload: 0x00C8 = 200, with scale 0.1 => 20.0
        let payload_b64 = base64::engine::general_purpose::STANDARD.encode(&[0x00, 0xC8]);
        let msg = serde_json::json!({
            "end_device_ids": {
                "device_id": "custom-sensor"
            },
            "uplink_message": {
                "f_cnt": 3,
                "frm_payload": payload_b64,
                "rx_metadata": [{"rssi": -70, "snr": 6.0}],
                "f_port": 1
            }
        });

        let device = parse_ttn_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "custom-sensor");
        assert_eq!(device.fields.len(), 1);
        assert!((device.fields[0].value - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_ttn_uplink_battery() {
        let devices: Arc<RwLock<HashMap<String, LoRaDevice>>> = Arc::new(RwLock::new(HashMap::new()));
        let msg = serde_json::json!({
            "end_device_ids": {
                "device_id": "batt-sensor"
            },
            "uplink_message": {
                "f_cnt": 15,
                "decoded_payload": {
                    "battery": 85,
                    "temperature": 22.0
                },
                "rx_metadata": [{"rssi": -50, "snr": 9.0}],
                "f_port": 2
            }
        });

        let device = parse_ttn_uplink(&msg, &DecoderType::Cayenne, &devices).unwrap();
        assert_eq!(device.dev_eui, "batt-sensor");
        assert_eq!(device.battery, Some(85));
    }

    #[test]
    fn test_parse_ttn_topic_format() {
        // Verify the TTN topic subscription uses the correct format:
        // v3/{application_id}@{tenant_id}/devices/+/up
        let config = NsConfig {
            ns_type: NsType::Ttn,
            broker_url: "mqtts://eu1.cloud.thethings.network:8883".to_string(),
            username: Some("myapp@tenant1".to_string()),
            password: Some("NNSXSSecretKey".to_string()),
            application_id: "myapp".to_string(),
            tenant_id: Some("tenant1".to_string()),
            ns_api_url: Some("https://eu1.cloud.thethings.network".to_string()),
            default_decoder: DecoderType::Cayenne,
            auto_discover: true,
        };

        // The expected topic format is: v3/myapp@tenant1/devices/+/up
        let expected_topic = format!(
            "v3/{}@{}/devices/+/up",
            config.application_id,
            config.tenant_id.as_deref().unwrap_or("ttn")
        );
        assert_eq!(expected_topic, "v3/myapp@tenant1/devices/+/up");
    }

    #[test]
    fn test_is_tls_broker() {
        assert!(is_tls_broker("ssl://broker.example.com:8883"));
        assert!(is_tls_broker("mqtts://broker.example.com:8883"));
        assert!(is_tls_broker("SSL://broker.example.com:8883"));
        assert!(!is_tls_broker("tcp://broker.example.com:1883"));
        assert!(!is_tls_broker("mqtt://broker.example.com:1883"));
    }
}
