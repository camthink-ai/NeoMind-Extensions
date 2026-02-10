# NeoMind Extension Development Guide

This guide explains how to develop, build, and install extensions for the NeoMind Edge AI Platform.

---

## Table of Contents

1. [Overview](#overview)
2. [Extension Architecture](#extension-architecture)
3. [Project Structure](#project-structure)
4. [Core Concepts](#core-concepts)
5. [Available APIs](#available-apis)
6. [Building and Installation](#building-and-installation)
7. [Testing](#testing)
8. [Best Practices](#best-practices)

---

## Overview

A NeoMind Extension is a dynamic library (`.dylib`, `.so`, `.dll`) that extends the platform's capabilities. Extensions can provide:

| Capability | Description |
|-----------|-------------|
| **Tools** | Functions that AI agents can call (e.g., query_weather) |
| **Metrics** | Time-series data points (e.g., temperature, humidity) |
| **Commands** | RPC-style commands (e.g., refresh, configure) |
| **Channels** | Event notification channels |

---

## Extension Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    NeoMind Server                        │
├─────────────────────────────────────────────────────────┤
│  Extension Registry                                      │
│  ├─ Discovery (scan directories)                         │
│  ├─ Loading (dlopen)                                     │
│  └─ Lifecycle Management                                 │
├─────────────────────────────────────────────────────────┤
│  Extension Safety                                        │
│  ├─ Circuit Breaker (5 failures → open)                  │
│  ├─ Panic Isolation (catch_unwind)                       │
│  └─ Health Monitoring                                    │
├─────────────────────────────────────────────────────────┤
│  Your Extension (Dynamic Library)                        │
│  └─ Implements: Extension Trait                         │
│      ├─ metadata()                                       │
│      ├─ capabilities()                                   │
│      ├─ execute_command()                                │
│      ├─ execute_tool()                                   │
│      ├─ health_check()                                   │
│      └─ produce_metrics()                                │
└─────────────────────────────────────────────────────────┘
```

---

## Project Structure

```
my-extension/
├── Cargo.toml          # Package configuration
├── build.rs            # Optional build script
├── src/
│   └── lib.rs          # Extension implementation
└── README.md           # Extension documentation
```

### Minimal Cargo.toml

```toml
[package]
name = "neomind-my-extension"
version = "0.1.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib"]   # Important: produces dynamic library

[dependencies]
neomind-extension-sdk = { path = "../NeoTalk/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

---

## Core Concepts

### 1. Extension Trait

Every extension must implement the `Extension` trait:

```rust
use neomind_extension_sdk::prelude::*;

pub struct MyExtension {
    // Your internal state here
}

impl Extension for MyExtension {
    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: "my.extension".to_string(),
            name: "My Extension".to_string(),
            version: "0.1.0".to_string(),
            author: "Your Name".to_string(),
            description: "Does something useful".to_string(),
            license: "MIT".to_string(),
            homepage: None,
        }
    }

    fn capabilities(&self) -> Vec<ExtensionCapabilityDescriptor> {
        vec![
            ExtensionCapabilityDescriptor {
                id: "my_tool".to_string(),
                name: "My Tool".to_string(),
                description: "What this tool does".to_string(),
                capability_type: ExtensionCapabilityType::Tool,
                config_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" }
                    }
                })),
            },
        ]
    }

    fn execute_tool(&self, tool: &str, args: &Value) -> Result<Value, ExtensionError> {
        match tool {
            "my_tool" => {
                // Implement tool logic
                Ok(json!({ "result": "success" }))
            }
            _ => Err(ExtensionError::ToolNotFound(tool.to_string())),
        }
    }

    // ... other required methods
}
```

### 2. Capability Types

```rust
pub enum ExtensionCapabilityType {
    Tool,      // AI agent can call
    Command,   // RPC-style command
    Metric,    // Time-series data
    Channel,   // Event notification
}
```

### 3. Error Handling

```rust
pub enum ExtensionError {
    NotFound(String),           // Resource not found
    InvalidInput(String),       // Invalid parameters
    ExecutionFailed(String),    // Runtime error
    ToolNotFound(String),       // Unknown tool
    CommandNotFound(String),    // Unknown command
    IoError(String),            // I/O error
}
```

---

## Available APIs

### Extension Metadata

```rust
pub struct ExtensionMetadata {
    pub id: String,              // Unique ID (e.g., "my.company.extension")
    pub name: String,            // Display name
    pub version: String,         // SemVer version
    pub author: String,          // Author name
    pub description: String,     // Short description
    pub license: String,         // License identifier
    pub homepage: Option<String>, // Project URL
}
```

### Capability Descriptor

```rust
pub struct ExtensionCapabilityDescriptor {
    pub id: String,                    // Unique capability ID
    pub name: String,                  // Display name
    pub description: String,           // What it does
    pub capability_type: ExtensionCapabilityType,
    pub config_schema: Option<Value>,  // JSON Schema for parameters
}
```

### Metric Descriptor

```rust
pub struct MetricDescriptor {
    pub id: String,              // Metric ID (e.g., "temperature_c")
    pub name: String,            // Display name
    pub description: String,     // What it measures
    pub unit: String,            // Unit (e.g., "°C", "%", "km/h")
    pub data_type: String,       // "float", "integer", "string", "boolean"
}
```

### Extension Trait Methods

| Method | Description |
|--------|-------------|
| `metadata()` | Return extension metadata |
| `capabilities()` | List all capabilities (tools, commands, etc.) |
| `metrics()` | List all provided metrics |
| `execute_command()` | Execute a command (blocking) |
| `execute_tool()` | Execute a tool (for AI agents) |
| `health_check()` | Return true if extension is healthy |
| `produce_metrics()` | Return current metric values |

---

## Building and Installation

### 1. Build the Extension

```bash
cd ~/NeoMind-Extension
cargo build --release
```

Output location:
- macOS: `target/release/libneomind_extension_weather_forecast.dylib`
- Linux: `target/release/libneomind_extension_weather_forecast.so`
- Windows: `target/release/neomind_extension_weather_forecast.dll`

### 2. Install the Extension

```bash
# Create extensions directory if it doesn't exist
mkdir -p ~/.neomind/extensions

# Copy the compiled extension
cp target/release/libneomind_extension_weather_forecast.* ~/.neomind/extensions/
```

### 3. Restart NeoMind

The extension will be auto-discovered and loaded on startup.

---

## Testing

### Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_creation() {
        let ext = WeatherExtension::new(&json!({})).unwrap();
        assert_eq!(ext.metadata().id, "neomind.weather.forecast");
    }

    #[test]
    fn test_tool_execution() {
        let ext = WeatherExtension::new(&json!({})).unwrap();
        let result = ext.execute_tool(
            "query_weather",
            &json!({"city": "Beijing"})
        );
        assert!(result.is_ok());
    }
}
```

### Run Tests

```bash
cargo test
```

---

## Best Practices

### 1. Panic Safety

**Never let panics escape from your extension!**

```rust
fn execute_tool(&self, tool: &str, args: &Value) -> Result<Value, ExtensionError> {
    // Bad: unwrap() will panic
    // let city = args.get("city").unwrap().as_str().unwrap();

    // Good: handle errors properly
    let city = args.get("city")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExtensionError::InvalidInput("city required".to_string()))?;
    // ...
}
```

### 2. Thread Safety

Extensions may be called concurrently. Use appropriate synchronization:

```rust
use std::sync::{Arc, RwLock};

pub struct MyExtension {
    state: Arc<RwLock<MyState>>,
}
```

### 3. Resource Cleanup

```rust
impl Drop for MyExtension {
    fn drop(&mut self) {
        // Clean up resources (close connections, etc.)
    }
}
```

### 4. Idempotent Operations

Design commands to be idempotent where possible:

```rust
// Good: calling multiple times has same effect
fn refresh(&self) -> Result<Value, ExtensionError> {
    // Refresh logic
}
```

### 5. Graceful Degradation

Return sensible defaults when external services fail:

```rust
fn fetch_data(&self) -> Result<Data, ExtensionError> {
    match self.api_call() {
        Ok(data) => Ok(data),
        Err(_) => Ok(self.get_cached_data().unwrap_or_default()),
    }
}
```

---

## ABI Version

The extension system uses ABI versioning to ensure compatibility:

```rust
pub const NEO_EXT_ABI_VERSION: u32 = 1;

// Extension must export this
#[no_mangle]
pub extern "C" fn neomind_ext_version() -> u32 {
    NEO_EXT_ABI_VERSION
}
```

---

## Configuration

Extensions can receive configuration via JSON:

```rust
fn new(config: &Value) -> Result<Self, ExtensionError> {
    let api_key = config
        .get("api_key")
        .and_then(|v| v.as_str())
        .or_else(|| std::env::var("MY_API_KEY").ok())
        .unwrap_or_else(|| "default".to_string());
    // ...
}
```

Config is passed when the extension is loaded via the API or stored in the extension registry.

---

## Troubleshooting

| Issue | Solution |
|-------|----------|
| Extension not loading | Check ABI version matches NEO_EXT_ABI_VERSION |
| Panic on load | Ensure no panics in `new()` constructor |
| Commands failing | Check circuit breaker hasn't opened due to failures |
| Metrics not appearing | Verify `produce_metrics()` returns valid data |

---

## Example Extensions

| Extension | Description | Repository |
|-----------|-------------|------------|
| Weather Forecast | Weather data for global cities | This repository |
| Device Discovery | Auto-discover IoT devices | Coming soon |
| LLM Integration | Connect to external LLMs | Coming soon |

---

## Further Reading

- NeoMind Extension SDK: `../NeoTalk/crates/neomind-extension-sdk/`
- Extension Tests: `../NeoTalk/crates/neomind-core/tests/extension_test.rs`
- Example Extensions: `../NeoTalk/examples/extensions/` (historical)
