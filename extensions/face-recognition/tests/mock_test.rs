//! Mock-based integration tests for face-recognition extension.
//!
//! These tests use mock detector and recognizer implementations so they do NOT
//! require ONNX Runtime or real model files. They exercise the full command
//! pipeline through `execute_command`, covering all error codes and edge cases.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use base64::Engine;
use neomind_extension_sdk::Extension;
use parking_lot::RwLock;
use serde_json::json;

use neomind_extension_face_recognition::database::FaceDatabase;
use neomind_extension_face_recognition::detector::FaceDetect;
use neomind_extension_face_recognition::recognizer::FaceExtract;
use neomind_extension_face_recognition::{
    FaceBox, FaceRecognition, Landmark,
};

// ============================================================================
// Mock Implementations
// ============================================================================

/// Mock face detector that returns a pre-configured set of face boxes.
struct MockDetector {
    /// Faces to return on detect() calls.
    faces: Vec<FaceBox>,
    /// If Some, detect() returns this error instead.
    error: Option<String>,
}

impl MockDetector {
    fn new(faces: Vec<FaceBox>) -> Self {
        Self { faces, error: None }
    }

    #[allow(dead_code)]
    fn with_error(msg: &str) -> Self {
        Self {
            faces: vec![],
            error: Some(msg.to_string()),
        }
    }
}

impl FaceDetect for MockDetector {
    fn detect(&mut self, _image_data: &[u8], _confidence: f32) -> Result<Vec<FaceBox>, String> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        Ok(self.faces.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Mock face recognizer that returns a pre-configured feature vector.
struct MockRecognizer {
    /// Feature vector to return on extract() calls.
    feature: Vec<f32>,
    /// If Some, extract() returns this error instead.
    error: Option<String>,
}

impl MockRecognizer {
    fn new(feature: Vec<f32>) -> Self {
        Self {
            feature,
            error: None,
        }
    }

    #[allow(dead_code)]
    fn with_error(msg: &str) -> Self {
        Self {
            feature: vec![],
            error: Some(msg.to_string()),
        }
    }
}

impl FaceExtract for MockRecognizer {
    fn extract(&mut self, _face_crop: &[u8]) -> Result<Vec<f32>, String> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        Ok(self.feature.clone())
    }

    fn extract_batch(&mut self, faces: Vec<&[u8]>) -> Result<Vec<Vec<f32>>, String> {
        if let Some(ref err) = self.error {
            return Err(err.clone());
        }
        Ok(faces.iter().map(|_| self.feature.clone()).collect())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Create a normalized feature vector of the given dimension.
fn make_feature(dim: usize, seed: f32) -> Vec<f32> {
    let mut v: Vec<f32> = (0..dim).map(|i| seed + i as f32 * 0.01).collect();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

/// Create a single FaceBox suitable for mock detection.
fn make_face_box() -> FaceBox {
    FaceBox {
        x: 50.0,
        y: 50.0,
        width: 100.0,
        height: 120.0,
        confidence: 0.95,
        landmarks: Some(vec![
            Landmark { x: 70.0, y: 80.0 },
            Landmark { x: 120.0, y: 78.0 },
            Landmark { x: 95.0, y: 105.0 },
            Landmark { x: 75.0, y: 130.0 },
            Landmark { x: 115.0, y: 128.0 },
        ]),
    }
}

/// Create a minimal valid JPEG image as base64 string.
/// Uses a 2x2 red pixel JPEG which is valid enough for image::load_from_memory.
fn make_minimal_image_b64() -> String {
    // Create a tiny 4x4 image and encode it as JPEG, then base64.
    let img = image::RgbImage::from_pixel(4, 4, image::Rgb([200, 150, 100]));
    let mut jpeg_bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 90);
    encoder
        .encode(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ColorType::Rgb8.into(),
        )
        .expect("Failed to encode test image");
    base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes)
}

/// Create a FaceRecognition extension with mock detector and recognizer.
fn create_ext(detector: MockDetector, recognizer: MockRecognizer) -> FaceRecognition {
    FaceRecognition::with_models(
        Box::new(detector),
        Box::new(recognizer),
    )
}

/// Create extension with default mocks (1 face, 512-dim feature).
fn create_default_ext() -> FaceRecognition {
    create_ext(
        MockDetector::new(vec![make_face_box()]),
        MockRecognizer::new(make_feature(512, 1.0)),
    )
}

/// Run an execute_command call and return the result as serde_json::Value.
/// This is a synchronous wrapper around the async execute_command.
fn run_command(
    ext: &FaceRecognition,
    command: &str,
    args: &serde_json::Value,
) -> serde_json::Value {
    tokio::task::block_in_place(|| {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(ext.execute_command(command, args))
            .expect("execute_command returned an ExtensionError")
    })
}

// ============================================================================
// 1. register_face Tests
// ============================================================================

#[test]
fn test_register_face_success() {
    let ext = create_default_ext();
    let image_b64 = make_minimal_image_b64();

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "Alice",
            "image": image_b64,
        }),
    );

    assert_eq!(result["success"], true, "register_face should succeed");
    assert!(
        result["face_id"].as_str().unwrap().len() > 0,
        "face_id should be non-empty"
    );
    assert_eq!(result["name"], "Alice");
}

