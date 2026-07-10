//! NeoMind PaddleOCR-VL Extension
//!
//! Bridges NeoMind with a remote PaddleOCR-VL inference service for high-accuracy
//! OCR, table recognition, and key information extraction (KIE) on document images.
//!
//! # Architecture
//!
//! This extension is an HTTP client (sync `ureq`) — the actual PaddleOCR-VL model
//! runs in a separate Python service (FastAPI + PaddleOCRVL). This keeps the
//! extension cross-platform and lightweight (~MB), while the heavy VLM inference
//! happens on a GPU server.
//!
//! # Commands
//!
//! - `recognize`        — Plain text OCR, returns `text_blocks` compatible with
//!                        ne101_camera's `ocr_text_blocks` rendering.
//! - `recognize_table`  — Table structure recognition, returns HTML.
//! - `extract_keys`     — KIE: extract structured fields per a JSON schema.
//! - `health`           — Probe the backend service reachability.
//!
//! # Configuration Parameters
//!
//! - `endpoint`                  — Base URL of the PaddleOCR-VL service
//! - `language`                  — Language hint: `ch`, `en`, `japan`, `korean`, ...
//! - `use_doc_orientation_classify`
//! - `use_doc_unwarping`
//! - `timeout_ms`                — HTTP request timeout

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionCommand, ExtensionError, ExtensionMetadata, ExtensionMetricValue,
    MetricDataType, MetricDescriptor, MetricValue, ParameterDefinition, Result,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

// ============================================================================
// Configuration
// ============================================================================

/// Default PaddleOCR-VL service URL (local FastAPI server)
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8000";

/// Default request timeout — VLM inference is slower than traditional OCR
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaddleOcrConfig {
    endpoint: String,
    language: String,
    use_doc_orientation_classify: bool,
    use_doc_unwarping: bool,
    timeout_ms: u64,
}

impl Default for PaddleOcrConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            language: "ch".to_string(),
            use_doc_orientation_classify: false,
            use_doc_unwarping: false,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

// ============================================================================
// Response types (normalized for ne101 compatibility)
// ============================================================================

