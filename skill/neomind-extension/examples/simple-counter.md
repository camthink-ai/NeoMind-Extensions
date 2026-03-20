# Example: Simple Counter Extension

A minimal NeoMind extension that demonstrates basic functionality.

## Features

- Single command: `increment`
- Single metric: `counter`
- No external dependencies
- Process-isolated execution

## Implementation

### Cargo.toml

```toml
[package]
name = "simple-counter-v2"
version = "2.0.0"
edition = "2021"

[lib]
name = "neomind_extension_simple_counter_v2"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../../../NeoMind/crates/neomind-extension-sdk" }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = "0.1"
tokio = { version = "1", features = ["rt", "sync"] }
semver = "1"
chrono = "0.4"
```

If this extension is part of a Cargo workspace, keep `[profile.release]` in the workspace root `Cargo.toml`.

### src/lib.rs

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementResult {
    pub old_value: i64,
    pub new_value: i64,
    pub increment_amount: i64,
}

pub struct SimpleCounterExtension {
    counter: AtomicI64,
}

impl SimpleCounterExtension {
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl Extension for SimpleCounterExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "simple-counter-v2".to_string(),
                name: "Simple Counter".to_string(),
                version: Version::parse("2.0.0").unwrap(),
                description: Some("A simple counter extension for demonstration".to_string()),
                author: Some("NeoMind Team".to_string()),
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
                    display_name: "Counter Value".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: String::new(),
                    min: None,
                    max: None,
                    required: true,
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
                    display_name: "Increment Counter".to_string(),
                    payload_template: String::new(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "amount".to_string(),
                            display_name: "Amount".to_string(),
                            description: "Amount to increment by".to_string(),
                            param_type: MetricDataType::Integer,
                            required: false,
                            default_value: Some(ParamMetricValue::Integer(1)),
                            min: Some(1.0),
                            max: Some(100.0),
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![
                        json!({ "amount": 1 }),
                        json!({ "amount": 5 }),
                        json!({ "amount": 10 }),
                    ],
                    llm_hints: "Increment the counter by the specified amount (default: 1)".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "increment" => {
                // Extract amount from arguments (default to 1)
                let amount = args
                    .get("amount")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);

                // Validate amount
                if amount < 1 || amount > 100 {
                    return Err(ExtensionError::InvalidArguments(
                        "Amount must be between 1 and 100".to_string(),
                    ));
                }

                // Increment counter atomically
                let old_value = self.counter.fetch_add(amount, Ordering::SeqCst);
                let new_value = old_value + amount;

                // Return result
                let result = IncrementResult {
                    old_value,
                    new_value,
                    increment_amount: amount,
                };

                Ok(serde_json::to_value(result)
                    .map_err(|e| ExtensionError::Other(e.to_string()))?)
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let counter_value = self.counter.load(Ordering::SeqCst);

        Ok(vec![ExtensionMetricValue {
            name: "counter".to_string(),
            value: ParamMetricValue::Integer(counter_value),
            timestamp: now,
        }])
    }
}

// Export FFI
neomind_extension_sdk::neomind_export!(SimpleCounterExtension);
```

## Building

```bash
# From NeoMind-Extension root
cargo build --release -p simple-counter-v2

# Output
ls -lh target/release/libneomind_extension_simple_counter_v2.dylib
```

## Installing

```bash
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_simple_counter_v2.dylib ~/.neomind/extensions/
```

## Testing

```bash
# Using curl
curl -X POST http://localhost:9375/api/extensions/simple-counter-v2/command \
  -H "Content-Type: application/json" \
  -d '{"command": "increment", "args": {"amount": 5}}'

# Expected response
{
  "success": true,
  "data": {
    "old_value": 0,
    "new_value": 5,
    "increment_amount": 5
  }
}

# Get metrics
curl http://localhost:9375/api/extensions/simple-counter-v2/metrics

# Expected response
{
  "metrics": [
    {
      "name": "counter",
      "value": 5,
      "timestamp": 1709481600000
    }
  ]
}
```

## Key Concepts Demonstrated

1. **Atomic Operations**: Using `AtomicI64` for thread-safe counter
2. **Parameter Validation**: Checking argument bounds
3. **Error Handling**: Proper error types and messages
4. **Static Metadata**: Using `OnceLock` for efficient initialization
5. **Typed Results**: Custom result struct with serialization

## Extension Points

To extend this example:

1. **Add Reset Command**: Reset counter to zero
2. **Add Decrement Command**: Decrease counter value
3. **Add History Metric**: Track increment history
4. **Add Frontend**: Display counter with buttons
5. **Add Persistence**: Save counter to disk
