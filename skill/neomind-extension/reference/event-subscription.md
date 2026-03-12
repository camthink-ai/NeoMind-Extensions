# 事件订阅指南

## 概述

NeoMind 扩展可以订阅系统事件并对其做出反应。所有事件类型都自动支持，无需手动维护。

## 事件订阅

扩展通过实现 `event_subscriptions()` 方法声明要订阅的事件类型：

```rust
use neomind_extension_sdk::prelude::*;

pub struct MyExtension;

impl Extension for MyExtension {
    fn event_subscriptions(&self) -> &[&str] {
        // 订阅特定事件
        &["DeviceMetric", "AgentExecutionStarted"]
    }
}
```

### 订阅方式

1. **精确匹配**：订阅特定事件类型
   ```rust
   &["DeviceMetric"]  // 只接收 DeviceMetric 事件
   ```

2. **前缀匹配**：订阅一类事件
   ```rust
   &["Device"]  // 接收所有 Device* 事件（DeviceMetric, DeviceOnline 等）
   ```

3. **通配符**：订阅所有事件
   ```rust
   &["all"]  // 接收所有事件
   ```

4. **不订阅**：不接收任何事件
   ```rust
   &[]  // 默认值，不接收事件
   ```

## 事件处理

扩展通过实现 `handle_event()` 方法处理接收到的事件：

```rust
impl Extension for MyExtension {
    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        match event_type {
            "DeviceMetric" => {
                // 处理设备指标事件
                let device_id = payload["payload"]["device_id"].as_str().unwrap_or("");
                let metric = payload["payload"]["metric"].as_str().unwrap_or("");
                let value = &payload["payload"]["value"];

                tracing::info!(
                    device_id = %device_id,
                    metric = %metric,
                    value = ?value,
                    "Received device metric event"
                );

                // 根据事件执行相应操作
                if metric == "temperature" && value.as_f64().unwrap_or(0.0) > 30.0 {
                    // 温度过高，触发告警
                    self.trigger_alert(device_id, "Temperature too high");
                }
            }
            "AgentExecutionStarted" => {
                // 处理 Agent 执行开始事件
                let agent_id = payload["payload"]["agent_id"].as_str().unwrap_or("");
                tracing::info!(agent_id = %agent_id, "Agent execution started");
            }
            _ => {
                // 处理其他事件
                tracing::debug!(event_type = %event_type, "Received event");
            }
        }
        Ok(())
    }
}
```

## 事件格式

所有事件都使用统一的 JSON 格式：

```json
{
  "event_type": "DeviceMetric",
  "payload": {
    "device_id": "sensor-1",
    "metric": "temperature",
    "value": 25.5,
    "timestamp": 1234567890,
    "quality": 0.95
  },
  "timestamp": 1234567890
}
```

### 字段说明

- `event_type`: 事件类型名称（如 "DeviceMetric", "AgentExecutionStarted"）
- `payload`: 事件数据，包含事件的具体信息
- `timestamp`: 事件时间戳（毫秒）

## 支持的事件类型

所有 `NeoMindEvent` 类型都自动支持，包括：

### 设备事件
- `DeviceMetric` - 设备指标更新
- `DeviceOnline` - 设备上线
- `DeviceOffline` - 设备离线
- `DeviceCommandResult` - 设备命令执行结果

### 规则事件
- `RuleEvaluated` - 规则条件评估
- `RuleTriggered` - 规则触发
- `RuleExecuted` - 规则执行完成

### 工作流事件
- `WorkflowTriggered` - 工作流触发
- `WorkflowStepCompleted` - 工作流步骤完成
- `WorkflowCompleted` - 工作流完成

### 告警/消息事件
- `AlertCreated` - 告警创建
- `AlertAcknowledged` - 告警确认
- `MessageCreated` - 消息创建
- `MessageAcknowledged` - 消息确认
- `MessageResolved` - 消息解决

### Agent 事件
- `AgentExecutionStarted` - Agent 执行开始
- `AgentThinking` - Agent 思考过程
- `AgentDecision` - Agent 决策
- `AgentProgress` - Agent 执行进度
- `AgentExecutionCompleted` - Agent 执行完成
- `AgentMemoryUpdated` - Agent 记忆更新

### LLM 事件
- `LlmDecisionProposed` - LLM 决策提议
- `LlmDecisionExecuted` - LLM 决策执行

### 工具执行事件
- `ToolExecutionStart` - 工具执行开始
- `ToolExecutionSuccess` - 工具执行成功
- `ToolExecutionFailure` - 工具执行失败