/// A single detected text region with normalized bbox (0..1).
/// Matches `ocr_text_blocks` shape consumed by ne101_camera.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    pub confidence: f64,
    /// Normalized bbox — top-left origin, fractional image dims
    pub bbox: NormalizedBBox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedBBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecognizeResult {
    pub text_blocks: Vec<TextBlock>,
    pub full_text: String,
    pub processing_time_ms: f64,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableResult {
    pub html: String,
    pub processing_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KieResult {
    pub fields: serde_json::Value,
    pub processing_time_ms: f64,
}

// ============================================================================
// Raw response shapes from the PaddleOCR-VL service
// ============================================================================

/// Raw item from PaddleOCR-VL `/ocr` response.
/// `bbox` is in pixel coordinates `[x1, y1, x2, y2]`.
#[derive(Debug, Deserialize)]
struct RawOcrItem {
    #[serde(default)]
    pub rec_text: Option<String>,
    #[serde(default)]
    pub rec_score: Option<f64>,
    #[serde(default)]
    pub dt_polynomial: Option<Vec<Vec<f64>>>,
    /// Fallback: some servers return `det_boxes` instead
    #[serde(default)]
    pub det_boxes: Option<Vec<Vec<f64>>>,
    /// Or already-flat bbox
    #[serde(default)]
    pub bbox: Option<Vec<f64>>,
}

#[derive(Debug, Deserialize)]
struct RawOcrResponse {
    #[serde(default)]
    pub results: Option<Vec<RawOcrItem>>,
    /// Alternate flat shape: list of items directly
    #[serde(default)]
    pub items: Option<Vec<RawOcrItem>>,
    #[serde(default)]
    pub processing_time_ms: Option<f64>,
}

impl RawOcrResponse {
    fn items(&self) -> Vec<&RawOcrItem> {
        if let Some(items) = self.results.as_ref() {
            items.iter().collect()
        } else if let Some(items) = self.items.as_ref() {
            items.iter().collect()
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawTableResponse {
    #[serde(default)]
    pub html: Option<String>,
    #[serde(default)]
    pub processing_time_ms: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawKieResponse {
    #[serde(default)]
    pub fields: Option<Value>,
    #[serde(default)]
    pub processing_time_ms: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawHealthResponse {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub model_loaded: Option<bool>,
}

// ============================================================================
// Extension
// ============================================================================

pub struct PaddleOcrVlExtension {
    config: RwLock<PaddleOcrConfig>,
    request_count: AtomicI64,
    success_count: AtomicI64,
    failure_count: AtomicI64,
    last_latency_ms: AtomicI64,
    last_recognized_block_count: AtomicI64,
    last_update_ts: AtomicI64,
    has_data: AtomicBool,
}

impl PaddleOcrVlExtension {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(PaddleOcrConfig::default()),
            request_count: AtomicI64::new(0),
            success_count: AtomicI64::new(0),
            failure_count: AtomicI64::new(0),
            last_latency_ms: AtomicI64::new(0),
            last_recognized_block_count: AtomicI64::new(0),
            last_update_ts: AtomicI64::new(0),
            has_data: AtomicBool::new(false),
        }
    }

    fn config_snapshot(&self) -> PaddleOcrConfig {
        self.config.read().clone()
    }

    // -----------------------------------------------------------------------
    // Argument helpers
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // HTTP helpers
    // -----------------------------------------------------------------------

    fn build_agent(timeout_ms: u64) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
    }

    fn post_json<T: for<'de> Deserialize<'de>>(
        endpoint: &str,
        path: &str,
        body: &Value,
        timeout_ms: u64,
    ) -> std::result::Result<T, String> {
        let url = format!("{}{}", endpoint.trim_end_matches('/'), path);
        let agent = Self::build_agent(timeout_ms);
        let resp = agent.post(&url).send_json(body.clone());
        match resp {
            Ok(r) => r.into_json::<T>().map_err(|e| format!("Parse error: {}", e)),
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string().unwrap_or_default();
                Err(format!("HTTP {}: {}", code, truncate(&body, 500)))
            }
            Err(e) => Err(format!("Request failed: {}", e)),
        }
    }

    // -----------------------------------------------------------------------
    // Command: recognize
    // -----------------------------------------------------------------------

    fn recognize_sync(
        &self,
        image_base64: Option<&str>,
        image_url: Option<&str>,
        image_width: Option<u64>,
        image_height: Option<u64>,
        language: Option<&str>,
        use_doc_orientation_classify: Option<bool>,
        use_doc_unwarping: Option<bool>,
    ) -> Result<RecognizeResult> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        let cfg = self.config_snapshot();
        let t0 = std::time::Instant::now();

        // Per-request args override config defaults; fall back to config.
        let lang = language.unwrap_or(&cfg.language);
        let rotate = use_doc_orientation_classify.unwrap_or(cfg.use_doc_orientation_classify);
        let unwarp = use_doc_unwarping.unwrap_or(cfg.use_doc_unwarping);

        let mut body = json!({
            "language": lang,
            "use_doc_orientation_classify": rotate,
            "use_doc_unwarping": unwarp,
        });
        if let Some(b64) = image_base64 {
            body["image_base64"] = json!(b64);
        } else if let Some(url) = image_url {
            body["image_url"] = json!(url);
        }

        let raw: RawOcrResponse = Self::post_json(
            &cfg.endpoint,
            "/ocr",
            &body,
            cfg.timeout_ms,
        )
        .map_err(|e| {
            self.failure_count.fetch_add(1, Ordering::SeqCst);
            ExtensionError::ExecutionFailed(format!("PaddleOCR-VL /ocr failed: {}", e))
        })?;

        let elapsed = t0.elapsed().as_millis() as f64;

        // Normalize items → TextBlock with normalized bbox.
        // If caller provided width/height we use it; otherwise we infer from the
        // max coordinate in the returned boxes (best-effort).
        let items = raw.items();
        let (img_w, img_h) = Self::resolve_image_dims(&items, image_width, image_height);

        let mut blocks = Vec::with_capacity(items.len());
        let mut full_text_parts = Vec::with_capacity(items.len());

        for item in items {
            let text = item.rec_text.clone().unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            let confidence = item.rec_score.unwrap_or(1.0);
            let bbox = if let Some(poly) = item.dt_polynomial.as_ref().filter(|p| !p.is_empty()) {
                Self::bbox_from_polygon(poly, img_w, img_h)
            } else if let Some(boxes) = item.det_boxes.as_ref().filter(|p| !p.is_empty()) {
                Self::bbox_from_polygon(boxes, img_w, img_h)
            } else if let Some(b) = item.bbox.as_ref().filter(|p| p.len() >= 4) {
                // Pixel coords [x1, y1, x2, y2]
                let x1 = b[0];
                let y1 = b[1];
                let x2 = b[2];
                let y2 = b[3];
                NormalizedBBox {
                    x: x1 / img_w,
                    y: y1 / img_h,
                    width: (x2 - x1) / img_w,
                    height: (y2 - y1) / img_h,
                }
            } else {
                // No bbox info — return zero-box; consumer should still display the text
                NormalizedBBox { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }
            };

            full_text_parts.push(text.clone());
            blocks.push(TextBlock { text, confidence, bbox });
        }

        let full_text = full_text_parts.join("\n");
        let block_count = blocks.len() as i64;

        let result = RecognizeResult {
            text_blocks: blocks,
            full_text,
            processing_time_ms: raw.processing_time_ms.unwrap_or(elapsed),
            language: lang.to_string(),
        };

        // Update metrics cache
        self.success_count.fetch_add(1, Ordering::SeqCst);
        self.last_latency_ms
            .store(elapsed as i64, Ordering::SeqCst);
        self.last_recognized_block_count.store(block_count, Ordering::SeqCst);
        self.last_update_ts
            .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        self.has_data.store(true, Ordering::SeqCst);

        Ok(result)
    }

    // -----------------------------------------------------------------------
    // Command: recognize_table
    // -----------------------------------------------------------------------

    fn recognize_table_sync(
        &self,
        image_base64: Option<&str>,
        image_url: Option<&str>,
    ) -> Result<TableResult> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        let cfg = self.config_snapshot();
        let t0 = std::time::Instant::now();

        let mut body = json!({});
        if let Some(b64) = image_base64 {
            body["image_base64"] = json!(b64);
        } else if let Some(url) = image_url {
            body["image_url"] = json!(url);
        }

        let raw: RawTableResponse = Self::post_json(&cfg.endpoint, "/table", &body, cfg.timeout_ms)
            .map_err(|e| {
                self.failure_count.fetch_add(1, Ordering::SeqCst);
                ExtensionError::ExecutionFailed(format!("PaddleOCR-VL /table failed: {}", e))
            })?;

        let elapsed = t0.elapsed().as_millis() as f64;
        self.success_count.fetch_add(1, Ordering::SeqCst);
        self.last_latency_ms.store(elapsed as i64, Ordering::SeqCst);
        self.last_update_ts
            .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        self.has_data.store(true, Ordering::SeqCst);

        Ok(TableResult {
            html: raw.html.unwrap_or_default(),
            processing_time_ms: raw.processing_time_ms.unwrap_or(elapsed),
        })
    }

    // -----------------------------------------------------------------------
    // Command: extract_keys
    // -----------------------------------------------------------------------

    fn extract_keys_sync(
        &self,
        image_base64: Option<&str>,
        image_url: Option<&str>,
        schema: Option<&Value>,
    ) -> Result<KieResult> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        let cfg = self.config_snapshot();
        let t0 = std::time::Instant::now();

        let mut body = json!({});
        if let Some(b64) = image_base64 {
            body["image_base64"] = json!(b64);
        } else if let Some(url) = image_url {
            body["image_url"] = json!(url);
        }
        if let Some(s) = schema {
            body["schema"] = s.clone();
        }

        let raw: RawKieResponse = Self::post_json(&cfg.endpoint, "/kie", &body, cfg.timeout_ms)
            .map_err(|e| {
                self.failure_count.fetch_add(1, Ordering::SeqCst);
                ExtensionError::ExecutionFailed(format!("PaddleOCR-VL /kie failed: {}", e))
            })?;

        let elapsed = t0.elapsed().as_millis() as f64;
        self.success_count.fetch_add(1, Ordering::SeqCst);
        self.last_latency_ms.store(elapsed as i64, Ordering::SeqCst);
        self.last_update_ts
            .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
        self.has_data.store(true, Ordering::SeqCst);

        Ok(KieResult {
            fields: raw.fields.unwrap_or(json!({})),
            processing_time_ms: raw.processing_time_ms.unwrap_or(elapsed),
        })
    }

    // -----------------------------------------------------------------------
    // Command: health
    // -----------------------------------------------------------------------

    fn health_sync(&self) -> Result<Value> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        let cfg = self.config_snapshot();

        let raw: RawHealthResponse = Self::post_json(&cfg.endpoint, "/health", &json!({}), 5_000)
            .map_err(|e| {
                self.failure_count.fetch_add(1, Ordering::SeqCst);
                ExtensionError::ExecutionFailed(format!("Health check failed: {}", e))
            })?;

        Ok(json!({
            "endpoint": cfg.endpoint,
            "status": raw.status.unwrap_or_else(|| "unknown".into()),
            "version": raw.version,
            "model_loaded": raw.model_loaded.unwrap_or(false),
        }))
    }

    // -----------------------------------------------------------------------
    // Geometry helpers
    // -----------------------------------------------------------------------

    /// Resolve (width, height) for bbox normalization.
    /// Strategy: caller-provided → infer from max box coordinate (fallback 1.0).
    fn resolve_image_dims(
        items: &[&RawOcrItem],
        width_hint: Option<u64>,
        height_hint: Option<u64>,
    ) -> (f64, f64) {
        if let (Some(w), Some(h)) = (width_hint, height_hint) {
            return (w as f64, h as f64);
        }
        // Infer from max x/y across all boxes
        let mut max_x = 1.0_f64;
        let mut max_y = 1.0_f64;
        for item in items {
            for coords in item.dt_polynomial.iter().chain(item.det_boxes.iter()).flatten() {
                if coords.len() >= 2 {
                    max_x = max_x.max(coords[0]);
                    max_y = max_y.max(coords[1]);
                }
            }
            if let Some(b) = item.bbox.as_ref() {
                if b.len() >= 4 {
                    max_x = max_x.max(b[2]);
                    max_y = max_y.max(b[3]);
                }
            }
        }
        (max_x.max(1.0), max_y.max(1.0))
    }

    /// Convert a polygon `[[x,y], [x,y], ...]` into an axis-aligned bbox.
    fn bbox_from_polygon(poly: &[Vec<f64>], img_w: f64, img_h: f64) -> NormalizedBBox {
        if poly.is_empty() {
            return NormalizedBBox { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
        }
        let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
        let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
        for pt in poly {
            if pt.len() < 2 {
                continue;
            }
            min_x = min_x.min(pt[0]);
            max_x = max_x.max(pt[0]);
            min_y = min_y.min(pt[1]);
            max_y = max_y.max(pt[1]);
        }
        let (w, h) = (img_w.max(1.0), img_h.max(1.0));
        NormalizedBBox {
            x: (min_x / w).clamp(0.0, 1.0),
            y: (min_y / h).clamp(0.0, 1.0),
            width: ((max_x - min_x) / w).clamp(0.0, 1.0),
            height: ((max_y - min_y) / h).clamp(0.0, 1.0),
        }
    }
}

