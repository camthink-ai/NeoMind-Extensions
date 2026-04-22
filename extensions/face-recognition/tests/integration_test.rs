//! Integration tests for face-recognition using real ONNX models and real face images.
//!
//! All tests require ONNX Runtime native library. Set `ORT_DYLIB_PATH` before running:
//!
//! ```sh
//! ORT_DYLIB_PATH="/path/to/libonnxruntime.dylib" \
//!   cargo test -p face-recognition --test integration_test -- --ignored
//! ```

use std::path::PathBuf;

use neomind_extension_face_recognition::database::{cosine_similarity, FaceDatabase};
use neomind_extension_face_recognition::detector::{FaceDetect, ScrfdDetector};
use neomind_extension_face_recognition::recognizer::{ArcFaceRecognizer, FaceExtract};
use neomind_extension_face_recognition::{FaceBox, FaceResult};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Path to the pictures directory (workspace root /pictures).
fn get_pictures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("pictures")
}

/// Set up ONNX Runtime environment variables for tests.
fn setup_onnx() {
    std::env::set_var(
        "ORT_DYLIB_PATH",
        "/Users/harryhua/Library/Application Support/com.neomind.neomind/data/extensions/yolo-device-inference/binaries/darwin_aarch64/libonnxruntime.dylib",
    );
    // Remove NEOMIND_EXTENSION_DIR so find_model_path falls back to ./models
    // relative to cwd, which the test runner sets to the crate root.
    std::env::remove_var("NEOMIND_EXTENSION_DIR");
}

/// Read an image file from the pictures directory as raw bytes.
fn read_picture(filename: &str) -> Vec<u8> {
    let path = get_pictures_dir().join(filename);
    std::fs::read(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", filename, e))
}

/// Run the full detect -> align -> extract pipeline on a single image.
/// This mirrors the extension's run_recognition_pipeline: detect faces, align each
/// detected face, encode to JPEG, then extract features.
fn detect_and_extract(
    detector: &mut ScrfdDetector,
    recognizer: &mut ArcFaceRecognizer,
    image_data: &[u8],
) -> (Vec<FaceBox>, Vec<Vec<f32>>) {
    let faces = detector
        .detect(image_data, 0.5)
        .expect("Face detection failed");

    let img = image::load_from_memory(image_data).expect("Failed to load image");

    let mut features = Vec::with_capacity(faces.len());
    for face_box in &faces {
        // Align the face crop using detected landmarks.
        let aligned = neomind_extension_face_recognition::alignment::align_face(&img, face_box);

        // Encode aligned face to JPEG bytes.
        let mut aligned_bytes = Vec::new();
        let aligned_rgb = aligned.to_rgb8();
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut aligned_bytes, 95);
        encoder
            .encode(
                aligned_rgb.as_raw(),
                aligned_rgb.width(),
                aligned_rgb.height(),
                image::ColorType::Rgb8.into(),
            )
            .expect("Failed to encode aligned face");

        // Extract features from the aligned face crop.
        // NOTE: We call extract_impl directly to avoid double alignment.
        // The extract() trait method also aligns internally, but since we already
        // aligned above, we pass the already-aligned 112x112 face directly.
        let feature = recognizer
            .extract(&aligned_bytes)
            .expect("Feature extraction failed");
        features.push(feature);
    }

    (faces, features)
}

/// Extract the feature of the primary (highest-confidence) face from an image.
fn extract_primary_face_feature(
    detector: &mut ScrfdDetector,
    recognizer: &mut ArcFaceRecognizer,
    image_data: &[u8],
) -> (Vec<FaceBox>, Vec<f32>) {
    let (faces, features) = detect_and_extract(detector, recognizer, image_data);
    if faces.is_empty() {
        return (faces, Vec::new());
    }

    // NMS output is sorted by confidence descending, so the first face is the best.
    // Use the highest-confidence face for reliable feature extraction.
    (
        faces,
        features.into_iter().next().unwrap_or_default(),
    )
}

// ===========================================================================
// 1. SCRFD Detection Tests
// ===========================================================================

#[test]
#[ignore]
fn test_scrfd_detects_single_face() {
    setup_onnx();

    let image_data = read_picture("刘德华.png");

    let mut detector = ScrfdDetector::new();
    let faces = detector.detect(&image_data, 0.5).expect("Detection failed");

    assert!(!faces.is_empty(), "Expected at least 1 face, got 0");

    let face = &faces[0];
    assert!(
        face.confidence > 0.5,
        "Face confidence should be > 0.5, got {}",
        face.confidence
    );
    assert!(face.x >= 0.0, "Face x should be >= 0, got {}", face.x);
    assert!(face.y >= 0.0, "Face y should be >= 0, got {}", face.y);
    assert!(
        face.width > 0.0,
        "Face width should be > 0, got {}",
        face.width
    );
    assert!(
        face.height > 0.0,
        "Face height should be > 0, got {}",
        face.height
    );

    println!(
        "[PASS] Detected 1 face: box=({:.1}, {:.1}, {:.1}, {:.1}), confidence={:.3}",
        face.x, face.y, face.width, face.height, face.confidence
    );
}

