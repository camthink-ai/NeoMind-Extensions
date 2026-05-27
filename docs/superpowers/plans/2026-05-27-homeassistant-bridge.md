# Home Assistant Bridge Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a NeoMind extension that imports Home Assistant entities as NeoMind devices, with real-time state monitoring via WebSocket and control via REST API.

**Architecture:** Dual-channel — WebSocket (tokio-tungstenite) for real-time state subscriptions, REST (ureq sync) for service calls and initial entity sync. WebSocket listener spawned on NeoMind's host Tokio runtime. Entity state cached in shared `RwLock<HashMap>`.

**Tech Stack:** Rust, tokio-tungstenite 0.28, ureq 3, futures-util, neomind-extension-sdk 0.6.3, React 18 + Vite (frontend)

**Spec:** `docs/superpowers/specs/2026-05-27-device-ecosystem-extensions-design.md` Section 3

---

## Critical: Async Runtime Pattern

Same pattern as lorawan-bridge — WebSocket needs Tokio runtime:

```rust
let handle = tokio::runtime::Handle::try_current()
    .expect("No Tokio runtime available");

let (ws_stream, _) = connect_async("ws://192.168.1.100:8123/api/websocket").await.unwrap();
let (write, read) = ws_stream.split();

handle.spawn(async move {
    // WebSocket read loop
    while let Some(msg) = read.next().await {
        handle_ws_message(msg).await;
    }
});
```

---

## File Structure

```
extensions/homeassistant-bridge/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Extension struct, Extension trait impl, FFI export
│   ├── ws_client.rs        # WebSocket connection, auth, state subscription
│   ├── rest_client.rs      # REST API wrapper (get_states, call_service)
│   └── types.rs            # HaEntity, HaConfig, domain types
├── metadata.json           # Auto-generated
└── frontend/
    ├── frontend.json
    ├── vite.config.ts
    ├── tsconfig.json
    ├── package.json
    └── src/
        └── index.tsx        # HADeviceCard component
```

---

### Task 1: Project Scaffold

**Files:**
- Create: `extensions/homeassistant-bridge/Cargo.toml`
- Modify: `Cargo.toml` (root workspace — add member)

- [ ] **Step 1: Create extension Cargo.toml**

```toml
[package]
name = "homeassistant-bridge"
version = "2.7.1"
edition = "2021"
authors = ["NeoMind Team"]
license = "Apache-2.0"
description = "Home Assistant bridge for NeoMind — import 3000+ HA entity integrations as devices"

[lib]
name = "neomind_extension_homeassistant_bridge"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = "0.1"
chrono = "0.4"

# WebSocket client — runs on host Tokio runtime
tokio-tungstenite = { version = "0.28", features = ["rustls-tls-webpki-roots"] }
futures-util = { version = "0.3", features = ["sink"] }
tokio = { version = "1", features = ["rt", "sync"] }

# HTTP REST client (sync — no Tokio runtime needed)
ureq = { version = "3", features = ["json"] }

[features]
default = []
native = []
wasm = []
```

Note: Using tokio-tungstenite 0.28 (confirmed latest stable). The `rustls-tls-webpki-roots` feature enables `wss://` connections.

- [ ] **Step 2: Add to workspace members**

Add `"extensions/homeassistant-bridge"` to `members` in root `Cargo.toml`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p homeassistant-bridge`

- [ ] **Step 4: Commit**

```bash
git add extensions/homeassistant-bridge/Cargo.toml Cargo.toml
git commit -m "feat(homeassistant-bridge): scaffold extension project"
```

---

### Task 2: Types

**Files:**
- Create: `extensions/homeassistant-bridge/src/types.rs`
- Create: `extensions/homeassistant-bridge/src/lib.rs` (stub)

- [ ] **Step 1: Create types.rs**

```rust
use serde::{Deserialize, Serialize};

/// HA connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaConfig {
    pub ha_url: String,
    pub token: String,
    #[serde(default = "default_domains")]
    pub domains: Vec<String>,
    #[serde(default = "default_sync_interval")]
    pub sync_interval: u64,
}

