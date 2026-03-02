//! NeoMind WASM Demo Extension
//!
//! A simple counter extension demonstrating the complete WASM extension API.
//! This extension shows how to:
//! - Declare metrics and commands
//! - Implement the Extension trait for WASM target
//! - Export functions using the neomind_export! macro
//!
//! # Features
//!
//! - **Counter metric**: A simple integer counter
//! - **Increment command**: Add a value to the counter
//! - **Reset command**: Reset the counter to zero
//! - **Get command**: Get the current counter value
//!
//! # Building
//!
//! ```bash
//! # Build for WASM target
//! cargo build --target wasm32-unknown-unknown --release
//!
//! # The output will be at:
//! # target/wasm32-unknown-unknown/release/neomind_extension_wasm_demo.wasm
//! ```

use std::sync::atomic::{AtomicI64, Ordering};

use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};

// ============================================================================
// Extension Implementation
// ============================================================================

/// A simple counter extension for demonstrating WASM support
pub struct WasmDemoExtension {
    /// The counter value
    counter: AtomicI64,
    /// Request count
    request_count: AtomicI64,
}

impl WasmDemoExtension {
    /// Create a new instance
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
            request_count: AtomicI64::new(0),
        }
    }
}

impl Default for WasmDemoExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for WasmDemoExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "wasm-demo",
                "WASM Demo Extension",
                "1.0.0",  // Use string version for WASM compatibility
            )
            .with_description("A simple counter extension demonstrating WASM support")
            .with_author("NeoMind Team")
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
                MetricDescriptor {
                    name: "request_count".to_string(),
                    display_name: "Request Count".to_string(),
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
                    description: "Increment the counter by the specified amount".to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to add to the counter".to_string(),
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
                    llm_hints: "Increment the counter by the specified amount".to_string(),
                    parameter_groups: Vec::new(),
                },
                ExtensionCommand {
                    name: "decrement".to_string(),
                    display_name: "Decrement".to_string(),
                    payload_template: String::new(),
                    description: "Decrement the counter by the specified amount".to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to subtract from the counter".to_string(),
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
                    llm_hints: "Decrement the counter by the specified amount".to_string(),
                    parameter_groups: Vec::new(),
                },
                ExtensionCommand {
                    name: "reset".to_string(),
                    display_name: "Reset".to_string(),
                    payload_template: String::new(),
                    description: "Reset the counter to zero".to_string(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![json!({})],
                    llm_hints: "Reset the counter to zero".to_string(),
                    parameter_groups: Vec::new(),
                },
                ExtensionCommand {
                    name: "get".to_string(),
                    display_name: "Get".to_string(),
                    payload_template: String::new(),
                    description: "Get the current counter value".to_string(),
                    parameters: Vec::new(),
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![json!({})],
                    llm_hints: "Get the current counter value".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        self.request_count.fetch_add(1, Ordering::SeqCst);

        match command {
            "increment" => {
                let amount = args.get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let new_value = self.counter.fetch_add(amount, Ordering::SeqCst) + amount;
                Ok(json!({
                    "counter": new_value,
                    "incremented_by": amount
                }))
            }
            "decrement" => {
                let amount = args.get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let new_value = self.counter.fetch_sub(amount, Ordering::SeqCst) - amount;
                Ok(json!({
                    "counter": new_value,
                    "decremented_by": amount
                }))
            }
            "reset" => {
                self.counter.store(0, Ordering::SeqCst);
                Ok(json!({
                    "counter": 0,
                    "message": "Counter reset to zero"
                }))
            }
            "get" => {
                let value = self.counter.load(Ordering::SeqCst);
                Ok(json!({
                    "counter": value,
                    "request_count": self.request_count.load(Ordering::SeqCst)
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // For WASM, use timestamp 0 (host can provide actual timestamp)
        let now = 0i64;
        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "request_count".to_string(),
                value: ParamMetricValue::Integer(self.request_count.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// Export FFI Functions
// ============================================================================

// This single macro exports all necessary FFI functions for both Native and WASM
neomind_extension_sdk::neomind_export!(WasmDemoExtension);