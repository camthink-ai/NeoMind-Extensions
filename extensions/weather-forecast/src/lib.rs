//! NeoMind Weather Forecast Extension
//!
//! This extension provides weather data for global cities.
//!
//! ## Capabilities
//!
//! - **Metrics**: Temperature, humidity, wind speed, cloud cover
//! - **Commands**: Query weather for any city
//!
//! ## Configuration
//!
//! Set via JSON config when loading:
//! ```json
//! {
//!   "default_city": "Beijing"
//! }
//! ```

use std::sync::Arc;
use std::time::SystemTime;

// Import from neomind-core (the actual extension system)
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, MetricDescriptor, ExtensionCommand,
    ExtensionMetricValue, ParamMetricValue, MetricDataType, ParameterDefinition,
    ParameterGroup, ABI_VERSION, Result,
};
use serde_json::Value;
use once_cell::sync::Lazy;

// ============================================================================
// Extension State
// ============================================================================

struct WeatherState {
    default_city: String,
    // Collection strategy configuration
    collection_interval_seconds: u64,
    last_collection_timestamp: Arc<std::sync::Mutex<i64>>,
    // Cached metric data (for returning between collections)
    cached_metrics: Arc<std::sync::Mutex<Vec<ExtensionMetricValue>>>,
}

// ============================================================================
// Static Metrics and Commands
// ============================================================================

/// Static metric descriptors - defined once to avoid lifetime issues
static METRICS: Lazy<[MetricDescriptor; 4]> = Lazy::new(|| [
    MetricDescriptor {
        name: "temperature_c".to_string(),
        display_name: "Temperature".to_string(),
        data_type: MetricDataType::Float,
        unit: "°C".to_string(),
        min: Some(-50.0),
        max: Some(60.0),
        required: false,
    },
    MetricDescriptor {
        name: "humidity_percent".to_string(),
        display_name: "Humidity".to_string(),
        data_type: MetricDataType::Integer,
        unit: "%".to_string(),
        min: Some(0.0),
        max: Some(100.0),
        required: false,
    },
    MetricDescriptor {
        name: "wind_speed_kmph".to_string(),
        display_name: "Wind Speed".to_string(),
        data_type: MetricDataType::Float,
        unit: "km/h".to_string(),
        min: Some(0.0),
        max: Some(200.0),
        required: false,
    },
    MetricDescriptor {
        name: "cloud_cover_percent".to_string(),
        display_name: "Cloud Cover".to_string(),
        data_type: MetricDataType::Integer,
        unit: "%".to_string(),
        min: Some(0.0),
        max: Some(100.0),
        required: false,
    },
]);

