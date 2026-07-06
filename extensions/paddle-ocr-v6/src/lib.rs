//! paddle-ocr-v6 — PP-OCRv6 native ONNX inference extension.
//!
//! Architecture: this extension ONLY exposes inference commands
//! (`recognize`, `switch_tier`, `health`). It does NOT bind to NeoMind
//! devices — that's the upper layer's job. The upper layer calls
//! `recognize` with image bytes + ROI and gets back text + bboxes.
//!
//! Internally: OcrEngine holds a single detector (DB) + single
//! multilingual recognizer (SVTR). Tier switching reloads both
//! models. Tiny tier ships in the .nep; small/medium lazy-download
//! from HuggingFace on first switch.

mod downloader;
mod preset;
mod tier;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::io::Read;

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionCommand, ExtensionError, ExtensionMetadata, MetricDataType,
    MetricDescriptor, MetricValue, ParameterDefinition, Result,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::downloader::Downloader;
use crate::tier::Tier;

// ---------------------------------------------------------------------------
// Result types (returned by `recognize`)
// ---------------------------------------------------------------------------

/// Normalized bounding box. All coordinates in `[0, 1]` relative to
/// the source image dimensions, so callers don't need to know pixel
/// dimensions to render overlays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One recognized text region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
    /// Raw detection polygon in pixel coordinates of the source image.
    /// Useful for non-axis-aligned text (rotated, curved). Optional
    /// because some detections may collapse to a degenerate polygon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polygon: Option<Vec<[f32; 2]>>,
}

/// Result of a `recognize` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub text_blocks: Vec<TextBlock>,
    pub full_text: String,
    pub total_blocks: usize,
    pub avg_confidence: f32,
    pub processing_time_ms: u64,
    pub image_width: u32,
    pub image_height: u32,
    /// Active tier when this result was produced. Lets callers
    /// correlate accuracy/speed with tier choice.
    pub tier: String,
}

// ---------------------------------------------------------------------------
// Geometry helpers (copied from ocr-device-inference, simplified)
// ---------------------------------------------------------------------------

/// Crop the axis-aligned bounding rectangle of a detection polygon.
/// Returns None for crops smaller than 8x8 (too small for SVTR).
fn crop_polygon(img: &usls::Image, polygon: &usls::Polygon) -> Option<usls::Image> {
    let coords = polygon.points();
    if coords.is_empty() {
        return None;
    }

    let xs: Vec<f32> = coords.iter().map(|p| p[0]).collect();
    let ys: Vec<f32> = coords.iter().map(|p| p[1]).collect();

    let img_w = img.width() as f32;
    let img_h = img.height() as f32;

    let x_min = xs.iter().cloned().fold(f32::INFINITY, f32::min).max(0.0).min(img_w - 1.0) as u32;
    let x_max = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max).max(0.0).min(img_w - 1.0) as u32;
    let y_min = ys.iter().cloned().fold(f32::INFINITY, f32::min).max(0.0).min(img_h - 1.0) as u32;
    let y_max = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max).max(0.0).min(img_h - 1.0) as u32;

    let w = x_max.saturating_sub(x_min) + 1;
    let h = y_max.saturating_sub(y_min) + 1;

    const MIN_CROP_SIZE: u32 = 8;
    if w < MIN_CROP_SIZE || h < MIN_CROP_SIZE {
        tracing::debug!("[paddle-ocr-v6] skipping small crop: {}x{}", w, h);
        return None;
    }

    let cropped = img.to_dyn().crop_imm(x_min, y_min, w, h);
    Some(cropped.into())
}

/// Convert a detection polygon into a normalized bounding box.
fn polygon_to_bbox(polygon: &usls::Polygon, img_w: u32, img_h: u32) -> BoundingBox {
    let coords = polygon.points();
    let xs: Vec<f32> = coords.iter().map(|p| p[0]).collect();
    let ys: Vec<f32> = coords.iter().map(|p| p[1]).collect();

    let x_min = xs.iter().cloned().fold(f32::INFINITY, f32::min);
    let x_max = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let y_min = ys.iter().cloned().fold(f32::INFINITY, f32::min);
    let y_max = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

    BoundingBox {
        x: x_min / img_w as f32,
        y: y_min / img_h as f32,
        width: (x_max - x_min) / img_w as f32,
        height: (y_max - y_min) / img_h as f32,
    }
}

