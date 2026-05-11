# NeoMind 扩展开发指南

面向 **NeoMind 扩展运行时** 开发扩展的完整指南。

[English Guide](EXTENSION_GUIDE.md)

---

## 目录

1. [概述](#概述)
2. [架构](#架构)
3. [快速开始](#快速开始)
4. [Extension Trait 参考](#extension-trait-参考)
5. [构建器模式](#构建器模式)
6. [能力系统](#能力系统)
7. [流式 API](#流式-api)
8. [辅助宏](#辅助宏)
9. [错误处理](#错误处理)
10. [前端组件](#前端组件)
11. [构建与部署](#构建与部署)
12. [安全要求](#安全要求)

---

## 概述

NeoMind 扩展在 Native 和 WASM 目标上共享一套运行时模型。

### 核心特性

- **进程隔离架构**：所有扩展默认在独立进程中运行，崩溃不影响主进程
- **统一运行时模型**：Native 和 WASM 共用一套开发与运行模型
- **运行时协议 v3**：更稳定的隔离扩展协议与安全边界
- **构建器模式**：流式 API 定义指标、命令和参数
- **声明式宏**：通过 `neomind_export!`、`metric_float!` 等减少样板代码
- **前端组件**：基于 React 的仪表板小部件
- **流式处理**：支持实时数据流处理（视频、传感器等）
- **能力系统**：细粒度平台功能访问控制

### 扩展类型

| 类型 | 文件扩展名 | 用途 |
|-----|-----------|------|
| Native | `.dylib` / `.so` / `.dll` | 最大性能，AI 推理 |
| WASM | `.wasm` | 跨平台，沙箱执行 |

---

## 架构

### V2 进程隔离架构

所有扩展默认在独立进程中运行，确保系统稳定性：

```
┌─────────────────────────────────────────────────────────────┐
│                   NeoMind 主进程                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │             UnifiedExtensionService                      ││
│  │  - 通过 stdin/stdout 进行 IPC 通信                       ││
│  │  - 管理所有扩展的生命周期                                 ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                  Extension Runner 进程                       │
│  - 您的扩展在此隔离环境中运行                                 │
│  - Native: 通过 FFI 加载                                     │
│  - WASM: 通过 wasmtime 执行                                  │
│  - 崩溃不影响主进程                                          │
└─────────────────────────────────────────────────────────────┘
```

### 进程隔离优势

- **崩溃安全**：扩展崩溃不影响 NeoMind 主进程
- **内存隔离**：每个扩展有独立的内存空间
- **资源限制**：可为每个扩展限制 CPU 和内存
- **独立生命周期**：扩展可独立重启，不影响其他扩展

### IPC 通信协议

主进程与扩展进程通过 JSON 消息进行通信：

```json
// 执行命令
{ "ExecuteCommand": { "command": "analyze", "args": {...}, "request_id": 1 } }

// 获取指标
{ "ProduceMetrics": { "request_id": 2 } }

// 流式处理
{ "InitStreamSession": { "session_id": "xxx", "config": {...} } }
```

---

## 快速开始

### 1. 创建扩展项目

```bash
# 从模板复制
cp -r extensions/weather-forecast-v2 extensions/my-extension
cd extensions/my-extension

# 更新 Cargo.toml
sed -i 's/weather-forecast-v2/my-extension/g' Cargo.toml
```

### 2. 配置 Cargo.toml

```toml
[package]
name = "my-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
semver = "1"
```

如果扩展是在 Cargo workspace 里构建，发布配置应放在 workspace 根 `Cargo.toml`。成员包里的 `[profile.release]` 会被 Cargo 忽略。

### 3. 实现扩展（构建器模式）

```rust
// src/lib.rs
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use neomind_extension_sdk::{MetricBuilder, CommandBuilder, ParamBuilder};
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct MyExtension {
    counter: AtomicI64,
}

impl MyExtension {
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("my-extension", "My Extension", "1.0.0")
                .with_description("我的自定义扩展")
                .with_author("Your Name")
                .with_license("MIT")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("counter", "Counter")
                .integer()
                .build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("increment")
                .display_name("Increment")
                .description("递增计数器")
                .param(
                    ParamBuilder::new("amount", MetricDataType::Integer)
                        .display_name("Amount")
                        .description("增加的数量")
                        .default(ParamMetricValue::Integer(1))
                        .build()
                )
                .sample(json!({ "amount": 1 }))
                .build(),
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "increment" => {
                let amount = args.get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({ "counter": new_value }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            metric_int!("counter", self.counter.load(Ordering::SeqCst)),
        ])
    }
}

// 导出 FFI - 只需要这一行！
neomind_extension_sdk::neomind_export!(MyExtension);
```

### 4. 构建

```bash
cargo build --release
```

### 5. 安装

```bash
cp target/release/libneomind_extension_my_extension.dylib ~/.neomind/extensions/
```

---

## Extension Trait 参考

### 必需方法

| 方法 | 返回值 | 同步/异步 | 描述 |
|------|--------|----------|------|
| `metadata()` | `&ExtensionMetadata` | 同步 | 扩展身份和版本信息 |
| `metrics()` | `Vec<MetricDescriptor>` | 同步 | 指标描述符 |
| `commands()` | `Vec<ExtensionCommand>` | 同步 | 命令描述符 |
| `execute_command()` | `Result<Value>` | **异步** | 执行命名命令 |
| `produce_metrics()` | `Result<Vec<ExtensionMetricValue>>` | 同步 | 产生当前指标值 |

### 可选生命周期方法

| 方法 | 默认值 | 描述 |
|------|--------|------|
| `init(&mut self)` | `Ok(())` | 初始化扩展（加载时调用一次） |
| `start(&mut self)` | `Ok(())` | 启动扩展（init 之后） |
| `stop(&mut self)` | `Ok(())` | 优雅停止 |
| `status(&self)` | `"unknown"` | 当前状态字符串 |
| `health_check(&self)` | `Ok(true)` | 异步健康检查 |
| `configure(&mut self, config)` | `Ok(())` | 应用配置变更 |
| `get_stats(&self)` | `ExtensionStats::default()` | 扩展统计信息 |
| `descriptor(&self)` | `None` | 可选描述符 |

### 流式方法

| 方法 | 描述 |
|------|------|
| `stream_capability(&self)` | 返回 `StreamCapability`（如支持流式处理） |
| `latest_output(&self)` | 获取推送模式的最新输出 |
| `init_session(&self, session)` | 初始化流式会话 |
| `process_session_chunk(&self, id, chunk)` | 处理会话中的数据块 |
| `close_session(&self, id)` | 关闭流式会话 |
| `process_chunk(&self, chunk)` | 处理单个数据块（无状态） |

### ExtensionMetadata 构建器

```rust
ExtensionMetadata::new("my-extension", "My Extension", "1.0.0")
    .with_description("扩展功能描述")
    .with_author("作者名")
    .with_homepage("https://example.com")
    .with_license("MIT")
    .with_config_parameters(vec![...])
```

### 扩展 ID 规范

```
{类别}-{名称}-v{主版本}

示例：
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2
```

---

## 构建器模式

SDK 提供流式构建器模式，替代冗长的结构体构造。

### MetricBuilder

```rust
use neomind_extension_sdk::MetricBuilder;

// 整数指标
MetricBuilder::new("counter", "Counter")
    .integer()
    .unit("count")
    .min(0.0)
    .build()

// 浮点数指标
MetricBuilder::new("temperature", "Temperature")
    .float()
    .unit("°C")
    .min(-40.0)
    .max(85.0)
    .build()

// 布尔指标
MetricBuilder::new("is_active", "Active")
    .boolean()
    .build()
```

| 方法 | 描述 |
|------|------|
| `.integer()` / `.float()` / `.boolean()` / `.string()` | 设置数据类型 |
| `.unit(str)` | 设置单位标签 |
| `.min(f64)` / `.max(f64)` | 设置范围 |
| `.required()` | 标记为必需 |

### CommandBuilder

```rust
use neomind_extension_sdk::CommandBuilder;

CommandBuilder::new("analyze")
    .display_name("分析图像")
    .description("运行图像分析")
    .param(
        ParamBuilder::new("image_data", MetricDataType::String)
            .display_name("图像数据")
            .description("Base64 编码的图像")
            .required()
            .build()
    )
    .param(
        ParamBuilder::new("threshold", MetricDataType::Float)
            .display_name("置信度阈值")
            .default(ParamMetricValue::Float(0.5))
            .min(0.0)
            .max(1.0)
            .build()
    )
    .sample(json!({ "image_data": "base64...", "threshold": 0.5 }))
    .build()
```

| 方法 | 描述 |
|------|------|
| `.display_name(str)` | 人类可读名称 |
| `.description(str)` | 命令描述 |
| `.param(ParamDefinition)` | 添加参数 |
| `.param_simple(...)` | 简单必需参数快捷方式 |
| `.param_optional(...)` | 可选参数快捷方式 |
| `.param_with_default(...)` | 带默认值参数快捷方式 |
| `.sample(Value)` | 添加示例载荷 |

### ParamBuilder

```rust
use neomind_extension_sdk::ParamBuilder;

ParamBuilder::new("city", MetricDataType::String)
    .display_name("城市")
    .description("城市名称")
    .required()
    .options(vec!["Beijing".into(), "Shanghai".into(), "New York".into()])
    .build()
```

| 方法 | 描述 |
|------|------|
| `.display_name(str)` | 人类可读名称 |
| `.description(str)` | 参数描述 |
| `.required()` / `.optional()` | 设置是否必需 |
| `.default(MetricValue)` | 设置默认值 |
| `.min(f64)` / `.max(f64)` | 设置范围 |
| `.options(Vec<String>)` | 设置允许值（下拉选择） |

---

## 能力系统

NeoMind 提供了一个**解耦的、版本化的能力系统**，允许扩展安全地访问平台功能。

### ExtensionCapability 枚举

| 能力 | 常量 | 描述 |
|------|------|------|
| `DeviceMetricsRead` | `device_metrics_read` | 读取设备指标 |
| `DeviceMetricsWrite` | `device_metrics_write` | 写入设备指标（含虚拟指标） |
| `DeviceControl` | `device_control` | 向设备发送命令 |
| `StorageQuery` | `storage_query` | 查询遥测存储 |
| `EventPublish` | `event_publish` | 发布事件 |
| `EventSubscribe` | `event_subscribe` | 订阅事件 |
| `TelemetryHistory` | `telemetry_history` | 查询设备遥测历史 |
| `MetricsAggregate` | `metrics_aggregate` | 聚合设备指标 |
| `ExtensionCall` | `extension_call` | 调用其他扩展 |
| `AgentTrigger` | `agent_trigger` | 触发 AI 代理 |
| `RuleTrigger` | `rule_trigger` | 触发自动化规则 |
| `DeviceTemplateRegister` | `device_template_register` | 注册设备类型模板 |
| `DeviceRegister` | `device_register` | 注册设备实例 |
| `DeviceUnregister` | `device_unregister` | 注销设备实例 |
| `Custom(String)` | — | 自定义能力 |

### 虚拟指标

扩展可以报告自定义指标，而无需真实硬件：

```rust
use neomind_extension_sdk::capabilities::device;

// 异步上下文（如 execute_command 中）
async fn report_metrics(&self) -> Result<()> {
    device::write_virtual_metric(
        "virtual-sensor-1",
        "temperature",
        25.5,
        None
    ).await?;
    Ok(())
}

// 同步上下文（如 produce_metrics 中）
fn report_metrics_sync(&self) -> Result<()> {
    device::write_virtual_metric_sync(
        "virtual-sensor-1",
        "temperature",
        25.5,
        Some(chrono::Utc::now().timestamp_millis())
    )?;
    Ok(())
}
```

**何时使用同步与异步：**
- 在 `produce_metrics()` 和其他非异步上下文中使用 `write_virtual_metric_sync()`
- 在异步函数如 `execute_command()` 中使用 `write_virtual_metric()`

---

## 流式 API

处理实时数据（视频、传感器等）的扩展可以实现流式处理。

### StreamCapability

```rust
fn stream_capability(&self) -> Option<StreamCapability> {
    Some(
        StreamCapability::push()
            .with_data_type(StreamDataType::Binary)
            .with_chunk_size(65536, 1048576)
    )
}
```

### 流模式

| 模式 | 方向 | 用途 |
|------|------|------|
| `Stateless` | 任意 | 无会话的单块处理 |
| `Stateful` | 任意 | 基于 `init_session` / `close_session` 的会话 |
| `Push` | 输出 | 扩展向客户端推送数据 |

### 流方向

| 方向 | 描述 |
|------|------|
| `Upload` | 客户端向扩展发送数据 |
| `Download` | 扩展向客户端发送数据 |
| `Bidirectional` | 双向传输 |

### 流数据类型

| 类型 | 描述 |
|------|------|
| `Binary` | 原始二进制数据 |
| `Text` | 文本数据 |
| `Json` | JSON 数据 |

### FlowControl

```rust
FlowControl {
    supports_backpressure: true,
    window_size: 16,
    supports_throttling: false,
    max_rate: 0,
}
```

---

## 辅助宏

### 指标创建宏

用一行代码替代冗长的结构体构造：

```rust
use neomind_extension_sdk::{metric_float, metric_int, metric_bool, metric_string};

// 每个宏自动填充时间戳
metric_float!("temperature", 25.5)   // Float 类型的 ExtensionMetricValue
metric_int!("counter", 42)           // Integer 类型的 ExtensionMetricValue
metric_bool!("is_active", true)      // Boolean 类型的 ExtensionMetricValue
metric_string!("status", "ok")       // String 类型的 ExtensionMetricValue
```

### 日志宏

```rust
use neomind_extension_sdk::{ext_info, ext_warn, ext_error, ext_debug};

ext_info!("扩展已启动");
ext_warn!("内存不足: {}MB", mem);
ext_error!("模型加载失败: {}", err);
ext_debug!("处理数据块 {}/{}", i, total);
```

### 静态辅助宏

```rust
use neomind_extension_sdk::static_metadata;

// 创建静态元数据（避免重复分配）
static_metadata! {
    ExtensionMetadata::new("my-ext", "My Extension", "1.0.0")
        .with_description("...")
}
```

---

## 错误处理

### ExtensionError 变体

| 变体 | 使用场景 |
|------|---------|
| `CommandNotFound(name)` | 未知命令名 |
| `InvalidArguments(msg)` | 参数错误或缺失 |
| `ExecutionFailed(msg)` | 通用执行失败 |
| `NotSupported(msg)` | 功能不支持 |
| `Timeout(msg)` | 操作超时 |
| `NotFound(msg)` | 资源未找到 |
| `InvalidFormat(msg)` | 数据格式错误 |
| `InferenceFailed(msg)` | ML 模型推理失败 |
| `SessionNotFound(id)` | 流式会话不存在 |
| `SessionAlreadyExists(id)` | 重复会话 |
| `InvalidStreamData(msg)` | 无效流数据 |
| `LoadFailed(msg)` | 扩展加载失败 |
| `SecurityError(msg)` | 安全违规 |
| `ConfigurationError(msg)` | 配置问题 |
| `Io(msg)` | I/O 错误 |
| `Json(msg)` | JSON 解析/序列化错误 |

### 错误传播模式

```rust
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    match command {
        "process_data" => {
            let data = args.get("data")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExtensionError::InvalidArguments("缺少 data 参数".into()))?;

            let result = self.process(data)
                .map_err(|e| ExtensionError::ExecutionFailed(format!("处理失败: {}", e)))?;

            Ok(json!({ "result": result }))
        }
        _ => Err(ExtensionError::CommandNotFound(command.to_string())),
    }
}
```

### Panic 安全

```rust
// 推荐：使用 ? 操作符
let value = self.get_value()?;

// 推荐：使用 unwrap_or 提供默认值
let count = args.get("count").and_then(|v| v.as_i64()).unwrap_or(1);

// 避免：直接 unwrap 可能导致扩展进程退出
let value = some_option.unwrap();
```

---

## 前端组件

### 项目结构

```
extensions/my-extension/frontend/
├── src/
│   └── index.tsx          # React 组件
├── package.json           # npm 依赖
├── vite.config.ts         # Vite 构建配置
├── tsconfig.json          # TypeScript 配置
├── frontend.json          # 组件清单
└── README.md              # 组件文档
```

### 组件模板

```tsx
// src/index.tsx
import { forwardRef, useState } from 'react'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: {
    type: string
    deviceId?: string
    device_id?: string
    extensionId?: string
    command?: string
    config?: Record<string, any>
    [key: string]: any
  }
  className?: string
  config?: Record<string, any>
  /** 打开全屏弹窗，渲染任意 React 内容（由宿主提供） */
  openFullscreen?: (content: React.ReactNode) => void
  /** 关闭全屏弹窗（由宿主提供） */
  closeFullscreen?: () => void
}

export const MyCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function MyCard(props, ref) {
    const { title = 'My Extension', className = '', config, dataSource } = props

    return (
      <div ref={ref} className={`my-card ${className}`}>
        <style>{`
          .my-card {
            --ext-bg: rgba(255, 255, 255, 0.25);
            --ext-fg: hsl(240 10% 10%);
            --ext-muted: hsl(240 5% 40%);
            --ext-border: rgba(255, 255, 255, 0.5);
            --ext-accent: hsl(221 83% 53%);
          }
          .dark .my-card {
            --ext-bg: rgba(30, 30, 30, 0.4);
            --ext-fg: hsl(0 0% 95%);
            --ext-muted: hsl(0 0% 65%);
          }
        `}</style>
        <h3>{title}</h3>
        {/* 组件内容 */}
      </div>
    )
  }
)

export default { MyCard }
```

### 前端清单

```json
{
  "id": "my-extension",
  "version": "1.0.0",
  "entrypoint": "my-extension-components.umd.cjs",
  "components": [
    {
      "name": "MyCard",
      "type": "card",
      "displayName": "My Extension Card",
      "description": "显示扩展数据",
      "defaultSize": { "width": 300, "height": 200 },
      "refreshable": true,
      "refreshInterval": 5000,
      "hasDataSource": true,
      "dataSourceAllowedTypes": ["device"],
      "configSchema": {
        "mode": {
          "type": "string",
          "title": "显示模式",
          "enum": ["auto", "dark", "light"],
          "enumTitles": ["自动", "深色", "浅色"],
          "default": "auto"
        }
      },
      "uiHints": {
        "fieldOrder": ["mode"],
        "visibilityRules": []
      }
    }
  ],
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

**配置对话框字段说明：**
- `hasDataSource: true` — 启用数据源绑定选项卡，用于设备/数据选择
- `dataSourceAllowedTypes` — 允许的数据源类型：`"device"`, `"device-metric"`, `"extension"` 等
- `configSchema` — 自动生成表单字段。使用 `enum` + `enumTitles` 生成下拉选择框
- `uiHints.visibilityRules` — 根据其他字段值条件显示/隐藏字段

### 构建前端

```bash
cd frontend
npm install
npm run build
```

---

## 构建与部署

### 构建命令

```bash
# 构建所有扩展
cargo build --release

# 构建特定扩展
cargo build --release -p my-extension

# 开发构建 + 自动安装
./build.sh --dev --single my-extension

# 构建所有并打包
./build.sh

# 带版本号发布
./build.sh --release 2.4.0
```

### 安装

```bash
# 创建扩展目录
mkdir -p ~/.neomind/extensions

# 复制 Native 二进制
cp target/release/libneomind_extension_my_extension.dylib ~/.neomind/extensions/

# 复制前端（如果存在）
cp -r extensions/my-extension/frontend/dist ~/.neomind/extensions/my-extension/frontend/
```

---

## 安全要求

### Panic 配置

**务必在 workspace 根 Cargo.toml 中设置 `panic = "unwind"`：**

```toml
[profile.release]
panic = "unwind"  # 必需！"abort" 会在任何 panic 时导致服务器崩溃
opt-level = 3
lto = "thin"
```

### 异步运行时注意事项

| 方法 | 类型 | 允许 `.await`? |
|------|------|---------------|
| `metadata()` | 同步 | 否 |
| `metrics()` | 同步 | 否 |
| `commands()` | 同步 | 否 |
| `produce_metrics()` | 同步 | **否** |
| `execute_command()` | 异步 | 是 |
| `health_check()` | 异步 | 是 |
| `configure()` | 异步 | 是 |

**模式：** 将异步结果缓存在原子类型中，供同步的 `produce_metrics()` 读取。

```rust
pub struct MyExtension {
    last_temperature: AtomicI64,  // 定点存储（温度 * 100）
}

// 在异步命令中：
self.last_temperature.store((temp * 100.0) as i64, Ordering::SeqCst);

// 在 produce_metrics 中（同步）：
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    Ok(vec![
        metric_float!("temperature", self.last_temperature.load(Ordering::SeqCst) as f64 / 100.0),
    ])
}
```

### 资源配置（可选）

如需自定义扩展进程的资源限制，可在 `metadata.json` 中配置：

```json
{
  "id": "yolo-video-v2",
  "version": "2.0.0",
  "process_config": {
    "timeout_seconds": 60,
    "max_memory_mb": 1024,
    "restart_on_crash": true,
    "restart_delay_ms": 1000
  }
}
```

---

## 平台支持

| 平台 | 架构 | 二进制扩展 |
|-----|------|-----------|
| macOS | ARM64 | `*.dylib` |
| macOS | x86_64 | `*.dylib` |
| Linux | x86_64 | `*.so` |
| Linux | ARM64 | `*.so` |
| Windows | x86_64 | `*.dll` |
| **跨平台** | 任意 | `*.wasm` |

---

## 故障排除

### 扩展无法加载

1. 检查 ABI 版本：`neomind_extension_abi_version()` 必须返回 3
2. 验证二进制格式：必须匹配平台（macOS 用 .dylib，Linux 用 .so）
3. 检查 extension runner 日志中的 IPC 错误

### 扩展进程崩溃

1. 检查可能导致 panic 的 `unwrap()` 或 `expect()` 调用
2. 检查命令执行中的错误处理
3. 处理大数据时监控内存使用

### 前端不显示

1. 验证 frontend.json 存在于扩展目录
2. 检查 frontend.json 中的组件名称
3. 验证 UMD 构建输出存在
4. 检查组件类型在所有扩展中是否唯一

### 性能问题

1. 在 process_config 中使用适当的超时值
2. 大载荷考虑分块传输
3. 在 produce_metrics() 中缓存结果，而非使用异步操作

---

## 许可证

MIT 许可证
