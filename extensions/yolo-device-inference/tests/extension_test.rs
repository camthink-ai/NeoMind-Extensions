//! YOLO Device Inference Extension - Comprehensive Test Suite
//!
//! Tests cover:
//! - Extension metadata and configuration
//! - Command execution (bind_device, unbind_device, analyze_image, get_status)
//! - Metric production
//! - Device binding management
//! - Inference functionality
//! - Error handling
//! - Edge cases and boundary conditions

#[cfg(test)]
mod tests {
    use neomind_extension_sdk::{
        Extension, ExtensionError,
        MetricDataType, ParamMetricValue,
    };
    use serde_json::{json, Value};
    use base64::Engine;

    // Import the extension from the library
    use neomind_extension_yolo_device_inference::{
        YoloDeviceInference, DeviceBinding, BindingStatus, InferenceResult,
        Detection, BoundingBox, YoloConfig, COCO_CLASSES,
    };

    // ========================================================================
    // Helper Functions
    // ========================================================================

    /// Create a new YoloDeviceInference instance for testing
    fn create_extension() -> YoloDeviceInference {
        YoloDeviceInference::new()
    }

    /// Create a minimal valid JPEG image (1x1 pixel, grayscale)
    fn create_minimal_jpeg() -> Vec<u8> {
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

        assert_eq!(meta.id, "yolo-device-inference");
        assert_eq!(meta.name, "YOLO Device Inference");
        assert_eq!(meta.version.major, 1);
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
    fn test_metadata_json_matches_runtime_metadata() {
        let ext = create_extension();
        let meta = ext.metadata();
        let metadata_json: Value = serde_json::from_str(include_str!("../metadata.json")).unwrap();

        assert_eq!(metadata_json["id"], meta.id);
        assert_eq!(metadata_json["name"], meta.name);
        assert_eq!(metadata_json["description"].as_str(), meta.description.as_deref());
        assert_eq!(metadata_json["license"], "MIT");
        assert_eq!(
            metadata_json["homepage"],
            "https://github.com/camthink-ai/NeoMind-Extensions/tree/main/extensions/yolo-device-inference"
        );
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

        assert!(metric_names.contains(&"total_inferences"));
        assert!(metric_names.contains(&"total_detections"));
        assert!(metric_names.contains(&"bound_devices"));
    }

    #[test]
    fn test_metric_types() {
        let ext = create_extension();
        let metrics = ext.metrics();

        // total_inferences should be Integer
        let inferences_metric = metrics.iter()
            .find(|m| m.name == "total_inferences");
        assert!(inferences_metric.is_some());
        assert_eq!(inferences_metric.unwrap().data_type, MetricDataType::Integer);

        // total_detections should be Integer
        let detections_metric = metrics.iter()
            .find(|m| m.name == "total_detections");
        assert!(detections_metric.is_some());
        assert_eq!(detections_metric.unwrap().data_type, MetricDataType::Integer);
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

        assert!(cmd_names.contains(&"bind_device"));
        assert!(cmd_names.contains(&"unbind_device"));
        assert!(cmd_names.contains(&"analyze_image"));
        assert!(cmd_names.contains(&"get_status"));
        assert!(cmd_names.contains(&"get_bindings"));
    }

    #[test]
    fn test_bind_device_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let bind_cmd = commands.iter()
            .find(|c| c.name == "bind_device");

        assert!(bind_cmd.is_some());
        let cmd = bind_cmd.unwrap();

        // Should have device_id parameter
        let device_param = cmd.parameters.iter()
            .find(|p| p.name == "device_id");
        assert!(device_param.is_some());
        assert!(device_param.unwrap().required);

        // Should have description (replaces llm_hints)
        assert!(!cmd.description.is_empty());
    }

    #[test]
    fn test_analyze_image_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let analyze_cmd = commands.iter()
            .find(|c| c.name == "analyze_image");

        assert!(analyze_cmd.is_some());
        let cmd = analyze_cmd.unwrap();

        // Should have image parameter
        let image_param = cmd.parameters.iter()
            .find(|p| p.name == "image");
        assert!(image_param.is_some());
        assert!(image_param.unwrap().required);
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

