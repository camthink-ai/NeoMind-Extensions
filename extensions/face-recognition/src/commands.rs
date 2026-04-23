//! Command dispatch and individual command handlers for the face-recognition extension.

use base64::Engine;
use neomind_extension_sdk::{ExtensionError, Result};
use serde_json::json;

use crate::types::{BindingStats, DeviceBinding};
use crate::FaceRecognition;

impl FaceRecognition {
    /// Execute a command on the extension (non-WASM).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn execute_command_impl(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            // 1. bind_device
            "bind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid device_id".to_string(),
                        )
                    })?;

                if device_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "device_id cannot be empty",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                // Check if already bound
                if self.bindings.read().contains_key(device_id) {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Device {} is already bound", device_id),
                        "error_code": "DEVICE_ALREADY_BOUND"
                    }));
                }

                let metric_name = args
                    .get("metric_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image")
                    .to_string();

                let binding = DeviceBinding {
                    device_id: device_id.to_string(),
                    metric_name,
                    active: true,
                    created_at: chrono::Utc::now().timestamp(),
                };

                self.bindings
                    .write()
                    .insert(device_id.to_string(), binding.clone());
                self.binding_stats.write().insert(
                    device_id.to_string(),
                    BindingStats {
                        total_inferences: 0,
                        total_recognized: 0,
                        total_unknown: 0,
                        last_image: None,
                        last_faces: None,
                        last_error: None,
                    },
                );

                self.persist_config();

                tracing::info!(
                    "[FaceRecognition] Device bound: {} (metric: {})",
                    device_id,
                    binding.metric_name
                );

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "message": format!("Device {} bound successfully", device_id)
                }))
            }

            // 2. unbind_device
            "unbind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid device_id".to_string(),
                        )
                    })?;

                if device_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "device_id cannot be empty",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                self.bindings.write().remove(device_id);
                self.binding_stats.write().remove(device_id);

                self.persist_config();

                tracing::info!("[FaceRecognition] Device unbound: {}", device_id);

                Ok(json!({
                    "success": true,
                    "device_id": device_id,
                    "message": format!("Device {} unbound successfully", device_id)
                }))
            }

            // 3. toggle_binding
            "toggle_binding" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid device_id".to_string(),
                        )
                    })?;

                let active = args
                    .get("active")
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments(
                            "Missing or invalid active parameter".to_string(),
                        )
                    })?;

                if device_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "device_id cannot be empty",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                let mut bindings = self.bindings.write();
                if let Some(binding) = bindings.get_mut(device_id) {
                    binding.active = active;
                    drop(bindings); // Release write lock before persist

                    self.persist_config();

                    tracing::info!(
                        "[FaceRecognition] Device {} binding toggled to active={}",
                        device_id,
                        active
                    );

                    Ok(json!({
                        "success": true,
                        "device_id": device_id,
                        "active": active
                    }))
                } else {
                    Ok(json!({
                        "success": false,
                        "error": format!("Device {} not bound", device_id),
                        "error_code": "DEVICE_NOT_FOUND"
                    }))
                }
            }

            // 4. get_bindings
            "get_bindings" => {
                let bindings: Vec<serde_json::Value> = self
                    .bindings
                    .read()
                    .iter()
                    .map(|(id, b)| {
                        let stats = self
                            .binding_stats
                            .read()
                            .get(id)
                            .cloned()
                            .unwrap_or(BindingStats {
                                total_inferences: 0,
                                total_recognized: 0,
                                total_unknown: 0,
                                last_image: None,
                                last_faces: None,
                                last_error: None,
                            });
                        json!({
                            "device_id": b.device_id,
                            "metric_name": b.metric_name,
                            "active": b.active,
                            "created_at": b.created_at,
                            "stats": {
                                "total_inferences": stats.total_inferences,
                                "total_recognized": stats.total_recognized,
                                "total_unknown": stats.total_unknown,
                            },
                            "last_image": stats.last_image,
                            "last_faces": stats.last_faces,
                            "last_error": stats.last_error,
                        })
                    })
                    .collect();

                Ok(json!({
                    "success": true,
                    "bindings": bindings
                }))
            }

            // 5. register_face
            "register_face" => {
                let raw_name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Validate name: trim whitespace, check length, reject control characters
                let name = raw_name.trim();
                if name.is_empty() || name.len() > 100 {
                    return Ok(json!({
                        "success": false,
                        "error": "name is required, cannot be empty, and must not exceed 100 characters",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }
                if name.chars().any(|c| c.is_control()) {
                    return Ok(json!({
                        "success": false,
                        "error": "name contains invalid control characters",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                let image_b64 = args
                    .get("image")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if image_b64.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "image is required",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                // Handle data URI prefix
                let image_b64_clean = if image_b64.starts_with("data:") {
                    if let Some(comma_pos) = image_b64.find(',') {
                        &image_b64[comma_pos + 1..]
                    } else {
                        image_b64
                    }
                } else {
                    image_b64
                };

                // Decode base64 image
                let image_data: Vec<u8> =
                    match base64::engine::general_purpose::STANDARD.decode(image_b64_clean) {
                        Ok(data) => data,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Invalid base64 image: {}", e),
                                "error_code": "INVALID_ARGUMENTS"
                            }));
                        }
                    };

                // Check size limit (10MB)
                if image_data.len() > 10 * 1024 * 1024 {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Image too large: {} bytes (max 10MB)", image_data.len()),
                        "error_code": "IMAGE_TOO_LARGE"
                    }));
                }

                // Run detection to find faces
                let config = self.config.read();
                let confidence = config.confidence_threshold as f32;
                let max_faces = config.max_faces;

                let faces = {
                    let mut detector = self.detector.lock();
                    match detector.detect(&image_data, confidence) {
                        Ok(f) => f,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Face detection failed: {}", e),
                                "error_code": "MODEL_NOT_LOADED"
                            }));
                        }
                    }
                };

                // Validate face count
                if faces.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "No face detected in the image",
                        "error_code": "NO_FACE_DETECTED"
                    }));
                }

                if faces.len() > 1 {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Multiple faces ({}) detected, please provide an image with exactly one face", faces.len()),
                        "error_code": "MULTIPLE_FACES"
                    }));
                }

                let face_box = &faces[0];

                // Align the face
                let img = match image::load_from_memory(&image_data) {
                    Ok(i) => i,
                    Err(e) => {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to load image: {}", e),
                            "error_code": "INVALID_ARGUMENTS"
                        }));
                    }
                };

                let aligned = crate::alignment::align_face(&img, face_box);

                // Generate thumbnail: encode aligned 112x112 face as JPEG -> base64 -> data URI
                let thumbnail = {
                    let mut jpeg_bytes = Vec::new();
                    let aligned_rgb = aligned.to_rgb8();
                    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                        &mut jpeg_bytes,
                        90,
                    );
                    if let Err(e) = encoder.encode(
                        aligned_rgb.as_raw(),
                        aligned_rgb.width(),
                        aligned_rgb.height(),
                        image::ColorType::Rgb8.into(),
                    ) {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Failed to encode thumbnail: {}", e),
                            "error_code": "INVALID_ARGUMENTS"
                        }));
                    }
                    format!(
                        "data:image/jpeg;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes)
                    )
                };

                // Encode aligned face to JPEG for feature extraction
                let mut aligned_bytes = Vec::new();
                let aligned_rgb = aligned.to_rgb8();
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut aligned_bytes,
                    95,
                );
                if let Err(e) = encoder.encode(
                    aligned_rgb.as_raw(),
                    aligned_rgb.width(),
                    aligned_rgb.height(),
                    image::ColorType::Rgb8.into(),
                ) {
                    return Ok(json!({
                        "success": false,
                        "error": format!("Failed to encode aligned face: {}", e),
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                // Extract features
                let feature = {
                    let mut recognizer = self.recognizer.lock();
                    match recognizer.extract(&aligned_bytes) {
                        Ok(f) => f,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Feature extraction failed: {}", e),
                                "error_code": "MODEL_NOT_LOADED"
                            }));
                        }
                    }
                };

                // Acquire a single write lock and perform all checks + registration atomically
                // to prevent TOCTOU race conditions (e.g., concurrent duplicate name registration).
                let entry = {
                    let mut db = self.face_db.write();

                    // Check for duplicate name (atomic with registration)
                    let existing = db.list_faces();
                    if existing.iter().any(|e| e.name == name) {
                        return Ok(json!({
                            "success": false,
                            "error": format!("A face with name '{}' is already registered", name),
                            "error_code": "DUPLICATE_NAME"
                        }));
                    }

                    // Check max faces limit (atomic with registration)
                    if db.len() >= max_faces {
                        return Ok(json!({
                            "success": false,
                            "error": format!("Maximum face limit of {} reached", max_faces),
                            "error_code": "MAX_FACES_EXCEEDED"
                        }));
                    }

                    // Register in database under the same lock
                    match db.register(name, feature, &thumbnail) {
                        Ok(entry) => entry,
                        Err(e) => {
                            return Ok(json!({
                                "success": false,
                                "error": format!("Failed to register face: {}", e),
                                "error_code": "INVALID_ARGUMENTS"
                            }));
                        }
                    }
                };

                // Save database to file
                self.save_face_database();

                tracing::info!(
                    "[FaceRecognition] Face registered: {} (id: {})",
                    entry.name,
                    entry.id
                );

                Ok(json!({
                    "success": true,
                    "face_id": entry.id,
                    "name": entry.name,
                    "registered_at": entry.registered_at,
                    "message": format!("Face '{}' registered successfully", name)
                }))
            }

            // 6. delete_face
            "delete_face" => {
                let face_id = args
                    .get("face_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if face_id.is_empty() {
                    return Ok(json!({
                        "success": false,
                        "error": "face_id is required",
                        "error_code": "INVALID_ARGUMENTS"
                    }));
                }

                let mut db = self.face_db.write();
                match db.delete(face_id) {
                    Ok(()) => {
                        drop(db); // Release lock before saving
                        self.save_face_database();

                        tracing::info!(
                            "[FaceRecognition] Face deleted: {}",
                            face_id
                        );

                        Ok(json!({
                            "success": true,
                            "face_id": face_id,
                            "message": "Face deleted successfully"
                        }))
                    }
                    Err(e) => Ok(json!({
                        "success": false,
                        "error": format!("Face not found: {}", e),
                        "error_code": "FACE_NOT_FOUND"
                    })),
                }
            }

            // 7. list_faces
            "list_faces" => {
                let db = self.face_db.read();
                let faces = db.list_faces();
                Ok(json!({
                    "success": true,
                    "faces": faces,
                    "count": faces.len()
                }))
            }

            // 8. get_status
            "get_status" => Ok(self.get_status()),

            // 9. configure
            "configure" => {
                // Accept config either as a "config" parameter or directly in args
                let config_value = args.get("config").unwrap_or(args);

                if let Some(confidence) =
                    config_value.get("confidence_threshold").and_then(|v| v.as_f64())
                {
                    self.config.write().confidence_threshold = confidence;
                }
                if let Some(threshold) = config_value
                    .get("recognition_threshold")
                    .and_then(|v| v.as_f64())
                {
                    self.config.write().recognition_threshold = threshold;
                    // Sync threshold with FaceDatabase
                    self.face_db.write().set_threshold(threshold);
                }
                if let Some(max_faces) =
                    config_value.get("max_faces").and_then(|v| v.as_u64())
                {
                    let max_faces_usize = max_faces as usize;
                    self.config.write().max_faces = max_faces_usize;
                    // Sync max_faces with FaceDatabase
                    self.face_db.write().set_max_faces(max_faces_usize);
                }
                if let Some(auto_detect) =
                    config_value.get("auto_detect").and_then(|v| v.as_bool())
                {
                    self.config.write().auto_detect = auto_detect;
                }

                self.persist_config();

                tracing::info!("[FaceRecognition] Configuration updated");

                Ok(json!({
                    "success": true,
                    "message": "Configuration updated",
                    "config": *self.config.read()
                }))
            }

            // 10. get_config
            "get_config" => {
                let config = self.config.read();
                Ok(json!({
                    "success": true,
                    "config": *config
                }))
            }

            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    /// Execute a command synchronously (WASM target).
    #[cfg(target_arch = "wasm32")]
    pub fn execute_command_sync_impl(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match command {
            "get_status" => Ok(self.get_status()),
            "get_config" => {
                let config = self.config.read();
                Ok(json!({
                    "success": true,
                    "config": *config
                }))
            }
            "list_faces" => {
                let db = self.face_db.read();
                let faces = db.list_faces();
                Ok(json!({
                    "success": true,
                    "faces": faces,
                    "count": faces.len()
                }))
            }
            "get_bindings" => {
                let bindings: Vec<serde_json::Value> = self
                    .bindings
                    .read()
                    .iter()
                    .map(|(_, b)| json!(b))
                    .collect();
                Ok(json!({
                    "success": true,
                    "bindings": bindings
                }))
            }
            "bind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("Missing device_id".to_string())
                    })?;
                let metric_name = args
                    .get("metric_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("image")
                    .to_string();

                let binding = DeviceBinding {
                    device_id: device_id.to_string(),
                    metric_name,
                    active: true,
                    created_at: chrono::Utc::now().timestamp(),
                };
                self.bindings
                    .write()
                    .insert(device_id.to_string(), binding);
                self.binding_stats.write().insert(
                    device_id.to_string(),
                    BindingStats {
                        total_inferences: 0,
                        total_recognized: 0,
                        total_unknown: 0,
                        last_image: None,
                        last_faces: None,
                        last_error: None,
                    },
                );
                Ok(json!({"success": true, "device_id": device_id}))
            }
            "unbind_device" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("Missing device_id".to_string())
                    })?;
                self.bindings.write().remove(device_id);
                self.binding_stats.write().remove(device_id);
                Ok(json!({"success": true, "device_id": device_id}))
            }
            "toggle_binding" => {
                let device_id = args
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ExtensionError::InvalidArguments("Missing device_id".to_string())
                    })?;
                let active = args.get("active").and_then(|v| v.as_bool());
                if let Some(binding) = self.bindings.write().get_mut(device_id) {
                    binding.active = active.unwrap_or(!binding.active);
                    Ok(json!({"success": true, "device_id": device_id, "active": binding.active}))
                } else {
                    Err(ExtensionError::NotFound(format!(
                        "Device {} not bound",
                        device_id
                    )))
                }
            }
            "configure" => Ok(json!({"success": true, "message": "Configuration applied (WASM)"})),
            "register_face" => Err(ExtensionError::NotSupported(
                "Face registration not supported in WASM".to_string(),
            )),
            "delete_face" => Err(ExtensionError::NotSupported(
                "Face deletion not supported in WASM".to_string(),
            )),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }
}
