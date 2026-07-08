# SDK API Reference

> Verified against `neomind-extension-sdk` v0.6.x (ABI v3). All signatures below are copied
> from source at `NeoMind/crates/neomind-extension-sdk/src/`.

## Constants

```rust
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");   // "0.6.x"
pub const SDK_ABI_VERSION: u32 = 3;
pub const MIN_NEOMIND_VERSION: &str = "0.5.0";
```

## Core Trait: `Extension`

All extensions implement `Extension` from `neomind_extension_sdk::host`. Only `metadata()`
and `as_any()` are required; everything else has a default.

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    // === Required ===
    fn metadata(&self) -> &ExtensionMetadata;
    fn as_any(&self) -> &dyn std::any::Any;

    // === Descriptor / capabilities ===
    fn descriptor(&self) -> Option<ExtensionDescriptor> { None }
    fn stream_capability(&self) -> Option<StreamCapability> { None }

    // === Lifecycle (sync, in load order) ===
    fn init(&mut self) -> Result<()> { Ok(()) }
    fn start(&mut self) -> Result<()> { Ok(()) }
    fn stop(&mut self) -> Result<()> { Ok(()) }
    fn status(&self) -> String { "unknown".to_string() }
    async fn health_check(&self) -> Result<bool> { Ok(true) }
    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> { Ok(()) }
    async fn on_unload(&self) -> Result<()> { Ok(()) }

    // === Commands & metrics ===
    fn metrics(&self) -> Vec<MetricDescriptor> { Vec::new() }
    fn commands(&self) -> Vec<CommandDescriptor> { Vec::new() }
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }
    fn get_stats(&self) -> ExtensionStats { ExtensionStats::default() }

    // === Command execution (async) ===
    async fn execute_command(
        &self,
        command_name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let _ = args;
        Err(ExtensionError::CommandNotFound(command_name.to_string()))
    }

    // === Events (sync!) ===
    // Subscribe by event type name. Empty = no subscriptions.
    fn event_subscriptions(&self) -> &[&str] { &[] }
    // Handle a delivered event. SYNC — no .await, use parking_lot locks not tokio::Mutex.
    fn handle_event(&self, _event_type: &str, _payload: &serde_json::Value) -> Result<()> { Ok(()) }

    // === Streaming (stateless) ===
    async fn process_chunk(&self, _chunk: DataChunk) -> Result<StreamResult> {
        Err(ExtensionError::ExecutionFailed("Streaming not supported".into()))
    }

    // === Streaming (session-based) ===
    async fn init_session(&self, _session: &StreamSession) -> Result<()> { Ok(()) }
    async fn process_session_chunk(
        &self, _session_id: &str, _chunk: DataChunk,
    ) -> Result<StreamResult> {
        Err(ExtensionError::ExecutionFailed("Session streaming not supported".into()))
    }
    async fn close_session(&self, _session_id: &str) -> Result<SessionStats> {
        Ok(SessionStats::default())
    }

    // === Push mode ===
    fn latest_output(&self) -> Option<PushOutputMessage> { None }
    async fn start_push(&self, _session_id: &str) -> Result<()> { Ok(()) }
    async fn stop_push(&self, _session_id: &str) -> Result<()> { Ok(()) }
    #[cfg(not(target_arch = "wasm32"))]
    fn set_output_sender(
        &self, _sender: Arc<tokio::sync::mpsc::Sender<PushOutputMessage>>,
    ) {}
}
```

### Sync vs Async Cheat Sheet

| Method | Sync? | Notes |
|---|---|---|
| `metadata` / `metrics` / `commands` / `produce_metrics` / `get_stats` / `status` | ✅ sync | No `.await`. Cache async results into `Atomic*` / `parking_lot::Mutex` fields. |
| `init` / `start` / `stop` | ✅ sync | One-shot lifecycle. |
| `event_subscriptions` / `handle_event` | ✅ sync | EventDispatcher calls `handle_event` from a sync context. **Use `parking_lot::RwLock`, not `tokio::Mutex`** (the latter would deadlock). |
| `execute_command` / `configure` / `health_check` / `on_unload` | 🔁 async | Free to `.await`. |
| `process_chunk` / `init_session` / `process_session_chunk` / `close_session` / `start_push` / `stop_push` | 🔁 async | Streaming entry points. |

---

## ExtensionMetadata

```rust
pub struct ExtensionMetadata {
    pub id: String,
    pub name: String,
    pub version: String,                // plain "2.0.0", NOT semver::Version
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(skip)]
    pub file_path: Option<std::path::PathBuf>,
    pub config_parameters: Option<Vec<ParameterDefinition>>,
}