// ---------------------------------------------------------------------------
// OcrEngine — detector + single multilingual recognizer
// ---------------------------------------------------------------------------

pub struct OcrEngine {
    pub detector: Option<usls::models::DB>,
    pub recognizer: Option<usls::models::SVTR>,
    pub tier: Tier,
    pub downloader: Downloader,
    pub models_dir: PathBuf,
    pub loaded: bool,
    pub load_error: Option<String>,
}

impl OcrEngine {
    pub fn new() -> Self {
        Self {
            detector: None,
            recognizer: None,
            tier: Tier::default(),
            downloader: Downloader::default(),
            models_dir: default_models_dir(),
            loaded: false,
            load_error: None,
        }
    }

    /// Lazy-load detector + recognizer for `tier`. No-op if already
    /// loaded at the same tier. Records any failure in `load_error`
    /// rather than returning Err — callers check `loaded` / `load_error`.
    pub fn ensure_loaded(&mut self, tier: Tier, device_hint: Option<&str>) {
        if self.loaded && self.tier == tier {
            return;
        }

        // Resolve Auto → concrete tier using host capability.
        let resolved = tier.resolve(
            has_cuda(device_hint),
            has_coreml(device_hint),
            total_ram_gb(),
        );
        self.tier = resolved;

        // Lazy-download missing files. Tiny tier ships in .nep so this
        // is usually a no-op; only small/medium trigger downloads.
        if let Err(e) = self.downloader.ensure_models(resolved, &self.models_dir) {
            self.loaded = false;
            self.load_error = Some(format!("download failed: {}", e));
            return;
        }

        let device = auto_device(device_hint);

        match try_load_detector(resolved, &self.models_dir, device) {
            Ok(d) => self.detector = Some(d),
            Err(e) => {
                self.loaded = false;
                self.load_error = Some(format!("detector load failed: {}", e));
                return;
            }
        }
        match try_load_recognizer(resolved, &self.models_dir, device) {
            Ok(r) => self.recognizer = Some(r),
            Err(e) => {
                self.loaded = false;
                self.load_error = Some(format!("recognizer load failed: {}", e));
                return;
            }
        }
        self.loaded = true;
        self.load_error = None;
    }

    /// Run detect → crop → recognize on a raw decoded image.
    /// `image_data` must be PNG / JPEG / similar bytes. The caller
    /// is responsible for fetching (from base64 / URL) before calling.
    pub fn recognize(&mut self, image_data: &[u8]) -> Result<OcrResult> {
        let start = std::time::Instant::now();

        if !self.loaded {
            return Err(ExtensionError::ExecutionFailed(
                "models not loaded — call ensure_loaded() first".to_string(),
            ));
        }

        // Decode image bytes → DynamicImage → usls::Image
        let dyn_img = image::load_from_memory(image_data).map_err(|e| {
            ExtensionError::InvalidArguments(format!("decode image failed: {}", e))
        })?;
        let img_w = dyn_img.width();
        let img_h = dyn_img.height();
        let img: usls::Image = dyn_img.into();

        // ---- Detect text regions (DB) ------------------------------------
        let det_results = {
            let detector = self.detector.as_mut().ok_or_else(|| {
                ExtensionError::ExecutionFailed("detector missing".to_string())
            })?;
            detector
                .forward(&[img.clone()])
                .map_err(|e| ExtensionError::ExecutionFailed(format!("detect failed: {}", e)))?
        };

        // ---- Crop each detection polygon ---------------------------------
        let mut crops: Vec<usls::Image> = Vec::new();
        let mut bboxes: Vec<BoundingBox> = Vec::new();
        let mut polygons: Vec<Option<Vec<[f32; 2]>>> = Vec::new();

        if let Some(det_result) = det_results.first() {
            for polygon in &det_result.polygons {
                if let Some(crop) = crop_polygon(&img, polygon) {
                    let bbox = polygon_to_bbox(polygon, img_w, img_h);
                    let poly_norm = Some(
                        polygon
                            .points()
                            .iter()
                            .map(|p| [p[0] / img_w as f32, p[1] / img_h as f32])
                            .collect(),
                    );
                    crops.push(crop);
                    bboxes.push(bbox);
                    polygons.push(poly_norm);
                }
            }
        }

        // ---- Recognize crops in a single batch (SVTR) --------------------
        let mut text_blocks: Vec<TextBlock> = Vec::with_capacity(bboxes.len());
        let mut total_conf = 0.0_f32;

        if !crops.is_empty() {
            let rec_results = {
                let recognizer = self.recognizer.as_mut().ok_or_else(|| {
                    ExtensionError::ExecutionFailed("recognizer missing".to_string())
                })?;
                recognizer
                    .forward(&crops)
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("recognize failed: {}", e)))?
            };

