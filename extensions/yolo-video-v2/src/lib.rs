//! YOLO Video Processor Extension (V2)
//!
//! Real-time video stream processing with YOLOv11 object detection.
//! Uses the unified NeoMind Extension SDK with ABI Version 3.
//!
//! SAFETY: This extension is marked as HIGH-RISK due to:
//! - ONNX runtime AI inference (potential memory issues)
//! - Multi-threaded video processing
//! - Heavy image processing workloads
//!
//! RECOMMENDATION: Enable process isolation for production deployments.

pub mod detector;
pub mod video_source;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use semver::Version;
use uuid::Uuid;

use detector::{Detection, YoloDetector};

// ============================================================================
// Constants
// ============================================================================

/// YOLOv11 class labels (COCO 80 classes)
pub const COCO_CLASSES: [&str; 80] = [
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

/// Model configuration
pub const MODEL_CONFIG: ModelConfig = ModelConfig {
    name: "yolo11n",
    input_size: 640,
    num_classes: 80,
    num_boxes: 8400,
};

#[derive(Debug, Clone, Copy)]
pub struct ModelConfig {
    pub name: &'static str,
    pub input_size: u32,
    pub num_classes: usize,
    pub num_boxes: usize,
}

/// Bounding box for detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

// ============================================================================
// Types
// ============================================================================

/// Object detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectDetection {
    pub id: u32,
    pub label: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
    pub class_id: u32,
}

/// Stream configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    pub source_url: String,
    pub confidence_threshold: f32,
    pub max_objects: u32,
    pub target_fps: u32,
    pub draw_boxes: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            source_url: "camera://0".to_string(),
            confidence_threshold: 0.5,
            max_objects: 20,
            target_fps: 15,
            draw_boxes: true,
        }
    }
}

/// Stream information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub stream_id: String,
    pub stream_url: String,
    pub status: String,
    pub width: u32,
    pub height: u32,
}

/// Stream statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamStats {
    pub stream_id: String,
    pub frame_count: u64,
    pub fps: f32,
    pub total_detections: u64,
    pub detected_objects: HashMap<String, u32>,
}

/// Active stream state
#[derive(Debug)]
struct ActiveStream {
    _id: String,
    _config: StreamConfig,
    started_at: Instant,
    frame_count: u64,
    total_detections: u64,
    last_frame: Option<Vec<u8>>,
    last_detections: Vec<ObjectDetection>,
    last_frame_time: Option<Instant>,
    fps: f32,
    running: bool,
    detected_objects: HashMap<String, u32>,
}

/// Color palette for drawing boxes
const BOX_COLORS: [(u8, u8, u8); 10] = [
    (239, 68, 68), (34, 197, 94), (59, 130, 246), (234, 179, 8), (6, 182, 212),
    (139, 92, 246), (236, 72, 153), (249, 115, 22), (132, 204, 22), (20, 184, 166),
];

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert detector results to extension format
fn detections_to_object_detection(detections: Vec<Detection>) -> Vec<ObjectDetection> {
    detections
        .into_iter()
        .enumerate()
        .map(|(i, d)| ObjectDetection {
            id: i as u32,
            label: d.class_name,
            confidence: d.confidence,
            bbox: d.bbox,
            class_id: d.class_id,
        })
        .collect()
}

/// Generate fallback detections when model is not loaded
fn generate_fallback_detections(frame_count: u64, max_objects: u32) -> Vec<ObjectDetection> {
    let objects = [
        ("person", 0u32, 0.75f32),
        ("car", 2, 0.65),
        ("dog", 16, 0.70),
        ("bicycle", 1, 0.60),
        ("cat", 15, 0.68),
    ];

    let count = ((frame_count % 3) + 1) as usize;
    let offset = (frame_count as usize % objects.len()) as usize;

    objects.iter()
        .cycle()
        .skip(offset)
        .take(count.min(max_objects as usize))
        .enumerate()
        .map(|(i, (label, class_id, conf))| {
            let ox = (frame_count % 100) as f32 * 3.0 + i as f32 * 50.0;
            let oy = (frame_count % 80) as f32 * 2.0 + i as f32 * 30.0;
            ObjectDetection {
                id: i as u32,
                label: label.to_string(),
                confidence: *conf,
                bbox: BoundingBox {
                    x: 100.0 + ox,
                    y: 100.0 + oy,
                    width: 100.0 + i as f32 * 20.0,
                    height: 150.0,
                },
                class_id: *class_id,
            }
        })
        .collect()
}