/// Static command descriptors - defined once to avoid lifetime issues
static COMMANDS: Lazy<[ExtensionCommand; 3]> = Lazy::new(|| [
    // Query weather with location parameters
    ExtensionCommand {
        name: "query_weather".to_string(),
        display_name: "Query Weather".to_string(),
        payload_template: r#"{"city": "{{city}}", "units": "{{units}}"}"#.to_string(),
        parameters: vec![
            // City parameter (required)
            ParameterDefinition {
                name: "city".to_string(),
                display_name: "City".to_string(),
                description: "Name of the city to query weather for".to_string(),
                param_type: MetricDataType::String,
                required: true,
                default_value: Some(ParamMetricValue::String("Beijing".to_string())),
                min: None,
                max: None,
                options: vec![],
            },
            // Units parameter (optional, with enum options)
            ParameterDefinition {
                name: "units".to_string(),
                display_name: "Units".to_string(),
                description: "Temperature units to use".to_string(),
                param_type: MetricDataType::Enum {
                    options: vec!["celsius".to_string(), "fahrenheit".to_string(), "kelvin".to_string()],
                },
                required: false,
                default_value: Some(ParamMetricValue::String("celsius".to_string())),
                min: None,
                max: None,
                options: vec!["celsius".to_string(), "fahrenheit".to_string(), "kelvin".to_string()],
            },
            // Days ahead parameter (integer with range)
            ParameterDefinition {
                name: "days_ahead".to_string(),
                display_name: "Days Ahead".to_string(),
                description: "Number of days ahead for forecast (1-7)".to_string(),
                param_type: MetricDataType::Integer,
                required: false,
                default_value: Some(ParamMetricValue::Integer(1)),
                min: Some(1.0),
                max: Some(7.0),
                options: vec![],
            },
            // Include alerts parameter (boolean)
            ParameterDefinition {
                name: "include_alerts".to_string(),
                display_name: "Include Alerts".to_string(),
                description: "Whether to include weather alerts in response".to_string(),
                param_type: MetricDataType::Boolean,
                required: false,
                default_value: Some(ParamMetricValue::Boolean(false)),
                min: None,
                max: None,
                options: vec![],
            },
        ],
        fixed_values: Default::default(),
        samples: vec![
            serde_json::json!({"city": "Tokyo", "units": "celsius"}),
            serde_json::json!({"city": "New York", "units": "fahrenheit", "days_ahead": 3}),
        ],
        llm_hints: "Query current weather for any city. Returns temperature, humidity, wind speed, and cloud cover. Specify units as 'celsius', 'fahrenheit', or 'kelvin'. Use days_ahead for forecasts (1-7 days).".to_string(),
        parameter_groups: vec![
            ParameterGroup {
                name: "location".to_string(),
                display_name: "Location".to_string(),
                description: "Location parameters for the weather query".to_string(),
                parameters: vec!["city".to_string()],
            },
            ParameterGroup {
                name: "options".to_string(),
                display_name: "Options".to_string(),
                description: "Optional parameters for customizing the response".to_string(),
                parameters: vec!["units".to_string(), "days_ahead".to_string(), "include_alerts".to_string()],
            },
        ],
    },
    // Refresh command (no parameters)
    ExtensionCommand {
        name: "refresh".to_string(),
        display_name: "Refresh Weather Data".to_string(),
        payload_template: "{}".to_string(),
        parameters: vec![],
        fixed_values: Default::default(),
        samples: vec![],
        llm_hints: "Refresh the cached weather data for the default city.".to_string(),
        parameter_groups: vec![],
    },
    // Get forecast summary command (with single parameter)
    ExtensionCommand {
        name: "forecast_summary".to_string(),
        display_name: "Forecast Summary".to_string(),
        payload_template: r#"{"city": "{{city}}", "days": {{days}}}"#.to_string(),
        parameters: vec![
            ParameterDefinition {
                name: "days".to_string(),
                display_name: "Days".to_string(),
                description: "Number of days to forecast (1-14)".to_string(),
                param_type: MetricDataType::Integer,
                required: false,
                default_value: Some(ParamMetricValue::Integer(3)),
                min: Some(1.0),
                max: Some(14.0),
                options: vec![],
            },
        ],
        fixed_values: {
            let mut map = std::collections::HashMap::new();
            map.insert("detailed".to_string(), serde_json::Value::Bool(true));
            map
        },
        samples: vec![
            serde_json::json!({"days": 5}),
            serde_json::json!({}),
        ],
        llm_hints: "Get a forecast summary for the specified number of days. Defaults to 3 days.".to_string(),
        parameter_groups: vec![],
    },
]);

// ============================================================================
// Extension Implementation
// ============================================================================

struct WeatherExtension {
    metadata: ExtensionMetadata,
    state: Arc<WeatherState>,
}

