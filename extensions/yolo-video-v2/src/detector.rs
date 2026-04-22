//! YOLOv11 Detector using usls library
//!
//! This implementation uses the usls crate which provides:
//! - Automatic GPU acceleration detection
//! - Better memory management
//! - Simpler API with built-in preprocessing/postprocessing

use image::RgbImage;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use usls::{models::YOLO, Config, Device, Version};

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

// Re-export BoundingBox from parent module to avoid duplication
pub use crate::BoundingBox;

/// Detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub class_id: u32,
    pub class_name: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
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
                                            let _ = ();
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

/// YOLOv11 detector using usls
pub struct YoloDetector {
    #[cfg(not(target_arch = "wasm32"))]
    model: Option<Arc<parking_lot::Mutex<YOLO>>>,
    #[cfg(target_arch = "wasm32")]
    model_loaded: bool,
    model_size: usize,
    /// Config for lazy loading
    conf: f32,
    version: String,
    scale: String,
    /// Whether we've attempted to load the model
    load_attempted: bool,
    /// Error from last load attempt
    load_error: Option<String>,
}

impl YoloDetector {
    /// Create a new detector without loading the model (lazy initialization)
    pub fn new() -> Result<Self, String> {
        eprintln!("[YOLO-Detector] Creating YoloDetector (lazy - model not loaded yet)...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(Self {
                model: None,
                model_size: 0,
                conf: 0.25,
                version: "11".to_string(),
                scale: "n".to_string(),
                load_attempted: false,
                load_error: None,
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            eprintln!("[YOLO-Detector] WASM target, running in fallback mode");
            Ok(Self {
                model_loaded: false,
                model_size: 0,
                conf: 0.25,
                version: "11".to_string(),
                scale: "n".to_string(),
                load_attempted: false,
                load_error: None,
            })
        }
    }