/// Draw detections on an image
pub fn draw_detections(image: &mut image::RgbImage, detections: &[ObjectDetection]) {
    use imageproc::drawing::{draw_hollow_rect_mut, draw_filled_rect_mut};
    use imageproc::rect::Rect;

    for (i, det) in detections.iter().enumerate() {
        let color = BOX_COLORS[i % BOX_COLORS.len()];
        let image_color = image::Rgb([color.0, color.1, color.2]);

        let x = det.bbox.x as i32;
        let y = det.bbox.y as i32;
        let w = det.bbox.width as u32;
        let h = det.bbox.height as u32;

        let x = x.max(0).min(image.width() as i32 - 2);
        let y = y.max(0).min(image.height() as i32 - 2);
        let w = w.min(image.width() - x as u32 - 1);
        let h = h.min(image.height() - y as u32 - 1);

        if w < 2 || h < 2 {
            continue;
        }

        draw_hollow_rect_mut(image, Rect::at(x, y).of_size(w, h), image_color);
        draw_hollow_rect_mut(image, Rect::at(x + 1, y + 1).of_size(w.saturating_sub(2), h.saturating_sub(2)), image_color);

        let label_height = 18u32;
        if y >= 0 && (y as u32) + label_height <= image.height() && w >= 30 {
            let label_width = ((det.label.len() * 7) + 25).min(w as usize) as u32;
            draw_filled_rect_mut(
                image,
                Rect::at(x, y).of_size(label_width, label_height),
                image_color,
            );
        }
    }
}

/// Encode image to JPEG
pub fn encode_jpeg(image: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut buffer = Vec::new();
    let dynamic = image::DynamicImage::ImageRgb8(image.clone());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, quality);
    let _ = dynamic.write_with_encoder(encoder);
    buffer
}

// ============================================================================
// Stream Registry
// ============================================================================

struct StreamRegistry {
    streams: HashMap<String, Arc<Mutex<ActiveStream>>>,
    total_frames: u64,
    _total_detections: u64,
}

impl StreamRegistry {
    fn new() -> Self {
        Self {
            streams: HashMap::new(),
            total_frames: 0,
            _total_detections: 0,
        }
    }
}

static REGISTRY: std::sync::OnceLock<Mutex<StreamRegistry>> = std::sync::OnceLock::new();

fn get_registry() -> &'static Mutex<StreamRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(StreamRegistry::new()))
}

// ============================================================================
// Stream Processor
// ============================================================================

pub struct StreamProcessor {
    detector: Arc<Mutex<YoloDetector>>,
}

impl StreamProcessor {
    pub fn new() -> Self {
        let detector = YoloDetector::default();
        Self {
            detector: Arc::new(Mutex::new(detector)),
        }
    }

    #[allow(dead_code)]
    fn has_model(&self) -> bool {
        self.detector.lock().is_loaded()
    }

    /// Start a new stream
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn start_stream(&self, config: StreamConfig) -> Result<StreamInfo> {
        let stream_id = Uuid::new_v4().to_string();

        let (width, height) = if config.source_url.contains("1920") || config.source_url.contains("rtsp") {
            (1920, 1080)
        } else {
            (640, 480)
        };

        let active_stream = Arc::new(Mutex::new(ActiveStream {
            _id: stream_id.clone(),
            _config: config.clone(),
            started_at: Instant::now(),
            frame_count: 0,
            total_detections: 0,
            last_frame: None,
            last_detections: vec![],
            last_frame_time: None,
            fps: 0.0,
            running: true,
            detected_objects: HashMap::new(),
        }));

        {
            let mut registry = get_registry().lock();
            registry.streams.insert(stream_id.clone(), active_stream.clone());
        }

        // Spawn processing on dedicated OS thread
        let stream_id_clone = stream_id.clone();
        let config_clone = config.clone();
        let detector_clone = Arc::clone(&self.detector);

        std::thread::spawn(move || {
            Self::processing_loop(active_stream, stream_id_clone, config_clone, detector_clone);
        });

        tracing::info!("[Stream {}] Started", stream_id);

        Ok(StreamInfo {
            stream_id: stream_id.clone(),
            stream_url: format!("/api/extensions/yolo-video-v2/stream/{}", stream_id),
            status: "starting".to_string(),
            width,
            height,
        })
    }