#[test]
fn test_register_face_no_face_detected() {
    let ext = create_ext(
        MockDetector::new(vec![]), // No faces
        MockRecognizer::new(make_feature(512, 1.0)),
    );
    let image_b64 = make_minimal_image_b64();

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "Bob",
            "image": image_b64,
        }),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "NO_FACE_DETECTED");
}

#[test]
fn test_register_face_multiple_faces() {
    let ext = create_ext(
        MockDetector::new(vec![make_face_box(), make_face_box()]), // 2 faces
        MockRecognizer::new(make_feature(512, 1.0)),
    );
    let image_b64 = make_minimal_image_b64();

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "Charlie",
            "image": image_b64,
        }),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "MULTIPLE_FACES");
}

#[test]
fn test_register_face_duplicate_name() {
    let ext = create_default_ext();
    let image_b64 = make_minimal_image_b64();

    // Register first face
    let result1 = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "Dave",
            "image": &image_b64,
        }),
    );
    assert_eq!(result1["success"], true);

    // Try to register again with same name
    let result2 = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "Dave",
            "image": image_b64,
        }),
    );
    assert_eq!(result2["success"], false);
    assert_eq!(result2["error_code"], "DUPLICATE_NAME");
}

#[test]
fn test_register_face_max_faces_exceeded() {
    let ext = create_ext(
        MockDetector::new(vec![make_face_box()]),
        MockRecognizer::new(make_feature(512, 1.0)),
    );

    // Set max_faces to 2
    let _ = run_command(
        &ext,
        "configure",
        &json!({
            "config": {
                "max_faces": 2,
            },
        }),
    );

    let image_b64 = make_minimal_image_b64();

    // Register first two faces (should succeed)
    let r1 = run_command(&ext, "register_face", &json!({"name": "F1", "image": &image_b64}));
    assert_eq!(r1["success"], true);

    let r2 = run_command(&ext, "register_face", &json!({"name": "F2", "image": &image_b64}));
    assert_eq!(r2["success"], true);

    // Third face should exceed limit
    let r3 = run_command(&ext, "register_face", &json!({"name": "F3", "image": image_b64}));
    assert_eq!(r3["success"], false);
    assert_eq!(r3["error_code"], "MAX_FACES_EXCEEDED");
}

#[test]
fn test_register_face_image_too_large() {
    let ext = create_default_ext();

    // Create base64 data that decodes to > 10MB
    // We need decoded size > 10*1024*1024 = 10485760 bytes
    // Use a large base64 string (base64 expands 3:4, so we need ~14MB of base64)
    let large_data = vec![0u8; 10 * 1024 * 1024 + 1]; // Just over 10MB
    let large_b64 = base64::engine::general_purpose::STANDARD.encode(&large_data);

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "BigFace",
            "image": large_b64,
        }),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "IMAGE_TOO_LARGE");
}

#[test]
fn test_register_face_invalid_arguments_missing_name() {
    let ext = create_default_ext();
    let image_b64 = make_minimal_image_b64();

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "image": image_b64,
            // name is missing
        }),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "INVALID_ARGUMENTS");
}

#[test]
fn test_register_face_invalid_arguments_missing_image() {
    let ext = create_default_ext();

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "Eve",
            // image is missing
        }),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "INVALID_ARGUMENTS");
}

#[test]
fn test_register_face_invalid_arguments_empty_name() {
    let ext = create_default_ext();
    let image_b64 = make_minimal_image_b64();

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "",
            "image": image_b64,
        }),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "INVALID_ARGUMENTS");
}

