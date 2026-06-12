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
const DEFAULT_GENERATION_MODE: &str = "slow";
const DEFAULT_MAX_NEW_TOKENS: i64 = 2048;
const DEFAULT_NMS_IOU_THRESHOLD: f64 = 0.7;
const DEFAULT_MIN_AREA_RATIO: f64 = 0.0005;  // 0.05% of image area
const DEFAULT_MAX_AREA_RATIO: f64 = 0.98;    // 98% of image area

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
    /// NMS IoU threshold (boxes with IoU > threshold are suppressed)
    nms_iou_threshold: RwLock<f64>,
    /// Minimum box area ratio relative to image area
    min_area_ratio: RwLock<f64>,
    /// Maximum box area ratio relative to image area
    max_area_ratio: RwLock<f64>,
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
            nms_iou_threshold: RwLock::new(DEFAULT_NMS_IOU_THRESHOLD),
            min_area_ratio: RwLock::new(DEFAULT_MIN_AREA_RATIO),
            max_area_ratio: RwLock::new(DEFAULT_MAX_AREA_RATIO),
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

    /// Apply post-processing with per-command overrides from args.
    /// Reads nms_iou_threshold / min_area_ratio / max_area_ratio from args if present,
    /// falling back to global config defaults.
    fn postprocess_args(&self, result: &mut serde_json::Value, args: &serde_json::Value) {
        let iou_threshold = args.get("nms_iou_threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(*self.nms_iou_threshold.read());
        let min_ratio = args.get("min_area_ratio")
            .and_then(|v| v.as_f64())
            .unwrap_or(*self.min_area_ratio.read());
        let max_ratio = args.get("max_area_ratio")
            .and_then(|v| v.as_f64())
            .unwrap_or(*self.max_area_ratio.read());

        self.postprocess_result(result, iou_threshold, min_ratio, max_ratio);
    }
}

impl Default for LocateAnythingExtension {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Bounding box types and post-processing
// ============================================================================

#[derive(Clone, Debug)]
struct BBox {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl BBox {
    fn area(&self) -> f64 {
        let w = (self.x2 - self.x1).max(0.0);
        let h = (self.y2 - self.y1).max(0.0);
        w * h
    }

    fn iou(&self, other: &BBox) -> f64 {
        let x1 = self.x1.max(other.x1);
        let y1 = self.y1.max(other.y1);
        let x2 = self.x2.min(other.x2);
        let y2 = self.y2.min(other.y2);

        let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        if inter == 0.0 {
            return 0.0;
        }

        let union = self.area() + other.area() - inter;
        if union <= 0.0 {
            return 0.0;
        }

        inter / union
    }
}

/// Non-Maximum Suppression: remove overlapping boxes keeping the larger one.
/// Boxes are sorted by area (descending), then suppressed by IoU threshold.
/// Returns boxes in original order.
fn nms(boxes: Vec<BBox>, iou_threshold: f64) -> Vec<BBox> {
    if boxes.is_empty() {
        return Vec::new();
    }

    // Sort by area descending (larger boxes have priority)
    let mut indexed: Vec<(usize, BBox)> = boxes.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| {
        b.1.area().partial_cmp(&a.1.area()).unwrap_or(std::cmp::Ordering::Equal)
    });

    let n = indexed.len();
    let mut keep = vec![true; n];

    for i in 0..n {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..n {
            if !keep[j] {
                continue;
            }
            if indexed[i].1.iou(&indexed[j].1) > iou_threshold {
                keep[j] = false;
            }
        }
    }

    // Collect kept items, then sort by original index to preserve order
    let mut kept: Vec<(usize, BBox)> = indexed.into_iter()
        .zip(keep.iter())
        .filter(|(_, &k)| k)
        .map(|((idx, bbox), _)| (idx, bbox))
        .collect();
    kept.sort_by_key(|(idx, _)| *idx);
    kept.into_iter().map(|(_, bbox)| bbox).collect()
}

/// Filter boxes by area ratio relative to image dimensions.
fn filter_by_area(boxes: Vec<BBox>, img_w: f64, img_h: f64, min_ratio: f64, max_ratio: f64) -> Vec<BBox> {
    let image_area = img_w * img_h;
    if image_area <= 0.0 {
        return boxes;
    }

    boxes.into_iter().filter(|b| {
        let ratio = b.area() / image_area;
        ratio >= min_ratio && ratio <= max_ratio
    }).collect()
}

impl LocateAnythingExtension {
    /// Apply NMS and area filtering to the service response.
    /// Extracts boxes from the response JSON, filters them, and updates the response.
    fn postprocess_result(
        &self,
        result: &mut serde_json::Value,
        iou_threshold: f64,
        min_area_ratio: f64,
        max_area_ratio: f64,
    ) {
        // Extract image dimensions from response
        let img_w = result.get("image_width")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let img_h = result.get("image_height")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Parse boxes from response
        let boxes_arr = match result.get("boxes").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return,
        };

