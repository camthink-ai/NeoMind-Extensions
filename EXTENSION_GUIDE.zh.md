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

- **统一 SDK**：Native 和 WASM 单一代码库
- **ABI 版本 3**：新的扩展接口，改进安全性
- **声明式宏**：样板代码从 50+ 行减少到 5 行
- **前端组件**：基于 React 的仪表板小部件
- **进程隔离**：高风险扩展的可选隔离

### 扩展类型

| 类型 | 文件扩展名 | 用途 |
|-----|-----------|------|
| Native | `.dylib` / `.so` / `.dll` | 最大性能，AI 推理 |
| WASM | `.wasm` | 跨平台，沙箱执行 |

---

## 架构

```
┌─────────────────────────────────────────────────────────┐
│                    NeoMind Core                         │
│  ┌─────────────────────────────────────────────────────┐│
│  │               Extension Registry                     ││
│  │  ┌────────────┐ ┌────────────┐ ┌────────────────┐   ││
│  │  │In-Process  │ │OutOfProcess│ │WASM Sandbox    │   ││
│  │  │Loader      │ │Loader      │ │Loader          │   ││
│  │  └────────────┘ └────────────┘ └────────────────┘   ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
                          │
         ┌────────────────┼────────────────┐
         ▼                ▼                ▼
  ┌────────────┐   ┌────────────┐   ┌────────────────┐
  │Native Ext  │   │Subprocess  │   │WASM Runtime    │
  │(共享内存)  │   │(进程隔离)  │   │(沙箱隔离)      │
  └────────────┘   └────────────┘   └────────────────┘
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

### 关键：Panic 处理

**所有扩展必须使用 `panic = "unwind"` 编译**

```toml
# Cargo.toml (workspace)
[profile.release]
opt-level = 3
lto = "thin"
panic = "unwind"  # 必需！

[profile.dev]
opt-level = 1
lto = false
panic = "unwind"  # 必需！
```

使用 `panic = "abort"` 会导致任何 panic 时整个 NeoMind 服务器崩溃。

### 进程隔离

对于高风险扩展（AI 推理、重计算）：

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

## 平台支持

| 平台 | 架构 | 二进制扩展 |
|-----|------|-----------|
| macOS | ARM64 | `*.dylib` |
| macOS | x86_64 | `*.dylib` |
| Linux | x86_64 | `*.so` |
| Linux | ARM64 | `*.so` |
| Windows | x86_64 | `*.dll` |

---

## 故障排除

### 扩展无法加载

1. 检查 ABI 版本：`neomind_extension_abi_version()` 必须返回 3
2. 验证 panic 设置：Cargo.toml 中 `panic = "unwind"`
3. 检查二进制格式：必须匹配平台（macOS 用 .dylib，Linux 用 .so）

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