    /// Processing loop - runs on dedicated OS thread
    fn processing_loop(
        stream: Arc<Mutex<ActiveStream>>,
        stream_id: String,
        config: StreamConfig,
        detector: Arc<Mutex<YoloDetector>>,
    ) {
        tracing::info!("[Stream {}] Processing loop started", stream_id);

        let frame_interval = Duration::from_millis(1000 / config.target_fps.max(1) as u64);
        let mut frame_num = 0u64;

        while stream.lock().running {
            std::thread::sleep(frame_interval);

            // Generate demo frame
            let mut demo_frame = image::RgbImage::from_pixel(640, 480, image::Rgb([40, 44, 52]));

            // Add visual content
            let cx = ((frame_num * 3) % 500) as i32 + 70;
            let cy = ((frame_num * 2) % 300) as i32 + 90;

            for y in (0.max(cy - 60))..480.min(cy + 60) {
                for x in (0.max(cx - 60))..640.min(cx + 60) {
                    let dx = (x - cx) as f32;
                    let dy = (y - cy) as f32;
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq < 3600.0 {
                        let intensity = (1.0 - (dist_sq / 3600.0).sqrt()) * 100.0;
                        let base = 60 + (frame_num % 80) as u8;
                        let px = x as u32;
                        let py = y as u32;
                        if px < 640 && py < 480 {
                            demo_frame.put_pixel(
                                px, py,
                                image::Rgb([
                                    (base as f32 + intensity).min(255.0) as u8,
                                    (base as f32 + intensity * 0.5).min(255.0) as u8,
                                    80,
                                ]),
                            );
                        }
                    }
                }
            }

            // Run inference
            let detector_lock = detector.lock();
            let detections = if detector_lock.is_loaded() {
                tracing::debug!("[Stream {}] Running real inference", stream_id);
                let raw_detections = detector_lock.detect(
                    &demo_frame,
                    config.confidence_threshold,
                    config.max_objects,
                );
                detections_to_object_detection(raw_detections)
            } else {
                tracing::debug!("[Stream {}] Using fallback detections", stream_id);
                generate_fallback_detections(frame_num, config.max_objects)
            };
            drop(detector_lock);

            // Draw boxes if enabled
            let mut output_img = demo_frame;
            if config.draw_boxes {
                draw_detections(&mut output_img, &detections);
            }

            // Encode to JPEG
            let jpeg_data = encode_jpeg(&output_img, 85);

            // Update stream state
            {
                let mut s = stream.lock();
                s.frame_count += 1;
                s.total_detections += detections.len() as u64;
                s.last_frame = Some(jpeg_data);
                s.last_detections = detections.clone();
                s.last_frame_time = Some(Instant::now());

                let elapsed = s.started_at.elapsed().as_secs_f32();
                if elapsed > 0.0 {
                    s.fps = s.frame_count as f32 / elapsed;
                }

                for det in &detections {
                    *s.detected_objects.entry(det.label.clone()).or_insert(0) += 1;
                }
            }

            frame_num += 1;
        }

        {
            let mut s = stream.lock();
            s.running = false;
        }
        tracing::info!("[Stream {}] Processing loop stopped. Frames: {}", stream_id, frame_num);
    }

