//! NeoMind Image Analyzer Extension (V2)
//!
//! Image analysis using YOLOv8 via usls library.
//! Built for the NeoMind isolated extension runtime.

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use base64::Engine;
use parking_lot::Mutex;

#[cfg(not(target_arch = "wasm32"))]

#[cfg(not(target_arch = "wasm32"))]
use usls::{models::YOLO, Config, DataLoader, Device, Version as YOLOVersion};

/// Auto-detect best available inference device.
/// macOS → CoreML, Linux → CUDA, others → CPU.
#[cfg(not(target_arch = "wasm32"))]
fn auto_device() -> Device {
    #[cfg(target_os = "macos")]
    { Device::CoreMl }
    #[cfg(all(not(target_os = "macos"), target_os = "linux"))]
    { Device::Cuda(0) }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    { Device::Cpu(0) }
}

/// Try building a model with the auto-detected device, fall back to CPU on failure.
#[cfg(not(target_arch = "wasm32"))]
fn with_device_fallback<M, F>(try_build: F) -> std::result::Result<M, String>
where
    F: Fn(Device) -> std::result::Result<M, String>,
{
    let device = auto_device();
    eprintln!("[HW] Trying device: {:?}", device);
    match try_build(device) {
        Ok(model) => {
            eprintln!("[HW] Model loaded with device: {:?}", device);
            Ok(model)
        }
        Err(e) if !matches!(device, Device::Cpu(_)) => {
            eprintln!("[HW] {:?} failed ({}), falling back to CPU", device, e);
            try_build(Device::Cpu(0))
        }
        Err(e) => Err(e),
    }
}

// ============================================================================
// Types
// ============================================================================

/// Image detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub label: String,
    pub confidence: f32,
    pub bbox: Option<BoundingBox>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub objects: Vec<Detection>,
    pub description: String,
    pub processing_time_ms: u64,
    pub model_loaded: bool,
    pub model_error: Option<String>,
}

// ============================================================================
// COCO Classes
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
pub const COCO_CLASSES: [&str; 80] = [
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat",
    "traffic light", "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat",
    "dog", "horse", "sheep", "cow", "elephant", "bear", "zebra", "giraffe", "backpack",
    "umbrella", "handbag", "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball",
    "kite", "baseball bat", "baseball glove", "skateboard", "surfboard", "tennis racket",
    "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
    "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake",
    "chair", "couch", "potted plant", "bed", "dining table", "toilet", "tv", "laptop",
    "mouse", "remote", "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
    "refrigerator", "book", "clock", "vase", "scissors", "teddy bear", "hair drier", "toothbrush",
];

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct ImageAnalyzer {
    images_processed: AtomicU64,
    total_processing_time_ms: AtomicU64,
    detections_found: AtomicU64,
    #[cfg(not(target_arch = "wasm32"))]
    detector: Mutex<YOLODetector>,
    // Configuration
    confidence_threshold: Mutex<f32>,
    nms_threshold: Mutex<f32>,
    model_version: Mutex<String>,
}

#[cfg(not(target_arch = "wasm32"))]
struct YOLODetector {
    model: Option<YOLO>,
    load_error: Option<String>,
    /// Config for lazy loading
    conf: f32,
    iou: f32,
    version: String,
    scale: String,
    /// Whether we've attempted to load the model
    load_attempted: bool,
}

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
                                            #[cfg(unix)]
                                            let _ = std::os::unix::fs::symlink(&file_path, &link_path);
                                            #[cfg(not(unix))]
                                            let _ = (); // Windows: no versioned library symlinks needed
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

    // Set ORT_DYLIB_PATH to the exact location of libonnxruntime
    // This is the recommended way for the `ort` crate's load-dynamic feature.
    // DYLD_LIBRARY_PATH set at runtime via set_var may not affect dlopen on macOS.
    if std::env::var("ORT_DYLIB_PATH").is_err() {
        let ort_filename = if cfg!(target_os = "macos") {
            "libonnxruntime.dylib"
        } else if cfg!(target_os = "windows") {
            "onnxruntime.dll"
        } else {
            "libonnxruntime.so"
        };

        for dir in &paths {
            let ort_path = std::path::Path::new(dir).join(ort_filename);
            if ort_path.exists() {
                tracing::info!("[NativeLibs] Setting ORT_DYLIB_PATH = {}", ort_path.display());
                std::env::set_var("ORT_DYLIB_PATH", &ort_path);
                break;
            }
        }
    }
}