        if boxes_arr.is_empty() {
            return;
        }

        let original_count = boxes_arr.len();
        let mut bboxes: Vec<BBox> = Vec::with_capacity(boxes_arr.len());
        for b in boxes_arr {
            let x1 = b.get("x1").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y1 = b.get("y1").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let x2 = b.get("x2").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y2 = b.get("y2").and_then(|v| v.as_f64()).unwrap_or(0.0);
            bboxes.push(BBox { x1, y1, x2, y2 });
        }

        // Step 1: Area filtering
        if img_w > 0.0 && img_h > 0.0 {
            bboxes = filter_by_area(bboxes, img_w, img_h, min_area_ratio, max_area_ratio);
        }

        // Step 2: NMS
        bboxes = nms(bboxes, iou_threshold);

        let filtered_count = bboxes.len();

        // Update response
        result["boxes"] = json!(bboxes.iter().map(|b| json!({
            "x1": b.x1, "y1": b.y1, "x2": b.x2, "y2": b.y2
        })).collect::<Vec<_>>());

        // Also strip filtered boxes from the text answer
        // (we can't easily re-parse the answer text, so just note the filtering)
        if filtered_count < original_count {
            result["filtered_count"] = json!(original_count - filtered_count);
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "postprocess".to_string(),
                    json!({
                        "nms_iou_threshold": iou_threshold,
                        "min_area_ratio": min_area_ratio,
                        "max_area_ratio": max_area_ratio,
                        "original_count": original_count,
                        "kept_count": filtered_count,
                        "removed_count": original_count - filtered_count,
                    }),
                );
            }
        }
    }
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
                    ParameterDefinition {
                        name: "nms_iou_threshold".to_string(),
                        display_name: "NMS IoU Threshold".to_string(),
                        description: "Non-Maximum Suppression IoU threshold. Overlapping boxes with IoU above this value are merged. Lower = more aggressive filtering (0.0 = keep all, 1.0 = no filtering)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(DEFAULT_NMS_IOU_THRESHOLD)),
                        min: Some(0.0),
                        max: Some(1.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "min_area_ratio".to_string(),
                        display_name: "Min Area Ratio".to_string(),
                        description: "Minimum box area as ratio of image area. Boxes smaller than this are filtered out (e.g. 0.001 = 0.1% of image)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(DEFAULT_MIN_AREA_RATIO)),
                        min: Some(0.0),
                        max: Some(0.5),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "max_area_ratio".to_string(),
                        display_name: "Max Area Ratio".to_string(),
                        description: "Maximum box area as ratio of image area. Boxes larger than this are filtered out (e.g. 0.95 = filter near-full-image boxes)".to_string(),
                        param_type: MetricDataType::Float,
                        required: false,
                        default_value: Some(ParamMetricValue::Float(DEFAULT_MAX_AREA_RATIO)),
                        min: Some(0.1),
                        max: Some(1.0),
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

        // Update NMS IoU threshold
        if let Some(threshold) = config.get("nms_iou_threshold").and_then(|v| v.as_f64()) {
            if threshold < 0.0 || threshold > 1.0 {
                return Err(ExtensionError::InvalidArguments(
                    "nms_iou_threshold must be between 0.0 and 1.0".to_string()
                ));
            }
            *self.nms_iou_threshold.write() = threshold;
            eprintln!("[LocateAnything] NMS IoU threshold: {}", threshold);
        }

        // Update min area ratio
        if let Some(ratio) = config.get("min_area_ratio").and_then(|v| v.as_f64()) {
            if ratio < 0.0 || ratio > 0.5 {
                return Err(ExtensionError::InvalidArguments(
                    "min_area_ratio must be between 0.0 and 0.5".to_string()
                ));
            }
            *self.min_area_ratio.write() = ratio;
            eprintln!("[LocateAnything] Min area ratio: {}", ratio);
        }

        // Update max area ratio
        if let Some(ratio) = config.get("max_area_ratio").and_then(|v| v.as_f64()) {
            if ratio < 0.1 || ratio > 1.0 {
                return Err(ExtensionError::InvalidArguments(
                    "max_area_ratio must be between 0.1 and 1.0".to_string()
                ));
            }
            *self.max_area_ratio.write() = ratio;
            eprintln!("[LocateAnything] Max area ratio: {}", ratio);
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
                let mut result = self.call_service("detect", &body)?;
                self.postprocess_args(&mut result, args);
                Ok(result)
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
                let mut result = self.call_service("ground", &body)?;
                self.postprocess_args(&mut result, args);
                Ok(result)
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
                let mut result = self.call_service("ground_gui", &body)?;
                self.postprocess_args(&mut result, args);
                Ok(result)
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

    #[test]
    fn test_configure_nms_params() {
        let mut ext = LocateAnythingExtension::new();
        let config = json!({
            "nms_iou_threshold": 0.3,
            "min_area_ratio": 0.005,
            "max_area_ratio": 0.8
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(ext.configure(&config)).unwrap();
        assert!((*ext.nms_iou_threshold.read() - 0.3).abs() < f64::EPSILON);
        assert!((*ext.min_area_ratio.read() - 0.005).abs() < f64::EPSILON);
        assert!((*ext.max_area_ratio.read() - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_area() {
        let bbox = BBox { x1: 10.0, y1: 20.0, x2: 60.0, y2: 70.0 };
        assert!((bbox.area() - 2500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_iou_no_overlap() {
        let a = BBox { x1: 0.0, y1: 0.0, x2: 10.0, y2: 10.0 };
        let b = BBox { x1: 20.0, y1: 20.0, x2: 30.0, y2: 30.0 };
        assert!(a.iou(&b).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_iou_full_overlap() {
        let a = BBox { x1: 0.0, y1: 0.0, x2: 10.0, y2: 10.0 };
        assert!((a.iou(&a) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_iou_partial() {
        let a = BBox { x1: 0.0, y1: 0.0, x2: 10.0, y2: 10.0 };
        let b = BBox { x1: 5.0, y1: 5.0, x2: 15.0, y2: 15.0 };
        // Intersection: 5x5 = 25, Union: 100 + 100 - 25 = 175
        let expected = 25.0 / 175.0;
        assert!((a.iou(&b) - expected).abs() < 1e-10);
    }

    #[test]
    fn test_nms_removes_overlapping() {
        let boxes = vec![
            BBox { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0 },   // large
            BBox { x1: 5.0, y1: 5.0, x2: 95.0, y2: 95.0 },     // 90% overlap with large
            BBox { x1: 200.0, y1: 200.0, x2: 300.0, y2: 300.0 }, // separate
        ];
        let result = nms(boxes, 0.5);
        assert_eq!(result.len(), 2);
        // The larger box (0,0,100,100) should be kept, the overlapping one removed
        assert!((result[0].x1 - 0.0).abs() < f64::EPSILON);
        assert!((result[1].x1 - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_nms_keeps_non_overlapping() {
        let boxes = vec![
            BBox { x1: 0.0, y1: 0.0, x2: 50.0, y2: 50.0 },
            BBox { x1: 100.0, y1: 100.0, x2: 150.0, y2: 150.0 },
            BBox { x1: 200.0, y1: 200.0, x2: 250.0, y2: 250.0 },
        ];
        let result = nms(boxes, 0.5);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_nms_empty() {
        let result: Vec<BBox> = nms(vec![], 0.5);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_by_area_removes_small() {
        // Image: 1000x1000 = 1_000_000 area
        let boxes = vec![
            BBox { x1: 0.0, y1: 0.0, x2: 10.0, y2: 10.0 },       // 100 / 1M = 0.0001 (too small)
            BBox { x1: 100.0, y1: 100.0, x2: 300.0, y2: 300.0 },  // 40000 / 1M = 0.04 (ok)
            BBox { x1: 0.0, y1: 0.0, x2: 990.0, y2: 990.0 },     // 980100 / 1M = 0.98 (too large)
        ];
        let result = filter_by_area(boxes, 1000.0, 1000.0, 0.001, 0.95);
        assert_eq!(result.len(), 1);
        assert!((result[0].x1 - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_filter_by_area_keeps_all_valid() {
        let boxes = vec![
            BBox { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0 },   // 1%
            BBox { x1: 200.0, y1: 200.0, x2: 400.0, y2: 400.0 }, // 4%
        ];
        let result = filter_by_area(boxes, 1000.0, 1000.0, 0.001, 0.95);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_postprocess_result_integration() {
        let ext = LocateAnythingExtension::new();
        let mut result = json!({
            "success": true,
            "image_width": 1000,
            "image_height": 1000,
            "boxes": [
                {"x1": 10, "y1": 10, "x2": 15, "y2": 15},          // too small: 25/1M = 0.000025
                {"x1": 100, "y1": 100, "x2": 300, "y2": 300},      // ok: 40000/1M = 0.04
                {"x1": 105, "y1": 105, "x2": 295, "y2": 295},      // overlaps with above
                {"x1": 0, "y1": 0, "x2": 990, "y2": 990},          // too large: 980100/1M = 0.98
            ]
        });

        ext.postprocess_result(&mut result, 0.5, 0.001, 0.95);

        let boxes = result["boxes"].as_array().unwrap();
        // Should keep only one box (the 200x200 one after area filter + NMS)
        assert_eq!(boxes.len(), 1);
        assert!(result.get("postprocess").is_some());
        assert_eq!(result["postprocess"]["removed_count"], 3);
    }
}