### 扩展事件
- `ExtensionOutput` - 扩展输出更新
- `ExtensionLifecycle` - 扩展生命周期状态变化
- `ExtensionCommandStarted` - 扩展命令开始执行
- `ExtensionCommandCompleted` - 扩展命令执行完成
- `ExtensionCommandFailed` - 扩展命令执行失败

### 用户事件
- `UserMessage` - 用户消息
- `LlmResponse` - LLM 响应

### 自定义事件
- 任何自定义事件类型（通过 `Custom` 事件）

## 完整示例

```rust
use neomind_extension_sdk::prelude::*;
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};

pub struct EventMonitoringExtension {
    alert_count: AtomicI64,
}

impl EventMonitoringExtension {
    pub fn new() -> Self {
        Self {
            alert_count: AtomicI64::new(0),
        }
    }

    fn trigger_alert(&self, device_id: &str, message: &str) {
        let count = self.alert_count.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::warn!(
            device_id = %device_id,
            message = %message,
            alert_count = count,
            "Alert triggered"
        );
    }
}

impl Extension for EventMonitoringExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static_metadata!("event-monitor", "Event Monitor", "1.0.0")
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static_metrics!(
            MetricDescriptor::new("alert_count", "Alert Count", MetricDataType::Integer)
        )
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static_commands!(
            CommandBuilder::new("get_alert_count")
                .display_name("Get Alert Count")
                .llm_hints("Get the total number of alerts triggered")
                .build()
        )
    }

    fn event_subscriptions(&self) -> &[&str] {
        // 订阅所有设备事件和 Agent 事件
        &["Device", "Agent"]
    }

    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        match event_type {
            // 处理所有设备事件
            evt if evt.starts_with("Device") => {
                let device_id = payload["payload"]["device_id"]
                    .as_str()
                    .unwrap_or("unknown");

                match event_type {
                    "DeviceMetric" => {
                        let metric = payload["payload"]["metric"]
                            .as_str()
                            .unwrap_or("");
                        let value = payload["payload"]["value"];

                        // 检查温度
                        if metric == "temperature" {
                            if let Some(temp) = value.as_f64() {
                                if temp > 30.0 {
                                    self.trigger_alert(device_id, "Temperature too high");
                                }
                            }
                        }
                    }
                    "DeviceOffline" => {
                        let reason = payload["payload"]["reason"]
                            .as_str()
                            .unwrap_or("unknown");
                        self.trigger_alert(device_id, &format!("Device offline: {}", reason));
                    }
                    _ => {
                        tracing::debug!(
                            device_id = %device_id,
                            event_type = %event_type,
                            "Received device event"
                        );
                    }
                }
            }
            // 处理所有 Agent 事件
            evt if evt.starts_with("Agent") => {
                let agent_id = payload["payload"]["agent_id"]
                    .as_str()
                    .unwrap_or("unknown");

                match event_type {
                    "AgentExecutionFailed" => {
                        let error = payload["payload"]["error"]
                            .as_str()
                            .unwrap_or("unknown");
                        self.trigger_alert(agent_id, &format!("Agent failed: {}", error));
                    }
                    _ => {
                        tracing::debug!(
                            agent_id = %agent_id,
                            event_type = %event_type,
                            "Received agent event"
                        );
                    }
                }
            }
            _ => {
                tracing::debug!(event_type = %event_type, "Received event");
            }
        }
        Ok(())
    }

    async fn execute_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "get_alert_count" => {
                let count = self.alert_count.load(Ordering::SeqCst);
                Ok(json!({ "alert_count": count }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let count = self.alert_count.load(Ordering::SeqCst);
        Ok(vec![
            ExtensionMetricValue::new("alert_count", count.into())
        ])
    }
}

neomind_export!(EventMonitoringExtension);
```

## 最佳实践

1. **选择性订阅**：只订阅需要的事件，避免不必要的处理
2. **错误处理**：在 `handle_event` 中妥善处理错误，避免影响其他扩展
3. **性能考虑**：事件处理应该快速完成，避免阻塞
4. **日志记录**：使用适当的日志级别记录事件处理情况
5. **状态管理**：使用线程安全的数据结构管理扩展状态

## 注意事项

1. 事件处理是同步的，不应使用 `.await`
2. 事件处理失败不会影响其他扩展
3. 事件按订阅顺序分发，但处理是并行的
4. 新增事件类型自动支持，无需修改代码