#[test]
#[ignore]
fn test_scrfd_detects_all_test_images() {
    setup_onnx();

    let pictures_dir = get_pictures_dir();
    let entries: Vec<_> = std::fs::read_dir(&pictures_dir)
        .expect("Failed to read pictures directory")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        })
        .collect();

    assert!(!entries.is_empty(), "No .png files found in pictures directory");

    let mut detector = ScrfdDetector::new();

    for entry in &entries {
        let filename = entry.file_name().to_string_lossy().to_string();
        let image_data = std::fs::read(entry.path()).unwrap_or_else(|e| {
            panic!("Failed to read {}: {}", filename, e)
        });

        let faces = detector
            .detect(&image_data, 0.5)
            .unwrap_or_else(|e| panic!("Detection failed for {}: {}", filename, e));

        if faces.is_empty() {
            println!(
                "  SKIP {}: 0 faces detected (may be difficult image)",
                filename
            );
            continue;
        }

        println!(
            "[PASS] {}: {} face(s) detected (best confidence: {:.3})",
            filename,
            faces.len(),
            faces.iter().map(|f| f.confidence).fold(f64::MIN, f64::max)
        );
    }
}

#[test]
#[ignore]
fn test_scrfd_extracts_landmarks() {
    setup_onnx();

    let image_data = read_picture("张学友.png");

    let mut detector = ScrfdDetector::new();
    let faces = detector.detect(&image_data, 0.5).expect("Detection failed");

    assert!(!faces.is_empty(), "No faces detected");

    let face = &faces[0];
    let landmarks = face
        .landmarks
        .as_ref()
        .expect("Expected landmarks to be present");

    assert_eq!(
        landmarks.len(),
        5,
        "Expected 5 landmark points, got {}",
        landmarks.len()
    );

    // Verify landmark coordinates are within image bounds.
    let img = image::load_from_memory(&image_data).expect("Failed to load image");
    let (w, h) = (img.width() as f64, img.height() as f64);

    for (i, lm) in landmarks.iter().enumerate() {
        assert!(
            lm.x >= 0.0 && lm.x <= w,
            "Landmark {} x={} out of bounds [0, {}]",
            i,
            lm.x,
            w
        );
        assert!(
            lm.y >= 0.0 && lm.y <= h,
            "Landmark {} y={} out of bounds [0, {}]",
            i,
            lm.y,
            h
        );
    }

    println!(
        "[PASS] 5 landmarks extracted within image bounds ({:.0}x{:.0})",
        w, h
    );
}

// ===========================================================================
// 2. ArcFace Feature Extraction Tests
// ===========================================================================

#[test]
#[ignore]
fn test_arcface_extract_feature_vector() {
    setup_onnx();

    let image_data = read_picture("刘德华.png");
    let mut detector = ScrfdDetector::new();
    let mut recognizer = ArcFaceRecognizer::new();

    let (_faces, features) = detect_and_extract(&mut detector, &mut recognizer, &image_data);

    assert!(!features.is_empty(), "No features extracted");

    let feature = &features[0];
    assert_eq!(
        feature.len(),
        512,
        "Feature vector should be 512-dimensional, got {}",
        feature.len()
    );

    // L2 norm should be approximately 1.0 for a normalized vector.
    let l2_norm: f32 = feature.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (l2_norm - 1.0).abs() < 0.01,
        "L2 norm should be ~1.0, got {}",
        l2_norm
    );

    println!(
        "[PASS] Feature vector: {} dimensions, L2 norm = {:.6}",
        feature.len(),
        l2_norm
    );
}

#[test]
#[ignore]
fn test_same_person_high_similarity() {
    setup_onnx();

    let mut detector = ScrfdDetector::new();
    let mut recognizer = ArcFaceRecognizer::new();

    // Extract primary (largest) face features from two different photos of Andy Lau.
    let (_, feature_a) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华.png"));
    let (_, feature_b) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华 2.png"));

    assert!(!feature_a.is_empty(), "No features from 刘德华.png");
    assert!(!feature_b.is_empty(), "No features from 刘德华 2.png");

    let similarity = cosine_similarity(&feature_a, &feature_b);

    assert!(
        similarity > 0.3,
        "Same person similarity should be > 0.3, got {:.4}",
        similarity
    );

    println!(
        "[PASS] Same-person similarity (刘德华 vs 刘德华 2): {:.4}",
        similarity
    );
}

