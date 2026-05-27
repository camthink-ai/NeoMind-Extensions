# LoRaWAN Bridge Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a NeoMind extension that bridges LoRaWAN Network Servers (ChirpStack, The Things Stack) into NeoMind, auto-discovering and registering LoRa sensors as devices.

**Architecture:** Async MQTT client (rumqttc AsyncClient) connects to NS broker. EventLoop polled in a background task spawned on NeoMind's host Tokio runtime. Payload decoded (Cayenne LPP + custom). Devices auto-registered on first uplink. Downlink via NS HTTP API (sync ureq).

**Tech Stack:** Rust, rumqttc 0.25, ureq 3, neomind-extension-sdk 0.6.3, React 18 + Vite (frontend)

**Spec:** `docs/superpowers/specs/2026-05-27-device-ecosystem-extensions-design.md` Section 2

---

## Critical: Async Runtime Pattern

rumqttc's sync `Client` creates its own runtime internally — **this panics inside a cdylib**. We MUST use `AsyncClient` and borrow NeoMind's Tokio runtime:

```rust
// Get host runtime handle (NeoMind SDK uses Tokio internally)
let handle = tokio::runtime::Handle::try_current()
    .expect("No Tokio runtime available");

// AsyncClient::new() does NOT create a runtime
let (client, eventloop) = AsyncClient::new(mqtt_options, 10);

// Spawn eventloop poller on host runtime
handle.spawn(async move {
    loop {
        match eventloop.poll().await {
            Ok(event) => handle_event(event),
            Err(e) => { /* auto-reconnect on next poll */ }
        }
    }
});
```

---

## File Structure

```
extensions/lorawan-bridge/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Extension struct, Extension trait impl, FFI export
│   ├── ns_client.rs        # NS MQTT connection, event loop, topic handling
│   ├── decoders.rs         # Cayenne LPP decoder + custom binary decoder
│   └── types.rs            # LoRaDevice, NsConfig, decoder types
├── metadata.json           # Auto-generated
└── frontend/
    ├── frontend.json
    ├── vite.config.ts
    ├── tsconfig.json
    ├── package.json
    └── src/
        └── index.tsx        # LoRaWANDeviceCard component
```

---

### Task 1: Project Scaffold

**Files:**
- Create: `extensions/lorawan-bridge/Cargo.toml`
- Modify: `Cargo.toml` (root workspace — add member)

- [ ] **Step 1: Create extension Cargo.toml**

```toml
[package]
name = "lorawan-bridge"
version = "2.7.1"
edition = "2021"
authors = ["NeoMind Team"]
license = "Apache-2.0"
description = "LoRaWAN bridge extension for NeoMind — connect ChirpStack/TTN sensors with auto-discovery"

[lib]
name = "neomind_extension_lorawan_bridge"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = "0.1"
chrono = "0.4"
hex = "0.4"

# MQTT client — AsyncClient only (sync Client panics in cdylib)
rumqttc = "0.25"

# HTTP client for NS REST API (downlink commands)
ureq = { version = "3", features = ["json"] }

# Tokio — required by SDK + for spawning async tasks on host runtime
tokio = { version = "1", features = ["rt", "sync"] }

[features]
default = []
native = []
wasm = []
```

- [ ] **Step 2: Add to workspace members**

Add `"extensions/lorawan-bridge"` to `members` in root `Cargo.toml`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p lorawan-bridge`
Expected: Compiles (warnings about empty lib.rs)

- [ ] **Step 4: Commit**

```bash
git add extensions/lorawan-bridge/Cargo.toml Cargo.toml
git commit -m "feat(lorawan-bridge): scaffold extension project"
```

---

### Task 2: Types and Payload Decoders

**Files:**
- Create: `extensions/lorawan-bridge/src/types.rs`
- Create: `extensions/lorawan-bridge/src/decoders.rs`
- Create: `extensions/lorawan-bridge/src/lib.rs` (minimal stub)

- [ ] **Step 1: Create types.rs**

```rust
use serde::{Deserialize, Serialize};

/// Network Server type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NsType {
    Chirpstack,
    Ttn,
}

/// NS connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NsConfig {
    pub ns_type: NsType,
    pub broker_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub application_id: String,
    pub tenant_id: Option<String>,       // Required for TTN
    pub ns_api_url: Option<String>,       // For downlink via HTTP API
    pub default_decoder: DecoderType,
    pub auto_discover: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoderType {
    Cayenne,
    Custom,
}

