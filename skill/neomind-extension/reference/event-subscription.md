# 事件订阅指南 (Event Subscription Guide)

> 双语版 — 中英文混排，便于检索。

## Overview / 概述

NeoMind 扩展可以订阅平台事件并做出反应。事件通过 `EventBus` 广播，由
`EventDispatcher` 按 `event_subscriptions()` 过滤后，**异步推送给每个扩展进程**
（通过 `EventPush` IPC 通道）。

> **重要**：事件由 `EventDispatcher` 按 `event_subscriptions()` **过滤**。
> 不重写该方法（默认 `&[]`），所有事件都会被静默丢弃。这是已知的头号踩坑点。

## 1. 订阅声明

扩展通过重写 `event_subscriptions()` 方法声明要订阅的事件类型：

```rust
use neomind_extension_sdk::prelude::*;

pub struct MyExtension;

#[async_trait::async_trait]
impl Extension for MyExtension {
    fn event_subscriptions(&self) -> &[&str] {
        // 精确匹配 — 只接收列出的 event type
        &["AgentStreamChunk", "AgentStreamEnd"]
    }
}
```

### 订阅语义

| 写法 | 行为 |
|---|---|
| `&["AgentStreamChunk"]` | 精确匹配 — 只接收该类型 |
| `&["Agent"]` | **前缀匹配** — 接收所有以 `Agent` 开头的事件（`AgentStreamChunk`, `AgentExecutionStarted`, ...） |
| `&["*"]` 或 `&["all"]` | 通配 — 接收所有事件（**慎用**，吞吐压力大） |
| `&[]` （默认） | 不订阅任何事件 |

### 默认陷阱

SDK trait 默认实现是 `&[]`，**如果你不重写，事件分发器会静默过滤掉所有事件**。
这是 ChatStream 集成中最常见的 bug — 扩展写得没问题，但 `handle_event` 从来不被
调用，因为没有订阅。永远记得在用事件时显式重写 `event_subscriptions()`。

## 2. 事件处理

`handle_event` 是 **同步方法**（**不能用 `.await`**）。所有跨 await point 的
共享状态必须用 `parking_lot::Mutex` / `parking_lot::RwLock`，**不能**用
`tokio::Mutex`（会死锁）。

```rust
use neomind_extension_sdk::prelude::*;
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};
use parking_lot::RwLock;
use std::collections::HashMap;

pub struct EventMonitor {
    alert_count: AtomicI64,
    // 跨 handle_event + execute_command 共享：用 parking_lot
    per_device_state: RwLock<HashMap<String, DeviceState>>,
}

#[async_trait::async_trait]
impl Extension for EventMonitor {
    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        // EventDispatcher 把事件包成 envelope:
        //   { "event_type": "...", "payload": { ... 真实事件 ... }, "timestamp": ... }
        // 永远先 unwrap 一层
        let inner = payload.get("payload").unwrap_or(payload);

        match event_type {
            "DeviceMetric" => {
                let device_id = inner.get("device_id").and_then(|v| v.as_str()).unwrap_or("");
                let metric = inner.get("metric").and_then(|v| v.as_str()).unwrap_or("");
                let value = inner.get("value");

                // 温度告警示例
                if metric == "temperature"
                    && value.and_then(|v| v.as_f64()).map(|t| t > 30.0).unwrap_or(false)
                {
                    self.trigger_alert(device_id, "Temperature too high");
                }
            }
            "AgentStreamChunk" => {
                // ChatStream 的 LLM token 流
                if let Some(sid) = inner.get("session_id").and_then(|v| v.as_str()) {
                    // 路由到 per-session mpsc::Sender ...
                }
            }
            "AgentStreamEnd" => {
                // 权威的流终止信号 — 清理 session 状态
            }
            _ => {
                ext_debug!(event_type = %event_type, "unhandled event");
            }
        }
        Ok(())
    }
}
```

### handle_event 三大陷阱（每个都引发过真实 bug）

