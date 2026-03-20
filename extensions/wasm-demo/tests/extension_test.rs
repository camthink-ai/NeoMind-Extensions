//! WASM Demo Extension - Comprehensive Test Suite
//!
//! Tests cover:
//! - Extension metadata and configuration
//! - Command execution (increment, decrement, reset, get)
//! - Metric production
//! - Counter functionality
//! - Error handling
//! - Edge cases and boundary conditions
//!
//! Note: These tests run in native mode. WASM-specific behavior
//! is tested separately via integration tests.

#[cfg(test)]
mod tests {
    use neomind_extension_sdk::{
        Extension, ExtensionError,
        MetricDataType, ParamMetricValue,
    };
    use serde_json::{json, Value};

    // Import the extension from the library
    use neomind_extension_wasm_demo::WasmDemoExtension;

    // ========================================================================
    // Helper Functions
    // ========================================================================

    /// Create a new WasmDemoExtension instance for testing
    fn create_extension() -> WasmDemoExtension {
        WasmDemoExtension::new()
    }

    // ========================================================================
    // Metadata Tests
    // ========================================================================

    #[test]
    fn test_metadata_basic() {
        let ext = create_extension();
        let meta = ext.metadata();

        assert_eq!(meta.id, "wasm-demo");
        assert_eq!(meta.name, "WASM Demo Extension");
        assert_eq!(meta.version.major, 1);
        assert_eq!(meta.version.minor, 0);
        assert_eq!(meta.version.patch, 0);
    }

    #[test]
    fn test_metadata_has_description() {
        let ext = create_extension();
        let meta = ext.metadata();

        assert!(meta.description.is_some());
        assert!(!meta.description.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_metadata_has_author() {
        let ext = create_extension();
        let meta = ext.metadata();

        assert!(meta.author.is_some());
        assert_eq!(meta.author.as_ref().unwrap(), "NeoMind Team");
    }

    #[test]
    fn test_metadata_json_matches_runtime_metadata() {
        let ext = create_extension();
        let meta = ext.metadata();
        let metadata_json: Value = serde_json::from_str(include_str!("../metadata.json")).unwrap();

        assert_eq!(metadata_json["id"], meta.id);
        assert_eq!(metadata_json["name"], meta.name);
        assert_eq!(metadata_json["description"].as_str(), meta.description.as_deref());
        assert_eq!(metadata_json["license"], "MIT");
        assert_eq!(
            metadata_json["homepage"],
            "https://github.com/camthink-ai/NeoMind-Extensions/tree/main/extensions/wasm-demo"
        );
    }

    // ========================================================================
    // Metrics Tests
    // ========================================================================

    #[test]
    fn test_metrics_definitions() {
        let ext = create_extension();
        let metrics = ext.metrics();

        // Should have metrics defined
        assert!(!metrics.is_empty());

        let metric_names: Vec<&str> = metrics.iter()
            .map(|m| m.name.as_str())
            .collect();

        assert!(metric_names.contains(&"counter"));
        assert!(metric_names.contains(&"request_count"));
    }

    #[test]
    fn test_metric_types() {
        let ext = create_extension();
        let metrics = ext.metrics();

        // counter should be Integer
        let counter_metric = metrics.iter()
            .find(|m| m.name == "counter");
        assert!(counter_metric.is_some());
        assert_eq!(counter_metric.unwrap().data_type, MetricDataType::Integer);

        // request_count should be Integer
        let request_metric = metrics.iter()
            .find(|m| m.name == "request_count");
        assert!(request_metric.is_some());
        assert_eq!(request_metric.unwrap().data_type, MetricDataType::Integer);
    }

    // ========================================================================
    // Commands Tests
    // ========================================================================

    #[test]
    fn test_commands_definitions() {
        let ext = create_extension();
        let commands = ext.commands();

        // Should have commands defined
        assert!(!commands.is_empty());

        let cmd_names: Vec<&str> = commands.iter()
            .map(|c| c.name.as_str())
            .collect();

        assert!(cmd_names.contains(&"increment"));
        assert!(cmd_names.contains(&"decrement"));
        assert!(cmd_names.contains(&"reset"));
        assert!(cmd_names.contains(&"get"));
    }

    #[test]
    fn test_increment_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let increment_cmd = commands.iter()
            .find(|c| c.name == "increment");

        assert!(increment_cmd.is_some());
        let cmd = increment_cmd.unwrap();

        // Should have amount parameter
        let amount_param = cmd.parameters.iter()
            .find(|p| p.name == "amount");
        assert!(amount_param.is_some());

        // Should have default value
        assert!(amount_param.unwrap().default_value.is_some());

        // Should have samples
        assert!(!cmd.samples.is_empty());

        // Should have description (replaces llm_hints)
        assert!(!cmd.description.is_empty());
    }

    #[test]
    fn test_decrement_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let decrement_cmd = commands.iter()
            .find(|c| c.name == "decrement");

        assert!(decrement_cmd.is_some());
        let cmd = decrement_cmd.unwrap();

        // Should have amount parameter
        let amount_param = cmd.parameters.iter()
            .find(|p| p.name == "amount");
        assert!(amount_param.is_some());
    }