impl ImageAnalyzer {
    pub fn new() -> Self {
        Self {
            images_processed: AtomicU64::new(0),
            total_processing_time_ms: AtomicU64::new(0),
            detections_found: AtomicU64::new(0),
            #[cfg(not(target_arch = "wasm32"))]
            detector: Mutex::new(YOLODetector::new(0.25, 0.45, "v8", "n")),
            confidence_threshold: Mutex::new(0.25),
            nms_threshold: Mutex::new(0.45),
            model_version: Mutex::new("v8-n".to_string()),
        }
    }

    /// Analyze image data and return detection results
    pub fn analyze_image(&self, data: &[u8]) -> Result<AnalysisResult> {
        let start = std::time::Instant::now();

        #[cfg(not(target_arch = "wasm32"))]
        let (objects, description, model_loaded, model_error) = {
            let mut detector = self.detector.lock();
            detector.ensure_loaded();

            if let Some(ref mut model) = detector.model {
                match Self::run_detection(model, data) {
                    Ok(detections) => {
                        tracing::info!("[ImageAnalyzer] YOLO detected {} objects", detections.len());
                        let desc = format!("YOLO detected {} objects", detections.len());
                        (detections, desc, true, None)
                    }
                    Err(e) => {
                        tracing::error!("[ImageAnalyzer] YOLO inference error: {}", e);
                        let (objs, desc) = self.fallback_analysis(data);
                        (objs, desc, false, Some(e))
                    }
                }
            } else {
                tracing::warn!("[ImageAnalyzer] Model not loaded, using fallback");
                let (objs, desc) = self.fallback_analysis(data);
                (objs, desc, false, detector.load_error.clone())
            }
        };

        #[cfg(target_arch = "wasm32")]
        let (objects, description, model_loaded, model_error) = {
            let (objs, desc) = self.fallback_analysis(data);
            (objs, desc, false, Some("YOLO not available in WASM".to_string()))
        };

        let processing_time = start.elapsed().as_millis() as u64;

        // Update stats
        self.images_processed.fetch_add(1, Ordering::SeqCst);
        self.total_processing_time_ms.fetch_add(processing_time, Ordering::SeqCst);
        self.detections_found.fetch_add(objects.len() as u64, Ordering::SeqCst);

        Ok(AnalysisResult {
            objects,
            description,
            processing_time_ms: processing_time,
            model_loaded,
            model_error,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_detection(model: &mut YOLO, image_data: &[u8]) -> std::result::Result<Vec<Detection>, String> {
        // Create temporary file for image data
        let temp_path = std::env::temp_dir().join(format!("neomind_img_{}.jpg", std::process::id()));
        std::fs::write(&temp_path, image_data)
            .map_err(|e| format!("Failed to write temp image: {}", e))?;

        // Load image using usls DataLoader - use try_read_one for single image
        let temp_path_str = temp_path.to_str().ok_or("Invalid temp path")?;
        let image = DataLoader::try_read_one(temp_path_str)
            .map_err(|e| format!("Failed to load image: {}", e))?;

        // Run inference - forward() requires a slice of images
        let xs = vec![image];
        let ys = model.forward(&xs)
            .map_err(|e| format!("Inference failed: {}", e))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        // Convert results - new API returns Y struct with hbbs field
        let mut detections = Vec::new();

        for y in ys.iter() {
            // Get bounding boxes from hbbs field
            for hbb in &y.hbbs {
                // Get class ID - hbb.id() returns Option<usize>
                let class_id = hbb.id().unwrap_or(0);
                let label = if class_id < COCO_CLASSES.len() {
                    COCO_CLASSES[class_id].to_string()
                } else {
                    // Fallback to name() if available
                    hbb.name().unwrap_or(&format!("class_{}", class_id)).to_string()
                };

                // New API: hbb has xmin(), ymin(), xmax(), ymax() instead of bbox()
                let xmin = hbb.xmin();
                let ymin = hbb.ymin();
                let xmax = hbb.xmax();
                let ymax = hbb.ymax();

                detections.push(Detection {
                    label,
                    confidence: hbb.confidence().unwrap_or(0.0),
                    bbox: Some(BoundingBox {
                        x: xmin,
                        y: ymin,
                        width: xmax - xmin,
                        height: ymax - ymin,
                    }),
                });
            }
        }

        Ok(detections)
    }

    /// Fallback analysis when YOLO is not available
    pub fn fallback_analysis(&self, data: &[u8]) -> (Vec<Detection>, String) {
        let size = data.len();
        let mut objects = Vec::new();

        // Check image format
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            objects.push(Detection {
                label: "jpeg_image".to_string(),
                confidence: 0.95,
                bbox: None,
            });
        } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            objects.push(Detection {
                label: "png_image".to_string(),
                confidence: 0.95,
                bbox: None,
            });
        }

        let description = format!(
            "Fallback analysis (YOLO unavailable). Size: {} bytes.",
            size
        );

        (objects, description)
    }

    /// Reset statistics
    pub fn reset_stats(&self) -> serde_json::Value {
        self.images_processed.store(0, Ordering::SeqCst);
        self.total_processing_time_ms.store(0, Ordering::SeqCst);
        self.detections_found.store(0, Ordering::SeqCst);
        json!({"status": "reset"})
    }

    /// Get model status for diagnostics
    pub fn get_model_status(&self) -> serde_json::Value {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut detector = self.detector.lock();
            detector.ensure_loaded();
            json!({
                "loaded": detector.model.is_some(),
                "error": detector.load_error,
                "confidence_threshold": *self.confidence_threshold.lock(),
                "nms_threshold": *self.nms_threshold.lock(),
                "model_version": self.model_version.lock().clone(),
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            json!({
                "loaded": false,
                "error": "YOLO not available in WASM"
            })
        }
    }

    /// Reload model with new configuration
    #[cfg(not(target_arch = "wasm32"))]
    pub fn reload_model(&self) -> std::result::Result<(), String> {
        let conf = *self.confidence_threshold.lock();
        let nms = *self.nms_threshold.lock();
        let version_str = self.model_version.lock().clone();

        // Parse version string (e.g., "v8-n" -> version="v8", scale="n")
        let parts: Vec<&str> = version_str.split('-').collect();
        let (version, scale) = if parts.len() >= 2 {
            (parts[0], parts[1])
        } else {
            ("v8", "n")
        };

        let mut detector = self.detector.lock();
        // Reset and force reload
        *detector = YOLODetector::new(conf, nms, version, scale);
        detector.ensure_loaded();

        if detector.model.is_some() {
            Ok(())
        } else {
            Err(detector.load_error.clone().unwrap_or_else(|| "Unknown error".to_string()))
        }
    }
}

impl Default for ImageAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// YOLO Detector Implementation
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
impl YOLODetector {
    /// Create a new detector without loading the model (lazy initialization)
    fn new(conf: f32, iou: f32, version: &str, scale: &str) -> Self {
        Self {
            model: None,
            load_error: None,
            conf,
            iou,
            version: version.to_string(),
            scale: scale.to_string(),
            load_attempted: false,
        }
    }

    /// Ensure the model is loaded (lazy init on first use)
    fn ensure_loaded(&mut self) {
        if self.load_attempted {
            return;
        }
        self.load_attempted = true;

        // Set up native library paths before ONNX Runtime is loaded
        setup_native_lib_paths();

        tracing::info!("[YOLODetector] Lazy loading model: {}-{}", self.version, self.scale);
        match Self::try_load_model(self.conf, self.iou, &self.version, &self.scale) {
            Ok(model) => {
                tracing::info!("[YOLODetector] Model loaded successfully: {}-{}", self.version, self.scale);
                self.model = Some(model);
            }
            Err(e) => {
                tracing::error!("[YOLODetector] Failed to load model: {}", e);
                self.load_error = Some(e);
            }
        }
    }

    fn try_load_model(conf: f32, iou: f32, version: &str, _scale: &str) -> std::result::Result<YOLO, String> {
        // Parse version number from string (e.g., "v8" -> 8)
        let version_num: u8 = version.trim_start_matches('v')
            .parse()
            .map_err(|_| format!("Invalid version: {}", version))?;

        tracing::info!("Loading YOLO v{} model from local file...", version_num);

        // Load model data from local models directory
        let model_data = Self::load_model_data(version_num)?;

        if model_data.is_none() {
            return Err(format!("YOLOv{} model not found in models directory", version_num));
        }

        let model_bytes = model_data.unwrap();
        let model_size = model_bytes.len();
        tracing::info!("Loading YOLO model ({} bytes)", model_size);

        // Save model to temp file (usls requires file path)
        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join(format!("yolov{}n.onnx", version_num));
        std::fs::write(&model_path, &model_bytes)
            .map_err(|e| format!("Failed to write temp model file: {}", e))?;

        // Create config using usls 0.1.11 API
        let config = Config::yolo()
            .with_model_file(model_path.to_str().unwrap())
            .with_version(YOLOVersion(version_num, 0, None))
            .with_class_confs(&[conf])
            .with_iou(iou);

        // Create YOLO model with hardware acceleration + CPU fallback
        let model = with_device_fallback(|device| {
            let cfg = config.clone()
                .with_device_all(device)
                .commit()
                .map_err(|e| format!("Config failed: {:?}", e))?;
            YOLO::new(cfg)
                .map_err(|e| format!("Model failed: {:?}", e))
        })?;

        tracing::info!("✓ YOLO model loaded successfully");

        // Clean up temp file
        let _ = std::fs::remove_file(&model_path);

        Ok(model)
    }

    /// Load model data from disk
    fn load_model_data(version: u8) -> std::result::Result<Option<Vec<u8>>, String> {
        let model_filename = format!("yolov{}n.onnx", version);

        // Try to get extension directory from environment variable (set by runner)
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            tracing::info!("[ImageAnalyzer] NEOMIND_EXTENSION_DIR = {}", ext_dir);
            
            // Primary path: <extension_dir>/models/yolov8n.onnx
            let model_path = std::path::PathBuf::from(&ext_dir).join("models").join(&model_filename);
            tracing::info!("[ImageAnalyzer] Checking primary path: {}", model_path.display());
            
            if model_path.exists() {
                tracing::info!("[ImageAnalyzer] ✓ Found model at: {}", model_path.display());
                return std::fs::read(&model_path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            } else {
                tracing::warn!("[ImageAnalyzer] ✗ Model not found at primary path");
            }
        } else {
            tracing::warn!("[ImageAnalyzer] NEOMIND_EXTENSION_DIR not set");
        }

        // Fallback: Try to find model relative to current working directory
        // When running in isolated process, the working directory should be the extension root
        tracing::info!("[ImageAnalyzer] Checking current working directory...");
        if let Ok(cwd) = std::env::current_dir() {
            tracing::info!("[ImageAnalyzer] Current working directory: {}", cwd.display());
            
            let model_path = cwd.join("models").join(&model_filename);
            tracing::info!("[ImageAnalyzer] Checking: {}", model_path.display());
            
            if model_path.exists() {
                tracing::info!("[ImageAnalyzer] ✓ Found model at: {}", model_path.display());
                return std::fs::read(&model_path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            }
        }

        // Additional fallback paths
        let fallback_paths = vec![
            std::path::PathBuf::from("models").join(&model_filename),
            std::path::PathBuf::from("../models").join(&model_filename),
            std::path::PathBuf::from("../../models").join(&model_filename),
        ];

        for path in &fallback_paths {
            tracing::info!("[ImageAnalyzer] Checking fallback path: {}", path.display());
            if path.exists() {
                tracing::info!("[ImageAnalyzer] ✓ Found model at: {}", path.display());
                return std::fs::read(path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            }
        }

        tracing::error!("[ImageAnalyzer] ❌ Model not found in any location");
        Ok(None)
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for ImageAnalyzer {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                ExtensionMetadata::new(
                    "image-analyzer-v2",
                    "Image Analyzer V2",
                    "2.0.0"
                )
                .with_description("Image analysis with YOLOv8 via usls")
                .with_author("NeoMind Team")
            }
            #[cfg(target_arch = "wasm32")]
            {
                ExtensionMetadata::new(
                    "image-analyzer-v2",
                    "Image Analyzer V2",
                    "2.0.0"
                )
                .with_description("Image analysis with YOLOv8 via usls")
                .with_author("NeoMind Team")
            }
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "images_processed".to_string(),
                display_name: "Images Processed".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "avg_processing_time_ms".to_string(),
                display_name: "Avg Processing Time".to_string(),
                data_type: MetricDataType::Float,
                unit: "ms".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_detections".to_string(),
                display_name: "Total Detections".to_string(),
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
                name: "analyze_image".to_string(),
                display_name: "Analyze Image".to_string(),
                description: "Analyze an image and return detected objects with bounding boxes".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "image".to_string(),
                        display_name: "Image".to_string(),
                        description: "Base64 encoded image data".to_string(),
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
                    serde_json::json!({ "image": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==" }),
                ],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "reset_stats".to_string(),
                display_name: "Reset Statistics".to_string(),
                description: "Reset extension statistics".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: Vec::new(),
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".to_string(),
                display_name: "Get Model Status".to_string(),
                description: "Get current model loading status and configuration".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: Vec::new(),
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "reload_model".to_string(),
                display_name: "Reload Model".to_string(),
                description: "Reload YOLO model with current configuration".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: Vec::new(),
                parameter_groups: Vec::new(),
            },
        ]
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // Get current timestamp - use wasmbind for WASM compatibility
        #[cfg(not(target_arch = "wasm32"))]
        let now = chrono::Utc::now().timestamp_millis();
        
        #[cfg(target_arch = "wasm32")]
        let now = {
            // For WASM, use js_sys for timestamp
            js_sys::Date::now() as i64
        };
        
        let images = self.images_processed.load(Ordering::SeqCst);
        let total_time = self.total_processing_time_ms.load(Ordering::SeqCst);
        let avg_time = if images > 0 {
            total_time as f64 / images as f64
        } else {
            0.0
        };

        Ok(vec![
            ExtensionMetricValue {
                name: "images_processed".to_string(),
                value: ParamMetricValue::Integer(images as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "avg_processing_time_ms".to_string(),
                value: ParamMetricValue::Float(avg_time),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_detections".to_string(),
                value: ParamMetricValue::Integer(self.detections_found.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
        ])
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "analyze_image" => {
                let image_b64 = args.get("image")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'image' parameter".to_string()))?;

                let image_data = base64::engine::general_purpose::STANDARD.decode(image_b64)
                    .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid base64: {}", e)))?;

                let result = self.analyze_image(&image_data)?;
                Ok(serde_json::to_value(result)
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("Serialization error: {}", e)))?)
            }
            "reset_stats" => {
                Ok(self.reset_stats())
            }
            "get_status" => {
                Ok(self.get_model_status())
            }
            "reload_model" => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.reload_model()
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("Model reload failed: {}", e)))?;
                    Ok(json!({"status": "reloaded"}))
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Err(ExtensionError::NotSupported("Model reload not available in WASM".to_string()))
                }
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// FFI Export
// ============================================================================

neomind_extension_sdk::neomind_export!(ImageAnalyzer);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_extension_metadata() {
        let ext = ImageAnalyzer::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "image-analyzer-v2");
        assert_eq!(meta.name, "Image Analyzer V2");
        assert_eq!(meta.description.as_deref(), Some("Image analysis with YOLOv8 via usls"));
    }

    #[test]
    fn test_metadata_json_matches_runtime_metadata() {
        let ext = ImageAnalyzer::new();
        let meta = ext.metadata();
        let metadata_json: Value = serde_json::from_str(include_str!("../metadata.json")).unwrap();

        assert_eq!(metadata_json["id"], meta.id);
        assert_eq!(metadata_json["name"], meta.name);
        assert_eq!(
            metadata_json["license"],
            "Apache-2.0"
        );
        assert_eq!(
            metadata_json["repository"],
            "https://github.com/camthink-ai/NeoMind-Extensions"
        );
    }

    #[test]
    fn test_extension_metrics() {
        let ext = ImageAnalyzer::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 3);
    }

    #[test]
    fn test_extension_commands() {
        let ext = ImageAnalyzer::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 4);
        assert_eq!(commands[0].name, "analyze_image");
    }

    #[test]
    fn test_fallback_analysis() {
        let ext = ImageAnalyzer::new();
        let (objects, description) = ext.fallback_analysis(&[0xFF, 0xD8, 0xFF, 0x00]);

        assert!(!objects.is_empty());
        assert!(description.contains("Fallback"));
    }
}