// ============================================================================
// 2. delete_face Tests
// ============================================================================

#[test]
fn test_delete_face_success() {
    let ext = create_default_ext();
    let image_b64 = make_minimal_image_b64();

    // Register a face first
    let reg = run_command(
        &ext,
        "register_face",
        &json!({"name": "Frank", "image": &image_b64}),
    );
    assert_eq!(reg["success"], true);
    let face_id = reg["face_id"].as_str().unwrap().to_string();

    // Delete it
    let del = run_command(
        &ext,
        "delete_face",
        &json!({"face_id": face_id}),
    );
    assert_eq!(del["success"], true);
    assert_eq!(del["face_id"], reg["face_id"]);

    // Verify it is gone
    let list = run_command(&ext, "list_faces", &json!({}));
    assert_eq!(list["count"], 0);
}

#[test]
fn test_delete_face_not_found() {
    let ext = create_default_ext();

    let result = run_command(
        &ext,
        "delete_face",
        &json!({"face_id": "nonexistent-id-12345"}),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "FACE_NOT_FOUND");
}

#[test]
fn test_delete_face_empty_id() {
    let ext = create_default_ext();

    let result = run_command(
        &ext,
        "delete_face",
        &json!({"face_id": ""}),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "INVALID_ARGUMENTS");
}

// ============================================================================
// 3. list_faces Tests
// ============================================================================

#[test]
fn test_list_faces_returns_summaries_without_features() {
    let ext = create_ext(
        MockDetector::new(vec![make_face_box()]),
        MockRecognizer::new(make_feature(512, 1.0)),
    );
    let image_b64 = make_minimal_image_b64();

    // Register 3 faces
    for name in &["G1", "G2", "G3"] {
        let r = run_command(&ext, "register_face", &json!({"name": *name, "image": &image_b64}));
        assert_eq!(r["success"], true, "Failed to register {}", name);
    }

    let result = run_command(&ext, "list_faces", &json!({}));

    assert_eq!(result["success"], true);
    assert_eq!(result["count"], 3);

    let faces = result["faces"].as_array().expect("faces should be array");
    assert_eq!(faces.len(), 3);

    // Verify no feature vectors in the output (feature vectors would be large arrays)
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(
        !serialized.contains("\"feature\""),
        "list_faces should not include feature vectors"
    );

    // Verify names are present
    let names: Vec<&str> = faces
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"G1"));
    assert!(names.contains(&"G2"));
    assert!(names.contains(&"G3"));
}

#[test]
fn test_list_faces_empty_database() {
    let ext = create_default_ext();

    let result = run_command(&ext, "list_faces", &json!({}));

    assert_eq!(result["success"], true);
    assert_eq!(result["count"], 0);
    assert_eq!(result["faces"].as_array().unwrap().len(), 0);
}

// ============================================================================
// 4. Recognition Tests
// ============================================================================

#[test]
fn test_empty_db_recognize_all_unknown() {
    // With an empty database, match_face returns None for all faces.
    // We test this at the database level since the recognition pipeline
    // requires real image decoding.
    let db = FaceDatabase::new(0.45, 10);
    let feature = make_feature(512, 1.0);

    let result = db.match_face(&feature);
    assert!(result.is_none(), "Empty database should return no matches");
}

#[test]
fn test_recognize_with_known_face() {
    // Register a face directly in the database, then match it.
    let mut db = FaceDatabase::new(0.45, 10);
    let feature = make_feature(512, 42.0);
    let entry = db.register("KnownPerson", feature.clone(), "thumb").unwrap();

    // Match with the same feature vector
    let result = db.match_face(&feature).expect("Should find a match");
    assert_eq!(result.name, "KnownPerson");
    assert_eq!(result.face_id, entry.id);
    assert!(
        (result.similarity - 1.0).abs() < 1e-6,
        "Same feature should have similarity ~1.0, got {}",
        result.similarity
    );
}

// ============================================================================
// 5. Device Binding Tests
// ============================================================================

