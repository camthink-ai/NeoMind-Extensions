//! NeoMind Weather Forecast Extension (V2)
//!
//! Weather forecast extension built for the NeoMind isolated extension runtime.
//!
//! Features:
//! - Real-time weather data from Open-Meteo API
//! - Metrics export for temperature, humidity, wind speed, etc.
//! - Data caching for metrics collection
//!
//! # Architecture Note
//!
//! This extension uses **sync HTTP client (ureq)** to avoid Tokio runtime
//! compatibility issues when loaded as a dynamic library (.dylib/.so/.dll).

use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, AtomicBool, Ordering};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherResult {
    pub city: String,
    pub country: Option<String>,
    pub temperature_c: f64,
    pub feels_like_c: f64,
    pub humidity_percent: i32,
    pub wind_speed_kmph: f64,
    pub wind_direction_deg: i32,
    pub wind_direction: String,
    pub cloud_cover_percent: i32,
    pub pressure_hpa: f64,
    pub description: String,
    pub is_day: bool,
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    results: Option<Vec<GeoLocation>>,
}

#[derive(Debug, Deserialize)]
struct GeoLocation {
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WeatherResponse {
    current: CurrentWeather,
}

#[derive(Debug, Deserialize)]
struct CurrentWeather {
    temperature_2m: f64,
    relative_humidity_2m: i32,
    wind_speed_10m: f64,
    wind_direction_10m: Option<i32>,
    weather_code: i32,
    apparent_temperature: Option<f64>,
    cloud_cover: Option<i32>,
    pressure_msl: Option<f64>,
    is_day: Option<i32>,
}

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct WeatherExtension {
    default_city: std::sync::RwLock<String>,
    request_count: AtomicI64,
    last_temperature_c: AtomicI64,
    last_feels_like_c: AtomicI64,
    last_humidity_percent: AtomicI64,
    last_wind_speed_kmph: AtomicI64,
    last_wind_direction_deg: AtomicI64,
    last_cloud_cover_percent: AtomicI64,
    last_pressure_hpa: AtomicI64,
    last_update_ts: AtomicI64,
    has_data: AtomicBool,
}

impl WeatherExtension {
    pub fn new() -> Self {
        Self {
            default_city: std::sync::RwLock::new("Beijing".to_string()),
            request_count: AtomicI64::new(0),
            last_temperature_c: AtomicI64::new(0),
            last_feels_like_c: AtomicI64::new(0),
            last_humidity_percent: AtomicI64::new(0),
            last_wind_speed_kmph: AtomicI64::new(0),
            last_wind_direction_deg: AtomicI64::new(0),
            last_cloud_cover_percent: AtomicI64::new(0),
            last_pressure_hpa: AtomicI64::new(101325),
            last_update_ts: AtomicI64::new(0),
            has_data: AtomicBool::new(false),
        }
    }

    fn get_default_city(&self) -> String {
        self.default_city.read().unwrap().clone()
    }

    fn set_default_city(&self, city: &str) {
        *self.default_city.write().unwrap() = city.to_string();
    }

    fn store_weather_metrics(&self, weather: &WeatherResult) {
        self.last_temperature_c.store((weather.temperature_c * 100.0) as i64, Ordering::SeqCst);
        self.last_feels_like_c.store((weather.feels_like_c * 100.0) as i64, Ordering::SeqCst);
        self.last_humidity_percent.store(weather.humidity_percent as i64, Ordering::SeqCst);
        self.last_wind_speed_kmph.store((weather.wind_speed_kmph * 100.0) as i64, Ordering::SeqCst);
        self.last_wind_direction_deg.store(weather.wind_direction_deg as i64, Ordering::SeqCst);
        self.last_cloud_cover_percent.store(weather.cloud_cover_percent as i64, Ordering::SeqCst);
        self.last_pressure_hpa.store((weather.pressure_hpa * 100.0) as i64, Ordering::SeqCst);
        self.last_update_ts.store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        self.has_data.store(true, Ordering::SeqCst);
    }

    /// Get weather using sync HTTP client (ureq)
    fn get_weather_sync(&self, city: &str) -> Result<WeatherResult> {
        self.request_count.fetch_add(1, Ordering::SeqCst);

        let location = self.geocode_sync(city)
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        let mut weather = self.fetch_weather_sync(&location)
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;

        weather.timestamp = Some(chrono::Utc::now().to_rfc3339());
        self.store_weather_metrics(&weather);

        Ok(weather)
    }