/// A decoded LoRa sensor field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedField {
    pub name: String,
    pub value: f64,
    pub unit: String,
}

/// Custom decoder field definition
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomDataType {
    Uint8,
    Uint16,
    Int16,
    Uint32,
    Int32,
}

/// Internal state for a discovered LoRa device
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
```

- [ ] **Step 2: Create decoders.rs — Cayenne LPP decoder**

Cayenne LPP spec: each data point is `[channel, type_code, data_bytes...]`. Decode common types:

| Type Code | Size | Field | Unit |
|-----------|------|-------|------|
| 0x00 | 1 | digital_input | - |
| 0x01 | 1 | digital_output | - |
| 0x02 | 2 | analog_in | V |
| 0x67 | 2 | temperature | °C (×0.1) |
| 0x68 | 1 | humidity | % |
| 0x73 | 2 | barometer | hPa (×0.1) |
| 0x65 | 2 | illuminance | lux |
| 0x88 | 9 | gps (lat/lng/alt) | - |

Implement a `decode_cayenne_lpp(payload: &[u8]) -> Vec<DecodedField>` function that iterates through the payload, matches type codes, and decodes values.

Also implement `decode_custom(payload: &[u8], fields: &[CustomDecoderField]) -> Vec<DecodedField>` for user-defined decoders.

- [ ] **Step 3: Write tests for decoders**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cayenne_temperature() {
        // Channel 0, type 0x67 (temp), value 235 = 23.5°C
        let payload = [0x00, 0x67, 0x00, 0xEB];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "temperature_0");
        assert!((fields[0].value - 23.5).abs() < 0.01);
    }

    #[test]
    fn test_cayenne_humidity() {
        // Channel 0, type 0x68 (humidity), value 65%
        let payload = [0x00, 0x68, 0x41];
        let fields = decode_cayenne_lpp(&payload);
        assert_eq!(fields.len(), 1);
        assert!((fields[0].value - 65.0).abs() < 0.01);
    }

    #[test]
    fn test_custom_decoder() {
        let payload: Vec<u8> = vec![0x02, 0x8A, 0xFF, 0x9C, 0x5A]; // 650, -100, 90
        let fields = vec![
            CustomDecoderField { offset: 0, length: 2, name: "soil_moisture".into(), data_type: CustomDataType::Uint16, scale: 0.01, unit: "%".into() },
            CustomDecoderField { offset: 2, length: 2, name: "temperature".into(), data_type: CustomDataType::Int16, scale: 0.1, unit: "°C".into() },
            CustomDecoderField { offset: 4, length: 1, name: "battery".into(), data_type: CustomDataType::Uint8, scale: 0.0, unit: "%".into() },
        ];
        let decoded = decode_custom(&payload, &fields);
        assert!((decoded[0].value - 6.50).abs() < 0.01);  // 650 * 0.01
        assert!((decoded[1].value - (-10.0)).abs() < 0.1); // -100 * 0.1
        assert!((decoded[2].value - 90.0).abs() < 0.01);
    }
}
```

- [ ] **Step 4: Create lib.rs stub**

```rust
pub mod types;
pub mod decoders;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p lorawan-bridge`
Expected: 3 tests pass

- [ ] **Step 6: Commit**

```bash
git add extensions/lorawan-bridge/src/
git commit -m "feat(lorawan-bridge): add types, Cayenne LPP and custom decoders with tests"
```

---

### Task 3: NS Client (MQTT Connection + Event Loop)

**Files:**
- Create: `extensions/lorawan-bridge/src/ns_client.rs`
- Modify: `extensions/lorawan-bridge/src/lib.rs` (add mod)

- [ ] **Step 1: Create ns_client.rs**

Core architecture:
- `NsClient` struct holds `AsyncClient` + shared device state
- `connect()` creates `AsyncClient`, subscribes to NS topics, spawns event loop
- Event loop runs in a `handle.spawn()` task on NeoMind's Tokio runtime
- Incoming Publish events are parsed per NS type (ChirpStack vs TTN)
- Decoded data updates shared device state (Arc<RwLock<HashMap>>)