#[test]
fn test_bind_and_unbind_device() {
    let ext = create_default_ext();

    // Bind a device
    let bind = run_command(
        &ext,
        "bind_device",
        &json!({"device_id": "cam-01", "metric_name": "image"}),
    );
    assert_eq!(bind["success"], true);
    assert_eq!(bind["device_id"], "cam-01");

    // Verify binding exists
    let bindings = run_command(&ext, "get_bindings", &json!({}));
    assert_eq!(bindings["success"], true);
    let bindings_arr = bindings["bindings"].as_array().unwrap();
    assert_eq!(bindings_arr.len(), 1);
    assert_eq!(bindings_arr[0]["device_id"], "cam-01");
    assert_eq!(bindings_arr[0]["active"], true);

    // Unbind
    let unbind = run_command(
        &ext,
        "unbind_device",
        &json!({"device_id": "cam-01"}),
    );
    assert_eq!(unbind["success"], true);

    // Verify removed
    let bindings2 = run_command(&ext, "get_bindings", &json!({}));
    assert_eq!(bindings2["bindings"].as_array().unwrap().len(), 0);
}

#[test]
fn test_bind_device_already_bound() {
    let ext = create_default_ext();

    // First bind should succeed
    let bind1 = run_command(
        &ext,
        "bind_device",
        &json!({"device_id": "cam-02", "metric_name": "image"}),
    );
    assert_eq!(bind1["success"], true);

    // Second bind of same device should fail
    let bind2 = run_command(
        &ext,
        "bind_device",
        &json!({"device_id": "cam-02", "metric_name": "image"}),
    );
    assert_eq!(bind2["success"], false);
    assert_eq!(bind2["error_code"], "DEVICE_ALREADY_BOUND");
}

#[test]
fn test_bind_device_empty_device_id() {
    let ext = create_default_ext();

    let result = run_command(
        &ext,
        "bind_device",
        &json!({"device_id": "", "metric_name": "image"}),
    );
    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "INVALID_ARGUMENTS");
}

#[test]
fn test_bind_device_missing_device_id() {
    let ext = create_default_ext();

    // Missing device_id entirely -- this returns ExtensionError::InvalidArguments
    // which gets propagated as an Err, not a JSON response.
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let result = rt.block_on(ext.execute_command(
        "bind_device",
        &json!({"metric_name": "image"}),
    ));
    assert!(result.is_err(), "Missing device_id should return error");
}

// ============================================================================
// 6. Toggle Binding Tests
// ============================================================================

#[test]
fn test_toggle_binding() {
    let ext = create_default_ext();

    // Bind a device
    let _ = run_command(
        &ext,
        "bind_device",
        &json!({"device_id": "cam-03", "metric_name": "image"}),
    );

    // Toggle to inactive
    let toggle = run_command(
        &ext,
        "toggle_binding",
        &json!({"device_id": "cam-03", "active": false}),
    );
    assert_eq!(toggle["success"], true);
    assert_eq!(toggle["active"], false);

    // Verify binding is inactive
    let bindings = run_command(&ext, "get_bindings", &json!({}));
    let arr = bindings["bindings"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["active"], false);

    // Toggle back to active
    let toggle2 = run_command(
        &ext,
        "toggle_binding",
        &json!({"device_id": "cam-03", "active": true}),
    );
    assert_eq!(toggle2["success"], true);
    assert_eq!(toggle2["active"], true);
}

#[test]
fn test_toggle_binding_device_not_bound() {
    let ext = create_default_ext();

    let result = run_command(
        &ext,
        "toggle_binding",
        &json!({"device_id": "nonexistent", "active": true}),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "DEVICE_NOT_FOUND");
}

// ============================================================================
// 7. Configuration Tests
// ============================================================================

#[test]
fn test_configure_and_get_config() {
    let ext = create_default_ext();

    // Configure with custom values
    let config_result = run_command(
        &ext,
        "configure",
        &json!({
            "config": {
                "confidence_threshold": 0.7,
                "recognition_threshold": 0.6,
                "max_faces": 50,
                "auto_detect": false,
            },
        }),
    );

    assert_eq!(config_result["success"], true);

    // Verify via get_config
    let get_config = run_command(&ext, "get_config", &json!({}));
    assert_eq!(get_config["success"], true);

    let config = &get_config["config"];
    assert_eq!(config["confidence_threshold"], 0.7);
    assert_eq!(config["recognition_threshold"], 0.6);
    assert_eq!(config["max_faces"], 50);
    assert_eq!(config["auto_detect"], false);
}

