# NeoMind Extension Development Guide

Complete guide for developing extensions for the **NeoMind extension runtime**.

[中文指南](EXTENSION_GUIDE.zh.md)

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Quick Start](#quick-start)
4. [SDK Reference](#sdk-reference)
5. [Frontend Components](#frontend-components)
6. [Building & Deployment](#building--deployment)
7. [Safety Requirements](#safety-requirements)

---

## Overview

NeoMind extensions share one runtime model across Native and WASM targets.

### Key Features

- **Process Isolation Architecture**: All extensions run in isolated processes by default - crashes don't affect the main process
- **Shared Runtime Model**: Single codebase and runtime model for Native and WASM
- **Runtime Protocol v3**: Stable isolated extension protocol with improved safety
- **Declarative Macros**: Reduce boilerplate from 50+ lines to 5 lines
- **Frontend Components**: React-based dashboard widgets
- **Stream Processing**: Support for real-time data streams (video, sensors, etc.)

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
# Only need SDK dependency!
neomind-extension-sdk = { path = "../../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
semver = "1"
```

If you build extensions inside a Cargo workspace, keep release profile settings at the workspace root `Cargo.toml`. Member-level `[profile.release]` sections are ignored by Cargo.

### 3. Implement Extension

```rust
// src/lib.rs
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

// Your data types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyResult {
    pub value: i64,
    pub message: String,
}

// Your extension struct
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

// Implement the Extension trait
#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "my-extension".to_string(),
                name: "My Extension".to_string(),
                version: Version::parse("1.0.0").unwrap(),
                description: Some("My custom extension".to_string()),
                author: Some("Your Name".to_string()),
                homepage: None,
                license: Some("MIT".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "counter".to_string(),
                    display_name: "Counter".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: String::new(),
                    min: None,
                    max: None,
                    required: false,
                },
            ]
        })
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "increment".to_string(),
                    display_name: "Increment".to_string(),
                    payload_template: String::new(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to add".to_string(),
                            param_type: MetricDataType::Integer,
                            required: false,
                            default_value: Some(ParamMetricValue::Integer(1)),
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![json!({ "amount": 1 })],
                    llm_hints: "Increment the counter".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
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
        let now = chrono::Utc::now().timestamp_millis();
        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// Export FFI - Just one line!
// ============================================================================

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

## Capability System

NeoMind provides a **decoupled, versioned capability system** that allows extensions to access platform features safely.

### Virtual Metrics

Extensions can report custom metrics without requiring real hardware:

```rust
use neomind_extension_sdk::capabilities::device;

// Async context
async fn report_metrics(&self) -> Result<()> {
    device::write_virtual_metric(
        "virtual-sensor-1",
        "temperature",
        25.5,
        None
    ).await?;
    Ok(())
}

// Sync context (non-async function)
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

### Typed Virtual Metrics

For type-safe metric reporting:

```rust
use neomind_extension_sdk::capabilities::device;

// Report integer value
device::write_virtual_metric_typed_sync::<i64>(
    device_id,
    metric_name,
    value,
    Some(timestamp)
)?;

// Report float value
device::write_virtual_metric_typed_sync::<f64>(
    device_id,
    metric_name,
    value,
    Some(timestamp)
)?;
```

---

## SDK Reference

### FFI Interface

All extensions must export these functions:

| Function | Required | Description |
|----------|----------|-------------|
| `neomind_extension_abi_version()` | Yes | Return ABI version (3) |
| `neomind_extension_metadata()` | Yes | Return extension metadata |
| `neomind_extension_create()` | Yes | Create extension instance |
| `neomind_extension_destroy()` | Yes | Cleanup extension |

### Metadata Structure

```rust
#[repr(C)]
pub struct CExtensionMetadata {
    pub abi_version: u32,        // Must be 3
    pub id: *const c_char,       // Extension ID
    pub name: *const c_char,     // Display name
    pub version: *const c_char,  // Semantic version
    pub description: *const c_char,
    pub author: *const c_char,
    pub metric_count: usize,
    pub command_count: usize,
}
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
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
}

export interface DataSource {
  type: string
  extensionId?: string
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
    const { title = 'My Extension', dataSource, className = '' } = props
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
  "entrypoint": "my-extension-components.umd.js",
  "components": [
    {
      "name": "MyCard",
      "type": "card",
      "displayName": "My Extension Card",
      "description": "Displays data from my extension",
      "defaultSize": { "width": 300, "height": 200 },
      "refreshable": true,
      "refreshInterval": 5000
    }
  ],
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

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
cargo build --release -p neomind-my-extension

# Build for WASM target
cargo build --release --target wasm32-unknown-unknown
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

### Process Isolation (V2 Architecture)

**In V2, all extensions run in isolated processes by default** - no additional configuration needed. Benefits include:

- Crash isolation: Extension panics don't affect the NeoMind main process
- Memory isolation: Each extension has its own memory space
- Resource management: Monitor and limit resource usage per extension

### Panic Handling

With process isolation, extension panics won't crash the main process. However, follow these best practices:

```rust
// Recommended: Use ? operator for error propagation
fn process_data(&self) -> Result<Data> {
    let value = self.get_value()?;  // Instead of unwrap()
    Ok(Data::new(value))
}

// Recommended: Use unwrap_or for default values
let count = args.get("count").and_then(|v| v.as_i64()).unwrap_or(1);

// Avoid: Direct unwrap may cause extension process to exit
let value = some_option.unwrap();  // Not recommended
```

### Async Runtime Considerations

- `produce_metrics()` is a **synchronous method** - do NOT use `.await` inside
- `execute_command()` is an **async method** - can use `.await`

```rust
// ❌ Wrong: Using async in produce_metrics
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let data = block_on(async_fetch());  // Don't do this!
    // ...
}

// ✅ Correct: Cache data, return synchronously
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let data = self.cached_data.lock().unwrap().clone();
    Ok(vec![/* ... */])
}
```

### Resource Configuration (Optional)

To customize resource limits for extension processes, configure in `metadata.json`:

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

### Performance Issues

1. Use appropriate timeout values in process_config
2. Consider data chunking for large payloads
3. Cache results in produce_metrics() instead of async operations

---

## License

MIT License
