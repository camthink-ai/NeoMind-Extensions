# NeoMind Extensions

Official extension repository for the NeoMind Edge AI Platform.

[中文文档](README.zh.md)

## Project Description

This repository contains officially maintained extensions for the NeoMind Edge AI Platform.

### Extension Types

NeoMind supports **two types of extensions**:

| Type | File Format | Description | Best For |
|------|-------------|-------------|----------|
| **Native** | `.dylib` / `.so` / `.dll` | Platform-specific dynamic libraries loaded via FFI | Maximum performance, full system access |
| **WASM** | `.wasm` + `.json` | WebAssembly modules with sandboxed execution | Cross-platform distribution, safe execution |

### Why WASM Extensions?

**Native Extensions** (`.dylib`/`.so`/`.dll`):
- **Pros**: Maximum performance, full system access, wide language support via C FFI
- **Cons**: Must compile for each platform (macOS ARM64, macOS x64, Linux, Windows)

**WASM Extensions** (`.wasm`):
- **Pros**: Write once, run anywhere; sandboxed execution; small file size (<100KB); multi-language support (Rust, AssemblyScript/TypeScript, Go, etc.)
- **Cons**: ~10-30% performance overhead; limited system access (via host API)

> **Tip**: Choose WASM for ease of distribution and cross-platform compatibility. Choose Native for performance-critical extensions that need direct system access.

---

## For Users: Installing Extensions

### Via NeoMind Extension Marketplace (Recommended)

The easiest way to install extensions is through the built-in marketplace in NeoMind:

1. Open NeoMind Web UI
2. Navigate to **Extensions** → **Marketplace**
3. Browse available extensions
4. Click **Install** on any extension
5. The extension will be automatically downloaded and installed

Extensions are fetched from:
- **Index**: https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions/main/extensions/index.json
- **Metadata**: https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions/main/extensions/{id}/metadata.json
- **Binaries**: https://github.com/camthink-ai/NeoMind-Extensions/releases

### Manual Installation

#### Pre-built Binaries

