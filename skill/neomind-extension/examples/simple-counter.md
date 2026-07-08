# Example: Simple Counter Extension

A minimal NeoMind extension demonstrating the canonical patterns: builder API,
`OnceLock`-cached metadata, atomic counter, proper error handling.

## Features

- One command: `increment` (with optional `amount` 1–100)
- One metric: `counter`
- No external dependencies
- Process-isolated execution

## Cargo.toml

```toml
[package]
name = "simple-counter-v2"
version = "2.0.0"
edition = "2021"

[lib]
name = "neomind_extension_simple_counter_v2"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }     # preserve_order feature is REQUIRED (ABI compat)
async-trait = "0.1"
parking_lot = "0.12"                  # for sync locks (not strictly needed here)
tokio = { version = "1", features = ["rt", "sync"] }
chrono = "0.4"
```

> `[profile.release]` with `panic = "unwind"` lives in the **workspace root**
> `NeoMind-Extensions/Cargo.toml`, not the member crate.

## src/lib.rs

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use neomind_extension_sdk::{CommandBuilder, MetricBuilder, ParamBuilder, metric_int};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
        Self { counter: AtomicI64::new(0) }
    }
}

impl Default for SimpleCounterExtension {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Extension for SimpleCounterExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("simple-counter-v2", "Simple Counter", "2.0.0")
                .with_description("A simple counter extension for demonstration")
                .with_author("NeoMind Team")
                .with_license("MIT")
        })
    }

    // Owned Vec — NOT &[MetricDescriptor]
    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("counter", "Counter Value")
                .integer()
                .min(0.0)
                .required()
                .build(),
        ]
    }

    // Owned Vec — NOT &[ExtensionCommand]
    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("increment")
                .display_name("Increment Counter")
                .description("Increment the counter by the specified amount (default: 1)")
                .param(
                    ParamBuilder::new("amount", MetricDataType::Integer)
                        .display_name("Amount")
                        .description("Amount to increment by (1–100)")
                        .optional()
                        .default(ParamMetricValue::Integer(1))
                        .min(1.0)
                        .max(100.0)
                        .build(),
                )
                .sample(json!({ "amount": 1 }))
                .sample(json!({ "amount": 5 }))
                .sample(json!({ "amount": 10 }))
                .build(),
        ]
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "increment" => {
                let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(1);

                if !(1..=100).contains(&amount) {
                    return Err(ExtensionError::InvalidArguments(
                        "Amount must be between 1 and 100".into(),
                    ));
                }

                let old_value = self.counter.fetch_add(amount, Ordering::SeqCst);
                let new_value = old_value + amount;

                Ok(serde_json::to_value(IncrementResult {
                    old_value,
                    new_value,
                    increment_amount: amount,
                })
                .map_err(|e| ExtensionError::Json(e.to_string()))?)
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // metric_int! auto-fills the timestamp
        Ok(vec![
            metric_int!("counter", self.counter.load(Ordering::SeqCst)),
        ])
    }
}

// One line — generates all ABI v3 FFI exports.
neomind_extension_sdk::neomind_export!(SimpleCounterExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_increment() {
        let ext = SimpleCounterExtension::new();
        let r = futures::executor::block_on(
            ext.execute_command("increment", &json!({ "amount": 5 })),
        ).unwrap();
        assert_eq!(r["new_value"], 5);
        assert_eq!(r["increment_amount"], 5);
    }

    #[test]
    fn test_default_amount() {
        let ext = SimpleCounterExtension::new();
        let r = futures::executor::block_on(
            ext.execute_command("increment", &json!({})),
        ).unwrap();
        assert_eq!(r["new_value"], 1);
    }

    #[test]
    fn test_invalid_amount() {
        let ext = SimpleCounterExtension::new();
        let r = futures::executor::block_on(
            ext.execute_command("increment", &json!({ "amount": 999 })),
        );
        assert!(matches!(r, Err(ExtensionError::InvalidArguments(_))));
    }

    #[test]
    fn test_produce_metrics() {
        let ext = SimpleCounterExtension::new();
        let _ = futures::executor::block_on(
            ext.execute_command("increment", &json!({ "amount": 7 })),
        );
        let m = ext.produce_metrics().unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].name, "counter");
    }
}
```

## Build & install

```bash
# Dev build + auto-install (preferred):
./build.sh --dev --single simple-counter-v2

# Or manual:
cargo build --release -p simple-counter-v2
cp target/release/libneomind_extension_simple_counter_v2.dylib \
   ~/.neomind/extensions/
```

## Test

```bash
curl -X POST http://localhost:9375/api/extensions/simple-counter-v2/command \
  -H "Content-Type: application/json" \
  -d '{"command": "increment", "args": {"amount": 5}}'

# → { "success": true, "data": { "old_value": 0, "new_value": 5, "increment_amount": 5 } }

curl http://localhost:9375/api/extensions/simple-counter-v2/metrics
# → { "metrics": [{ "name": "counter", "value": 5, "timestamp": 1709481600000 }] }
```

## Unit tests

```bash
cargo test -p simple-counter-v2
```

## Concepts demonstrated

1. **Builders** — `MetricBuilder` / `CommandBuilder` / `ParamBuilder` (preferred over raw struct literals)
2. **`OnceLock`-cached metadata** — return `&ExtensionMetadata` from a static
3. **Owned Vec returns** — `metrics()` and `commands()` return `Vec<...>`, not slices
4. **Atomic counter** — `AtomicI64` for thread-safe state
5. **`metric_int!` macro** — auto-fills timestamp
6. **Proper error types** — `ExtensionError::InvalidArguments` / `CommandNotFound` / `Json`
7. **`neomind_export!`** — one line generates all ABI v3 FFI symbols

## Extension points

1. Add `reset` / `decrement` commands
2. Add a `history` metric (last N increments)
3. Add a frontend card (see `reference/frontend.md`)
4. Add persistence (save counter to disk on `stop()`)
5. Subscribe to events (see `reference/event-subscription.md`)
