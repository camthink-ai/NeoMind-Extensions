# NeoMind 扩展

NeoMind 边缘 AI 平台的官方扩展仓库。

[English Documentation](README.md)

## 概述

本仓库包含使用 **NeoMind Extension SDK V2** 构建的官方维护扩展。

### 核心特性

- **统一 SDK**：单个 SDK 同时支持 Native 和 WASM 目标
- **ABI 版本 3**：新的扩展接口，改进安全性
- **前端组件**：基于 React 的仪表板组件
- **CSS 变量主题**：明暗模式支持
- **进程隔离**：高风险扩展的可选隔离

---

## 可用扩展

### 天气预报 V2

**ID**: `weather-forecast-v2`

使用 Open-Meteo API 的实时天气数据。

| 能力 | 类型 | 描述 |
|-----|------|------|
| `get_weather` | 命令 | 获取任意城市的天气 |
| temperature_c | 指标 | 摄氏温度 |
| humidity_percent | 指标 | 相对湿度 |
| wind_speed_kmph | 指标 | 风速 |

**前端组件**：WeatherCard - 美观的天气显示，带动态图标

```bash
# 构建
cargo build --release -p neomind-weather-forecast-v2

# 安装
cp target/release/libneomind_extension_weather_forecast_v2.dylib ~/.neomind/extensions/
```

---

### 图像分析器 V2

**ID**: `image-analyzer-v2`

使用 YOLOv8 目标检测的 AI 图像分析。

| 能力 | 类型 | 描述 |
|-----|------|------|
| `analyze_image` | 命令 | 分析图像中的物体 |
| images_processed | 指标 | 已处理图像总数 |
| total_detections | 指标 | 检测到的物体数 |
| avg_processing_time_ms | 指标 | 平均处理时间 |

**前端组件**：ImageAnalyzer - 拖放图像上传，带检测框显示

```bash
# 构建
cargo build --release -p neomind-image-analyzer-v2

# 安装
cp target/release/libneomind_extension_image_analyzer_v2.dylib ~/.neomind/extensions/
```

---

### YOLO 视频 V2

**ID**: `yolo-video-v2`

使用 YOLOv11 目标检测的实时视频流处理。

| 能力 | 类型 | 描述 |
|-----|------|------|
| `start_stream` | 命令 | 启动视频流 |
| `stop_stream` | 命令 | 停止视频流 |
| `get_stream_stats` | 命令 | 获取流统计 |
| active_streams | 指标 | 活跃流数量 |
| total_frames_processed | 指标 | 已处理帧数 |

**前端组件**：YoloVideoDisplay - MJPEG 流显示，带实时检测

```bash
# 构建
cargo build --release -p neomind-yolo-video-v2

# 安装
cp target/release/libneomind_extension_yolo_video_v2.dylib ~/.neomind/extensions/
```

---

## 快速开始

### 前置条件

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind-Extension.git
cd NeoMind-Extension
```

### 构建所有扩展

```bash
cargo build --release
```

### 构建前端组件

```bash
cd extensions/weather-forecast-v2/frontend
npm install && npm run build

cd ../../image-analyzer-v2/frontend
npm install && npm run build

cd ../../yolo-video-v2/frontend
npm install && npm run build
```

---

## SDK V2 特性

### FFI 接口

```rust
#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    3  // ABI 版本 3
}

#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> CExtensionMetadata {
    // 返回扩展元数据
}

#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Any>> {
    // 创建扩展实例
}

#[no_mangle]
pub extern "C" fn neomind_extension_destroy(ptr: *mut RwLock<Box<dyn Any>>) {
    // 清理扩展
}
```

---

## 安全要求

**关键**：所有扩展必须使用 `panic = "unwind"` 编译

```toml
# 在 Cargo.toml workspace 中
[profile.release]
opt-level = 3
lto = "thin"
panic = "unwind"  # 安全必需！
```

使用 `panic = "abort"` 会导致任何 panic 时整个 NeoMind 服务器崩溃。

---

## 进程隔离

对于高风险扩展（如 AI 推理），启用进程隔离：

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

## 仓库结构

```
NeoMind-Extension/
├── extensions/
│   ├── weather-forecast-v2/    # 天气扩展
│   ├── image-analyzer-v2/      # 图像分析扩展
│   ├── yolo-video-v2/          # 视频处理扩展
│   └── index.json              # 市场索引
├── Cargo.toml                  # Workspace 配置
├── EXTENSION_GUIDE.md          # 开发者指南
└── README.md                   # 本文件
```

---

## 平台支持

| 平台 | 架构 | 二进制扩展 |
|-----|------|-----------|
| macOS | ARM64 (Apple Silicon) | `*.dylib` |
| macOS | x86_64 (Intel) | `*.dylib` |
| Linux | x86_64 | `*.so` |
| Linux | ARM64 | `*.so` |
| Windows | x86_64 | `*.dll` |

---

## 许可证

MIT 许可证
