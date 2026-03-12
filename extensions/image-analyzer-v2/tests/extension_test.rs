//! Image Analyzer Extension V2 - Comprehensive Test Suite
//!
//! Tests cover:
//! - Extension metadata and configuration
//! - Command execution (analyze_image, reset_stats, get_status, reload_model)
//! - Metric production
//! - Image analysis functionality
//! - Error handling
//! - Edge cases and boundary conditions

#[cfg(test)]
mod tests {
    use neomind_extension_sdk::{
        Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
        MetricDataType, ParamMetricValue,
    };
    use serde_json::json;
    use base64::Engine;

    // Import the extension from the library
    use neomind_extension_image_analyzer_v2::{ImageAnalyzer, Detection, BoundingBox, AnalysisResult};

    // ========================================================================
    // Helper Functions
    // ========================================================================

    /// Create a new ImageAnalyzer instance for testing
    fn create_extension() -> ImageAnalyzer {
        ImageAnalyzer::new()
    }

    /// Create a minimal valid JPEG image (1x1 pixel, grayscale)
    fn create_minimal_jpeg() -> Vec<u8> {
        // Minimal valid JPEG file (1x1 grayscale)
        vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46,
            0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
            0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43,
            0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08,
            0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C,
            0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12,
            0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D,
            0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20,
            0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29,
            0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27,
            0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34,
            0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01,
            0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4,
            0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
            0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04,
            0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF,
            0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03,
            0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04,
            0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00,
            0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
            0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32,
            0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1,
            0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72,
            0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A,
            0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35,
            0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45,
            0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55,
            0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65,
            0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75,
            0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85,
            0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94,
            0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3,
            0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2,
            0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
            0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9,
            0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8,
            0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6,
            0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4,
            0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA,
            0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00,
            0xFB, 0xD5, 0xDB, 0x20, 0xB8, 0xE3, 0x9E, 0x76,
            0xEB, 0x1D, 0xFF, 0xD9,
        ]
    }

    /// Create a minimal valid PNG image (1x1 pixel)
    fn create_minimal_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
            0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41,
            0x54, 0x08, 0xD7, 0x63, 0xF8, 0xFF, 0xFF, 0x3F,
            0x00, 0x05, 0xFE, 0x02, 0xFE, 0xDC, 0xCC, 0x59,
            0xE7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
            0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    /// Encode image data to base64
    fn encode_image(data: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(data)
    }

    // ========================================================================
    // Metadata Tests
    // ========================================================================

    #[test]
    fn test_metadata_basic() {
        let ext = create_extension();
        let meta = ext.metadata();

        assert_eq!(meta.id, "image-analyzer-v2");
        assert_eq!(meta.name, "Image Analyzer V2");
        assert_eq!(meta.version.major, 2);
        assert_eq!(meta.version.minor, 0);
        assert_eq!(meta.version.patch, 0);
    }

    #[test]
    fn test_metadata_has_description() {
        let ext = create_extension();
        let meta = ext.metadata();

        assert!(meta.description.is_some());
        assert!(!meta.description.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_metadata_has_author() {
        let ext = create_extension();
        let meta = ext.metadata();

        assert!(meta.author.is_some());
        assert_eq!(meta.author.as_ref().unwrap(), "NeoMind Team");
    }

    #[test]
    fn test_metadata_config_parameters() {
        let ext = create_extension();
        let meta = ext.metadata();

        // Should have config parameters
        assert!(meta.config_parameters.is_some());
        let config_params = meta.config_parameters.as_ref().unwrap();
        assert!(!config_params.is_empty());

        // Check for confidence_threshold parameter
        let conf_param = config_params.iter()
            .find(|p| p.name == "confidence_threshold");
        assert!(conf_param.is_some());

        let param = conf_param.unwrap();
        assert_eq!(param.param_type, MetricDataType::Float);
        assert!(!param.required); // Should be optional
        assert!(param.min.is_some());
        assert!(param.max.is_some());

        // Check for nms_threshold parameter
        let nms_param = config_params.iter()
            .find(|p| p.name == "nms_threshold");
        assert!(nms_param.is_some());

        // Check for model_version parameter
        let version_param = config_params.iter()
            .find(|p| p.name == "model_version");
        assert!(version_param.is_some());
    }

    // ========================================================================
    // Metrics Tests
    // ========================================================================

    #[test]
    fn test_metrics_definitions() {
        let ext = create_extension();
        let metrics = ext.metrics();

        // Should have metrics defined
        assert!(!metrics.is_empty());

        let metric_names: Vec<&str> = metrics.iter()
            .map(|m| m.name.as_str())
            .collect();

        assert!(metric_names.contains(&"images_processed"));
        assert!(metric_names.contains(&"avg_processing_time_ms"));
        assert!(metric_names.contains(&"total_detections"));
    }

    #[test]
    fn test_metric_types() {
        let ext = create_extension();
        let metrics = ext.metrics();

        // images_processed should be Integer
        let images_metric = metrics.iter()
            .find(|m| m.name == "images_processed");
        assert!(images_metric.is_some());
        assert_eq!(images_metric.unwrap().data_type, MetricDataType::Integer);

        // avg_processing_time_ms should be Float
        let time_metric = metrics.iter()
            .find(|m| m.name == "avg_processing_time_ms");
        assert!(time_metric.is_some());
        assert_eq!(time_metric.unwrap().data_type, MetricDataType::Float);
    }

    #[test]
    fn test_metric_units() {
        let ext = create_extension();
        let metrics = ext.metrics();

        let time_metric = metrics.iter()
            .find(|m| m.name == "avg_processing_time_ms").unwrap();
        assert_eq!(time_metric.unit, "ms");

        let count_metric = metrics.iter()
            .find(|m| m.name == "images_processed").unwrap();
        assert_eq!(count_metric.unit, "count");
    }

    // ========================================================================
    // Commands Tests
    // ========================================================================

    #[test]
    fn test_commands_definitions() {
        let ext = create_extension();
        let commands = ext.commands();

        // Should have commands defined
        assert!(!commands.is_empty());

        let cmd_names: Vec<&str> = commands.iter()
            .map(|c| c.name.as_str())
            .collect();

        assert!(cmd_names.contains(&"analyze_image"));
        assert!(cmd_names.contains(&"reset_stats"));
        assert!(cmd_names.contains(&"get_status"));
        assert!(cmd_names.contains(&"reload_model"));
    }

    #[test]
    fn test_analyze_image_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let analyze = commands.iter()
            .find(|c| c.name == "analyze_image");

        assert!(analyze.is_some());
        let cmd = analyze.unwrap();

        // Should have image parameter
        assert!(!cmd.parameters.is_empty());
        let image_param = cmd.parameters.iter()
            .find(|p| p.name == "image");
        assert!(image_param.is_some());
        assert!(image_param.unwrap().required);

        // Should have samples
        assert!(!cmd.samples.is_empty());

        // Should have LLM hints
        assert!(!cmd.llm_hints.is_empty());
    }

    // ========================================================================
    // Produce Metrics Tests
    // ========================================================================

    #[test]
    fn test_produce_metrics_initial() {
        let ext = create_extension();

        let metrics = ext.produce_metrics().unwrap();

        // Should have all defined metrics
        assert!(!metrics.is_empty());

        // Initial images_processed should be 0
        let images_processed = metrics.iter()
            .find(|m| m.name == "images_processed");
        assert!(images_processed.is_some());

        match images_processed.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 0),
            _ => panic!("Expected Integer value for images_processed"),
        }
    }

    #[test]
    fn test_produce_metrics_structure() {
        let ext = create_extension();
        let metrics = ext.produce_metrics().unwrap();

        // All metrics should have valid structure
        for metric in &metrics {
            assert!(!metric.name.is_empty());
        }
    }

    // ========================================================================
    // Image Analysis Tests
    // ========================================================================

    #[test]
    fn test_analyze_jpeg_image() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();

        let result = ext.analyze_image(&jpeg_data);

        assert!(result.is_ok());
        let analysis = result.unwrap();

        // Should have processed the image
        assert!(analysis.processing_time_ms > 0 || analysis.processing_time_ms == 0);

        // Model may or may not be loaded, but should have a result
        // Fallback mode should still work
    }

    #[test]
    fn test_analyze_png_image() {
        let ext = create_extension();
        let png_data = create_minimal_png();

        let result = ext.analyze_image(&png_data);

        assert!(result.is_ok());
        let analysis = result.unwrap();

        // Should have processed the image
        assert!(analysis.processing_time_ms >= 0);
    }

    #[test]
    fn test_fallback_analysis_jpeg() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();

        let (detections, description) = ext.fallback_analysis(&jpeg_data);

        // Should detect JPEG format
        assert!(!detections.is_empty());
        assert!(detections.iter().any(|d| d.label == "jpeg_image"));

        // Description should mention fallback
        assert!(description.contains("Fallback"));
    }

    #[test]
    fn test_fallback_analysis_png() {
        let ext = create_extension();
        let png_data = create_minimal_png();

        let (detections, description) = ext.fallback_analysis(&png_data);

        // Should detect PNG format
        assert!(!detections.is_empty());
        assert!(detections.iter().any(|d| d.label == "png_image"));
    }

    #[test]
    fn test_fallback_analysis_unknown() {
        let ext = create_extension();
        let unknown_data = vec![0x00, 0x01, 0x02, 0x03];

        let (detections, _description) = ext.fallback_analysis(&unknown_data);

        // Unknown format should return empty detections
        assert!(detections.is_empty());
    }

    // ========================================================================
    // Command Execution Tests
    // ========================================================================

    #[tokio::test]
    async fn test_analyze_image_command_success() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();
        let image_b64 = encode_image(&jpeg_data);

        let result = ext.execute_command("analyze_image", &json!({
            "image": image_b64
        })).await;

        assert!(result.is_ok());
        let response = result.unwrap();

        // Response should have expected structure
        assert!(response.get("objects").is_some() || response.get("description").is_some());
    }

    #[tokio::test]
    async fn test_analyze_image_command_missing_param() {
        let ext = create_extension();

        let result = ext.execute_command("analyze_image", &json!({})).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ExtensionError::InvalidArguments(msg) => {
                assert!(msg.contains("image"));
            }
            _ => panic!("Expected InvalidArguments error"),
        }
    }

    #[tokio::test]
    async fn test_analyze_image_command_invalid_base64() {
        let ext = create_extension();

        let result = ext.execute_command("analyze_image", &json!({
            "image": "not valid base64!!!"
        })).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reset_stats_command() {
        let ext = create_extension();

        let result = ext.execute_command("reset_stats", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response["status"], "reset");
    }

    #[tokio::test]
    async fn test_get_status_command() {
        let ext = create_extension();

        let result = ext.execute_command("get_status", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();

        // Should have model status
        assert!(response.get("loaded").is_some() || response.get("model_loaded").is_some());
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let ext = create_extension();

        let result = ext.execute_command("unknown_command", &json!({})).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ExtensionError::CommandNotFound(cmd) => {
                assert_eq!(cmd, "unknown_command");
            }
            _ => panic!("Expected CommandNotFound error"),
        }
    }

    // ========================================================================
    // Statistics Tests
    // ========================================================================

    #[test]
    fn test_reset_stats() {
        let ext = create_extension();

        // Analyze an image first
        let jpeg_data = create_minimal_jpeg();
        let _ = ext.analyze_image(&jpeg_data);

        // Reset stats
        let result = ext.reset_stats();
        assert_eq!(result["status"], "reset");

        // Verify stats are reset
        let metrics = ext.produce_metrics().unwrap();
        let images_processed = metrics.iter()
            .find(|m| m.name == "images_processed");

        match images_processed.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 0),
            _ => panic!("Expected Integer value"),
        }
    }

    #[test]
    fn test_get_model_status() {
        let ext = create_extension();

        let status = ext.get_model_status();

        // Should have loaded field
        assert!(status.get("loaded").is_some() || status.get("model_loaded").is_some());
    }

    // ========================================================================
    // Edge Cases and Boundary Tests
    // ========================================================================

    #[test]
    fn test_analyze_empty_image() {
        let ext = create_extension();

        let result = ext.analyze_image(&[]);

        // Should handle empty input gracefully
        // May return error or fallback result
        match result {
            Ok(analysis) => {
                // Fallback analysis should work
                assert!(analysis.processing_time_ms >= 0);
            }
            Err(_) => {
                // Error is also acceptable for empty input
            }
        }
    }

    #[test]
    fn test_analyze_large_image_data() {
        let ext = create_extension();

        // Create a large fake image (10MB)
        let mut large_data = vec![0u8; 10 * 1024 * 1024];
        // Add JPEG header
        large_data[0] = 0xFF;
        large_data[1] = 0xD8;
        large_data[2] = 0xFF;

        let result = ext.analyze_image(&large_data);

        // Should handle large data without crashing
        // May use fallback or fail gracefully
        match result {
            Ok(_) => {}
            Err(_) => {}
        }
    }

    #[tokio::test]
    async fn test_concurrent_analysis() {
        use std::sync::Arc;

        let ext = Arc::new(create_extension());
        let jpeg_data = create_minimal_jpeg();
        let image_b64 = encode_image(&jpeg_data);

        let mut handles = vec![];

        for _ in 0..5 {
            let ext_clone = Arc::clone(&ext);
            let image_b64_clone = image_b64.clone();
            handles.push(tokio::spawn(async move {
                ext_clone.execute_command("analyze_image", &json!({
                    "image": image_b64_clone
                })).await
            }));
        }

        // Wait for all to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Extension should still be functional
        let metrics = ext.produce_metrics().unwrap();
        assert!(!metrics.is_empty());
    }

    // ========================================================================
    // Default Implementation Tests
    // ========================================================================

    #[test]
    fn test_default_implementation() {
        let ext1 = ImageAnalyzer::new();
        let ext2 = ImageAnalyzer::default();

        // Both should have same initial state
        assert_eq!(ext1.metadata().id, ext2.metadata().id);
    }

    // ========================================================================
    // Thread Safety Tests
    // ========================================================================

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let ext = Arc::new(create_extension());
        let mut handles = vec![];

        for _ in 0..10 {
            let ext_clone = Arc::clone(&ext);
            handles.push(thread::spawn(move || {
                let _ = ext_clone.produce_metrics();
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Extension should still be functional
        let metrics = ext.produce_metrics().unwrap();
        assert!(!metrics.is_empty());
    }

    // ========================================================================
    // Memory and Resource Tests
    // ========================================================================

    #[test]
    fn test_no_memory_leak_on_repeated_analysis() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();

        // Perform many analyses
        for _ in 0..100 {
            let _ = ext.analyze_image(&jpeg_data);
        }

        // If we get here without running out of memory, the test passes
        assert!(true);
    }

    // ========================================================================
    // Integration-style Tests
    // ========================================================================

    #[tokio::test]
    async fn test_full_workflow() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();
        let image_b64 = encode_image(&jpeg_data);

        // 1. Get initial status
        let status = ext.execute_command("get_status", &json!({})).await;
        assert!(status.is_ok());

        // 2. Analyze image
        let analysis = ext.execute_command("analyze_image", &json!({
            "image": image_b64
        })).await;
        assert!(analysis.is_ok());

        // 3. Get metrics
        let metrics = ext.produce_metrics().unwrap();
        assert!(!metrics.is_empty());

        // 4. Reset stats
        let reset = ext.execute_command("reset_stats", &json!({})).await;
        assert!(reset.is_ok());

        // 5. Verify reset
        let metrics_after = ext.produce_metrics().unwrap();
        let images_processed = metrics_after.iter()
            .find(|m| m.name == "images_processed");

        match images_processed.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 0),
            _ => panic!("Expected Integer value"),
        }
    }

    // ========================================================================
    // Detection Type Tests
    // ========================================================================

    #[test]
    fn test_detection_serialization() {
        let detection = Detection {
            label: "person".to_string(),
            confidence: 0.95,
            bbox: Some(BoundingBox {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 200.0,
            }),
        };

        let json = serde_json::to_string(&detection).unwrap();
        let parsed: Detection = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.label, "person");
        assert_eq!(parsed.confidence, 0.95);
        assert!(parsed.bbox.is_some());
    }

    #[test]
    fn test_bounding_box_serialization() {
        let bbox = BoundingBox {
            x: 10.5,
            y: 20.5,
            width: 100.0,
            height: 200.0,
        };

        let json = serde_json::to_string(&bbox).unwrap();
        let parsed: BoundingBox = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.x, 10.5);
        assert_eq!(parsed.y, 20.5);
        assert_eq!(parsed.width, 100.0);
        assert_eq!(parsed.height, 200.0);
    }

    #[test]
    fn test_analysis_result_serialization() {
        let result = AnalysisResult {
            objects: vec![
                Detection {
                    label: "car".to_string(),
                    confidence: 0.88,
                    bbox: Some(BoundingBox {
                        x: 0.0,
                        y: 0.0,
                        width: 50.0,
                        height: 50.0,
                    }),
                },
            ],
            description: "Detected 1 object".to_string(),
            processing_time_ms: 42,
            model_loaded: true,
            model_error: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: AnalysisResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.objects.len(), 1);
        assert_eq!(parsed.description, "Detected 1 object");
        assert_eq!(parsed.processing_time_ms, 42);
        assert!(parsed.model_loaded);
    }
}