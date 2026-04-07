//! OCR Device Inference Extension
//!
//! This extension provides OCR (Optical Character Recognition) inference
//! on device image data sources using PPOCR DB + SVTR pipeline.
//!
//! # Features
//! - Bind/unbind devices with image data sources
//! - Device validation before binding
//! - Event-driven inference on device data updates
//! - Store detection results as virtual metrics
//!
//! # Event Handling
//! This extension uses the SDK's built-in event handling mechanism:
//! - `event_subscriptions()` declares which events to subscribe to
//! - `handle_event()` is called by the system when events are received

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use neomind_extension_sdk::capabilities::CapabilityContext;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use chrono::Utc;
use base64::Engine;

// ============================================================================
// Types
// ============================================================================

/// Supported OCR languages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[serde(alias = "zh", alias = "chinese")]
    Chinese,
    #[serde(alias = "en", alias = "english")]
    #[default]
    English,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Chinese => write!(f, "chinese"),
            Language::English => write!(f, "english"),
        }
    }
}

/// Device binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceBinding {
    pub device_id: String,
    pub device_name: Option<String>,
    pub image_metric: String,
    pub result_metric_prefix: String,
    pub draw_boxes: bool,
    pub active: bool,
    #[serde(default)]
    pub language: Language,
}

impl Default for DeviceBinding {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            device_name: None,
            image_metric: "image".to_string(),
            result_metric_prefix: "ocr_".to_string(),
            draw_boxes: true,
            active: true,
            language: Language::default(),
        }
    }
}

/// Bounding box for text region
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Single text block recognition result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
}

/// OCR inference result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub device_id: String,
    pub text_blocks: Vec<TextBlock>,
    pub full_text: String,
    pub total_blocks: usize,
    pub avg_confidence: f32,
    pub inference_time_ms: u64,
    pub image_width: u32,
    pub image_height: u32,
    pub timestamp: i64,
    pub annotated_image_base64: Option<String>,
}

/// Binding status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingStatus {
    pub binding: DeviceBinding,
    pub last_inference: Option<i64>,
    pub total_inferences: u64,
    pub total_text_blocks: u64,
    pub last_error: Option<String>,
    /// Last original image (base64 data URI)
    pub last_image: Option<String>,
    /// Last recognized text blocks
    pub last_text_blocks: Option<Vec<TextBlock>>,
    /// Last full text
    pub last_full_text: Option<String>,
    /// Last annotated image with bounding boxes (base64 data URI)
    pub last_annotated_image: Option<String>,
}

// ============================================================================
// OCR Engine (Native Only)
// ============================================================================