            for (i, rec_y) in rec_results.iter().enumerate() {
                if i >= bboxes.len() {
                    break;
                }
                // Each crop may yield multiple text objects (rare); we
                // concatenate text and use the highest confidence.
                if rec_y.texts.is_empty() {
                    // No text recognized for this crop — emit a
                    // zero-confidence placeholder so bboxes stay aligned.
                    text_blocks.push(TextBlock {
                        text: String::new(),
                        confidence: 0.0,
                        bbox: bboxes[i].clone(),
                        polygon: polygons[i].clone(),
                    });
                    continue;
                }
                let mut best_text = String::new();
                let mut best_conf: f32 = 0.0;
                for t in &rec_y.texts {
                    let s = t.text().to_string();
                    let c = t.confidence().unwrap_or(0.0);
                    if c >= best_conf {
                        best_conf = c;
                        best_text = s;
                    }
                }
                total_conf += best_conf;
                text_blocks.push(TextBlock {
                    text: best_text,
                    confidence: best_conf,
                    bbox: bboxes[i].clone(),
                    polygon: polygons[i].clone(),
                });
            }
        }

        let total_blocks = text_blocks.len();
        let avg_confidence = if total_blocks == 0 {
            0.0
        } else {
            total_conf / total_blocks as f32
        };
        let full_text = text_blocks
            .iter()
            .map(|b| b.text.as_str())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(OcrResult {
            text_blocks,
            full_text,
            total_blocks,
            avg_confidence,
            processing_time_ms: start.elapsed().as_millis() as u64,
            image_width: img_w,
            image_height: img_h,
            tier: self.tier.as_str().to_string(),
        })
    }
}

impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn try_load_detector(
    tier: Tier,
    models_dir: &std::path::Path,
    device: usls::Device,
) -> std::result::Result<usls::models::DB, String> {
    let cfg = preset::ppocr_det_v6(tier, models_dir)
        .with_device_all(device)
        .commit()
        .map_err(|e| format!("det config commit failed: {}", e))?;
    usls::models::DB::new(cfg).map_err(|e| format!("DB init failed: {}", e))
}

fn try_load_recognizer(
    tier: Tier,
    models_dir: &std::path::Path,
    device: usls::Device,
) -> std::result::Result<usls::models::SVTR, String> {
    let cfg = preset::ppocr_rec_v6(tier, models_dir)
        .with_device_all(device)
        .commit()
        .map_err(|e| format!("rec config commit failed: {}", e))?;
    usls::models::SVTR::new(cfg).map_err(|e| format!("SVTR init failed: {}", e))
}

// ---------------------------------------------------------------------------
// Host capability detection (dependency-free)
// ---------------------------------------------------------------------------

/// Locate bundled models directory. Searches candidate paths relative
/// to the current executable and cwd.
fn default_models_dir() -> PathBuf {
    // Try: <exe_dir>/models, <exe_dir>/../models, <cwd>/models
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidates: [Option<PathBuf>; 2] = [
                Some(exe_dir.join("models")),
                exe_dir.parent().map(|p| p.join("models")),
            ];
            for cand in candidates.into_iter().flatten() {
                if cand.is_dir() {
                    return cand;
                }
            }
        }
    }
    std::env::current_dir().unwrap_or_default().join("models")
}

fn has_cuda(hint: Option<&str>) -> bool {
    if let Some(h) = hint {
        return h.eq_ignore_ascii_case("cuda");
    }
    // Best-effort: env var presence is a reasonable proxy. v1 additionally
    // probes GPU free memory; we keep this dependency-free.
    cfg!(target_os = "linux") && std::env::var("CUDA_VISIBLE_DEVICES").is_ok()
}

fn has_coreml(hint: Option<&str>) -> bool {
    if let Some(h) = hint {
        return h.eq_ignore_ascii_case("coreml");
    }
    cfg!(target_os = "macos")
}