impl ExtensionMetadata {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Self;
    pub fn with_description(self, d: impl Into<String>) -> Self;
    pub fn with_author(self, a: impl Into<String>) -> Self;
    pub fn with_homepage(self, h: impl Into<String>) -> Self;
    pub fn with_license(self, l: impl Into<String>) -> Self;
    pub fn with_config_parameters(self, p: Vec<ParameterDefinition>) -> Self;
    pub fn validate(&self) -> std::result::Result<(), &'static str>;
}
```

Static-cache it inside `metadata()` with `OnceLock` so you can return a reference:

```rust
fn metadata(&self) -> &ExtensionMetadata {
    static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
    META.get_or_init(|| {
        ExtensionMetadata::new("my-ext-v2", "My Extension", "2.0.0")
            .with_description("...")
            .with_author("Me")
    })
}
```

---

## Descriptors & Values

### MetricDescriptor + MetricDataType + MetricValue

```rust
pub enum MetricDataType {
    Float, Integer, Boolean, String, Binary,
    Enum { options: Vec<String> },
}   // serialized lowercase: "float", "integer", ...

pub enum ParamMetricValue {   // a.k.a. MetricValue in some aliases
    Float(f64),
    Integer(i64),
    Boolean(bool),
    String(String),
    Binary(Vec<u8>),
    Null,
}

pub struct MetricDescriptor {
    pub name: String,
    pub display_name: String,
    pub data_type: MetricDataType,
    pub unit: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub required: bool,
}
```

### ExtensionMetricValue

```rust
pub struct ExtensionMetricValue {
    pub name: String,
    pub value: ParamMetricValue,
    pub timestamp: i64,   // unix ms
}
```

### CommandDescriptor + ParameterDefinition

```rust
pub struct CommandDescriptor {     // legacy name ExtensionCommand — same type
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub payload_template: String,
    pub parameters: Vec<ParameterDefinition>,
    pub fixed_values: HashMap<String, serde_json::Value>,
    pub samples: Vec<serde_json::Value>,
    pub parameter_groups: Vec<ParameterGroup>,
}

pub struct ParameterDefinition {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub param_type: MetricDataType,
    pub required: bool,
    pub default_value: Option<ParamMetricValue>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub options: Vec<String>,
}
```

---

## Builders (preferred — fluent API)

```rust
MetricBuilder::new("temperature", "Temperature")
    .float()
    .unit("°C")
    .min(-40.0).max(85.0)
    .required()
    .build()

CommandBuilder::new("analyze")
    .display_name("Analyze")
    .description("Run analysis")
    .param(
        ParamBuilder::new("source", MetricDataType::String)
            .display_name("Source")
            .required()
            .build()
    )
    .sample(json!({"source": "device-1"}))
    .build()
```

### MetricBuilder methods
`new(name, display_name)` · `float()` / `integer()` / `boolean()` / `string()` / `enum_type(Vec<String>)` · `unit(s)` · `min(f64)` / `max(f64)` · `required()` · `build()`

### CommandBuilder methods
`new(name)` · `display_name(s)` · `description(s)` · `param(ParameterDefinition)` · `param_simple(name, display_name, type)` · `param_optional(name, display_name, type)` · `param_with_default(name, display_name, type, default)` · `sample(Value)` · `build()`

### ParamBuilder methods
`new(name, data_type)` · `display_name(s)` · `description(s)` · `optional()` / `required()` · `default(MetricValue)` · `min(f64)` / `max(f64)` · `options(Vec<String>)` · `build()`

---

## Macros

### `neomind_export!` — FFI entry point

```rust
neomind_extension_sdk::neomind_export!(MyExtension);
// or, if your constructor needs config:
neomind_extension_sdk::neomind_export_with_constructor!(MyExtension, MyExtension::with_config);
```

Generates the ABI v3 JSON-over-FFI symbols the runner loads (see "FFI exports" below).
**Never hand-write `#[no_mangle]` symbols** — old ABI v2 `_neomind_extension_create` / `_destroy`
exports are deprecated and will crash the runner.

