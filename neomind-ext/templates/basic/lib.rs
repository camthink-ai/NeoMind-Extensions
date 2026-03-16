use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

// ============================================================================
// Extension Metadata
// ============================================================================

pub struct {{EXTENSION_STRUCT_NAME}};

impl {{EXTENSION_STRUCT_NAME}} {
    pub fn new() -> Self {
        Self
    }
}

// ============================================================================
// Extension Implementation
// ============================================================================

#[async_trait]
impl Extension for {{EXTENSION_STRUCT_NAME}} {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "{{EXTENSION_ID}}".to_string(),
                name: "{{EXTENSION_NAME}}".to_string(),
                version: Version::parse("1.0.0").unwrap(),
                description: Some("{{EXTENSION_DESCRIPTION}}".to_string()),
                author: Some("{{EXTENSION_AUTHOR}}".to_string()),
                homepage: None,
                license: Some("MIT".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| vec![
            // Add your metrics here
            MetricDescriptor {
                name: "status".to_string(),
                display_name: "Status".to_string(),
                data_type: MetricDataType::String,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
        ])
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| vec![
            // Add your commands here
            ExtensionCommand {
                name: "ping".to_string(),
                display_name: "Ping".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: std::collections::HashMap::new(),
                samples: vec![json!({})],
                llm_hints: "Check if extension is responsive".to_string(),
                parameter_groups: Vec::new(),
            },
        ])
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "ping" => {
                Ok(json!({
                    "status": "ok",
                    "message": "Extension is running",
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        Ok(vec![
            ExtensionMetricValue {
                name: "status".to_string(),
                value: ParamMetricValue::String("running".to_string()),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// FFI Export
// ============================================================================

neomind_extension_sdk::neomind_export!({{EXTENSION_STRUCT_NAME}});