```rust
use crate::decoders::{decode_cayenne_lpp, decode_custom};
use crate::types::*;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Packet, QoS, Transport};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct NsClient {
    config: NsConfig,
    client: Option<AsyncClient>,
    devices: Arc<RwLock<HashMap<String, LoRaDevice>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl NsClient {
    pub fn new(config: NsConfig) -> Self {
        Self {
            config,
            client: None,
            devices: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub async fn connect(&mut self) -> Result<(), String> {
        let host = parse_broker_host(&self.config.broker_url)?;
        let port = parse_broker_port(&self.config.broker_url)?;

        let mut opts = MqttOptions::new(
            format!("neomind-lorawan-{}", self.config.application_id),
            host,
            port,
        );
        opts.set_keep_alive(std::time::Duration::from_secs(30));

        if let (Some(user), Some(pass)) = (&self.config.username, &self.config.password) {
            opts.set_credentials(user, pass);
        }

        // TLS if port 8883 or url starts with ssl/tls/mqtts
        if port == 8883 {
            opts.set_transport(Transport::tls_with_default_config());
        }

        let (client, eventloop) = AsyncClient::new(opts, 10);

        // Subscribe to uplink topics based on NS type
        let topic = match self.config.ns_type {
            NsType::Chirpstack => format!(
                "application/{}/device/+/event/up",
                self.config.application_id
            ),
            NsType::Ttn => format!(
                "v3/{}@{}/devices/+/up",
                self.config.application_id,
                self.config.tenant_id.as_deref().unwrap_or("tenant")
            ),
        };
        client.subscribe(&topic, QoS::AtLeastOnce).await
            .map_err(|e| format!("Subscribe error: {}", e))?;

        // Also subscribe to status events (battery, margin)
        let status_topic = match self.config.ns_type {
            NsType::Chirpstack => format!(
                "application/{}/device/+/event/status",
                self.config.application_id
            ),
            NsType::Ttn => format!(
                "v3/{}@{}/devices/+/up",
                self.config.application_id,
                self.config.tenant_id.as_deref().unwrap_or("tenant")
            ),
        };
        client.subscribe(&status_topic, QoS::AtLeastOnce).await
            .map_err(|e| format!("Status subscribe error: {}", e))?;

        // Spawn event loop on host runtime
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|e| format!("No Tokio runtime: {}", e))?;

        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        let devices = self.devices.clone();
        let running = self.running.clone();
        let ns_type = self.config.ns_type.clone();

        handle.spawn(async move {
            event_loop_runner(eventloop, devices, running, ns_type).await;
        });

        self.client = Some(client);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(client) = self.client.take() {
            let _ = client.try_disconnect();
        }
    }

    pub async fn get_devices(&self) -> Vec<LoRaDevice> {
        let devices = self.devices.read().await;
        devices.values().cloned().collect()
    }

    pub async fn get_device(&self, dev_eui: &str) -> Option<LoRaDevice> {
        let devices = self.devices.read().await;
        devices.get(dev_eui).cloned()
    }

    /// Send downlink via NS HTTP API
    pub fn send_downlink(&self, dev_eui: &str, payload_hex: &str, f_port: u8) -> Result<(), String> {
        let api_url = self.config.ns_api_url.as_deref()
            .ok_or("NS API URL not configured")?;
        // Use ureq (sync) to call NS REST API
        // Implementation depends on NS type (ChirpStack vs TTN)
        // For ChirpStack: POST /api/devices/{dev_eui}/queue
        let payload_base64 = hex_to_base64(payload_hex)?;
        let body = serde_json::json!({
            "devEui": dev_eui,
            "fPort": f_port,
            "data": payload_base64,
            "confirmed": false,
        });
        ureq::post(&format!("{}/api/devices/{}/queue", api_url, dev_eui))
            .send_json(&body)
            .map_err(|e| format!("Downlink API error: {}", e))?;
        Ok(())
    }
}

async fn event_loop_runner(
    mut eventloop: rumqttc::EventLoop,
    devices: Arc<RwLock<HashMap<String, LoRaDevice>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
    ns_type: NsType,
) {
    while running.load(std::sync::atomic::Ordering::SeqCst) {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                match ns_type {
                    NsType::Chirpstack => handle_chirpstack_uplink(&publish, &devices).await,
                    NsType::Ttn => handle_ttn_uplink(&publish, &devices).await,
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("LoRaWAN MQTT error: {:?}", e);
                // poll() continues — auto-reconnect
            }
        }
    }
}

async fn handle_chirpstack_uplink(
    publish: &rumqttc::Publish,
    devices: &Arc<RwLock<HashMap<String, LoRaDevice>>>,
) {
    let payload: serde_json::Value = match serde_json::from_slice(&publish.payload) {
        Ok(v) => v,
        Err(_) => return,
    };

    let dev_eui = match payload.get("devEui").and_then(|v| v.as_str()) {
        Some(eui) => eui.to_string(),
        None => return,
    };

    // Extract RSSI/SNR from rxInfo
    let rssi = payload.pointer("/rxInfo/0/rssi")
        .and_then(|v| v.as_i64()).unwrap_or(-100) as i32;
    let snr = payload.pointer("/rxInfo/0/snr")
        .and_then(|v| v.as_f64()).unwrap_or(0.0);
    let f_cnt = payload.get("fCnt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // If ChirpStack already decoded (object field present), use it
    let fields = if let Some(obj) = payload.get("object") {
        decode_chirpstack_object(obj)
    } else if let Some(data_b64) = payload.get("data").and_then(|v| v.as_str()) {
        // Raw data — decode with Cayenne LPP
        match base64_to_bytes(data_b64) {
            Ok(bytes) => decode_cayenne_lpp(&bytes),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    let device = LoRaDevice {
        dev_eui: dev_eui.clone(),
        fields,
        rssi,
        snr,
        battery: None,
        f_cnt,
        last_seen: chrono::Utc::now().timestamp_millis(),
        decoder_type: DecoderType::Cayenne,
        custom_decoder: None,
    };

    let mut devs = devices.write().await;
    devs.insert(dev_eui, device);
}

async fn handle_ttn_uplink(
    publish: &rumqttc::Publish,
    devices: &Arc<RwLock<HashMap<String, LoRaDevice>>>,
) {
    let payload: serde_json::Value = match serde_json::from_slice(&publish.payload) {
        Ok(v) => v,
        Err(_) => return,
    };

    let dev_eui = payload.pointer("/end_device_ids/device_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let rssi = payload.pointer("/uplink_message/rx_metadata/0/rssi")
        .and_then(|v| v.as_i64()).unwrap_or(-100) as i32;
    let snr = payload.pointer("/uplink_message/rx_metadata/0/snr")
        .and_then(|v| v.as_f64()).unwrap_or(0.0);
    let f_cnt = payload.pointer("/uplink_message/f_cnt")
        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    // TTN provides decoded_payload if payload formatter is configured
    let fields = if let Some(decoded) = payload.pointer("/uplink_message/decoded_payload") {
        decode_chirpstack_object(decoded) // Same flat JSON format
    } else {
        vec![]
    };

    let device = LoRaDevice {
        dev_eui: dev_eui.clone(),
        fields,
        rssi,
        snr,
        battery: None,
        f_cnt,
        last_seen: chrono::Utc::now().timestamp_millis(),
        decoder_type: DecoderType::Cayenne,
        custom_decoder: None,
    };

    let mut devs = devices.write().await;
    devs.insert(dev_eui, device);
}

/// Decode a flat JSON object into DecodedFields
fn decode_chirpstack_object(obj: &serde_json::Value) -> Vec<DecodedField> {
    let mut fields = Vec::new();
    if let Some(map) = obj.as_object() {
        for (key, val) in map {
            if let Some(n) = val.as_f64() {
                fields.push(DecodedField { name: key.clone(), value: n, unit: String::new() });
            } else if let Some(n) = val.as_i64() {
                fields.push(DecodedField { name: key.clone(), value: n as f64, unit: String::new() });
            }
        }
    }
    fields
}

// Helper: parse broker URL
fn parse_broker_host(url: &str) -> Result<String, String> {
    let url = url.trim_start_matches("mqtt://").trim_start_matches("mqtts://")
        .trim_start_matches("ssl://").trim_start_matches("tcp://");
    let host = url.split(':').next().unwrap_or(url);
    if host.is_empty() { Err("Invalid broker URL".into()) } else { Ok(host.into()) }
}

fn parse_broker_port(url: &str) -> Result<u16, String> {
    let url = url.trim_start_matches("mqtt://").trim_start_matches("mqtts://")
        .trim_start_matches("ssl://").trim_start_matches("tcp://");
    if url.contains(':') {
        url.split(':').nth(1).and_then(|p| p.parse().ok()).ok_or("Invalid port".into())
    } else if url.starts_with("mqtts://") || url.starts_with("ssl://") {
        Ok(8883)
    } else {
        Ok(1883)
    }
}

fn base64_to_bytes(b64: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut decoder = base64::read::DecoderReader::new(b64.as_bytes(), &base64::engine::general_purpose::STANDARD);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).map_err(|e| format!("Base64 error: {}", e))?;
    Ok(bytes)
}

fn hex_to_base64(hex: &str) -> Result<String, String> {
    let bytes = hex::decode(hex).map_err(|e| format!("Hex decode error: {}", e))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}
```

