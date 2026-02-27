//! NeoMind Weather Forecast Extension (V2)
//!
//! Weather forecast extension using the unified SDK with ABI Version 3.

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};
use std::collections::HashMap;
use semver::Version;

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
    request_count: AtomicI64,
}

impl WeatherExtension {
    pub fn new() -> Self {
        Self {
            request_count: AtomicI64::new(0),
        }
    }

    /// Get weather for a city (Native version)
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_weather(&self, city: &str) -> Result<WeatherResult> {
        self.request_count.fetch_add(1, Ordering::SeqCst);

        let location = self.geocode(city).await
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;
        let weather = self.fetch_weather(&location).await
            .map_err(|e| ExtensionError::ExecutionFailed(e))?;
        Ok(weather)
    }

    /// Geocode city name to coordinates
    #[cfg(not(target_arch = "wasm32"))]
    async fn geocode(&self, city: &str) -> std::result::Result<GeoLocation, String> {
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
            urlencoding::encode(city)
        );

        let response: serde_json::Value = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?
            .json()
            .await
            .map_err(|e| format!("JSON error: {}", e))?;

        let geo_data: GeocodingResponse = serde_json::from_value(response)
            .map_err(|e| format!("Parse error: {}", e))?;

        geo_data
            .results
            .and_then(|mut v| v.pop())
            .ok_or_else(|| format!("City not found: {}", city))
    }

    /// Fetch weather data from API
    #[cfg(not(target_arch = "wasm32"))]
    async fn fetch_weather(&self, location: &GeoLocation) -> std::result::Result<WeatherResult, String> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,cloud_cover,pressure_msl,wind_speed_10m,wind_direction_10m,is_day&timezone=auto&windspeed_unit=kmh",
            location.latitude,
            location.longitude
        );

        let response: serde_json::Value = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?
            .json()
            .await
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
            ExtensionMetadata {
                id: "weather-forecast-v2".to_string(),
                name: "Weather Forecast V2".to_string(),
                version: Version::parse("2.0.0").unwrap(),
                description: Some("Weather forecast extension using unified SDK".to_string()),
                author: Some("NeoMind Team".to_string()),
                homepage: None,
                license: Some("Apache-2.0".to_string()),
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
                    name: "temperature_c".to_string(),
                    display_name: "Temperature".to_string(),
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
                    name: "get_weather".to_string(),
                    display_name: "Get Weather".to_string(),
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
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: HashMap::new(),
                    samples: vec![
                        json!({ "city": "Beijing" }),
                        json!({ "city": "Shanghai" }),
                    ],
                    llm_hints: "Get current weather for a city. Use this when user asks about weather conditions.".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "get_weather" => {
                let city = args.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'city' parameter".to_string()))?;

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let result = self.get_weather(city).await?;
                    Ok(serde_json::to_value(result)
                        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?)
                }

                #[cfg(target_arch = "wasm32")]
                {
                    Err(ExtensionError::NotSupported("WASM not yet supported".to_string()))
                }
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        Ok(vec![
            ExtensionMetricValue {
                name: "request_count".to_string(),
                value: ParamMetricValue::Integer(self.request_count.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn wind_direction_to_cardinal(degrees: i32) -> String {
    let directions = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE",
        "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW",
    ];
    let index = ((degrees + 11) / 23 % 16) as usize;
    directions[index].to_string()
}

fn weather_code_to_description(code: i32) -> String {
    match code {
        0 => "Clear sky".to_string(),
        1 => "Mainly clear".to_string(),
        2 => "Partly cloudy".to_string(),
        3 => "Overcast".to_string(),
        45 | 48 => "Fog".to_string(),
        51 => "Light drizzle".to_string(),
        53 => "Drizzle".to_string(),
        55 => "Heavy drizzle".to_string(),
        61 => "Slight rain".to_string(),
        63 => "Rain".to_string(),
        65 => "Heavy rain".to_string(),
        71 => "Slight snow".to_string(),
        73 => "Snow".to_string(),
        75 => "Heavy snow".to_string(),
        80 => "Slight showers".to_string(),
        81 => "Showers".to_string(),
        82 => "Heavy showers".to_string(),
        95 => "Thunderstorm".to_string(),
        96 | 99 => "Thunderstorm with hail".to_string(),
        _ => "Unknown".to_string(),
    }
}

// ============================================================================
// Export FFI using SDK macro
// ============================================================================

neomind_extension_sdk::neomind_export!(WeatherExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = WeatherExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "weather-forecast-v2");
        assert_eq!(meta.name, "Weather Forecast V2");
    }

    #[test]
    fn test_extension_metrics() {
        let ext = WeatherExtension::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 4);
    }

    #[test]
    fn test_extension_commands() {
        let ext = WeatherExtension::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "get_weather");
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