### `static_metadata!` / `static_metrics!` / `static_commands!`

```rust
static_metadata!("my-ext", "My Extension", "1.0.0");

static_metrics!(
    MetricDescriptor::new("temp", "Temperature", MetricDataType::Float),
    MetricDescriptor::new("humidity", "Humidity", MetricDataType::Float),
);

static_commands!(
    CommandBuilder::new("start").display_name("Start").build(),
    CommandBuilder::new("stop").display_name("Stop").build(),
);
```

### Logging

```rust
ext_info!("Extension started");
ext_warn!("Slow request: {} ms", elapsed_ms);
ext_error!("Failed: {}", err);
ext_debug!("Payload: {:?}", payload);
ext_log!(Level::Info, "custom level");
```

### Metric value helpers (auto-fills timestamp)

```rust
metric_int!("counter", 42)                 // (name, value)
metric_float!("temperature", 25.5)
metric_bool!("is_active", true)
metric_string!("status", "ok")
metric_value!("raw", ParamMetricValue::Binary(bytes), chrono::Utc::now().timestamp_millis())
```

---

## ExtensionError (23 variants)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExtensionError {
    CommandNotFound(String),
    MetricNotFound(String),
    InvalidArguments(String),
    ExecutionFailed(String),
    Timeout(String),
    NotFound(String),
    InvalidFormat(String),
    LoadFailed(String),
    SecurityError(String),
    SymbolNotFound(String),
    IncompatibleVersion { expected: u32, got: u32 },
    NullPointer,
    AlreadyRegistered(String),
    NotSupported(String),
    InvalidStreamData(String),
    SessionNotFound(String),
    SessionAlreadyExists(String),
    InferenceFailed(String),
    Io(String),
    Json(String),
    ConfigurationError(String),
    InternalError(String),
    Other(String),
}

pub type Result<T> = std::result::Result<T, ExtensionError>;
```

> Historical names you may still see in older code: `NetworkError` / `IoError` / `ConfigError` /
> `ExecutionError` — all replaced. New code uses `Io("...")`, `ConfigurationError("...")`,
> `ExecutionFailed("...")`.

---

## ExtensionCapability (20 variants)

Capabilities are fine-grained permissions the runner grants to your extension. Declare them
via the runner config (`ALLOWED_CAPABILITIES`) or — for full native extensions — invoke them
dynamically via `CapabilityContext::default()`.

```rust
pub enum ExtensionCapability {
    DeviceMetricsRead,        // "device_metrics_read"
    DeviceMetricsWrite,       // "device_metrics_write"
    DeviceControl,            // "device_control"
    StorageQuery,             // "storage_query"
    EventPublish,             // "event_publish"
    EventSubscribe,           // "event_subscribe"
    TelemetryHistory,         // "telemetry_history"
    MetricsAggregate,         // "metrics_aggregate"
    ExtensionCall,            // "extension_call"
    AgentTrigger,             // "agent_trigger"
    ChatStream,               // "chat_stream"          (Phase 1 one-shot)
    ChatStreamCancel,         // "chat_stream_cancel"
    ChatSessionOpen,          // "chat_session_open"    (Phase 2 persistent)
    ChatSessionSend,          // "chat_session_send"
    ChatSessionClose,         // "chat_session_close"
    ChatStreamCancelTurn,     // "chat_stream_cancel_turn"
    RuleTrigger,              // "rule_trigger"
    DeviceTemplateRegister,   // "device_template_register"
    DeviceRegister,           // "device_register"
    DeviceUnregister,         // "device_unregister"
    Custom(String),           // "custom:<name>"
}
```

### Capability name constants

```rust
use neomind_extension_sdk::host::capabilities::*;
// DEVICE_METRICS_READ, DEVICE_METRICS_WRITE, DEVICE_CONTROL, STORAGE_QUERY,
// EVENT_PUBLISH, EVENT_SUBSCRIBE, TELEMETRY_HISTORY, METRICS_AGGREGATE,
// EXTENSION_CALL, AGENT_TRIGGER, CHAT_STREAM, CHAT_STREAM_CANCEL,
// CHAT_SESSION_OPEN, CHAT_SESSION_SEND, CHAT_SESSION_CLOSE,
// CHAT_STREAM_CANCEL_TURN, RULE_TRIGGER, DEVICE_TEMPLATE_REGISTER,
// DEVICE_REGISTER, DEVICE_UNREGISTER
```

---

## CapabilityContext — invoking host capabilities

`CapabilityContext` is a cheap handle to the host's capability dispatcher. Always
construct it locally where needed; **don't store it in your struct** (it's a global singleton
under the hood).

```rust
use neomind_extension_sdk::host::CapabilityContext;
use serde_json::json;

