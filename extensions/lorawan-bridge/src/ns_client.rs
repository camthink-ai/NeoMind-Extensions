//! Network Server MQTT and REST client.
//!
//! Connects to a LoRaWAN Network Server (ChirpStack or TTN) via MQTT to receive
//! uplink messages, and uses HTTP (ureq sync client) for downlink.

use crate::decoders::decode_cayenne_lpp;
use crate::types::{DecoderType, LoRaDevice, NsConfig, NsType};

use base64::Engine;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Outgoing, Packet, QoS};
use tokio::sync::RwLock;

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
    if hex_str.len() % 2 != 0 {
        return Err("Hex string has odd length".to_string());
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

        let (mqtt_client, eventloop) = AsyncClient::new(mqtt_opts, 10);

        // Determine the subscription topic based on NS type.
        let topic = match config.ns_type {
            NsType::Chirpstack => {
                format!(
                    "application/{}/device/+/event/up",
                    config.application_id
                )
            }
            NsType::Ttn => {
                format!(
                    "v3/{}/devices/{}/up",
                    config.application_id,
                    config.tenant_id.as_deref().unwrap_or("#")
                )
            }
        };

        mqtt_client
            .subscribe(&topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| format!("MQTT subscribe failed: {}", e))?;

        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let running_clone = running.clone();
        let ns_type = config.ns_type.clone();
        let default_decoder = config.default_decoder.clone();

        handle.spawn(async move {
            event_loop_runner(eventloop, running_clone, devices, ns_type, default_decoder).await;
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
    pub fn send_downlink(
        &self,
        dev_eui: &str,
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
            NsType::Ttn => self.send_ttn_downlink(api_url, dev_eui, &payload_b64, f_port, confirmed),
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
        if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", user, pass));
            req = req.header("Authorization", format!("Basic {}", encoded));
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
) {
    loop {
        if !running.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }

        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                let payload_str = match String::from_utf8(publish.payload.to_vec()) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let parsed: serde_json::Value = match serde_json::from_str(&payload_str) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let device = match ns_type {
                    NsType::Chirpstack => parse_chirpstack_uplink(&parsed, &default_decoder),
                    NsType::Ttn => parse_ttn_uplink(&parsed, &default_decoder),
                };

                if let Some(device) = device {
                    let dev_eui = device.dev_eui.clone();
                    let mut map = devices.write().await;
                    map.insert(dev_eui, device);
                }
            }
            Ok(Event::Incoming(Incoming::Disconnect)) => {
                break;
            }
            Ok(Event::Outgoing(Outgoing::PingReq)) => {
                // Normal keep-alive ping, ignore.
            }
            Ok(_) => {
                // Other incoming/outgoing events — ignore.
            }
            Err(e) => {
                // rumqttc will automatically reconnect, but log the error.
                eprintln!("[lorawan-bridge] MQTT event loop error: {}", e);
                // Brief back-off to avoid tight error loops.
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
) -> Option<LoRaDevice> {
    let dev_eui = msg.get("devEui")?.as_str()?.to_string();
    let f_cnt = msg.get("fCnt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // Decode payload fields.
    let fields = if let Some(object) = msg.get("object") {
        // The NS already decoded the payload into a JSON object.
        decode_object_fields(object)
    } else if let Some(data_b64) = msg.get("data").and_then(|v| v.as_str()) {
        // Raw base64 payload — decode locally.
        match base64_to_bytes(data_b64) {
            Ok(bytes) => match default_decoder {
                DecoderType::Cayenne => decode_cayenne_lpp(&bytes),
                DecoderType::Custom => {
                    // Custom decoder requires a per-device descriptor which we
                    // don't have at this point. Return empty fields.
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
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
        last_seen: chrono::Utc::now().timestamp_millis(),
        decoder_type: default_decoder.clone(),
        custom_decoder: None,
    })
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
) -> Option<LoRaDevice> {
    let dev_eui = msg
        .get("end_device_ids")
        .and_then(|ids| ids.get("device_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let uplink = msg.get("uplink_message")?;
    let f_cnt = uplink.get("f_cnt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    let fields = if let Some(decoded) = uplink.get("decoded_payload") {
        decode_object_fields(decoded)
    } else if let Some(data_b64) = uplink.get("frm_payload").and_then(|v| v.as_str()) {
        match base64_to_bytes(data_b64) {
            Ok(bytes) => match default_decoder {
                DecoderType::Cayenne => decode_cayenne_lpp(&bytes),
                DecoderType::Custom => Vec::new(),
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
        last_seen: chrono::Utc::now().timestamp_millis(),
        decoder_type: default_decoder.clone(),
        custom_decoder: None,
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