    fn geocode_sync(&self, city: &str) -> std::result::Result<GeoLocation, String> {
        let encoded_city = urlencoding::encode(city);
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
            encoded_city
        );

        let response: serde_json::Value = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .call()
            .map_err(|e| format!("HTTP error: {}", e))?
            .into_json()
            .map_err(|e| format!("JSON error: {}", e))?;

        let geo_data: GeocodingResponse = serde_json::from_value(response)
            .map_err(|e| format!("Parse error: {}", e))?;

        geo_data.results
            .and_then(|mut v| v.pop())
            .ok_or_else(|| format!("City not found: {}", city))
    }

    fn fetch_weather_sync(&self, location: &GeoLocation) -> std::result::Result<WeatherResult, String> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,cloud_cover,pressure_msl,wind_speed_10m,wind_direction_10m,is_day&timezone=auto&windspeed_unit=kmh",
            location.latitude,
            location.longitude
        );

        let response: serde_json::Value = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .call()
            .map_err(|e| format!("HTTP error: {}", e))?
            .into_json()
            .map_err(|e| format!("JSON error: {}", e))?;

        let weather: WeatherResponse = serde_json::from_value(response)
            .map_err(|e| format!("Parse error: {}", e))?;

        let cw = &weather.current;
        let wind_deg = cw.wind_direction_10m.unwrap_or(0);

        Ok(WeatherResult {
            city: location.name.clone(),
            country: location.country.clone(),
            temperature_c: cw.temperature_2m,
            feels_like_c: cw.apparent_temperature.unwrap_or(cw.temperature_2m),
            humidity_percent: cw.relative_humidity_2m,
            wind_speed_kmph: cw.wind_speed_10m,
            wind_direction_deg: wind_deg,
            wind_direction: wind_direction_to_cardinal(wind_deg),
            cloud_cover_percent: cw.cloud_cover.unwrap_or(0),
            pressure_hpa: cw.pressure_msl.unwrap_or(1013.0),
            description: weather_code_to_description(cw.weather_code),
            is_day: cw.is_day.unwrap_or(1) == 1,
            timestamp: None,
        })
    }
}