let ctx = CapabilityContext::default();

// Register a device template (idempotent — guard with an AtomicBool flag)
let tpl = json!({
    "device_type": "my_thing",
    "name": "My Thing",
    "metrics": [{ "name": "state", "display_name": "State", "data_type": "String" }],
    "commands": []
});
let r = ctx.invoke_capability("device_template_register", &tpl);
// r.get("success") == true on success

// Register a device instance
ctx.invoke_capability("device_register", &json!({
    "device_id": "my-thing-001",
    "name": "My Thing #1",
    "device_type": "my_thing",
}));

// Write a metric for that device
ctx.invoke_capability("device_metrics_write", &json!({
    "device_id": "my-thing-001",
    "metric": "state",
    "value": "on",
    "timestamp": chrono::Utc::now().timestamp_millis(),
}));
```

### SDK convenience helpers

```rust
use neomind_extension_sdk::capabilities::{device, chat};

// Device read / write / telemetry
let metrics = device::get_metrics(&ctx, "my-thing-001").await?;
device::write_virtual_metric(&ctx, "my-thing-001", "state", &json!("on")).await?;
let last_24h = device::query_telemetry_last_24h(&ctx, "my-thing-001", "temperature").await?;
let avg = device::aggregate_avg_24h(&ctx, "my-thing-001", "temperature").await?;

// ChatStream — Phase 1 one-shot
let r = chat::invoke(&ctx, "Hello", None).await?;             // -> {session_id, created}
let sid = chat::invoke_for_session_id(&ctx, "Hello", None).await?;

// ChatStream — Phase 2 persistent session
let open = chat::open_session(&ctx, None).await?;             // -> {session_id, created}
let turn = chat::send_message(&ctx, sid, "What's up?").await?;// -> {turn_id}
chat::cancel_turn(&ctx, sid, Some(turn_id)).await?;           // optional
chat::close_session(&ctx, sid).await?;
```

> Chunks arrive as **events**, not as return values — see "Event subscriptions" below.

---

## Events: subscribe + handle

```rust
#[async_trait]
impl Extension for MyExt {
    // 1) Declare which event type names you want. Empty = no subscriptions.
    fn event_subscriptions(&self) -> &[&str] {
        &["AgentStreamChunk", "AgentStreamEnd"]
    }

    // 2) Handle them. SYNC. Use parking_lot::RwLock for any shared state.
    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        // The dispatcher wraps events in an envelope; unwrap if present.
        let inner = payload.get("payload").unwrap_or(payload);