/// Set up native library search paths before ONNX Runtime is loaded.
/// Checks NEOMIND_EXTENSION_DIR/lib/ and common system paths.
#[cfg(not(target_arch = "wasm32"))]
fn setup_native_lib_paths() {
    let lib_env = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };

    let mut paths = vec![];

    // 1. Extension's bundled libraries - check all possible locations
    if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
        let ext_path = std::path::Path::new(&ext_dir);

        // lib/ directory (top-level)
        let lib_dir = ext_path.join("lib");
        if lib_dir.is_dir() {
            tracing::info!("[NativeLibs] Adding extension lib dir: {}", lib_dir.display());
            paths.push(lib_dir.to_string_lossy().to_string());
        }

        // binaries/<platform>/ directory (where extension.dylib and bundled libs live)
        let binaries_dir = ext_path.join("binaries");
        if binaries_dir.is_dir() {
            // Add all subdirectories (darwin_aarch64, linux_amd64, etc.)
            if let Ok(entries) = std::fs::read_dir(&binaries_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        tracing::info!("[NativeLibs] Adding platform dir: {}", path.display());
                        paths.push(path.to_string_lossy().to_string());

                        // Create unversioned symlinks for versioned libraries
                        // e.g. libonnxruntime.1.19.2.dylib -> libonnxruntime.dylib
                        if let Ok(files) = std::fs::read_dir(&path) {
                            for file in files.flatten() {
                                let file_path = file.path();
                                let name = file_path.file_name().unwrap_or_default().to_string_lossy();
                                // Match versioned dylib/so patterns
                                if let Some(base) = name.strip_suffix(".dylib")
                                    .or_else(|| name.strip_suffix(".so"))
                                {
                                    if base.contains('.') {
                                        // Has version suffix like libonnxruntime.1.19.2
                                        let unversioned = if cfg!(target_os = "macos") {
                                            format!("{}.dylib", base.split('.').next().unwrap_or(base))
                                        } else {
                                            format!("{}.so", base.split('.').next().unwrap_or(base))
                                        };
                                        let link_path = path.join(&unversioned);
                                        if !link_path.exists() {
                                            let _ = std::os::unix::fs::symlink(&file_path, &link_path);
                                            tracing::info!("[NativeLibs] Created symlink: {} -> {}", unversioned, name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Also check current working directory (extension runner sets this)
    if let Ok(cwd) = std::env::current_dir() {
        let lib_dir = cwd.join("lib");
        if lib_dir.is_dir() {
            paths.push(lib_dir.to_string_lossy().to_string());
        }
    }

    // 2. Inherit existing paths
    if let Ok(existing) = std::env::var(lib_env) {
        paths.push(existing);
    }

    // 3. Common system paths
    for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
        if std::path::Path::new(dir).is_dir() {
            paths.push(dir.to_string());
        }
    }

    if !paths.is_empty() {
        let combined = paths.join(":");
        tracing::info!("[NativeLibs] Setting {} = {}", lib_env, combined);
        std::env::set_var(lib_env, &combined);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct OcrEngine {
    detector: Option<usls::models::DB>,
    recognizer_chinese: Option<usls::models::SVTR>,
    recognizer_english: Option<usls::models::SVTR>,
    loaded: bool,
    load_error: Option<String>,
    /// Whether we've attempted to load the model
    load_attempted: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl OcrEngine {
    /// Create a new engine without loading the model (lazy initialization)
    pub fn new() -> Self {
        tracing::info!("[OcrDeviceInference] OCR engine created (lazy - models not loaded yet)");
        Self {
            detector: None,
            recognizer_chinese: None,
            recognizer_english: None,
            loaded: false,
            load_error: None,
            load_attempted: false,
        }
    }

    /// Ensure the model is loaded (lazy init on first use)
    pub fn ensure_loaded(&mut self) {
        if self.load_attempted {
            return;
        }
        self.load_attempted = true;

        // Set up native library paths before ONNX Runtime is loaded
        setup_native_lib_paths();

        // Find models directory
        let models_dir = match Self::find_models_dir_static() {
            Ok(dir) => dir,
            Err(e) => {
                tracing::error!("[OcrEngine] Failed to find models directory: {}", e);
                self.load_error = Some(e.to_string());
                return;
            }
        };

        tracing::info!("[OcrEngine] Lazy loading OCR models from: {:?}", models_dir);
        let start = std::time::Instant::now();

        // Initialize DB detector with v5 config (language-agnostic)
        match Self::try_load_detector(&models_dir) {
            Ok(detector) => {
                self.detector = Some(detector);
            }
            Err(e) => {
                let err_msg = format!("Detector init failed: {}", e);
                tracing::error!("[OcrEngine] {}", err_msg);
                self.load_error = Some(err_msg);
                return;
            }
        }

        // Try to load both recognizers so they're ready for either language
        if let Err(e) = self.load_recognizer(&models_dir, &Language::Chinese) {
            tracing::warn!("[OcrEngine] Chinese recognizer not loaded (will load on demand): {}", e);
        }
        if let Err(e) = self.load_recognizer(&models_dir, &Language::English) {
            tracing::warn!("[OcrEngine] English recognizer not loaded (will load on demand): {}", e);
        }

        self.loaded = true;
        let elapsed = start.elapsed().as_millis();
        tracing::info!("[OcrEngine] Models loaded in {}ms", elapsed);
    }

    /// Find the models directory (static version for use in ensure_loaded)
    fn find_models_dir_static() -> Result<std::path::PathBuf> {
        // Try NEOMIND_EXTENSION_DIR first (set by NeoMind runtime)
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            let models_dir = std::path::PathBuf::from(&ext_dir).join("models");
            if models_dir.exists() {
                tracing::info!("[OcrEngine] Found models at: {:?}", models_dir);
                return Ok(models_dir);
            }
        }

        // Fallback: Check current working directory
        if let Ok(cwd) = std::env::current_dir() {
            let models_dir = cwd.join("models");
            if models_dir.exists() {
                tracing::info!("[OcrEngine] Found models at: {:?}", models_dir);
                return Ok(models_dir);
            }
        }

        // Additional fallback paths
        let fallback_paths = vec![
            std::path::PathBuf::from("models"),
            std::path::PathBuf::from("../models"),
        ];

        for models_dir in fallback_paths {
            if models_dir.exists() {
                tracing::info!("[OcrEngine] Found models at: {:?}", models_dir);
                return Ok(models_dir);
            }
        }

        Err(ExtensionError::ExecutionFailed(
            "Models directory not found. Please ensure OCR models are installed.".to_string()
        ))
    }

    /// Try to load the detector model
    fn try_load_detector(models_dir: &std::path::Path) -> Result<usls::models::DB> {
        let detector_config = usls::Config::ppocr_det_v5_mobile()
            .with_model_file(&models_dir.join("det_mv3_db.onnx").to_string_lossy())
            .with_device_all(usls::Device::Cpu(0))
            .commit()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Detector config failed: {}", e)))?;

        usls::models::DB::new(detector_config)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Detector init failed: {}", e)))
    }

    /// Initialize the engine (delegates to ensure_loaded for lazy loading)
    pub fn init(&mut self, _models_dir: &std::path::Path, _language: &Language) -> Result<()> {
        if self.loaded {
            return Ok(());
        }

        self.ensure_loaded();

        if self.loaded {
            Ok(())
        } else {
            Err(ExtensionError::ExecutionFailed(
                self.load_error.clone().unwrap_or_else(|| "Failed to load OCR models".to_string())
            ))
        }
    }

    fn load_recognizer(&mut self, models_dir: &std::path::Path, language: &Language) -> Result<()> {
        match language {
            Language::Chinese => {
                if self.recognizer_chinese.is_none() {
                    let vocab_path = models_dir.join("vocab.txt");
                    tracing::info!("[OcrDeviceInference] Loading vocab from {:?}", vocab_path);

                    // Use base SVTR config with local vocab file path
                    let config = usls::Config::svtr()
                        .with_model_file(&models_dir.join("rec_svtr.onnx").to_string_lossy())
                        .with_vocab_txt(&vocab_path.to_string_lossy())
                        .with_device_all(usls::Device::Cpu(0))
                        .with_model_ixx(0, 3, 960.into())  // max text length
                        .commit()
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("Chinese recognizer config failed: {}", e)))?;

                    let recognizer = usls::models::SVTR::new(config)
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("Chinese recognizer init failed: {}", e)))?;
                    self.recognizer_chinese = Some(recognizer);
                }
            }
            Language::English => {
                if self.recognizer_english.is_none() {
                    let config = usls::Config::ppocr_rec_v4_en()
                        .with_model_file(&models_dir.join("rec_en.onnx").to_string_lossy())
                        .with_device_all(usls::Device::Cpu(0))
                        .with_model_ixx(0, 3, 960.into())
                        .commit()
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("English recognizer config failed: {}", e)))?;

                    let recognizer = usls::models::SVTR::new(config)
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("English recognizer init failed: {}", e)))?;
                    self.recognizer_english = Some(recognizer);
                }
            }
        }
        Ok(())
    }

    pub fn recognize(&mut self, image_data: &[u8], device_id: &str, language: &Language) -> Result<OcrResult> {
        let start = std::time::Instant::now();

        // Lazy load on first use
        self.ensure_loaded();

        if !self.loaded {
            return Err(ExtensionError::ExecutionFailed(
                self.load_error.clone().unwrap_or_else(|| "OCR engine not initialized".to_string())
            ));
        }

        // On-demand recognizer loading for the requested language
        let needs_recognizer = match language {
            Language::Chinese => self.recognizer_chinese.is_none(),
            Language::English => self.recognizer_english.is_none(),
        };
        if needs_recognizer {
            if let Ok(models_dir) = Self::find_models_dir_static() {
                if let Err(e) = self.load_recognizer(&models_dir, language) {
                    tracing::warn!("[OcrEngine] On-demand recognizer load failed: {}", e);
                }
            }
        }

        // Load image using image crate, then convert to usls::Image
        let dyn_img = image::load_from_memory(image_data)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to load image: {}", e)))?;

        // Keep original for annotation
        let mut annotated_img = dyn_img.to_rgba8();

        let img: usls::Image = dyn_img.into();
        let (img_width, img_height) = (img.width(), img.height());

        // Detect text regions
        let det_results = if let Some(ref mut detector) = self.detector {
            detector.forward(&[img.clone()])
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Detection failed: {}", e)))?
        } else {
            return Err(ExtensionError::ExecutionFailed("Detector not initialized".to_string()));
        };

        // Collect cropped images and their bounding boxes first (before borrowing recognizer)
        let mut crops_with_bboxes: Vec<(usls::Image, BoundingBox)> = Vec::new();

        if let Some(det_result) = det_results.first() {
            tracing::info!("[OcrDeviceInference] Detection found {} polygons", det_result.polygons.len());
            for polygon in &det_result.polygons {
                let cropped = Self::crop_polygon_static(&img, polygon);
                if let Some(crop_img) = cropped {
                    let bbox = Self::polygon_to_bbox_static(polygon, img_width, img_height);
                    crops_with_bboxes.push((crop_img, bbox));
                }
            }
        } else {
            tracing::warn!("[OcrDeviceInference] Detection returned no results");
        }

        tracing::info!("[OcrDeviceInference] Created {} crops for recognition", crops_with_bboxes.len());

        // Now recognize all cropped images
        let mut text_blocks = Vec::new();
        let mut all_texts = Vec::new();
        let mut total_confidence = 0.0;

        for (crop_img, bbox) in crops_with_bboxes {
            // Recognize text using the selected recognizer
            let rec_results = match language {
                Language::Chinese => {
                    if let Some(ref mut recognizer) = self.recognizer_chinese {
                        recognizer.forward(&[crop_img])
                            .map_err(|e| ExtensionError::ExecutionFailed(format!("Recognition failed: {}", e)))?
                    } else {
                        return Err(ExtensionError::ExecutionFailed("Chinese recognizer not initialized".to_string()));
                    }
                }
                Language::English => {
                    if let Some(ref mut recognizer) = self.recognizer_english {
                        recognizer.forward(&[crop_img])
                            .map_err(|e| ExtensionError::ExecutionFailed(format!("Recognition failed: {}", e)))?
                    } else {
                        return Err(ExtensionError::ExecutionFailed("English recognizer not initialized".to_string()));
                    }
                }
            };

            if let Some(rec_result) = rec_results.first() {
                tracing::info!("[OcrDeviceInference] Recognition found {} texts", rec_result.texts.len());
                for text_obj in &rec_result.texts {
                    let text_str = text_obj.text().to_string();
                    let conf = text_obj.confidence().unwrap_or(0.0);

                    tracing::info!("[OcrDeviceInference] Recognized: '{}' (confidence: {:.2})", text_str, conf);

                    text_blocks.push(TextBlock {
                        text: text_str.clone(),
                        confidence: conf,
                        bbox: bbox.clone(),
                    });
                    all_texts.push(text_str);
                    total_confidence += conf;

                    // Draw bounding box on annotated image
                    Self::draw_bbox_static(&mut annotated_img, &bbox, img_width, img_height);
                }
            } else {
                tracing::warn!("[OcrDeviceInference] Recognition returned no results for crop");
            }
        }

        tracing::info!("[OcrDeviceInference] Total text blocks: {}, full text length: {}", text_blocks.len(), all_texts.join("\n").len());

        let inference_time = start.elapsed().as_millis() as u64;
        let timestamp = Utc::now().timestamp();

        let avg_conf = if text_blocks.is_empty() {
            0.0
        } else {
            total_confidence / text_blocks.len() as f32
        };

        // Convert annotated image to base64
        let annotated_base64 = Self::image_to_base64_static(&annotated_img);

        Ok(OcrResult {
            device_id: device_id.to_string(),
            text_blocks: text_blocks.clone(),
            full_text: all_texts.join("\n"),
            total_blocks: text_blocks.len(),
            avg_confidence: avg_conf,
            inference_time_ms: inference_time,
            image_width: img_width,
            image_height: img_height,
            timestamp,
            annotated_image_base64: Some(annotated_base64),
        })
    }

    fn crop_polygon_static(img: &usls::Image, polygon: &usls::Polygon) -> Option<usls::Image> {
        let coords = polygon.points();
        if coords.is_empty() {
            return None;
        }

        let xs: Vec<f32> = coords.iter().map(|p| p[0]).collect();
        let ys: Vec<f32> = coords.iter().map(|p| p[1]).collect();

        let x_min = xs.iter().cloned().fold(f32::INFINITY, f32::min) as u32;
        let x_max = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max) as u32;
        let y_min = ys.iter().cloned().fold(f32::INFINITY, f32::min) as u32;
        let y_max = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max) as u32;

        let x_min = x_min.max(0);
        let x_max = x_max.min(img.width() - 1);
        let y_min = y_min.max(0);
        let y_max = y_max.min(img.height() - 1);

        if x_max <= x_min || y_max <= y_min {
            return None;
        }

        let cropped = img.to_dyn().crop_imm(x_min, y_min, x_max - x_min + 1, y_max - y_min + 1);
        Some(cropped.into())
    }

    fn polygon_to_bbox_static(polygon: &usls::Polygon, img_w: u32, img_h: u32) -> BoundingBox {
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

    fn draw_bbox_static(img: &mut image::RgbaImage, bbox: &BoundingBox, img_w: u32, img_h: u32) {
        use imageproc::drawing::draw_hollow_rect_mut;
        use imageproc::rect::Rect;

        let x = (bbox.x * img_w as f32) as i32;
        let y = (bbox.y * img_h as f32) as i32;
        let w = (bbox.width * img_w as f32) as u32;
        let h = (bbox.height * img_h as f32) as u32;

        let color = image::Rgba([0u8, 255u8, 0u8, 255u8]);

        let x = x.max(0).min(img_w as i32 - 1);
        let y = y.max(0).min(img_h as i32 - 1);
        let w = w.min(img_w.saturating_sub(x as u32));
        let h = h.min(img_h.saturating_sub(y as u32));

        if w > 0 && h > 0 {
            let rect = Rect::at(x, y).of_size(w, h);
            draw_hollow_rect_mut(img, rect, color);
            if w > 2 && h > 2 {
                let rect2 = Rect::at(x + 1, y + 1).of_size(w - 2, h - 2);
                draw_hollow_rect_mut(img, rect2, color);
            }
        }
    }

    fn image_to_base64_static(img: &image::RgbaImage) -> String {
        use base64::Engine;

        let rgb_img = image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
        let mut buffer = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buffer);

        if rgb_img.write_to(&mut cursor, image::ImageFormat::Jpeg).is_ok() {
            base64::engine::general_purpose::STANDARD.encode(&buffer)
        } else {
            String::new()
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn get_load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Main Extension
// ============================================================================

pub struct OcrDeviceInference {
    #[cfg(not(target_arch = "wasm32"))]
    ocr_engine: Mutex<OcrEngine>,

    bindings: Arc<RwLock<HashMap<String, DeviceBinding>>>,
    binding_stats: Arc<RwLock<HashMap<String, BindingStatus>>>,
    total_inferences: Arc<AtomicU64>,
    total_text_blocks: Arc<AtomicU64>,
    total_errors: Arc<AtomicU64>,
}

impl OcrDeviceInference {
    pub fn new() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            ocr_engine: Mutex::new(OcrEngine::new()),
            bindings: Arc::new(RwLock::new(HashMap::new())),
            binding_stats: Arc::new(RwLock::new(HashMap::new())),
            total_inferences: Arc::new(AtomicU64::new(0)),
            total_text_blocks: Arc::new(AtomicU64::new(0)),
            total_errors: Arc::new(AtomicU64::new(0)),
        }
    }

    fn get_status(&self) -> serde_json::Value {
        let bindings = self.bindings.read();
        let stats = self.binding_stats.read();

        #[cfg(not(target_arch = "wasm32"))]
        let (model_loaded, model_error) = {
            let engine = self.ocr_engine.lock();
            (engine.is_loaded(), engine.get_load_error().map(|s| s.to_string()))
        };
        #[cfg(target_arch = "wasm32")]
        let (model_loaded, model_error): (bool, Option<String>) = (false, Some("WASM not supported".to_string()));

        json!({
            "model_loaded": model_loaded,
            "model_error": model_error,
            "total_inferences": self.total_inferences.load(Ordering::Relaxed),
            "total_text_blocks": self.total_text_blocks.load(Ordering::Relaxed),
            "total_errors": self.total_errors.load(Ordering::Relaxed),
            "bindings_count": bindings.len(),
            "bindings": stats.values().map(|s| {
                json!({
                    "device_id": s.binding.device_id,
                    "active": s.binding.active,
                    "total_inferences": s.total_inferences
                })
            }).collect::<Vec<_>>()
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn find_models_dir(&self) -> Result<std::path::PathBuf> {
        // Try NEOMIND_EXTENSION_DIR first (set by NeoMind runtime)
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            let models_dir = std::path::PathBuf::from(&ext_dir).join("models");
            if models_dir.exists() {
                tracing::info!("[OcrDeviceInference] Found models at: {:?}", models_dir);
                return Ok(models_dir);
            }
        }

        // Fallback: Check current working directory
        if let Ok(cwd) = std::env::current_dir() {
            let models_dir = cwd.join("models");
            if models_dir.exists() {
                tracing::info!("[OcrDeviceInference] Found models at: {:?}", models_dir);
                return Ok(models_dir);
            }
        }

        // Additional fallback paths
        let fallback_paths = vec![
            std::path::PathBuf::from("models"),
            std::path::PathBuf::from("../models"),
        ];

        for models_dir in fallback_paths {
            if models_dir.exists() {
                tracing::info!("[OcrDeviceInference] Found models at: {:?}", models_dir);
                return Ok(models_dir);
            }
        }

        Err(ExtensionError::ExecutionFailed(
            "Models directory not found. Please ensure OCR models are installed.".to_string()
        ))
    }

    /// Invoke a capability synchronously (for use in handle_event)
    #[cfg(not(target_arch = "wasm32"))]
    fn invoke_capability_sync(
        &self,
        capability_name: &str,
        params: &serde_json::Value,
    ) -> serde_json::Value {
        tokio::task::block_in_place(|| {
            let capability_context = CapabilityContext::default();
            capability_context.invoke_capability(capability_name, params)
        })
    }

    /// Extract image data from a value, supporting nested paths
    fn extract_image_from_value<'a>(&self, value: Option<&'a serde_json::Value>, nested_path: Option<&str>) -> Option<String> {
        let v = value?;

        // If we have a nested path, navigate to it
        let target_value = if let Some(path) = nested_path {
            let mut current = v;
            for part in path.split('.') {
                current = current.get(part)?;
            }
            current
        } else {
            v
        };

        // Try to extract string
        if let Some(s) = target_value.as_str() {
            // Check if it's a data URI
            if s.starts_with("data:image") {
                let parts: Vec<&str> = s.splitn(2, ',').collect();
                if parts.len() == 2 {
                    return Some(parts[1].to_string());
                }
            }
            return Some(s.to_string());
        }

        None
    }

    /// Process inference result and write virtual metrics
    #[cfg(not(target_arch = "wasm32"))]
    fn write_inference_results(
        &self,
        device_id: &str,
        result: &OcrResult,
        image_b64: &str,
    ) {
        // Update binding stats
        if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
            stats.last_image = Some(format!("data:image/jpeg;base64,{}", image_b64));
            stats.last_text_blocks = Some(result.text_blocks.clone());
            stats.last_full_text = Some(result.full_text.clone());
            stats.last_inference = Some(result.timestamp);
            stats.total_inferences += 1;
            stats.total_text_blocks += result.total_blocks as u64;
            if let Some(annotated_b64) = &result.annotated_image_base64 {
                stats.last_annotated_image = Some(format!("data:image/jpeg;base64,{}", annotated_b64));
            }
        }

        // Write virtual metrics through the native capability bridge.
        // Virtual metrics must start with: transform., virtual., computed., derived., or aggregated.

        // Write text block count (virtual metric)
        let metric_name = "virtual.ocr.count";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": result.total_blocks,
            "timestamp": result.timestamp,
        });
        let _ = self.invoke_capability_sync("device_metrics_write", &params);

        // Write full text (virtual metric)
        let metric_name = "virtual.ocr.full_text";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": result.full_text,
            "timestamp": result.timestamp,
        });
        let _ = self.invoke_capability_sync("device_metrics_write", &params);

        // Write average confidence (virtual metric)
        let metric_name = "virtual.ocr.confidence";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": result.avg_confidence,
            "timestamp": result.timestamp,
        });
        let _ = self.invoke_capability_sync("device_metrics_write", &params);

        // Write inference time (virtual metric)
        let metric_name = "virtual.ocr.inference_time_ms";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": result.inference_time_ms,
            "timestamp": result.timestamp,
        });
        let _ = self.invoke_capability_sync("device_metrics_write", &params);

        // Write annotated image (virtual metric) with proper data URI format
        if let Some(img) = &result.annotated_image_base64 {
            let metric_name = "virtual.ocr.annotated_image";
            let data_uri = format!("data:image/jpeg;base64,{}", img);
            let params = serde_json::json!({
                "device_id": device_id,
                "metric": metric_name,
                "value": data_uri,
                "timestamp": result.timestamp,
            });
            let _ = self.invoke_capability_sync("device_metrics_write", &params);
        }

        // Write text blocks as JSON (virtual metric)
        let metric_name = "virtual.ocr.text";
        let params = serde_json::json!({
            "device_id": device_id,
            "metric": metric_name,
            "value": serde_json::to_string(&result.text_blocks).unwrap_or_default(),
            "timestamp": result.timestamp,
        });
        let _ = self.invoke_capability_sync("device_metrics_write", &params);

        tracing::info!(
            "[OcrDeviceInference] Wrote inference results for device={}, blocks={}, time={}ms",
            device_id,
            result.total_blocks,
            result.inference_time_ms
        );
    }
}

