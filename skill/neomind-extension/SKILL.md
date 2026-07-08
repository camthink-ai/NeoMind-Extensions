---
name: neomind-extension
description: |
  Comprehensive guide for creating NeoMind Edge AI Platform extensions.

  Use this skill when:
  - Creating new NeoMind extensions from scratch (Rust cdylib)
  - Implementing extension commands, metrics, and event handlers
  - Building bridge extensions that import external systems (Modbus / LoRaWAN / HA / OPC UA / ONVIF / BACnet)
  - Building voice / TTS / ASR extensions with a Python sidecar
  - Using ChatStream / ChatSession capabilities for streaming LLM access
  - Adding React frontend components
  - Building cross-platform .nep packages
  - Implementing ML / YOLO / OCR extensions

  This skill teaches:
  - SDK v0.6 (ABI v3) Extension trait + builders
  - neomind_export! FFI entry point (never hand-write symbols)
  - CapabilityContext — invoking host capabilities (device_register, chat_stream, ...)
  - Event subscription + sync handle_event
  - Bridge pattern: auto device discovery + background polling + reconnect
  - Python sidecar pattern: Rust WS/HTTP client + external Python service
  - Cross-platform building for 6 platforms + hardware acceleration caveats

  Based on 23 production extensions: weather-forecast-v2, image-analyzer-v2,
  yolo-video-v2, yolo-device-inference, face-recognition, ocr-device-inference,
  paddle-ocr-vl, stream-player, deepstream, modbus/lorawan/homeassistant/opcua/onvif/bacnet-bridge,
  uink-rms-bridge, locate-anything-v2, voice-assistant, cosyvoice-3, moss-tts-nano,
  sensevoice-asr, voice-edge-tts, wasm-demo.

version: 3.0.0
argument-hint: "[extension-name]"
allowed-tools: [Read, Write, Edit, Bash, Glob, Grep, mcp__serena_serena__find_symbol, mcp__serena_serena__get_symbols_overview]
---

# NeoMind Extension Development Guide

Learn to create production-ready extensions for the NeoMind Edge AI Platform (SDK v0.6, ABI v3).

> **Companion docs in the repo root** — read before going deep:
> - `CLAUDE.md` — repo conventions, build scripts, version model
> - `EXTENSION_FRONTEND_DESIGN_GUIDE.md` — frontend CSS variables, dark mode, fallback
> - `HARDWARE_ACCELERATION.zh.md` — CoreML / CUDA / Jetson cross-platform ML deployment

## Quick Start

```bash
cd NeoMind-Extensions

# 1) Scaffold by copying the simplest reference impl that matches your goal
cp -r extensions/weather-forecast-v2 extensions/my-extension-v2
# (bridge? cp -r extensions/modbus-bridge)
# (voice? cp -r extensions/voice-edge-tts)
cd extensions/my-extension-v2
# Update Cargo.toml + src/lib.rs

# 2) Dev build + auto-install to ~/.neomind/extensions/
./build.sh --dev --single my-extension-v2

# 3) Release build with version in filenames
./build.sh --release 2.4.0      # or: ./release.sh 2.4.0
ls -lh dist/*.nep
```

---

## Essential Extension Structure

```
extensions/your-extension-v2/
├── Cargo.toml              # Project config (version = source of truth)
├── src/
│   └── lib.rs              # Extension trait impl + neomind_export!
├── frontend/               # Optional React components
│   ├── frontend.json       # Component definitions + configSchema
│   ├── src/index.tsx
│   ├── package.json
│   ├── vite.config.ts
│   └── dist/               # Built UMD bundle
├── models/                 # Optional ONNX / weights
└── README.md
```

---

## Step 1: Configure Cargo.toml

```toml
[package]
name = "your-extension-v2"
version = "2.0.0"
edition = "2021"

[lib]
name = "neomind_extension_your_extension_v2"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }     # preserve_order feature is REQUIRED (ABI compat)
async-trait = "0.1"
parking_lot = "0.12"                  # for sync locks in handle_event
tokio = { version = "1", features = ["rt", "sync"] }
chrono = "0.4"

# For HTTP — pick ONE, prefer ureq:
ureq = { version = "2" }              # sync HTTP client (safe in cdylib)
# ❌ Do NOT add reqwest — async clients can panic in cdylib runtime
```

**CRITICAL SAFETY:** Put this in the **workspace root** Cargo.toml (NeoMind-Extensions/Cargo.toml), not the member crate:

```toml
[profile.release]
panic = "unwind"      # REQUIRED — panic=abort crashes the whole runner
opt-level = 3
lto = "thin"
```

---

## Step 2: Basic Extension Template

```rust
// src/lib.rs
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use neomind_extension_sdk::{MetricBuilder, CommandBuilder, ParamBuilder, metric_int};
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct YourExtension {
    counter: AtomicI64,
}

impl YourExtension {
    pub fn new() -> Self { Self { counter: AtomicI64::new(0) } }
}
impl Default for YourExtension {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Extension for YourExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("your-extension-v2", "Your Extension", "2.0.0")
                .with_description("What it does")
                .with_author("Your Name")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("counter", "Counter")
                .integer()
                .unit("count")
                .min(0.0)
                .build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("your_command")
                .display_name("Your Command")
                .description("What this command does")
                .param(
                    ParamBuilder::new("param1", MetricDataType::String)
                        .display_name("Parameter 1")
                        .required()
                        .build(),
                )
                .sample(json!({"param1": "example"}))
                .build(),
        ]
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "your_command" => {
                let p1 = args.get("param1").and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing param1".into()))?;
                self.counter.fetch_add(1, Ordering::SeqCst);
                Ok(json!({
                    "result": "success",
                    "param1": p1,
                    "count": self.counter.load(Ordering::SeqCst),
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // SYNC — no .await allowed!
        Ok(vec![
            metric_int!("counter", self.counter.load(Ordering::SeqCst)),
        ])
    }
}

// One line — generates all ABI v3 FFI exports.
neomind_extension_sdk::neomind_export!(YourExtension);
```

