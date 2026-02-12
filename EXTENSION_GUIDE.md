# NeoMind Extension Development Guide V2

This guide explains how to develop, build, and install extensions for the NeoMind Edge AI Platform using the **V2 Extension API**.

**[中文指南](EXTENSION_GUIDE.zh.md)** | English Documentation

---

## Table of Contents

1. [Overview](#overview)
2. [V2 Extension Architecture](#v2-extension-architecture)
3. [Project Structure](#project-structure)
4. [Core Concepts](#core-concepts)
5. [V2 API Reference](#v2-api-reference)
6. [Building and Installation](#building-and-installation)
7. [Testing](#testing)
8. [Best Practices](#best-practices)
9. [Migration from V1](#migration-from-v1)

---

## Overview

A NeoMind Extension is a dynamic library (`.dylib`, `.so`, `.dll`) that extends the platform's capabilities. Extensions can provide:

| Capability | Description |
|-----------|-------------|
| **Metrics** | Time-series data points (e.g., temperature, humidity) |
| **Commands** | RPC-style commands for AI agents and direct API calls |

### Key V2 Changes

| V1 | V2 |
|----|----|
| `capabilities()` method | Separate `metrics()` and `commands()` methods |
| `execute_tool()` | `execute_command()` (async) |
| ABI Version 1 | ABI Version 2 |
| `neomind-extension-sdk` | `neomind-core::extension::system` |

---

## Extension Types: Native vs WASM

NeoMind supports two types of extensions, each with different trade-offs:

### Native Extensions (.dylib / .so / .dll)

Native extensions are platform-specific dynamic libraries loaded via FFI (Foreign Function Interface).

**Pros:**
- Maximum performance (no runtime overhead)
- Full system access (file system, network, hardware)
- Wide language support via C FFI (Rust, C, C++, Go, etc.)

**Cons:**
- Must compile for each platform separately
- Complex build setup for cross-platform distribution
- Security concerns (full system access)

### WASM Extensions (.wasm)

WASM extensions are WebAssembly modules that run in a sandboxed environment using Wasmtime.

**Pros:**
- **Write once, run anywhere** - single binary for all platforms
- Sandboxed execution (safe, controlled resource access)
- Small file size (typically < 100KB)
- Multi-language support (Rust, AssemblyScript, Go, C/C++)

**Cons:**
- ~10-30% performance overhead
- Limited system access (via host API only)
- Requires WASM target (`wasm32-wasi`)

### Which Type to Choose?

| Use Case | Recommended Type |
|----------|------------------|
| Production distribution | WASM (cross-platform) |
| Performance-critical operations | Native |
| Learning/Development | Native (easier debugging) |
| Untrusted extensions | WASM (sandboxed) |
| Hardware/OS integration | Native |

---

## V2 Extension Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    NeoMind Server                        │
├─────────────────────────────────────────────────────────┤
│  Extension Registry                                      │
│  ├─ Discovery (scan ~/.neomind/extensions)              │
│  ├─ Loading (dlopen + symbol resolution)                │
│  └─ Lifecycle Management                                 │
├─────────────────────────────────────────────────────────┤
│  Extension Safety (V2)                                   │
│  ├─ Circuit Breaker (5 failures → open)                  │
│  ├─ Panic Isolation (catch_unwind)                       │
│  └─ Health Monitoring                                    │
├─────────────────────────────────────────────────────────┤
│  Your Extension (Dynamic Library)                        │
│  └─ Implements Extension Trait (V2)                     │
│      ├─ metadata()                                       │
│      ├─ metrics() → &[MetricDescriptor]                  │
│      ├─ commands() → &[ExtensionCommand]                 │
│      ├─ execute_command() (async)                        │
│      ├─ produce_metrics()                                │
│      └─ health_check() (async)                           │
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
# Use neomind-core from the NeoMind project
neomind-core = { path = "../NeoMind/crates/neomind-core" }
neomind-devices = { path = "../NeoMind/crates/neomind-devices" }

serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
once_cell = "1.19"   # For static metrics/commands
semver = "1.0"       # For version metadata
```

---

## Core Concepts

### 1. Extension Trait (V2)

Every extension must implement the `Extension` trait from `neomind_core::extension::system`:

```rust
use async_trait::async_trait;
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError,
    MetricDescriptor, ExtensionCommand,
    ExtensionMetricValue, ParamMetricValue,
    MetricDataType, ABI_VERSION, Result,
};
use serde_json::Value;
use once_cell::sync::Lazy;
use std::sync::Arc;

pub struct MyExtension {
    metadata: ExtensionMetadata,
    state: Arc<MyState>,
}

// IMPORTANT: Use #[async_trait::async_trait] macro
#[async_trait::async_trait]
impl Extension for MyExtension {
    // Returns reference to metadata (not owned value)
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    // Returns slice of metric descriptors (use static to avoid lifetime issues)
    fn metrics(&self) -> &[MetricDescriptor] {
        &METRICS
    }

    // Returns slice of command descriptors (use static to avoid lifetime issues)
    fn commands(&self) -> &[ExtensionCommand] {
        &COMMANDS
    }

    // Async command execution
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "my_command" => {
                // Handle command
                Ok(json!({ "result": "success" }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    // Synchronous metric production (dylib compatibility)
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            ExtensionMetricValue {
                name: "my_metric".to_string(),
                value: ParamMetricValue::Float(42.0),
                timestamp: current_timestamp(),
            },
        ])
    }

    // Async health check
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}
```

### 2. Static Metrics and Commands

**Critical**: Use `once_cell::sync::Lazy` or `lazy_static` for metrics and commands to avoid lifetime issues:

```rust
use once_cell::sync::Lazy;

/// Static metric descriptors
static METRICS: Lazy<[MetricDescriptor; 1]> = Lazy::new(|| [
    MetricDescriptor {
        name: "temperature_c".to_string(),
        display_name: "Temperature".to_string(),
        data_type: MetricDataType::Float,
        unit: "°C".to_string(),
        min: Some(-50.0),
        max: Some(60.0),
        required: false,
    },
]);

/// Static command descriptors
static COMMANDS: Lazy<[ExtensionCommand; 1]> = Lazy::new(|| [
    ExtensionCommand {
        name: "query_weather".to_string(),
        display_name: "Query Weather".to_string(),
        payload_template: r#"{"city": "{{city}}"}"#.to_string(),
        parameters: vec![],
        fixed_values: Default::default(),
        samples: vec![],
        llm_hints: "Query current weather for any city.".to_string(),
        parameter_groups: vec![],
    },
]);
```

### 3. Error Handling

```rust
pub enum ExtensionError {
    NotFound(String),           // Resource not found
    InvalidInput(String),       // Invalid parameters
    ExecutionFailed(String),    // Runtime error
    CommandNotFound(String),    // Unknown command (V2)
    IoError(String),            // I/O error
    Serialization(String),      // JSON serialization error
}
```

---

## V2 API Reference

### Extension Metadata

```rust
pub struct ExtensionMetadata {
    pub id: String,                  // Unique ID (e.g., "my.company.extension")
    pub name: String,                // Display name
    pub version: semver::Version,    // SemVer version (not String!)
    pub description: Option<String>, // Short description
    pub author: Option<String>,      // Author name
    pub homepage: Option<String>,    // Project URL
    pub license: Option<String>,     // License identifier
    pub file_path: Option<String>,   // Set by loader
}
```

### Metric Descriptor

```rust
pub struct MetricDescriptor {
    pub name: String,           // Metric ID (e.g., "temperature_c")
    pub display_name: String,   // Display name
    pub data_type: MetricDataType,  // Float, Integer, String, Boolean
    pub unit: String,           // Unit (e.g., "°C", "%", "km/h")
    pub min: Option<f64>,       // Minimum value
    pub max: Option<f64>,       // Maximum value
    pub required: bool,         // Whether this metric is required
}

pub enum MetricDataType {
    Float,
    Integer,
    String,
    Boolean,
}
```

### Extension Command

```rust
pub struct ExtensionCommand {
    pub name: String,                  // Command ID
    pub display_name: String,          // Display name
    pub payload_template: String,      // JSON template for parameters
    pub parameters: Vec<Parameter>,    // Parameter definitions
    pub fixed_values: HashMap<String, Value>,  // Fixed parameter values
    pub samples: Vec<Value>,           // Example inputs
    pub llm_hints: String,             // AI agent hints
    pub parameter_groups: Vec<ParameterGroup>,  // Parameter groups
}
```

### Extension Trait Methods (V2)

| Method | Return Type | Async | Description |
|--------|-------------|-------|-------------|
| `metadata()` | `&ExtensionMetadata` | No | Return extension metadata reference |
| `metrics()` | `&[MetricDescriptor]` | No | List all provided metrics |
| `commands()` | `&[ExtensionCommand]` | No | List all supported commands |
| `execute_command()` | `Result<Value>` | Yes | Execute a command |
| `produce_metrics()` | `Result<Vec<ExtensionMetricValue>>` | No | Return current metric values |
| `health_check()` | `Result<bool>` | Yes | Health check |

---

## FFI Exports (Required)

Your extension must export these C-compatible functions:

```rust
use std::ffi::CString;
use std::sync::RwLock;

/// ABI version (must be 2 for V2)
#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    ABI_VERSION  // = 2
}

/// C-compatible metadata
#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> CExtensionMetadata {
    use std::ffi::CStr;

    let id = CString::new("my.extension").unwrap();
    let name = CString::new("My Extension").unwrap();
    let version = CString::new("0.1.0").unwrap();
    let description = CString::new("Does something useful").unwrap();
    let author = CString::new("Your Name").unwrap();

    CExtensionMetadata {
        abi_version: ABI_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        version: version.as_ptr(),
        description: description.as_ptr(),
        author: author.as_ptr(),
        metric_count: 1,   // Number of metrics
        command_count: 1,  // Number of commands
    }
}

/// Create extension instance
#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Extension>> {
    // Parse config
    let config = if config_json.is_null() || config_len == 0 {
        serde_json::json!({})
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(config_json, config_len);
            let s = std::str::from_utf8_unchecked(slice);
            serde_json::from_str(s).unwrap_or(serde_json::json!({}))
        }
    };

    // Create extension
    match MyExtension::new(&config) {
        Ok(ext) => {
            let boxed: Box<dyn Extension> = Box::new(ext);
            let wrapped = Box::new(RwLock::new(boxed));
            Box::into_raw(wrapped)
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy extension instance
#[no_mangle]
pub extern "C" fn neomind_extension_destroy(
    instance: *mut RwLock<Box<dyn Extension>>
) {
    if !instance.is_null() {
        unsafe {
            let _ = Box::from_raw(instance);
        }
    }
}
```

---

## Building and Installation

### 1. Build the Extension

```bash
cd ~/NeoMind-Extension
cargo build --release
```

Output location:
- macOS: `target/release/libneomind_extension_my_extension.dylib`
- Linux: `target/release/libneomind_extension_my_extension.so`
- Windows: `target/release/neomind_extension_my_extension.dll`

### 2. Install the Extension

```bash
# Create extensions directory if it doesn't exist
mkdir -p ~/.neomind/extensions

# Copy the compiled extension
cp target/release/libneomind_extension_my_extension.* ~/.neomind/extensions/
```

### 3. Verify Installation

```bash
# List loaded extensions via API
curl http://localhost:9375/api/extensions

# Check specific extension health
curl http://localhost:9375/api/extensions/my.extension/health
```

---

## Testing

### Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_creation() {
        let ext = MyExtension::new(&json!({})).unwrap();
        assert_eq!(ext.metadata().id, "my.extension");
    }

    #[tokio::test]
    async fn test_command_execution() {
        let ext = MyExtension::new(&json!({})).unwrap();
        let result = ext.execute_command(
            "my_command",
            &json!({"param": "value"})
        ).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_production() {
        let ext = MyExtension::new(&json!({})).unwrap();
        let metrics = ext.produce_metrics().unwrap();
        assert!(!metrics.is_empty());
    }

    #[tokio::test]
    async fn test_health_check() {
        let ext = MyExtension::new(&json!({})).unwrap();
        let healthy = ext.health_check().await.unwrap();
        assert!(healthy);
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
async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
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

### 3. Static Metrics/Commands

Always use `once_cell::sync::Lazy` for metrics and commands:

```rust
// Good: static with Lazy
static METRICS: Lazy<[MetricDescriptor; 1]> = Lazy::new(|| [
    MetricDescriptor { /* ... */ },
]);

fn metrics(&self) -> &[MetricDescriptor] {
    &METRICS
}

// Bad: returning reference to local array
fn metrics(&self) -> &[MetricDescriptor] {
    &[
        MetricDescriptor { /* ... */ },  // ❌ Lifetime error!
    ]
}
```

### 4. Resource Cleanup

```rust
impl Drop for MyExtension {
    fn drop(&mut self) {
        // Clean up resources (close connections, etc.)
    }
}
```

### 5. Idempotent Operations

Design commands to be idempotent where possible:

```rust
// Good: calling multiple times has same effect
async fn refresh(&self) -> Result<Value, ExtensionError> {
    // Refresh logic with cache check
}
```

---

## Migration from V1

| V1 API | V2 API |
|--------|--------|
| `use neomind_extension_sdk::prelude::*;` | `use neomind_core::extension::system::*;` |
| `fn metadata() -> ExtensionMetadata` | `fn metadata() -> &ExtensionMetadata` |
| `fn capabilities()` | `fn metrics() + fn commands()` |
| `fn execute_tool()` | `fn execute_command()` (async) |
| `NEO_EXT_ABI_VERSION = 1` | `ABI_VERSION = 2` |
| `neomind_ext_version()` | `neomind_extension_abi_version()` |
| Returns owned values | Returns references (use static) |

### Migration Steps

1. Update `Cargo.toml`: Change dependency to `neomind-core`
2. Add `async-trait` and `once_cell` dependencies
3. Change `capabilities()` to separate `metrics()` and `commands()`
4. Convert `execute_tool()` to async `execute_command()`
5. Use `once_cell::sync::Lazy` for static descriptors
6. Update FFI export names and ABI version
7. Add `#[async_trait::async_trait]` to impl block

---

## Complete Example

See the `extensions/weather-forecast/` directory for a complete working example:

```bash
cat ~/NeoMind-Extension/extensions/weather-forecast/src/lib.rs
```

This extension demonstrates:
- Static metrics and commands with `once_cell::sync::Lazy`
- Async command execution
- Metric production
- Health checks
- Proper FFI exports

---

## WASM Extension Development

WASM extensions provide cross-platform compatibility with a single build artifact. They run in a sandboxed environment using Wasmtime.

### Project Structure

```
my-wasm-extension/
├── Cargo.toml          # Package configuration
├── my-extension.json   # Metadata sidecar file
├── README.md           # Extension documentation
└── src/
    └── lib.rs          # WASM extension implementation
```

### Minimal Cargo.toml for WASM

```toml
[package]
name = "my-wasm-extension"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### WASM Extension Implementation

WASM extensions use a different approach than native extensions:

```rust
use serde::Serialize;

/// Simple metric value
#[derive(Debug, Clone, Serialize)]
struct MetricValue {
    name: String,
    value: f64,
    unit: String,
}

/// Command response
#[derive(Debug, Clone, Serialize)]
struct CommandResponse {
    success: bool,
    message: String,
    data: Option<MetricValue>,
}

/// Get a metric value
#[no_mangle]
pub extern "C" fn get_my_metric() -> f64 {
    42.0
}

/// Process a command and return JSON response
#[no_mangle]
pub extern "C" fn neomind_execute(
    command_ptr: *const u8,
    _args_ptr: *const u8,
    result_buf_ptr: *mut u8,
    result_buf_len: usize,
) -> usize {
    // Read command string (null-terminated)
    let command = unsafe {
        let mut len = 0;
        while *command_ptr.add(len) != 0 {
            len += 1;
        }
        std::slice::from_raw_parts(command_ptr, len)
    };

    let command_str = std::str::from_utf8(command).unwrap_or("unknown");

    // Match command and generate response
    let response = match command_str {
        "get_metric" => CommandResponse {
            success: true,
            message: "Metric retrieved".to_string(),
            data: Some(MetricValue {
                name: "my_metric".to_string(),
                value: get_my_metric(),
                unit: "count".to_string(),
            }),
        },
        "hello" => CommandResponse {
            success: true,
            message: "Hello from WASM!".to_string(),
            data: None,
        },
        _ => CommandResponse {
            success: false,
            message: format!("Unknown command: {}", command_str),
            data: None,
        },
    };

    // Serialize to JSON and write to buffer
    let json = serde_json::to_string(&response).unwrap_or_default();
    let json_bytes = json.as_bytes();
    let write_len = std::cmp::min(json_bytes.len(), result_buf_len.saturating_sub(1));

    unsafe {
        std::ptr::copy_nonoverlapping(json_bytes.as_ptr(), result_buf_ptr, write_len);
        *result_buf_ptr.add(write_len) = 0; // Null-terminate
    }

    write_len
}

/// Health check
#[no_mangle]
pub extern "C" fn health() -> i32 {
    1 // 1 = healthy
}
```

### Metadata Sidecar File

WASM extensions use a JSON sidecar file for metadata:

```json
{
    "id": "my-wasm-extension",
    "name": "My WASM Extension",
    "version": "0.1.0",
    "description": "A cross-platform WASM extension",
    "author": "Your Name",
    "homepage": "https://github.com/your/repo",
    "license": "MIT",
    "metrics": [
        {
            "name": "my_metric",
            "display_name": "My Metric",
            "data_type": "float",
            "unit": "count",
            "min": null,
            "max": null,
            "required": false
        }
    ],
    "commands": [
        {
            "name": "get_metric",
            "display_name": "Get Metric",
            "description": "Get the current metric value",
            "payload_template": "{}",
            "parameters": [],
            "fixed_values": {},
            "samples": [],
            "llm_hints": "Returns the current metric value",
            "parameter_groups": []
        },
        {
            "name": "hello",
            "display_name": "Hello",
            "description": "Say hello from WASM",
            "payload_template": "{}",
            "parameters": [],
            "fixed_values": {},
            "samples": [],
            "llm_hints": "Returns a greeting message",
            "parameter_groups": []
        }
    ]
}
```

### Building WASM Extensions

For AssemblyScript WASM (recommended):
```bash
# Install dependencies
cd ~/NeoMind-Extension/extensions/as-hello
npm install

# Build extension
npm run build

# Output: build/as-hello.wasm
```

### Installing WASM Extensions

```bash
# Copy both files to extensions directory
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json

# Restart NeoMind or use extension discovery
```

### WASM vs Native: Key Differences

| Aspect | Native | WASM |
|--------|--------|------|
| **Metadata** | Embedded in code via FFI | Separate JSON file |
| **Exports** | `neomind_extension_*` functions | `neomind_execute`, `health`, custom functions |
| **State** | Can use `Arc<RwLock<T>>` | Limited to WASM memory |
| **System Access** | Full access | Via host API only |
| **Distribution** | Platform-specific binaries | Single `.wasm` file for all platforms |

### WASM Best Practices

1. **Keep it simple**: WASM extensions work best for simple operations
2. **Avoid external dependencies**: Many crates don't work with `wasm32-wasi`
3. **Use JSON for responses**: The sandbox handles JSON serialization
4. **Test on target platform**: WASM behavior can differ from native
5. **Provide good metadata**: The JSON file is your primary documentation

### Example: as-hello Extension (AssemblyScript)

See the `extensions/as-hello/` directory for a complete working example:

```bash
cat ~/NeoMind-Extension/extensions/as-hello/assembly/extension.ts
cat ~/NeoMind-Extension/extensions/as-hello/metadata.json
```

This extension demonstrates:
- TypeScript-like syntax for WASM
- Fast compile times (~1s)
- Small binary size (~15 KB)
- Metric exports and command execution
- Health check implementation
- Metadata sidecar file

---

### AssemblyScript WASM Extensions

AssemblyScript is a TypeScript-like language that compiles to WebAssembly. It's ideal for JavaScript/TypeScript developers who want to create WASM extensions with fast compile times.

#### Why AssemblyScript?

| Feature | AssemblyScript | Rust WASM |
|---------|----------------|-----------|
| **Syntax** | TypeScript-like | Rust |
| **Learning Curve** | Low (for JS/TS devs) | High |
| **Compile Time** | ~1s | ~5s |
| **Binary Size** | ~15 KB | ~50 KB |
| **Performance** | 90-95% native | 85-90% native |

#### Project Structure

```
my-as-extension/
├── package.json          # npm dependencies
├── asconfig.json         # AssemblyScript compiler config
├── metadata.json         # Extension metadata sidecar
├── README.md             # Extension documentation
└── assembly/
    └── extension.ts      # AssemblyScript source
```

#### Minimal package.json

```json
{
  "name": "my-as-extension",
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "build": "asc assembly/extension.ts --target release --outFile build/my-extension.wasm -O3z"
  },
  "devDependencies": {
    "assemblyscript": "^0.27.29"
  }
}
```

#### Minimal asconfig.json

```json
{
  "extends": "assemblyscript/std/assembly.json",
  "entry": "assembly/extension.ts",
  "targets": {
    "release": {
      "binaryFile": "build/my-extension.wasm",
      "textFile": "build/my-extension.wat",
      "optimizeLevel": 3,
      "shrinkLevel": 0,
      "runtime": "minimal"
    }
  }
}
```

#### AssemblyScript Implementation

```typescript
// ABI Version - must be 2 for NeoMind V2 API
const ABI_VERSION: u32 = 2;

// Extension state
let counter: i32 = 0;

// Export ABI version
export function neomind_extension_abi_version(): u32 {
  return ABI_VERSION;
}

// Get metric value
export function get_counter(): i32 {
  return counter;
}

// Increment counter
export function increment_counter(): i32 {
  counter = counter + 1;
  return counter;
}

// Execute command
export function neomind_execute(
  command_ptr: usize,
  args_ptr: usize,
  result_buf_ptr: usize,
  result_buf_len: usize
): usize {
  // Read command string
  const command = getString(command_ptr);

  // Generate response based on command
  let response: string;

  switch (command) {
    case "get_counter":
      response = JSON.stringify({
        success: true,
        message: "Counter retrieved",
        data: {
          name: "counter",
          value: counter,
          unit: "count"
        }
      });
      break;

    case "increment_counter":
      counter = counter + 1;
      response = JSON.stringify({
        success: true,
        message: "Counter incremented",
        data: {
          name: "counter",
          value: counter,
          unit: "count"
        }
      });
      break;

    default:
      response = JSON.stringify({
        success: false,
        message: "Unknown command",
        error: `Command '${command}' not found`
      });
      break;
  }

  // Write response to buffer
  return writeString(result_buf_ptr, result_buf_len, response);
}

// Health check
export function health(): i32 {
  return 1; // 1 = healthy
}

// Helper: Read null-terminated string from memory
@inline
function getString(ptr: usize): string {
  if (ptr === 0) return "";

  let len = 0;
  while (load<u8>(ptr + len) !== 0) {
    len++;
  }

  return String.UTF8.decode(ptr, len);
}

// Helper: Write string to memory buffer
@inline
function writeString(ptr: usize, maxLen: usize, str: string): usize {
  const encoded = String.UTF8.encode(str);
  const len = encoded.length;
  const writeLen = min(len, maxLen - 1);

  memory.copy(ptr, changetype<usize>(encoded), writeLen);
  store<u8>(ptr + writeLen, 0);

  return writeLen;
}
```

#### Building AssemblyScript Extensions

```bash
# Install dependencies
cd ~/NeoMind-Extension/extensions/as-hello
npm install

# Build the extension
npm run build

# Output: build/as-hello.wasm
```

#### Installing AssemblyScript Extensions

```bash
# Copy both files to extensions directory
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json

# Restart NeoMind or use extension discovery
```

---

## Troubleshooting

| Issue | Solution |
|-------|----------|
| Extension not loading | Check ABI version returns 2, not 1 |
| Lifetime errors in metrics()/commands() | Use `once_cell::sync::Lazy` for static data |
| Panic on load | Ensure no panics in `new()` constructor |
| Commands failing | Check circuit breaker hasn't opened due to failures |
| Metrics not appearing | Verify `produce_metrics()` returns valid data |
| Wrong SDK reference | Use `neomind-core`, not `neomind-extension-sdk` |

### WASM-Specific Issues

| Issue | Solution |
|-------|----------|
| WASM extension not loading | Ensure both `.wasm` and `.json` files are present |
| `wasm32-wasi` target not found | Run `rustup target add wasm32-wasi` |
| Dependencies not compiling for WASM | Some crates don't support `wasm32-wasi`; check compatibility |
| Extension loads but commands fail | Check that function names match exactly (case-sensitive) |
| JSON metadata not recognized | Verify JSON is valid and matches the schema |
| Metrics not showing | Ensure `data_type` in JSON is valid (`integer`, `float`, `string`, `boolean`) |

---

## Further Reading

- NeoMind Core: `~/NeoMind/crates/neomind-core/src/extension/system.rs`
- Extension Tests: `~/NeoMind/crates/neomind-core/tests/extension_test.rs`
- Example Extensions: `~/NeoMind-Extension/extensions/`