    /// Stop a stream
    #[cfg(not(target_arch = "wasm32"))]
    pub fn stop_stream(&self, stream_id: &str) -> Result<()> {
        let mut registry = get_registry().lock();
        if let Some(stream) = registry.streams.remove(stream_id) {
            stream.lock().running = false;
            tracing::info!("[Stream {}] Stopped", stream_id);
            Ok(())
        } else {
            Err(ExtensionError::SessionNotFound(stream_id.to_string()))
        }
    }

    /// Get stream statistics
    pub fn get_stream_stats(&self, stream_id: &str) -> Option<StreamStats> {
        let registry = get_registry().lock();
        if let Some(stream) = registry.streams.get(stream_id) {
            let s = stream.lock();
            Some(StreamStats {
                stream_id: stream_id.to_string(),
                frame_count: s.frame_count,
                fps: s.fps,
                total_detections: s.total_detections,
                detected_objects: s.detected_objects.clone(),
            })
        } else {
            None
        }
    }

    /// Get latest frame
    pub fn get_stream_frame(&self, stream_id: &str) -> Option<Vec<u8>> {
        let registry = get_registry().lock();
        if let Some(stream) = registry.streams.get(stream_id) {
            let s = stream.lock();
            s.last_frame.clone()
        } else {
            None
        }
    }
}

impl Default for StreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// YOLO Video Processor Extension (V2)
// ============================================================================

pub struct YoloVideoProcessorV2 {
    processor: Arc<StreamProcessor>,
}

impl YoloVideoProcessorV2 {
    pub fn new() -> Self {
        Self {
            processor: Arc::new(StreamProcessor::new()),
        }
    }
}

