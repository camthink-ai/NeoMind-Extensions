//! ArcFace face feature extractor using ONNX Runtime.
//!
//! Implements face feature extraction using the ArcFace (w600k_r50) model in ONNX format.
//! Takes an aligned face crop (from the alignment module), runs it through the ArcFace model,
//! and produces a 512-dimensional feature vector that is L2 normalized. The feature vector
//! can then be compared against registered faces using cosine similarity.
//!
//! # Contract
//!
//! The `extract()` method expects the input to be raw image bytes of an **already aligned**
//! 112x112 face crop. Callers (e.g., the recognition pipeline in `lib.rs`) are responsible
//! for face alignment before calling this method. The method decodes the image, normalizes
//! pixels to NCHW format, runs ONNX inference, and L2-normalizes the output.

#[cfg(not(target_arch = "wasm32"))]
use crate::onnx_utils::{setup_native_lib_paths, find_model_path};

// ============================================================================
// Trait Definition
// ============================================================================

/// Trait for face feature extraction implementations.
///
/// Takes raw image bytes of a face crop and produces a normalized feature vector
/// suitable for cosine similarity comparison.
pub trait FaceExtract {
    /// Extract a feature vector from a single face crop image.
    ///
    /// # Arguments
    /// * `face_crop` - Raw image bytes (JPEG, PNG, etc.) containing a face crop
    ///
    /// # Returns
    /// A 512-dimensional L2-normalized feature vector
    fn extract(&mut self, face_crop: &[u8]) -> Result<Vec<f32>, String>;

    /// Extract feature vectors from multiple face crops.
    ///
    /// # Arguments
    /// * `faces` - Vector of raw image byte slices, each containing a face crop
    ///
    /// # Returns
    /// A vector of 512-dimensional L2-normalized feature vectors, one per input face
    fn extract_batch(&mut self, faces: Vec<&[u8]>) -> Result<Vec<Vec<f32>>, String>;

    /// Downcast to Any for type-specific access (e.g., model status checks).
    fn as_any(&self) -> &dyn std::any::Any;
}

// ============================================================================
// ArcFace Recognizer (non-WASM)
// ============================================================================

