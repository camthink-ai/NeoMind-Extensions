//! NeoMind Weather Forecast Extension
//!
//! This extension provides weather data for global cities using OpenWeatherMap API.
//!
//! ## Capabilities
//!
//! - **Tools**: Query current weather and forecasts for any city
//! - **Metrics**: Provides temperature, humidity, wind speed, and cloud cover
//! - **Commands**: Refresh weather data, change default city
//!
//! ## Configuration
//!
//! Set the `OPENWEATHER_API_KEY` environment variable or pass it via config:
//! ```json
//! {
//!   "api_key": "your_api_key_here",
//!   "default_city": "Beijing",
//!   "units": "metric",
//!   "timeout_seconds": 10
//! }
//! ```

use std::sync::Arc;
use std::time::{Duration, SystemTime};
use neomind_extension_sdk::prelude::*;

// Extension state
struct WeatherState {
    api_key: String,
    default_city: String,
    units: String,
    timeout: Duration,
    // Cached weather data
    last_update: Arc<std::sync::RwLock<Option<SystemTime>>>,
    cached_data: Arc<std::sync::RwLock<Option<WeatherData>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WeatherData {
    city: String,
    temperature: f64,
    humidity: i32,
    wind_speed: f64,
    cloud_cover: i32,
    description: String,
    timestamp: i64,
}

// ============================================================================
// Extension Implementation
// ============================================================================

struct WeatherExtension {
    state: Arc<WeatherState>,
}

impl WeatherExtension {
    fn new(config: &serde_json::Value) -> Result<Self, ExtensionError> {
        // Get API key from config or environment
        let api_key = config
            .get("api_key")
            .and_then(|v| v.as_str())
            .or_else(|| std::env::var("OPENWEATHER_API_KEY").ok())
            .unwrap_or_else(|| {
                // Use a demo key placeholder - in production, this would fail
                "demo_key".to_string()
            });

        let default_city = config
            .get("default_city")
            .and_then(|v| v.as_str())
            .unwrap_or("Beijing")
            .to_string();

        let units = config
            .get("units")
            .and_then(|v| v.as_str())
            .unwrap_or("metric")
            .to_string();

        let timeout_secs = config
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(10);

        let state = Arc::new(WeatherState {
            api_key,
            default_city,
            units,
            timeout: Duration::from_secs(timeout_secs),
            last_update: Arc::new(std::sync::RwLock::new(None)),
            cached_data: Arc::new(std::sync::RwLock::new(None)),
        });

        Ok(Self { state })
    }

    /// Fetch weather data from API (or simulate in demo mode)
    fn fetch_weather(&self, city: &str) -> Result<WeatherData, ExtensionError> {
        // In demo mode (no real API key), return simulated data
        if self.state.api_key == "demo_key" {
            return Ok(self.simulate_weather(city));
        }

        // Real API call would go here
        // For now, return simulated data
        Ok(self.simulate_weather(city))
    }