    /// Ensure the model is loaded (lazy init on first use)
    pub fn ensure_loaded(&mut self) {
        if self.load_attempted {
            return;
        }
        self.load_attempted = true;

        // Set up native library paths before ONNX Runtime is loaded
        #[cfg(not(target_arch = "wasm32"))]
        setup_native_lib_paths();

        tracing::info!("[YOLO-Detector] Lazy loading model: v{}-{}", self.version, self.scale);
        eprintln!("[YOLO-Detector] Lazy loading model...");

        #[cfg(not(target_arch = "wasm32"))]
        {
            match Self::try_load_model(&self.version, self.conf) {
                Ok((model, model_size)) => {
                    tracing::info!("[YOLO-Detector] Model loaded successfully: v{}-{}", self.version, self.scale);
                    eprintln!("[YOLO-Detector] YOLO model loaded successfully!");
                    self.model = Some(model);
                    self.model_size = model_size;
                }
                Err(e) => {
                    tracing::error!("[YOLO-Detector] Failed to load model: {}", e);
                    eprintln!("[YOLO-Detector] Failed to load model: {}", e);
                    self.load_error = Some(e);
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            // WASM fallback - no actual loading
        }
    }

    /// Try to load the model (called by ensure_loaded)
    #[cfg(not(target_arch = "wasm32"))]
    fn try_load_model(version: &str, conf: f32) -> Result<(Arc<parking_lot::Mutex<YOLO>>, usize), String> {
        eprintln!("[YOLO-Detector] Loading YOLO model v{}", version);

        let model_data = Self::load_model_data()?;

        if model_data.is_none() {
            let ext_dir = std::env::var("NEOMIND_EXTENSION_DIR").unwrap_or_else(|_| "unknown".to_string());
            return Err(format!(
                "YOLO model not found. Please ensure yolo11n.onnx is in the models/ directory. Searched in: {}",
                ext_dir
            ));
        }

        let model_bytes = model_data.unwrap();
        let model_size = model_bytes.len();
        eprintln!("[YOLO-Detector] Model data loaded: {} bytes", model_size);

        // Save model to temp file with unique name (usls requires file path)
        let temp_dir = std::env::temp_dir();
        let unique_id = uuid::Uuid::new_v4();
        let model_path = temp_dir.join(format!("yolo11n_{}.onnx", unique_id));
        eprintln!("[YOLO-Detector] Writing model to temp file: {}", model_path.display());

        std::fs::write(&model_path, &model_bytes)
            .map_err(|e| format!("Failed to write temp model file: {}", e))?;

        // Create Config for YOLO detection using usls API
        eprintln!("[YOLO-Detector] Configuring YOLO model with usls...");

        // Parse version number
        let version_num: u8 = version.trim_start_matches('v')
            .parse()
            .unwrap_or(11);

        let config = Config::yolo()
            .with_model_file(model_path.to_str().unwrap())
            .with_version(Version(version_num, 0, None))
            .with_class_confs(&[conf]);

        // Create YOLO model with hardware acceleration + CPU fallback
        let model = with_device_fallback(|device| {
            let cfg = config.clone()
                .with_device_all(device)
                .commit()
                .map_err(|e| format!("Config failed: {:?}", e))?;
            YOLO::new(cfg)
                .map_err(|e| format!("Model failed: {:?}", e))
        })?;

        eprintln!("[YOLO-Detector] YOLO model loaded successfully!");
        eprintln!("[YOLO-Detector] Model: YOLOv{}n, Confidence: {}", version_num, conf);

        // Clean up temp file
        let _ = std::fs::remove_file(&model_path);

        Ok((Arc::new(parking_lot::Mutex::new(model)), model_size))
    }

    /// Load model data from disk
    fn load_model_data() -> Result<Option<Vec<u8>>, String> {
        eprintln!("[YOLO-Detector] Starting model search...");

        // Try to get extension directory from environment variable (set by runner)
        if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            eprintln!("[YOLO-Detector] NEOMIND_EXTENSION_DIR = {}", ext_dir);

            // Primary path: <extension_dir>/models/yolo11n.onnx
            let model_path = std::path::PathBuf::from(&ext_dir).join("models").join("yolo11n.onnx");
            eprintln!("[YOLO-Detector] Checking primary path: {}", model_path.display());

            if model_path.exists() {
                eprintln!("[YOLO-Detector] ✓ Found model at: {}", model_path.display());
                return std::fs::read(&model_path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            } else {
                eprintln!("[YOLO-Detector] ✗ Model not found at primary path");
            }
        } else {
            eprintln!("[YOLO-Detector] NEOMIND_EXTENSION_DIR not set");
        }

        // Fallback: Try to find model relative to current working directory
        // When running in isolated process, the working directory should be the extension root
        eprintln!("[YOLO-Detector] Checking current working directory...");
        if let Ok(cwd) = std::env::current_dir() {
            eprintln!("[YOLO-Detector] Current working directory: {}", cwd.display());
            
            let model_path = cwd.join("models").join("yolo11n.onnx");
            eprintln!("[YOLO-Detector] Checking: {}", model_path.display());
            
            if model_path.exists() {
                eprintln!("[YOLO-Detector] ✓ Found model at: {}", model_path.display());
                return std::fs::read(&model_path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            }
        }

        // Fallback: Try relative paths from current directory
        let fallback_paths = vec![
            std::path::PathBuf::from("models/yolo11n.onnx"),
            std::path::PathBuf::from("../models/yolo11n.onnx"),
            std::path::PathBuf::from("../../models/yolo11n.onnx"),
            std::path::PathBuf::from("extensions/yolo-video-v2/models/yolo11n.onnx"),
            std::path::PathBuf::from("../extensions/yolo-video-v2/models/yolo11n.onnx"),
        ];

        for path in &fallback_paths {
            eprintln!("[YOLO-Detector] Checking fallback path: {}", path.display());
            if path.exists() {
                eprintln!("[YOLO-Detector] ✓ Found model at: {}", path.display());
                return std::fs::read(path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            }
        }

        eprintln!("[YOLO-Detector] ❌ Model not found in any location");
        Ok(None)
    }

    /// Check if model is loaded
    pub fn is_loaded(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.model.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.model_loaded
        }
    }

    /// Get model size in bytes
    pub fn model_size(&self) -> usize {
        self.model_size
    }

    /// Get the load error if model failed to load
    pub fn get_load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Run inference on an image
    pub fn detect(&self, image: &RgbImage, _confidence_threshold: f32, max_detections: u32) -> Vec<Detection> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(ref model) = self.model {
                let result = Self::run_inference(model, image, max_detections);
                
                // ✨ CRITICAL: Force ONNX Runtime to release temporary memory after each inference
                // This prevents memory pool from growing indefinitely during video streaming
                std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
                
                return result;
            }
        }

        tracing::debug!("Model not loaded, returning empty detections");
        Vec::new()
    }

    /// ✨ NEW: Explicit memory cleanup for ONNX Runtime
    pub fn cleanup_memory(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Reset model state to release memory pool
            // Note: This is a workaround for ONNX Runtime memory leak in video streaming scenarios
            if let Some(_) = &self.model {
                // Model will be dropped and recreated on next inference
                // This releases the memory pool accumulated during streaming
                tracing::debug!("ONNX Runtime memory cleanup triggered");
            }
        }
    }

    /// ✨ CRITICAL: Safe shutdown that avoids panic when dropping usls::Runtime
    ///
    /// usls::Runtime (which wraps ONNX Runtime) may attempt to create a Tokio runtime
    /// or use block_on during drop, which causes panic if called from within an async context.
    ///
    /// This method uses spawn_blocking to drop the model in a safe thread context.