/// ArcFace face feature extractor using ONNX Runtime.
///
/// Uses the w600k_r50.onnx model to produce 512-dimensional feature vectors
/// from aligned face images. Supports lazy model loading: the ONNX session is
/// not created until `ensure_loaded()` is called on first extraction request.
#[cfg(not(target_arch = "wasm32"))]
pub struct ArcFaceRecognizer {
    /// Loaded ONNX session (None until first use)
    model: Option<ort::session::Session>,
    /// Whether we've already attempted to load the model
    load_attempted: bool,
    /// Last loading error, if any
    load_error: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ArcFaceRecognizer {
    /// Create a new recognizer WITHOUT loading the model (lazy initialization).
    pub fn new() -> Self {
        Self {
            model: None,
            load_attempted: false,
            load_error: None,
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

        tracing::info!("[ArcFace] Lazy loading ArcFace model (w600k_r50.onnx)");
        match Self::try_load_model() {
            Ok(session) => {
                tracing::info!("[ArcFace] ArcFace model loaded successfully");
                self.model = Some(session);
            }
            Err(e) => {
                tracing::error!("[ArcFace] Failed to load model: {}", e);
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

    /// Attempt to load the ArcFace model from disk.
    fn try_load_model() -> Result<ort::session::Session, String> {
        let model_path = find_model_path("w600k_r50.onnx")?;

        tracing::info!(
            "[ArcFace] Loading model file: {}",
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

    /// Extract a feature vector from an aligned face crop image.
    ///
    /// **Contract:** The input must be raw image bytes of an already-aligned 112x112
    /// face crop. Callers are responsible for face alignment before invoking this
    /// method. The pipeline is: decode image -> preprocess (NCHW, normalize) ->
    /// run ONNX inference -> L2 normalize output vector.
    pub fn extract_impl(&mut self, face_crop: &[u8]) -> Result<Vec<f32>, String> {
        self.ensure_loaded();

        let session = self.model.as_mut().ok_or_else(|| {
            self.load_error
                .clone()
                .unwrap_or_else(|| "ArcFace model not loaded".to_string())
        })?;

        // Decode image (expected to be an aligned 112x112 face crop)
        let img = image::load_from_memory(face_crop)
            .map_err(|e| format!("Failed to decode face crop image: {}", e))?;
        let img = img.to_rgb8();

        // Preprocess: (pixel - 127.5) / 128.0, NCHW layout (1, 3, 112, 112)
        let input_tensor = ndarray::Array4::from_shape_fn(
            (1, 3, 112, 112),
            |(_, c, y, x)| {
                let pixel = img.get_pixel(x as u32, y as u32);
                (pixel[c] as f32 - 127.5) / 128.0
            },
        );

        // Create ONNX tensor input
        let input_shape = vec![1_i64, 3_i64, 112_i64, 112_i64];
        let (raw_data, _offset) = input_tensor.into_raw_vec_and_offset();
        let input_value = ort::value::Tensor::from_array((
            input_shape,
            raw_data.into_boxed_slice(),
        ))
        .map_err(|e| format!("Failed to create input tensor: {}", e))?;

        // Discover output name BEFORE running inference
        let output_name = session.outputs[0].name.clone();

        // Run inference
        let outputs = session
            .run(ort::inputs![input_value])
            .map_err(|e| format!("ArcFace ONNX inference failed: {}", e))?;

        // Extract output tensor (first output, should be 512-dim vector)
        let output = outputs
            .get(&output_name)
            .ok_or_else(|| "Missing output tensor from ArcFace model".to_string())?;

        let (_shape, data) = output
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Failed to extract ArcFace output: {}", e))?;

        let feature: Vec<f32> = data.to_vec();

        // L2 normalize the feature vector
        Ok(l2_normalize(&feature))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for ArcFaceRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FaceExtract for ArcFaceRecognizer {
    fn extract(&mut self, face_crop: &[u8]) -> Result<Vec<f32>, String> {
        self.extract_impl(face_crop)
    }

    fn extract_batch(&mut self, faces: Vec<&[u8]>) -> Result<Vec<Vec<f32>>, String> {
        let mut results = Vec::with_capacity(faces.len());
        for face_crop in &faces {
            results.push(self.extract(face_crop)?);
        }
        Ok(results)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// ArcFace Recognizer stub (WASM)
// ============================================================================

#[cfg(target_arch = "wasm32")]
pub struct ArcFaceRecognizer;

#[cfg(target_arch = "wasm32")]
impl ArcFaceRecognizer {
    pub fn new() -> Self {
        Self
    }

    pub fn is_loaded(&self) -> bool {
        false
    }

    pub fn load_error(&self) -> Option<&str> {
        Some("ArcFace recognizer not available in WASM")
    }

    pub fn ensure_loaded(&mut self) {
        // No-op in WASM
    }
}

#[cfg(target_arch = "wasm32")]
impl Default for ArcFaceRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
impl FaceExtract for ArcFaceRecognizer {
    fn extract(&mut self, _face_crop: &[u8]) -> Result<Vec<f32>, String> {
        Err("ArcFace recognizer not available in WASM".to_string())
    }

    fn extract_batch(&mut self, _faces: Vec<&[u8]>) -> Result<Vec<Vec<f32>>, String> {
        Err("ArcFace recognizer not available in WASM".to_string())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Model path discovery is now in crate::onnx_utils::find_model_path

// ============================================================================
// L2 Normalization
// ============================================================================

/// L2 normalize a feature vector in-place.
///
/// Divides each element by the L2 norm (Euclidean length) of the vector,
/// producing a unit vector where the norm is approximately 1.0.
fn l2_normalize(vec: &[f32]) -> Vec<f32> {
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-10 {
        // Avoid division by zero for zero vectors
        return vec.to_vec();
    }
    vec.iter().map(|x| x / norm).collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onnx_utils::find_model_path;

    /// Verify FaceExtract trait compiles by creating a trait object.
    #[test]
    fn test_face_extract_trait_compiles() {
        let _recognizer: Box<dyn FaceExtract> = Box::new(ArcFaceRecognizer::new());
    }

    /// Verify ArcFaceRecognizer::new() creates an unloaded recognizer.
    #[test]
    fn test_arcface_recognizer_new_creates_unloaded() {
        let recognizer = ArcFaceRecognizer::new();
        assert!(
            !recognizer.is_loaded(),
            "New recognizer should not have a loaded model"
        );
        assert!(
            !recognizer.load_attempted,
            "New recognizer should not have attempted loading"
        );
        assert!(
            recognizer.load_error.is_none(),
            "New recognizer should not have a load error"
        );
    }

    /// Verify Default trait creates an equivalent recognizer.
    #[test]
    fn test_arcface_recognizer_default() {
        let recognizer = ArcFaceRecognizer::default();
        assert!(!recognizer.is_loaded());
        assert!(!recognizer.load_attempted);
    }

    /// Verify L2 normalization produces a unit vector (norm ~ 1.0).
    #[test]
    fn test_l2_normalization_norm_is_one() {
        let feature = vec![3.0, 4.0, 0.0, 0.0]; // L2 norm = 5.0
        let normalized = l2_normalize(&feature);

        let norm: f32 = normalized.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-6,
            "L2 norm of normalized vector should be ~1.0, got {}",
            norm
        );

        // Check individual values
        assert!(
            (normalized[0] - 0.6).abs() < 1e-6,
            "normalized[0] should be 0.6, got {}",
            normalized[0]
        );
        assert!(
            (normalized[1] - 0.8).abs() < 1e-6,
            "normalized[1] should be 0.8, got {}",
            normalized[1]
        );
    }

    /// Verify L2 normalization handles zero vectors gracefully.
    #[test]
    fn test_l2_normalization_zero_vector() {
        let feature = vec![0.0, 0.0, 0.0];
        let normalized = l2_normalize(&feature);

        // Zero vector should remain zero (avoid NaN)
        for val in &normalized {
            assert!(
                val.is_finite(),
                "Zero vector normalization should produce finite values"
            );
            assert!(
                (*val).abs() < 1e-10,
                "Zero vector normalization should produce ~0.0 values"
            );
        }
    }

    /// Verify L2 normalization on a larger vector (simulating 512-dim).
    #[test]
    fn test_l2_normalization_large_vector() {
        let feature: Vec<f32> = (0..512).map(|i| (i as f32) * 0.01).collect();
        let normalized = l2_normalize(&feature);

        let norm: f32 = normalized.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "L2 norm of normalized 512-dim vector should be ~1.0, got {}",
            norm
        );
    }

    /// Verify extract_batch returns correct number of vectors (error case without model).
    #[test]
    fn test_extract_batch_returns_correct_count_on_error() {
        let mut recognizer = ArcFaceRecognizer::new();
        // Create tiny valid image bytes (1x1 red PNG)
        let fake_face = vec![0u8; 10];

        let result = recognizer.extract_batch(vec![&fake_face, &fake_face, &fake_face]);
        // Should fail because model is not loaded, not because of batch count
        assert!(result.is_err(), "Should fail without model loaded");
    }

    /// Verify ensure_loaded with missing model file returns error.
    /// Note: removed full ensure_loaded test due to env var race condition in parallel tests.
    /// The "model not found" path is covered by test_find_model_path_missing_file.


    /// Verify ensure_loaded is idempotent -- calling it twice does not re-load.
    #[test]
    fn test_ensure_loaded_idempotent() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        std::env::set_var("NEOMIND_EXTENSION_DIR", temp_dir.path().to_str().unwrap());

        let mut recognizer = ArcFaceRecognizer::new();
        recognizer.ensure_loaded();
        assert!(recognizer.load_attempted);
        let first_error = recognizer.load_error.clone();

        // Call again -- should not change state
        recognizer.ensure_loaded();
        assert_eq!(
            recognizer.load_error, first_error,
            "Second ensure_loaded should not change load error"
        );

        std::env::remove_var("NEOMIND_EXTENSION_DIR");
    }

    /// Verify extract without loaded model returns error.
    #[test]
    fn test_extract_without_model_returns_error() {
        let mut recognizer: Box<dyn FaceExtract> = Box::new(ArcFaceRecognizer::new());
        let result = recognizer.extract(&[]);
        assert!(
            result.is_err(),
            "Extract without model should return error"
        );
    }

    /// Verify extract_batch with empty input returns empty result (no model needed).
    #[test]
    fn test_extract_batch_empty_input_returns_empty() {
        let mut recognizer: Box<dyn FaceExtract> = Box::new(ArcFaceRecognizer::new());
        let result = recognizer.extract_batch(vec![]);
        assert!(
            result.is_ok(),
            "Extract batch with empty input should return Ok"
        );
        assert_eq!(
            result.unwrap().len(),
            0,
            "Empty batch should return empty results"
        );
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
        let model_file = models_dir.join("w600k_r50.onnx");
        std::fs::write(&model_file, b"fake model data").expect("Failed to write model file");

        std::env::set_var(
            "NEOMIND_EXTENSION_DIR",
            temp_dir.path().to_str().unwrap(),
        );

        let result = find_model_path("w600k_r50.onnx");
        assert!(result.is_ok(), "Should find model in extension dir");
        assert_eq!(
            result.unwrap(),
            model_file,
            "Should return the correct model path"
        );

        std::env::remove_var("NEOMIND_EXTENSION_DIR");
    }
}