Download pre-built binaries from [Releases](https://github.com/camthink-ai/NeoMind-Extensions/releases):

**Native Extensions (.dylib / .so / .dll)**:
```bash
# After downloading, copy to extensions directory
mkdir -p ~/.neomind/extensions
cp ~/Downloads/libneomind_extension_weather_forecast.dylib ~/.neomind/extensions/

# Restart NeoMind
```

**WASM Extensions (.wasm)**:
```bash
# Download both files:
# - my-extension.wasm (the WebAssembly module)
# - my-extension.json (metadata file)

mkdir -p ~/.neomind/extensions
cp ~/Downloads/my-extension.wasm ~/.neomind/extensions/
cp ~/Downloads/my-extension.json ~/.neomind/extensions/

# Restart NeoMind
```

#### Build from Source

**Native Extensions**:
```bash
# Clone the repository
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions

# Build all extensions
cargo build --release

# Copy to extensions directory
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_*.dylib ~/.neomind/extensions/
```

**WASM Extensions**:
```bash
# Clone the repository
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions/extensions/as-hello

# Install dependencies and build
npm install
npm run build

# Copy both files to extensions directory
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json
```

---

## Available Extensions

### [image-analyzer](extensions/image-analyzer/) - Stateless Streaming

Demonstrates **Stateless** streaming mode for single-chunk image processing.

| Capability | Type | Description |
|-----------|------|-------------|
| Image Analysis | Stream | Analyze JPEG/PNG/WebP images |
| Object Detection | Metric | Detected objects with bounding boxes |
| `reset_stats` | Command | Reset processing statistics |

**Streaming Mode**: Stateless (independent processing of each chunk)
**Direction**: Upload (client → extension)
**Max Chunk Size**: 10MB

**Installation**:
```bash
cargo build --release -p neomind-image-analyzer
cp target/release/libneomind_extension_image_analyzer.dylib ~/.neomind/extensions/
```

### [yolo-video](extensions/yolo-video/) - Stateful Streaming

Demonstrates **Stateful** streaming mode for session-based video processing.

| Capability | Type | Description |
|-----------|------|-------------|
| Video Processing | Stream | Process H264/H265 video frames |
| Object Detection | Stream | YOLO-based real-time detection |
| `get_session_info` | Command | Get active session statistics |

**Streaming Mode**: Stateful (maintains session context)
**Direction**: Upload (client → extension)
**Max Chunk Size**: 5MB per frame
**Max Concurrent Sessions**: 5

**Installation**:
```bash
cargo build --release -p neomind-yolo-video
cp target/release/libneomind_extension_yolo_video.dylib ~/.neomind/extensions/
```

### [as-hello](extensions/as-hello/) - WASM Example (AssemblyScript/TypeScript)

A WASM extension written in AssemblyScript (TypeScript-like language) with fast compile times and small binary size.

| Capability | Type | Description |
|-----------|------|-------------|
| `get_counter` | Command | Get the current counter value |
| `increment_counter` | Command | Increment the counter |
| `reset_counter` | Command | Reset counter to default value |
| `get_temperature` | Command | Get temperature reading (simulated) |
| `set_temperature` | Command | Set temperature value (for testing) |
| `get_humidity` | Command | Get humidity reading (simulated) |
| `hello` | Command | Say hello from AssemblyScript |
| `get_all_metrics` | Command | Get all metrics with variation |

**Metrics**: counter, temperature, humidity

**Installation**:
```bash
# Build AssemblyScript WASM extension
cd extensions/as-hello
npm install
npm run build

# Install (both files required)
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json
```

**Why AssemblyScript?**
- TypeScript-like syntax (easy for JS/TS developers)
- Very fast compile time (~1s vs ~5s for Rust WASM)
- Small binary size (~15 KB vs ~50 KB for Rust WASM)
- Single `.wasm` file for all platforms

### [template](extensions/template/) - Native Template
Template for creating native extensions.

### [weather-forecast](extensions/weather-forecast/)
Weather data and forecasts for global cities.

| Capability | Type | Description |
|-----------|------|-------------|
| `query_weather` | Command | Get current weather for any city |
| `refresh` | Command | Force refresh cached data |

**Metrics**: temperature_c, humidity_percent, wind_speed_kmph, cloud_cover_percent

**Installation**:
```bash
# Via marketplace (in NeoMind UI)
# Or manual:
cp target/release/libneomind_extension_weather_forecast.dylib ~/.neomind/extensions/
```

---

## Repository Structure

```
NeoMind-Extensions/
├── extensions/
│   ├── index.json              # Main marketplace index
│   │   # Lists all available extensions with metadata URLs
│   ├── image-analyzer/         # Stateless streaming extension
│   │   ├── Cargo.toml          # Package configuration
│   │   └── src/lib.rs          # Source code
│   ├── yolo-video/             # Stateful streaming extension
│   │   ├── Cargo.toml          # Package configuration
│   │   └── src/lib.rs          # Source code
│   ├── as-hello/               # WASM extension example (AssemblyScript) ⭐ Recommended
│   │   ├── package.json        # npm dependencies
│   │   ├── asconfig.json       # AssemblyScript compiler config
│   │   ├── metadata.json       # Extension metadata (for marketplace)
│   │   ├── README.md           # Extension documentation
│   │   └── assembly/extension.ts  # Source code
│   ├── weather-forecast/       # Native extension
│   │   ├── metadata.json       # Extension metadata (for marketplace)
│   │   ├── Cargo.toml          # Package configuration
│   │   ├── README.md           # Extension documentation
│   │   └── src/lib.rs          # Source code
│   └── template/               # Template for native extensions
│       ├── Cargo.toml
│       ├── README.md
│       └── src/lib.rs
├── EXTENSION_GUIDE.md          # Developer guide
├── USER_GUIDE.md               # User guide
├── Cargo.toml                  # Workspace configuration
├── build.sh                    # Build script
└── README.md                   # This file
```

---

## Marketplace Data Format

### extensions/index.json

Main index that lists all available extensions:

```json
{
  "version": "1.0",
  "last_updated": "2025-02-10T12:00:00Z",
  "extensions": [
    {
      "id": "weather-forecast",
      "name": "Weather Forecast",
      "description": "Global weather data and forecasts",
      "version": "0.1.0",
      "author": "CamThink",
      "license": "MIT",
      "categories": ["weather", "data"],
      "metadata_url": "https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions/main/extensions/weather-forecast/metadata.json"
    }
  ]
}
```

### extensions/{id}/metadata.json

Detailed metadata for each extension:

```json
{
  "id": "weather-forecast",
  "name": "Weather Forecast",
  "description": "...",
  "version": "0.1.0",
  "capabilities": {
    "tools": [...],
    "metrics": [...],
    "commands": [...]
  },
  "builds": {
    "darwin-aarch64": {
      "url": "https://github.com/.../download/v0.1.0/...",
      "sha256": "...",
      "size": 123456
    }
  },
  "requirements": {
    "min_neomind_version": "0.5.8",
    "network": true
  },
  "safety": {
    "timeout_seconds": 30,
    "max_memory_mb": 100
  }
}
```

---

## For Developers: Creating Extensions

See [EXTENSION_GUIDE.md](EXTENSION_GUIDE.md) for complete documentation.

### Quick Start

**Choose Your Extension Type:**

| Goal | Recommended Type |
|------|------------------|
| Cross-platform without rebuilding | WASM |
| Maximum performance | Native |
| Learning/Development | Native (template) or WASM (as-hello for JS/TS devs) |
| Production distribution | WASM |
| Fast iteration/prototyping | WASM (AssemblyScript - ~1s compile) |

**Native Extension (from template):**
```bash
cd extensions
cp -r template my-extension
cd my-extension

# Update Cargo.toml with your extension name
# Update src/lib.rs with your implementation
# Create metadata.json for marketplace listing

# Build
cargo build --release
```

**WASM Extension (from as-hello - AssemblyScript/TypeScript):**
```bash
cd extensions
cp -r as-hello my-as-extension
cd my-as-extension

# Install dependencies
npm install

# Update package.json with your extension name
# Update assembly/extension.ts with your implementation
# Update my-extension.json metadata
# Update asconfig.json if changing output file names

# Build (very fast ~1s, single binary for all platforms!)
npm run build
```

### Submitting to Marketplace

1. Fork this repository
2. Create your extension in `extensions/your-extension/`
3. Add metadata.json following the format above
4. Add your extension to `extensions/index.json`
5. Submit a pull request

After your PR is merged:
1. Build multi-platform binaries
2. Create a GitHub Release
3. Upload binaries to the Release
4. Update metadata.json with SHA256 checksums

---

## Release Process

When preparing a release:

```bash
# 1. Update version numbers
# - In each extension's Cargo.toml
# - In each extension's metadata.json
# - In extensions/index.json

# 2. Build for all platforms
./build.sh --all-platforms

# 3. Calculate SHA256
shasum -a 256 target/release/libneomind_extension_*

# 4. Create GitHub Release
gh release create v0.1.0 \
  target/release/*.dylib \
  target/release/*.so \
  target/release/*.dll

# 5. Update metadata.json with checksums
# 6. Commit and push
git add .
git commit -m "Release v0.1.0"
git push origin main
```

---

## Platform Support

| Platform | Architecture | Native Binary | WASM Binary |
|----------|--------------|---------------|-------------|
| macOS | ARM64 (Apple Silicon) | `libneomind_extension_*.dylib` | `*.wasm` (universal) |
| macOS | x86_64 (Intel) | `libneomind_extension_*.dylib` | `*.wasm` (universal) |
| Linux | x86_64 | `libneomind_extension_*.so` | `*.wasm` (universal) |
| Linux | ARM64 | `libneomind_extension_*.so` | `*.wasm` (universal) |
| Windows | x86_64 | `neomind_extension_*.dll` | `*.wasm` (universal) |

> **Note**: WASM extensions work on all platforms without recompilation - the same `.wasm` file runs everywhere!

---

## License

MIT License
---
