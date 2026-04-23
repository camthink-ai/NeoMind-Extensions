//! Face recognition pipeline and image extraction helpers.

use neomind_extension_sdk::{ExtensionError, Result};
use serde_json::json;

use crate::types::FaceResult;
use crate::FaceRecognition;

impl FaceRecognition {
    /// Write recognition results as virtual metrics on the device.
    pub fn write_recognition_results(
        &self,
        device_id: &str,
        face_count: usize,
        face_names: &[String],
        annotated_image_b64: &str,
        avg_confidence: f64,
        timestamp: i64,
    ) {
        // Write face count
        let params = json!({
            "device_id": device_id,
            "metric": "virtual.face_recognition.face_count",
            "value": face_count,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write face_count metric");
        }

        // Write face names
        let names_str = face_names.join(",");
        let params = json!({
            "device_id": device_id,
            "metric": "virtual.face_recognition.face_names",
            "value": names_str,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write face_names metric");
        }

        // Write annotated image
        let data_uri = format!("data:image/jpeg;base64,{}", annotated_image_b64);
        let params = json!({
            "device_id": device_id,
            "metric": "virtual.face_recognition.annotated_image",
            "value": data_uri,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write annotated_image metric");
        }

        // Write confidence
        let params = json!({
            "device_id": device_id,
            "metric": "virtual.face_recognition.confidence",
            "value": avg_confidence,
            "timestamp": timestamp,
        });
        let resp = self.invoke_capability_sync("device_metrics_write", &params);
        if !resp.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            tracing::warn!("[FaceRecognition] Failed to write confidence metric");
        }
    }

    /// Run the full face recognition pipeline on image data.
    /// Returns a tuple of (face_results, annotated_image_base64).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_recognition_pipeline(
        &self,
        image_data: &[u8],
    ) -> Result<(Vec<FaceResult>, String)> {
        let config = self.config.read();
        let confidence = config.confidence_threshold as f32;

        // Step 1: Detect faces
        let faces = {
            let mut detector = self.detector.lock();
            detector
                .detect(image_data, confidence)
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Detection failed: {}", e)))?
        };

        if faces.is_empty() {
            return Ok((Vec::new(), String::new()));
        }

        // Step 2: For each face, align, extract features, and match
        let img = image::load_from_memory(image_data)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to load image: {}", e)))?;

        let mut results = Vec::with_capacity(faces.len());
        let db = self.face_db.read();

        for face_box in &faces {
            // Align the face
            let aligned = crate::alignment::align_face(&img, face_box);

            // Encode aligned face to JPEG bytes for the recognizer
            let mut aligned_bytes = Vec::new();
            let aligned_rgb = aligned.to_rgb8();
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut aligned_bytes,
                95,
            );
            encoder
                .encode(
                    aligned_rgb.as_raw(),
                    aligned_rgb.width(),
                    aligned_rgb.height(),
                    image::ColorType::Rgb8.into(),
                )
                .map_err(|e| {
                    ExtensionError::ExecutionFailed(format!(
                        "Failed to encode aligned face: {}",
                        e
                    ))
                })?;

            // Extract features
            let feature = {
                let mut recognizer = self.recognizer.lock();
                recognizer.extract(&aligned_bytes).map_err(|e| {
                    ExtensionError::ExecutionFailed(format!("Feature extraction failed: {}", e))
                })?
            };

            // Match against database
            let match_result = db.match_face(&feature);

            let face_result = match match_result {
                Some(m) => FaceResult {
                    face_box: face_box.clone(),
                    name: Some(m.name.clone()),
                    similarity: Some(m.similarity),
                    face_id: Some(m.face_id),
                },
                None => FaceResult {
                    face_box: face_box.clone(),
                    name: None,
                    similarity: None,
                    face_id: None,
                },
            };
            results.push(face_result);
        }

        // Step 3: Draw recognition results on the image
        let annotated_b64 = if !results.is_empty() {
            crate::drawing::draw_recognition_results(image_data, &results)
                .map_err(|e| {
                    ExtensionError::ExecutionFailed(format!("Drawing failed: {}", e))
                })?
        } else {
            String::new()
        };

        Ok((results, annotated_b64))
    }

    /// Extract image data from a value, supporting nested paths and data URIs.
    pub fn extract_image_from_value<'a>(
        &self,
        value: Option<&'a serde_json::Value>,
        nested_path: Option<&str>,
    ) -> Option<String> {
        let v = value?;

        // Navigate through nested path if provided
        let target_value = if let Some(path) = nested_path {
            let mut current = v;
            for part in path.split('.') {
                current = current.get(part)?;
            }
            current
        } else {
            v
        };

        // Try to extract string from the target value
        let raw_string: Option<&str> = {
            // Format 1: Direct string
            if let Some(s) = target_value.as_str() {
                Some(s)
            // Format 2: MetricValue wrapper {"String": "..."}
            } else if let Some(s) = target_value.get("String").and_then(|s| s.as_str()) {
                Some(s)
            // Format 3: Object with string fields (try common field names)
            } else {
                for field in &["image", "data", "value", "base64"] {
                    if let Some(s) = target_value.get(field).and_then(|s| s.as_str()) {
                        return Some(s.to_string());
                    }
                }
                None
            }
        };

        if let Some(s) = raw_string {
            // Handle data URL format: "data:image/jpeg;base64,actual_base64_data"
            if s.starts_with("data:") {
                if let Some(comma_pos) = s.find(',') {
                    let base64_part = &s[comma_pos + 1..];
                    return Some(base64_part.to_string());
                }
            }
            return Some(s.to_string());
        }

        None
    }
}
