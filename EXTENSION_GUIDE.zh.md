# NeoMind 扩展开发指南 V2

使用 **NeoMind Extension SDK V2**（ABI 版本 3）开发扩展的完整指南。

[English Guide](EXTENSION_GUIDE.md)

---

## 目录

1. [概述](#概述)
2. [架构](#架构)
3. [快速开始](#快速开始)
4. [SDK 参考](#sdk-参考)
5. [前端组件](#前端组件)
6. [构建与部署](#构建与部署)
7. [安全要求](#安全要求)

---

## 概述

NeoMind Extension SDK V2 为 Native 和 WASM 目标提供统一的开发体验。

### 核心特性

- **进程隔离架构**：所有扩展默认在独立进程中运行，崩溃不影响主进程
- **统一 SDK**：Native 和 WASM 单一代码库
- **ABI 版本 3**：新的扩展接口，改进安全性
- **声明式宏**：样板代码从 50+ 行减少到 5 行
- **前端组件**：基于 React 的仪表板小部件
- **流式处理**：支持实时数据流处理（视频、传感器等）

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
# 只需要 SDK 依赖！
neomind-extension-sdk = { path = "../../NeoMind/crates/neomind-extension-sdk" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
semver = "1"

[profile.release]
panic = "unwind"  # 安全性必需！
opt-level = 3
lto = "thin"
```

### 3. 实现扩展

```rust
// src/lib.rs
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

// ============================================================================
// 类型定义
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyResult {
    pub value: i64,
    pub message: String,
}

// ============================================================================
// 扩展实现
// ============================================================================

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
            ExtensionMetadata {
                id: "my-extension".to_string(),
                name: "My Extension".to_string(),
                version: Version::parse("1.0.0").unwrap(),
                description: Some("我的第一个扩展".to_string()),
                author: Some("Your Name".to_string()),
                homepage: None,
                license: Some("MIT".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "counter".to_string(),
                    display_name: "Counter".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: String::new(),
                    min: None,
                    max: None,
                    required: false,
                },
            ]
        })
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "increment".to_string(),
                    display_name: "Increment".to_string(),
                    payload_template: String::new(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to add".to_string(),
                            param_type: MetricDataType::Integer,
                            required: false,
                            default_value: Some(ParamMetricValue::Integer(1)),
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![json!({ "amount": 1 })],
                    llm_hints: "Increment the counter".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
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
        let now = chrono::Utc::now().timestamp_millis();
        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// 导出 FFI - 只需要这一行！
// ============================================================================

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

## SDK 参考

### FFI 接口

所有扩展必须导出以下函数：

| 函数 | 必需 | 描述 |
|-----|------|------|
| `neomind_extension_abi_version()` | 是 | 返回 ABI 版本（3） |
| `neomind_extension_metadata()` | 是 | 返回扩展元数据 |
| `neomind_extension_create()` | 是 | 创建扩展实例 |
| `neomind_extension_destroy()` | 是 | 清理扩展 |

### 元数据结构

```rust
#[repr(C)]
pub struct CExtensionMetadata {
    pub abi_version: u32,        // 必须为 3
    pub id: *const c_char,       // 扩展 ID
    pub name: *const c_char,     // 显示名称
    pub version: *const c_char,  // 语义版本
    pub description: *const c_char,
    pub author: *const c_char,
    pub metric_count: usize,
    pub command_count: usize,
}
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
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
}

export const MyCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function MyCard(props, ref) {
    const { title = 'My Extension', className = '' } = props

    return (
      <div ref={ref} className={`my-card ${className}`}>
        <style>{`
          .my-card {
            --ext-bg: rgba(255, 255, 255, 0.25);
            --ext-fg: hsl(240 10% 10%);
          }
          .dark .my-card {
            --ext-bg: rgba(30, 30, 30, 0.4);
            --ext-fg: hsl(0 0% 95%);
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
  "entrypoint": "my-extension-components.umd.js",
  "components": [
    {
      "name": "MyCard",
      "type": "card",
      "displayName": "My Extension Card",
      "description": "显示扩展数据",
      "defaultSize": { "width": 300, "height": 200 },
      "refreshable": true,
      "refreshInterval": 5000
    }
  ],
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

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
cargo build --release -p neomind-my-extension

# 构建 WASM 目标
cargo build --release --target wasm32-unknown-unknown
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

### 进程隔离（V2 新架构）

**V2 版本中，所有扩展默认在独立进程中运行**，无需额外配置。这带来以下优势：

- 崩溃隔离：扩展 panic 不会影响 NeoMind 主进程
- 内存隔离：每个扩展有独立的内存空间
- 资源管理：可监控和限制每个扩展的资源使用

### Panic 处理

由于进程隔离，扩展 panic 不会导致主进程崩溃。但仍建议遵循以下最佳实践：

```rust
// 推荐：使用 ? 操作符进行错误传播
fn process_data(&self) -> Result<Data> {
    let value = self.get_value()?;  // 而非 unwrap()
    Ok(Data::new(value))
}

// 推荐：使用 unwrap_or 提供默认值
let count = args.get("count").and_then(|v| v.as_i64()).unwrap_or(1);

// 避免：直接 unwrap 可能导致扩展进程退出
let value = some_option.unwrap();  // 不推荐
```

### 异步运行时注意事项

- `produce_metrics()` 是**同步方法** - 不要在其中使用 `.await`
- `execute_command()` 是**异步方法** - 可以使用 `.await`

```rust
// ❌ 错误：produce_metrics 中使用异步
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let data = block_on(async_fetch());  // 不要这样做！
    // ...
}

// ✅ 正确：缓存数据，同步返回
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let data = self.cached_data.lock().unwrap().clone();
    Ok(vec![/* ... */])
}
```

### 资源配置（可选）

如需自定义扩展进程的资源限制，可在扩展的 `metadata.json` 中配置：

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

## WASM 扩展开发

### 概述

WASM（WebAssembly）扩展提供跨平台支持，可以在任何支持 WASM 的系统上运行。

**优势**：
- ✅ 跨平台兼容性
- ✅ 沙箱安全隔离
- ✅ 体积小（~100-200KB）
- ✅ 90-95% 的原生性能

**限制**：
- ⚠️ 无法直接访问系统资源
- ⚠️ 需要通过 Host Functions 与外部交互
- ⚠️ 不支持异步运行时（必须使用同步 API）

### 快速开始

#### 1. 配置 Cargo.toml

```toml
[package]
name = "my-wasm-extension"
version = "1.0.0"
edition = "2021"

[lib]
name = "neomind_extension_my_wasm_extension"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "path/to/sdk" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
semver = "1"
chrono = { version = "0.4", features = ["serde"] }

# WASM 优化配置
[profile.release]
opt-level = "s"
lto = true
panic = "abort"  # WASM 必需
strip = true
```

#### 2. 实现 Extension Trait

```rust
#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicI64, Ordering};

use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionMetadata, ExtensionError,
    ExtensionMetricValue, MetricDescriptor, ExtensionCommand, MetricDataType,
    ParameterDefinition, ParamMetricValue, Result,
};
use semver::Version;

pub struct MyWasmExtension {
    counter: AtomicI64,
}

impl MyWasmExtension {
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl Extension for MyWasmExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        // 使用静态变量返回元数据
        // ...
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        // 声明指标
        // ...
    }

    fn commands(&self) -> &[ExtensionCommand] {
        // 声明命令
        // ...
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        // 执行命令（同步方式）
        // ...
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // 返回当前指标值
        // ...
    }
}

// 导出 FFI 函数
neomind_extension_sdk::neomind_export!(MyWasmExtension);
```

#### 3. 构建 WASM

```bash
# 添加 WASM 目标
rustup target add wasm32-unknown-unknown

# 构建
cargo build --target wasm32-unknown-unknown --release
```

#### 4. 安装

```bash
# 创建扩展目录
mkdir -p /path/to/neomind/extensions/my-wasm-extension

# 复制 WASM 文件
cp target/wasm32-unknown-unknown/release/neomind_extension_my_wasm_extension.wasm \
   /path/to/neomind/extensions/my-wasm-extension/extension.wasm

# 复制元数据
cp metadata.json /path/to/neomind/extensions/my-wasm-extension/
```

### Host Functions

WASM 扩展可以通过 Host Functions 访问外部资源：

```rust
#[cfg(target_arch = "wasm32")]
use neomind_extension_sdk::wasm::Host;

// 发送 HTTP 请求
let response = Host::http_request("GET", "https://api.example.com/data")?;

// 记录日志
Host::log("info", "Hello from WASM!");

// 存储指标
Host::store_metric("temperature", &json!(25.5));

// 读取设备数据
let data = Host::device_read("sensor-01", "temperature")?;

// 写入设备
let result = Host::device_write("actuator-01", "turn_on", &json!({"value": true}))?;
```

### 条件编译

使用条件编译处理 Native 和 WASM 的差异：

```rust
// Native 特定代码
#[cfg(not(target_arch = "wasm32"))]
{
    // 使用 tokio 异步运行时
    // 访问文件系统
    // 使用标准库
}

// WASM 特定代码
#[cfg(target_arch = "wasm32")]
{
    // 使用同步 API
    // 通过 Host Functions 访问资源
    // 避免 alloc 以外的标准库
}
```

### 完整示例

参考 `extensions/wasm-demo/` 目录，这是一个完整的 WASM 扩展示例。

---

## 故障排除

### 扩展无法加载

1. 检查 ABI 版本：`neomind_extension_abi_version()` 必须返回 3
2. 验证 panic 设置：Cargo.toml 中 `panic = "unwind"`（Native）或 `panic = "abort"`（WASM）
3. 检查二进制格式：必须匹配平台（macOS 用 .dylib，Linux 用 .so）

### WASM 扩展问题

1. **编译错误**: 确保添加了 `wasm32-unknown-unknown` 目标
2. **运行时错误**: 检查是否使用了不支持的 API（如文件系统）
3. **内存问题**: WASM 默认内存限制，大数据使用流式处理

### 前端不显示

1. 验证 frontend.json 存在于扩展目录
2. 检查 frontend.json 中的组件名称
3. 验证 UMD 构建输出存在

### 性能问题

1. 为 AI 扩展启用进程隔离
2. 计算密集型任务考虑使用 Native 而非 WASM
3. 在隔离配置中使用适当的 max_memory_mb

---

## 许可证

MIT 许可证
