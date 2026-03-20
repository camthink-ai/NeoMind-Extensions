# Architecture Reference

## Process Isolation Architecture

### Overview

NeoMind V2 uses a multi-process architecture where all extensions run in isolated processes:

```
┌─────────────────────────────────────────────────────────────┐
│                   NeoMind Main Process                       │
│  ┌─────────────────────────────────────────────────────────┐│
│  │             UnifiedExtensionService                      ││
│  │  - IPC communication via stdin/stdout                    ││
│  │  - Manages lifecycle of all extensions                   ││
│  │  - Handles command routing and metrics collection        ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
                               │
                    ┌──────────┴───────────┐
                    ▼                      ▼
┌─────────────────────────────┐  ┌─────────────────────────────┐
│  Extension Runner Process 1 │  │  Extension Runner Process 2 │
│  - Native: loaded via FFI   │  │  - WASM: via wasmtime       │
│  - Crashes isolated         │  │  - Independent lifecycle    │
└─────────────────────────────┘  └─────────────────────────────┘
```

### Benefits

1. **Crash Isolation**: Extension panics don't affect the main process
2. **Memory Isolation**: Each extension has its own memory space
3. **Resource Management**: CPU and memory limits per extension
4. **Independent Lifecycle**: Extensions can restart independently

### IPC Protocol

Communication between main process and extension processes uses JSON messages over stdin/stdout:

**Execute Command:**
```json
{
  "ExecuteCommand": {
    "command": "analyze_image",
    "args": { "path": "/path/to/image.jpg" },
    "request_id": 1
  }
}
```

**Response:**
```json
{
  "success": true,
  "data": { "detections": [...] },
  "request_id": 1
}
```

**Get Metrics:**
```json
{
  "ProduceMetrics": {
    "request_id": 2
  }
}
```

**Metrics Response:**
```json
{
  "metrics": [
    {
      "name": "images_processed",
      "value": 42,
      "timestamp": 1709481600000
    }
  ],
  "request_id": 2
}
```

### Resource Configuration

Configure resource limits in `metadata.json`:

```json
{
  "id": "your-extension-v2",
  "version": "2.0.0",
  "process_config": {
    "timeout_seconds": 60,
    "max_memory_mb": 512,
    "restart_on_crash": true,
    "restart_delay_ms": 1000
  }
}
```

## Runtime Protocol v3

### Required FFI Exports

Every extension must export these C-compatible functions:

```rust
#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    3  // Must return 3 for runtime protocol v3
}

#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> CExtensionMetadata {
    // Return extension metadata
}

#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Any>> {
    // Create and return extension instance
}

#[no_mangle]
pub extern "C" fn neomind_extension_destroy(ptr: *mut RwLock<Box<dyn Any>>) {
    // Cleanup extension resources
}
```

**Note:** When using the SDK macro `neomind_export!(YourExtension)`, these are automatically generated.

### CExtensionMetadata Structure

```rust
#[repr(C)]
pub struct CExtensionMetadata {
    pub abi_version: u32,
    pub id: *const c_char,
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub author: *const c_char,
    pub metric_count: usize,
    pub command_count: usize,
}
```

## Extension Lifecycle

```
1. Extension Loaded
   ↓
2. neomind_extension_create() called
   ↓
3. Ready to receive commands
   ↓
4. Commands executed via IPC
   ↓
5. Metrics collected periodically
   ↓
6. Extension shutdown
   ↓
7. neomind_extension_destroy() called
```

## Native vs WASM

| Feature | Native | WASM |
|---------|--------|------|
| Performance | Maximum | Good |
| Platform | Platform-specific binary | Universal |
| File Extension | .dylib/.so/.dll | .wasm |
| System Access | Full | Sandboxed |
| Dependencies | Any Rust crate | Limited to WASM-compatible |
| Use Case | AI inference, Heavy I/O | Cross-platform, Lightweight |