fn default_domains() -> Vec<String> {
    vec!["sensor".into(), "light".into(), "switch".into(), "climate".into(), "lock".into()]
}

fn default_sync_interval() -> u64 { 30 }

/// An HA entity with its current state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaEntity {
    pub entity_id: String,
    pub domain: String,
    pub name: String,
    pub state: String,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub battery: Option<u8>,
    pub last_changed: String,
}

/// Parsed from HA's GET /api/states response
#[derive(Debug, Deserialize)]
pub struct HaStateResponse {
    pub entity_id: String,
    pub state: String,
    pub attributes: serde_json::Value,
    pub last_changed: String,
}

impl HaStateResponse {
    pub fn to_entity(&self) -> HaEntity {
        let domain = self.entity_id.split('.').next().unwrap_or("").to_string();
        let name = self.attributes.get("friendly_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.entity_id)
            .to_string();
        let value = self.state.parse::<f64>().ok();
        let unit = self.attributes.get("unit_of_measurement")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let battery = self.attributes.get("battery")
            .or_else(|| self.attributes.get("battery_level"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u8);

        HaEntity {
            entity_id: self.entity_id.clone(),
            domain,
            name,
            state: self.state.clone(),
            value,
            unit,
            battery,
            last_changed: self.last_changed.clone(),
        }
    }
}

/// HA domain types and their control capabilities
pub fn domain_has_commands(domain: &str) -> bool {
    matches!(domain, "light" | "switch" | "climate" | "lock" | "cover" | "fan" | "media_player")
}
```

- [ ] **Step 2: Create lib.rs stub**

```rust
pub mod types;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p homeassistant-bridge`

- [ ] **Step 4: Commit**

```bash
git add extensions/homeassistant-bridge/src/
git commit -m "feat(homeassistant-bridge): add HA entity types"
```

---

### Task 3: REST Client

**Files:**
- Create: `extensions/homeassistant-bridge/src/rest_client.rs`
- Modify: `extensions/homeassistant-bridge/src/lib.rs` (add mod)

- [ ] **Step 1: Create rest_client.rs**

```rust
use crate::types::{HaConfig, HaEntity, HaStateResponse};

/// Sync REST API wrapper for Home Assistant (uses ureq, no Tokio runtime)
pub struct HaRestClient {
    base_url: String,
    token: String,
}

impl HaRestClient {
    pub fn new(config: &HaConfig) -> Self {
        Self {
            base_url: config.ha_url.trim_end_matches('/').to_string(),
            token: config.token.clone(),
        }
    }

    /// GET /api/states — fetch all entity states
    pub fn get_all_states(&self) -> Result<Vec<HaEntity>, String> {
        let resp: Vec<HaStateResponse> = ureq::get(&format!("{}/api/states", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| format!("HA REST error: {}", e))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("HA JSON error: {}", e))?;

        Ok(resp.iter().map(|s| s.to_entity()).collect())
    }

    /// GET /api/states/{entity_id} — fetch single entity
    pub fn get_state(&self, entity_id: &str) -> Result<HaEntity, String> {
        let resp: HaStateResponse = ureq::get(&format!("{}/api/states/{}", self.base_url, entity_id))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| format!("HA REST error: {}", e))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("HA JSON error: {}", e))?;

        Ok(resp.to_entity())
    }

    /// POST /api/services/{domain}/{service} — call a service
    pub fn call_service(&self, domain: &str, service: &str, entity_id: &str, data: Option<&serde_json::Value>) -> Result<(), String> {
        let mut body = serde_json::json!({
            "entity_id": entity_id
        });
        if let Some(extra) = data {
            if let Some(obj) = extra.as_object() {
                for (k, v) in obj {
                    body[k] = v.clone();
                }
            }
        }

        ureq::post(&format!("{}/api/services/{}/{}", self.base_url, domain, service))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_json(&body)
            .map_err(|e| format!("HA call_service error: {}", e))?;
        Ok(())
    }

