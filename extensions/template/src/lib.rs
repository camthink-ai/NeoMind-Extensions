//! NeoMind Template Extension
//!
//! This is a minimal template extension that you can copy and modify
//! to create your own extensions for the NeoMind Edge AI Platform.
//!
//! ## Steps to Create Your Extension
//!
//! 1. Copy this entire `template/` folder to a new name (e.g., `my-extension/`)
//! 2. Update `Cargo.toml`: Change the `name` and `description` fields
//! 3. Update this file (`src/lib.rs`): Implement your extension logic
//! 4. Update the extension ID, name, and metadata in the code below
//! 5. Build and test: `cargo build --release`

use std::sync::Arc;
use std::time::SystemTime;

use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, MetricDescriptor, ExtensionCommand,
    ExtensionMetricValue, ParamMetricValue, MetricDataType, ParameterDefinition,
    ABI_VERSION, Result,
};
use serde_json::Value;
use once_cell::sync::Lazy;

// ============================================================================
// Extension State
// ============================================================================

/// Your extension's internal state
struct TemplateState {
    config_value: String,
    // Collection strategy: interval in seconds (0 = always produce new data)
    collection_interval_seconds: u64,
    last_collection_timestamp: Arc<std::sync::Mutex<i64>>,
    // Cached metric value
    cached_value: Arc<std::sync::Mutex<f64>>,
}

// ============================================================================
// Static Metrics and Commands
// ============================================================================

/// Static metric descriptors - defined once to avoid lifetime issues
///
/// IMPORTANT: Use `once_cell::sync::Lazy` for static data that will be
/// returned as references. This prevents lifetime errors.
static METRICS: Lazy<[MetricDescriptor; 1]> = Lazy::new(|| [
    MetricDescriptor {
        name: "example_metric".to_string(),
        display_name: "Example Metric".to_string(),
        data_type: MetricDataType::Float,
        unit: "units".to_string(),
        min: Some(0.0),
        max: Some(100.0),
        required: false,
    },
]);

/// Static command descriptors - defined once to avoid lifetime issues
static COMMANDS: Lazy<[ExtensionCommand; 1]> = Lazy::new(|| [
    ExtensionCommand {
        name: "example_command".to_string(),
        display_name: "Example Command".to_string(),
        payload_template: r#"{"input": "{{input}}"}"#.to_string(),
        parameters: vec![],
        fixed_values: Default::default(),
        samples: vec![],
        llm_hints: "This is an example command that echoes back the input.".to_string(),
        parameter_groups: vec![],
    },
]);

// ============================================================================
// Extension Implementation
// ============================================================================

struct TemplateExtension {
    metadata: ExtensionMetadata,
    state: Arc<TemplateState>,
}

impl TemplateExtension {
    /// Check if enough time has passed to collect fresh metrics
    fn should_collect_metrics(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let mut last = self.state.last_collection_timestamp.lock().unwrap();
        let should_update = (now - *last) >= self.state.collection_interval_seconds as i64;
        if should_update {
            *last = now;
        }
        should_update
    }

    /// Generate fresh metric values (simulated data)
    fn generate_fresh_metrics(&self) -> Vec<ExtensionMetricValue> {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Simulate a value that changes over time
        let fresh_value = 42.0 + (timestamp % 1000) as f64 / 100.0;

        // Update cached value
        *self.state.cached_value.lock().unwrap() = fresh_value;

        vec![
            ExtensionMetricValue {
                name: "example_metric".to_string(),
                value: ParamMetricValue::Float(fresh_value),
                timestamp,
            },
        ]
    }

