# NeoMind Extensions

Official extension repository for the NeoMind Edge AI Platform.

[中文文档](README.zh.md)

## Overview

This repository contains officially maintained extensions built with the **NeoMind Extension SDK V2**.

### Key Features

- **Unified SDK**: Single SDK for both Native and WASM targets
- **ABI Version 3**: New extension interface with improved safety
- **Frontend Components**: React-based dashboard components
- **CSS Variable Theming**: Light/dark mode support
- **Process Isolation**: Optional isolation for high-risk extensions

---

## Available Extensions

> **Latest Release (v2.0.0)**: 27 extension packages across 6 platforms  
> **ABI Version**: 3 (Process Isolation Architecture)  
> **Release**: [View on GitHub](https://github.com/camthink-ai/NeoMind-Extensions/releases/tag/v2.0.0)

### Weather Forecast V2

**ID**: `weather-forecast-v2`

Real-time weather data using Open-Meteo API.

| Capability | Type | Description |
|-----------|------|-------------|
| `get_weather` | Command | Get weather for any city |
| temperature_c | Metric | Temperature in Celsius |
| humidity_percent | Metric | Relative humidity |
| wind_speed_kmph | Metric | Wind speed |

**Frontend Component**: WeatherCard - Beautiful weather display with dynamic icons

```bash
# Build
cargo build --release -p neomind-weather-forecast-v2

# Install
cp target/release/libneomind_extension_weather_forecast_v2.dylib ~/.neomind/extensions/
```

---

### Image Analyzer V2

**ID**: `image-analyzer-v2`

AI-powered image analysis using YOLOv8 object detection.

| Capability | Type | Description |
|-----------|------|-------------|
| `analyze_image` | Command | Analyze image for objects |
| images_processed | Metric | Total images processed |
| total_detections | Metric | Objects detected |
| avg_processing_time_ms | Metric | Average processing time |

**Frontend Component**: ImageAnalyzer - Drag-drop image upload with detection boxes

```bash
# Build
cargo build --release -p neomind-image-analyzer-v2

# Install
cp target/release/libneomind_extension_image_analyzer_v2.dylib ~/.neomind/extensions/
```

---

### YOLO Video V2

**ID**: `yolo-video-v2`

Real-time video stream processing with YOLOv11 object detection.

| Capability | Type | Description |
|-----------|------|-------------|
| `start_stream` | Command | Start video detection stream |
| `stop_stream` | Command | Stop video stream |
| `get_stream_stats` | Command | Get stream statistics |
| active_streams | Metric | Number of active streams |
| total_frames_processed | Metric | Frames processed |

**Frontend Component**: YoloVideoDisplay - MJPEG stream display with live stats

**Safety Note**: This extension uses AI inference with process isolation enabled by default. The YOLOv11 model remains loaded across video sessions for optimal performance.

```bash
# Build
cargo build --release -p neomind-yolo-video-v2

# Install
cp target/release/libneomind_extension_yolo_video_v2.dylib ~/.neomind/extensions/
```

---

### YOLO Device Inference

**ID**: `yolo-device-inference`

High-performance YOLOv8-based object detection for device camera feeds.

| Capability | Type | Description |
|-----------|------|-------------|
| `start_inference` | Command | Start camera inference |
| `stop_inference` | Command | Stop inference |
| `get_inference_stats` | Command | Get inference statistics |
| active_sessions | Metric | Number of active sessions |
| total_detections | Metric | Total objects detected |

**Frontend Component**: YoloDeviceInference - Real-time camera feed with detection overlay

**Safety Note**: This extension uses AI inference with process isolation. The YOLOv8 model is loaded once and reused across sessions.

```bash
# Build
cargo build --release -p neomind-yolo-device-inference

# Install
cp target/release/libneomind_extension_yolo_device_inference.dylib ~/.neomind/extensions/
```

---


## CLI Tools

NeoMind provides a command-line interface for service management and extension operations:

### Health Check

```bash
neomind health
```

Check server status, database connection, LLM backend, and extensions directory.

### Log Viewing

```bash
# View all logs
neomind logs

# View logs for a specific extension
neomind logs --extension my-extension

# Filter by log level
neomind logs --level error

# Follow logs in real-time
neomind logs --follow

## 🚀 NeoMind Extension CLI

为了加速扩展开发，我们提供了一个专门的 CLI 工具 `neomind-ext`。

### 快速开始

```bash
# 安装工具
cargo install --path neomind-ext

# 创建新扩展
neomind-ext new my-extension --with-frontend

# 构建和打包
cd my-extension
neomind-ext build --release
neomind-ext package --with-frontend
```

### 核心功能

| 命令 | 说明 |
|------|------|
| `neomind-ext new` | 创建新扩展项目 |
| `neomind-ext build` | 构建扩展 |
| `neomind-ext package` | 打包为 .nep 文件 |
| `neomind-ext validate` | 验证扩展规范 |
| `neomind-ext test` | 运行测试 |
| `neomind-ext watch` | 监视文件变化并自动重建 |
| `neomind-ext clean` | 清理构建产物 |

### 优势

- ⚡ **3 分钟快速启动** - 从零到运行
- 📦 **自动化打包** - 一键生成 .nep 文件
- ✅ **规范验证** - 自动检查扩展规范
- 🧪 **内置测试** - 快速验证功能
- 📝 **模板系统** - 包含最佳实践

**文档**: [neomind-ext/README.md](neomind-ext/README.md) | [neomind-ext/QUICKSTART.md](neomind-ext/QUICKSTART.md)

---

```

### Extension Management

```bash
# List installed extensions
neomind extension list

# Install a .nep package
neomind extension install my-extension-1.0.0.nep

# Uninstall an extension
neomind extension uninstall my-extension

# Validate package format
neomind extension validate my-extension-1.0.0.nep
```

---

## Quick Start

### Prerequisites

- Rust 1.75+
- NeoMind Extension SDK (located at `../../NeoMind/crates/neomind-extension-sdk`)

### Building All Extensions

```bash
# Build all extensions
cargo build --release

# Output binaries
ls target/release/libneomind_extension_*.dylib
```

### Installing Extensions

#### Method 1: Using NeoMind CLI (Recommended)

The NeoMind CLI provides convenient commands for extension management:

```bash
# Check system health
neomind health

# View extension logs
neomind logs --extension my-extension

# List installed extensions
neomind extension list

# Install a .nep package
neomind extension install my-extension-1.0.0.nep

# Uninstall an extension
neomind extension uninstall my-extension

# Validate a package
neomind extension validate my-extension-1.0.0.nep
```

#### Method 2: Manual Installation

```bash
# Create extensions directory
mkdir -p ~/.neomind/extensions

# Copy native binaries
cp target/release/libneomind_extension_*.dylib ~/.neomind/extensions/

# Copy frontend components (optional)
cp -r extensions/weather-forecast-v2/frontend/dist ~/.neomind/extensions/weather-forecast-v2/frontend/
```

---

## AI-Assisted Development with Claude Code

### 🤖 Claude Code Skill

Get AI-powered guidance for extension development! This repository includes a comprehensive **Claude Code skill** that provides intelligent assistance when developing NeoMind extensions.

**Features:**
- 🎯 **Automatic Guidance**: Claude automatically helps when you ask about extension development
- 📚 **Complete Documentation**: Architecture, SDK API, frontend development guides
- 💻 **Code Generation**: Generate boilerplate code and templates
- 🔧 **Troubleshooting**: AI-assisted debugging and problem solving
- 💡 **Best Practices**: Built-in knowledge of NeoMind patterns

**Installation:**

```bash
# From repository root
./skill/install.sh
```

**Usage:**

Ask Claude naturally:
- "How do I create a NeoMind extension?"
- "Create a temperature sensor extension"
- "Add a frontend component to my extension"

Or invoke directly:
```bash
/neomind-extension [extension-name]
```

**📖 Learn More**: See [skill/README.md](skill/README.md) for complete documentation.

---

## For Extension Developers

### Extension Structure

```
extensions/
└── your-extension-v2/
├── Cargo.toml              # Rust project configuration
├── src/
│   └── lib.rs              # Extension implementation
├── frontend/               # React components (optional)
│   ├── src/
│   │   └── index.tsx       # Component implementation
│   ├── package.json        # npm dependencies
│   ├── vite.config.ts      # Vite build config
│   ├── tsconfig.json       # TypeScript config
│   └── frontend.json       # Component manifest
└── README.md               # Documentation
```

### Using the SDK

```rust
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError,
    MetricDefinition, CommandDefinition, ExtensionMetricValue,
};

pub struct MyExtension {
    metadata: ExtensionMetadata,
}

impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ExtensionError> {
        match command {
            "my_command" => Ok(serde_json::json!({"result": "success"})),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }
}
```

### Frontend Components

```tsx
import { forwardRef, useState, useCallback } from 'react'

const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>
): Promise<{ success: boolean; data?: T; error?: string }> {
  const response = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args })
  })
  return response.json()
}

export const MyComponent = forwardRef<HTMLDivElement, Props>(
  function MyComponent(props, ref) {
    const { dataSource, className = '' } = props
    const extensionId = dataSource?.extensionId || 'my-extension-v2'

    // Component implementation...

    return (
      <div ref={ref} className={`my-component ${className}`}>
        {/* ... */}
      </div>
    )
  }
)
```

### CSS Variable Theming

```css
.my-component {
  --ext-bg: rgba(255, 255, 255, 0.25);
  --ext-fg: hsl(240 10% 10%);
  --ext-muted: hsl(240 5% 40%);
  --ext-border: rgba(255, 255, 255, 0.5);
  --ext-accent: hsl(221 83% 53%);
}

.dark .my-component {
  --ext-bg: rgba(30, 30, 30, 0.4);
  --ext-fg: hsl(0 0% 95%);
  --ext-muted: hsl(0 0% 65%);
}
```

---

## ABI Version 3

All V2 extensions use ABI version 3, which provides:

- **Improved safety**: Better panic handling
- **Frontend support**: Built-in component registration
- **Standardized metrics**: Unified metric types
- **Better error handling**: Structured error responses

### FFI Exports

```rust
#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    3  // ABI Version 3
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
    // Create extension instance
}

#[no_mangle]
pub extern "C" fn neomind_extension_destroy(ptr: *mut RwLock<Box<dyn Any>>) {
    // Cleanup extension
}
```

---


## Memory Considerations for Large Extensions

**Important**: Large extensions (e.g., YOLO extensions) are **not auto-loaded during server startup** to prevent Out of Memory (OOM) errors.

### Affected Extensions

- `yolo-device-inference` - 38MB+ binary + 200MB+ model files
- `yolo-video-v2` - Video processing extension

### Manual Loading Methods

For these large extensions, load them manually after server startup:

```bash
# Method 1: Using CLI
neomind extension install yolo-device-inference-1.0.0.nep

# Method 2: Using Web UI
# Visit http://localhost:9375 → Extensions → Add Extension → Select .nep file

# Method 3: Using API
curl -X POST http://localhost:9375/api/extensions/upload/file \
  -H "Content-Type: application/octet-stream" \
  --data-binary @yolo-device-inference-1.0.0.nep
```

### Why Manual Loading?

YOLO extensions contain:
- Large binary files (38MB+)
- Deep learning models (200MB+)
- Auto-loading during server startup would cause system memory exhaustion

**Solution**: Lazy loading - extensions are loaded only when explicitly requested by the user.

---

## Safety Requirements

**CRITICAL**: All extensions MUST be compiled with `panic = "unwind"`

```toml
# In Cargo.toml workspace
[profile.release]
opt-level = 3
lto = "thin"
panic = "unwind"  # REQUIRED for safety!
```

Using `panic = "abort"` will cause the entire NeoMind server to crash on any panic.

---

## Process Isolation

For high-risk extensions (like AI inference), enable process isolation:

```json
// manifest.json
{
  "isolation": {
    "mode": "process",
    "timeout_seconds": 30,
    "max_memory_mb": 512,
    "restart_on_crash": true
  }
}
```

---

## Repository Structure

```
NeoMind-Extension/
├── extensions/
│   ├── weather-forecast-v2/    # Weather extension
│   ├── image-analyzer-v2/      # Image analysis extension
│   ├── yolo-video-v2/          # Video processing extension
│   └── index.json              # Marketplace index
├── skill/                      # Claude Code skill for AI-assisted development
│   ├── install.sh              # Skill installation script
│   ├── README.md               # Skill overview and quick start
│   ├── GUIDE.md                # Complete skill documentation
│   ├── QUICK_START.md          # Interactive examples
│   └── neomind-extension/      # Skill package
│       ├── SKILL.md            # Main skill instructions
│       ├── reference/          # Architecture, SDK, frontend guides
│       └── examples/           # Working code examples
├── Cargo.toml                  # Workspace configuration
├── EXTENSION_GUIDE.md          # Developer guide
└── README.md                   # This file
```

---

## Platform Support

| Platform | Architecture | Binary Extension |
|----------|--------------|------------------|
| macOS | ARM64 (Apple Silicon) | `*.dylib` |
| macOS | x86_64 (Intel) | `*.dylib` |
| Linux | x86_64 | `*.so` |
| Linux | ARM64 | `*.so` |
| Windows | x86_64 (64-bit) | `*.dll` |
| Windows | x86 (32-bit) | `*.dll` |

**Total: 6 platforms, 27 extension packages (v2.0.0)**

---

## License

MIT License