1. **没有重写 `event_subscriptions()`** → 默认 `&[]` → 所有事件被静默过滤。
2. **用了 `tokio::Mutex`** → `handle_event` 是 sync，无法 `.await` lock，会死锁
   或必须用 `try_lock`。**永远用 `parking_lot::RwLock` / `parking_lot::Mutex`**。
3. **直接读 `payload.get("session_id")`** → 读不到。实际送达的 shape 是
   `{event_type, payload: {session_id, ...}, timestamp}`。必须先
   `payload.get("payload").unwrap_or(payload)` 取内层。

## 3. 事件格式

所有事件使用统一的 envelope：

```json
{
  "event_type": "DeviceMetric",
  "payload": {
    "device_id": "sensor-1",
    "metric": "temperature",
    "value": 25.5,
    "timestamp": 1709481600000,
    "quality": 0.95
  },
  "timestamp": 1709481600000
}
```

- `event_type` — 类型名（用于 `event_subscriptions()` 匹配）
- `payload` — 真实事件数据（在 `handle_event` 里需要再 unwrap 一层）
- `timestamp` — unix 毫秒

## 4. 常用事件类型速查

完整列表见 `NeoMind/crates/neomind-core/src/event.rs` 的 `NeoMindEvent` 枚举。
常用分类：

### Agent / LLM 流（最重要 — ChatStream 集成必用）

| 事件 | 触发时机 | payload 关键字段 |
|---|---|---|
| `AgentStreamChunk` | LLM 产生一个 token / chunk | `session_id`, `chunk: {type, content}`, `timestamp` |
| `AgentStreamEnd` | 流终止（**权威终止信号**） | `session_id`, `reason`, `error`, `timestamp` |

> `chunk.type` 取值：`"Content"` / `"reasoning"` / `"end"`（**小写！**）。
> 推理模型会发中间 `"end"`，所以 chunk 里的 end **不是**权威终止信号 — 必须等
> `AgentStreamEnd` 才能清理 session 状态。

### 设备事件

| 事件 | 触发时机 |
|---|---|
| `DeviceMetric` | 设备指标更新 |
| `DeviceOnline` / `DeviceOffline` | 设备上线 / 离线 |
| `DeviceCommandResult` | 设备命令执行结果 |
| `DeviceDataUpdated` | 设备数据更新（如摄像头出新帧） |

### 规则 / Agent 执行

| 事件 | 触发时机 |
|---|---|
| `RuleEvaluated` / `RuleTriggered` / `RuleExecuted` | 规则引擎 |
| `AgentExecutionStarted` / `AgentExecutionCompleted` | Agent 执行生命周期 |
| `AgentThinking` / `AgentDecision` / `AgentProgress` | Agent 中间状态 |

### 告警 / 消息

| 事件 | 触发时机 |
|---|---|
| `AlertCreated` / `AlertAcknowledged` | 告警 |
| `MessageCreated` / `MessageAcknowledged` / `MessageResolved` | 消息中心 |

### 工具执行

| 事件 | 触发时机 |
|---|---|
| `ToolExecutionStart` / `ToolExecutionSuccess` / `ToolExecutionFailure` | MCP / 工具调用 |

### 扩展自身事件

| 事件 | 触发时机 |
|---|---|
| `ExtensionOutput` | 扩展输出更新 |
| `ExtensionLifecycle` | 扩展加载 / 卸载 / 崩溃 |
| `ExtensionCommandStarted` / `ExtensionCommandCompleted` / `ExtensionCommandFailed` | 命令执行 |

### 自定义事件

通过 `EventPublish` capability 发布的任意事件类型（`Custom(...)` 变体）。

### ❌ 已移除的事件

`WorkflowTriggered` / `WorkflowStepCompleted` / `WorkflowCompleted` 等
**Workflow 事件已废弃**（NeoMind 不再内置 workflow 引擎）。`EventFilter.workflow_id`
字段保留但已 deprecated。新代码不要订阅这些事件。