    fn new(config: &Value) -> Result<Self> {
        // Parse configuration
        let config_value = config
            .get("config_value")
            .and_then(|v| v.as_str())
            .unwrap_or("default_value")
            .to_string();

        // Create metadata
        // TODO: Change these values for your extension
        let metadata = ExtensionMetadata {
            id: "com.example.template".to_string(),
            name: "Template Extension".to_string(),
            version: semver::Version::new(0, 5, 8),
            description: Some("A template extension for NeoMind".to_string()),
            author: Some("CamThink".to_string()),
            homepage: Some("https://github.com/yourusername/extensions".to_string()),
            license: Some("MIT".to_string()),
            file_path: None,
            config_parameters: Some(vec![
                ParameterDefinition {
                    name: "config_value".to_string(),
                    display_name: "Config Value".to_string(),
                    description: "A sample configuration value for the template extension".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("default_value".to_string())),
                    min: None,
                    max: None,
                    options: vec![],
                },
                ParameterDefinition {
                    name: "enable_logging".to_string(),
                    display_name: "Enable Logging".to_string(),
                    description: "Whether to log extension activities".to_string(),
                    param_type: MetricDataType::Boolean,
                    required: false,
                    default_value: Some(ParamMetricValue::Boolean(false)),
                    min: None,
                    max: None,
                    options: vec![],
                },
                ParameterDefinition {
                    name: "collection_interval_seconds".to_string(),
                    display_name: "Collection Interval (seconds)".to_string(),
                    description: "How often to generate fresh metric values. Between collections, cached values are returned.".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(60)),
                    min: Some(10.0),
                    max: Some(86400.0),
                    options: vec![],
                },
            ]),
        };

        // Parse collection interval from config (default: 60 seconds)
    let collection_interval_seconds = config
        .get("collection_interval_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(60);

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let state = Arc::new(TemplateState {
        config_value,
        collection_interval_seconds,
        last_collection_timestamp: Arc::new(std::sync::Mutex::new(now)),
        cached_value: Arc::new(std::sync::Mutex::new(42.0)),
    });

    Ok(Self { metadata, state })
    }
}