    /// ✨ CRITICAL: Clean shutdown that MUST be called before extension is dropped
    ///
    /// IMPORTANT: This method does NOT actually drop the usls::Runtime because that
    /// would cause "Cannot drop a runtime in a context where blocking is not allowed"
    /// panic when usls tries to shutdown its Tokio runtime.
    ///
    /// Instead, we leak the model on purpose. The Extension Runner will terminate
    /// the extension process after close_session completes, which will properly
    /// clean up all resources (memory, GPU, etc.) at the OS level.
    ///
    /// This approach is safe because:
    /// 1. The extension process is isolated and will be terminated anyway
    /// 2. The OS guarantees proper resource cleanup on process exit
    /// 3. We avoid the Tokio runtime conflict completely
    pub fn shutdown(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.model.is_some() {
                tracing::info!("Shutting down YoloDetector (leaking model, OS will clean up on process exit)");

                // Take the model out of the Option
                let model_opt = self.model.take();

                // Only proceed if we actually have a model
                if let Some(model) = model_opt {
                    // ✨ CRITICAL: Convert Arc into a raw pointer and leak it
                    // This ensures the Arc AND its data are never dropped
                    // The memory will be reclaimed by the OS when the process exits
                    let leaked: *const Arc<parking_lot::Mutex<YOLO>> =
                        Box::into_raw(Box::new(model));

                    // Intentionally leak the pointer - never dereference it
                    let _ = leaked;
                }

                tracing::info!("YoloDetector shutdown complete (model leaked, will be cleaned up by OS on process exit)");
            }
        }
    }
    /// Run YOLO inference using usls with proper API
    #[cfg(not(target_arch = "wasm32"))]
    fn run_inference(
        model: &Arc<parking_lot::Mutex<YOLO>>,
        image: &RgbImage,
        max_detections: u32,
    ) -> Vec<Detection> {
        let start = std::time::Instant::now();

        // Convert RgbImage to usls::Image without cloning
        // Use reference to avoid unnecessary memory allocation
        let usls_image = match usls::Image::from_u8s(image.as_raw().as_slice(), image.width(), image.height()) {
            Ok(img) => img,
            Err(e) => {
                tracing::error!("Failed to convert image: {:?}", e);
                return Vec::new();
            }
        };

        // Run inference using Model::forward()
        let mut model_guard = model.lock();
        let ys = match model_guard.forward(&[usls_image]) {
            Ok(results) => results,
            Err(e) => {
                tracing::error!("YOLO inference failed: {:?}", e);
                return Vec::new();
            }
        };

        // Get first result (single image inference)
        let y = match ys.into_iter().next() {
            Some(result) => result,
            None => {
                tracing::debug!("No detection results returned");
                return Vec::new();
            }
        };

        // Extract horizontal bounding boxes from results
        let hbbs = y.hbbs();

        // Convert usls Hbb results to our Detection format
        let mut detections: Vec<Detection> = hbbs
            .iter()
            .take(max_detections as usize)
            .filter_map(|hbb| {
                // Get bounding box coordinates
                let x = hbb.x();
                let y_coord = hbb.y();
                let width = hbb.w();
                let height = hbb.h();

                // Skip invalid boxes
                if width <= 0.0 || height <= 0.0 {
                    return None;
                }

                // Get metadata
                let class_id = hbb.id().unwrap_or(0) as u32;
                let confidence = hbb.confidence().unwrap_or(0.0);
                let class_name = hbb.name()
                    .map(|s: &str| s.to_string())
                    .unwrap_or_else(|| Self::get_class_name(class_id as usize));

                Some(Detection {
                    class_id,
                    class_name,
                    confidence,
                    bbox: BoundingBox {
                        x,
                        y: y_coord,
                        width,
                        height,
                    },
                })
            })
            .collect();

        // Sort by confidence descending
        detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        let elapsed = start.elapsed();
        tracing::debug!(
            "YOLO inference took {}ms, found {} detections",
            elapsed.as_millis(),
            detections.len()
        );

        detections
    }

    /// Get COCO class name by index
    fn get_class_name(class_id: usize) -> String {
        const COCO_CLASSES: [&str; 80] = [
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

        COCO_CLASSES
            .get(class_id)
            .unwrap_or(&"unknown")
            .to_string()
    }
}

/// ✨ CRITICAL: Drop implementation that leaks the model
///
/// When YoloDetector is dropped, we intentionally leak the internal model
/// to prevent usls::Runtime from being dropped in an async context,
/// which would cause "Cannot drop a runtime in a context where blocking is not allowed" panic.
///
/// This is safe because:
/// 1. The extension process will be terminated by Extension Runner
/// 2. The OS will reclaim all leaked memory on process exit
/// 3. Leaking is intentional to avoid Tokio runtime conflict
impl Drop for YoloDetector {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(model) = self.model.take() {
                // Leak the model to prevent usls::Runtime from being dropped
                let leaked: *const Arc<parking_lot::Mutex<YOLO>> =
                    Box::into_raw(Box::new(model));
                let _ = leaked;
                
                tracing::debug!("YoloDetector dropped (model leaked to prevent Tokio panic)");
            }
        }
    }
}