impl WeatherExtension {
    fn new(config: &Value) -> Result<Self> {
        let default_city = config
            .get("default_city")
            .and_then(|v| v.as_str())
            .unwrap_or("Beijing")
            .to_string();

        let metadata = ExtensionMetadata {
            id: "neomind.weather.forecast".to_string(),
            name: "Weather Forecast Extension".to_string(),
            version: semver::Version::new(0, 5, 8),
            description: Some("Provides weather data and forecasts for global cities".to_string()),
            author: Some("CamThink".to_string()),
            homepage: Some("https://github.com/neomind/extensions".to_string()),
            license: Some("MIT".to_string()),
            file_path: None,
            config_parameters: Some(vec![
                ParameterDefinition {
                    name: "default_city".to_string(),
                    display_name: "Default City".to_string(),
                    description: "Default city for weather queries when not specified".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("Beijing".to_string())),
                    min: None,
                    max: None,
                    options: vec![],
                },
                ParameterDefinition {
                    name: "temperature_unit".to_string(),
                    display_name: "Temperature Unit".to_string(),
                    description: "Default temperature unit for forecasts".to_string(),
                    param_type: MetricDataType::Enum {
                        options: vec!["celsius".to_string(), "fahrenheit".to_string(), "kelvin".to_string()],
                    },
                    required: false,
                    default_value: Some(ParamMetricValue::String("celsius".to_string())),
                    min: None,
                    max: None,
                    options: vec![],
                },
                ParameterDefinition {
                    name: "update_interval_minutes".to_string(),
                    display_name: "Update Interval (minutes)".to_string(),
                    description: "How often to refresh weather data in metrics. Commands always fetch fresh data.".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(60)),
                    min: Some(10.0),
                    max: Some(1440.0),
                    options: vec![],
                },
                ParameterDefinition {
                    name: "return_cached_between_updates".to_string(),
                    display_name: "Return Cached Data".to_string(),
                    description: "Whether to return cached data between updates. If false, returns no metrics until next update.".to_string(),
                    param_type: MetricDataType::Boolean,
                    required: false,
                    default_value: Some(ParamMetricValue::Boolean(true)),
                    min: None,
                    max: None,
                    options: vec![],
                },
            ]),
        };

        // Parse collection interval configuration
        let collection_interval_minutes = config
            .get("update_interval_minutes")
            .and_then(|v| v.as_i64())
            .unwrap_or(60) as u64;

        let state = Arc::new(WeatherState {
            default_city,
            collection_interval_seconds: collection_interval_minutes * 60,
            last_collection_timestamp: Arc::new(std::sync::Mutex::new(0)),
            cached_metrics: Arc::new(std::sync::Mutex::new(Vec::new())),
        });

        Ok(Self { metadata, state })
    }

    /// Check if enough time has passed to collect new metrics
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

    /// Generate fresh metrics for the default city
    fn generate_fresh_metrics(&self) -> Vec<ExtensionMetricValue> {
        let data = self.simulate_weather(&self.state.default_city);
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let metrics = vec![
            ExtensionMetricValue {
                name: "temperature_c".to_string(),
                value: ParamMetricValue::Float(data["temperature_c"].as_f64().unwrap_or(0.0)),
                timestamp,
            },
            ExtensionMetricValue {
                name: "humidity_percent".to_string(),
                value: ParamMetricValue::Integer(data["humidity_percent"].as_i64().unwrap_or(0)),
                timestamp,
            },
            ExtensionMetricValue {
                name: "wind_speed_kmph".to_string(),
                value: ParamMetricValue::Float(data["wind_speed_kmph"].as_f64().unwrap_or(0.0)),
                timestamp,
            },
            ExtensionMetricValue {
                name: "cloud_cover_percent".to_string(),
                value: ParamMetricValue::Integer(data["cloud_cover_percent"].as_i64().unwrap_or(0)),
                timestamp,
            },
        ];

        // Update cache
        *self.state.cached_metrics.lock().unwrap() = metrics.clone();

        metrics
    }

    /// Simulate weather data for a city
    fn simulate_weather(&self, city: &str) -> Value {
        let hash = city.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        // Use wrapping_sub to avoid overflow, then cast to i32 and subtract
        let temp = (hash % 40) as i32 as f64 - 10.0; // -10 to 30 C
        let humidity = 30 + ((hash % 60) as i32);
        let wind = (hash % 20) as f64;
        let clouds = (hash % 100) as i32;

        serde_json::json!({
            "city": city,
            "temperature_c": temp,
            "humidity_percent": humidity,
            "wind_speed_kmph": wind,
            "cloud_cover_percent": clouds,
            "description": if clouds > 50 { "Cloudy" } else if humidity > 70 { "Humid" } else { "Clear" }
        })
    }
}

#[async_trait::async_trait]
impl Extension for WeatherExtension {
    /// Get extension metadata
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    /// Declare metrics provided by this extension
    fn metrics(&self) -> &[MetricDescriptor] {
        &*METRICS
    }

    /// Declare commands supported by this extension
    fn commands(&self) -> &[ExtensionCommand] {
        &*COMMANDS
    }

