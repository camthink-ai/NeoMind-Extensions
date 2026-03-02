# WASM Demo Extension

A simple counter extension demonstrating complete WASM support for NeoMind.

## Features

- **Counter metric**: A simple integer counter
- **Increment command**: Add a value to the counter
- **Decrement command**: Subtract a value from the counter  
- **Reset command**: Reset the counter to zero
- **Get command**: Get the current counter value

## Building

### Prerequisites

```bash
# Add the WASM target
rustup target add wasm32-unknown-unknown
```

### Build Commands

```bash
# Build release version (recommended)
./build.sh

# Or manually:
cargo build --target wasm32-unknown-unknown --release

# Build debug version
cargo build --target wasm32-unknown-unknown
```

### Output

The compiled WASM file will be at:
```
target/wasm32-unknown-unknown/release/neomind_extension_wasm_demo.wasm
```

## Installation

1. Create the extension directory:
```bash
mkdir -p /path/to/neomind/extensions/wasm-demo
```

2. Copy the WASM file and metadata:
```bash
cp target/wasm32-unknown-unknown/release/neomind_extension_wasm_demo.wasm /path/to/neomind/extensions/wasm-demo/extension.wasm
cp metadata.json /path/to/neomind/extensions/wasm-demo/
```

## Usage

### Via API

```bash
# Get current counter value
curl -X POST http://localhost:9375/api/extensions/wasm-demo/execute \
  -H "Content-Type: application/json" \
  -d '{"command": "get", "args": {}}'

# Increment counter by 5
curl -X POST http://localhost:9375/api/extensions/wasm-demo/execute \
  -H "Content-Type: application/json" \
  -d '{"command": "increment", "args": {"amount": 5}}'

# Reset counter
curl -X POST http://localhost:9375/api/extensions/wasm-demo/execute \
  -H "Content-Type: application/json" \
  -d '{"command": "reset", "args": {}}'
```

### Via Chat

Simply ask NeoMind to interact with the counter:
- "What's the current counter value?"
- "Increment the counter by 10"
- "Reset the demo counter"

## API Reference

### Metrics

| Name | Type | Description |
|------|------|-------------|
| `counter` | Integer | Current counter value |
| `request_count` | Integer | Total requests processed |

### Commands

| Command | Parameters | Description |
|---------|------------|-------------|
| `increment` | `amount` (int, optional, default: 1) | Add to counter |
| `decrement` | `amount` (int, optional, default: 1) | Subtract from counter |
| `reset` | none | Reset counter to zero |
| `get` | none | Get current value |

## Code Structure

```
wasm-demo/
├── Cargo.toml           # Package configuration
├── metadata.json        # Extension metadata
├── build.sh             # Build script
├── README.md            # This file
└── src/
    └── lib.rs           # Extension implementation
```

## Key Implementation Details

This extension demonstrates:

1. **WASM-compatible imports**: Uses `#![cfg_attr(target_arch = "wasm32", no_std)]` for WASM compatibility

2. **Conditional compilation**: Uses `#[cfg(target_arch = "wasm32")]` for WASM-specific code

3. **neomind_export! macro**: Single macro exports all necessary FFI functions

4. **Descriptor export**: The `get_descriptor_json()` function is automatically generated

5. **Command execution**: The `execute_command_json()` function handles commands via JSON

## Size Optimization

The release build is optimized for size:
- `opt-level = "s"` - Optimize for size
- `lto = true` - Link-time optimization
- `panic = "abort"` - Required for WASM
- `strip = true` - Strip symbols

Expected size: ~100-200KB for a simple extension.