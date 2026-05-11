# NeoMind Extensions

Official extension marketplace for the [NeoMind Edge AI Platform](https://github.com/camthink-ai/NeoMind).

[中文文档](README.zh.md)

---

## Overview

This repository contains officially maintained extensions for the **NeoMind extension runtime**. Each extension runs in an isolated process, includes optional React dashboard components, and can be packaged as `.nep` files for cross-platform distribution.

- **Runtime Protocol**: v3 (process-isolated architecture)
- **SDK Version**: 0.6+ (builder patterns, helper macros, streaming API)
- **ABI Version**: 3
- **Platforms**: macOS (ARM64/x86_64), Linux (x86_64/ARM64), Windows (x86_64/x86), WASM

---

## Available Extensions

| Extension | ID | Category | Frontend | Description |
|-----------|----|----------|----------|-------------|
| Weather Forecast V2 | `weather-forecast-v2` | Data | WeatherCard | Real-time weather via Open-Meteo API |
| Image Analyzer V2 | `image-analyzer-v2` | AI/ML | ImageAnalyzer | YOLOv11 object detection on images |
| YOLO Video V2 | `yolo-video-v2` | AI/ML | YoloVideoDisplay | Real-time video stream detection with ROI/line crossing |
| YOLO Device Inference | `yolo-device-inference` | AI/ML | DeviceBindingCard | Auto YOLO detection on device camera feeds |
| Face Recognition | `face-recognition` | AI/ML | FaceRecognitionCard | ArcFace face recognition with gallery management |
| OCR Device Inference | `ocr-device-inference` | AI/ML | OcrDeviceCard | PP-OCRv4 text recognition on device images |
| Stream Player | `stream-player` | Media | StreamPlayerCard | RTSP/RTMP/HLS video player via FFmpeg |
| Uink-RMS Bridge | `uink-rms-bridge` | Device | DisplayEditorCard | E-paper display content push & management |
| WASM Demo | `wasm-demo` | Demo | — | Counter demo for WASM target |

> **Latest Release**: See [GitHub Releases](https://github.com/camthink-ai/NeoMind-Extensions/releases)

---

## Quick Start

### Prerequisites

- Rust 1.75+
- NeoMind Extension SDK (`neomind-extension-sdk` v0.6+)

### Build & Install

```bash
# Build all extensions
./build.sh

# Build single extension (dev mode + auto-install)
./build.sh --dev --single weather-forecast-v2

# Build release packages
./build.sh --release 2.6.0

# Or manual build
cargo build --release -p weather-forecast-v2
cp target/release/libneomind_extension_weather_forecast_v2.dylib ~/.neomind/extensions/
```

### Install from Marketplace

```bash
# Via NeoMind CLI
neomind extension install weather-forecast-v2-2.0.0-darwin_aarch64.nep

# Via Web UI
# Navigate to Extensions → Marketplace → Install
```

---

## Extension Development

Full development guide: **[EXTENSION_GUIDE.md](EXTENSION_GUIDE.md)**

### Minimal Extension

```rust
use neomind_extension_sdk::prelude::*;
use neomind_extension_sdk::{MetricBuilder, CommandBuilder, ParamBuilder, metric_int};
use serde_json::json;

pub struct MyExtension { counter: AtomicI64 }

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| ExtensionMetadata::new("my-ext", "My Ext", "1.0.0")
            .with_description("..."))
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![MetricBuilder::new("counter", "Counter").integer().build()]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![CommandBuilder::new("increment").build()]
    }

    async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> {
        Ok(json!({"ok": true}))
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![metric_int!("counter", self.counter.load(Ordering::SeqCst))])
    }
}

neomind_extension_sdk::neomind_export!(MyExtension);
```

### AI-Assisted Development

This repository includes a **Claude Code skill** for AI-powered extension development:

```bash
./skill/install.sh   # Install the skill
# Then ask Claude: "Create a NeoMind extension for..."
```

---

## Repository Structure

```
NeoMind-Extensions/
├── extensions/                    # All extension projects
│   ├── weather-forecast-v2/
│   ├── image-analyzer-v2/
│   ├── yolo-video-v2/
│   ├── yolo-device-inference/
│   ├── face-recognition/
│   ├── ocr-device-inference/
│   ├── stream-player/
│   ├── uink-rms-bridge/
│   ├── wasm-demo/
│   └── index.json               # Marketplace index (auto-generated)
├── scripts/
│   └── update-versions.sh        # Generate metadata.json + index.json
├── skill/                        # Claude Code skill for AI-assisted dev
├── build.sh                      # Unified build script
├── release.sh                    # Release helper
├── EXTENSION_GUIDE.md            # Complete developer guide
├── EXTENSION_FRONTEND_DESIGN_GUIDE.md  # Frontend design spec
├── CLAUDE.md                     # AI assistant instructions
└── Cargo.toml                    # Workspace configuration
```

---

## Build Scripts

| Command | Description |
|---------|-------------|
| `./build.sh` | Build all + create .nep packages |
| `./build.sh --dev` | Dev build + auto-install |
| `./build.sh --dev --single <ext>` | Dev build single extension |
| `./build.sh --release 2.6.0` | Release with version |
| `./build.sh --skip-frontend` | Skip frontend builds |
| `./release.sh 2.6.0` | Same as `./build.sh --release` |

---

## Platform Support

| Platform | Architecture | Binary | Target |
|----------|-------------|--------|--------|
| macOS | ARM64 | `*.dylib` | `aarch64-apple-darwin` |
| macOS | x86_64 | `*.dylib` | `x86_64-apple-darwin` |
| Linux | x86_64 | `*.so` | `x86_64-unknown-linux-gnu` |
| Linux | ARM64 | `*.so` | `aarch64-unknown-linux-gnu` |
| Windows | x86_64 | `*.dll` | `x86_64-pc-windows-msvc` |
| Windows | x86 | `*.dll` | `i686-pc-windows-msvc` |
| Cross-platform | Any | `*.wasm` | `wasm32-unknown-unknown` |

---

## Safety Requirements

**CRITICAL**: All native extensions MUST use `panic = "unwind"` in the workspace root `Cargo.toml`:

```toml
[profile.release]
panic = "unwind"  # REQUIRED! "abort" crashes the server on any panic
opt-level = 3
lto = "thin"
```

---

## License

MIT License
