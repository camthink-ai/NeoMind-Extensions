//! NeoMind LocateAnything Extension (V2)
//!
//! Visual grounding skill for AI agents - object detection, phrase grounding,
//! OCR text detection, GUI element grounding, and pointing via LocateAnything-3B.
//!
//! # Architecture
//!
//! This extension calls a separate LocateAnything Python service via HTTP.
//! The Python service loads the nvidia/LocateAnything-3B model and handles inference.
//!
//! Uses sync HTTP client (ureq) to avoid Tokio runtime issues in dynamic libraries.

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType,
    ParameterDefinition, ParamMetricValue, Result,
    CommandBuilder, MetricBuilder, ParamBuilder,
    metric_int, metric_float,
};
use serde_json::json;
use std::sync::atomic::{AtomicI64, AtomicBool, Ordering};
use parking_lot::RwLock;

// ============================================================================
// Default config values
// ============================================================================

const DEFAULT_SERVICE_URL: &str = "http://127.0.0.1:9380";
const DEFAULT_GENERATION_MODE: &str = "hybrid";
const DEFAULT_MAX_NEW_TOKENS: i64 = 2048;

// ============================================================================
// Extension
// ============================================================================

pub struct LocateAnythingExtension {
    /// Base URL of the LocateAnything Python service
    service_url: RwLock<String>,
    /// Inference generation mode: fast / slow / hybrid
    generation_mode: RwLock<String>,
    /// Max new tokens per inference
    max_new_tokens: RwLock<i64>,
    /// Whether the service is reachable
    service_ok: AtomicBool,
    /// Total inference requests
    total_requests: AtomicI64,
    /// Last inference time in ms
    last_inference_ms: AtomicI64,
    /// HTTP agent with 25s timeout (under 30s FFI limit)
    http_agent: ureq::Agent,
}

impl LocateAnythingExtension {
    pub fn new() -> Self {
        let service_url = std::env::var("LOCATE_ANYTHING_SERVICE_URL")
            .unwrap_or_else(|_| DEFAULT_SERVICE_URL.to_string());

        let http_agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(180)))
            .build()
            .into();

        Self {
            service_url: RwLock::new(service_url),
            generation_mode: RwLock::new(DEFAULT_GENERATION_MODE.to_string()),
            max_new_tokens: RwLock::new(DEFAULT_MAX_NEW_TOKENS),
            service_ok: AtomicBool::new(false),
            total_requests: AtomicI64::new(0),
            last_inference_ms: AtomicI64::new(0),
            http_agent,
        }
    }

    /// Check if the Python service is healthy
    fn check_health(&self) -> bool {
        let url = format!("{}/health", *self.service_url.read());
        match self.http_agent.get(&url).call() {
            Ok(resp) => {
                let ok = resp.status() == 200;
                self.service_ok.store(ok, Ordering::SeqCst);
                ok
            }
            Err(_) => {
                self.service_ok.store(false, Ordering::SeqCst);
                false
            }
        }
    }

    /// POST to the Python service and return the response
    fn call_service(&self, endpoint: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/{}", *self.service_url.read(), endpoint);

        let resp = self.http_agent.post(&url)
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Service call failed: {}", e)))?;

        let status = resp.status();
        if status != 200 {
            return Err(ExtensionError::ExecutionFailed(
                format!("Service returned status {}", status)
            ));
        }

        let result: serde_json::Value = resp.into_body().read_json()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to parse response: {}", e)))?;

        // Update metrics
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        if let Some(time_ms) = result.get("inference_time_ms").and_then(|v| v.as_f64()) {
            self.last_inference_ms.store(time_ms as i64, Ordering::SeqCst);
        }

        // Check success
        if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            Ok(result)
        } else {
            let error = result.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            Err(ExtensionError::ExecutionFailed(error.to_string()))
        }
    }

    /// Build the common body fields (generation_mode, max_new_tokens)
    fn inject_defaults(&self, body: &mut serde_json::Value) {
        let mode = self.generation_mode.read();
        let tokens = *self.max_new_tokens.read();
        if body.get("generation_mode").is_none() {
            body["generation_mode"] = json!(*mode);
        }
        if body.get("max_new_tokens").is_none() {
            body["max_new_tokens"] = json!(tokens);
        }
    }
}