    /// Execute a command
    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "query_weather" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.state.default_city);

                // Get units parameter (default: celsius)
                let units = args.get("units")
                    .and_then(|v| v.as_str())
                    .unwrap_or("celsius");

                // Get include_alerts parameter
                let include_alerts = args.get("include_alerts")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let mut data = self.simulate_weather(city);

                // Convert units if requested
                if units == "fahrenheit" {
                    if let Some(temp_c) = data["temperature_c"].as_f64() {
                        data["temperature_c"] = serde_json::json!(temp_c * 9.0 / 5.0 + 32.0);
                        data["temperature_unit"] = "°F".into();
                    }
                } else if units == "kelvin" {
                    if let Some(temp_c) = data["temperature_c"].as_f64() {
                        data["temperature_c"] = serde_json::json!(temp_c + 273.15);
                        data["temperature_unit"] = "K".into();
                    }
                }

                // Add alerts if requested
                if include_alerts {
                    data["alerts"] = serde_json::json!([]);
                    data["alerts_enabled"] = serde_json::Value::Bool(true);
                }

                // Echo back parameters for verification
                data["units_requested"] = units.into();
                data["days_ahead"] = args.get("days_ahead")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1)
                    .into();

                Ok(data)
            }
            "refresh" => {
                let city = &self.state.default_city;
                Ok(self.simulate_weather(city))
            }
            "forecast_summary" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.state.default_city);

                let days = args.get("days")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(3) as usize;

                // Generate multi-day forecast
                let mut forecasts = Vec::new();
                for day in 1..=days {
                    let city_with_day = format!("{}{}", city, day);
                    let daily_data = self.simulate_weather(&city_with_day);
                    forecasts.push(serde_json::json!({
                        "day": day,
                        "date": format!("Day {}", day),
                        "temperature_c": daily_data["temperature_c"],
                        "humidity_percent": daily_data["humidity_percent"],
                        "description": daily_data["description"],
                    }));
                }

                Ok(serde_json::json!({
                    "city": city,
                    "forecast_days": days,
                    "detailed": true,
                    "forecasts": forecasts
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    /// Produce metric data with collection strategy
    ///
    /// This method implements a smart collection strategy:
    /// - If the configured interval has passed, generate fresh metrics
    /// - If return_cached_between_updates is true, return cached data
    /// - Otherwise, return empty vector (no new data until next update)
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // Check if we should collect fresh data
        if self.should_collect_metrics() {
            return Ok(self.generate_fresh_metrics());
        }

        // Interval hasn't passed - return cached or empty based on config
        // Note: We need to get the return_cached config from the loaded config
        // Since config is only available at creation time, we use a default behavior
        // In a real scenario, you'd store this in the state

        // For now, always return cached data if available
        let cached = self.state.cached_metrics.lock().unwrap();
        if !cached.is_empty() {
            Ok(cached.clone())
        } else {
            // First call - generate initial data
            drop(cached);
            Ok(self.generate_fresh_metrics())
        }
    }

    /// Health check
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

// ============================================================================
// FFI Exports
// ============================================================================

use tokio::sync::RwLock;

/// ABI version
#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    ABI_VERSION
}

/// Extension metadata (C-compatible)
#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> neomind_core::extension::system::CExtensionMetadata {
    use std::ffi::CStr;

    // Use static CStr references to avoid dangling pointers
    // Static strings have 'static lifetime, so the pointer is always valid
    let id = CStr::from_bytes_with_nul(b"neomind.weather.forecast\0").unwrap();
    let name = CStr::from_bytes_with_nul(b"Weather Forecast Extension\0").unwrap();
    let version = CStr::from_bytes_with_nul(b"0.3.0\0").unwrap();
    let description = CStr::from_bytes_with_nul(b"Provides weather data and forecasts\0").unwrap();
    let author = CStr::from_bytes_with_nul(b"CamThink\0").unwrap();

    neomind_core::extension::system::CExtensionMetadata {
        abi_version: ABI_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        version: version.as_ptr(),
        description: description.as_ptr(),
        author: author.as_ptr(),
        metric_count: 4,
        command_count: 3,
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
    match WeatherExtension::new(&config) {
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

    /// Helper to create a test extension with default config
    fn create_test_extension() -> WeatherExtension {
        WeatherExtension::new(&json!({})).unwrap()
    }

    /// Helper to create a test extension with custom config
    fn create_extension_with_config(config: Value) -> WeatherExtension {
        WeatherExtension::new(&config).unwrap()
    }

    #[test]
    fn test_extension_creation() {
        let ext = create_test_extension();
        assert_eq!(ext.metadata().id, "neomind.weather.forecast");
        assert_eq!(ext.metadata().name, "Weather Forecast Extension");
        assert_eq!(ext.metadata().version, semver::Version::new(0, 3, 0));
    }

    #[test]
    fn test_extension_with_custom_city() {
        let ext = create_extension_with_config(json!({"default_city": "Shanghai"}));
        assert_eq!(ext.state.default_city, "Shanghai");
    }

    #[test]
    fn test_metrics_count() {
        let ext = create_test_extension();
        assert_eq!(ext.metrics().len(), 4);
    }

    #[test]
    fn test_metrics_content() {
        let ext = create_test_extension();
        let metrics = ext.metrics();

        // Check temperature metric
        assert_eq!(metrics[0].name, "temperature_c");
        assert_eq!(metrics[0].display_name, "Temperature");
        assert_eq!(metrics[0].data_type, MetricDataType::Float);
        assert_eq!(metrics[0].unit, "°C");
        assert_eq!(metrics[0].min, Some(-50.0));
        assert_eq!(metrics[0].max, Some(60.0));

        // Check humidity metric
        assert_eq!(metrics[1].name, "humidity_percent");
        assert_eq!(metrics[1].data_type, MetricDataType::Integer);

        // Check wind speed metric
        assert_eq!(metrics[2].name, "wind_speed_kmph");

        // Check cloud cover metric
        assert_eq!(metrics[3].name, "cloud_cover_percent");
    }

    #[test]
    fn test_commands_count() {
        let ext = create_test_extension();
        assert_eq!(ext.commands().len(), 3);
    }

    #[test]
    fn test_commands_content() {
        let ext = create_test_extension();
        let commands = ext.commands();

        assert_eq!(commands[0].name, "query_weather");
        assert_eq!(commands[0].display_name, "Query Weather");
        assert!(!commands[0].llm_hints.is_empty());

        assert_eq!(commands[1].name, "refresh");
        assert_eq!(commands[1].display_name, "Refresh Weather Data");

        assert_eq!(commands[2].name, "forecast_summary");
        assert_eq!(commands[2].display_name, "Forecast Summary");
    }

    #[tokio::test]
    async fn test_query_weather_default_city() {
        let ext = create_test_extension();
        let result = ext.execute_command("query_weather", &json!({})).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["city"], "Beijing");
        assert!(data["temperature_c"].is_number());
        assert!(data["humidity_percent"].is_number());
    }

    #[tokio::test]
    async fn test_query_weather_custom_city() {
        let ext = create_test_extension();
        let result = ext.execute_command("query_weather", &json!({"city": "Tokyo"})).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["city"], "Tokyo");
    }

    #[tokio::test]
    async fn test_refresh_command() {
        let ext = create_test_extension();
        let result = ext.execute_command("refresh", &json!({})).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["city"], "Beijing");
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = create_test_extension();
        let result = ext.execute_command("unknown_command", &json!({})).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ExtensionError::CommandNotFound(cmd) => assert_eq!(cmd, "unknown_command"),
            _ => panic!("Expected CommandNotFound error"),
        }
    }

    #[test]
    fn test_produce_metrics() {
        let ext = create_test_extension();
        let metrics = ext.produce_metrics().unwrap();
        assert_eq!(metrics.len(), 4);

        // Check each metric
        let metric_names: Vec<&str> = metrics.iter().map(|m| m.name.as_str()).collect();
        assert!(metric_names.contains(&"temperature_c"));
        assert!(metric_names.contains(&"humidity_percent"));
        assert!(metric_names.contains(&"wind_speed_kmph"));
        assert!(metric_names.contains(&"cloud_cover_percent"));
    }

    #[test]
    fn test_produce_metrics_types() {
        let ext = create_test_extension();
        let metrics = ext.produce_metrics().unwrap();

        // Temperature should be float
        assert!(matches!(metrics[0].value, ParamMetricValue::Float(_)));

        // Humidity should be integer
        assert!(matches!(metrics[1].value, ParamMetricValue::Integer(_)));

        // Wind speed should be float
        assert!(matches!(metrics[2].value, ParamMetricValue::Float(_)));

        // Cloud cover should be integer
        assert!(matches!(metrics[3].value, ParamMetricValue::Integer(_)));
    }

    #[tokio::test]
    async fn test_health_check() {
        let ext = create_test_extension();
        let healthy = ext.health_check().await.unwrap();
        assert!(healthy);
    }

    #[test]
    fn test_simulate_weather_consistency() {
        let ext = create_test_extension();
        let data1 = ext.simulate_weather("Paris");
        let data2 = ext.simulate_weather("Paris");

        // Same city should produce same data (hash-based)
        assert_eq!(data1["temperature_c"], data2["temperature_c"]);
        assert_eq!(data1["humidity_percent"], data2["humidity_percent"]);
    }

    #[test]
    fn test_simulate_weather_different_cities() {
        let ext = create_test_extension();
        let data1 = ext.simulate_weather("Paris");
        let data2 = ext.simulate_weather("London");

        // Cities are different, so the city field should reflect that
        assert_eq!(data1["city"], "Paris");
        assert_eq!(data2["city"], "London");

        // The data might be the same due to hash collision (unlikely but possible),
        // but we at least verify the simulation runs without error
        assert!(data1["temperature_c"].is_number());
        assert!(data2["temperature_c"].is_number());
    }

    #[test]
    fn test_simulate_weather_all_fields_present() {
        let ext = create_test_extension();
        let data = ext.simulate_weather("TestCity");

        assert_eq!(data["city"], "TestCity");
        assert!(data["temperature_c"].is_number());
        assert!(data["humidity_percent"].is_number());
        assert!(data["wind_speed_kmph"].is_number());
        assert!(data["cloud_cover_percent"].is_number());
        assert!(data["description"].is_string());
    }

    #[test]
    fn test_simulate_weather_description_logic() {
        let ext = create_test_extension();

        // Test high humidity (should be "Humid")
        let humid_city = format!("H{}", 100); // Will hash to high humidity
        let data = ext.simulate_weather(&humid_city);
        if data["cloud_cover_percent"].as_i64().unwrap_or(0) <= 50
            && data["humidity_percent"].as_i64().unwrap_or(0) > 70 {
            assert_eq!(data["description"], "Humid");
        }

        // Test high clouds (should be "Cloudy")
        let cloudy_city = format!("C{}", 200);
        let data2 = ext.simulate_weather(&cloudy_city);
        if data2["cloud_cover_percent"].as_i64().unwrap_or(0) > 50 {
            assert_eq!(data2["description"], "Cloudy");
        }
    }

    #[tokio::test]
    async fn test_with_custom_default_city() {
        let ext = create_extension_with_config(json!({"default_city": "New York"}));

        // Query without city should use default
        let result = ext.execute_command("query_weather", &json!({})).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["city"], "New York");

        // Refresh should also use default
        let result2 = ext.execute_command("refresh", &json!({})).await;
        assert!(result2.is_ok());
        let data2 = result2.unwrap();
        assert_eq!(data2["city"], "New York");
    }

    #[test]
    fn test_metric_ranges() {
        let ext = create_test_extension();
        let metrics = ext.produce_metrics().unwrap();

        // Temperature should be between -50 and 60
        if let ParamMetricValue::Float(temp) = metrics[0].value {
            assert!(temp >= -50.0 && temp <= 60.0, "Temperature out of range: {}", temp);
        } else {
            panic!("Temperature should be Float");
        }

        // Humidity should be between 0 and 100
        if let ParamMetricValue::Integer(humidity) = metrics[1].value {
            assert!(humidity >= 0 && humidity <= 100, "Humidity out of range: {}", humidity);
        } else {
            panic!("Humidity should be Integer");
        }

        // Wind speed should be non-negative
        if let ParamMetricValue::Float(wind) = metrics[2].value {
            assert!(wind >= 0.0, "Wind speed should be non-negative: {}", wind);
        }

        // Cloud cover should be between 0 and 100
        if let ParamMetricValue::Integer(clouds) = metrics[3].value {
            assert!(clouds >= 0 && clouds <= 100, "Cloud cover out of range: {}", clouds);
        } else {
            panic!("Cloud cover should be Integer");
        }
    }

    // ========================================================================
    // Parameter Tests
    // ========================================================================

    #[tokio::test]
    async fn test_query_weather_with_units() {
        let ext = create_test_extension();

        // Test fahrenheit conversion
        let result = ext.execute_command(
            "query_weather",
            &json!({"city": "Tokyo", "units": "fahrenheit"})
        ).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["city"], "Tokyo");
        assert_eq!(data["units_requested"], "fahrenheit");
        // Temperature should be in fahrenheit
        if let Some(temp) = data["temperature_c"].as_f64() {
            // Should be converted from Celsius to Fahrenheit
            assert!(temp > 30.0, "Fahrenheit temperature should be higher");
        }

        // Test kelvin conversion
        let result = ext.execute_command(
            "query_weather",
            &json!({"city": "Tokyo", "units": "kelvin"})
        ).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["units_requested"], "kelvin");
        assert_eq!(data["temperature_unit"], "K");
    }

    #[tokio::test]
    async fn test_query_weather_with_alerts() {
        let ext = create_test_extension();

        let result = ext.execute_command(
            "query_weather",
            &json!({"city": "London", "include_alerts": true})
        ).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["alerts_enabled"], true);
        assert!(data["alerts"].is_array());
    }

    #[tokio::test]
    async fn test_query_weather_with_days_ahead() {
        let ext = create_test_extension();

        let result = ext.execute_command(
            "query_weather",
            &json!({"city": "Paris", "days_ahead": 5})
        ).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["days_ahead"], 5);
    }

    #[tokio::test]
    async fn test_forecast_summary_default() {
        let ext = create_test_extension();

        let result = ext.execute_command(
            "forecast_summary",
            &json!({})
        ).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["city"], "Beijing");  // Default city
        assert_eq!(data["forecast_days"], 3);  // Default days
        assert_eq!(data["detailed"], true);
        assert!(data["forecasts"].is_array());
    }

    #[tokio::test]
    async fn test_forecast_summary_custom_days() {
        let ext = create_test_extension();

        let result = ext.execute_command(
            "forecast_summary",
            &json!({"city": "Osaka", "days": 5})
        ).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["city"], "Osaka");
        assert_eq!(data["forecast_days"], 5);

        let forecasts = data["forecasts"].as_array().unwrap();
        assert_eq!(forecasts.len(), 5);
    }

    #[test]
    fn test_commands_have_parameters() {
        let ext = create_test_extension();
        let commands = ext.commands();

        // query_weather should have 4 parameters
        let query_cmd = commands.iter().find(|c| c.name == "query_weather").unwrap();
        assert_eq!(query_cmd.parameters.len(), 4);

        // Check parameter types
        let city_param = &query_cmd.parameters[0];
        assert_eq!(city_param.name, "city");
        assert_eq!(city_param.param_type, MetricDataType::String);
        assert!(city_param.required);

        let units_param = &query_cmd.parameters[1];
        assert_eq!(units_param.name, "units");
        assert!(!units_param.required);
        assert!(matches!(units_param.param_type, MetricDataType::Enum { .. }));

        let days_param = &query_cmd.parameters[2];
        assert_eq!(days_param.name, "days_ahead");
        assert_eq!(days_param.min, Some(1.0));
        assert_eq!(days_param.max, Some(7.0));

        let alerts_param = &query_cmd.parameters[3];
        assert_eq!(alerts_param.name, "include_alerts");
        assert!(matches!(alerts_param.param_type, MetricDataType::Boolean));
    }

    #[test]
    fn test_commands_have_samples() {
        let ext = create_test_extension();
        let commands = ext.commands();

        let query_cmd = commands.iter().find(|c| c.name == "query_weather").unwrap();
        assert_eq!(query_cmd.samples.len(), 2);
    }

    #[test]
    fn test_commands_have_parameter_groups() {
        let ext = create_test_extension();
        let commands = ext.commands();

        let query_cmd = commands.iter().find(|c| c.name == "query_weather").unwrap();
        assert_eq!(query_cmd.parameter_groups.len(), 2);

        let location_group = &query_cmd.parameter_groups[0];
        assert_eq!(location_group.name, "location");
        assert_eq!(location_group.parameters.len(), 1);
        assert!(location_group.parameters.contains(&"city".to_string()));

        let options_group = &query_cmd.parameter_groups[1];
        assert_eq!(options_group.name, "options");
        assert_eq!(options_group.parameters.len(), 3);
    }

    #[tokio::test]
    async fn test_forecast_summary_command() {
        let ext = create_test_extension();

        let result = ext.execute_command(
            "forecast_summary",
            &json!({"days": 2})
        ).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["forecast_days"], 2);
    }

    #[test]
    fn test_commands_count_updated() {
        let ext = create_test_extension();
        assert_eq!(ext.commands().len(), 3);
    }
}