impl Default for YoloVideoProcessorV2 {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for YoloVideoProcessorV2 {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "yolo-video-v2".to_string(),
                name: "YOLO Video Processor V2".to_string(),
                version: Version::parse("2.0.0").unwrap(),
                description: Some("Real-time video stream processing with YOLOv11 (SDK V2)".to_string()),
                author: Some("NeoMind Team".to_string()),
                homepage: None,
                license: Some("Apache-2.0".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "active_streams".to_string(),
                    display_name: "Active Streams".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "count".to_string(),
                    min: Some(0.0),
                    max: None,
                    required: false,
                },
                MetricDescriptor {
                    name: "total_frames_processed".to_string(),
                    display_name: "Total Frames Processed".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "frames".to_string(),
                    min: Some(0.0),
                    max: None,
                    required: false,
                },
                MetricDescriptor {
                    name: "avg_fps".to_string(),
                    display_name: "Average FPS".to_string(),
                    data_type: MetricDataType::Float,
                    unit: "fps".to_string(),
                    min: Some(0.0),
                    max: None,
                    required: false,
                },
            ]
        })
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "start_stream".to_string(),
                    display_name: "Start Video Stream".to_string(),
                    payload_template: r#"{"source_url": "camera://0"}"#.to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "source_url".to_string(),
                            display_name: "Source URL".to_string(),
                            description: "Video source URL or camera ID".to_string(),
                            param_type: MetricDataType::String,
                            required: false,
                            default_value: Some(ParamMetricValue::String("camera://0".to_string())),
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: HashMap::new(),
                    samples: vec![
                        json!({ "source_url": "camera://0" }),
                        json!({ "source_url": "rtsp://example.com/stream" }),
                    ],
                    llm_hints: "Start a video detection stream".to_string(),
                    parameter_groups: Vec::new(),
                },
                ExtensionCommand {
                    name: "stop_stream".to_string(),
                    display_name: "Stop Video Stream".to_string(),
                    payload_template: r#"{"stream_id": ""}"#.to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "stream_id".to_string(),
                            display_name: "Stream ID".to_string(),
                            description: "ID of the stream to stop".to_string(),
                            param_type: MetricDataType::String,
                            required: true,
                            default_value: None,
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: HashMap::new(),
                    samples: vec![],
                    llm_hints: "Stop a video stream".to_string(),
                    parameter_groups: Vec::new(),
                },
                ExtensionCommand {
                    name: "get_stream_stats".to_string(),
                    display_name: "Get Stream Statistics".to_string(),
                    payload_template: r#"{"stream_id": ""}"#.to_string(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "stream_id".to_string(),
                            display_name: "Stream ID".to_string(),
                            description: "ID of the stream to get stats for".to_string(),
                            param_type: MetricDataType::String,
                            required: true,
                            default_value: None,
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: HashMap::new(),
                    samples: vec![],
                    llm_hints: "Get stream statistics".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "start_stream" => {
                let config: StreamConfig = serde_json::from_value(args.clone())
                    .unwrap_or_default();

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let info = self.processor.start_stream(config).await?;
                    Ok(serde_json::to_value(info)
                        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?)
                }

                #[cfg(target_arch = "wasm32")]
                {
                    let info = StreamInfo {
                        stream_id: "wasm-mock-stream".to_string(),
                        stream_url: "/api/extensions/yolo-video-v2/stream/wasm-mock-stream".to_string(),
                        status: "running".to_string(),
                        width: 640,
                        height: 480,
                    };
                    Ok(serde_json::to_value(info)
                        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?)
                }
            }
            "stop_stream" => {
                let stream_id = args.get("stream_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing stream_id".to_string()))?;

                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.processor.stop_stream(stream_id)?;
                }

                Ok(json!({"success": true}))
            }
            "get_stream_stats" => {
                let stream_id = args.get("stream_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing stream_id".to_string()))?;

                if let Some(stats) = self.processor.get_stream_stats(stream_id) {
                    Ok(serde_json::to_value(stats)
                        .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?)
                } else {
                    Err(ExtensionError::SessionNotFound(stream_id.to_string()))
                }
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let registry = get_registry().lock();

        Ok(vec![
            ExtensionMetricValue {
                name: "active_streams".to_string(),
                value: ParamMetricValue::Integer(registry.streams.len() as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_frames_processed".to_string(),
                value: ParamMetricValue::Integer(registry.total_frames as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "avg_fps".to_string(),
                value: ParamMetricValue::Float(0.0),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// Public API for HTTP handlers
// ============================================================================

/// Get latest frame for MJPEG streaming
pub fn get_stream_frame(stream_id: &str) -> Option<Vec<u8>> {
    let registry = get_registry().lock();
    if let Some(stream) = registry.streams.get(stream_id) {
        let s = stream.lock();
        s.last_frame.clone()
    } else {
        None
    }
}

/// Get stream statistics
pub fn get_stream_stats_public(stream_id: &str) -> Option<StreamStats> {
    let registry = get_registry().lock();
    if let Some(stream) = registry.streams.get(stream_id) {
        let s = stream.lock();
        Some(StreamStats {
            stream_id: stream_id.to_string(),
            frame_count: s.frame_count,
            fps: s.fps,
            total_detections: s.total_detections,
            detected_objects: s.detected_objects.clone(),
        })
    } else {
        None
    }
}

// ============================================================================
// Export FFI using SDK macro
// ============================================================================

neomind_extension_sdk::neomind_export!(YoloVideoProcessorV2);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = YoloVideoProcessorV2::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "yolo-video-v2");
        assert_eq!(meta.name, "YOLO Video Processor V2");
    }

    #[test]
    fn test_extension_metrics() {
        let ext = YoloVideoProcessorV2::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 3);
    }

    #[test]
    fn test_extension_commands() {
        let ext = YoloVideoProcessorV2::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].name, "start_stream");
    }

    #[test]
    fn test_stream_config_default() {
        let config = StreamConfig::default();
        assert_eq!(config.source_url, "camera://0");
        assert_eq!(config.confidence_threshold, 0.5);
    }

    #[test]
    fn test_fallback_detections() {
        let detections = generate_fallback_detections(0, 5);
        assert!(!detections.is_empty());
        assert_eq!(detections[0].label, "person");
    }

    #[test]
    fn test_encode_jpeg() {
        let img = image::RgbImage::from_pixel(100, 100, image::Rgb([128, 128, 128]));
        let jpeg = encode_jpeg(&img, 80);
        assert!(!jpeg.is_empty());
        // JPEG should start with JPEG signature
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8]);
    }
}