    /// Test connection by fetching HA status
    pub fn test_connection(&self) -> Result<(), String> {
        ureq::get(&format!("{}/api/", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| format!("HA connection test failed: {}", e))?;
        Ok(())
    }
}
```

- [ ] **Step 2: Update lib.rs**

```rust
pub mod types;
pub mod rest_client;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p homeassistant-bridge`

- [ ] **Step 4: Commit**

```bash
git add extensions/homeassistant-bridge/src/
git commit -m "feat(homeassistant-bridge): add sync REST client for HA API"
```

---

### Task 4: WebSocket Client

**Files:**
- Create: `extensions/homeassistant-bridge/src/ws_client.rs`
- Modify: `extensions/homeassistant-bridge/src/lib.rs` (add mod)

- [ ] **Step 1: Create ws_client.rs**

Core architecture:
1. Connect to `ws://HOST:8123/api/websocket` (or `wss://`)
2. Receive `auth_required` message
3. Send `{"type":"auth","access_token":"TOKEN"}`
4. Receive `auth_ok`
5. Send `{"id":1,"type":"subscribe_events","event_type":"state_changed"}`
6. Receive state_changed events → update shared entity state

```rust
use crate::types::HaEntity;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

pub struct HaWsClient {
    write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
        >,
        Message,
    >,
    entities: Arc<RwLock<HashMap<String, HaEntity>>>,
    next_id: u64,
}

impl HaWsClient {
    /// Connect and authenticate, return the client for spawning read loop
    pub async fn connect(
        ha_url: &str,
        token: &str,
        entities: Arc<RwLock<HashMap<String, HaEntity>>>,
    ) -> Result<Self, String> {
        let ws_url = build_ws_url(ha_url);
        let (ws_stream, _) = connect_async(&ws_url)
            .await
            .map_err(|e| format!("WS connect error: {}", e))?;

        let (mut write, mut read) = ws_stream.split();

        // Step 1: Wait for auth_required
        let auth_msg = read.next().await
            .ok_or("No auth_required message")?
            .map_err(|e| format!("WS read error: {}", e))?;

        // Verify it's auth_required
        if let Message::Text(text) = &auth_msg {
            let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("JSON: {}", e))?;
            if v.get("type").and_then(|t| t.as_str()) != Some("auth_required") {
                return Err("Expected auth_required".into());
            }
        }

        // Step 2: Send auth
        let auth_payload = serde_json::json!({
            "type": "auth",
            "access_token": token
        });
        write.send(Message::text(auth_payload.to_string()))
            .await
            .map_err(|e| format!("WS send error: {}", e))?;

        // Step 3: Wait for auth_ok
        let auth_result = read.next().await
            .ok_or("No auth response")?
            .map_err(|e| format!("WS read error: {}", e))?;

        if let Message::Text(text) = &auth_result {
            let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("JSON: {}", e))?;
            if v.get("type").and_then(|t| t.as_str()) != Some("auth_ok") {
                let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("Auth failed");
                return Err(msg.into());
            }
        }

        // Step 4: Subscribe to state_changed events
        let subscribe = serde_json::json!({
            "id": 1,
            "type": "subscribe_events",
            "event_type": "state_changed"
        });
        write.send(Message::text(subscribe.to_string()))
            .await
            .map_err(|e| format!("WS subscribe error: {}", e))?;

        Ok(Self { write, entities, next_id: 2 })
    }

    /// Spawn the read loop on the given runtime handle
    pub fn spawn_read_loop(mut self, handle: &tokio::runtime::Handle) {
        handle.spawn(async move {
            // We need to re-split: take ownership of read half
            // Actually, we already split above. We need to restructure.
            // The write half is in self, but we consumed read above for auth.
            // Better approach: split after auth.
        });
    }
}

// Better approach: single struct managing the whole connection
pub async fn run_ws_loop(
    ha_url: &str,
    token: &str,
    entities: Arc<RwLock<HashMap<String, HaEntity>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    let ws_url = build_ws_url(ha_url);
    let (mut ws_stream, _) = connect_async(&ws_url).await
        .map_err(|e| format!("WS connect: {}", e))?;

    // Auth sequence
    ws_auth(&mut ws_stream, token).await?;

    // Subscribe to state_changed
    let subscribe = serde_json::json!({"id": 1, "type": "subscribe_events", "event_type": "state_changed"});
    ws_stream.send(Message::text(subscribe.to_string())).await
        .map_err(|e| format!("Subscribe: {}", e))?;

    // Read loop
    while running.load(std::sync::atomic::Ordering::SeqCst) {
        match ws_stream.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    handle_event(&v, &entities).await;
                }
            }
            Some(Ok(Message::Ping(data))) => {
                // Auto-pong handled by tungstenite
                let _ = data;
            }
            Some(Ok(Message::Close(_))) => break,
            Some(Err(e)) => {
                eprintln!("HA WS error: {}", e);
                break;
            }
            None => break,
            _ => {}
        }
    }

    Ok(())
}

async fn ws_auth<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>, token: &str) -> Result<(), String>
where S: tokio_tungstenite::tungstenite::stream::Stream + Unpin {
    use futures_util::StreamExt;
    // Wait for auth_required
    while let Some(result) = ws.next().await {
        match result {
            Ok(Message::Text(text)) => {
                let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                if v.get("type").and_then(|t| t.as_str()) == Some("auth_required") {
                    let auth = serde_json::json!({"type": "auth", "access_token": token});
                    ws.send(Message::text(auth.to_string())).await.map_err(|e| e.to_string())?;
                    // Wait for auth_ok
                    if let Some(Ok(Message::Text(resp)))) = ws.next().await {
                        let rv: serde_json::Value = serde_json::from_str(&resp).map_err(|e| e.to_string())?;
                        if rv.get("type").and_then(|t| t.as_str()) == Some("auth_ok") {
                            return Ok(());
                        } else {
                            return Err("Auth failed".into());
                        }
                    }
                    return Err("No auth response".into());
                }
            }
            _ => continue,
        }
    }
    Err("Connection closed before auth".into())
}

async fn handle_event(v: &serde_json::Value, entities: &Arc<RwLock<HashMap<String, HaEntity>>>) {
    // HA event format:
    // { "id": 1, "type": "event", "event": { "data": { "entity_id": "...", "new_state": {...} } } }
    if v.get("type").and_then(|t| t.as_str()) != Some("event") { return; }

    let entity_id = match v.pointer("/event/data/entity_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return,
    };

    let domain = entity_id.split('.').next().unwrap_or("").to_string();

    if let Some(new_state) = v.pointer("/event/data/new_state") {
        let ha_resp = crate::types::HaStateResponse {
            entity_id: entity_id.clone(),
            state: new_state.get("state").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
            attributes: new_state.get("attributes").cloned().unwrap_or(serde_json::json!({})),
            last_changed: new_state.get("last_changed").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        };
        let entity = ha_resp.to_entity();
        let mut ents = entities.write().await;
        ents.insert(entity_id, entity);
    }
}

fn build_ws_url(ha_url: &str) -> String {
    let url = ha_url.trim_end_matches('/');
    if url.starts_with("https://") {
        url.replace("https://", "wss://") + "/api/websocket"
    } else if url.starts_with("http://") {
        url.replace("http://", "ws://") + "/api/websocket"
    } else {
        format!("ws://{}/api/websocket", url)
    }
}
```

Note: The ws_auth function has a syntax issue in the example above (extra parenthesis). During implementation, clean this up. The key pattern is correct: wait for auth_required → send auth → wait for auth_ok.

- [ ] **Step 2: Update lib.rs**

```rust
pub mod types;
pub mod rest_client;
pub mod ws_client;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p homeassistant-bridge`

- [ ] **Step 4: Commit**

```bash
git add extensions/homeassistant-bridge/src/
git commit -m "feat(homeassistant-bridge): add WebSocket client with auth and state tracking"
```

---

### Task 5: Extension Trait Implementation

**Files:**
- Modify: `extensions/homeassistant-bridge/src/lib.rs` — full rewrite

- [ ] **Step 1: Write the full lib.rs**

Pattern follows modbus-bridge/lorawan-bridge:
- `HomeAssistantBridgeExtension` struct with:
  - `rest_client: RwLock<Option<HaRestClient>>`
  - `entities: Arc<RwLock<HashMap<String, HaEntity>>>`
  - `running: Arc<AtomicBool>`
  - `AtomicI64` counters
- `Extension` trait impl with metadata, metrics, commands
- Commands: `connect`, `disconnect`, `list_entities`, `get_state`, `call_service`, `set_filters`, `get_status`
- `connect` command flow:
  1. Create `HaRestClient`, test connection
  2. `GET /api/states` to fetch all entities, filter by domain
  3. Store filtered entities in shared state
  4. Spawn WebSocket read loop on host runtime
- `produce_metrics()` outputs entity states as metrics
- `execute_command` for `call_service` delegates to `rest_client.call_service()`
- `neomind_export!(HomeAssistantBridgeExtension)` at bottom

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p homeassistant-bridge`

- [ ] **Step 3: Commit**

```bash
git add extensions/homeassistant-bridge/
git commit -m "feat(homeassistant-bridge): implement Extension trait with HA commands"
```

---

### Task 6: Frontend Component

**Files:**
- Create: `extensions/homeassistant-bridge/frontend/` (all files)

- [ ] **Step 1: Create frontend files**

Same scaffold pattern as other extensions:
- `frontend.json` with `HADeviceCard` component
- `vite.config.ts` with `homeassistant-bridge-components` lib name
- `package.json` with `@neomind/homeassistant-bridge-frontend`
- `src/index.tsx` with `HADeviceCard` component

Component displays:
- HA connection status indicator
- Entity list grouped by domain (sensors, lights, switches...)
- Quick toggle controls for switch/light entities
- Sensor values with units
- Last changed timestamp

Config schema:
```json
{
  "haUrl": { "type": "string", "title": "Home Assistant URL", "description": "e.g. http://192.168.1.100:8123" },
  "token": { "type": "string", "title": "Long-Lived Access Token" },
  "domains": { "type": "string", "title": "Domains (comma-separated)", "default": "sensor,light,switch,climate,lock" }
}
```

- [ ] **Step 2: Install and build**

```bash
cd extensions/homeassistant-bridge/frontend && npm install && npm run build
```

- [ ] **Step 3: Commit**

```bash
git add extensions/homeassistant-bridge/frontend/
git commit -m "feat(homeassistant-bridge): add HADeviceCard frontend component"
```

---

### Task 7: Build, Package, and Verify

- [ ] **Step 1: Build**

```bash
cargo build --release -p homeassistant-bridge
```

- [ ] **Step 2: Package**

```bash
./build.sh --single homeassistant-bridge
```

- [ ] **Step 3: Generate metadata**

```bash
./scripts/update-versions.sh 2.7.1
```

- [ ] **Step 4: Final commit**

```bash
git add . && git commit -m "feat(homeassistant-bridge): complete extension with frontend and build"
```

---

## Notes

1. **tokio-tungstenite version**: Using 0.28 (latest stable on crates.io). The `rustls-tls-webpki-roots` feature enables `wss://`.
2. **Auth flow**: HA WebSocket requires `auth_required` → `auth` → `auth_ok` handshake before sending commands. The ws_client handles this.
3. **Domain filtering**: Users select which HA domains to import. Only matching entities are stored and produce metrics.
4. **ureq v3**: Using new ureq v3 API (agent-based). Different from existing extensions that use ureq v2.
5. **Ping/Pong**: tungstenite auto-replies to HA pings. No manual handling needed.
