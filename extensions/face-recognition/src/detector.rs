//! SCRFD face detector module using ONNX Runtime.
//!
//! Implements face detection using the SCRFD (Sample and Computation Redistribution
//! for Face Detection) model in ONNX format. Supports lazy model loading, dynamic
//! output tensor discovery, anchor-based decoding, and NMS post-processing.

use crate::{FaceBox, Landmark};
use std::path::PathBuf;

// ============================================================================
// Native Library Path Setup (copied from yolo-device-inference pattern)
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

    // 1. Extension's bundled libraries
    if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
        let ext_path = std::path::Path::new(&ext_dir);

        let lib_dir = ext_path.join("lib");
        if lib_dir.is_dir() {
            tracing::info!("[NativeLibs] Adding extension lib dir: {}", lib_dir.display());
            paths.push(lib_dir.to_string_lossy().to_string());
        }

        let binaries_dir = ext_path.join("binaries");
        if binaries_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&binaries_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        tracing::info!("[NativeLibs] Adding platform dir: {}", path.display());
                        paths.push(path.to_string_lossy().to_string());

                        if let Ok(files) = std::fs::read_dir(&path) {
                            for file in files.flatten() {
                                let file_path = file.path();
                                let name =
                                    file_path.file_name().unwrap_or_default().to_string_lossy();
                                if let Some(base) = name
                                    .strip_suffix(".dylib")
                                    .or_else(|| name.strip_suffix(".so"))
                                {
                                    if base.contains('.') {
                                        let unversioned = if cfg!(target_os = "macos") {
                                            format!(
                                                "{}.dylib",
                                                base.split('.').next().unwrap_or(base)
                                            )
                                        } else {
                                            format!("{}.so", base.split('.').next().unwrap_or(base))
                                        };
                                        let link_path = path.join(&unversioned);
                                        if !link_path.exists() {
                                            #[cfg(unix)]
                                            let _ =
                                                std::os::unix::fs::symlink(&file_path, &link_path);
                                            #[cfg(not(unix))]
                                            let _ = ();
                                            tracing::info!(
                                                "[NativeLibs] Created symlink: {} -> {}",
                                                unversioned,
                                                name
                                            );
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

    if let Ok(cwd) = std::env::current_dir() {
        let lib_dir = cwd.join("lib");
        if lib_dir.is_dir() {
            paths.push(lib_dir.to_string_lossy().to_string());
        }
    }

    if let Ok(existing) = std::env::var(lib_env) {
        paths.push(existing);
    }

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
                tracing::info!(
                    "[NativeLibs] Setting ORT_DYLIB_PATH = {}",
                    ort_path.display()
                );
                std::env::set_var("ORT_DYLIB_PATH", &ort_path);
                break;
            }
        }
    }
}

// ============================================================================
// Trait Definition
// ============================================================================

/// Trait for face detection implementations.
pub trait FaceDetect {
    /// Detect faces in the given image data.
    ///
    /// # Arguments
    /// * `image_data` - Raw image bytes (JPEG, PNG, etc.)
    /// * `confidence` - Minimum confidence threshold (0.0 - 1.0)
    ///
    /// # Returns
    /// A vector of detected face bounding boxes with optional landmarks.
    fn detect(&mut self, image_data: &[u8], confidence: f32) -> Result<Vec<FaceBox>, String>;

    /// Downcast to Any for type-specific access (e.g., model status checks).
    fn as_any(&self) -> &dyn std::any::Any;
}

// ============================================================================
// SCRFD Detector (non-WASM)
// ============================================================================

/// SCRFD face detector using ONNX Runtime.
///
/// Supports lazy model loading: the ONNX session is not created until
/// `ensure_loaded()` is called on first detection request.
#[cfg(not(target_arch = "wasm32"))]
pub struct ScrfdDetector {
    /// Loaded ONNX session (None until first use)
    model: Option<ort::session::Session>,
    /// Whether we've already attempted to load the model
    load_attempted: bool,
    /// Last loading error, if any
    load_error: Option<String>,
    /// Model input size (width and height, assumed square)
    input_size: u32,
}

#[cfg(not(target_arch = "wasm32"))]
impl ScrfdDetector {
    /// Create a new detector WITHOUT loading the model (lazy initialization).
    pub fn new() -> Self {
        Self {
            model: None,
            load_attempted: false,
            load_error: None,
            input_size: 640,
        }
    }

    /// Create a new detector with a custom input size.
    pub fn with_input_size(input_size: u32) -> Self {
        Self {
            model: None,
            load_attempted: false,
            load_error: None,
            input_size,
        }
    }

    /// Ensure the model is loaded. On the first call this performs the actual
    /// ONNX Runtime initialization; subsequent calls are no-ops.
    pub fn ensure_loaded(&mut self) {
        if self.load_attempted {
            return;
        }
        self.load_attempted = true;

        // Set up native library paths before ONNX Runtime is loaded
        setup_native_lib_paths();

        tracing::info!("[ScrfdDetector] Lazy loading SCRFD model (det_10g.onnx)");
        match Self::try_load_model() {
            Ok(session) => {
                tracing::info!("[ScrfdDetector] SCRFD model loaded successfully");
                self.model = Some(session);
            }
            Err(e) => {
                tracing::error!("[ScrfdDetector] Failed to load model: {}", e);
                self.load_error = Some(e);
            }
        }
    }

    /// Check if the model is loaded and ready for inference.
    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    /// Get the last loading error, if any.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Attempt to load the SCRFD model from disk.
    fn try_load_model() -> Result<ort::session::Session, String> {
        let model_path = find_model_path("det_10g.onnx")?;

        tracing::info!(
            "[ScrfdDetector] Loading model file: {}",
            model_path.display()
        );

        let session = ort::session::Session::builder()
            .map_err(|e| format!("Failed to create session builder: {}", e))?
            .commit_from_file(&model_path)
            .map_err(|e| {
                format!(
                    "Failed to load ONNX model from {}: {}",
                    model_path.display(),
                    e
                )
            })?;

        Ok(session)
    }

    /// Detect faces in the given image data using the SCRFD model.
    ///
    /// Preprocessing: resize to input_size x input_size, normalize with
    /// `(pixel - 127.5) / 128.0`, NCHW layout.
    ///
    /// Postprocessing: dynamic output tensor discovery (group by stride),
    /// anchor decode, NMS, extract 5-point landmarks.
    pub fn detect_impl(
        &mut self,
        image_data: &[u8],
        confidence: f32,
    ) -> Result<Vec<FaceBox>, String> {
        self.ensure_loaded();

        let session = self.model.as_mut().ok_or_else(|| {
            self.load_error
                .clone()
                .unwrap_or_else(|| "Model not loaded".to_string())
        })?;

        // Decode image
        let img = image::load_from_memory(image_data)
            .map_err(|e| format!("Failed to decode image: {}", e))?;
        let (orig_w, orig_h) = (img.width() as f64, img.height() as f64);
        let img = img.to_rgb8();

        let input_size = self.input_size;

        // Preprocess: resize to input_size x input_size
        let resized = image::imageops::resize(
            &img,
            input_size,
            input_size,
            image::imageops::FilterType::Triangle,
        );

        // Compute scale factors for mapping detections back to original coordinates
        let scale_w = orig_w / input_size as f64;
        let scale_h = orig_h / input_size as f64;

        // Normalize: (pixel - 127.5) / 128.0, NCHW layout
        let input_tensor = ndarray::Array4::from_shape_fn(
            (1, 3, input_size as usize, input_size as usize),
            |(_, c, y, x)| {
                let pixel = resized.get_pixel(x as u32, y as u32);
                (pixel[c] as f32 - 127.5) / 128.0
            },
        );

        // Create ONNX tensor input using Tensor::from_array with (shape, data) tuple.
        // The data must be in contiguous (row-major) layout.
        let input_shape = vec![
            1_i64,
            3_i64,
            input_size as i64,
            input_size as i64,
        ];
        let (raw_data, _offset) = input_tensor.into_raw_vec_and_offset();
        let input_value = ort::value::Tensor::from_array((
            input_shape,
            raw_data.into_boxed_slice(),
        ))
        .map_err(|e| format!("Failed to create input tensor: {}", e))?;

        // Discover output tensor names and group by stride BEFORE mutably borrowing session for run.
        let stride_groups = discover_stride_groups(session);

        // Run inference
        let outputs = session
            .run(ort::inputs![input_value])
            .map_err(|e| format!("ONNX inference failed: {}", e))?;

        // Decode detections from all stride groups
        let mut candidates: Vec<FaceCandidate> = Vec::new();

        for (stride, group) in &stride_groups {
            let stride_val = *stride;

            // Extract scores -- try_extract_tensor::<f32>() returns (&Shape, &[f32])
            let scores_output = outputs
                .get(&group.score_name)
                .ok_or_else(|| format!("Missing scores output: {}", group.score_name))?;
            let (scores_shape, scores_slice) = scores_output
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("Failed to extract scores: {}", e))?;

            // Extract bboxes
            let bboxes_output = outputs
                .get(&group.bbox_name)
                .ok_or_else(|| format!("Missing bboxes output: {}", group.bbox_name))?;
            let (bboxes_shape, bboxes_slice) = bboxes_output
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("Failed to extract bboxes: {}", e))?;

            // Extract keypoints (optional)
            let kps_slice = if let Some(ref kps_name) = group.kps_name {
                if let Some(kps_output) = outputs.get(kps_name) {
                    Some(
                        kps_output
                            .try_extract_tensor::<f32>()
                            .map_err(|e| format!("Failed to extract kps: {}", e))?,
                    )
                } else {
                    None
                }
            } else {
                None
            };

            // Determine number of anchors and bbox dimension from tensor shapes.
            // Scores shape: [1, num_anchors, 1] or [1, num_anchors]
            // Bboxes shape: [1, num_anchors, 4]
            let num_anchors = if scores_shape.len() >= 2 {
                scores_shape[1] as usize
            } else {
                0
            };
            if num_anchors == 0 {
                continue;
            }

            // Bbox element stride: number of f32 values per anchor in the bbox tensor
            let bbox_dim = if bboxes_shape.len() >= 3 {
                bboxes_shape[2] as usize
            } else {
                4
            };

            // Kps element stride: number of f32 values per anchor in the kps tensor
            let kps_dim: usize = kps_slice
                .as_ref()
                .map(|(shape, _)| {
                    if shape.len() >= 3 {
                        shape[2] as usize
                    } else {
                        10
                    }
                })
                .unwrap_or(10);

            // Compute feature map size for this stride
            let feat_size = input_size as usize / stride_val;

            // Decode anchors and collect candidates
            for anchor_idx in 0..num_anchors {
                // Score: squeeze last dim if present
                let score = scores_slice[anchor_idx];
                if score < confidence {
                    continue;
                }

                // Compute anchor center from grid position
                let (cx, cy) = {
                    let row = anchor_idx / feat_size;
                    let col = anchor_idx % feat_size;
                    (
                        (col as f64 + 0.5) * stride_val as f64,
                        (row as f64 + 0.5) * stride_val as f64,
                    )
                };

                // Decode bbox: distances from anchor center to edges
                let bbox_offset = anchor_idx * bbox_dim;
                let dl = bboxes_slice[bbox_offset] as f64;
                let dt = bboxes_slice[bbox_offset + 1] as f64;
                let dr = bboxes_slice[bbox_offset + 2] as f64;
                let db = bboxes_slice[bbox_offset + 3] as f64;

                let x = cx - dl;
                let y = cy - dt;
                let w = dl + dr;
                let h = dt + db;

                // Extract landmarks
                let landmarks = kps_slice.as_ref().map(|(_, kps_data)| {
                    let kps_offset = anchor_idx * kps_dim;
                    let mut pts = Vec::with_capacity(5);
                    for k in 0..5 {
                        let kx = kps_data[kps_offset + k * 2] as f64;
                        let ky = kps_data[kps_offset + k * 2 + 1] as f64;
                        pts.push(Landmark {
                            x: kx * scale_w,
                            y: ky * scale_h,
                        });
                    }
                    pts
                });

                candidates.push(FaceCandidate {
                    x: x * scale_w,
                    y: y * scale_h,
                    width: w * scale_w,
                    height: h * scale_h,
                    score: score as f64,
                    landmarks,
                });
            }
        }

        // Apply NMS
        let faces = nms(&candidates, 0.4);

        Ok(faces
            .into_iter()
            .map(|c| FaceBox {
                x: c.x,
                y: c.y,
                width: c.width,
                height: c.height,
                confidence: c.score,
                landmarks: c.landmarks,
            })
            .collect())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for ScrfdDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FaceDetect for ScrfdDetector {
    fn detect(&mut self, image_data: &[u8], confidence: f32) -> Result<Vec<FaceBox>, String> {
        self.detect_impl(image_data, confidence)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// SCRFD Detector stub (WASM)
// ============================================================================

#[cfg(target_arch = "wasm32")]
pub struct ScrfdDetector;

#[cfg(target_arch = "wasm32")]
impl ScrfdDetector {
    pub fn new() -> Self {
        Self
    }

    pub fn is_loaded(&self) -> bool {
        false
    }

    pub fn load_error(&self) -> Option<&str> {
        Some("SCRFD detector not available in WASM")
    }

    pub fn ensure_loaded(&mut self) {
        // No-op in WASM
    }
}

#[cfg(target_arch = "wasm32")]
impl Default for ScrfdDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
impl FaceDetect for ScrfdDetector {
    fn detect(&mut self, _image_data: &[u8], _confidence: f32) -> Result<Vec<FaceBox>, String> {
        Err("SCRFD detector not available in WASM".to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Model Path Discovery
// ============================================================================

/// Find model file by searching common locations.
pub fn find_model_path(filename: &str) -> Result<PathBuf, String> {
    // If NEOMIND_EXTENSION_DIR is set, use it exclusively
    if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
        let path = PathBuf::from(&ext_dir).join("models").join(filename);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("Model file '{}' not found in NEOMIND_EXTENSION_DIR/models ({})", filename, ext_dir));
    }

    // Fallback: Check current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join("models").join(filename);
        if path.exists() {
            return Ok(path);
        }
    }

    // Additional fallback paths
    let fallback_paths = vec![
        PathBuf::from("models").join(filename),
        PathBuf::from("../models").join(filename),
    ];

    for path in fallback_paths {
        if path.exists() {
            return Ok(path);
        }
    }

    Err(format!(
        "Model file '{}' not found in extension models directory",
        filename
    ))
}

// ============================================================================
// SCRFD Output Parsing
// ============================================================================

/// Grouped output tensors for a single stride level.
#[derive(Debug, Clone)]
struct StrideGroup {
    /// Name of the scores tensor (e.g., "score_8")
    score_name: String,
    /// Name of the bboxes tensor (e.g., "bbox_8")
    bbox_name: String,
    /// Name of the keypoints tensor (e.g., "kps_8"), if available
    kps_name: Option<String>,
}

/// Discover stride groups from ONNX session output names.
///
/// SCRFD model outputs are named with a stride suffix (e.g., "score_8", "bbox_8", "kps_8").
/// This function dynamically discovers available strides and groups the outputs.
#[cfg(not(target_arch = "wasm32"))]
fn discover_stride_groups(session: &ort::session::Session) -> Vec<(usize, StrideGroup)> {
    let output_names: Vec<&str> = session.outputs.iter().map(|o| o.name.as_str()).collect();

    // Collect all unique stride numbers found in output names
    let mut strides: Vec<usize> = Vec::new();
    for name in &output_names {
        if let Some(stride) = extract_stride_from_name(name) {
            if stride > 0 && !strides.contains(&stride) {
                strides.push(stride);
            }
        }
    }

    strides.sort();

    let mut groups = Vec::new();

    for stride in strides {
        let score_name = find_tensor_by_stride(&output_names, stride, &["score"])
            .unwrap_or_else(|| format!("score_{}", stride));
        let bbox_name = find_tensor_by_stride(&output_names, stride, &["bbox"])
            .unwrap_or_else(|| format!("bbox_{}", stride));
        let kps_name = find_tensor_by_stride(&output_names, stride, &["kps", "keypoint"]);

        groups.push((
            stride,
            StrideGroup {
                score_name,
                bbox_name,
                kps_name,
            },
        ));
    }

    groups
}

/// Extract stride number from an output tensor name.
///
/// Tries to find a numeric suffix in the name. For example:
/// - "score_8" -> Some(8)
/// - "bbox_16" -> Some(16)
/// - "kps_32" -> Some(32)
/// - "stride8" -> Some(8)
fn extract_stride_from_name(name: &str) -> Option<usize> {
    let trimmed = name.trim();

    // Find the last sequence of digits
    let mut last_digits_start = None;
    let mut last_digits_end = None;

    for (i, c) in trimmed.char_indices().rev() {
        if c.is_ascii_digit() {
            if last_digits_end.is_none() {
                last_digits_end = Some(i + c.len_utf8());
            }
            last_digits_start = Some(i);
        } else if last_digits_end.is_some() {
            break;
        }
    }

    if let (Some(start), Some(end)) = (last_digits_start, last_digits_end) {
        let num_str = &trimmed[start..end];
        num_str.parse().ok()
    } else {
        None
    }
}

/// Find a tensor name matching a given stride and prefix list.
fn find_tensor_by_stride<'a>(
    names: &[&'a str],
    stride: usize,
    prefixes: &[&str],
) -> Option<String> {
    let stride_str = stride.to_string();
    for name in names {
        for prefix in prefixes {
            if name.contains(prefix) && name.contains(&stride_str) {
                return Some(name.to_string());
            }
        }
    }
    None
}

// ============================================================================
// NMS and Candidate Types
// ============================================================================

/// Internal candidate during detection, before NMS.
#[derive(Debug, Clone)]
struct FaceCandidate {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    score: f64,
    landmarks: Option<Vec<Landmark>>,
}

impl FaceCandidate {
    fn area(&self) -> f64 {
        self.width * self.height
    }

    fn iou(&self, other: &Self) -> f64 {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);

        let inter = ((x2 - x1).max(0.0)) * ((y2 - y1).max(0.0));
        let union = self.area() + other.area() - inter;

        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }
}

/// Apply greedy Non-Maximum Suppression.
fn nms(candidates: &[FaceCandidate], iou_threshold: f64) -> Vec<FaceCandidate> {
    if candidates.is_empty() {
        return Vec::new();
    }

    // Sort by score descending
    let mut indices: Vec<usize> = (0..candidates.len()).collect();
    indices.sort_by(|a, b| {
        candidates[*b]
            .score
            .partial_cmp(&candidates[*a].score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut keep = Vec::new();
    let mut suppressed = vec![false; candidates.len()];

    for &i in &indices {
        if suppressed[i] {
            continue;
        }
        keep.push(candidates[i].clone());

        for &j in &indices {
            if suppressed[j] || i == j {
                continue;
            }
            if candidates[i].iou(&candidates[j]) > iou_threshold {
                suppressed[j] = true;
            }
        }
    }

    keep
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify FaceDetect trait compiles and ScrfdDetector can be constructed.
    #[test]
    fn test_scrfd_detector_new_creates_unloaded_detector() {
        let detector = ScrfdDetector::new();
        assert!(
            !detector.is_loaded(),
            "New detector should not have a loaded model"
        );
        assert!(
            !detector.load_attempted,
            "New detector should not have attempted loading"
        );
        assert!(
            detector.load_error.is_none(),
            "New detector should not have a load error"
        );
    }

    /// Verify Default trait creates an equivalent detector.
    #[test]
    fn test_scrfd_detector_default() {
        let detector = ScrfdDetector::default();
        assert!(!detector.is_loaded());
        assert!(!detector.load_attempted);
    }

    /// Verify FaceDetect trait object can be created from ScrfdDetector,
    /// and that calling detect without a loaded model returns an error.
    #[test]
    fn test_face_detect_trait_object() {
        let mut detector: Box<dyn FaceDetect> = Box::new(ScrfdDetector::new());

        // Calling detect without a model should return an error
        let result = detector.detect(&[], 0.5);
        assert!(result.is_err(), "Detect without model should return error");
    }

    /// Verify ensure_loaded with missing model file returns error.
    /// Note: removed full ensure_loaded test due to env var race condition in parallel tests.
    /// The "model not found" path is covered by test_find_model_path_missing_file.

    /// Verify ensure_loaded is idempotent -- calling it twice does not re-load.
    #[test]
    fn test_ensure_loaded_idempotent() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        std::env::set_var("NEOMIND_EXTENSION_DIR", temp_dir.path().to_str().unwrap());

        let mut detector = ScrfdDetector::new();
        detector.ensure_loaded();
        assert!(detector.load_attempted);
        let first_error = detector.load_error.clone();

        // Call again -- should not change state
        detector.ensure_loaded();
        assert_eq!(
            detector.load_error, first_error,
            "Second ensure_loaded should not change load error"
        );

        std::env::remove_var("NEOMIND_EXTENSION_DIR");
    }

    /// Verify find_model_path returns error for missing model.
    #[test]
    fn test_find_model_path_missing_file() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        std::env::set_var("NEOMIND_EXTENSION_DIR", temp_dir.path().to_str().unwrap());

        let result = find_model_path("nonexistent_model.onnx");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("not found"),
            "Error should mention not found: {}",
            err
        );

        std::env::remove_var("NEOMIND_EXTENSION_DIR");
    }

    /// Verify find_model_path finds model in NEOMIND_EXTENSION_DIR/models.
    #[test]
    fn test_find_model_path_finds_in_extension_dir() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let models_dir = temp_dir.path().join("models");
        std::fs::create_dir_all(&models_dir).expect("Failed to create models dir");
        let model_file = models_dir.join("det_10g.onnx");
        std::fs::write(&model_file, b"fake model data").expect("Failed to write model file");

        std::env::set_var(
            "NEOMIND_EXTENSION_DIR",
            temp_dir.path().to_str().unwrap(),
        );

        let result = find_model_path("det_10g.onnx");
        assert!(result.is_ok(), "Should find model in extension dir");
        assert_eq!(
            result.unwrap(),
            model_file,
            "Should return the correct model path"
        );

        std::env::remove_var("NEOMIND_EXTENSION_DIR");
    }

    /// Verify stride extraction from output tensor names.
    #[test]
    fn test_extract_stride_from_name() {
        assert_eq!(extract_stride_from_name("score_8"), Some(8));
        assert_eq!(extract_stride_from_name("bbox_16"), Some(16));
        assert_eq!(extract_stride_from_name("kps_32"), Some(32));
        assert_eq!(extract_stride_from_name("stride8"), Some(8));
        assert_eq!(extract_stride_from_name("score_64"), Some(64));
        assert_eq!(extract_stride_from_name("score_128"), Some(128));
        assert_eq!(extract_stride_from_name("output0"), Some(0));
        assert_eq!(extract_stride_from_name("noscore"), None);
    }

    /// Verify discover_stride_groups correctly parses output names.
    #[test]
    fn test_discover_stride_groups_parsing() {
        // Verify that our name parsing functions work correctly with known patterns.
        // We can't test discover_stride_groups directly without an ONNX session,
        // so we test the helper functions.

        let names = vec![
            "score_8", "bbox_8", "kps_8",
            "score_16", "bbox_16", "kps_16",
            "score_32", "bbox_32", "kps_32",
        ];

        // Should find stride 8, 16, 32
        let stride8_score = find_tensor_by_stride(&names, 8, &["score"]);
        assert_eq!(stride8_score, Some("score_8".to_string()));

        let stride16_bbox = find_tensor_by_stride(&names, 16, &["bbox"]);
        assert_eq!(stride16_bbox, Some("bbox_16".to_string()));

        let stride32_kps = find_tensor_by_stride(&names, 32, &["kps"]);
        assert_eq!(stride32_kps, Some("kps_32".to_string()));

        // Non-existent stride should return None
        let stride64_score = find_tensor_by_stride(&names, 64, &["score"]);
        assert_eq!(stride64_score, None);
    }

    /// Verify NMS correctly filters overlapping detections.
    #[test]
    fn test_nms_filters_overlapping_candidates() {
        let candidates = vec![
            FaceCandidate {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 100.0,
                score: 0.95,
                landmarks: None,
            },
            // Overlapping with first, lower confidence
            FaceCandidate {
                x: 15.0,
                y: 15.0,
                width: 100.0,
                height: 100.0,
                score: 0.80,
                landmarks: None,
            },
            // Non-overlapping, should be kept
            FaceCandidate {
                x: 200.0,
                y: 200.0,
                width: 100.0,
                height: 100.0,
                score: 0.90,
                landmarks: None,
            },
        ];

        let result = nms(&candidates, 0.4);

        // Should keep the high-confidence overlapping one and the non-overlapping one
        assert_eq!(result.len(), 2, "NMS should keep 2 of 3 candidates");
        assert!(
            result[0].score > result[1].score,
            "Results should be sorted by score descending"
        );
    }

    /// Verify NMS handles empty input.
    #[test]
    fn test_nms_empty_candidates() {
        let result = nms(&[], 0.4);
        assert!(result.is_empty());
    }

    /// Verify IoU calculation for FaceCandidate.
    #[test]
    fn test_face_candidate_iou() {
        let a = FaceCandidate {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            score: 0.9,
            landmarks: None,
        };

        // Exact overlap
        let b = FaceCandidate {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            score: 0.8,
            landmarks: None,
        };
        assert!(
            (a.iou(&b) - 1.0).abs() < 1e-6,
            "Identical boxes should have IoU 1.0"
        );

        // No overlap
        let c = FaceCandidate {
            x: 200.0,
            y: 200.0,
            width: 100.0,
            height: 100.0,
            score: 0.7,
            landmarks: None,
        };
        assert!(
            a.iou(&c) < 1e-6,
            "Non-overlapping boxes should have IoU ~0"
        );

        // Partial overlap
        let d = FaceCandidate {
            x: 50.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
            score: 0.6,
            landmarks: None,
        };
        let iou = a.iou(&d);
        assert!(
            iou > 0.1 && iou < 0.9,
            "Partial overlap should have IoU between 0 and 1, got {}",
            iou
        );
    }

    /// Verify ScrfdDetector with custom input size.
    #[test]
    fn test_scrfd_detector_with_custom_input_size() {
        let detector = ScrfdDetector::with_input_size(320);
        assert!(!detector.is_loaded());
        assert_eq!(detector.input_size, 320);
    }
}