Note: Add `base64` and `tokio` full sync dependencies to Cargo.toml.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p lorawan-bridge`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add extensions/lorawan-bridge/src/
git commit -m "feat(lorawan-bridge): add NS client with MQTT event loop"
```

---

### Task 4: Extension Trait Implementation

**Files:**
- Modify: `extensions/lorawan-bridge/src/lib.rs` — full rewrite with Extension struct

- [ ] **Step 1: Write the full lib.rs**

Pattern follows weather-forecast-v2 and modbus-bridge conventions:
- `LorawanBridgeExtension` struct with `RwLock<Option<NsClient>>`, `AtomicI64` for counters
- `Extension` trait impl with metadata, metrics, commands
- Commands: `connect`, `disconnect`, `list_devices`, `get_device`, `send_downlink`, `set_decoder`, `get_status`
- `produce_metrics()` reads shared device state and outputs metrics per device
- `neomind_export!(LorawanBridgeExtension)` at bottom

The Extension trait impl follows the exact same pattern as modbus-bridge Task 4, adapted for LoRaWAN-specific commands.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p lorawan-bridge`

- [ ] **Step 3: Run tests**

Run: `cargo test -p lorawan-bridge`
Expected: 3 decoder tests pass

- [ ] **Step 4: Commit**

```bash
git add extensions/lorawan-bridge/
git commit -m "feat(lorawan-bridge): implement Extension trait with NS commands"
```

---

### Task 5: Frontend Component

**Files:**
- Create: `extensions/lorawan-bridge/frontend/` (all files)

- [ ] **Step 1: Create frontend files**

Same pattern as modbus-bridge Task 5:
- `frontend.json` with `LoRaWANDeviceCard` component, config for NS connection
- `vite.config.ts` with `lorawan-bridge-components` lib name
- `package.json` with `@neomind/lorawan-bridge-frontend`
- `src/index.tsx` with `LoRaWANDeviceCard` component

Component displays:
- NS connection status (connected/disconnected)
- Device list with signal bars (RSSI), battery icon, SNR
- Sensor readings per device in card grid
- Refresh button for manual update

Config schema:
```json
{
  "nsType": { "type": "string", "enum": ["chirpstack", "ttn"], "enumTitles": ["ChirpStack v4", "The Things Stack v3"] },
  "brokerUrl": { "type": "string", "title": "MQTT Broker URL" },
  "applicationId": { "type": "string", "title": "Application ID" },
  "nsApiUrl": { "type": "string", "title": "NS API URL (for downlink)" }
}
```

- [ ] **Step 2: Install and build**

```bash
cd extensions/lorawan-bridge/frontend && npm install && npm run build
```

- [ ] **Step 3: Commit**

```bash
git add extensions/lorawan-bridge/frontend/
git commit -m "feat(lorawan-bridge): add LoRaWANDeviceCard frontend component"
```

---

### Task 6: Build, Package, and Verify

- [ ] **Step 1: Build**

```bash
cargo build --release -p lorawan-bridge
```

- [ ] **Step 2: Package**

```bash
./build.sh --single lorawan-bridge
```

- [ ] **Step 3: Generate metadata**

```bash
./scripts/update-versions.sh 2.7.1
```

- [ ] **Step 4: Final commit**

```bash
git add . && git commit -m "feat(lorawan-bridge): complete extension with frontend and build"
```

---

## Notes

1. **rumqttc AsyncClient only**: The sync `Client` creates its own Tokio runtime and panics in cdylib. Always use `AsyncClient`.
2. **Event loop on host runtime**: `Handle::try_current()` gets NeoMind's runtime handle. `handle.spawn()` runs the MQTT event loop as a background task.
3. **Auto-discovery**: First uplink from unknown DevEUI auto-creates a device in shared state. `produce_metrics()` picks it up on next poll.
4. **Base64 handling**: ChirpStack sends raw payload as Base64. Need `base64` crate for decoding.