impl Default for WeatherExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for WeatherExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "weather-forecast-v2",
                "Weather Forecast V2",
                "2.0.0"
            )
            .with_description("Weather forecast extension for the NeoMind isolated runtime using a sync HTTP client")
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "defaultCity".to_string(),
                    display_name: "Default City".to_string(),
                    description: "Default city for weather display".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("Beijing".to_string())),
                    min: None,
                    max: None,
                    options: vec![
                        "Beijing".to_string(),
                        "Shanghai".to_string(),
                        "New York".to_string(),
                        "London".to_string(),
                        "Tokyo".to_string(),
                    ],
                },
                ParameterDefinition {
                    name: "refreshInterval".to_string(),
                    display_name: "Refresh Interval".to_string(),
                    description: "Refresh interval in milliseconds (default: 5 minutes)".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(ParamMetricValue::Integer(300000)),
                    min: Some(60000.0),  // 1 minute minimum
                    max: Some(3600000.0), // 1 hour maximum
                    options: Vec::new(),
                },
                ParameterDefinition {
                    name: "unit".to_string(),
                    display_name: "Temperature Unit".to_string(),
                    description: "Temperature unit (celsius or fahrenheit)".to_string(),
                    param_type: MetricDataType::Enum {
                        options: vec!["celsius".to_string(), "fahrenheit".to_string()],
                    },
                    required: false,
                    default_value: Some(ParamMetricValue::String("celsius".to_string())),
                    min: None,
                    max: None,
                    options: vec!["celsius".to_string(), "fahrenheit".to_string()],
                },
            ])
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "temperature_c".to_string(),
                display_name: "Temperature".to_string(),
                data_type: MetricDataType::Float,
                unit: "°C".to_string(),
                min: Some(-100.0),
                max: Some(100.0),
                required: false,
            },
            MetricDescriptor {
                name: "feels_like_c".to_string(),
                display_name: "Feels Like".to_string(),
                data_type: MetricDataType::Float,
                unit: "°C".to_string(),
                min: Some(-100.0),
                max: Some(100.0),
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
                max: Some(500.0),
                required: false,
            },
            MetricDescriptor {
                name: "wind_direction_deg".to_string(),
                display_name: "Wind Direction".to_string(),
                data_type: MetricDataType::Integer,
                unit: "°".to_string(),
                min: Some(0.0),
                max: Some(360.0),
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
            MetricDescriptor {
                name: "pressure_hpa".to_string(),
                display_name: "Pressure".to_string(),
                data_type: MetricDataType::Float,
                unit: "hPa".to_string(),
                min: Some(800.0),
                max: Some(1200.0),
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
            MetricDescriptor {
                name: "last_update_ts".to_string(),
                display_name: "Last Update Timestamp".to_string(),
                data_type: MetricDataType::Integer,
                unit: "ms".to_string(),
                min: None,
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "get_weather".to_string(),
                display_name: "Get Weather".to_string(),
                description: "Get current weather for a city".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "city".to_string(),
                        display_name: "City".to_string(),
                        description: "City name to get weather for".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec!["Beijing".to_string(), "Shanghai".to_string(), "New York".to_string()],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({ "city": "Beijing" }),
                    json!({ "city": "Shanghai" }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "refresh".to_string(),
                display_name: "Refresh Weather".to_string(),
                description: "Refresh weather data for the default city".to_string(),
                payload_template: String::new(),
                parameters: Vec::new(),
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "set_default_city".to_string(),
                display_name: "Set Default City".to_string(),
                description: "Set the default city for weather queries".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "city".to_string(),
                        display_name: "City".to_string(),
                        description: "City name to set as default".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "city": "Shanghai" })],
                parameter_groups: Vec::new(),
            },
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "get_weather" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'city' parameter".to_string()))?;

                let result = self.get_weather_sync(city)?;
                serde_json::to_value(result)
                    .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))
            }

            "refresh" => {
                let default_city = self.get_default_city();
                let result = self.get_weather_sync(&default_city)?;
                Ok(json!({
                    "success": true,
                    "city": default_city,
                    "data": result
                }))
            }

            "set_default_city" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'city' parameter".to_string()))?;

                self.set_default_city(city);
                Ok(json!({
                    "success": true,
                    "default_city": city
                }))
            }

            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = Vec::with_capacity(9);

        metrics.push(ExtensionMetricValue {
            name: "request_count".to_string(),
            value: ParamMetricValue::Integer(self.request_count.load(Ordering::SeqCst)),
            timestamp: now,
        });

        if self.has_data.load(Ordering::SeqCst) {
            metrics.extend(vec![
                ExtensionMetricValue {
                    name: "temperature_c".to_string(),
                    value: ParamMetricValue::Float(self.last_temperature_c.load(Ordering::SeqCst) as f64 / 100.0),
                    timestamp: now,
                },
                ExtensionMetricValue {
                    name: "feels_like_c".to_string(),
                    value: ParamMetricValue::Float(self.last_feels_like_c.load(Ordering::SeqCst) as f64 / 100.0),
                    timestamp: now,
                },
                ExtensionMetricValue {
                    name: "humidity_percent".to_string(),
                    value: ParamMetricValue::Integer(self.last_humidity_percent.load(Ordering::SeqCst)),
                    timestamp: now,
                },
                ExtensionMetricValue {
                    name: "wind_speed_kmph".to_string(),
                    value: ParamMetricValue::Float(self.last_wind_speed_kmph.load(Ordering::SeqCst) as f64 / 100.0),
                    timestamp: now,
                },
                ExtensionMetricValue {
                    name: "wind_direction_deg".to_string(),
                    value: ParamMetricValue::Integer(self.last_wind_direction_deg.load(Ordering::SeqCst)),
                    timestamp: now,
                },
                ExtensionMetricValue {
                    name: "cloud_cover_percent".to_string(),
                    value: ParamMetricValue::Integer(self.last_cloud_cover_percent.load(Ordering::SeqCst)),
                    timestamp: now,
                },
                ExtensionMetricValue {
                    name: "pressure_hpa".to_string(),
                    value: ParamMetricValue::Float(self.last_pressure_hpa.load(Ordering::SeqCst) as f64 / 100.0),
                    timestamp: now,
                },
                ExtensionMetricValue {
                    name: "last_update_ts".to_string(),
                    value: ParamMetricValue::Integer(self.last_update_ts.load(Ordering::SeqCst)),
                    timestamp: now,
                },
            ]);
        }

        Ok(metrics)
    }

    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        // Apply configuration parameters
        if let Some(default_city) = config.get("defaultCity").and_then(|v| v.as_str()) {
            self.set_default_city(default_city);
        }

        // Note: refreshInterval and unit would be used by the frontend component
        // The extension itself doesn't need to handle these directly

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn wind_direction_to_cardinal(degrees: i32) -> String {
    let directions = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE",
                      "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
    directions[((degrees + 11) / 23 % 16) as usize].to_string()
}