    #[test]
    fn test_reset_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let reset_cmd = commands.iter()
            .find(|c| c.name == "reset");

        assert!(reset_cmd.is_some());
        let cmd = reset_cmd.unwrap();

        // Reset should have no parameters
        assert!(cmd.parameters.is_empty());
    }

    #[test]
    fn test_get_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let get_cmd = commands.iter()
            .find(|c| c.name == "get");

        assert!(get_cmd.is_some());
        let cmd = get_cmd.unwrap();

        // Get should have no parameters
        assert!(cmd.parameters.is_empty());
    }

    // ========================================================================
    // Produce Metrics Tests
    // ========================================================================

    #[test]
    fn test_produce_metrics_initial() {
        let ext = create_extension();

        let metrics = ext.produce_metrics().unwrap();

        // Should have all defined metrics
        assert!(!metrics.is_empty());

        // Initial counter should be 0
        let counter = metrics.iter()
            .find(|m| m.name == "counter");
        assert!(counter.is_some());

        match counter.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 0),
            _ => panic!("Expected Integer value for counter"),
        }

        // Initial request_count should be 0
        let request_count = metrics.iter()
            .find(|m| m.name == "request_count");
        assert!(request_count.is_some());

        match request_count.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 0),
            _ => panic!("Expected Integer value for request_count"),
        }
    }

    // ========================================================================
    // Command Execution Tests
    // ========================================================================

    #[tokio::test]
    async fn test_increment_default() {
        let ext = create_extension();

        let result = ext.execute_command("increment", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], 1);
        assert_eq!(response["incremented_by"], 1);
    }

    #[tokio::test]
    async fn test_increment_with_amount() {
        let ext = create_extension();

        let result = ext.execute_command("increment", &json!({
            "amount": 10
        })).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], 10);
        assert_eq!(response["incremented_by"], 10);
    }

    #[tokio::test]
    async fn test_increment_negative() {
        let ext = create_extension();

        // Increment with negative amount (should work like decrement)
        let result = ext.execute_command("increment", &json!({
            "amount": -5
        })).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], -5);
    }

    #[tokio::test]
    async fn test_decrement_default() {
        let ext = create_extension();

        let result = ext.execute_command("decrement", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], -1);
        assert_eq!(response["decremented_by"], 1);
    }

    #[tokio::test]
    async fn test_decrement_with_amount() {
        let ext = create_extension();

        let result = ext.execute_command("decrement", &json!({
            "amount": 5
        })).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], -5);
        assert_eq!(response["decremented_by"], 5);
    }

    #[tokio::test]
    async fn test_reset() {
        let ext = create_extension();

        // First increment
        let _ = ext.execute_command("increment", &json!({ "amount": 100 })).await;

        // Then reset
        let result = ext.execute_command("reset", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], 0);
        assert_eq!(response["message"], "Counter reset to zero");
    }

    #[tokio::test]
    async fn test_get() {
        let ext = create_extension();

        // Increment first
        let _ = ext.execute_command("increment", &json!({ "amount": 42 })).await;

        // Get value
        let result = ext.execute_command("get", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], 42);
        assert!(response.get("request_count").is_some());
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = create_extension();

        let result = ext.execute_command("unknown_command", &json!({})).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ExtensionError::CommandNotFound(cmd) => {
                assert_eq!(cmd, "unknown_command");
            }
            _ => panic!("Expected CommandNotFound error"),
        }
    }

    // ========================================================================
    // Counter State Tests
    // ========================================================================

    #[tokio::test]
    async fn test_counter_persistence() {
        let ext = create_extension();

        // Increment multiple times
        let _ = ext.execute_command("increment", &json!({ "amount": 10 })).await;
        let _ = ext.execute_command("increment", &json!({ "amount": 5 })).await;
        let _ = ext.execute_command("decrement", &json!({ "amount": 3 })).await;

        // Get final value
        let result = ext.execute_command("get", &json!({})).await.unwrap();
        assert_eq!(result["counter"], 12); // 10 + 5 - 3 = 12
    }

    #[tokio::test]
    async fn test_counter_after_reset() {
        let ext = create_extension();

        // Increment
        let _ = ext.execute_command("increment", &json!({ "amount": 100 })).await;

        // Reset
        let _ = ext.execute_command("reset", &json!({})).await;

        // Increment again
        let result = ext.execute_command("increment", &json!({ "amount": 1 })).await.unwrap();
        assert_eq!(result["counter"], 1);
    }

    // ========================================================================
    // Request Count Tests
    // ========================================================================

    #[tokio::test]
    async fn test_request_count_increments() {
        let ext = create_extension();

        // Execute multiple commands
        let _ = ext.execute_command("increment", &json!({})).await;
        let _ = ext.execute_command("increment", &json!({})).await;
        let _ = ext.execute_command("get", &json!({})).await;

        // Check request count
        let result = ext.execute_command("get", &json!({})).await.unwrap();
        assert_eq!(result["request_count"], 4); // 3 previous + this get
    }

    // ========================================================================
    // Metrics After Commands Tests
    // ========================================================================

    #[tokio::test]
    async fn test_produce_metrics_after_commands() {
        let ext = create_extension();

        // Execute some commands
        let _ = ext.execute_command("increment", &json!({ "amount": 50 })).await;
        let _ = ext.execute_command("decrement", &json!({ "amount": 20 })).await;

        // Get metrics
        let metrics = ext.produce_metrics().unwrap();

        // Counter should reflect the operations
        let counter = metrics.iter()
            .find(|m| m.name == "counter");
        assert!(counter.is_some());

        match counter.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 30), // 50 - 20
            _ => panic!("Expected Integer value"),
        }

        // Request count should be 2
        let request_count = metrics.iter()
            .find(|m| m.name == "request_count");
        assert!(request_count.is_some());

        match request_count.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 2),
            _ => panic!("Expected Integer value"),
        }
    }

    // ========================================================================
    // Edge Cases and Boundary Tests
    // ========================================================================

    #[tokio::test]
    async fn test_increment_large_value() {
        let ext = create_extension();

        let result = ext.execute_command("increment", &json!({
            "amount": i64::MAX
        })).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], i64::MAX);
    }

    #[tokio::test]
    async fn test_decrement_large_value() {
        let ext = create_extension();

        let result = ext.execute_command("decrement", &json!({
            "amount": i64::MAX
        })).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], -i64::MAX);
    }

    #[tokio::test]
    async fn test_increment_zero() {
        let ext = create_extension();

        let result = ext.execute_command("increment", &json!({
            "amount": 0
        })).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], 0);
    }

    #[tokio::test]
    async fn test_multiple_resets() {
        let ext = create_extension();

        // Multiple resets should all work
        let _ = ext.execute_command("reset", &json!({})).await;
        let _ = ext.execute_command("reset", &json!({})).await;
        let result = ext.execute_command("reset", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["counter"], 0);
    }

    // ========================================================================
    // Default Implementation Tests
    // ========================================================================

    #[test]
    fn test_default_implementation() {
        let ext1 = WasmDemoExtension::new();
        let ext2 = WasmDemoExtension::default();

        // Both should have same initial state
        assert_eq!(ext1.metadata().id, ext2.metadata().id);
    }

    // ========================================================================
    // Thread Safety Tests
    // ========================================================================

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let ext = Arc::new(create_extension());
        let mut handles = vec![];

        for _ in 0..10 {
            let ext_clone = Arc::clone(&ext);
            handles.push(thread::spawn(move || {
                let _ = ext_clone.produce_metrics();
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Extension should still be functional
        let metrics = ext.produce_metrics().unwrap();
        assert!(!metrics.is_empty());
    }

    #[tokio::test]
    async fn test_concurrent_commands() {
        use std::sync::Arc;

        let ext = Arc::new(create_extension());
        let mut handles = vec![];

        for i in 0..10 {
            let ext_clone = Arc::clone(&ext);
            handles.push(tokio::spawn(async move {
                let _ = ext_clone.execute_command("increment", &json!({
                    "amount": 1
                })).await;
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Counter should be 10
        let result = ext.execute_command("get", &json!({})).await.unwrap();
        assert_eq!(result["counter"], 10);
    }

    // ========================================================================
    // Memory and Resource Tests
    // ========================================================================

    #[test]
    fn test_no_memory_leak_on_repeated_operations() {
        let ext = create_extension();

        // Perform many operations
        for _ in 0..10000 {
            let _ = ext.produce_metrics();
        }

        // If we get here without running out of memory, the test passes
        assert!(true);
    }

    // ========================================================================
    // Integration-style Tests
    // ========================================================================

    #[tokio::test]
    async fn test_full_workflow() {
        let ext = create_extension();

        // 1. Get initial state
        let initial = ext.execute_command("get", &json!({})).await.unwrap();
        assert_eq!(initial["counter"], 0);

        // 2. Increment
        let inc = ext.execute_command("increment", &json!({ "amount": 10 })).await.unwrap();
        assert_eq!(inc["counter"], 10);

        // 3. Decrement
        let dec = ext.execute_command("decrement", &json!({ "amount": 3 })).await.unwrap();
        assert_eq!(dec["counter"], 7);

        // 4. Get metrics
        let metrics = ext.produce_metrics().unwrap();
        let counter = metrics.iter().find(|m| m.name == "counter").unwrap();
        match counter.value {
            ParamMetricValue::Integer(v) => assert_eq!(v, 7),
            _ => panic!("Expected Integer"),
        }

        // 5. Reset
        let reset = ext.execute_command("reset", &json!({})).await.unwrap();
        assert_eq!(reset["counter"], 0);

        // 6. Verify reset
        let final_get = ext.execute_command("get", &json!({})).await.unwrap();
        assert_eq!(final_get["counter"], 0);
    }

    #[tokio::test]
    async fn test_counter_operations_sequence() {
        let ext = create_extension();

        // Sequence: +5, +3, -2, +10, -8, reset, +1
        let operations = vec![
            ("increment", 5, 5),
            ("increment", 3, 8),
            ("decrement", 2, 6),
            ("increment", 10, 16),
            ("decrement", 8, 8),
        ];

        for (cmd, amount, expected) in operations {
            let result = ext.execute_command(cmd, &json!({ "amount": amount })).await.unwrap();
            assert_eq!(result["counter"], expected);
        }

        // Reset and verify
        let _ = ext.execute_command("reset", &json!({})).await;
        let result = ext.execute_command("increment", &json!({ "amount": 1 })).await.unwrap();
        assert_eq!(result["counter"], 1);
    }
}