impl Default for OcrDeviceInference {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for OcrDeviceInference {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "ocr-device-inference",
                "OCR",
                env!("CARGO_PKG_VERSION")
            )
            .with_description("OCR device inference extension with automatic text recognition")
            .with_author("NeoMind Team")
        })
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "bind_device".to_string(),
                display_name: "Bind Device".to_string(),
                description: "Bind a device for automatic OCR inference on image updates".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device to bind".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "device_name".to_string(),
                        display_name: "Device Name".to_string(),
                        description: "Display name for the device".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "image_metric".to_string(),
                        display_name: "Image Metric".to_string(),
                        description: "Name of the image data source metric".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String("image".to_string())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "draw_boxes".to_string(),
                        display_name: "Draw Boxes".to_string(),
                        description: "Whether to draw text bounding boxes on images".to_string(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: Some(ParamMetricValue::Boolean(true)),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "language".to_string(),
                        display_name: "Language".to_string(),
                        description: "OCR language: 'chinese' (default) or 'english'".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String("chinese".to_string())),
                        min: None,
                        max: None,
                        options: vec!["chinese".to_string(), "english".to_string()],
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "unbind_device".to_string(),
                display_name: "Unbind Device".to_string(),
                description: "Unbind a device from OCR inference".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device to unbind".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "toggle_binding".to_string(),
                display_name: "Toggle Binding".to_string(),
                description: "Enable or disable a device binding".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".to_string(),
                        display_name: "Device ID".to_string(),
                        description: "ID of the device binding to toggle".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "active".to_string(),
                        display_name: "Active".to_string(),
                        description: "Whether to activate or deactivate the binding".to_string(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: Some(ParamMetricValue::Boolean(true)),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_bindings".to_string(),
                display_name: "Get Bindings".to_string(),
                description: "Get all device bindings".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "recognize_image".to_string(),
                display_name: "Recognize Image".to_string(),
                description: "Perform OCR on a base64 encoded image".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "image".to_string(),
                        display_name: "Image".to_string(),
                        description: "Base64 encoded image (supports data URI format)".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "language".to_string(),
                        display_name: "Language".to_string(),
                        description: "OCR language: 'chinese' (default) or 'english'".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String("chinese".to_string())),
                        min: None,
                        max: None,
                        options: vec!["chinese".to_string(), "english".to_string()],
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".to_string(),
                display_name: "Get Status".to_string(),
                description: "Get extension status and statistics".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
        ]
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "bound_devices".to_string(),
                display_name: "Bound Devices".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_inferences".to_string(),
                display_name: "Total Inferences".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_text_blocks".to_string(),
                display_name: "Total Text Blocks".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_errors".to_string(),
                display_name: "Total Errors".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "virtual.ocr.text".to_string(),
                display_name: "OCR Text Blocks".to_string(),
                data_type: MetricDataType::String,
                unit: "json".to_string(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "virtual.ocr.full_text".to_string(),
                display_name: "Full Text".to_string(),
                data_type: MetricDataType::String,
                unit: "text".to_string(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "virtual.ocr.count".to_string(),
                display_name: "Text Block Count".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "virtual.ocr.confidence".to_string(),
                display_name: "Average Confidence".to_string(),
                data_type: MetricDataType::Float,
                unit: "score".to_string(),
                min: Some(0.0),
                max: Some(1.0),
                required: false,
            },
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "bind_device" => {
                let device_id = args["device_id"].as_str()
                    .ok_or_else(|| ExtensionError::ExecutionFailed("device_id required".to_string()))?
                    .to_string();

                let device_name = args["device_name"].as_str().map(|s| s.to_string());
                let image_metric = args["image_metric"].as_str()
                    .unwrap_or("image")
                    .to_string();
                let draw_boxes = args["draw_boxes"].as_bool().unwrap_or(true);

                let binding = DeviceBinding {
                    device_id: device_id.clone(),
                    device_name,
                    image_metric,
                    result_metric_prefix: "ocr_".to_string(),
                    draw_boxes,
                    active: true,
                    language: args["language"].as_str()
                        .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                        .unwrap_or_default(),
                };

                self.bindings.write().insert(device_id.clone(), binding.clone());
                self.binding_stats.write().insert(device_id.clone(), BindingStatus {
                    binding,
                    last_inference: None,
                    total_inferences: 0,
                    total_text_blocks: 0,
                    last_error: None,
                    last_image: None,
                    last_text_blocks: None,
                    last_full_text: None,
                    last_annotated_image: None,
                });

                tracing::info!("[OcrDeviceInference] Bound device: {}", device_id);
                Ok(json!({"success": true, "device_id": device_id}))
            }

            "unbind_device" => {
                let device_id = args["device_id"].as_str()
                    .ok_or_else(|| ExtensionError::ExecutionFailed("device_id required".to_string()))?;

                self.bindings.write().remove(device_id);
                self.binding_stats.write().remove(device_id);

                tracing::info!("[OcrDeviceInference] Unbound device: {}", device_id);
                Ok(json!({"success": true}))
            }

            "get_bindings" => {
                let stats = self.binding_stats.read();
                Ok(json!({"success": true, "bindings": stats.values().collect::<Vec<_>>()}))
            }

            "recognize_image" => {
                tracing::info!("[OcrDeviceInference] recognize_image called");
                let image_b64 = args["image"].as_str()
                    .ok_or_else(|| ExtensionError::ExecutionFailed("image required".to_string()))?;

                tracing::info!("[OcrDeviceInference] Image base64 length: {} bytes", image_b64.len());

                // Parse language parameter
                let language: Language = args["language"].as_str()
                    .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok())
                    .unwrap_or_default();

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let image_data = if image_b64.starts_with("data:image") {
                        let parts: Vec<&str> = image_b64.splitn(2, ',').collect();
                        if parts.len() != 2 {
                            return Err(ExtensionError::ExecutionFailed("Invalid data URI".to_string()));
                        }
                        base64::engine::general_purpose::STANDARD.decode(parts[1])
                            .map_err(|e| ExtensionError::ExecutionFailed(format!("Base64 decode failed: {}", e)))?
                    } else {
                        base64::engine::general_purpose::STANDARD.decode(image_b64)
                            .map_err(|e| ExtensionError::ExecutionFailed(format!("Base64 decode failed: {}", e)))?
                    };

                    tracing::info!("[OcrDeviceInference] Decoded image data: {} bytes", image_data.len());

                    // recognize() will call ensure_loaded() internally for lazy init

                    tracing::info!("[OcrDeviceInference] Calling recognize with language: {:?}", language);
                    let mut engine = self.ocr_engine.lock();
                    let result = engine.recognize(&image_data, "manual", &language)?;
                    tracing::info!("[OcrDeviceInference] Recognize returned {} text blocks", result.text_blocks.len());

                    self.total_inferences.fetch_add(1, Ordering::Relaxed);
                    self.total_text_blocks.fetch_add(result.total_blocks as u64, Ordering::Relaxed);

                    Ok(json!({"success": true, "data": result}))
                }

                #[cfg(target_arch = "wasm32")]
                {
                    Ok(json!({"success": false, "error": "OCR not supported on WASM"}))
                }
            }

            "toggle_binding" => {
                let device_id = args["device_id"].as_str()
                    .ok_or_else(|| ExtensionError::ExecutionFailed("device_id required".to_string()))?;

                let active = args["active"].as_bool().unwrap_or(true);

                if let Some(binding) = self.bindings.write().get_mut(device_id) {
                    binding.active = active;
                    tracing::info!("[OcrDeviceInference] Toggled binding: {} -> {}", device_id, active);
                }

                if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
                    stats.binding.active = active;
                }

                Ok(json!({"success": true, "device_id": device_id, "active": active}))
            }

            "get_status" => {
                Ok(json!({"success": true, "data": self.get_status()}))
            }

            _ => Err(ExtensionError::ExecutionFailed(format!("Unknown command: {}", command))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Subscribe to DeviceMetric events to listen for device image updates
    fn event_subscriptions(&self) -> &[&str] {
        &["DeviceMetric"]
    }

    /// Handle events from the EventBus
    ///
    /// This method is called by the system when a DeviceMetric event is published.
    /// It checks if the device is bound and processes the image data.
    fn handle_event(
        &self,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<()> {
        tracing::debug!("[OcrDeviceInference] handle_event called: event_type={}", event_type);

        if event_type != "DeviceMetric" {
            return Ok(());
        }

        // Extract event data from the standardized format
        let inner_payload = payload.get("payload").unwrap_or(payload);

        let device_id = inner_payload.get("device_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let metric = inner_payload.get("metric")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        tracing::debug!("[OcrDeviceInference] Processing: device={}, metric={}", device_id, metric);

        let value = inner_payload.get("value");

        // Check if this device is bound
        let bindings = self.bindings.read();
        let binding = bindings.get(device_id).cloned();

        if let Some(binding) = binding {
            if !binding.active {
                tracing::debug!("[OcrDeviceInference] Binding inactive for device: {}", device_id);
                return Ok(());
            }

            // Check if metric matches
            let exact_match = metric == binding.image_metric;

            let (top_level_metric, nested_path) = if binding.image_metric.contains('.') {
                let parts: Vec<&str> = binding.image_metric.splitn(2, '.').collect();
                (parts[0].to_string(), Some(parts[1].to_string()))
            } else {
                (binding.image_metric.clone(), None)
            };

            let metric_matches = exact_match || metric == top_level_metric;
            let nested_path = if exact_match { None } else { nested_path };

            tracing::debug!(
                "[OcrDeviceInference] Event: device={}, metric={}, binding_metric={}, matches={}",
                device_id, metric, binding.image_metric, metric_matches
            );

            if !metric_matches {
                return Ok(());
            }

            // Extract image data from value
            let image_b64 = self.extract_image_from_value(value, nested_path.as_deref());

            if let Some(image_data_b64) = image_b64 {
                match base64::engine::general_purpose::STANDARD.decode(&image_data_b64) {
                    Ok(image_data) => {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            // recognize() will call ensure_loaded() internally for lazy init
                            let mut engine = self.ocr_engine.lock();
                            match engine.recognize(&image_data, device_id, &binding.language) {
                                Ok(result) => {
                                    tracing::info!(
                                        "[OcrDeviceInference] Inference: device={}, blocks={}, time={}ms",
                                        device_id,
                                        result.total_blocks,
                                        result.inference_time_ms
                                    );
                                    self.total_inferences.fetch_add(1, Ordering::SeqCst);
                                    self.total_text_blocks.fetch_add(result.total_blocks as u64, Ordering::SeqCst);
                                    self.write_inference_results(
                                        device_id,
                                        &result,
                                        &image_data_b64,
                                    );
                                }
                                Err(e) => {
                                    self.total_errors.fetch_add(1, Ordering::SeqCst);
                                    tracing::warn!("[OcrDeviceInference] Process failed: device={}, error={}", device_id, e);
                                    if let Some(stats) = self.binding_stats.write().get_mut(device_id) {
                                        stats.last_error = Some(e.to_string());
                                    }
                                }
                            }
                        }

                        #[cfg(target_arch = "wasm32")]
                        {
                            tracing::warn!("[OcrDeviceInference] Image processing not supported in WASM");
                            let _ = image_data;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[OcrDeviceInference] Base64 decode failed: device={}, error={}", device_id, e);
                    }
                }
            }
        }

        Ok(())
    }
}

neomind_extension_sdk::neomind_export!(OcrDeviceInference);