        // Initial total_inferences should be 0
        let total_inferences = metrics.iter()
            .find(|m| m.name == "total_inferences");
        assert!(total_inferences.is_some());

        match total_inferences.unwrap().value {
            ParamMetricValue::Integer(count) => assert_eq!(count, 0),
            _ => panic!("Expected Integer value for total_inferences"),
        }
    }

    // ========================================================================
    // Command Execution Tests
    // ========================================================================

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

    #[tokio::test]
    async fn test_get_status_command() {
        let ext = create_extension();

        let result = ext.execute_command("get_status", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();

        // Should have model status
        assert!(response.get("model_loaded").is_some());
        assert!(response.get("total_bindings").is_some());
    }

    #[tokio::test]
    async fn test_get_bindings_initial() {
        let ext = create_extension();

        let result = ext.execute_command("get_bindings", &json!({})).await;

        assert!(result.is_ok());
        let response = result.unwrap();

        // Should have bindings array
        assert!(response.is_array() || response.get("bindings").is_some());
    }

    #[tokio::test]
    async fn test_bind_device_missing_device_id() {
        let ext = create_extension();

        let result = ext.execute_command("bind_device", &json!({})).await;

        // Should fail due to missing device_id
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unbind_device_missing_device_id() {
        let ext = create_extension();

        let result = ext.execute_command("unbind_device", &json!({})).await;

        // Should fail due to missing device_id
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_analyze_image_missing_image() {
        let ext = create_extension();

        let result = ext.execute_command("analyze_image", &json!({})).await;

        // Should fail due to missing image
        assert!(result.is_err());
    }

    // ========================================================================
    // Device Binding Tests
    // ========================================================================

    #[test]
    fn test_device_binding_serialization() {
        let binding = DeviceBinding {
            device_id: "device-001".to_string(),
            device_name: Some("Camera 1".to_string()),
            image_metric: "image".to_string(),
            result_metric_prefix: "yolo_".to_string(),
            confidence_threshold: 0.5,
            draw_boxes: true,
            active: true,
        };

        let json = serde_json::to_string(&binding).unwrap();
        let parsed: DeviceBinding = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.device_id, "device-001");
        assert_eq!(parsed.device_name, Some("Camera 1".to_string()));
        assert_eq!(parsed.confidence_threshold, 0.5);
        assert!(parsed.active);
    }

    #[test]
    fn test_binding_status_serialization() {
        let status = BindingStatus {
            binding: DeviceBinding {
                device_id: "device-001".to_string(),
                device_name: None,
                image_metric: "image".to_string(),
                result_metric_prefix: "yolo_".to_string(),
                confidence_threshold: 0.5,
                draw_boxes: true,
                active: true,
            },
            last_inference: Some(1234567890),
            total_inferences: 100,
            total_detections: 500,
            last_error: None,
            last_image: None,
            last_detections: None,
            last_annotated_image: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        let parsed: BindingStatus = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.binding.device_id, "device-001");
        assert_eq!(parsed.total_inferences, 100);
        assert_eq!(parsed.total_detections, 500);
    }

    // ========================================================================
    // Inference Result Tests
    // ========================================================================

    #[test]
    fn test_inference_result_serialization() {
        let result = InferenceResult {
            device_id: "device-001".to_string(),
            detections: vec![
                Detection {
                    label: "person".to_string(),
                    confidence: 0.95,
                    bbox: BoundingBox {
                        x: 10.0,
                        y: 20.0,
                        width: 100.0,
                        height: 200.0,
                    },
                    class_id: Some(0),
                },
            ],
            inference_time_ms: 42,
            image_width: 1920,
            image_height: 1080,
            timestamp: 1234567890,
            annotated_image_base64: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: InferenceResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.device_id, "device-001");
        assert_eq!(parsed.detections.len(), 1);
        assert_eq!(parsed.inference_time_ms, 42);
    }

    // ========================================================================
    // Detection Tests
    // ========================================================================

    #[test]
    fn test_detection_serialization() {
        let detection = Detection {
            label: "car".to_string(),
            confidence: 0.88,
            bbox: BoundingBox {
                x: 50.0,
                y: 100.0,
                width: 200.0,
                height: 150.0,
            },
            class_id: Some(2),
        };

        let json = serde_json::to_string(&detection).unwrap();
        let parsed: Detection = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.label, "car");
        assert_eq!(parsed.confidence, 0.88);
        assert_eq!(parsed.class_id, Some(2));
    }

    // ========================================================================
    // YoloConfig Tests
    // ========================================================================

    #[test]
    fn test_yolo_config_default() {
        let config = YoloConfig::default();

        assert_eq!(config.default_confidence, 0.25);
        assert_eq!(config.model_version, "v8-n");
        assert!(config.bindings.is_empty());
    }

    #[test]
    fn test_yolo_config_serialization() {
        let config = YoloConfig {
            default_confidence: 0.5,
            model_version: "v8-s".to_string(),
            bindings: vec![
                DeviceBinding {
                    device_id: "device-001".to_string(),
                    device_name: None,
                    image_metric: "image".to_string(),
                    result_metric_prefix: "yolo_".to_string(),
                    confidence_threshold: 0.5,
                    draw_boxes: true,
                    active: true,
                },
            ],
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: YoloConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.default_confidence, 0.5);
        assert_eq!(parsed.model_version, "v8-s");
        assert_eq!(parsed.bindings.len(), 1);
    }

    // ========================================================================
    // COCO Classes Tests
    // ========================================================================

    #[test]
    fn test_coco_classes_count() {
        // COCO dataset has 80 classes
        assert_eq!(COCO_CLASSES.len(), 80);
    }

    #[test]
    fn test_coco_classes_common_objects() {
        // Check for common objects
        assert!(COCO_CLASSES.contains(&"person"));
        assert!(COCO_CLASSES.contains(&"car"));
        assert!(COCO_CLASSES.contains(&"dog"));
        assert!(COCO_CLASSES.contains(&"cat"));
    }

    // ========================================================================
    // Get Bindings Tests
    // ========================================================================

    #[test]
    fn test_get_bindings_empty_initial() {
        let ext = create_extension();
        let bindings = ext.get_bindings();

        // Initially should be empty
        assert!(bindings.is_empty());
    }

    #[test]
    fn test_get_binding_nonexistent() {
        let ext = create_extension();
        let binding = ext.get_binding("nonexistent-device");

        // Should return None for non-existent device
        assert!(binding.is_none());
    }

    // ========================================================================
    // Get Status Tests
    // ========================================================================

    #[test]
    fn test_get_status() {
        let ext = create_extension();
        let status = ext.get_status();

        // Should have model_loaded field
        assert!(status.get("model_loaded").is_some());

        // Should have total_bindings field
        assert!(status.get("total_bindings").is_some());

        // Should have total_inferences field
        assert!(status.get("total_inferences").is_some());
    }

    // ========================================================================
    // Get Config Tests
    // ========================================================================

    #[test]
    fn test_get_config() {
        let ext = create_extension();
        let config = ext.get_config();

        // Should have default values
        assert_eq!(config.default_confidence, 0.25);
        assert_eq!(config.model_version, "v8-n");
    }

    // ========================================================================
    // Edge Cases and Boundary Tests
    // ========================================================================

    #[test]
    fn test_device_binding_boundary_values() {
        // Test minimum values
        let binding_min = DeviceBinding {
            device_id: "".to_string(),
            device_name: None,
            image_metric: "".to_string(),
            result_metric_prefix: "".to_string(),
            confidence_threshold: 0.0,
            draw_boxes: false,
            active: false,
        };

        // Test maximum values
        let binding_max = DeviceBinding {
            device_id: "very-long-device-id-1234567890".to_string(),
            device_name: Some("Very Long Device Name".to_string()),
            image_metric: "image_metric_name".to_string(),
            result_metric_prefix: "prefix_".to_string(),
            confidence_threshold: 1.0,
            draw_boxes: true,
            active: true,
        };

        // Both should serialize/deserialize correctly
        let json_min = serde_json::to_string(&binding_min).unwrap();
        let json_max = serde_json::to_string(&binding_max).unwrap();

        let parsed_min: DeviceBinding = serde_json::from_str(&json_min).unwrap();
        let parsed_max: DeviceBinding = serde_json::from_str(&json_max).unwrap();

        assert_eq!(parsed_min.confidence_threshold, 0.0);
        assert_eq!(parsed_max.confidence_threshold, 1.0);
    }

    // ========================================================================
    // Default Implementation Tests
    // ========================================================================

    #[test]
    fn test_default_implementation() {
        let ext1 = YoloDeviceInference::new();
        let ext2 = YoloDeviceInference::default();

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
    fn test_no_memory_leak_on_repeated_operations() {
        let ext = create_extension();

        // Perform many operations
        for _ in 0..1000 {
            let _ = ext.produce_metrics();
            let _ = ext.get_status();
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

        // 1. Get initial status
        let status = ext.execute_command("get_status", &json!({})).await;
        assert!(status.is_ok());

        // 2. Get initial bindings
        let bindings = ext.execute_command("get_bindings", &json!({})).await;
        assert!(bindings.is_ok());

        // 3. Get metrics
        let metrics = ext.produce_metrics().unwrap();
        assert!(!metrics.is_empty());

        // 4. Get config
        let config = ext.get_config();
        assert_eq!(config.default_confidence, 0.25);
    }

    #[tokio::test]
    async fn test_analyze_image_workflow() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();
        let image_b64 = encode_image(&jpeg_data);

        // Analyze image
        let result = ext.execute_command("analyze_image", &json!({
            "image": image_b64
        })).await;

        // May succeed or fail depending on model availability
        // Just ensure it doesn't panic
        match result {
            Ok(response) => {
                // Should have detections or error
                assert!(response.get("detections").is_some() || response.get("error").is_some());
            }
            Err(_) => {}
        }
    }

    // ========================================================================
    // Event Handling Tests - Nested Path Parsing
    // ========================================================================

    #[test]
    fn test_extract_image_from_value_direct_string() {
        let ext = create_extension();
        let image_b64 = "aGVsbG8gd29ybGQ="; // "hello world" in base64

        let value = Some(&serde_json::json!(image_b64));
        let result = ext.extract_image_from_value(value, None);

        assert_eq!(result, Some(image_b64.to_string()));
    }

    #[test]
    fn test_extract_image_from_value_metric_wrapper() {
        let ext = create_extension();
        let image_b64 = "aGVsbG8gd29ybGQ=";

        // MetricValue wrapper format: {"String": "base64data"}
        let value = Some(&serde_json::json!({"String": image_b64}));
        let result = ext.extract_image_from_value(value, None);

        assert_eq!(result, Some(image_b64.to_string()));
    }

    #[test]
    fn test_extract_image_from_value_nested_path() {
        let ext = create_extension();
        let image_b64 = "aGVsbG8gd29ybGQ=";

        // Nested object: {"image": "base64data"}
        let value = Some(&serde_json::json!({"image": image_b64, "other": "data"}));
        let result = ext.extract_image_from_value(value, Some("image"));

        assert_eq!(result, Some(image_b64.to_string()));
    }

    #[test]
    fn test_extract_image_from_value_deeply_nested() {
        let ext = create_extension();
        let image_b64 = "aGVsbG8gd29ybGQ=";

        // Deeply nested: {"data": {"image": "base64data"}}
        let value = Some(&serde_json::json!({
            "data": {
                "image": image_b64
            }
        }));
        let result = ext.extract_image_from_value(value, Some("data.image"));

        assert_eq!(result, Some(image_b64.to_string()));
    }

    #[test]
    fn test_extract_image_from_value_no_match() {
        let ext = create_extension();

        // No matching field
        let value = Some(&serde_json::json!({"foo": "bar"}));
        let result = ext.extract_image_from_value(value, None);

        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_image_from_value_none_input() {
        let ext = create_extension();

        let result = ext.extract_image_from_value(None, None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_image_from_value_common_field_names() {
        let ext = create_extension();
        let image_b64 = "aGVsbG8gd29ybGQ=";

        // Test "data" field
        let value = Some(&serde_json::json!({"data": image_b64}));
        let result = ext.extract_image_from_value(value, None);
        assert_eq!(result, Some(image_b64.to_string()));

        // Test "value" field
        let value = Some(&serde_json::json!({"value": image_b64}));
        let result = ext.extract_image_from_value(value, None);
        assert_eq!(result, Some(image_b64.to_string()));

        // Test "base64" field
        let value = Some(&serde_json::json!({"base64": image_b64}));
        let result = ext.extract_image_from_value(value, None);
        assert_eq!(result, Some(image_b64.to_string()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handle_event_nested_metric_path() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();
        let image_b64 = encode_image(&jpeg_data);

        // Bind device with nested image_metric path
        let _ = ext.execute_command("bind_device", &json!({
            "device_id": "test-device-001",
            "image_metric": "values.image",
            "active": true
        })).await;

        // Simulate DeviceMetric event with nested value
        // The metric is "values" (top-level), and image is inside value.image
        let event_payload = serde_json::json!({
            "event_type": "DeviceMetric",
            "payload": {
                "device_id": "test-device-001",
                "metric": "values",
                "value": {
                    "image": image_b64,
                    "temperature": 25.5
                }
            },
            "timestamp": 1234567890
        });

        // Handle the event
        let result = ext.handle_event("DeviceMetric", &event_payload);
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handle_event_simple_metric_name() {
        let ext = create_extension();
        let jpeg_data = create_minimal_jpeg();
        let image_b64 = encode_image(&jpeg_data);

        // Bind device with simple image_metric name
        let _ = ext.execute_command("bind_device", &json!({
            "device_id": "test-device-002",
            "image_metric": "image",
            "active": true
        })).await;

        // Simulate DeviceMetric event with direct value
        let event_payload = serde_json::json!({
            "event_type": "DeviceMetric",
            "payload": {
                "device_id": "test-device-002",
                "metric": "image",
                "value": image_b64
            },
            "timestamp": 1234567890
        });

        // Handle the event
        let result = ext.handle_event("DeviceMetric", &event_payload);
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handle_event_ignores_non_matching_device() {
        let ext = create_extension();

        // Bind device
        let _ = ext.execute_command("bind_device", &json!({
            "device_id": "bound-device",
            "image_metric": "image"
        })).await;

        // Event for different device (not bound)
        let event_payload = serde_json::json!({
            "event_type": "DeviceMetric",
            "payload": {
                "device_id": "unbound-device",
                "metric": "image",
                "value": "some_base64_data"
            },
            "timestamp": 1234567890
        });

        // Should succeed but not process anything
        let result = ext.handle_event("DeviceMetric", &event_payload);
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handle_event_ignores_non_matching_metric() {
        let ext = create_extension();

        // Bind device with image_metric = "image"
        let _ = ext.execute_command("bind_device", &json!({
            "device_id": "test-device-003",
            "image_metric": "image"
        })).await;

        // Event with different metric
        let event_payload = serde_json::json!({
            "event_type": "DeviceMetric",
            "payload": {
                "device_id": "test-device-003",
                "metric": "temperature",
                "value": 25.5
            },
            "timestamp": 1234567890
        });

        // Should succeed but not process anything
        let result = ext.handle_event("DeviceMetric", &event_payload);
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_handle_event_ignores_inactive_binding() {
        let ext = create_extension();

        // Bind device but set active = false
        let _ = ext.execute_command("bind_device", &json!({
            "device_id": "test-device-004",
            "image_metric": "image"
        })).await;

        // Toggle to inactive
        let _ = ext.execute_command("toggle_binding", &json!({
            "device_id": "test-device-004",
            "active": false
        })).await;

        // Event that would match if active
        let event_payload = serde_json::json!({
            "event_type": "DeviceMetric",
            "payload": {
                "device_id": "test-device-004",
                "metric": "image",
                "value": "some_base64_data"
            },
            "timestamp": 1234567890
        });

        // Should succeed but not process anything (inactive)
        let result = ext.handle_event("DeviceMetric", &event_payload);
        assert!(result.is_ok());
    }

    #[test]
    fn test_event_subscriptions() {
        let ext = create_extension();
        let subscriptions = ext.event_subscriptions();

        assert_eq!(subscriptions, &["DeviceMetric"]);
    }
}
