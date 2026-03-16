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

## AI 辅助开发（Claude Code）

### 🤖 Claude Code 技能

获得 AI 驱动的扩展开发指导！本仓库包含一个全面的 **Claude Code 技能**，可在开发 NeoMind 扩展时提供智能辅助。

**功能特性：**
- 🎯 **自动指导**：当您询问扩展开发问题时，Claude 会自动提供帮助
- 📚 **完整文档**：架构、SDK API、前端开发指南
- 💻 **代码生成**：生成样板代码和模板
- 🔧 **故障排查**：AI 辅助调试和问题解决
- 💡 **最佳实践**：内置 NeoMind 模式和标准知识

**安装：**

```bash
# 从仓库根目录运行
./skill/install.sh
```

**使用：**

自然地向 Claude 提问：
- "如何创建 NeoMind 扩展？"
- "创建一个温度传感器扩展"
- "为我的扩展添加前端组件"

或直接调用：
```bash
/neomind-extension [扩展名称]
```

**📖 了解更多**：查看 [skill/README.md](skill/README.md) 获取完整文档。

---

## CLI 工具

NeoMind 提供了一套命令行工具用于服务管理和扩展操作：

### 健康检查

```bash
neomind health
```

检查服务器状态、数据库连接、LLM 后端和扩展目录。

### 日志查看

```bash
# 查看所有日志
neomind logs

# 查看特定扩展的日志
neomind logs --extension my-extension

# 过滤错误日志
neomind logs --level error

# 实时跟踪日志
neomind logs --follow
```

### 扩展管理

```bash
# 列出已安装扩展
neomind extension list

# 安装 .nep 包
neomind extension install my-extension-1.0.0.nep

# 卸载扩展
neomind extension uninstall my-extension

# 验证包格式
neomind extension validate my-extension-1.0.0.nep
```

---


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


## 大型扩展的内存考虑

**重要提示**：大型扩展（如 YOLO 扩展）在服务启动时**不会自动加载**，以防止系统内存溢出（OOM）。

### 受影响的扩展

- `yolo-device-inference` - 38MB+ 二进制文件 + 200MB+ 模型文件
- `yolo-video-v2` - 视频处理扩展

### 手动加载方法

对于这些大型扩展，请在服务启动后通过以下方式手动加载：

```bash
# 方法 1: 使用 CLI
neomind extension install yolo-device-inference-1.0.0.nep

# 方法 2: 使用 Web UI
# 访问 http://localhost:9375 → 扩展 → 添加扩展 → 选择 .nep 文件

# 方法 3: 使用 API
curl -X POST http://localhost:9375/api/extensions/upload/file \
  -H "Content-Type: application/octet-stream" \
  --data-binary @yolo-device-inference-1.0.0.nep
```

### 为什么需要手动加载？

YOLO 扩展包含：
- 大型二进制文件（38MB+）
- 深度学习模型（200MB+）
- 如果在服务启动时自动加载，会导致系统内存耗尽

**解决方案**：延迟加载 - 仅在用户明确请求时才加载扩展。

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
├── skill/                      # Claude Code 技能（AI 辅助开发）
│   ├── install.sh              # 技能安装脚本
│   ├── README.md               # 技能总览和快速开始
│   ├── GUIDE.md                # 完整技能文档
│   ├── QUICK_START.md          # 交互式示例教程
│   └── neomind-extension/      # 技能包（Claude 读取的文件）
│       ├── SKILL.md            # 主技能指令
│       ├── reference/          # 架构、SDK、前端指南
│       └── examples/           # 工作示例代码
├── Cargo.toml                  # Workspace 配置
├── EXTENSION_GUIDE.md          # 开发者指南
├── EXTENSION_GUIDE.zh.md       # 开发者指南（中文）
└── README.md                   # 项目说明（英文）
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