#[async_trait::async_trait]
impl Extension for TemplateExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        &*METRICS
    }

    fn commands(&self) -> &[ExtensionCommand] {
        &*COMMANDS
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "example_command" => {
                let input = args.get("input")
                    .and_then(|v| v.as_str())
                    .unwrap_or("no input");

                // TODO: Implement your command logic here
                Ok(serde_json::json!({
                    "message": format!("You said: {}", input),
                    "config_value": self.state.config_value,
                    "timestamp": SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // Check if we should collect fresh data
        if self.should_collect_metrics() {
            return Ok(self.generate_fresh_metrics());
        }

        // Return cached value (from previous collection)
        let cached = *self.state.cached_value.lock().unwrap();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        Ok(vec![
            ExtensionMetricValue {
                name: "example_metric".to_string(),
                value: ParamMetricValue::Float(cached),
                timestamp,
            },
        ])
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

use tokio::sync::RwLock;

/// ABI version - must return 2 for V2 API
#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    ABI_VERSION
}

/// Extension metadata (C-compatible)
#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> neomind_core::extension::system::CExtensionMetadata {
    use std::ffi::CStr;

    // TODO: Update these values for your extension
    // Use static CStr references to avoid dangling pointers
    let id = CStr::from_bytes_with_nul(b"com.example.template\0").unwrap();
    let name = CStr::from_bytes_with_nul(b"Template Extension\0").unwrap();
    let version = CStr::from_bytes_with_nul(b"0.5.8\\0").unwrap();
    let description = CStr::from_bytes_with_nul(b"A template extension for NeoMind\0").unwrap();
    let author = CStr::from_bytes_with_nul(b"CamThink\0").unwrap();

    neomind_core::extension::system::CExtensionMetadata {
        abi_version: ABI_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        version: version.as_ptr(),
        description: description.as_ptr(),
        author: author.as_ptr(),
        metric_count: 1,   // Update this to match your METRICS array length
        command_count: 1,  // Update this to match your COMMANDS array length
    }
}

/// Create extension instance
#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Extension>> {
    // Parse config
    let config = if config_json.is_null() || config_len == 0 {
        serde_json::json!({})
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(config_json, config_len);
            let s = std::str::from_utf8_unchecked(slice);
            serde_json::from_str(s).unwrap_or(serde_json::json!({}))
        }
    };

    // Create extension
    match TemplateExtension::new(&config) {
        Ok(ext) => {
            let boxed: Box<dyn Extension> = Box::new(ext);
            let wrapped = Box::new(RwLock::new(boxed));
            Box::into_raw(wrapped)
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy extension instance
#[no_mangle]
pub extern "C" fn neomind_extension_destroy(instance: *mut RwLock<Box<dyn Extension>>) {
    if !instance.is_null() {
        unsafe {
            let _ = Box::from_raw(instance);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extension_creation() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        assert_eq!(ext.metadata().id, "com.example.template");
    }

    #[test]
    fn test_metrics_count() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        assert_eq!(ext.metrics().len(), 1);
    }

    #[test]
    fn test_commands_count() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        assert_eq!(ext.commands().len(), 1);
    }

    #[tokio::test]
    async fn test_command_execution() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let result = ext.execute_command(
            "example_command",
            &json!({"input": "Hello World"})
        ).await;
        assert!(result.is_ok());
        let result_value = result.unwrap();
        assert!(result_value["message"].as_str().unwrap().contains("Hello World"));
    }

    #[test]
    fn test_metrics_production() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let metrics = ext.produce_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "example_metric");
    }

    #[tokio::test]
    async fn test_health_check() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let healthy = ext.health_check().await.unwrap();
        assert!(healthy);
    }

    #[test]
    fn test_extension_with_config() {
        let config = json!({"config_value": "test_value"});
        let ext = TemplateExtension::new(&config).unwrap();
        // The config value is stored in state, accessible during command execution
        assert_eq!(ext.state.config_value, "test_value");
    }

    #[test]
    fn test_metadata_fields() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        assert_eq!(ext.metadata().id, "com.example.template");
        assert_eq!(ext.metadata().name, "Template Extension");
        assert_eq!(ext.metadata().version, semver::Version::new(0, 3, 0));
        assert!(ext.metadata().description.is_some());
        assert!(ext.metadata().author.is_some());
    }

    #[test]
    fn test_metric_descriptor() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let metrics = ext.metrics();
        assert_eq!(metrics[0].name, "example_metric");
        assert_eq!(metrics[0].display_name, "Example Metric");
        assert_eq!(metrics[0].data_type, MetricDataType::Float);
        assert_eq!(metrics[0].unit, "units");
        assert_eq!(metrics[0].min, Some(0.0));
        assert_eq!(metrics[0].max, Some(100.0));
    }

    #[test]
    fn test_command_descriptor() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let commands = ext.commands();
        assert_eq!(commands[0].name, "example_command");
        assert_eq!(commands[0].display_name, "Example Command");
        assert!(!commands[0].payload_template.is_empty());
        assert!(!commands[0].llm_hints.is_empty());
    }

    #[tokio::test]
    async fn test_command_execution_with_config() {
        let config = json!({"config_value": "custom_value"});
        let ext = TemplateExtension::new(&config).unwrap();
        let result = ext.execute_command(
            "example_command",
            &json!({"input": "test"})
        ).await;
        assert!(result.is_ok());
        let result_value = result.unwrap();
        assert_eq!(result_value["config_value"], "custom_value");
    }

    #[tokio::test]
    async fn test_command_execution_default_input() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let result = ext.execute_command(
            "example_command",
            &json!({})
        ).await;
        assert!(result.is_ok());
        let result_value = result.unwrap();
        // Should contain "no input" when no input is provided
        assert!(result_value["message"].as_str().unwrap().contains("no input"));
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let result = ext.execute_command("unknown_command", &json!({})).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ExtensionError::CommandNotFound(cmd) => assert_eq!(cmd, "unknown_command"),
            _ => panic!("Expected CommandNotFound error"),
        }
    }

    #[test]
    fn test_metric_value_type() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let metrics = ext.produce_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert!(matches!(metrics[0].value, ParamMetricValue::Float(_)));
    }

    #[test]
    fn test_metric_has_timestamp() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        let metrics = ext.produce_metrics().unwrap();
        assert!(metrics[0].timestamp > 0);
    }

    #[tokio::test]
    async fn test_configure_method() {
        let mut ext = TemplateExtension::new(&json!({})).unwrap();
        let new_config = json!({"config_value": "updated"});
        let result = ext.configure(&new_config).await;
        assert!(result.is_ok());
        // Note: configure implementation would need to update state
        // This tests the interface exists and accepts the config
    }

    #[test]
    fn test_extension_with_empty_config() {
        let ext = TemplateExtension::new(&json!({})).unwrap();
        assert_eq!(ext.state.config_value, "default_value");
    }

    #[test]
    fn test_extension_with_complex_config() {
        let config = json!({
            "config_value": "complex",
            "extra_field": "ignored",
            "number_field": 42
        });
        let ext = TemplateExtension::new(&config).unwrap();
        // Should pick up config_value, ignore others
        assert_eq!(ext.state.config_value, "complex");
    }
}
