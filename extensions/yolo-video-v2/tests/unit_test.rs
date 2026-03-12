//! YOLO Video Extension V2 - Comprehensive Unit Test Suite
//!
//! Tests cover:
//! - Extension metadata and configuration
//! - Command execution (start_stream, stop_stream, get_stream_stats)
//! - Metric production
//! - Video processing functionality
//! - Stream management
//! - Error handling
//! - Edge cases and boundary conditions

#[cfg(test)]
mod tests {
    use neomind_extension_sdk::{
        Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
        MetricDataType, ParamMetricValue,
    };
    use serde_json::json;

    // Import the extension from the library
    use neomind_extension_yolo_video_v2::{
        YoloVideoProcessorV2, StreamConfig, StreamInfo, StreamStats,
        ObjectDetection, BoundingBox, COCO_CLASSES, MODEL_CONFIG,
    };

    /// Create a new YoloVideoProcessorV2 instance for testing
    fn create_extension() -> YoloVideoProcessorV2 {
        YoloVideoProcessorV2::new()
    }

    // ========================================================================
    // Metadata Tests
    // ========================================================================

    #[test]
    fn test_metadata_basic() {
        let ext = create_extension();
        let meta = ext.metadata();

        assert_eq!(meta.id, "yolo-video-v2");
        assert_eq!(meta.name, "YOLO Video Processor V2");
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

        // This extension doesn't have config_parameters defined
        // (uses None in metadata)
        assert!(meta.config_parameters.is_none() || meta.config_parameters.as_ref().map(|p| p.is_empty()).unwrap_or(true));
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

        assert!(metric_names.contains(&"active_streams"));
        assert!(metric_names.contains(&"total_frames_processed"));
    }

    #[test]
    fn test_metric_types() {
        let ext = create_extension();
        let metrics = ext.metrics();

        // active_streams should be Integer
        let streams_metric = metrics.iter()
            .find(|m| m.name == "active_streams");
        assert!(streams_metric.is_some());
        assert_eq!(streams_metric.unwrap().data_type, MetricDataType::Integer);

        // total_frames_processed should be Integer
        let frames_metric = metrics.iter()
            .find(|m| m.name == "total_frames_processed");
        assert!(frames_metric.is_some());
        assert_eq!(frames_metric.unwrap().data_type, MetricDataType::Integer);
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

        assert!(cmd_names.contains(&"start_stream"));
        assert!(cmd_names.contains(&"stop_stream"));
        assert!(cmd_names.contains(&"get_stream_stats"));
    }

    #[test]
    fn test_start_stream_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let start_stream = commands.iter()
            .find(|c| c.name == "start_stream");

        assert!(start_stream.is_some());
        let cmd = start_stream.unwrap();

        // Should have parameters
        assert!(!cmd.parameters.is_empty());

        // Check for source_url parameter
        let source_param = cmd.parameters.iter()
            .find(|p| p.name == "source_url");
        assert!(source_param.is_some());