### Key points
- `metrics()` / `commands()` return owned `Vec<...>` (not slices)
- `produce_metrics()` and `handle_event()` are **sync** — no `.await`
- `execute_command()` and `configure()` are **async** — free to `.await`
- Use `MetricBuilder` / `CommandBuilder` / `ParamBuilder` for fluent definitions
- Use `metric_int!` / `metric_float!` / `metric_bool!` / `metric_string!` macros
- Static-cache metadata in `OnceLock` so you can return `&ExtensionMetadata`
- Logging: `ext_info!`, `ext_warn!`, `ext_error!`, `ext_debug!`
- **Never hand-write `#[no_mangle]` FFI symbols** — `neomind_export!` does it correctly

Full API surface (every trait method, every enum variant) is in
[`reference/sdk-api.md`](reference/sdk-api.md).

---

## Step 3: Extension ID Convention

```
{category}-{feature}-v{major}
✅ weather-forecast-v2  ✅ modbus-bridge  ✅ voice-edge-tts  ✅ deepstream
❌ weather_forecast (use hyphens, not underscores)
```

`-v2` suffix indicates the current generation of the isolated runtime protocol.

---

## Step 4: Commands, Errors, and Metrics

### Command with required + optional params

```rust
CommandBuilder::new("read_registers")
    .display_name("Read Registers")
    .description("Read holding registers")
    .param(
        ParamBuilder::new("address", MetricDataType::Integer)
            .display_name("Start Address")
            .required()
            .min(0.0).max(65535.0)
            .build(),
    )
    .param(
        ParamBuilder::new("count", MetricDataType::Integer)
            .display_name("Count")
            .required()
            .min(1.0).max(125.0)
            .build(),
    )
    .sample(json!({"address": 40001, "count": 10}))
    .build()
```

### Error variants you'll use most

`CommandNotFound(name)` · `InvalidArguments(msg)` · `ExecutionFailed(msg)` ·
`NotSupported(msg)` · `Timeout(msg)` · `NotFound(name)` · `InvalidFormat(msg)` ·
`LoadFailed(msg)` · `SessionNotFound(id)` · `ConfigurationError(msg)` ·
`Io(msg)` · `Json(msg)` · `InferenceFailed(msg)`

(Full list of 23 variants in `reference/sdk-api.md`.)

### Pattern: cache async results for sync `produce_metrics()`

```rust
pub struct WeatherExtension {
    last_temp_c: AtomicI64,   // store as temp * 100
    last_humidity: AtomicI64,
    last_update: AtomicI64,
}

// async command updates the cache
async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
    let w = self.fetch_weather_sync(city)?;   // sync ureq call
    self.last_temp_c.store((w.temp_c * 100.0) as i64, Ordering::SeqCst);
    self.last_humidity.store(w.humidity as i64, Ordering::SeqCst);
    self.last_update.store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
    Ok(json!(w))
}

// sync produce_metrics reads from cache
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    Ok(vec![
        metric_float!("temperature_c", self.last_temp_c.load(Ordering::SeqCst) as f64 / 100.0),
        metric_int!("humidity_percent", self.last_humidity.load(Ordering::SeqCst)),
    ])
}
```

---

## Step 5: ML Model Extensions (YOLO / OCR / Face)

### Lazy load + keep loaded across sessions

Models are 100s of MB. Load on first use, **never** unload on session close — the runner
reclaims memory when the extension process exits.

```rust
pub struct YoloVideoExtension {
    detector: Arc<tokio::sync::Mutex<Option<YoloDetector>>>,  // lazy
    sessions: Arc<parking_lot::Mutex<HashMap<String, SessionData>>>,
}

impl YoloVideoExtension {
    async fn ensure_detector_loaded(&self) -> Result<()> {
        let mut d = self.detector.lock().await;
        if d.is_none() {
            *d = Some(YoloDetector::try_load(conf, iou, version, scale)
                .map_err(|e| ExtensionError::LoadFailed(e))?);
            ext_info!("[YOLO] model loaded");
        }
        Ok(())
    }
}

// ✅ Correct — close session but keep detector
async fn close_session(&self, sid: &str) -> Result<()> {
    self.sessions.lock().remove(sid);
    // detector stays loaded
    Ok(())
}

// ❌ WRONG — unloads the model
// async fn close_session(&self, sid: &str) -> Result<()> {
//     self.detector.lock().await.take();
//     Ok(())
// }
```

### Try-load pattern (degrades gracefully if model missing)

```rust
struct YoloDetector {
    model: Option<Runtime<YOLO>>,
    load_error: Option<String>,
}

impl YoloDetector {
    fn new(conf: f32, iou: f32) -> Self {
        match Self::try_load(conf, iou) {
            Ok(m)  => Self { model: Some(m), load_error: None },
            Err(e) => {
                ext_error!("[YOLO] load failed: {}", e);
                Self { model: None, load_error: Some(e) }
            }
        }
    }
}
```

### Hardware acceleration — read this before deploying

Cross-platform ML deployment has sharp edges:
- **macOS**: CoreML EP (auto-selected by `ort`)
- **Linux**: CUDA EP requires matching ONNX Runtime + driver version
- **Jetson (aarch64 Linux)**: **mainstream prebuilt `ort` crates do NOT work — you MUST
  recompile ONNX Runtime from source on the Jetson.** See `HARDWARE_ACCELERATION.zh.md`.
- **Windows**: CPU EP works out of the box; CUDA needs manual setup
- Current SDK uses `ort = "2.0.0-rc.10"` which requires **ONNX Runtime 1.22.x**

Read [`HARDWARE_ACCELERATION.zh.md`](../../../../CamThink%20Project/NeoMind-Extensions/HARDWARE_ACCELERATION.zh.md)
before building .nep packages for any platform you can't test locally.

---

## Step 6: Video Streaming Extensions

Implement `stream_capability()` + `process_session_chunk` / `init_session` / `close_session`:

```rust
fn stream_capability(&self) -> Option<StreamCapability> {
    Some(StreamCapability::push())   // for output-push video
}

async fn init_session(&self, session: &StreamSession) -> Result<()> {
    self.ensure_detector_loaded().await?;
    let s = VideoSession::new(session.id.clone(), self.detector.clone(), &session.config)?;
    self.sessions.lock().insert(session.id.clone(), s);
    Ok(())
}

async fn process_session_chunk(
    &self, session_id: &str, chunk: DataChunk,
) -> Result<StreamResult> {
    let sessions = self.sessions.lock();
    let s = sessions.get(session_id)
        .ok_or_else(|| ExtensionError::SessionNotFound(session_id.into()))?;
    s.process_frame(&chunk.data)
}

async fn close_session(&self, session_id: &str) -> Result<SessionStats> {
    self.sessions.lock().remove(session_id);
    // keep detector loaded!
    Ok(SessionStats::default())
}
```

### GStreamer / DeepStream pipelines

For real-time RTSP → detection → RTSP output, see the `deepstream` extension. Hard-won
lessons (all documented in the extension's own commit history):
- **Insert queues between every element** — without queues, encoders back-pressure the
  pipeline and it freezes after exactly 5 buffers.
- **Don't tee for snapshots** — teeing to a snapshot branch stalls the live pipeline.
  Use a separate on-demand GStreamer pipeline for snapshots.
- Pass a `snapshot_token` through `list_streams` / `get_stream_info` so the frontend can
  correlate snapshot HTTP responses with the stream.

---

## Step 7: Device Integration Extensions (event-driven)

For extensions that react to device updates (e.g. running inference on a camera image
whenever it changes):

```rust
#[async_trait]
impl Extension for YoloDeviceInference {
    fn event_subscriptions(&self) -> &[&str] {
        // Declare which event type names you want to receive.
        // Empty (the default) means the dispatcher silently drops everything.
        &["DeviceDataUpdated"]
    }

    // SYNC! Use parking_lot::RwLock, not tokio::Mutex.
    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        // The dispatcher wraps events in {event_type, payload: {...}, timestamp}.
        let inner = payload.get("payload").unwrap_or(payload);

        match event_type {
            "DeviceDataUpdated" => {
                let device_id = inner.get("device_id").and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("missing device_id".into()))?;
                // ... kick off inference on a tokio task; sync handoff to internal channel
            }
            _ => {}
        }
        Ok(())
    }
}
```

### Gotchas (all caused real bugs)

1. **Forgot to override `event_subscriptions()`** → default `&[]` → dispatcher filters out
   every event silently. Always override when you expect events.
2. **Used `tokio::Mutex` for shared state** → deadlock because `handle_event` is sync.
   Use `parking_lot::RwLock` / `parking_lot::Mutex` instead.
3. **Read `payload.get("session_id")` directly** → wrong level; the actual delivered shape
   is `{event_type, payload: {session_id, ...}, timestamp}`. Always unwrap with
   `payload.get("payload").unwrap_or(payload)` first.
4. **String casing on event types** — agent stream terminators emit `"type": "end"`
   (lowercase). Match both `"end"` and `"End"`.

---

## Step 8: Frontend Components (Optional)

> **Read [`EXTENSION_FRONTEND_DESIGN_GUIDE.md`](../../../../CamThink%20Project/NeoMind-Extensions/EXTENSION_FRONTEND_DESIGN_GUIDE.md)
> before writing frontend.** The rules below are the short version.

### Hard rules

1. **Never use Tailwind** — extension bundles don't ship Tailwind. Use NeoMind CSS
   variables for all colors: `var(--foreground)`, `var(--card)`, `var(--border)`, etc.
2. **Never hardcode colors** (`#fff`, `rgb(...)`) — they break dark mode.
3. **Primary button text must use `var(--{prefix}-on-primary)`**, not
   `var(--primary-foreground)` or `#fff`. See design guide §5.1.
4. **UMD format**, React/ReactDOM external (provided by host).
5. **Component `type` in `frontend.json` must be unique** — duplicate types collide in the
   UI. The build script auto-generates types as `{extension-name-without-v2}-card`.
6. Every component: `forwardRef`, handle loading/error/empty states, scoped CSS with
   extension-prefixed class names (`.weather-`, `.yolo-`, `.deep-stream-`).

### Minimal component

```tsx
// frontend/src/index.tsx
import { forwardRef, useState, useEffect } from 'react'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: {
    type: string
    deviceId?: string
    device_id?: string
    extensionId?: string
    command?: string
    config?: Record<string, any>
    [key: string]: any
  }
  className?: string
  config?: Record<string, any>
}

const getApiBase = (): string =>
  (typeof window !== 'undefined' && (window as any).__TAURI__)
    ? 'http://localhost:9375/api'
    : '/api'

async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>,
): Promise<{ success: boolean; data?: T; error?: string }> {
  const r = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args }),
  })
  return r.json()
}

export const YourExtensionCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function YourExtensionCard(props, ref) {
    const { dataSource, className = '' } = props
    const [data, setData] = useState<any>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const extensionId = dataSource?.extensionId || 'your-extension-v2'

    useEffect(() => {
      (async () => {
        setLoading(true); setError(null)
        const r = await executeExtensionCommand<any>(extensionId, 'your_command', {})
        r.success ? setData(r.data) : setError(r.error || 'Unknown error')
        setLoading(false)
      })()
    }, [extensionId])

    return (
      <div ref={ref} className={`your-ext-card ${className}`}>
        <style>{`
          .your-ext-card {
            --ext-bg: var(--card);
            --ext-fg: var(--foreground);
            --ext-muted: var(--muted-foreground);
            --ext-border: var(--border);
            --ext-accent: var(--primary);
            --ext-on-accent: var(--your-ext-on-primary);   /* design guide §5.1 */
            padding: 16px;
            border-radius: 8px;
            background: var(--ext-bg);
            color: var(--ext-fg);
            border: 1px solid var(--ext-border);
          }
          .your-ext-card button.primary {
            background: var(--ext-accent);
            color: var(--ext-on-accent);   /* not #fff */
          }
        `}</style>
        {loading && <div>Loading…</div>}
        {error && <div>Error: {error}</div>}
        {data && <pre>{JSON.stringify(data, null, 2)}</pre>}
      </div>
    )
  },
)

export default { YourExtensionCard }
```

### frontend.json

```jsonc
{
  "id": "your-extension-v2",
  "version": "2.0.0",
  "entrypoint": "your-extension-v2-components.umd.cjs",
  "components": [
    {
      "name": "YourExtensionCard",
      "type": "your-extension-card",       // MUST be unique across all extensions
      "displayName": "Your Extension Card",
      "description": "Displays data from your extension",
      "defaultSize": { "width": 340, "height": 320 },
      "minSize": { "width": 240, "height": 260 },
      "maxSize": { "width": 480, "height": 400 },
      "refreshable": true,
      "refreshInterval": 30000,
      "icon": "cpu",
      "hasDataSource": true,
      "dataSourceAllowedTypes": ["device"],
      "configSchema": {
        "contentType": {
          "type": "string", "title": "Content Type",
          "enum": ["none", "text", "markdown"],
          "enumTitles": ["None", "Plain Text", "Markdown"],
          "default": "none"
        },
        "textContent": { "type": "string", "title": "Text Content" }
      },
      "uiHints": {
        "fieldOrder": ["contentType", "textContent"],
        "visibilityRules": [
          { "field": "contentType", "condition": "equals", "value": "text",
            "thenShow": ["textContent"] }
        ]
      }
    }
  ],
  "dependencies": { "react": ">=18.0.0" }
}
```

### Vite config (UMD, React external)

```ts
// frontend/vite.config.ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: 'src/index.tsx',
      name: 'YourExtensionV2Components',
      formats: ['umd', 'cjs'],
      fileName: (format) =>
        `your-extension-v2-components.${format === 'umd' ? 'umd.js' : 'umd.cjs'}`,
    },
    rollupOptions: {
      external: ['react', 'react-dom', 'react/jsx-runtime'],
      output: { globals: { react: 'React', 'react-dom': 'ReactDOM' } },
    },
  },
})
```

---

## Step 9: Bridge Extensions

Bridge extensions import external systems (Modbus / LoRaWAN / Home Assistant / OPC UA /
ONVIF / BACnet / custom REST) into NeoMind's device model. Reference impls in
`extensions/{modbus,lorawan,homeassistant,opcua,onvif,bacnet}-bridge/`.

### Pattern summary

1. **Connect command** (`connect`, `add_device`) — user provides address + credentials,
   extension establishes a client and starts a background polling / listening task.
2. **Auto-discovery** — fetch device list from the external system; for each discovered
   device, register a NeoMind device via `device_register`.
3. **Background worker** — dedicated thread reads data continuously and updates an
   in-memory cache (`Arc<parking_lot::Mutex<DeviceState>>` or `Arc<RwLock<...>>`).
4. **`produce_metrics()` fan-out** — periodically writes per-device metrics via
   `device_metrics_write` capability.
5. **Resilience** — exponential backoff (typically 1s → 60s), separate counters for
   transient vs auth failures (HA bridge gives up after 5 auth failures).

### Capabilities used

Bridges invoke these dynamically via `CapabilityContext::default()`:

| Capability | When |
|---|---|
| `device_template_register` | Once on first device — registers the metric/command schema |
| `device_register` | For each discovered device |
| `device_metrics_write` | On every poll cycle, for each device metric |
| `device_unregister` | On `remove_device` / `disconnect` |

### Choosing your thread model

| Client type | Use | Spawn pattern | Example |
|---|---|---|---|
| Sync (modbus, ureq) | `std::thread::spawn` | `Builder::new().name(...).spawn(move || loop {...})` | modbus-bridge |
| Async (MQTT, WS) | `tokio::spawn` on the **host runtime** | `Handle::try_current()?.spawn(async move {...})` | lorawan-bridge, HA bridge WS loop |
| Mixed (REST + WS) | Sync REST from anywhere + async WS on host runtime | HA bridge |

> **Never call `tokio::runtime::Runtime::new()` inside an extension** — it collides with the
> runner's runtime. To run async work, grab the host's `Handle::try_current()` and spawn there.

### Skeleton (modbus-style sync polling)

```rust
use neomind_extension_sdk::host::CapabilityContext;
use serde_json::{json, Value};

pub struct ModbusBridge {
    devices: Arc<parking_lot::RwLock<HashMap<String, DeviceState>>>,
    template_registered: AtomicBool,
}

impl ModbusBridge {
    fn register_template(&self) {
        // Guard with atomic flag — idempotent
        if self.template_registered.swap(true, Ordering::SeqCst) {
            return;
        }
        let ctx = CapabilityContext::default();
        let tpl = json!({
            "device_type": "modbus_device",
            "name": "Modbus Device",
            "metrics": [
                { "name": "connected", "display_name": "Connected", "data_type": "String" },
                { "name": "poll_errors", "display_name": "Poll Errors", "data_type": "Integer" },
            ],
            "commands": [],
        });
        let r = ctx.invoke_capability("device_template_register", &tpl);
        if !r.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            self.template_registered.store(false, Ordering::SeqCst); // retry later
        }
    }

    fn register_device(&self, device_id: &str, name: &str) {
        let ctx = CapabilityContext::default();
        let _ = ctx.invoke_capability("device_register", &json!({
            "device_id": device_id,
            "name": name,
            "device_type": "modbus_device",
        }));
    }
}

// Background poller — std::thread, persistent connection, poll-level reconnect
fn polling_loop(
    config: DeviceConfig,
    state: Arc<parking_lot::Mutex<DeviceState>>,
    running: Arc<AtomicBool>,
) {
    let mut ctx: Option<sync::Context> = None;   // persistent connection
    while running.load(Ordering::SeqCst) {
        let interval = state.lock().config.poll_interval_ms;
        let start = std::time::Instant::now();

        if ctx.is_none() {
            ctx = connect_sync(&config).ok();
        }
        if let Some(ref mut c) = ctx {
            match c.read_holding_registers(addr, count) {
                Ok(data) => {
                    let mut s = state.lock();
                    s.register_values = parse(data);
                    s.poll_errors = 0;
                    s.connected = true;
                }
                Err(_) => {
                    ctx = None;   // force reconnect next cycle
                    state.lock().poll_errors += 1;
                }
            }
        }
        let elapsed = start.elapsed().as_millis() as u64;
        let sleep_ms = interval.saturating_sub(elapsed);
        std::thread::sleep(Duration::from_millis(sleep_ms));
    }
}

// Sync produce_metrics fans out per-device writes via capability
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let ctx = CapabilityContext::default();
    let now = chrono::Utc::now().timestamp_millis();
    let devices = self.devices.read();
    for (id, st) in devices.iter() {
        let _ = ctx.invoke_capability("device_metrics_write", &json!({
            "device_id": id, "metric": "connected",
            "value": if st.connected { "true" } else { "false" },
            "timestamp": now,
        }));
        for rv in &st.register_values {
            let _ = ctx.invoke_capability("device_metrics_write", &json!({
                "device_id": id, "metric": rv.name,
                "value": rv.value, "timestamp": now,
            }));
        }
    }
    Ok(vec![])  // extension-level metrics optional
}
```

### Reconnect strategies

- **HA bridge** — exponential backoff 1s → 60s, separate counter for `AuthFailed` (stops
  after 5), REST resync on every WS reconnect to catch missed `state_changed` events.
- **LoRaWAN bridge** — `rumqttc` auto-reconnects at MQTT level; extension-level 500ms → 30s
  backoff for stream errors; re-subscribes on ConnAck.
- **Modbus bridge** — poll-level retry; persistent TCP connection; full reconnect on
  read error.

---

## Step 10: Python Sidecar Extensions (Voice / TTS / ASR)

When you need Python libraries (CosyVoice, edge-tts, sherpa-onnx ASR, PaddleOCR, etc.),
keep Rust thin and put the AI logic in a separate Python service. Reference impls:
`voice-assistant`, `voice-edge-tts`, `cosyvoice-3`, `moss-tts-nano`, `sensevoice-asr`.

### Architecture

```
┌─────────────────┐   HTTP (ureq) or WS    ┌─────────────────┐
│  Rust Extension │ ◄────────────────────► │ Python Service  │
│   (cdylib)      │                        │ (FastAPI / WS)  │
│ • command handler│                       │ • ML models     │
│ • spawn_blocking│                        │ • TTS / ASR     │
└─────────────────┘                        └─────────────────┘
        ▲
        │ Platform APIs (CapabilityContext, metrics, ...)
        ▼
   NeoMind runtime
```

**The Python service is external** — not spawned by Rust. Operators start it separately
(`python server.py --port 9386`) and configure the URL via env var:
```bash
export VOICE_EDGE_TTS_SERVICE_URL=http://127.0.0.1:9386
```

This keeps Python crashes isolated and lets you iterate on Python without recompiling Rust.

### Pattern A: HTTP sidecar (simplest — voice-edge-tts)

```rust
pub struct VoiceEdgeTts {
    inner: Arc<Inner>,
}

struct Inner {
    service_url: parking_lot::RwLock<String>,
    http_agent: ureq::Agent,
    service_ok: AtomicBool,
    total_requests: AtomicI64,
}

impl VoiceEdgeTts {
    pub fn new() -> Self {
        let url = std::env::var("VOICE_EDGE_TTS_SERVICE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:9386".into());
        let http_agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        Self {
            inner: Arc::new(Inner {
                service_url: parking_lot::RwLock::new(url),
                http_agent,
                service_ok: AtomicBool::new(false),
                total_requests: AtomicI64::new(0),
            }),
        }
    }
}

#[async_trait]
impl Extension for VoiceEdgeTts {
    // ... metadata / commands ...

    async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
        match cmd {
            "synthesize" => {
                let inner = self.inner.clone();
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let voice = args.get("voice").and_then(|v| v.as_str()).unwrap_or("中文女").to_string();
                // SYNC HTTP inside spawn_blocking — never use async client in cdylib
                tokio::task::spawn_blocking(move || {
                    let url = format!("{}/tts", *inner.service_url.read());
                    let body = json!({ "text": text, "voice": voice });
                    let resp = inner.http_agent.post(&url)
                        .header("Content-Type", "application/json")
                        .send_json(&body)
                        .map_err(|e| ExtensionError::Io(e.to_string()))?;
                    let wav = resp.into_body().read_to_vec()
                        .map_err(|e| ExtensionError::Io(e.to_string()))?;
                    inner.total_requests.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({
                        "audio_base64": base64::encode(&wav),
                        "format": "wav",
                        "sample_rate": 24000,
                    }))
                }).await?
            }
            "health" => {
                let inner = self.inner.clone();
                let ok = tokio::task::spawn_blocking(move || inner.check_health()).await?;
                Ok(json!({ "ok": ok, "service_url": *self.inner.service_url.read() }))
            }
            _ => Err(ExtensionError::CommandNotFound(cmd.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            metric_int!("service_ok", self.inner.service_ok.load(Ordering::SeqCst) as i64),
            metric_int!("total_requests", self.inner.total_requests.load(Ordering::SeqCst)),
        ])
    }
}
```

### Pattern B: WebSocket sidecar (real-time bidirectional — voice-assistant)

Use when you need continuous streaming (PCM audio in/out, ASR → LLM → TTS pipeline).
See `extensions/voice-assistant/src/lib.rs` — `run_session_pump` is the canonical WS
loop with `tokio::select!` over browser PCM input and Python event output.

Frame types in voice-assistant (Rust ↔ Python):
- **Rust → Python**: `start {session_id, sample_rate, channels, format}`,
  binary PCM frames, `chat_stream_request {message, session_id?}`,
  `chat_stream_cancel {session_id}`
- **Python → Rust**: `transcript {text, language, elapsed_ms}`,
  `tts_start` / `tts_end`, binary PCM frames, `barge_in`, `stop {reason}`,
  `error {phase, message}`, `chat_chunk` / `chat_stream_end` (forwarded from ChatStream)

### When HTTP vs WS

| Aspect | HTTP sidecar | WebSocket sidecar |
|---|---|---|
| Use case | One-shot TTS / ASR / OCR | Real-time bidirectional streaming |
| Client | `ureq` (sync, in `spawn_blocking`) | `tokio-tungstenite` (async) |
| Latency | Per-request overhead | Persistent connection |
| State | Stateless | Per-session |
| Reference | `voice-edge-tts`, `cosyvoice-3` | `voice-assistant` |

---

## Step 11: Platform Capabilities (ChatStream / Storage / Telemetry)

Beyond plain commands, extensions can invoke host capabilities via
`CapabilityContext::default().invoke_capability(name, &json)`. The SDK also provides
typed helpers in `neomind_extension_sdk::capabilities::{device, chat}`.

### ChatStream — streaming LLM without managing API tokens

The biggest capability addition. Two usage patterns:

#### Phase 1: one-shot (simplest)

```rust
use neomind_extension_sdk::capabilities::chat;
use neomind_extension_sdk::host::CapabilityContext;

let ctx = CapabilityContext::default();
let result = chat::invoke(&ctx, "Hello, what's the weather?", None).await?;
let sid = result["session_id"].as_str().unwrap();
// LLM tokens arrive as AgentStreamChunk events (see Step 7 / Step 12)
```

#### Phase 2: persistent session (multi-turn)

```rust
let ctx = CapabilityContext::default();

// 1. Open (or reuse) a session
let open = chat::open_session(&ctx, None).await?;     // {session_id, created}
let sid = open["session_id"].as_str().unwrap().to_string();

// 2. Send a turn — returns immediately with turn_id
let turn = chat::send_message(&ctx, &sid, "What about tomorrow?").await?;
let turn_id = turn["turn_id"].as_str().unwrap().to_string();
// Tokens stream in via AgentStreamChunk events tagged with this turn_id

// 3. (optional) Cancel an in-flight turn without closing the session
chat::cancel_turn(&ctx, &sid, Some(&turn_id)).await?;

// 4. Close when truly done
chat::close_session(&ctx, &sid).await?;
```

**Events you must subscribe to** (override `event_subscriptions()`):

```rust
fn event_subscriptions(&self) -> &[&str] {
    &["AgentStreamChunk", "AgentStreamEnd"]
}
```

- `AgentStreamChunk` — one token / chunk. Payload: `{session_id, chunk: {type, content}, timestamp}`.
  `chunk.type` can be `"Content"`, `"reasoning"`, or `"end"` (lowercase!) — `"end"` is **not**
  authoritative on its own because reasoning models emit intermediate ends.
- `AgentStreamEnd` — authoritative terminator. Payload: `{session_id, reason, error, timestamp}`.
  Use this to clean up session state.

### Other useful capabilities

| Capability | Helper | Purpose |
|---|---|---|
| `device_metrics_read` | `device::get_metrics` | Read another device's current metrics |
| `device_metrics_write` | `device::write_virtual_metric` | Write virtual metrics for your own devices |
| `device_control` | `device::send_command` | Send control commands to devices |
| `telemetry_history` | `device::query_telemetry_last_24h` | Historical metric data |
| `metrics_aggregate` | `device::aggregate_avg_24h` | Aggregated metrics (avg / min / max) |
| `storage_query` | — | Query platform storage |
| `extension_call` | — | Call commands on other extensions |
| `event_publish` | — | Publish events to other extensions |
| `agent_trigger` | — | Trigger NeoMind AI agents |
| `rule_trigger` | — | Trigger automation rules |

See `reference/sdk-api.md` → "CapabilityContext" for the full list and exact return shapes.

---

## Step 12: Python↔Rust ChatStream Bridge (voice-assistant full loop)

Real-world pattern from `voice-assistant`: Python side wants streaming LLM access without
holding tokens. The Rust side brokers between Python WS frames and the platform
ChatStream capability.

```
Python ──chat_stream_request──► Rust ──chat_session_open──► Platform
                              Rust ◄──{session_id}── Platform
                              Rust ──chat_session_send──► Platform
                              Rust ◄──{turn_id}── Platform
Rust ←──chat_session_turn_started {sid, turn_id}── (Rust → Python)
                              Rust ←──AgentStreamChunk── Platform   (event)
Rust →──chat_chunk {sid, chunk[turn_id]}──► Python   (per chunk)
                              Rust ←──AgentStreamEnd── Platform     (event)
Rust →──chat_stream_end {sid, reason}──► Python
```

Key implementation rules:
- Store per-session state in `Arc<parking_lot::RwLock<HashMap<String, mpsc::Sender<...>>>>`
  keyed by `session_id`. Do **not** remove the entry on every turn — only on WS teardown.
  Otherwise you force a redundant `chat_session_open` round-trip per turn.
- Barge-in (user interrupts TTS): send `chat_stream_cancel_turn` (keeps session alive).
- On WS close: send `chat_session_close` then remove the chat_streams entry.
- `handle_event` is sync — use `parking_lot::RwLock` and `try_send` (not `.await`).

---

## Building & Packaging

### Unified build script

```bash
./build.sh                              # Build all + create packages
./build.sh --dev                        # Dev build + auto-install to ~/.neomind/extensions/
./build.sh --dev --single my-ext-v2     # Single extension dev build
./build.sh --release 2.4.0              # Release with version in filenames
./build.sh --single my-ext-v2           # Single extension release
./build.sh --skip-frontend              # Skip frontend builds
./build.sh --debug                      # Debug build
./build.sh --help                       # All options
```

### Release process (version model)

The repo has **three version layers** that must all match on release:

| File | Meaning |
|---|---|
| `VERSION` | Market release version (e.g. `2.7.0`) |
| `extensions/index.json` → `version` | Market release version |
| `extensions/*/Cargo.toml` → `version` | Per-extension version (drives .nep filename) |
| `extensions/*/metadata.json` → `version` | Auto-generated from Cargo.toml |

```bash
VERSION=2.7.0

# Step 1: Sync Cargo.toml + VERSION + regenerate JSON files
./scripts/update-versions.sh $VERSION --bump-extensions

# Step 2: Verify consistency (MUST pass!)
./scripts/update-versions.sh $VERSION --check

# Step 3: Commit
git add . && git commit -m "chore: bump to v$VERSION"

# Step 4: Build + package
./build.sh --release $VERSION

# Step 5: Verify filenames
ls dist/*.nep
# e.g. weather-forecast-v2-2.7.0-darwin_aarch64.nep

# Step 6: Tag + publish
git tag v$VERSION
git push origin main --tags
gh release create v$VERSION ./dist/*.nep --title "v$VERSION"
```

### .nep package structure

```
your-extension-v2-2.0.0-darwin_aarch64.nep   (ZIP)
├── manifest.json
├── binaries/
│   └── darwin_aarch64/libneomind_extension_your_extension_v2.dylib
└── frontend/
    └── your-extension-v2-components.umd.cjs
```

### Platform matrix

| Platform | Binary | Target |
|---|---|---|
| macOS ARM64 | `*.dylib` | `aarch64-apple-darwin` |
| macOS x86_64 | `*.dylib` | `x86_64-apple-darwin` |
| Linux x86_64 | `*.so` | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `*.so` | `aarch64-unknown-linux-gnu` |
| Windows x86_64 | `*.dll` | `x86_64-pc-windows-msvc` |
| Windows x86 | `*.dll` | `i686-pc-windows-msvc` |

GitHub Actions (`.github/workflows/build-nep-packages.yml`) builds all 6 automatically.

---

## Safety Requirements

### `panic = "unwind"` is mandatory

Without it, any panic aborts the entire NeoMind server. Set in the **workspace root**
`Cargo.toml` (or crate root if standalone):

```toml
[profile.release]
panic = "unwind"
opt-level = 3
lto = "thin"
```

### Don't block in async commands

```rust
// ❌ Bad — blocks the executor
async fn execute_command(&self, ...) -> Result<Value> {
    std::thread::sleep(Duration::from_secs(5));
    Ok(json!({}))
}

// ✅ Good
async fn execute_command(&self, ...) -> Result<Value> {
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok(json!({}))
}

// ✅ Also good — sync work wrapped in spawn_blocking
async fn execute_command(&self, ...) -> Result<Value> {
    tokio::task::spawn_blocking(|| { /* sync work */ }).await?
}
```

### HTTP clients — always `ureq`

Async HTTP clients (`reqwest`, `hyper`) create their own Tokio runtime, which panics
inside a cdylib. Use `ureq` (sync). If you must call it from an async command, wrap in
`tokio::task::spawn_blocking`. See `reference/sdk-api.md` for the canonical pattern.

### Thread safety

- `Arc<parking_lot::Mutex<T>>` for state shared between sync (`handle_event`) and async
  (`execute_command`) code paths.
- `Arc<tokio::sync::Mutex<T>>` only when the lock is held across `.await` points AND never
  touched from `handle_event`.
- `Arc<AtomicI64>` / `AtomicBool` for simple counters / flags.

---

## Marketplace Release Checklist (CRITICAL)

### 1. Component `type` MUST be unique across all extensions

```bash
# Verify in every .nep you're about to ship:
unzip -p dist/your-extension-2.3.1-darwin_aarch64.nep manifest.json \
  | jq '.frontend.components[].type'
```

Duplicate types cause only one of the conflicting extensions to appear in the UI. The
build script auto-generates the type as `{extension-name-without-v2}-card`.

### 2. `frontend.components` in metadata.json MUST be a string array

```jsonc
// ❌ WRONG — full objects (causes "Failed to load details")
{ "frontend": { "components": [{ "name": "MyCard", "type": "card" }] } }

// ✅ CORRECT — names only
{ "frontend": { "components": ["MyCard"], "entrypoint": "..." } }
```

`FrontendInfo` in NeoMind parses `Vec<String>`.

### 3. Build URLs MUST use the market version, not the extension version

```jsonc
// ❌ Wrong (release v2.7.0 but URL has extension's 2.0.0)
"builds": { "darwin-aarch64": {
  "url": ".../v2.7.0/my-extension-2.0.0-darwin_aarch64.nep" } }

// ✅ Correct
"builds": { "darwin-aarch64": {
  "url": ".../v2.7.0/my-extension-2.7.0-darwin_aarch64.nep" } }
```

### 4. Pre-release script

```bash
#!/bin/bash
VERSION="2.7.0"

echo "=== metadata.json checks ==="
for d in extensions/*/; do
    [ -f "$d/metadata.json" ] || continue
    ext=$(basename "$d")
    # 1. components is string array?
    if jq -e '.frontend.components[0] | type == "object"' "$d/metadata.json" >/dev/null 2>&1; then
        echo "  ❌ $ext: frontend.components has objects, should be strings"
    fi
    # 2. URL version matches?
    url=$(jq -r '.builds["darwin-aarch64"].url' "$d/metadata.json")
    if [[ "$url" == *"-2.0.0-"* ]] && [ "$VERSION" != "2.0.0" ]; then
        echo "  ❌ $ext: URL has wrong version (2.0.0 vs $VERSION)"
    fi
done

echo "=== .nep manifest type uniqueness ==="
for nep in dist/*.nep; do
    [ -f "$nep" ] || continue
    t=$(unzip -p "$nep" manifest.json | jq -r '.frontend.components[0].type')
    echo "$(basename "$nep"): type=$t"
done | sort | uniq -c -f1   # flag duplicates
```

---

## Troubleshooting

### Extension not loading
1. Verify ABI version 3 — `neomind_export!` does this; don't hand-write the symbol.
2. Verify binary format matches platform (`.dylib` macOS, `.so` Linux, `.dll` Windows).
3. Check runner logs: `neomind logs --extension your-extension-v2`.
4. Old extensions exporting `_neomind_extension_create` / `_destroy` will crash — delete &
   reinstall from the marketplace.

### Process crashes
1. Check for `unwrap()` / `expect()` that could panic on bad input.
2. Ensure `panic = "unwind"` in workspace root Cargo.toml.
3. Monitor memory — leaks in long-running background threads are the usual suspect.

### Events silently dropped
1. Did you override `event_subscriptions()`? Default is `&[]` → all events filtered.
2. In `handle_event`, did you unwrap the envelope? `payload.get("payload").unwrap_or(payload)`.
3. Are you using `parking_lot::RwLock`? `tokio::Mutex` deadlocks from sync `handle_event`.

### Frontend not displaying
1. `frontend.json` exists and is valid JSON.
2. Component `name` matches what your UMD exports.
3. UMD bundle exists in `frontend/dist/`.
4. Component `type` is unique (see checklist above).
5. `metadata.json` has `frontend.components` as `["Name"]` not `[{...}]`.
6. Build URLs use the market version.
7. Run `./scripts/update-versions.sh <version>` to regenerate correct metadata.json.

### "Failed to load details" in marketplace
Usually caused by bad metadata.json format. Regenerate with
`./scripts/update-versions.sh <version>` and wait 5–10 minutes for GitHub CDN.

### Model loading issues
1. Check `models/` directory exists in the .nep.
2. For Jetson: see `HARDWARE_ACCELERATION.zh.md` — prebuilt `ort` doesn't work, must
   recompile ONNX Runtime.
3. `NEOMIND_EXTENSION_DIR` env var points to the extension's install dir at runtime —
   use it to locate model files: `std::env::var("NEOMIND_EXTENSION_DIR")`.

---

## Real Extension Examples (by category)

### Simple API client
- **weather-forecast-v2** — sync HTTP (ureq), metric caching, config parameters.
  Best template for new API-polling extensions.

### ML inference (image)
- **image-analyzer-v2** — YOLOv8 base64 image input, lazy model load with graceful fallback.
- **yolo-device-inference** — event-driven; runs inference when bound device updates.
- **face-recognition** — face embedding + matching across device events.
- **ocr-device-inference** — OCR on device-supplied images.
- **paddle-ocr-vl** — PaddleOCR-VL multimodal (Python sidecar + Rust wrapper).

### Video / streaming
- **yolo-video-v2** — `StreamCapability` + MJPEG push, keep detector across sessions.
- **stream-player** — generic stream player.
- **deepstream** — GStreamer pipeline (RTSP → detection → RTSP output), on-demand
  snapshot via one-shot pipeline. Read its commit history before touching GStreamer.

### Bridge extensions
- **modbus-bridge** — sync `tokio-modbus`, `std::thread` polling, persistent TCP. **Best
  template for new bridges.**
- **lorawan-bridge** — async `rumqttc` MQTT, Cayenne LPP decoder, ChirpStack v3/v4 + TTN.
- **homeassistant-bridge** — REST (ureq) + WebSocket (tokio-tungstenite) dual-channel.
- **opcua-bridge** / **onvif-bridge** / **bacnet-bridge** — same pattern, different protocol.
- **uink-rms-bridge** — REST bridge to a custom backend.

### Voice / TTS / ASR (Python sidecar)
- **voice-edge-tts** — HTTP sidecar (ureq + spawn_blocking). **Best template for new
  voice extensions.**
- **cosyvoice-3** / **moss-tts-nano** — same HTTP pattern, different Python backend.
- **sensevoice-asr** — HTTP sidecar for ASR.
- **voice-assistant** — WebSocket sidecar; full VAD → ASR → ChatStream → TTS pipeline.
  Read `lib.rs` `run_session_pump` before writing anything similar.

### Other
- **locate-anything-v2** — location/geo extension.
- **wasm-demo** — WASM extension experiment.

---

## Quick Reference

### File locations
- **Extensions**: `NeoMind-Extensions/extensions/`
- **SDK**: `NeoMind/crates/neomind-extension-sdk/`
- **Install location**: `~/.neomind/extensions/`
- **Design guide**: `NeoMind-Extensions/EXTENSION_FRONTEND_DESIGN_GUIDE.md`
- **HW accel guide**: `NeoMind-Extensions/HARDWARE_ACCELERATION.zh.md`

### Essential commands
```bash
./build.sh --dev --single your-extension-v2   # Dev build + auto-install
cargo build --release -p your-extension-v2    # Manual build
./build.sh --release 2.4.0                    # Release build
./scripts/update-versions.sh 2.4.0 --check    # Version consistency check
neomind logs --extension your-extension-v2    # View logs
```

### Extension trait method sync/async map
| Category | Methods |
|---|---|
| Sync | `metadata` · `metrics` · `commands` · `produce_metrics` · `get_stats` · `status` · `init` · `start` · `stop` · `event_subscriptions` · `handle_event` |
| Async | `execute_command` · `configure` · `health_check` · `on_unload` · `process_chunk` · `init_session` · `process_session_chunk` · `close_session` · `start_push` · `stop_push` |

### Capability name → SDK constant
`device_metrics_read` / `device_metrics_write` / `device_control` / `storage_query` /
`event_publish` / `event_subscribe` / `telemetry_history` / `metrics_aggregate` /
`extension_call` / `agent_trigger` / `chat_stream` / `chat_stream_cancel` /
`chat_session_open` / `chat_session_send` / `chat_session_close` /
`chat_stream_cancel_turn` / `rule_trigger` / `device_template_register` /
`device_register` / `device_unregister` / `custom:*`

---

## License

MIT License