/// Best-effort total RAM in GiB. Reads /proc/meminfo on Linux;
/// returns 16 (assume Mid-tier capable) elsewhere to avoid pulling
/// a sysinfo crate dependency for one probe.
fn total_ram_gb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let kib: u64 = rest
                        .trim()
                        .trim_end_matches(" kB")
                        .parse()
                        .unwrap_or(0);
                    return kib / (1024 * 1024);
                }
            }
        }
        return 8;
    }
    #[cfg(not(target_os = "linux"))]
    {
        16
    }
}

fn auto_device(hint: Option<&str>) -> usls::Device {
    // macOS → CoreML (Apple Silicon optimizes SVTR significantly)
    #[cfg(target_os = "macos")]
    {
        let _ = hint;
        return usls::Device::CoreMl;
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(h) = hint {
            if h.eq_ignore_ascii_case("cpu") {
                return usls::Device::Cpu(0);
            }
        }
        if has_cuda(Some("cuda")) || has_cuda(None) {
            return usls::Device::Cuda(0);
        }
        usls::Device::Cpu(0)
    }
}

// ---------------------------------------------------------------------------
// Extension shell — owns the engine behind a RwLock
// ---------------------------------------------------------------------------

pub struct PaddleOcrV6Extension {
    engine: Arc<RwLock<OcrEngine>>,
    configured_tier: RwLock<Tier>,
}

impl PaddleOcrV6Extension {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(OcrEngine::new())),
            configured_tier: RwLock::new(Tier::default()),
        }
    }
}