        match event_type {
            "AgentStreamChunk" => {
                if let Some(sid) = inner.get("session_id").and_then(|v| v.as_str()) {
                    // route chunk by sid ...
                }
            }
            "AgentStreamEnd" => { /* authoritative terminator */ }
            _ => {}
        }
        Ok(())
    }
}
```

**Critical gotchas (these have all caused real bugs):**
1. If you don't override `event_subscriptions()`, the default `&[]` causes the dispatcher
   to silently drop **all** your events.
2. `handle_event` is **sync**. Use `parking_lot::RwLock` (not `tokio::Mutex`) for any
   state it touches, otherwise you'll deadlock or have to `.try_write()`.
3. The payload is wrapped as `{event_type, payload: {...}, timestamp}`. Always
   `payload.get("payload").unwrap_or(payload)` before reading fields.
4. `execute_command` is async — but `handle_event` is not. Don't try to share a
   `tokio::Mutex` between them.

---

## Streaming types (for `process_chunk` / sessions / push)

```rust
pub struct StreamCapability {
    pub supported_data_types: Vec<StreamDataType>,
    pub max_chunk_size: usize,
    pub preferred_chunk_size: usize,
    pub max_concurrent_sessions: usize,
    pub mode: StreamMode,         // Stateless | Stateful | Push | Pull
    pub direction: StreamDirection, // None | Input | Output | Duplex
    pub flow_control: FlowControl,
    pub config_schema: Option<serde_json::Value>,
}
impl StreamCapability {
    pub fn push() -> Self; pub fn upload() -> Self;
    pub fn download() -> Self; pub fn stateful() -> Self;
}

pub enum StreamDataType {
    Binary, Text, Json,
    Image { format: String },
    Audio { format: String, sample_rate: u32, channels: u8 },
    Video { codec: String, width: u32, height: u32, fps: u32 },
    Sensor { sensor_type: String },
    Custom { mime_type: String },
}

pub struct DataChunk {
    pub sequence: u64,
    pub data_type: StreamDataType,
    pub data: Vec<u8>,
    pub timestamp: i64,
    pub metadata: Option<serde_json::Value>,
    pub is_last: bool,
}
impl DataChunk {
    pub fn binary(seq, bytes) -> Self;
    pub fn text(seq, s) -> Self;
    pub fn json(seq, Value) -> Result<Self, serde_json::Error>;
    pub fn image(seq, bytes, format) -> Self;
    pub fn with_last(self) -> Self;
    pub fn with_metadata(self, Value) -> Self;
}

pub struct StreamResult {
    pub input_sequence: Option<u64>,
    pub output_sequence: u64,
    pub data: Vec<u8>,
    pub data_type: StreamDataType,
    pub processing_ms: f32,
    pub metadata: Option<serde_json::Value>,
    pub error: Option<StreamError>,
}