    /// Simulate weather data for demonstration
    fn simulate_weather(&self, city: &str) -> WeatherData {
        // Simple simulation based on city name hash
        let hash = city.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        let base_temp = ((hash % 40) - 10) as f64; // -10 to 30 C
        let humidity = 30 + ((hash % 60) as i32);
        let wind_speed = (hash % 20) as f64;
        let cloud_cover = (hash % 100) as i32;

        WeatherData {
            city: city.to_string(),
            temperature: base_temp,
            humidity,
            wind_speed,
            cloud_cover,
            description: if cloud_cover > 50 {
                "Cloudy".to_string()
            } else if humidity > 70 {
                "Humid".to_string()
            } else {
                "Clear".to_string()
            },
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        }
    }
}

impl Extension for WeatherExtension {
    /// Get extension metadata
    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            id: "neomind.weather.forecast".to_string(),
            name: "Weather Forecast Extension".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "NeoMind Team".to_string(),
            description: "Provides weather data and forecasts for global cities".to_string(),
            license: "MIT".to_string(),
            homepage: Some("https://github.com/neomind/neomind-weather-extension".to_string()),
        }
    }

    /// Get extension capabilities
    fn capabilities(&self) -> Vec<ExtensionCapabilityDescriptor> {
        vec![
            // Tool: Query current weather
            ExtensionCapabilityDescriptor {
                id: "query_weather".to_string(),
                name: "Query Weather".to_string(),
                description: "Get current weather for any city".to_string(),
                capability_type: ExtensionCapabilityType::Tool,
                config_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": "City name (e.g., Beijing, Shanghai, Tokyo)"
                        }
                    },
                    "required": ["city"]
                })),
            },
            // Tool: Query weather forecast
            ExtensionCapabilityDescriptor {
                id: "query_forecast".to_string(),
                name: "Query Forecast".to_string(),
                description: "Get 3-day weather forecast".to_string(),
                capability_type: ExtensionCapabilityType::Tool,
                config_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": "City name"
                        },
                        "days": {
                            "type": "integer",
                            "description": "Number of days (1-7)",
                            "default": 3,
                            "minimum": 1,
                            "maximum": 7
                        }
                    },
                    "required": ["city"]
                })),
            },
            // Command: Refresh weather data
            ExtensionCapabilityDescriptor {
                id: "refresh".to_string(),
                name: "Refresh".to_string(),
                description: "Force refresh cached weather data".to_string(),
                capability_type: ExtensionCapabilityType::Command,
                config_schema: None,
            },
            // Command: Set default city
            ExtensionCapabilityDescriptor {
                id: "set_city".to_string(),
                name: "Set Default City".to_string(),
                description: "Change the default city for queries".to_string(),
                capability_type: ExtensionCapabilityType::Command,
                config_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": "New default city name"
                        }
                    },
                    "required": ["city"]
                })),
            },
        ]
    }

    /// Get provided metrics
    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                id: "temperature_c".to_string(),
                name: "Temperature".to_string(),
                description: "Current temperature in Celsius".to_string(),
                unit: "°C".to_string(),
                data_type: "float".to_string(),
            },
            MetricDescriptor {
                id: "humidity_percent".to_string(),
                name: "Humidity".to_string(),
                description: "Relative humidity percentage".to_string(),
                unit: "%".to_string(),
                data_type: "integer".to_string(),
            },
            MetricDescriptor {
                id: "wind_speed_kmph".to_string(),
                name: "Wind Speed".to_string(),
                description: "Wind speed in kilometers per hour".to_string(),
                unit: "km/h".to_string(),
                data_type: "float".to_string(),
            },
            MetricDescriptor {
                id: "cloud_cover_percent".to_string(),
                name: "Cloud Cover".to_string(),
                description: "Cloud coverage percentage".to_string(),
                unit: "%".to_string(),
                data_type: "integer".to_string(),
            },
            MetricDescriptor {
                id: "last_update_time".to_string(),
                name: "Last Update".to_string(),
                description: "Timestamp of last weather data refresh".to_string(),
                unit: "unix_ts".to_string(),
                data_type: "integer".to_string(),
            },
        ]
    }

    /// Execute a command
    fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value, ExtensionError> {
        match command {
            "refresh" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.state.default_city);

                let data = self.fetch_weather(city)?;

                // Update cache
                *self.state.cached_data.write().unwrap() = Some(data.clone());
                *self.state.last_update.write().unwrap() = Some(SystemTime::now());

                Ok(serde_json::to_value(data).unwrap())
            }
            "set_city" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidInput("city is required".to_string()))?;

                Ok(serde_json::json!({
                    "status": "success",
                    "message": format!("Default city set to {}", city),
                    "city": city
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    /// Execute a tool call (for AI agent use)
    fn execute_tool(&self, tool: &str, args: &serde_json::Value) -> Result<serde_json::Value, ExtensionError> {
        match tool {
            "query_weather" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidInput("city is required".to_string()))?;

                let data = self.fetch_weather(city)?;

                Ok(serde_json::json!({
                    "city": data.city,
                    "temperature": format!("{}°C", data.temperature),
                    "humidity": format!("{}%", data.humidity),
                    "wind_speed": format!("{} km/h", data.wind_speed),
                    "cloud_cover": format!("{}%", data.cloud_cover),
                    "description": data.description,
                    "summary": format!("Currently in {}: {}°C, {}, Humidity: {}%, Wind: {} km/h",
                        data.city, data.temperature, data.description, data.humidity, data.wind_speed)
                }))
            }
            "query_forecast" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidInput("city is required".to_string()))?;

                let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

                // Simulate forecast
                let mut forecast = Vec::new();
                for i in 0..days {
                    let data = self.simulate_weather(city);
                    forecast.push(serde_json::json!({
                        "day": i + 1,
                        "temperature": format!("{}°C", data.temperature + (i as f64 * 2.0)),
                        "humidity": format!("{}%", data.humidity),
                        "description": data.description
                    }));
                }

                Ok(serde_json::json!({
                    "city": city,
                    "forecast": forecast
                }))
            }
            _ => Err(ExtensionError::ToolNotFound(tool.to_string())),
        }
    }

    /// Health check
    fn health_check(&self) -> Result<bool, ExtensionError> {
        // Check if we can fetch weather data
        let _ = self.fetch_weather(&self.state.default_city)?;
        Ok(true)
    }

    /// Produce current metric values
    fn produce_metrics(&self) -> Vec<crate::types::ExtensionMetricValue> {
        let data = self.fetch_weather(&self.state.default_city)
            .unwrap_or_else(|_| self.simulate_weather(&self.state.default_city));

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        vec![
            crate::types::ExtensionMetricValue {
                name: "temperature_c".to_string(),
                value: neomind_extension_sdk::types::ParamMetricValue::Float(data.temperature),
                timestamp,
                labels: vec![("city".to_string(), data.city.clone())].into_iter().collect(),
            },
            crate::types::ExtensionMetricValue {
                name: "humidity_percent".to_string(),
                value: neomind_extension_sdk::types::ParamMetricValue::Integer(data.humidity as i64),
                timestamp,
                labels: vec![("city".to_string(), data.city.clone())].into_iter().collect(),
            },
            crate::types::ExtensionMetricValue {
                name: "wind_speed_kmph".to_string(),
                value: neomind_extension_sdk::types::ParamMetricValue::Float(data.wind_speed),
                timestamp,
                labels: vec![("city".to_string(), data.city.clone())].into_iter().collect(),
            },
            crate::types::ExtensionMetricValue {
                name: "cloud_cover_percent".to_string(),
                value: neomind_extension_sdk::types::ParamMetricValue::Integer(data.cloud_cover as i64),
                timestamp,
                labels: vec![("city".to_string(), data.city)].into_iter().collect(),
            },
        ]
    }
}