#[test]
fn test_configure_partial_update() {
    let ext = create_default_ext();

    // Only update recognition_threshold
    let _ = run_command(
        &ext,
        "configure",
        &json!({
            "config": {
                "recognition_threshold": 0.55,
            },
        }),
    );

    let config = run_command(&ext, "get_config", &json!({}));
    let c = &config["config"];

    // recognition_threshold should be updated
    assert_eq!(c["recognition_threshold"], 0.55);
    // Others should keep defaults
    assert_eq!(c["confidence_threshold"], 0.5);
    assert_eq!(c["max_faces"], 10);
    assert_eq!(c["auto_detect"], true);
}

// ============================================================================
// 8. get_status Tests
// ============================================================================

#[test]
fn test_get_status_valid_json_structure() {
    let ext = create_ext(
        MockDetector::new(vec![]), // Empty faces so get_status probe won't fail
        MockRecognizer::new(make_feature(512, 1.0)),
    );

    let status = run_command(&ext, "get_status", &json!({}));

    // Verify all expected fields exist
    assert!(status.get("total_bindings").is_some(), "Missing total_bindings");
    assert!(status.get("total_inferences").is_some(), "Missing total_inferences");
    assert!(status.get("total_recognized").is_some(), "Missing total_recognized");
    assert!(status.get("total_unknown").is_some(), "Missing total_unknown");
    assert!(status.get("registered_faces").is_some(), "Missing registered_faces");
    assert!(status.get("config").is_some(), "Missing config");

    // Verify initial values
    assert_eq!(status["total_bindings"], 0);
    assert_eq!(status["total_inferences"], 0);
    assert_eq!(status["total_recognized"], 0);
    assert_eq!(status["total_unknown"], 0);
    assert_eq!(status["registered_faces"], 0);
}

#[test]
fn test_get_status_reflects_registered_faces() {
    let ext = create_default_ext();
    let image_b64 = make_minimal_image_b64();

    // Register a face
    let _ = run_command(
        &ext,
        "register_face",
        &json!({"name": "StatusTest", "image": &image_b64}),
    );

    let status = run_command(&ext, "get_status", &json!({}));
    assert_eq!(status["registered_faces"], 1);
}

// ============================================================================
// 9. get_bindings Tests
// ============================================================================

#[test]
fn test_get_bindings_empty() {
    let ext = create_default_ext();

    let result = run_command(&ext, "get_bindings", &json!({}));
    assert_eq!(result["success"], true);
    assert_eq!(result["bindings"].as_array().unwrap().len(), 0);
}

#[test]
fn test_get_bindings_multiple() {
    let ext = create_default_ext();

    // Bind 3 devices
    for id in &["cam-a", "cam-b", "cam-c"] {
        let r = run_command(
            &ext,
            "bind_device",
            &json!({"device_id": *id, "metric_name": "image"}),
        );
        assert_eq!(r["success"], true);
    }

    let result = run_command(&ext, "get_bindings", &json!({}));
    assert_eq!(result["success"], true);
    assert_eq!(result["bindings"].as_array().unwrap().len(), 3);
}

// ============================================================================
// 10. Concurrent Safety Tests
// ============================================================================

#[test]
fn test_concurrent_register_and_delete_database() {
    let db = Arc::new(RwLock::new(FaceDatabase::new(0.5, 100)));
    let errors = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Spawn 10 threads each registering a face
    for i in 0..10 {
        let db = Arc::clone(&db);
        let errors = Arc::clone(&errors);
        handles.push(std::thread::spawn(move || {
            let feature = make_feature(128, i as f32);
            let name = format!("ThreadFace-{}", i);
            match db.write().register(&name, feature, "thumb") {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Register error in thread {}: {:?}", i, e);
                    errors.fetch_add(1, Ordering::SeqCst);
                }
            }
        }));
    }

    // Wait for all register threads
    for h in handles {
        h.join().expect("Thread panicked");
    }

    // Verify all 10 faces registered
    let count_after_register = db.read().len();
    assert_eq!(
        count_after_register, 10,
        "Expected 10 faces after concurrent register, got {}",
        count_after_register
    );
    assert_eq!(errors.load(Ordering::SeqCst), 0, "Some register calls failed");

    // Now spawn 5 threads deleting faces
    let face_ids: Vec<String> = db.read().list_faces().iter().map(|f| f.id.clone()).collect();
    let mut delete_handles = vec![];
    for i in 0..5 {
        let db = Arc::clone(&db);
        let face_id = face_ids[i].clone();
        delete_handles.push(std::thread::spawn(move || {
            db.write().delete(&face_id).expect("Delete should succeed");
        }));
    }

    for h in delete_handles {
        h.join().expect("Delete thread panicked");
    }

    let count_after_delete = db.read().len();
    assert_eq!(
        count_after_delete, 5,
        "Expected 5 faces after deleting 5, got {}",
        count_after_delete
    );

    // Verify database state is consistent (id_by_name index matches faces_by_id)
    let db_read = db.read();
    let list = db_read.list_faces();
    assert_eq!(list.len(), count_after_delete);
}