impl Default for PaddleOcrVlExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension trait impl
// ============================================================================

#[async_trait]
impl Extension for PaddleOcrVlExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "paddle-ocr-vl",
                "PaddleOCR-VL",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description(
                "High-accuracy multilingual OCR, table recognition, and key information \
                 extraction powered by a remote PaddleOCR-VL inference service",
            )
            .with_author("NeoMind Team")
            .with_config_parameters(vec![
                ParameterDefinition {
                    name: "endpoint".to_string(),
                    display_name: "Service Endpoint".to_string(),
                    description: "Base URL of the PaddleOCR-VL HTTP service".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(MetricValue::String(DEFAULT_ENDPOINT.into())),
                    min: None,
                    max: None,
                    options: vec![],
                },
                ParameterDefinition {
                    name: "language".to_string(),
                    display_name: "Language".to_string(),
                    description: "OCR language hint".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(MetricValue::String("ch".into())),
                    min: None,
                    max: None,
                    options: vec![
                        "ch".into(),
                        "en".into(),
                        "japan".into(),
                        "korean".into(),
                        "german".into(),
                        "french".into(),
                    ],
                },
                ParameterDefinition {
                    name: "use_doc_orientation_classify".to_string(),
                    display_name: "Auto-rotate".to_string(),
                    description: "Run document orientation classification before OCR".to_string(),
                    param_type: MetricDataType::Boolean,
                    required: false,
                    default_value: Some(MetricValue::Boolean(false)),
                    min: None,
                    max: None,
                    options: vec![],
                },
                ParameterDefinition {
                    name: "use_doc_unwarping".to_string(),
                    display_name: "De-warp".to_string(),
                    description: "Run document unwarping (curved/photographed documents)".to_string(),
                    param_type: MetricDataType::Boolean,
                    required: false,
                    default_value: Some(MetricValue::Boolean(false)),
                    min: None,
                    max: None,
                    options: vec![],
                },
                ParameterDefinition {
                    name: "timeout_ms".to_string(),
                    display_name: "Request Timeout (ms)".to_string(),
                    description: "HTTP timeout in milliseconds".to_string(),
                    param_type: MetricDataType::Integer,
                    required: false,
                    default_value: Some(MetricValue::Integer(DEFAULT_TIMEOUT_MS as i64)),
                    min: Some(1_000.0),
                    max: Some(120_000.0),
                    options: vec![],
                },
            ])
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "request_count".to_string(),
                display_name: "Request Count".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "success_count".to_string(),
                display_name: "Success Count".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "failure_count".to_string(),
                display_name: "Failure Count".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "last_latency_ms".to_string(),
                display_name: "Last Latency (ms)".to_string(),
                data_type: MetricDataType::Integer,
                unit: "ms".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "last_recognized_block_count".to_string(),
                display_name: "Last Recognized Blocks".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "recognize".to_string(),
                display_name: "Recognize Text".to_string(),
                description: "Run OCR on an image and return text blocks with bbox".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "image_base64".to_string(),
                        display_name: "Image (Base64)".to_string(),
                        description: "Base64-encoded image bytes (preferred)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "image_url".to_string(),
                        display_name: "Image URL".to_string(),
                        description: "URL the server will fetch (alternative to base64)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "image_width".to_string(),
                        display_name: "Image Width".to_string(),
                        description: "Pixel width — improves bbox normalization accuracy".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: None,
                        min: Some(1.0),
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "image_height".to_string(),
                        display_name: "Image Height".to_string(),
                        description: "Pixel height — improves bbox normalization accuracy".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: None,
                        min: Some(1.0),
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "language".to_string(),
                        display_name: "Language Hint".to_string(),
                        description: "OCR language hint: ch (Chinese), en (English), japan, korean, multilingual. Informational for the VLM model.".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![
                            "ch".to_string(),
                            "en".to_string(),
                            "japan".to_string(),
                            "korean".to_string(),
                            "multilingual".to_string(),
                        ],
                    },
                    ParameterDefinition {
                        name: "use_doc_orientation_classify".to_string(),
                        display_name: "Auto-rotate".to_string(),
                        description: "Run orientation classification and auto-rotate before OCR".to_string(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "use_doc_unwarping".to_string(),
                        display_name: "De-warp".to_string(),
                        description: "Run document unwarping (curved/photographed documents)".to_string(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({
                    "image_base64": "<base64-bytes>",
                    "image_width": 1920,
                    "image_height": 1080,
                    "language": "ch",
                    "use_doc_orientation_classify": false,
                    "use_doc_unwarping": false
                })],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "recognize_table".to_string(),
                display_name: "Recognize Table".to_string(),
                description: "Recognize table structure and return HTML".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "image_base64".to_string(),
                        display_name: "Image (Base64)".to_string(),
                        description: "Base64-encoded image bytes".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "image_url".to_string(),
                        display_name: "Image URL".to_string(),
                        description: "URL the server will fetch".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({ "image_base64": "<base64-bytes>" })],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "extract_keys".to_string(),
                display_name: "Extract Key Info".to_string(),
                description: "Extract structured fields per a JSON schema (KIE)".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "image_base64".to_string(),
                        display_name: "Image (Base64)".to_string(),
                        description: "Base64-encoded image bytes".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "image_url".to_string(),
                        display_name: "Image URL".to_string(),
                        description: "URL the server will fetch".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "schema".to_string(),
                        display_name: "Extraction Schema".to_string(),
                        description: "JSON schema describing the fields to extract".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({
                    "image_base64": "<base64-bytes>",
                    "schema": { "fields": ["invoice_no", "date", "total"] }
                })],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "health".to_string(),
                display_name: "Health Check".to_string(),
                description: "Probe the PaddleOCR-VL backend service".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: vec![],
            },
        ]
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "recognize" => {
                let image_base64 = args.get("image_base64").and_then(|v| v.as_str());
                let image_url = args.get("image_url").and_then(|v| v.as_str());
                let image_width = args.get("image_width").and_then(|v| v.as_u64());
                let image_height = args.get("image_height").and_then(|v| v.as_u64());
                let language = args.get("language").and_then(|v| v.as_str());
                let use_doc_orientation_classify = args.get("use_doc_orientation_classify").and_then(|v| v.as_bool());
                let use_doc_unwarping = args.get("use_doc_unwarping").and_then(|v| v.as_bool());

                if image_base64.is_none() && image_url.is_none() {
                    return Err(ExtensionError::InvalidArguments(
                        "Either 'image_base64' or 'image_url' is required".into(),
                    ));
                }

                let result = self.recognize_sync(
                    image_base64,
                    image_url,
                    image_width,
                    image_height,
                    language,
                    use_doc_orientation_classify,
                    use_doc_unwarping,
                )?;
                Ok(serde_json::to_value(&result).map_err(|e| {
                    ExtensionError::ExecutionFailed(format!("Serialize failed: {}", e))
                })?)
            }
            "recognize_table" => {
                let image_base64 = args.get("image_base64").and_then(|v| v.as_str());
                let image_url = args.get("image_url").and_then(|v| v.as_str());

                if image_base64.is_none() && image_url.is_none() {
                    return Err(ExtensionError::InvalidArguments(
                        "Either 'image_base64' or 'image_url' is required".into(),
                    ));
                }

                let result = self.recognize_table_sync(image_base64, image_url)?;
                Ok(serde_json::to_value(&result).map_err(|e| {
                    ExtensionError::ExecutionFailed(format!("Serialize failed: {}", e))
                })?)
            }
            "extract_keys" => {
                let image_base64 = args.get("image_base64").and_then(|v| v.as_str());
                let image_url = args.get("image_url").and_then(|v| v.as_str());
                let schema = args.get("schema");

                if image_base64.is_none() && image_url.is_none() {
                    return Err(ExtensionError::InvalidArguments(
                        "Either 'image_base64' or 'image_url' is required".into(),
                    ));
                }

                let result = self.extract_keys_sync(image_base64, image_url, schema)?;
                Ok(serde_json::to_value(&result).map_err(|e| {
                    ExtensionError::ExecutionFailed(format!("Serialize failed: {}", e))
                })?)
            }
            "health" => {
                let result = self.health_sync()?;
                Ok(result)
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    /// Apply runtime configuration changes (called by the host).
    async fn configure(&mut self, config: &Value) -> Result<()> {
        let mut current = self.config_snapshot();
        if let Some(endpoint) = config.get("endpoint").and_then(|v| v.as_str()) {
            if !endpoint.is_empty() {
                current.endpoint = endpoint.to_string();
            }
        }
        if let Some(language) = config.get("language").and_then(|v| v.as_str()) {
            current.language = language.to_string();
        }
        if let Some(v) = config.get("use_doc_orientation_classify").and_then(|v| v.as_bool()) {
            current.use_doc_orientation_classify = v;
        }
        if let Some(v) = config.get("use_doc_unwarping").and_then(|v| v.as_bool()) {
            current.use_doc_unwarping = v;
        }
        if let Some(v) = config.get("timeout_ms").and_then(|v| v.as_u64()) {
            if v >= 1000 && v <= 120_000 {
                current.timeout_ms = v;
            }
        }
        *self.config.write() = current;
        Ok(())
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut metrics = vec![
            ExtensionMetricValue {
                name: "request_count".to_string(),
                value: MetricValue::Integer(self.request_count.load(Ordering::SeqCst)),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "success_count".to_string(),
                value: MetricValue::Integer(self.success_count.load(Ordering::SeqCst)),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "failure_count".to_string(),
                value: MetricValue::Integer(self.failure_count.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ];

        if self.has_data.load(Ordering::SeqCst) {
            metrics.push(ExtensionMetricValue {
                name: "last_latency_ms".to_string(),
                value: MetricValue::Integer(self.last_latency_ms.load(Ordering::SeqCst)),
                timestamp: now,
            });
            metrics.push(ExtensionMetricValue {
                name: "last_recognized_block_count".to_string(),
                value: MetricValue::Integer(
                    self.last_recognized_block_count.load(Ordering::SeqCst),
                ),
                timestamp: now,
            });
        }

        Ok(metrics)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Utilities
// ============================================================================

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...(truncated)", &s[..max])
    }
}

// FFI Export — single macro
neomind_extension_sdk::neomind_export!(PaddleOcrVlExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_basic() {
        let ext = PaddleOcrVlExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "paddle-ocr-vl");
        assert!(!meta.name.is_empty());
    }

    #[test]
    fn test_metadata_config_parameters() {
        let ext = PaddleOcrVlExtension::new();
        let params = ext.metadata().config_parameters.as_ref().unwrap();
        let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"endpoint"));
        assert!(names.contains(&"language"));
        assert!(names.contains(&"timeout_ms"));
    }

    #[test]
    fn test_commands_present() {
        let ext = PaddleOcrVlExtension::new();
        let cmds = ext.commands();
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"recognize"));
        assert!(names.contains(&"recognize_table"));
        assert!(names.contains(&"extract_keys"));
        assert!(names.contains(&"health"));
    }

    #[test]
    fn test_metrics_descriptors() {
        let ext = PaddleOcrVlExtension::new();
        let metrics = ext.metrics();
        assert!(metrics.iter().any(|m| m.name == "request_count"));
        assert!(metrics.iter().any(|m| m.name == "last_latency_ms"));
    }

    #[test]
    fn test_default_config() {
        let cfg = PaddleOcrConfig::default();
        assert_eq!(cfg.language, "ch");
        assert_eq!(cfg.timeout_ms, DEFAULT_TIMEOUT_MS);
    }

    #[tokio::test]
    async fn test_configure_updates_endpoint() {
        let mut ext = PaddleOcrVlExtension::new();
        let new_config = json!({
            "endpoint": "http://10.0.0.5:9000",
            "language": "en",
            "timeout_ms": 60000
        });
        ext.configure(&new_config).await.unwrap();
        let cfg = ext.config_snapshot();
        assert_eq!(cfg.endpoint, "http://10.0.0.5:9000");
        assert_eq!(cfg.language, "en");
        assert_eq!(cfg.timeout_ms, 60_000);
    }

    #[tokio::test]
    async fn test_configure_ignores_invalid_timeout() {
        let mut ext = PaddleOcrVlExtension::new();
        let original = ext.config_snapshot();
        ext.configure(&json!({ "timeout_ms": 10 })).await.unwrap();
        let after = ext.config_snapshot();
        assert_eq!(after.timeout_ms, original.timeout_ms, "out-of-range timeout should be ignored");
    }

    #[test]
    fn test_bbox_from_polygon() {
        let poly = vec![
            vec![10.0, 20.0],
            vec![110.0, 20.0],
            vec![110.0, 80.0],
            vec![10.0, 80.0],
        ];
        let bbox = PaddleOcrVlExtension::bbox_from_polygon(&poly, 200.0, 100.0);
        assert!((bbox.x - 0.05).abs() < 1e-9);
        assert!((bbox.y - 0.2).abs() < 1e-9);
        assert!((bbox.width - 0.5).abs() < 1e-9);
        assert!((bbox.height - 0.6).abs() < 1e-9);
    }

    #[test]
    fn test_resolve_image_dims_uses_hints() {
        let items: Vec<&RawOcrItem> = vec![];
        let (w, h) = PaddleOcrVlExtension::resolve_image_dims(&items, Some(1920), Some(1080));
        assert_eq!(w, 1920.0);
        assert_eq!(h, 1080.0);
    }

    #[test]
    fn test_resolve_image_dims_infers_from_coords() {
        let item = RawOcrItem {
            rec_text: None,
            rec_score: None,
            dt_polynomial: Some(vec![vec![0.0, 0.0], vec![1280.0, 720.0]]),
            det_boxes: None,
            bbox: None,
        };
        let items = vec![&item];
        let (w, h) = PaddleOcrVlExtension::resolve_image_dims(&items, None, None);
        assert_eq!(w, 1280.0);
        assert_eq!(h, 720.0);
    }

    #[tokio::test]
    async fn test_execute_health_no_endpoint_returns_execution_error() {
        // Default endpoint is unreachable in test env — should return ExecutionFailed,
        // not panic.
        let ext = PaddleOcrVlExtension::new();
        let result = ext.execute_command("health", &json!({})).await;
        assert!(matches!(result, Err(ExtensionError::ExecutionFailed(_))));
    }

    #[tokio::test]
    async fn test_recognize_requires_image() {
        let ext = PaddleOcrVlExtension::new();
        let result = ext.execute_command("recognize", &json!({})).await;
        assert!(matches!(result, Err(ExtensionError::InvalidArguments(_))));
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = PaddleOcrVlExtension::new();
        let result = ext.execute_command("nonexistent", &json!({})).await;
        assert!(matches!(result, Err(ExtensionError::CommandNotFound(_))));
    }
}