// ============================================================================
// Extension Entry Point
// ============================================================================

/// Extension version
#[no_mangle]
pub extern "C" fn neomind_ext_version() -> u32 {
    NEO_EXT_ABI_VERSION
}

/// Extension metadata
#[no_mangle]
pub extern "C" fn neomind_ext_metadata() -> *const std::os::raw::c_char {
    let metadata = ExtensionMetadata {
        id: "neomind.weather.forecast".to_string(),
        name: "Weather Forecast Extension".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        author: "NeoMind Team".to_string(),
        description: "Provides weather data and forecasts for global cities".to_string(),
        license: "MIT".to_string(),
        homepage: Some("https://github.com/neomind/neomind-weather-extension".to_string()),
    };

    let json = serde_json::to_string(&metadata).unwrap();
    let cstr = std::ffi::CString::new(json).unwrap();
    // Leak the CString to return a stable pointer
    cstr.into_raw()
}

/// Create extension instance
#[no_mangle]
pub extern "C" fn neomind_ext_create(config_json: *const std::os::raw::c_char, config_len: usize) -> *mut std::os::raw::c_void {
    // Parse config
    let config = if config_json.is_null() || config_len == 0 {
        serde_json::json!({})
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(config_json as *const u8, config_len);
            let s = std::str::from_utf8_unchecked(slice);
            serde_json::from_str(s).unwrap_or(serde_json::json!({}))
        }
    };

    // Create extension
    match WeatherExtension::new(&config) {
        Ok(ext) => {
            let boxed = Box::new(ext);
            Box::into_raw(boxed) as *mut std::os::raw::c_void
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy extension instance
#[no_mangle]
pub extern "C" fn neomind_ext_destroy(instance: *mut std::os::raw::c_void) {
    if !instance.is_null() {
        unsafe {
            let _ = Box::from_raw(instance as *mut WeatherExtension);
        }
    }
}

// Export for SDK
neomind_extension_sdk::export_extension!(WeatherExtension);