#[test]
fn test_concurrent_reads_during_write() {
    let db = Arc::new(RwLock::new(FaceDatabase::new(0.45, 50)));

    // Pre-populate with some faces
    {
        let mut db_w = db.write();
        for i in 0..10 {
            let feature = make_feature(128, i as f32);
            db_w
                .register(&format!("Preload-{}", i), feature, "thumb")
                .unwrap();
        }
    }

    let read_count = Arc::new(AtomicUsize::new(0));
    let write_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Spawn 10 reader threads that continuously read
    for _ in 0..10 {
        let db = Arc::clone(&db);
        let read_count = Arc::clone(&read_count);
        handles.push(std::thread::spawn(move || {
            for _ in 0..100 {
                let db_read = db.read();
                let faces = db_read.list_faces();
                // Every read should return a valid list (no panics, no corruption)
                assert!(faces.len() <= 60, "Face count should be bounded");
                drop(db_read);
                read_count.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    // Spawn 5 writer threads that register/delete
    for i in 0..5 {
        let db = Arc::clone(&db);
        let write_count = Arc::clone(&write_count);
        let read_count = Arc::clone(&read_count);
        handles.push(std::thread::spawn(move || {
            let feature = make_feature(128, (100 + i) as f32);
            let name = format!("Writer-{}", i);
            // Register
            {
                let mut db_w = db.write();
                db_w.register(&name, feature, "thumb").unwrap();
            }
            write_count.fetch_add(1, Ordering::SeqCst);

            // List (read during write phase)
            {
                let db_r = db.read();
                let _faces = db_r.list_faces();
            }
            read_count.fetch_add(1, Ordering::SeqCst);

            // Delete
            {
                let mut db_w = db.write();
                // Find the face we just registered
                let faces = db_w.list_faces();
                let target = faces.iter().find(|f| f.name == name);
                if let Some(t) = target {
                    db_w.delete(&t.id).unwrap();
                }
            }
            write_count.fetch_add(1, Ordering::SeqCst);
        }));
    }

    for h in handles {
        h.join().expect("Thread panicked during concurrent access");
    }

    // No assertion on final count since writers register+delete,
    // but verify reads always returned valid data (no panics = success).
    assert!(
        read_count.load(Ordering::SeqCst) > 0,
        "Should have completed reads"
    );
    assert!(
        write_count.load(Ordering::SeqCst) > 0,
        "Should have completed writes"
    );
}

#[test]
fn test_concurrent_config_access() {
    let config = Arc::new(RwLock::new(serde_json::to_value(
        neomind_extension_face_recognition::FaceRecConfig::default(),
    )
    .unwrap()));

    let mut handles = vec![];

    // Writer: modify config values
    for i in 0..5 {
        let config = Arc::clone(&config);
        handles.push(std::thread::spawn(move || {
            let mut cfg = config.write();
            cfg["recognition_threshold"] = json!(0.3 + i as f64 * 0.1);
            cfg["max_faces"] = json!(10 + i * 5);
            drop(cfg);
        }));
    }

    // Readers: read config values
    for _ in 0..20 {
        let config = Arc::clone(&config);
        handles.push(std::thread::spawn(move || {
            for _ in 0..50 {
                let cfg = config.read();
                // Verify the JSON structure is always valid
                assert!(cfg.get("confidence_threshold").is_some());
                assert!(cfg.get("recognition_threshold").is_some());
                assert!(cfg.get("max_faces").is_some());
                assert!(cfg.get("auto_detect").is_some());
                assert!(cfg.get("bindings").is_some());
                drop(cfg);
            }
        }));
    }

    for h in handles {
        h.join().expect("Config access thread panicked");
    }

    // Verify final config is still valid JSON
    let final_config = config.read();
    assert!(final_config.get("confidence_threshold").unwrap().as_f64().is_some());
    assert!(final_config.get("max_faces").unwrap().as_u64().is_some());
}

#[test]
fn test_concurrent_match_face_reads() {
    let db = Arc::new(RwLock::new(FaceDatabase::new(0.3, 50)));

    // Register several faces
    {
        let mut db_w = db.write();
        for i in 0..20 {
            let feature = make_feature(128, i as f32);
            db_w
                .register(&format!("MatchFace-{}", i), feature, "thumb")
                .unwrap();
        }
    }

    let match_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Many threads doing match_face concurrently
    for i in 0..20 {
        let db = Arc::clone(&db);
        let match_count = Arc::clone(&match_count);
        handles.push(std::thread::spawn(move || {
            for _ in 0..100 {
                let feature = make_feature(128, i as f32);
                let db_read = db.read();
                let result = db_read.match_face(&feature);
                // Same feature should always match
                assert!(
                    result.is_some(),
                    "Same feature should match registered face"
                );
                drop(db_read);
                match_count.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    for h in handles {
        h.join().expect("Match thread panicked");
    }

    assert_eq!(
        match_count.load(Ordering::SeqCst),
        2000,
        "All match_face calls should complete"
    );
}

#[test]
fn test_concurrent_register_delete_with_bindings() {
    let ext = Arc::new(create_default_ext());
    let image_b64 = Arc::new(make_minimal_image_b64());
    let success_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Thread 1: Bind and unbind devices
    {
        let ext = Arc::clone(&ext);
        handles.push(std::thread::spawn(move || {
            for i in 0..5 {
                let device_id = format!("dev-{}", i);
                let r = run_command(
                    &ext,
                    "bind_device",
                    &json!({"device_id": &device_id, "metric_name": "image"}),
                );
                if r["success"] == true {
                    let _ = run_command(
                        &ext,
                        "unbind_device",
                        &json!({"device_id": &device_id}),
                    );
                }
            }
        }));
    }

    // Thread 2: Register faces
    {
        let ext = Arc::clone(&ext);
        let image_b64 = Arc::clone(&image_b64);
        let success_count = Arc::clone(&success_count);
        handles.push(std::thread::spawn(move || {
            for i in 0..5 {
                let name = format!("ConcFace-{}", i);
                let r = run_command(
                    &ext,
                    "register_face",
                    &json!({"name": &name, "image": &*image_b64}),
                );
                if r["success"] == true {
                    success_count.fetch_add(1, Ordering::SeqCst);
                }
            }
        }));
    }

    // Thread 3: Read status and list faces concurrently
    {
        let ext = Arc::clone(&ext);
        handles.push(std::thread::spawn(move || {
            for _ in 0..10 {
                let status = run_command(&ext, "get_status", &json!({}));
                assert!(status.get("total_bindings").is_some());

                let list = run_command(&ext, "list_faces", &json!({}));
                assert!(list["count"].as_u64().is_some());
            }
        }));
    }

    for h in handles {
        h.join().expect("Binding thread panicked");
    }

    // Verify extension state is consistent
    let status = run_command(&ext, "get_status", &json!({}));
    let registered = status["registered_faces"].as_u64().unwrap() as usize;
    let success = success_count.load(Ordering::SeqCst);
    assert_eq!(
        registered, success,
        "Registered face count should match successful registrations"
    );
}

// ============================================================================
// 11. Edge Cases
// ============================================================================

#[test]
fn test_register_face_data_uri_prefix() {
    let ext = create_default_ext();

    // Create base64 with data URI prefix
    let image_b64 = format!("data:image/jpeg;base64,{}", make_minimal_image_b64());

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "DataUriFace",
            "image": image_b64,
        }),
    );

    assert_eq!(result["success"], true, "data URI prefix should be handled");
}

#[test]
fn test_register_face_invalid_base64() {
    let ext = create_default_ext();

    let result = run_command(
        &ext,
        "register_face",
        &json!({
            "name": "BadBase64",
            "image": "not-valid-base64!!!",
        }),
    );

    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "INVALID_ARGUMENTS");
}

#[test]
fn test_unknown_command_returns_error() {
    let ext = create_default_ext();

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let result = rt.block_on(ext.execute_command("nonexistent_command", &json!({})));

    assert!(result.is_err(), "Unknown command should return error");
}

#[test]
fn test_unbind_nonexistent_device_succeeds() {
    // Unbinding a device that was never bound should succeed (idempotent remove)
    let ext = create_default_ext();

    let result = run_command(
        &ext,
        "unbind_device",
        &json!({"device_id": "never-bound"}),
    );
    assert_eq!(result["success"], true);
}