impl Default for PaddleOcrV6Extension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for PaddleOcrV6Extension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "paddle-ocr-v6",
                "PaddleOCR-v6",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description(
                "PP-OCRv6 native ONNX inference with multi-tier model support (tiny/small/medium)",
            )
            .with_author("NeoMind Team")
        })
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand::new("recognize")
                .with_display_name("Recognize Text")
                .with_description("Run PP-OCRv6 detection + recognition on an image")
                .param(param_optional(
                    "image_base64",
                    "Image (base64)",
                    "Base64-encoded image bytes (PNG/JPEG)",
                    MetricDataType::String,
                ))
                .param(param_optional(
                    "image_url",
                    "Image URL",
                    "HTTP URL to fetch image from",
                    MetricDataType::String,
                )),
            ExtensionCommand::new("switch_tier")
                .with_display_name("Switch Tier")
                .with_description("Switch to a different model tier (tiny/small/medium/auto)")
                .param({
                    let mut p = ParameterDefinition::new("tier", MetricDataType::String);
                    p.display_name = "Tier".to_string();
                    p.description = "tiny|small|medium|auto".to_string();
                    p.default_value = Some(MetricValue::String("auto".to_string()));
                    p
                }),
            ExtensionCommand::new("health")
                .with_display_name("Health Check")
                .with_description("Return model load status and current tier"),
        ]
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor::new("loaded", "Models Loaded", MetricDataType::Boolean),
            MetricDescriptor::new("tier", "Active Tier", MetricDataType::String),
        ]
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &Value,
    ) -> Result<Value> {
        match command {
            "recognize" => self.cmd_recognize(args).await,
            "switch_tier" => self.cmd_switch_tier(args).await,
            "health" => Ok(self.cmd_health()),
            _ => Err(ExtensionError::InvalidArguments(format!(
                "Unknown command: '{}'. Available: recognize | switch_tier | health",
                command
            ))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl PaddleOcrV6Extension {
    async fn cmd_recognize(&self, args: &Value) -> Result<Value> {
        // Resolve image bytes from one of three input shapes.
        // Priority: explicit base64 > explicit URL > (none).
        let image_base64 = args.get("image_base64").and_then(|v| v.as_str());
        let image_url = args.get("image_url").and_then(|v| v.as_str());

        let image_bytes: Vec<u8> = if let Some(b64) = image_base64 {
            // Accept data-URL prefix (`data:image/png;base64,...`) or raw base64.
            let raw = b64
                .strip_prefix("data:")
                .and_then(|s| s.split(',').nth(1))
                .unwrap_or(b64);
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(raw)
                .map_err(|e| ExtensionError::InvalidArguments(format!("base64 decode: {}", e)))?
        } else if let Some(url) = image_url {
            // Sync HTTP fetch (ureq to avoid Tokio runtime issues inside cdylib).
            Self::fetch_url(url)?
        } else {
            return Err(ExtensionError::InvalidArguments(
                "missing 'image_base64' or 'image_url' parameter".to_string(),
            ));
        };

        // Hold the write lock only for the duration of inference. Note:
        // a long OCR run will block parallel switch_tier calls — that's
        // acceptable for v1 (switch_tier is rare). If it becomes a real
        // bottleneck, snapshot the engine state and release the lock.
        let mut engine = self.engine.write();
        if !engine.loaded {
            // Lazy-load on first recognize using configured tier. This
            // makes the extension work out-of-the-box if models are on
            // disk (tiny tier ships in the .nep).
            let tier = *self.configured_tier.read();
            engine.ensure_loaded(tier, None);
        }
        if !engine.loaded {
            if let Some(err) = &engine.load_error {
                return Err(ExtensionError::ExecutionFailed(format!(
                    "models not loaded: {}",
                    err
                )));
            }
            return Err(ExtensionError::ExecutionFailed(
                "models not loaded — call switch_tier first".to_string(),
            ));
        }

        let result = engine.recognize(&image_bytes)?;
        serde_json::to_value(&result).map_err(|e| {
            ExtensionError::ExecutionFailed(format!("serialize result failed: {}", e))
        })
    }

    async fn cmd_switch_tier(&self, args: &Value) -> Result<Value> {
        let tier_str = args
            .get("tier")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("missing 'tier' parameter".to_string())
            })?;
        let new_tier = Tier::from_str(tier_str)?;

        // Update configured tier
        *self.configured_tier.write() = new_tier;

        // Reload engine. NOTE: this holds the write lock during the
        // (potentially long, if download is needed) reload. Spec §6.1
        // prescribes splitting download (no lock) from reload (short lock).
        // For MVP we accept the simpler synchronous path — small/medium
        // downloads are 18MB / 132MB and happen rarely (once per tier switch).
        // TODO: split download from reload per spec §6.1 if this becomes
        // a real bottleneck for parallel recognize calls during switch.
        {
            let mut engine = self.engine.write();
            engine.ensure_loaded(new_tier, None);
        }

        let engine = self.engine.read();
        if engine.loaded {
            Ok(json!({
                "success": true,
                "tier": engine.tier.as_str(),
                "loaded": true,
            }))
        } else {
            Ok(json!({
                "success": false,
                "tier": new_tier.as_str(),
                "loaded": false,
                "error": engine.load_error.clone().unwrap_or_default(),
            }))
        }
    }

    fn cmd_health(&self) -> Value {
        let engine = self.engine.read();
        json!({
            "loaded": engine.loaded,
            "tier": engine.tier.as_str(),
            "configured_tier": self.configured_tier.read().as_str(),
            "models_dir": engine.models_dir.to_string_lossy(),
            "load_error": engine.load_error.clone(),
        })
    }

    /// Synchronous HTTP fetch. Uses ureq (not async) to avoid pulling a
    /// Tokio runtime into the cdylib — same pattern as downloader.rs.
    fn fetch_url(url: &str) -> Result<Vec<u8>> {
        let resp = ureq::get(url)
            .timeout(std::time::Duration::from_secs(30))
            .call()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("HTTP {}: {}", url, e)))?;
        let status = resp.status();
        if status >= 400 {
            return Err(ExtensionError::ExecutionFailed(format!(
                "HTTP {} for {}",
                status, url
            )));
        }
        let mut buf = Vec::with_capacity(1 << 16);
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("read body {}: {}", url, e)))?;
        Ok(buf)
    }
}

// Drop unused import warnings; Mutex is reserved for future producer/consumer
// pattern when integrating with metrics dispatcher.
#[allow(dead_code)]
fn _retain_mutex_link() -> Mutex<()> {
    Mutex::new(())
}

/// Helper: build an optional ParameterDefinition with display name + description.
fn param_optional(
    name: &str,
    display: &str,
    desc: &str,
    ty: MetricDataType,
) -> ParameterDefinition {
    let mut p = ParameterDefinition::new(name, ty);
    p.display_name = display.to_string();
    p.description = desc.to_string();
    p.required = false;
    p
}

neomind_extension_sdk::neomind_export!(PaddleOcrV6Extension);
