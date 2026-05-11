# NeoMind Extension Development Guide

Complete guide for developing extensions for the **NeoMind extension runtime**.

[中文指南](EXTENSION_GUIDE.zh.md)

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Quick Start](#quick-start)
4. [Extension Trait Reference](#extension-trait-reference)
5. [Builder Patterns](#builder-patterns)
6. [Capability System](#capability-system)
7. [Streaming API](#streaming-api)
8. [Helper Macros](#helper-macros)
9. [Error Handling](#error-handling)
10. [Frontend Components](#frontend-components)
11. [Building & Deployment](#building--deployment)
12. [Safety Requirements](#safety-requirements)

---

## Overview

NeoMind extensions share one runtime model across Native and WASM targets.

### Key Features

- **Process Isolation Architecture**: All extensions run in isolated processes by default - crashes don't affect the main process
- **Shared Runtime Model**: Single codebase and runtime model for Native and WASM
- **Runtime Protocol v3**: Stable isolated extension protocol with improved safety
- **Builder Patterns**: Fluent API for defining metrics, commands, and parameters
- **Declarative Macros**: Reduce boilerplate with `neomind_export!`, `metric_float!`, etc.
- **Frontend Components**: React-based dashboard widgets
- **Stream Processing**: Support for real-time data streams (video, sensors, etc.)
- **Capability System**: Fine-grained access control for platform features

### Extension Types

| Type | File Extension | Use Case |
|------|---------------|----------|
| Native | `.dylib` / `.so` / `.dll` | Maximum performance, AI inference |
| WASM | `.wasm` | Cross-platform, sandboxed execution |

---

## Architecture

### V2 Process Isolation Architecture

All extensions run in isolated processes by default, ensuring system stability:

```
┌─────────────────────────────────────────────────────────────┐
│                   NeoMind Main Process                       │
│  ┌─────────────────────────────────────────────────────────┐│
│  │             UnifiedExtensionService                      ││
│  │  - IPC communication via stdin/stdout                    ││
│  │  - Manages lifecycle of all extensions                   ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                  Extension Runner Process                    │
│  - Your extension runs here in isolation                    │
│  - Native: loaded via FFI                                   │
│  - WASM: executed via wasmtime                              │
│  - Crashes don't affect main process                        │
└─────────────────────────────────────────────────────────────┘
```

### Benefits of Process Isolation

- **Crash Safety**: Extension crashes don't affect the main NeoMind process
- **Memory Isolation**: Each extension has its own memory space
- **Resource Limits**: CPU and memory can be limited per extension
- **Independent Lifecycle**: Extensions can be restarted without affecting others

### IPC Communication Protocol

The main process communicates with extension processes via JSON messages:

```json
// Execute command
{ "ExecuteCommand": { "command": "analyze", "args": {...}, "request_id": 1 } }

// Get metrics
{ "ProduceMetrics": { "request_id": 2 } }

// Stream processing
{ "InitStreamSession": { "session_id": "xxx", "config": {...} } }
```

---

## Quick Start

### 1. Create Extension Project

```bash
# Copy from template
cp -r extensions/weather-forecast-v2 extensions/my-extension
cd extensions/my-extension

# Update Cargo.toml
sed -i 's/weather-forecast-v2/my-extension/g' Cargo.toml
```

### 2. Configure Cargo.toml

```toml
[package]
name = "my-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
semver = "1"
```

If you build extensions inside a Cargo workspace, keep release profile settings at the workspace root `Cargo.toml`. Member-level `[profile.release]` sections are ignored by Cargo.

### 3. Implement Extension (Builder Pattern)

```rust
// src/lib.rs
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use neomind_extension_sdk::{MetricBuilder, CommandBuilder, ParamBuilder};
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,
}

impl MyExtension {
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("my-extension", "My Extension", "1.0.0")
                .with_description("My custom extension")
                .with_author("Your Name")
                .with_license("MIT")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("counter", "Counter")
                .integer()
                .build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("increment")
                .display_name("Increment")
                .description("Increment the counter")
                .param(
                    ParamBuilder::new("amount", MetricDataType::Integer)
                        .display_name("Amount")
                        .description("Amount to add")
                        .default(ParamMetricValue::Integer(1))
                        .build()
                )
                .sample(json!({ "amount": 1 }))
                .build(),
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "increment" => {
                let amount = args.get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({ "counter": new_value }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            metric_int!("counter", self.counter.load(Ordering::SeqCst)),
        ])
    }
}

// Export FFI - just one line!
neomind_extension_sdk::neomind_export!(MyExtension);
```

### 4. Build

```bash
cargo build --release
```

### 5. Install

```bash
cp target/release/libneomind_extension_my_extension.dylib ~/.neomind/extensions/
```

---

## Extension Trait Reference

### Required Methods

| Method | Returns | Sync/Async | Description |
|--------|---------|------------|-------------|
| `metadata()` | `&ExtensionMetadata` | Sync | Extension identity and version |
| `metrics()` | `Vec<MetricDescriptor>` | Sync | Metric descriptors |
| `commands()` | `Vec<ExtensionCommand>` | Sync | Command descriptors |
| `execute_command()` | `Result<Value>` | **Async** | Execute a named command |
| `produce_metrics()` | `Result<Vec<ExtensionMetricValue>>` | Sync | Produce current metric values |

### Optional Lifecycle Methods

| Method | Default | Description |
|--------|---------|-------------|
| `init(&mut self)` | `Ok(())` | Initialize extension (called once on load) |
| `start(&mut self)` | `Ok(())` | Start extension (after init) |
| `stop(&mut self)` | `Ok(())` | Graceful stop |
| `status(&self)` | `"unknown"` | Current status string |
| `health_check(&self)` | `Ok(true)` | Async health check |
| `configure(&mut self, config)` | `Ok(())` | Apply configuration changes |
| `get_stats(&self)` | `ExtensionStats::default()` | Extension statistics |
| `descriptor(&self)` | `None` | Optional descriptor with commands/metrics |

### Streaming Methods

| Method | Description |
|--------|-------------|
| `stream_capability(&self)` | Return `StreamCapability` if extension supports streaming |
| `latest_output(&self)` | Get latest output for push mode |
| `init_session(&self, session)` | Initialize a streaming session |
| `process_session_chunk(&self, id, chunk)` | Process a chunk in a session |
| `close_session(&self, id)` | Close a streaming session |
| `process_chunk(&self, chunk)` | Process a single chunk (stateless) |

### ExtensionMetadata Builder

```rust
ExtensionMetadata::new("my-extension", "My Extension", "1.0.0")
    .with_description("What it does")
    .with_author("Author Name")
    .with_homepage("https://example.com")
    .with_license("MIT")
    .with_config_parameters(vec![...])
```

### Extension ID Convention

```
{category}-{name}-v{major}

Examples:
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2
```

---

## Builder Patterns

The SDK provides fluent builder patterns to replace verbose struct construction.

### MetricBuilder

```rust
use neomind_extension_sdk::MetricBuilder;

// Integer metric
MetricBuilder::new("counter", "Counter")
    .integer()
    .unit("count")
    .min(0.0)
    .build()

// Float metric
MetricBuilder::new("temperature", "Temperature")
    .float()
    .unit("°C")
    .min(-40.0)
    .max(85.0)
    .build()

// Boolean metric
MetricBuilder::new("is_active", "Active")
    .boolean()
    .build()

// String metric
MetricBuilder::new("status", "Status")
    .string()
    .build()
```

| Method | Description |
|--------|-------------|
| `.integer()` / `.float()` / `.boolean()` / `.string()` | Set data type |
| `.unit(str)` | Set unit label |
| `.min(f64)` / `.max(f64)` | Set range |
| `.required()` | Mark as required |

### CommandBuilder

```rust
use neomind_extension_sdk::CommandBuilder;

CommandBuilder::new("analyze")
    .display_name("Analyze Image")
    .description("Run image analysis")
    .param(
        ParamBuilder::new("image_data", MetricDataType::String)
            .display_name("Image Data")
            .description("Base64-encoded image")
            .required()
            .build()
    )
    .param(
        ParamBuilder::new("threshold", MetricDataType::Float)
            .display_name("Confidence Threshold")
            .default(ParamMetricValue::Float(0.5))
            .min(0.0)
            .max(1.0)
            .build()
    )
    .sample(json!({ "image_data": "base64...", "threshold": 0.5 }))
    .build()
```

| Method | Description |
|--------|-------------|
| `.display_name(str)` | Human-readable name |
| `.description(str)` | What the command does |
| `.param(ParamDefinition)` | Add a parameter |
| `.param_simple(...)` | Shortcut for simple required parameter |
| `.param_optional(...)` | Shortcut for optional parameter |
| `.param_with_default(...)` | Shortcut with default value |
| `.sample(Value)` | Add example payload |

### ParamBuilder

```rust
use neomind_extension_sdk::ParamBuilder;

ParamBuilder::new("city", MetricDataType::String)
    .display_name("City")
    .description("City name")
    .required()
    .options(vec!["Beijing".into(), "Shanghai".into(), "New York".into()])
    .build()
```

| Method | Description |
|--------|-------------|
| `.display_name(str)` | Human-readable name |
| `.description(str)` | Parameter description |
| `.required()` / `.optional()` | Set requirement |
| `.default(MetricValue)` | Set default value |
| `.min(f64)` / `.max(f64)` | Set range |
| `.options(Vec<String>)` | Set allowed values (dropdown) |

---

## Capability System

NeoMind provides a **decoupled, versioned capability system** that allows extensions to access platform features safely.

### ExtensionCapability Enum

| Capability | Constant | Description |
|-----------|----------|-------------|
| `DeviceMetricsRead` | `device_metrics_read` | Read device metrics |
| `DeviceMetricsWrite` | `device_metrics_write` | Write device metrics (including virtual metrics) |
| `DeviceControl` | `device_control` | Send commands to devices |
| `StorageQuery` | `storage_query` | Query telemetry storage |
| `EventPublish` | `event_publish` | Publish events |
| `EventSubscribe` | `event_subscribe` | Subscribe to events |
| `TelemetryHistory` | `telemetry_history` | Query device telemetry history |
| `MetricsAggregate` | `metrics_aggregate` | Aggregate device metrics |
| `ExtensionCall` | `extension_call` | Call other extensions |
| `AgentTrigger` | `agent_trigger` | Trigger AI agents |
| `RuleTrigger` | `rule_trigger` | Trigger automation rules |
| `DeviceTemplateRegister` | `device_template_register` | Register device type templates |
| `DeviceRegister` | `device_register` | Register device instances |
| `DeviceUnregister` | `device_unregister` | Unregister device instances |
| `Custom(String)` | — | Custom capability |

### Virtual Metrics

Extensions can report custom metrics without requiring real hardware:

```rust
use neomind_extension_sdk::capabilities::device;

// Async context (e.g., in execute_command)
async fn report_metrics(&self) -> Result<()> {
    device::write_virtual_metric(
        "virtual-sensor-1",
        "temperature",
        25.5,
        None
    ).await?;
    Ok(())
}

// Sync context (e.g., in produce_metrics)
fn report_metrics_sync(&self) -> Result<()> {
    device::write_virtual_metric_sync(
        "virtual-sensor-1",
        "temperature",
        25.5,
        Some(chrono::Utc::now().timestamp_millis())
    )?;
    Ok(())
}
```

**When to use sync vs async:**
- Use `write_virtual_metric_sync()` in `produce_metrics()` and other non-async contexts
- Use `write_virtual_metric()` in async functions like `execute_command()`

---

## Streaming API

Extensions that process real-time data (video, sensors, etc.) can implement streaming.

### StreamCapability

```rust
fn stream_capability(&self) -> Option<StreamCapability> {
    Some(
        StreamCapability::push()
            .with_data_type(StreamDataType::Binary)
            .with_chunk_size(65536, 1048576)
    )
}
```

### Stream Modes

| Mode | Direction | Use Case |
|------|-----------|----------|
| `Stateless` | Any | Single-chunk processing without session |
| `Stateful` | Any | Session-based with `init_session` / `close_session` |
| `Push` | Output | Extension pushes data to client |

### Stream Directions

| Direction | Description |
|-----------|-------------|
| `Upload` | Client sends data to extension |
| `Download` | Extension sends data to client |
| `Bidirectional` | Both directions |

### Stream Data Types

| Type | Description |
|------|-------------|
| `Binary` | Raw binary data |
| `Text` | Text data |
| `Json` | JSON data |

### FlowControl

```rust
FlowControl {
    supports_backpressure: true,
    window_size: 16,
    supports_throttling: false,
    max_rate: 0,
}
```

---

## Helper Macros

### Metric Creation Macros

Replace verbose struct construction with one-liners:

```rust
use neomind_extension_sdk::{metric_float, metric_int, metric_bool, metric_string};

// Each macro auto-fills the timestamp
metric_float!("temperature", 25.5)   // ExtensionMetricValue with Float
metric_int!("counter", 42)           // ExtensionMetricValue with Integer
metric_bool!("is_active", true)      // ExtensionMetricValue with Boolean
metric_string!("status", "ok")       // ExtensionMetricValue with String
```

### Logging Macros

```rust
use neomind_extension_sdk::{ext_info, ext_warn, ext_error, ext_debug};

ext_info!("Extension started");
ext_warn!("Low memory: {}MB", mem);
ext_error!("Failed to load model: {}", err);
ext_debug!("Processing chunk {}/{}", i, total);
```

### Static Helpers

```rust
use neomind_extension_sdk::{static_metadata, static_metrics, static_commands};

// Create static metadata ( avoids repeated allocation)
static_metadata! {
    ExtensionMetadata::new("my-ext", "My Extension", "1.0.0")
        .with_description("...")
}
```

---

## Error Handling

### ExtensionError Variants

| Variant | When to Use |
|---------|-------------|
| `CommandNotFound(name)` | Unknown command name |
| `InvalidArguments(msg)` | Bad or missing parameters |
| `ExecutionFailed(msg)` | General execution failure |
| `NotSupported(msg)` | Feature not supported |
| `Timeout(msg)` | Operation timed out |
| `NotFound(msg)` | Resource not found |
| `InvalidFormat(msg)` | Data format error |
| `InferenceFailed(msg)` | ML model inference failure |
| `SessionNotFound(id)` | Stream session doesn't exist |
| `SessionAlreadyExists(id)` | Duplicate session |
| `InvalidStreamData(msg)` | Bad stream data |
| `LoadFailed(msg)` | Extension loading failure |
| `SecurityError(msg)` | Security violation |
| `ConfigurationError(msg)` | Configuration issue |
| `Io(msg)` | I/O error |
| `Json(msg)` | JSON parse/serialize error |

### Error Propagation Pattern

```rust
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    match command {
        "process_data" => {
            let data = args.get("data")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExtensionError::InvalidArguments("Missing data".into()))?;

            let result = self.process(data)
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Process failed: {}", e)))?;

            Ok(json!({ "result": result }))
        }
        _ => Err(ExtensionError::CommandNotFound(command.to_string())),
    }
}
```

### Panic Safety

```rust
// Recommended: Use ? operator
let value = self.get_value()?;

// Recommended: Use unwrap_or for defaults
let count = args.get("count").and_then(|v| v.as_i64()).unwrap_or(1);

// Avoid: Direct unwrap may cause extension process to exit
let value = some_option.unwrap();
```

---

## Frontend Components

### Project Structure

```
extensions/my-extension/frontend/
├── src/
│   └── index.tsx          # React component
├── package.json           # npm dependencies
├── vite.config.ts         # Vite build config
├── tsconfig.json          # TypeScript config
├── frontend.json          # Component manifest
└── README.md              # Component docs
```

### Component Template

```tsx
// src/index.tsx
import { forwardRef, useState, useCallback } from 'react'

// SDK Types
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
  /** Open a fullscreen dialog with arbitrary React content (provided by host) */
  openFullscreen?: (content: React.ReactNode) => void
  /** Close the fullscreen dialog (provided by host) */
  closeFullscreen?: () => void
}

export interface DataSource {
  type: string
  deviceId?: string
  device_id?: string
  extensionId?: string
  command?: string
  [key: string]: any
}

// API Helper
const EXTENSION_ID = 'my-extension'

async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>
): Promise<{ success: boolean; data?: T; error?: string }> {
  const apiBase = (window as any).__TAURI__
    ? 'http://localhost:9375/api'
    : '/api'

  const response = await fetch(`${apiBase}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args })
  })

  return response.json()
}

// Component
export const MyCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function MyCard(props, ref) {
    const { title = 'My Extension', dataSource, className = '', config } = props
    const [data, setData] = useState(null)

    const extensionId = dataSource?.extensionId || EXTENSION_ID

    return (
      <div ref={ref} className={`my-card ${className}`}>
        <style>{`
          .my-card {
            --ext-bg: rgba(255, 255, 255, 0.25);
            --ext-fg: hsl(240 10% 10%);
            --ext-muted: hsl(240 5% 40%);
            --ext-border: rgba(255, 255, 255, 0.5);
            --ext-accent: hsl(221 83% 53%);
          }
          .dark .my-card {
            --ext-bg: rgba(30, 30, 30, 0.4);
            --ext-fg: hsl(0 0% 95%);
            --ext-muted: hsl(0 0% 65%);
          }
        `}</style>
        <div className="my-card-content">
          <h3>{title}</h3>
          {/* Your component content */}
        </div>
      </div>
    )
  }
)

export default { MyCard }
```

### Frontend Manifest

```json
{
  "id": "my-extension",
  "version": "1.0.0",
  "entrypoint": "my-extension-components.umd.cjs",
  "components": [
    {
      "name": "MyCard",
      "type": "card",
      "displayName": "My Extension Card",
      "description": "Displays data from my extension",
      "defaultSize": { "width": 300, "height": 200 },
      "refreshable": true,
      "refreshInterval": 5000,
      "hasDataSource": true,
      "dataSourceAllowedTypes": ["device"],
      "configSchema": {
        "mode": {
          "type": "string",
          "title": "Display Mode",
          "enum": ["auto", "dark", "light"],
          "enumTitles": ["Auto", "Dark", "Light"],
          "default": "auto"
        }
      },
      "uiHints": {
        "fieldOrder": ["mode"],
        "visibilityRules": []
      }
    }
  ],
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

**Config dialog fields:**
- `hasDataSource: true` — Adds Data Source tab for device/data binding
- `dataSourceAllowedTypes` — Filter allowed types: `"device"`, `"device-metric"`, `"extension"`, etc.
- `configSchema` — Auto-generates form fields. Use `enum` + `enumTitles` for dropdowns
- `uiHints.visibilityRules` — Conditional field visibility based on other field values

### Build Frontend

```bash
cd frontend
npm install
npm run build
```

---

## Building & Deployment

### Build Commands

```bash
# Build all extensions
cargo build --release

# Build specific extension
cargo build --release -p my-extension

# Dev build + auto-install
./build.sh --dev --single my-extension

# Build all & package
./build.sh

# Release with version
./build.sh --release 2.4.0
```

### Installation

```bash
# Create extensions directory
mkdir -p ~/.neomind/extensions

# Copy native binary
cp target/release/libneomind_extension_my_extension.dylib ~/.neomind/extensions/

# Copy frontend (if exists)
cp -r extensions/my-extension/frontend/dist ~/.neomind/extensions/my-extension/frontend/
```

---

## Safety Requirements

### Panic Configuration

**Always set `panic = "unwind"` in the workspace root Cargo.toml:**

```toml
[profile.release]
panic = "unwind"  # REQUIRED! "abort" will crash the server on any panic
opt-level = 3
lto = "thin"
```

### Async Runtime Considerations

| Method | Type | `.await` Allowed? |
|--------|------|-------------------|
| `metadata()` | Sync | No |
| `metrics()` | Sync | No |
| `commands()` | Sync | No |
| `produce_metrics()` | Sync | **No** |
| `execute_command()` | Async | Yes |
| `health_check()` | Async | Yes |
| `configure()` | Async | Yes |

**Pattern:** Cache async results in atomic types for synchronous `produce_metrics()` access.

```rust
pub struct MyExtension {
    last_temperature: AtomicI64,  // Store as fixed-point (temp * 100)
}

// In async command:
self.last_temperature.store((temp * 100.0) as i64, Ordering::SeqCst);

// In produce_metrics (sync):
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    Ok(vec![
        metric_float!("temperature", self.last_temperature.load(Ordering::SeqCst) as f64 / 100.0),
    ])
}
```

### Resource Configuration (Optional)

Configure resource limits in `metadata.json`:

```json
{
  "id": "yolo-video-v2",
  "version": "2.0.0",
  "process_config": {
    "timeout_seconds": 60,
    "max_memory_mb": 1024,
    "restart_on_crash": true,
    "restart_delay_ms": 1000
  }
}
```

---

## Platform Support

| Platform | Architecture | Binary Extension |
|----------|--------------|------------------|
| macOS | ARM64 | `*.dylib` |
| macOS | x86_64 | `*.dylib` |
| Linux | x86_64 | `*.so` |
| Linux | ARM64 | `*.so` |
| Windows | x86_64 | `*.dll` |
| **Cross-platform** | Any | `*.wasm` |

---

## Troubleshooting

### Extension Not Loading

1. Check ABI version: `neomind_extension_abi_version()` must return 3
2. Verify binary format: Must match platform (.dylib for macOS, .so for Linux)
3. Check extension runner logs for IPC errors

### Extension Process Crashes

1. Check for `unwrap()` or `expect()` calls that may panic
2. Review error handling in command execution
3. Monitor memory usage if processing large data

### Frontend Not Displaying

1. Verify frontend.json exists in extension directory
2. Check component name matches in frontend.json
3. Verify UMD build output exists
4. Check component type is unique across all extensions

### Performance Issues

1. Use appropriate timeout values in process_config
2. Consider data chunking for large payloads
3. Cache results in produce_metrics() instead of async operations

---

## License

MIT License