fn weather_code_to_description(code: i32) -> String {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Heavy drizzle",
        61 => "Slight rain",
        63 => "Rain",
        65 => "Heavy rain",
        71 => "Slight snow",
        73 => "Snow",
        75 => "Heavy snow",
        80 => "Slight showers",
        81 => "Showers",
        82 => "Heavy showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }.to_string()
}

// ============================================================================
// FFI Exports
// ============================================================================

// Use SDK's export macro to generate all FFI functions
neomind_extension_sdk::neomind_export!(WeatherExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_extension_metadata() {
        let ext = WeatherExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "weather-forecast-v2");
        assert_eq!(meta.name, "Weather Forecast V2");
        assert_eq!(
            meta.description.as_deref(),
            Some("Weather forecast extension for the NeoMind isolated runtime using a sync HTTP client")
        );
    }

    #[test]
    fn test_metadata_json_matches_runtime_metadata() {
        let ext = WeatherExtension::new();
        let meta = ext.metadata();
        let metadata_json: Value = serde_json::from_str(include_str!("../metadata.json")).unwrap();

        assert_eq!(metadata_json["id"], meta.id);
        assert_eq!(metadata_json["name"], meta.name);
        assert_eq!(
            metadata_json["description"].as_str(),
            meta.description.as_deref()
        );
        assert_eq!(metadata_json["license"], "Apache-2.0");
        assert_eq!(
            metadata_json["repository"],
            "https://github.com/camthink-ai/NeoMind-Extensions"
        );
    }

    #[test]
    fn test_extension_metrics() {
        let ext = WeatherExtension::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 9);
    }

    #[test]
    fn test_extension_commands() {
        let ext = WeatherExtension::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 3);
        assert!(commands.iter().any(|c| c.name == "get_weather"));
        assert!(commands.iter().any(|c| c.name == "refresh"));
        assert!(commands.iter().any(|c| c.name == "set_default_city"));
    }

    #[test]
    fn test_produce_metrics_without_data() {
        let ext = WeatherExtension::new();
        let metrics = ext.produce_metrics().unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "request_count");
    }

    #[test]
    fn test_store_and_produce_metrics() {
        let ext = WeatherExtension::new();

        let weather = WeatherResult {
            city: "Test City".to_string(),
            country: Some("TC".to_string()),
            temperature_c: 25.5,
            feels_like_c: 26.0,
            humidity_percent: 65,
            wind_speed_kmph: 12.3,
            wind_direction_deg: 180,
            wind_direction: "S".to_string(),
            cloud_cover_percent: 30,
            pressure_hpa: 1013.25,
            description: "Partly cloudy".to_string(),
            is_day: true,
            timestamp: None,
        };
        ext.store_weather_metrics(&weather);

        let metrics = ext.produce_metrics().unwrap();
        assert_eq!(metrics.len(), 9);

        let temp_metric = metrics.iter().find(|m| m.name == "temperature_c").unwrap();
        if let ParamMetricValue::Float(temp) = temp_metric.value {
            assert!((temp - 25.5).abs() < 0.01);
        } else {
            panic!("Expected Float value for temperature");
        }
    }

    #[test]
    fn test_default_city() {
        let ext = WeatherExtension::new();
        assert_eq!(ext.get_default_city(), "Beijing");

        ext.set_default_city("Shanghai");
        assert_eq!(ext.get_default_city(), "Shanghai");
    }

    #[test]
    fn test_wind_direction() {
        assert_eq!(wind_direction_to_cardinal(0), "N");
        assert_eq!(wind_direction_to_cardinal(90), "E");
        assert_eq!(wind_direction_to_cardinal(180), "S");
        assert_eq!(wind_direction_to_cardinal(270), "W");
    }

    #[test]
    fn test_weather_code() {
        assert_eq!(weather_code_to_description(0), "Clear sky");
        assert_eq!(weather_code_to_description(3), "Overcast");
        assert_eq!(weather_code_to_description(63), "Rain");
    }
}