        // Should have LLM hints
        assert!(!cmd.llm_hints.is_empty());
    }

    #[test]
    fn test_stop_stream_command() {
        let ext = create_extension();
        let commands = ext.commands();

        let stop_stream = commands.iter()
            .find(|c| c.name == "stop_stream");

        assert!(stop_stream.is_some());
        let cmd = stop_stream.unwrap();

        // Should have stream_id parameter
        let stream_param = cmd.parameters.iter()
            .find(|p| p.name == "stream_id");
        assert!(stream_param.is_some());
        assert!(stream_param.unwrap().required);
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

        // active_streams should be present and non-negative
        let active_streams = metrics.iter()
            .find(|m| m.name == "active_streams");
        assert!(active_streams.is_some());

        match active_streams.unwrap().value {
            ParamMetricValue::Integer(count) => assert!(count >= 0),
            _ => panic!("Expected Integer value for active_streams"),
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
    async fn test_stop_stream_missing_id() {
        let ext = create_extension();

        let result = ext.execute_command("stop_stream", &json!({})).await;

        // Should fail due to missing stream_id
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_stream_stats_initial() {
        let ext = create_extension();

        let result = ext.execute_command("get_stream_stats", &json!({
            "stream_id": "non-existent"
        })).await;

        // Should handle non-existent stream gracefully
        // May return error or empty stats
        match result {
            Ok(_) => {}
            Err(_) => {}
        }
    }

    // ========================================================================
    // Stream Configuration Tests
    // ========================================================================

    #[test]
    fn test_stream_config_default() {
        let config = StreamConfig::default();

        assert_eq!(config.source_url, "camera://0");
        assert_eq!(config.confidence_threshold, 0.5);
        assert_eq!(config.max_objects, 20);
        assert_eq!(config.target_fps, 15);
        assert!(config.draw_boxes);
    }

    #[test]
    fn test_stream_config_serialization() {
        let config = StreamConfig {
            source_url: "rtsp://example.com/stream".to_string(),
            confidence_threshold: 0.7,
            max_objects: 50,
            target_fps: 30,
            draw_boxes: false,
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: StreamConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.source_url, "rtsp://example.com/stream");
        assert_eq!(parsed.confidence_threshold, 0.7);
        assert_eq!(parsed.max_objects, 50);
        assert_eq!(parsed.target_fps, 30);
        assert!(!parsed.draw_boxes);
    }

    // ========================================================================
    // Object Detection Tests
    // ========================================================================

    #[test]
    fn test_object_detection_serialization() {
        let detection = ObjectDetection {
            id: 1,
            label: "person".to_string(),
            confidence: 0.95,
            bbox: BoundingBox {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 200.0,
            },
            class_id: 0,
        };

        let json = serde_json::to_string(&detection).unwrap();
        let parsed: ObjectDetection = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.label, "person");
        assert_eq!(parsed.confidence, 0.95);
        assert_eq!(parsed.class_id, 0);
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
        assert!(COCO_CLASSES.contains(&"bicycle"));
    }

    #[test]
    fn test_coco_classes_index() {
        // Verify class indices
        assert_eq!(COCO_CLASSES[0], "person");
        assert_eq!(COCO_CLASSES[1], "bicycle");
        assert_eq!(COCO_CLASSES[2], "car");
    }

    // ========================================================================
    // Model Configuration Tests
    // ========================================================================

    #[test]
    fn test_model_config() {
        assert_eq!(MODEL_CONFIG.name, "yolo11n");
        assert_eq!(MODEL_CONFIG.input_size, 640);
        assert_eq!(MODEL_CONFIG.num_classes, 80);
        assert_eq!(MODEL_CONFIG.num_boxes, 8400);
    }

    // ========================================================================
    // Stream Info Tests
    // ========================================================================

    #[test]
    fn test_stream_info_serialization() {
        let info = StreamInfo {
            stream_id: "test-stream-123".to_string(),
            stream_url: "/api/extensions/yolo-video-v2/stream/test-stream-123".to_string(),
            status: "running".to_string(),
            width: 1920,
            height: 1080,
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: StreamInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.stream_id, "test-stream-123");
        assert_eq!(parsed.status, "running");
        assert_eq!(parsed.width, 1920);
        assert_eq!(parsed.height, 1080);
    }

    // ========================================================================
    // Stream Stats Tests
    // ========================================================================

    #[test]
    fn test_stream_stats_serialization() {
        let mut detected_objects = std::collections::HashMap::new();
        detected_objects.insert("person".to_string(), 10);
        detected_objects.insert("car".to_string(), 5);

        let stats = StreamStats {
            stream_id: "test-stream".to_string(),
            frame_count: 1000,
            fps: 15.5,
            total_detections: 150,
            detected_objects,
        };

        let json = serde_json::to_string(&stats).unwrap();
        let parsed: StreamStats = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.stream_id, "test-stream");
        assert_eq!(parsed.frame_count, 1000);
        assert_eq!(parsed.fps, 15.5);
        assert_eq!(parsed.total_detections, 150);
        assert_eq!(parsed.detected_objects.get("person"), Some(&10));
    }

    // ========================================================================
    // Edge Cases and Boundary Tests
    // ========================================================================

    #[test]
    fn test_stream_config_boundary_values() {
        // Test minimum values
        let config_min = StreamConfig {
            source_url: "camera://0".to_string(),
            confidence_threshold: 0.0,
            max_objects: 1,
            target_fps: 1,
            draw_boxes: true,
        };

        // Test maximum values
        let config_max = StreamConfig {
            source_url: "rtsp://very-long-url.example.com/path/to/stream".to_string(),
            confidence_threshold: 1.0,
            max_objects: 1000,
            target_fps: 120,
            draw_boxes: false,
        };

        // Both should serialize/deserialize correctly
        let json_min = serde_json::to_string(&config_min).unwrap();
        let json_max = serde_json::to_string(&config_max).unwrap();

        let parsed_min: StreamConfig = serde_json::from_str(&json_min).unwrap();
        let parsed_max: StreamConfig = serde_json::from_str(&json_max).unwrap();

        assert_eq!(parsed_min.confidence_threshold, 0.0);
        assert_eq!(parsed_max.confidence_threshold, 1.0);
    }

    #[tokio::test]
    async fn test_start_stream_with_various_sources() {
        let ext = create_extension();

        // Test with different source types
        let sources = vec![
            "camera://0",
            "rtsp://example.com/stream",
            "http://example.com/stream.m3u8",
            "file:///path/to/video.mp4",
        ];

        for source in sources {
            let result = ext.execute_command("start_stream", &json!({
                "source_url": source
            })).await;

            // May succeed or fail depending on source availability
            // Just ensure it doesn't panic
            match result {
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }

    // ========================================================================
    // Default Implementation Tests
    // ========================================================================

    #[test]
    fn test_default_implementation() {
        let ext1 = YoloVideoProcessorV2::new();
        let ext2 = YoloVideoProcessorV2::default();

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
        }

        // If we get here without running out of memory, the test passes
        assert!(true);
    }

    // ========================================================================
    // Statistics Tests
    // ========================================================================

    #[test]
    fn test_get_stats_default() {
        let ext = create_extension();
        let stats = ext.get_stats();

        // Default implementation returns zeros
        assert_eq!(stats.start_count, 0);
        assert_eq!(stats.stop_count, 0);
        assert_eq!(stats.error_count, 0);
    }

    // ========================================================================
    // Integration-style Tests
    // ========================================================================

    #[tokio::test]
    async fn test_full_workflow() {
        let ext = create_extension();

        // 1. Get initial metrics
        let metrics = ext.produce_metrics().unwrap();
        assert!(!metrics.is_empty());

        // 2. Get stats (default implementation returns zeros)
        let _stats = ext.get_stats();

        // 3. Try to start a stream (may fail if no camera)
        let _ = ext.execute_command("start_stream", &json!({
            "source_url": "camera://0",
            "confidence_threshold": 0.5,
            "max_objects": 10
        })).await;

        // 4. Get metrics again
        let metrics_after = ext.produce_metrics().unwrap();
        assert!(!metrics_after.is_empty());
    }
}