## 5. 完整示例：温度告警 + ChatStream 路由

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use neomind_extension_sdk::{CommandBuilder, MetricBuilder, metric_int};
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct EventMonitorExt {
    alert_count: AtomicI64,
    // 每个 chat session 一个 mpsc sender，handle_event 用 try_send 投递 chunk
    chat_streams: Arc<RwLock<HashMap<String, mpsc::Sender<String>>>>,
}

impl EventMonitorExt {
    pub fn new() -> Self {
        Self {
            alert_count: AtomicI64::new(0),
            chat_streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for EventMonitorExt {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Extension for EventMonitorExt {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("event-monitor", "Event Monitor", "1.0.0")
                .with_description("Demonstrates event subscription + ChatStream routing")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("alert_count", "Alert Count")
                .integer()
                .unit("count")
                .min(0.0)
                .build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("get_alert_count")
                .display_name("Get Alert Count")
                .description("Returns the total number of alerts triggered by events")
                .build(),
        ]
    }

    fn event_subscriptions(&self) -> &[&str] {
        // 订阅设备事件前缀 + ChatStream 关键事件
        &["Device", "AgentStreamChunk", "AgentStreamEnd"]
    }

    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        // 永远先 unwrap envelope
        let inner = payload.get("payload").unwrap_or(payload);

        match event_type {
            "DeviceMetric" => {
                let device_id = inner.get("device_id").and_then(|v| v.as_str()).unwrap_or("");
                let metric = inner.get("metric").and_then(|v| v.as_str()).unwrap_or("");
                let value = inner.get("value");

                if metric == "temperature"
                    && value.and_then(|v| v.as_f64()).map(|t| t > 30.0).unwrap_or(false)
                {
                    self.alert_count.fetch_add(1, Ordering::SeqCst);
                    ext_warn!(device = %device_id, "temperature alert");
                }
            }
            "AgentStreamChunk" => {
                let Some(sid) = inner.get("session_id").and_then(|v| v.as_str()) else {
                    return Ok(());   // 没有 session_id 直接丢
                };
                // try_send 而不是 send — handle_event 是 sync，不能 await
                if let Some(tx) = self.chat_streams.read().get(sid) {
                    let _ = tx.try_send(inner.to_string());
                }
            }
            "AgentStreamEnd" => {
                let sid = inner.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                // 注意：Phase 2 持久 session 不在这里 remove chat_streams
                // （只有 WS teardown 才清理，避免每轮都重新 open_session）
                ext_info!(session = %sid, "chat stream ended");
            }
            _ => {}
        }
        Ok(())
    }

    async fn execute_command(
        &self,
        command: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "get_alert_count" => {
                Ok(json!({ "alert_count": self.alert_count.load(Ordering::SeqCst) }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            metric_int!("alert_count", self.alert_count.load(Ordering::SeqCst)),
        ])
    }
}

neomind_extension_sdk::neomind_export!(EventMonitorExt);
```

## 6. 最佳实践

1. **选择性订阅** — 只订阅真正需要的事件类型，避免 `["*"]` 滥用。
2. **handle_event 必须快** — sync 调用，长任务用 channel 转交后台 task。
3. **parking_lot 而非 tokio::Mutex** — 跨 `handle_event` 和 `execute_command` 共享
   的状态必须用 sync 锁。
4. **try_send 而不是 send** — 在 `handle_event` 里向 mpsc 投递用 `try_send`，
   队列满时优雅丢而不是死锁。
5. **envelope unwrap** — 永远 `payload.get("payload").unwrap_or(payload)` 再读字段。
6. **`"end"` 不是终止信号** — 等 `AgentStreamEnd` 才是权威。

## 7. 调试技巧

如果 `handle_event` 不被调用：

1. 检查 `event_subscriptions()` 是否被重写且包含目标事件名。
2. 检查 runner 的 `ALLOWED_CAPABILITIES` 是否包含 `event_subscribe`。
3. 加一行 `ext_info!("got event: {}", event_type);` 看日志是否有任何事件到达。
4. 用 runner 的日志看 EventDispatcher 是否在过滤 — 默认 `&[]` 完全过滤。