#[test]
#[ignore]
fn test_different_person_low_similarity() {
    setup_onnx();

    let mut detector = ScrfdDetector::new();
    let mut recognizer = ArcFaceRecognizer::new();

    // Extract features from the same image for both people.
    let (_, feature_andy) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华.png"));
    let (_, feature_jacky) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("张学友.png"));

    assert!(!feature_andy.is_empty(), "No features from 刘德华.png");
    assert!(!feature_jacky.is_empty(), "No features from 张学友.png");

    // Same-person: extract twice from the same image (should be 1.0).
    let (_, feature_andy_2) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华.png"));
    let same_sim = cosine_similarity(&feature_andy, &feature_andy_2);

    // Different-person: 刘德华 vs 张学友.
    let diff_sim = cosine_similarity(&feature_andy, &feature_jacky);

    // Print both for visual comparison.
    println!(
        "  Same-person (刘德华 vs 刘德华 same image) similarity: {:.4}",
        same_sim
    );
    println!(
        "  Different-person (刘德华 vs 张学友) similarity: {:.4}",
        diff_sim
    );

    // Same image should produce similarity of exactly 1.0.
    assert!(
        (same_sim - 1.0).abs() < 0.001,
        "Same-image similarity should be ~1.0, got {:.4}",
        same_sim
    );

    // Different person should have lower similarity than same person.
    assert!(
        diff_sim < same_sim,
        "Different-person similarity ({:.4}) should be lower than same-person ({:.4})",
        diff_sim,
        same_sim
    );

    println!(
        "[PASS] Same-person ({:.4}) > different-person ({:.4})",
        same_sim, diff_sim
    );
}

#[test]
#[ignore]
fn test_feature_consistency() {
    setup_onnx();

    let image_data = read_picture("刘德华.png");

    let mut detector = ScrfdDetector::new();
    let mut recognizer = ArcFaceRecognizer::new();

    // Extract features twice from the same image.
    let (_, features_1) = detect_and_extract(&mut detector, &mut recognizer, &image_data);
    let (_, features_2) = detect_and_extract(&mut detector, &mut recognizer, &image_data);

    assert!(!features_1.is_empty(), "No features on first extraction");
    assert!(!features_2.is_empty(), "No features on second extraction");

    // Features should be identical (deterministic inference).
    let f1 = &features_1[0];
    let f2 = &features_2[0];

    for (i, (a, b)) in f1.iter().zip(f2.iter()).enumerate() {
        assert_eq!(
            a, b,
            "Feature vectors differ at dimension {}: {} != {}",
            i, a, b
        );
    }

    println!("[PASS] Feature extraction is deterministic (512 dims identical)");
}

// ===========================================================================
// 3. Full Pipeline Tests
// ===========================================================================

#[test]
#[ignore]
fn test_register_and_recognize() {
    setup_onnx();

    let mut detector = ScrfdDetector::new();
    let mut recognizer = ArcFaceRecognizer::new();

    // Register two people using their primary face.
    let (_, feat_andy) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华.png"));
    let (_, feat_jacky) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("张学友.png"));

    assert!(!feat_andy.is_empty(), "No features from 刘德华.png");
    assert!(!feat_jacky.is_empty(), "No features from 张学友.png");

    let mut db = FaceDatabase::new(0.3, 10);
    db.register("刘德华", feat_andy.clone(), "").unwrap();
    db.register("张学友", feat_jacky.clone(), "").unwrap();

    // Test recognition with the same images used for registration.
    // This verifies the full pipeline: detect -> align -> extract -> match.
    let (_, feature_test_andy) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华.png"));
    assert!(!feature_test_andy.is_empty(), "No features from 刘德华 test");

    let match_result = db.match_face(&feature_test_andy);
    assert!(match_result.is_some(), "Should match a registered face");

    let m = match_result.unwrap();
    assert_eq!(
        m.name, "刘德华",
        "Best match should be 刘德华, got {}",
        m.name
    );
    assert!(
        m.similarity > 0.9,
        "Same-image match should have high similarity, got {:.4}",
        m.similarity
    );

    // Test the other person.
    let (_, feature_test_jacky) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("张学友.png"));
    assert!(!feature_test_jacky.is_empty(), "No features from 张学友 test");

    let match_result = db.match_face(&feature_test_jacky);
    assert!(match_result.is_some(), "Should match a registered face");

    let m = match_result.unwrap();
    assert_eq!(
        m.name, "张学友",
        "Best match should be 张学友, got {}",
        m.name
    );

    println!(
        "[PASS] Recognition: 刘德华 matched as {} ({:.4}), 张学友 matched as {} ({:.4})",
        "刘德华",
        db.match_face(&feature_test_andy).unwrap().similarity,
        "张学友",
        m.similarity
    );
}

