# AssemblyScript Hello Extension

A NeoMind WASM extension written in [AssemblyScript](https://www.assemblyscript.org/) - a TypeScript-like language that compiles to WebAssembly.

[中文文档](README.zh.md)

## Why AssemblyScript?

| Feature | AssemblyScript | Rust WASM | Native Rust |
|----------|-----------------|-----------|-------------|
| **Syntax** | TypeScript-like | Rust | Rust |
| **Learning Curve** | Low (for JS/TS devs) | High | High |
| **Compile Time** | Very Fast (~1s) | Fast (~5s) | Fast (~10s) |
| **Binary Size** | ~15 KB | ~50 KB | ~100 KB |
| **Performance** | High (90-95% native) | High (85-90% native) | 100% |
| **Cross-Platform** | ✅ Single .wasm | ✅ Single .wasm | ❌ Platform-specific |

## Features

- **Metrics**: counter, temperature, humidity
- **Commands**: get_counter, increment_counter, reset_counter, get_temperature, set_temperature, get_humidity, hello, get_all_metrics
- **Cross-Platform**: Single `.wasm` file runs everywhere
- **Small Size**: ~15 KB compiled binary
- **Type-Safe**: AssemblyScript's strict TypeScript subset

## Prerequisites

```bash
# Install Node.js (if not already installed)
# On macOS:
brew install node

# Install npm dependencies
cd ~/NeoMind-Extension/extensions/as-hello
npm install
```

## Building

```bash
cd ~/NeoMind-Extension/extensions/as-hello

# Install dependencies (first time only)
npm install

# Build the extension
npm run build

# Output: build/as-hello.wasm
```

## Installation

```bash
# Copy both files to NeoMind extensions directory
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json

# Restart NeoMind or trigger extension discovery
```

## Usage

### Via NeoMind API

```bash
# List extensions
curl http://localhost:9375/api/extensions

# Execute a command
curl -X POST http://localhost:9375/api/extensions/as-hello/command/hello \
  -H "Content-Type: application/json" \
  -d '{}'

# Get counter
curl -X POST http://localhost:9375/api/extensions/as-hello/command/get_counter \
  -H "Content-Type: application/json" \
  -d '{}'

# Increment counter
curl -X POST http://localhost:9375/api/extensions/as-hello/command/increment_counter \
  -H "Content-Type: application/json" \
  -d '{}'
```

### Available Commands

| Command | Description | Example |
|---------|-------------|---------|
| `get_counter` | Get current counter value | `{}` |
| `increment_counter` | Increment counter by 1 | `{}` |
| `reset_counter` | Reset counter to 42 | `{}` |
| `get_temperature` | Get temperature reading | `{}` |
| `set_temperature` | Set new temperature | `{"temperature": 25.5}` |
| `get_humidity` | Get humidity reading | `{}` |
| `hello` | Say hello from AS extension | `{}` |
| `get_all_metrics` | Get all metrics with variation | `{}` |

## Metrics

| Metric | Type | Unit | Range | Description |
|--------|------|------|-------|-------------|
| `counter` | Integer | count | 0-1000+ | Simple counter |
| `temperature` | Float | °C | -20 to 50 | Simulated temperature |
| `humidity` | Float | % | 0-100 | Simulated humidity |

## Development

### Project Structure

```
as-hello/
├── package.json          # npm dependencies
├── asconfig.json          # AssemblyScript compiler config
├── as-hello.json          # Extension metadata (for NeoMind)
├── README.md              # This file
├── assembly/
│   └── extension.ts      # Main extension implementation
├── build/                 # Compiled output (generated)
│   └── as-hello.wasm     # Compiled WASM module
└── tests/
    └── test.js           # Node.js tests
```

### Key Implementation Details

**Memory Layout**: AssemblyScript uses linear memory for WASM. Strings are passed as pointers with null termination.

**Command Pattern**: The main entry point is `neomind_execute()` which receives:
- `command_ptr`: Pointer to command string
- `args_ptr`: Pointer to arguments JSON string
- `result_buf_ptr`: Pointer to result buffer
- `result_buf_len`: Maximum buffer length

**Type Safety**: AssemblyScript provides compile-time type checking while compiling to efficient WASM code.

## AssemblyScript Basics

### Type Definitions

```typescript
// Basic types
let counter: i32 = 42;
let temperature: f64 = 23.5;
let name: string = "as-hello";

// Arrays
let values: f64[] = [1.0, 2.0, 3.0];

// Constants
const ABI_VERSION: u32 = 2;
```

### Export Functions (WASM)

```typescript
// Export a function to be callable from WASM host
export function get_counter(): i32 {
  return counter;
}
```

### Memory Operations

```typescript
// Read from memory
let byte = load<u8>(ptr);

// Write to memory
store<u8>(ptr, 42);

// Copy memory
memory.copy(dest, source, length);
```

## Troubleshooting

### Build Errors

**"asc: command not found"**
```bash
npm install
```

**"Module not found"**
```bash
# Ensure you're in the as-hello directory
cd ~/NeoMind-Extension/extensions/as-hello
```

### Runtime Errors

**"Extension not loading"**
- Ensure both `.wasm` and `.json` files are in the extensions directory
- Check that the JSON file name matches the WASM file name

**"Command not found"**
- Check the command name is spelled correctly
- Verify the command is listed in the metadata

## Comparison with Other Extensions

| Aspect | as-hello (AS) | wasm-hello (Rust) | template (Native) |
|--------|----------------|-------------------|------------------|
| **Language** | TypeScript-like | Rust | Rust |
| **Build Time** | ~1s | ~5s | ~10s |
| **Binary Size** | ~15 KB | ~50 KB | ~100 KB |
| **Type Safety** | Compile-time | Compile-time | Compile-time |
| **Debugging** | Medium | Hard | Easy |
| **JS Developers** | Easy | Hard | Hard |

## Resources

- [AssemblyScript Website](https://www.assemblyscript.org/)
- [AssemblyScript GitHub](https://github.com/AssemblyScript/assemblyscript)
- [AssemblyScript Docs](https://www.assemblyscript.org/introduction.html)
- [WebAssembly Specification](https://webassembly.github.io/spec/)

## License

MIT License - see [NeoMind-Extensions LICENSE](../../../LICENSE)