pub struct StreamSession {
    pub id: String,
    pub extension_id: String,
    pub config: serde_json::Value,
    pub started_at: i64,
    pub last_activity: i64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub chunks_in: u64,
    pub chunks_out: u64,
    pub client_info: Option<ClientInfo>,
    pub metadata: Option<serde_json::Value>,
}
```

For push-mode extensions, call `set_output_sender`-installed sender (stored on your struct
as `Arc<tokio::sync::mpsc::Sender<PushOutputMessage>>`) to emit frames back to the client:

```rust
pub struct PushOutputMessage {
    pub session_id: String,
    pub sequence: u64,
    pub mime: String,        // e.g. "audio/pcm", "image/jpeg"
    pub data: Vec<u8>,
}
```

---

## FFI exports generated by `neomind_export!`

All symbols are JSON-over-FFI (ABI v3). Strings return as `*mut c_char` and must be freed
by the host via `neomind_extension_free_string`.

| Symbol | Purpose |
|---|---|
| `neomind_extension_abi_version` | Returns `3` |
| `neomind_extension_metadata` | C-struct metadata (legacy path) |
| `neomind_extension_descriptor_json` | Full descriptor (commands, metrics, capabilities) |
| `neomind_extension_execute_command_json` | `{"command","args"}` → `{"success","result"}` |
| `neomind_extension_produce_metrics_json` | Current metric values |
| `neomind_extension_stats_json` | `get_stats()` |
| `neomind_extension_health_check_json` | `health_check()` |
| `neomind_extension_configure_json` | Apply config |
| `neomind_extension_event_subscriptions_json` | `{"success","event_types":[...]}` |
| `neomind_extension_init_session_json` | Start session |
| `neomind_extension_process_session_chunk_json` | Push chunk into session |
| `neomind_extension_close_session_json` | End session |
| `neomind_extension_process_chunk_json` | Stateless chunk |
| `neomind_extension_stream_capability_json` | StreamCapability descriptor |
| `neomind_extension_start_push_json` / `stop_push_json` | Push-mode lifecycle |
| `neomind_extension_reset_instance` | Drop & recreate instance (panic recovery) |
| `neomind_extension_free_string` | Free a returned `*mut c_char` |
| `neomind_extension_set_capability_bridge` | Host injects capability invoke/free fn ptrs |

> **Deprecated (ABI v2):** `neomind_extension_create` and `neomind_extension_destroy`.
> **Never export these manually** — extensions that do will crash the runner.

---

## Prelude

`use neomind_extension_sdk::prelude::*;` re-exports:

- `async_trait`, `json`, `Value`
- `Extension`, `ExtensionCapability`, `CapabilityContext`, `CapabilityManifest`,
  `AvailableCapabilities`, `ExtensionContext`, `ExtensionContextConfig`, `ClientInfo`,
  `StreamCapability`, `StreamSession`, `StreamStats`, `SessionStats`, `DataChunk`,
  `StreamResult`, `StreamError`, `StreamDataType`, `StreamDirection`, `FlowControl`,
  `PushOutputMessage`
- IPC types: `ExtensionMetadata`, `ExtensionError`, `ExtensionDescriptor`,
  `ExtensionCommand` (= `CommandDescriptor`), `ExtensionMetricValue`, `ExtensionStats`,
  `MetricDescriptor`, `MetricDataType`, `ParamMetricValue`, `ParameterDefinition`,
  `ParameterGroup`, `ValidationRule`, `CExtensionMetadata`, `ExtensionRuntimeState`,
  `ABI_VERSION`
- Builders: `MetricBuilder`, `CommandBuilder`
- Macros: `neomind_export`, `static_metadata`, `static_metrics`, `static_commands`,
  `metric_*!`, `ext_*!`
- Constants: `SDK_VERSION`, `SDK_ABI_VERSION`, `MIN_NEOMIND_VERSION`

---

## Common dependencies

```toml
[dependencies]
neomind-extension-sdk = { workspace = true }     # via the workspace Cargo.toml
serde = { workspace = true }
serde_json = { workspace = true }                # preserve_order feature is REQUIRED
async-trait = "0.1"
parking_lot = "0.12"                              # for sync locks in handle_event
tokio = { version = "1", features = ["rt", "sync", "macros"] }
chrono = "0.4"

# HTTP — pick ONE, do not mix
ureq = { version = "2" }                          # PREFERRED for cdylib extensions

# Async WS (for Python sidecars, HA bridge, etc.)
tokio-tungstenite = "0.21"

# ML
ort = { version = "2.0.0-rc.10" }                 # requires ONNX Runtime 1.22.x
```

### HTTP client — always `ureq`

**Do not use `reqwest` (async) inside an extension cdylib.** Async HTTP clients spin up
their own Tokio runtime, which collides with the runner's runtime and panics on load.
Use `ureq` (sync) and wrap blocking calls in `tokio::task::spawn_blocking` if you need
to call them from an async command handler:

```rust
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let url = args.get("url").and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidArguments("Missing url".into()))?;
    let inner = self.inner.clone();
    tokio::task::spawn_blocking(move || {
        let resp = ureq::get(url).call()
            .map_err(|e| ExtensionError::Io(e.to_string()))?;
        let body: serde_json::Value = resp.into_body().read_json()
            .map_err(|e| ExtensionError::Json(e.to_string()))?;
        Ok(body)
    }).await?
}
```