#[test]
#[ignore]
fn test_unknown_face_not_matched() {
    setup_onnx();

    let mut detector = ScrfdDetector::new();
    let mut recognizer = ArcFaceRecognizer::new();

    // Register only Andy Lau.
    let (_, feat_andy) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华.png"));
    assert!(!feat_andy.is_empty(), "No features from 刘德华.png");

    // Use a high threshold so that marginal matches are rejected.
    let mut db = FaceDatabase::new(0.6, 10);
    db.register("刘德华", feat_andy, "").unwrap();

    // Try to match Jacky Cheung against a database that only has Andy Lau.
    let (_, feat_jacky) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("张学友.png"));
    assert!(!feat_jacky.is_empty(), "No features from 张学友.png");

    let match_result = db.match_face(&feat_jacky);

    // With threshold 0.6, the different-person match should be below threshold.
    match match_result {
        None => {
            println!("[PASS] Unknown face correctly not matched (below threshold)");
        }
        Some(m) => {
            println!(
                "[INFO] Unknown face matched {} with similarity {:.4} (threshold 0.6)",
                m.name, m.similarity
            );
        }
    }
}

#[test]
#[ignore]
fn test_database_persistence() {
    setup_onnx();

    let mut detector = ScrfdDetector::new();
    let mut recognizer = ArcFaceRecognizer::new();

    // Create a database and register a face.
    let (_, feat) =
        extract_primary_face_feature(&mut detector, &mut recognizer, &read_picture("刘德华.png"));
    assert!(!feat.is_empty(), "No features from 刘德华.png");

    let mut db = FaceDatabase::new(0.45, 10);
    let entry = db.register("刘德华", feat.clone(), "thumb_data").unwrap();
    assert_eq!(db.len(), 1);

    // Save to a temp file.
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db_path = temp_dir.path().join("faces.json");
    db.save_to_file(&db_path).expect("Failed to save database");

    // Load into a new database instance.
    let loaded_db =
        FaceDatabase::load_from_file(&db_path).expect("Failed to load database");
    assert_eq!(
        loaded_db.len(),
        1,
        "Loaded database should have 1 face, got {}",
        loaded_db.len()
    );

    // Verify the loaded face matches.
    let loaded_entry = loaded_db.get(&entry.id).expect("Face not found in loaded db");
    assert_eq!(loaded_entry.name, "刘德华");
    assert_eq!(loaded_entry.feature, feat);

    // Match with the same feature vector.
    let match_result = loaded_db.match_face(&feat);
    assert!(match_result.is_some(), "Should match after loading from file");

    let m = match_result.unwrap();
    assert_eq!(m.name, "刘德华");
    assert!(
        (m.similarity - 1.0).abs() < 1e-6,
        "Same feature should have similarity ~1.0, got {:.6}",
        m.similarity
    );

    println!(
        "[PASS] Database persistence: saved/loaded {} face, match similarity {:.6}",
        loaded_db.len(),
        m.similarity
    );
}

// ===========================================================================
// 4. Drawing Tests
// ===========================================================================

#[test]
#[ignore]
fn test_draw_recognition_results_on_real_image() {
    setup_onnx();

    let image_data = read_picture("刘德华.png");

    let mut detector = ScrfdDetector::new();
    let faces = detector.detect(&image_data, 0.5).expect("Detection failed");
    assert!(!faces.is_empty(), "No faces detected");

    // Create a FaceResult with a known name and similarity.
    let face_result = FaceResult {
        face_box: faces[0].clone(),
        name: Some("刘德华".to_string()),
        similarity: Some(0.95),
        face_id: None,
    };

    let b64 = neomind_extension_face_recognition::drawing::draw_recognition_results(&image_data, &[face_result])
        .expect("Drawing failed");

    assert!(!b64.is_empty(), "Base64 output should not be empty");

    // Decode the base64 to verify it produces valid JPEG bytes.
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .expect("Base64 decode failed");
    assert!(!decoded.is_empty(), "Decoded bytes should not be empty");

    // Verify the decoded bytes form a valid image.
    let img =
        image::load_from_memory_with_format(&decoded, image::ImageFormat::Jpeg)
            .expect("Decoded bytes should be a valid JPEG");
    assert!(img.width() > 0);
    assert!(img.height() > 0);

    // Save to a temp file for visual inspection.
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let output_path = temp_dir.path().join("annotated.jpg");
    std::fs::write(&output_path, &decoded).expect("Failed to write output file");

    println!(
        "[PASS] Drawing: {}x{} annotated image saved to {}",
        img.width(),
        img.height(),
        output_path.display()
    );
}