impl Default for LocateAnythingExtension {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Extension for LocateAnythingExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("locate-anything-v2", "LocateAnything", env!("CARGO_PKG_VERSION"))
                .with_description("Visual grounding AI skill - object detection, phrase grounding, OCR, GUI element localization via LocateAnything-3B model")
                .with_author("NeoMind Team")
                .with_config_parameters(vec![
                    ParameterDefinition {
                        name: "service_url".to_string(),
                        display_name: "Service URL".to_string(),
                        description: "LocateAnything Python service URL (e.g. http://192.168.1.10:9380)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: Some(ParamMetricValue::String(DEFAULT_SERVICE_URL.to_string())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "generation_mode".to_string(),
                        display_name: "Generation Mode".to_string(),
                        description: "Inference mode: fast (MTP, fastest), slow (NTP, most stable), hybrid (fast with slow fallback)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String(DEFAULT_GENERATION_MODE.to_string())),
                        min: None,
                        max: None,
                        options: vec!["hybrid".into(), "fast".into(), "slow".into()],
                    },
                    ParameterDefinition {
                        name: "max_new_tokens".to_string(),
                        display_name: "Max Output Tokens".to_string(),
                        description: "Maximum number of tokens to generate per inference (higher = more detailed results, slower)".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(DEFAULT_MAX_NEW_TOKENS)),
                        min: Some(128.0),
                        max: Some(8192.0),
                        options: Vec::new(),
                    },
                ])
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("service_status", "Service Status")
                .integer()
                .build(),
            MetricBuilder::new("total_requests", "Total Requests")
                .integer()
                .unit("count")
                .build(),
            MetricBuilder::new("last_inference_time", "Last Inference Time")
                .float()
                .unit("ms")
                .build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("check_status")
                .display_name("Check Service Status")
                .description("Check if LocateAnything service is running and model is loaded")
                .sample(json!({}))
                .build(),

            CommandBuilder::new("detect")
                .display_name("Detect Objects")
                .description("Detect specified object categories in an image")
                .param(
                    ParamBuilder::new("image_base64", MetricDataType::String)
                        .display_name("Image (Base64)")
                        .description("Base64-encoded image (JPEG/PNG)")
                        .required()
                        .build()
                )
                .param(
                    ParamBuilder::new("categories", MetricDataType::String)
                        .display_name("Categories")
                        .description("Comma-separated object categories to detect (e.g. 'person,car,bicycle')")
                        .required()
                        .build()
                )
                .sample(json!({"image_base64": "<base64_data>", "categories": "person,car"}))
                .build(),

            CommandBuilder::new("ground")
                .display_name("Ground Phrase")
                .description("Locate objects matching a natural language description in an image")
                .param(
                    ParamBuilder::new("image_base64", MetricDataType::String)
                        .display_name("Image (Base64)")
                        .description("Base64-encoded image")
                        .required()
                        .build()
                )
                .param(
                    ParamBuilder::new("phrase", MetricDataType::String)
                        .display_name("Description")
                        .description("Natural language description of objects to locate")
                        .required()
                        .build()
                )
                .param(
                    ParamBuilder::new("mode", MetricDataType::String)
                        .display_name("Mode")
                        .description("single = one instance, multi = all instances")
                        .options(vec!["multi".into(), "single".into()])
                        .build()
                )
                .sample(json!({"image_base64": "<base64_data>", "phrase": "people wearing red shirts"}))
                .build(),

            CommandBuilder::new("detect_text")
                .display_name("Detect Text (OCR)")
                .description("Detect and localize all text in an image")
                .param(
                    ParamBuilder::new("image_base64", MetricDataType::String)
                        .display_name("Image (Base64)")
                        .description("Base64-encoded image")
                        .required()
                        .build()
                )
                .sample(json!({"image_base64": "<base64_data>"}))
                .build(),

            CommandBuilder::new("ground_gui")
                .display_name("GUI Grounding")
                .description("Locate a UI element in a screenshot (box or point)")
                .param(
                    ParamBuilder::new("image_base64", MetricDataType::String)
                        .display_name("Screenshot (Base64)")
                        .description("Base64-encoded screenshot")
                        .required()
                        .build()
                )
                .param(
                    ParamBuilder::new("phrase", MetricDataType::String)
                        .display_name("Element Description")
                        .description("Description of the UI element to locate")
                        .required()
                        .build()
                )
                .param(
                    ParamBuilder::new("output_type", MetricDataType::String)
                        .display_name("Output Type")
                        .description("Return bounding box or center point")
                        .options(vec!["box".into(), "point".into()])
                        .build()
                )
                .sample(json!({"image_base64": "<base64_data>", "phrase": "the search button"}))
                .build(),

            CommandBuilder::new("point")
                .display_name("Point to Object")
                .description("Point to a specific object in an image")
                .param(
                    ParamBuilder::new("image_base64", MetricDataType::String)
                        .display_name("Image (Base64)")
                        .description("Base64-encoded image")
                        .required()
                        .build()
                )
                .param(
                    ParamBuilder::new("phrase", MetricDataType::String)
                        .display_name("Object Description")
                        .description("Description of the object to point to")
                        .required()
                        .build()
                )
                .sample(json!({"image_base64": "<base64_data>", "phrase": "the traffic light"}))
                .build(),
        ]
    }

    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        // Update service URL
        if let Some(url) = config.get("service_url").and_then(|v| v.as_str()) {
            if !url.is_empty() {
                *self.service_url.write() = url.to_string();
                self.service_ok.store(false, Ordering::SeqCst);
                eprintln!("[LocateAnything] Service URL updated: {}", url);
            }
        }

        // Update generation mode
        if let Some(mode) = config.get("generation_mode").and_then(|v| v.as_str()) {
            match mode {
                "fast" | "slow" | "hybrid" => {
                    *self.generation_mode.write() = mode.to_string();
                    eprintln!("[LocateAnything] Generation mode: {}", mode);
                }
                _ => return Err(ExtensionError::InvalidArguments(
                    format!("Invalid generation_mode '{}'. Must be: fast, slow, or hybrid", mode)
                )),
            }
        }

        // Update max_new_tokens
        if let Some(tokens) = config.get("max_new_tokens").and_then(|v| v.as_i64()) {
            if tokens < 128 || tokens > 8192 {
                return Err(ExtensionError::InvalidArguments(
                    "max_new_tokens must be between 128 and 8192".to_string()
                ));
            }
            *self.max_new_tokens.write() = tokens;
            eprintln!("[LocateAnything] Max new tokens: {}", tokens);
        }

        Ok(())
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "check_status" => {
                let healthy = self.check_health();
                Ok(json!({
                    "service_url": *self.service_url.read(),
                    "generation_mode": *self.generation_mode.read(),
                    "max_new_tokens": *self.max_new_tokens.read(),
                    "healthy": healthy,
                    "total_requests": self.total_requests.load(Ordering::SeqCst),
                }))
            }

            "detect" => {
                let image_b64 = args.get("image_base64")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing image_base64".into()))?;
                let categories_str = args.get("categories")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing categories".into()))?;
                let categories: Vec<&str> = categories_str.split(',').map(|s| s.trim()).collect();

                let mut body = json!({
                    "image_base64": image_b64,
                    "categories": categories,
                });
                self.inject_defaults(&mut body);
                self.call_service("detect", &body)
            }

            "ground" => {
                let image_b64 = args.get("image_base64")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing image_base64".into()))?;
                let phrase = args.get("phrase")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing phrase".into()))?;
                let mode = args.get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("multi");

                let mut body = json!({
                    "image_base64": image_b64,
                    "phrase": phrase,
                    "mode": mode,
                });
                self.inject_defaults(&mut body);
                self.call_service("ground", &body)
            }

            "detect_text" => {
                let image_b64 = args.get("image_base64")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing image_base64".into()))?;

                let mut body = json!({
                    "image_base64": image_b64,
                });
                self.inject_defaults(&mut body);
                self.call_service("detect_text", &body)
            }

            "ground_gui" => {
                let image_b64 = args.get("image_base64")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing image_base64".into()))?;
                let phrase = args.get("phrase")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing phrase".into()))?;
                let output_type = args.get("output_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("box");

                let mut body = json!({
                    "image_base64": image_b64,
                    "phrase": phrase,
                    "output_type": output_type,
                });
                self.inject_defaults(&mut body);
                self.call_service("ground_gui", &body)
            }

            "point" => {
                let image_b64 = args.get("image_base64")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing image_base64".into()))?;
                let phrase = args.get("phrase")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing phrase".into()))?;

                let mut body = json!({
                    "image_base64": image_b64,
                    "phrase": phrase,
                });
                self.inject_defaults(&mut body);
                self.call_service("point", &body)
            }

            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            metric_int!("service_status", if self.service_ok.load(Ordering::SeqCst) { 1 } else { 0 }),
            metric_int!("total_requests", self.total_requests.load(Ordering::SeqCst)),
            metric_float!("last_inference_time", self.last_inference_ms.load(Ordering::SeqCst) as f64),
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// FFI Export
neomind_extension_sdk::neomind_export!(LocateAnythingExtension);

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = LocateAnythingExtension::new();
        assert_eq!(ext.metadata().id, "locate-anything-v2");
        // Config parameters should be defined
        assert!(ext.metadata().config_parameters.as_ref().map_or(false, |p| !p.is_empty()));
    }

    #[test]
    fn test_extension_commands() {
        let ext = LocateAnythingExtension::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 6);
    }

    #[test]
    fn test_extension_metrics() {
        let ext = LocateAnythingExtension::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 3);
    }

    #[test]
    fn test_configure_valid() {
        let mut ext = LocateAnythingExtension::new();
        let config = json!({
            "service_url": "http://192.168.1.100:9380",
            "generation_mode": "fast",
            "max_new_tokens": 1024
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(ext.configure(&config)).unwrap();
        assert_eq!(*ext.service_url.read(), "http://192.168.1.100:9380");
        assert_eq!(*ext.generation_mode.read(), "fast");
        assert_eq!(*ext.max_new_tokens.read(), 1024);
    }

    #[test]
    fn test_configure_invalid_mode() {
        let mut ext = LocateAnythingExtension::new();
        let config = json!({"generation_mode": "invalid"});
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(rt.block_on(ext.configure(&config)).is_err());
    }
}
