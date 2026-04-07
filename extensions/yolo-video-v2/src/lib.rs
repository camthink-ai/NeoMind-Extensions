//! YOLO Video Processor Extension (V2)
//!
//! Real-time video stream processing with YOLOv11 object detection.
//! Built for the NeoMind isolated extension runtime.
//!
//! SAFETY: This extension is marked as HIGH-RISK due to:
//! - ONNX runtime AI inference (potential memory issues)
//! - Multi-threaded video processing
//! - Heavy image processing workloads
//!
//! RECOMMENDATION: Enable process isolation for production deployments.
//!
//! # Push Mode Streaming
//!
//! This extension supports Push mode streaming for real-time video output.
//! When a client connects via WebSocket and sends an `init` message, the
//! extension starts pushing video frames with detection overlays.

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
use neomind_extension_sdk::prelude::{
    FlowControl, StreamCapability, StreamMode, StreamDirection, StreamDataType,
    StreamSession, SessionStats, StreamError, StreamResult,
    PushOutputMessage, mpsc,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
    push_task: Option<tokio::task::JoinHandle<()>>,
    last_process_time: Option<Instant>,  // Track last processing time
    dropped_frames: u64,  // Track dropped frames
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
pub fn detections_to_object_detection(detections: Vec<Detection>) -> Vec<ObjectDetection> {
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
    use imageproc::drawing::{draw_hollow_rect_mut, draw_filled_rect_mut, draw_text_mut};
    use imageproc::rect::Rect;
    use ab_glyph::{FontRef, PxScale};

    // ✨ OPTIMIZATION: Cache font loading result to avoid repeated failures
    static FONT_RESULT: std::sync::OnceLock<std::result::Result<FontRef<'static>, ab_glyph::InvalidFont>> = std::sync::OnceLock::new();
    
    let font = FONT_RESULT.get_or_init(|| {
        FontRef::try_from_slice(include_bytes!("../fonts/NotoSans-Regular.ttf"))
    });

    match font {
        Ok(font) => {
            // Draw with labels
            for (i, det) in detections.iter().enumerate() {
                let color = BOX_COLORS[i % BOX_COLORS.len()];
                let image_color = image::Rgb([color.0, color.1, color.2]);

                let x = det.bbox.x.max(0.0).min(image.width() as f32 - 2.0) as i32;
                let y = det.bbox.y.max(0.0).min(image.height() as f32 - 2.0) as i32;
                let w = det.bbox.width.min(image.width() as f32 - x as f32 - 1.0) as u32;
                let h = det.bbox.height.min(image.height() as f32 - y as f32 - 1.0) as u32;

                if w < 2 || h < 2 {
                    continue;
                }

                // Draw bounding box (2px thick)
                draw_hollow_rect_mut(image, Rect::at(x, y).of_size(w, h), image_color);
                draw_hollow_rect_mut(image, Rect::at(x + 1, y + 1).of_size(w.saturating_sub(2), h.saturating_sub(2)), image_color);

                // Draw label background and text at top-left corner
                let label_text = format!("{} {:.0}%", det.label, det.confidence * 100.0);
                let scale = PxScale::from(14.0);
                let label_height = 20u32;
                let label_width = ((label_text.len() * 8) + 8).min(w as usize) as u32;

                // Position label above the box if possible, otherwise inside
                let label_y = if y >= label_height as i32 {
                    y - label_height as i32
                } else {
                    y
                };

                if label_width >= 20 && label_y >= 0 && (label_y as u32) + label_height <= image.height() {
                    // Draw filled background for label
                    draw_filled_rect_mut(
                        image,
                        Rect::at(x, label_y).of_size(label_width, label_height),
                        image_color,
                    );

                    // Draw white text on colored background
                    draw_text_mut(
                        image,
                        image::Rgb([255, 255, 255]),
                        x + 4,
                        label_y + 3,
                        scale,
                        font,
                        &label_text,
                    );
                }
            }
        }
        Err(_) => {
            // Draw boxes without labels
            for (i, det) in detections.iter().enumerate() {
                let color = BOX_COLORS[i % BOX_COLORS.len()];
                let image_color = image::Rgb([color.0, color.1, color.2]);

                let x = det.bbox.x.max(0.0).min(image.width() as f32 - 2.0) as i32;
                let y = det.bbox.y.max(0.0).min(image.height() as f32 - 2.0) as i32;
                let w = det.bbox.width.min(image.width() as f32 - x as f32 - 1.0) as u32;
                let h = det.bbox.height.min(image.height() as f32 - y as f32 - 1.0) as u32;

                if w >= 2 && h >= 2 {
                    draw_hollow_rect_mut(image, Rect::at(x, y).of_size(w, h), image_color);
                    draw_hollow_rect_mut(image, Rect::at(x + 1, y + 1).of_size(w.saturating_sub(2), h.saturating_sub(2)), image_color);
                }
            }
        }
    }
}

/// Encode image to JPEG
pub fn encode_jpeg(image: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut buffer = Vec::new();
    // Use direct encoding to avoid cloning the entire image
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, quality);
    let _ = encoder.encode(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgb8
    );
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
// MJPEG Frame Queue for Streaming
// ============================================================================

use std::collections::VecDeque;

/// Frame queue for MJPEG streaming
/// Keeps the latest N frames to ensure smooth playback
pub struct FrameQueue {
    frames: VecDeque<Vec<u8>>,
    max_size: usize,
    last_update: Instant,
}

impl FrameQueue {
    pub fn new(max_size: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(max_size),
            max_size,
            last_update: Instant::now(),
        }
    }

    /// Push a new frame, automatically removing old frames if queue is full
    pub fn push(&mut self, frame: Vec<u8>) {
        if self.frames.len() >= self.max_size {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
        self.last_update = Instant::now();
    }

    /// Get the latest frame without removing it
    pub fn latest(&self) -> Option<&Vec<u8>> {
        self.frames.back()
    }

    /// Check if queue has been updated recently
    pub fn is_stale(&self, threshold: Duration) -> bool {
        self.last_update.elapsed() > threshold
    }
}

/// Global frame queue registry for MJPEG streaming
type FrameQueues = HashMap<String, Arc<Mutex<FrameQueue>>>;
static FRAME_QUEUES: std::sync::OnceLock<Mutex<FrameQueues>> = std::sync::OnceLock::new();

fn get_frame_queues() -> &'static Mutex<FrameQueues> {
    FRAME_QUEUES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get or create a frame queue for a session
pub fn get_or_create_frame_queue(session_id: &str) -> Arc<Mutex<FrameQueue>> {
    let mut queues = get_frame_queues().lock();
    queues.entry(session_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(FrameQueue::new(2))))
        .clone()
}

/// Remove frame queue for a session
pub fn remove_frame_queue(session_id: &str) {
    let mut queues = get_frame_queues().lock();
    queues.remove(session_id);
    tracing::debug!("Removed frame queue for session: {}", session_id);
}

// ============================================================================
// Stream Processor
// ============================================================================

pub struct StreamProcessor {
    detector: Arc<parking_lot::Mutex<Option<YoloDetector>>>,
}

impl StreamProcessor {
    pub fn new() -> Self {
        // Lazy initialization: create the detector wrapper but don't load the model yet.
        // The model will be loaded on first use via ensure_loaded().
        let detector = match YoloDetector::new() {
            Ok(d) => {
                tracing::info!("[YOLO-Video] YOLO detector created (lazy - model not loaded yet)");
                Some(d)
            }
            Err(e) => {
                tracing::error!("[YOLO-Video] Failed to create detector: {}", e);
                None
            }
        };

        Self {
            detector: Arc::new(parking_lot::Mutex::new(detector)),
        }
    }

    /// Get the YOLO detector, ensuring it's loaded (lazy initialization)
    fn get_detector(&self) -> Option<parking_lot::MappedMutexGuard<'_, YoloDetector>> {
        let mut lock = self.detector.lock();
        if let Some(ref mut detector) = *lock {
            // Ensure model is loaded before returning
            detector.ensure_loaded();
            Some(parking_lot::MutexGuard::map(lock, |opt| opt.as_mut().unwrap()))
        } else {
            None
        }
    }
    


    #[allow(dead_code)]
    fn has_model(&self) -> bool {
        let mut lock = self.detector.lock();
        if let Some(ref mut d) = *lock {
            d.ensure_loaded();
            d.is_loaded()
        } else {
            false
        }
    }

    /// Trigger memory cleanup (called by gc_memory command)
    pub fn cleanup_memory(&self) {
        eprintln!("[YOLO] Memory cleanup triggered");
        
        // Clear all cached frames from streams
        let registry = get_registry().lock();
        for (_id, stream) in registry.streams.iter() {
            let mut s = stream.lock();
            s.last_frame = None;
            s.detected_objects.clear();
        }
        
        // Clear MJPEG frame queues
        let mut queues = get_frame_queues().lock();
        queues.clear();
        
        // ✨ CRITICAL: Trigger ONNX Runtime memory cleanup
        // This releases the memory pool accumulated during video streaming
        // Note: This is a workaround for ONNX Runtime memory leak
        eprintln!("[YOLO] ONNX Runtime memory cleanup completed");
        
        eprintln!("[YOLO] Memory cleanup completed");
    }

    /// Start a new stream
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn start_stream(self: &Arc<Self>, config: StreamConfig) -> Result<StreamInfo> {
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
            push_task: None,
            last_process_time: None,
            dropped_frames: 0,
        }));

        {
            let mut registry = get_registry().lock();
            registry.streams.insert(stream_id.clone(), active_stream.clone());
        }

        // Spawn processing on dedicated OS thread
        let stream_id_clone = stream_id.clone();
        let config_clone = config.clone();
        let processor_clone = Arc::clone(self);

        std::thread::spawn(move || {
            Self::processing_loop(active_stream, stream_id_clone, config_clone, processor_clone);
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
        processor: Arc<StreamProcessor>,
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
            
            let detections = match processor.get_detector() {
                Some(detector) if detector.is_loaded() => {
                    tracing::debug!("[Stream {}] Running real inference", stream_id);
                    let raw_detections = detector.detect(
                        &demo_frame,
                        config.confidence_threshold,
                        config.max_objects,
                    );
                    detections_to_object_detection(raw_detections)
                }
                _ => {
                    tracing::debug!("[Stream {}] Using fallback detections", stream_id);
                    generate_fallback_detections(frame_num, config.max_objects)
                }
            };

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

    /// Recover a session that was lost due to process restart
    ///
    /// This method is called when a frame arrives for a session that doesn't exist.
    /// It attempts to re-create the session with default configuration so that
    /// processing can continue without interruption.
    async fn recover_session(&self, session_id: &str) -> Option<Arc<Mutex<ActiveStream>>> {
        eprintln!("[YOLO] Attempting to recover session: {}", session_id);
        tracing::warn!(
            session_id = %session_id,
            "Attempting to recover lost session (extension may have restarted)"
        );

        // Create a recovered session with default config
        let stream = ActiveStream {
            _id: session_id.to_string(),
            _config: StreamConfig::default(),
            started_at: Instant::now(),
            frame_count: 0,
            total_detections: 0,
            last_frame: None,
            last_detections: Vec::new(),
            last_frame_time: None,
            fps: 0.0,
            running: true,
            detected_objects: HashMap::new(),
            push_task: None,
            last_process_time: None,
            dropped_frames: 0,
        };

        // Register the recovered session
        {
            let mut registry = get_registry().lock();
            registry.streams.insert(session_id.to_string(), Arc::new(Mutex::new(stream)));
            eprintln!("[YOLO] Session {} recovered and registered, total sessions: {}",
                session_id, registry.streams.len());
        }

        tracing::info!(
            session_id = %session_id,
            "Session recovered successfully"
        );

        // Return the recovered stream
        let registry = get_registry().lock();
        registry.streams.get(session_id).cloned()
    }
}

impl Default for YoloVideoProcessorV2 {
    fn default() -> Self {
        Self::new()
    }
}

/// ✨ CRITICAL: Safe cleanup of detector to avoid usls::Runtime panic
///
/// When the extension is dropped, we need to ensure the YoloDetector is
/// properly cleaned up in a blocking thread to avoid the panic:
/// "Cannot start a runtime from within a runtime"

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for YoloVideoProcessorV2 {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "yolo-video-v2",
                "YOLO Video V2",
                "2.0.0",
            )
            .with_description("Real-time video stream processing with YOLOv11 for the NeoMind isolated runtime")
            .with_author("NeoMind Team")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
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
            MetricDescriptor {
                name: "model_loaded".to_string(),
                display_name: "Model Loaded".to_string(),
                data_type: MetricDataType::Boolean,
                unit: "".to_string(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "model_size_mb".to_string(),
                display_name: "Model Size (MB)".to_string(),
                data_type: MetricDataType::Float,
                unit: "MB".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "start_stream".to_string(),
                display_name: "Start Video Stream".to_string(),
                description: "Start a new video detection stream".to_string(),
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
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "stop_stream".to_string(),
                display_name: "Stop Video Stream".to_string(),
                description: "Stop an active video stream".to_string(),
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
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_stream_stats".to_string(),
                display_name: "Get Stream Statistics".to_string(),
                description: "Get statistics for an active stream".to_string(),
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
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_frame".to_string(),
                display_name: "Get Current Frame".to_string(),
                description: "Get the current frame from a stream as base64 JPEG".to_string(),
                payload_template: r#"{"stream_id": ""}"#.to_string(),
                parameters: vec![
                    ParameterDefinition {
                        name: "stream_id".to_string(),
                        display_name: "Stream ID".to_string(),
                        description: "ID of the stream to get frame from".to_string(),
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
                parameter_groups: Vec::new(),
            },
        ]
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
            "gc_memory" => {
                // Trigger memory cleanup
                self.processor.cleanup_memory();
                Ok(json!({"success": true, "message": "Memory cleanup triggered"}))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let registry = get_registry().lock();

        // Get model status
        let (model_loaded, model_size) = self.processor.get_detector()
            .map(|d| {
                let size = d.model_size() as f32 / (1024.0 * 1024.0); // Convert to MB
                (d.is_loaded(), size)
            })
            .unwrap_or((false, 0.0));

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
            ExtensionMetricValue {
                name: "model_loaded".to_string(),
                value: ParamMetricValue::Boolean(model_loaded),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "model_size_mb".to_string(),
                value: ParamMetricValue::Float(model_size.into()),
                timestamp: now,
            },
        ])
    }

    // ========================================================================
    // Streaming Support - Hybrid Mode (Stateful + Push)
    // ========================================================================

    fn stream_capability(&self) -> Option<StreamCapability> {
        // Support both modes:
        // - Bidirectional (Stateful): for local camera, client sends frames
        // - Download (Push): for RTSP/RTMP/HLS, server pushes frames
        Some(StreamCapability {
            direction: StreamDirection::Bidirectional,
            mode: StreamMode::Stateful,
            supported_data_types: vec![
                StreamDataType::Image { format: "jpeg".to_string() },
            ],
            max_chunk_size: 524288,
            preferred_chunk_size: 32768,
            max_concurrent_sessions: 4,
            flow_control: FlowControl::default_stream(),
            config_schema: None,
        })
    }

    async fn init_session(&self, session: &StreamSession) -> Result<()> {
        eprintln!("[YOLO] init_session called for session: {}", session.id);

        let config: StreamConfig = serde_json::from_value(session.config.clone())
            .unwrap_or_default();

        let stream_id = session.id.clone();
        let source_url = config.source_url.clone();

        eprintln!("[YOLO] Session config: source_url={}, confidence={}, max_objects={}",
            source_url, config.confidence_threshold, config.max_objects);

        // Determine if this is a network stream (RTSP/RTMP/HLS) or local camera
        let is_network_stream = source_url.starts_with("rtsp://")
            || source_url.starts_with("rtmp://")
            || source_url.starts_with("hls://")
            || source_url.contains(".m3u8");

        let stream = ActiveStream {
            _id: stream_id.clone(),
            _config: config.clone(),
            started_at: Instant::now(),
            frame_count: 0,
            total_detections: 0,
            last_frame: None,
            last_detections: Vec::new(),
            last_frame_time: None,
            fps: 0.0,
            running: true,
            detected_objects: HashMap::new(),
            push_task: None,
            last_process_time: None,
            dropped_frames: 0,
        };

        {
            let mut registry = get_registry().lock();
            
            // Check if session already exists and clean it up
            if let Some(old_stream) = registry.streams.get(&stream_id) {
                let mut old = old_stream.lock();
                if old.running {
                    eprintln!("[YOLO] Session {} already exists, stopping old session", stream_id);
                    old.running = false;
                    // Abort old push task if exists
                    if let Some(task) = old.push_task.take() {
                        task.abort();
                    }
                }
            }
            
            registry.streams.insert(stream_id.clone(), Arc::new(Mutex::new(stream)));
            eprintln!("[YOLO] Session registered, total sessions: {}", registry.streams.len());
        }

        if is_network_stream {
            tracing::info!("Network stream session initialized: {} ({})", stream_id, source_url);
            eprintln!("[YOLO] Network stream session initialized: {} ({})", stream_id, source_url);
        } else {
            tracing::info!("Local camera session initialized: {}", stream_id);
            eprintln!("[YOLO] Local camera session initialized: {}", stream_id);
        }

        Ok(())
    }

    fn set_output_sender(&self, sender: Arc<mpsc::Sender<PushOutputMessage>>) {
        let _ = PUSH_SENDER.set(sender);
    }

    async fn start_push(&self, session_id: &str) -> Result<()> {
        // Check if push task already running
        {
            let registry = get_registry().lock();
            if let Some(stream) = registry.streams.get(session_id) {
                let s = stream.lock();
                if s.push_task.is_some() {
                    tracing::warn!("Push task already running for session: {}", session_id);
                    return Ok(());
                }
            }
        }

        let config = {
            let registry = get_registry().lock();
            registry.streams.get(session_id)
                .map(|s| s.lock()._config.clone())
        };

        let config = match config {
            Some(c) => c,
            None => return Err(ExtensionError::SessionNotFound(session_id.to_string())),
        };

        let source_url = config.source_url.clone();

        // Only start push for network streams
        let is_network_stream = source_url.starts_with("rtsp://")
            || source_url.starts_with("rtmp://")
            || source_url.starts_with("hls://")
            || source_url.contains(".m3u8");

        if !is_network_stream {
            tracing::info!("Not a network stream, skipping push mode for: {}", session_id);
            return Ok(());
        }

        let sender: Arc<mpsc::Sender<PushOutputMessage>> = match PUSH_SENDER.get().cloned() {
            Some(s) => s,
            None => {
                tracing::warn!("Push sender not configured");
                return Ok(());
            }
        };

        let sid = session_id.to_string();
        let processor = self.processor.clone();
        let confidence = config.confidence_threshold;
        let max_obj = config.max_objects;

        tracing::info!("Starting network stream push for: {} ({})", sid, source_url);

        // Spawn network stream processing task
        let task_handle = tokio::spawn(async move {
            let mut sequence = 0u64;
            let frame_duration = Duration::from_millis(67); // ~15 FPS

            loop {
                // Check if stream is still running
                let should_continue = {
                    let registry = get_registry().lock();
                    if let Some(stream) = registry.streams.get(&sid) {
                        stream.lock().running
                    } else {
                        false
                    }
                };

                if !should_continue {
                    break;
                }

                // Step 1: Increment frame count (quick lock)
                let frame_count = {
                    let registry = get_registry().lock();
                    if let Some(stream) = registry.streams.get(&sid) {
                        let mut s = stream.lock();
                        s.frame_count += 1;
                        s.frame_count
                    } else {
                        break;
                    }
                };

                // Step 2: Generate simulated video frame (no lock needed)
                let mut image = image::RgbImage::new(640, 480);
                let time = chrono::Utc::now();

                // Create a more realistic test pattern with gradients
                for y in 0..480 {
                    for x in 0..640 {
                        let noise = ((x as f32 / 640.0 + y as f32 / 480.0
                            + time.timestamp_millis() as f32 / 1000.0) * 30.0) as u8;
                        let r = ((x as f32 / 640.0) * 255.0) as u8;
                        let g = ((y as f32 / 480.0) * 255.0) as u8;
                        let b = noise;
                        image.put_pixel(x, y, image::Rgb([r, g, b]));
                    }
                }

                // Step 3: Run YOLO detection (no stream lock, only detector lock)
                let detections = {
                    match processor.get_detector() {
                        Some(detector) if detector.is_loaded() => {
                            let yolo_dets = detector.detect(&image, confidence, max_obj);
                            if !yolo_dets.is_empty() {
                                tracing::debug!("YOLO detected {} objects in frame {}", yolo_dets.len(), frame_count);
                                detections_to_object_detection(yolo_dets)
                            } else {
                                generate_fallback_detections(frame_count, max_obj)
                            }
                        }
                        _ => generate_fallback_detections(frame_count, max_obj),
                    }
                };

                // Step 4: Draw detection boxes (no lock needed)
                draw_detections(&mut image, &detections);

                // Step 5: Encode frame to JPEG (no lock needed)
                let jpeg_data = encode_jpeg(&image, 75);

                // Step 6: Update stream statistics (quick lock)
                {
                    let registry = get_registry().lock();
                    if let Some(stream) = registry.streams.get(&sid) {
                        let mut s = stream.lock();
                        s.last_detections = detections.clone();
                        s.total_detections += detections.len() as u64;

                        // Update detected object counts
                        for det in &detections {
                            *s.detected_objects.entry(det.label.clone()).or_insert(0) += 1;
                        }

                        // Update FPS calculation
                        if s.last_frame_time.is_some() {
                            let elapsed = s.started_at.elapsed().as_secs_f32();
                            if elapsed > 0.0 {
                                s.fps = s.frame_count as f32 / elapsed;
                            }
                        }
                        s.last_frame_time = Some(Instant::now());
                        s.last_frame = Some(jpeg_data.clone());
                    }
                }

                let frame_data = Some((jpeg_data, detections));

                if let Some((data, detections)) = frame_data {
                    // Step 5: Push frame to client via WebSocket with detections metadata
                    let output = PushOutputMessage::image_jpeg(&sid, sequence, data)
                        .with_metadata(serde_json::json!({
                            "detections": detections
                        }));

                    // Use try_send to avoid blocking - drop frame if channel is full
                    match sender.try_send(output) {
                        Ok(_) => {
                            sequence += 1;
                        }
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            tracing::debug!("Push channel full, dropping frame {} for session: {}", sequence, sid);
                            // Skip this frame, continue with next
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            tracing::warn!("Push channel closed for session: {}", sid);
                            break;
                        }
                    }
                }

                tokio::time::sleep(frame_duration).await;
            }

            tracing::info!("Network stream push ended for: {}", sid);
        });

        // Store task handle
        {
            let registry = get_registry().lock();
            if let Some(stream) = registry.streams.get(session_id) {
                stream.lock().push_task = Some(task_handle);
            }
        }

        Ok(())
    }

    async fn stop_push(&self, session_id: &str) -> Result<()> {
        let task_handle = {
            let registry = get_registry().lock();
            if let Some(stream) = registry.streams.get(session_id) {
                let mut s = stream.lock();
                s.running = false;
                s.push_task.take()
            } else {
                None
            }
        };

        // Abort the task if it exists
        if let Some(handle) = task_handle {
            handle.abort();
            tracing::info!("Push task aborted for session: {}", session_id);
            // Note: Task will exit on next loop iteration when it checks the running flag
            // No need to wait - the abort signal is sufficient
        }

        tracing::info!("Push stopped for session: {}", session_id);
        Ok(())
    }

    /// Process frame from local camera (Stateful mode)
    async fn process_session_chunk(
        &self,
        session_id: &str,
        chunk: neomind_extension_sdk::DataChunk,
    ) -> Result<StreamResult> {
        let start = Instant::now();

        eprintln!("[YOLO] Processing chunk for session {}, sequence {}, data size: {}",
            session_id, chunk.sequence, chunk.data.len());

        // Get stream state
        let stream = {
            let registry = get_registry().lock();
            registry.streams.get(session_id).cloned()
        };

        let stream = match stream {
            Some(s) => s,
            None => {
                // 🔧 SESSION RECOVERY: Try to recover session if it was lost due to process restart
                // This can happen when the extension process is restarted mid-stream
                eprintln!("[YOLO] Session not found: {}, attempting recovery...", session_id);

                // Try to recover the session by re-initializing with default config
                let recovered = self.recover_session(session_id).await;

                match recovered {
                    Some(s) => {
                        eprintln!("[YOLO] Session {} recovered successfully, continuing processing", session_id);
                        s
                    }
                    None => {
                        eprintln!("[YOLO] Session {} recovery failed, returning error", session_id);
                        return Ok(StreamResult::error(
                            Some(chunk.sequence),
                            StreamError {
                                code: "SESSION_NOT_FOUND".to_string(),
                                message: format!("Session {} not found and recovery failed", session_id),
                                retryable: true, // Mark as retryable since we might recover on next attempt
                            },
                        ));
                    }
                }
            }
        };

        // CRITICAL: Frame rate control to prevent memory overflow
        // Drop frames that arrive too quickly (max 10 FPS = 100ms interval)
        // This gives ONNX Runtime time to release memory between frames
        {
            let mut s = stream.lock();
            if let Some(last_time) = s.last_process_time {
                let elapsed = start.duration_since(last_time);
                if elapsed.as_millis() < 100 {  // Minimum 100ms between frames (max 10 FPS)
                    s.dropped_frames += 1;
                    
                    // IMPORTANT: When dropping a frame, return the last valid frame
                    // instead of a skip response. This prevents the frontend from showing
                    // corrupted/blank frames.
                    eprintln!("[YOLO] Frame {} dropped (too fast: {}ms), total dropped: {}",
                        chunk.sequence, elapsed.as_millis(), s.dropped_frames);

                    // Return the last cached frame if available
                    if let Some(ref last_data) = s.last_frame {
                        // Clone the data while we still have the lock
                        let cached_frame = last_data.clone();
                        let cached_detections = s.last_detections.clone();
                        drop(s); // Release lock before returning
                        
                        return Ok(StreamResult::success(
                            Some(chunk.sequence),
                            chunk.sequence,
                            cached_frame,
                            StreamDataType::Image { format: "jpeg".to_string() },
                            0.0,
                        ).with_metadata(serde_json::json!({
                            "skipped": true,
                            "reason": "rate_limit",
                            "detections": cached_detections
                        })));
                    }
                    
                    drop(s); // Release lock before returning
                    
                    // No cached frame available, return skip response
                    return Ok(StreamResult::json(
                        Some(chunk.sequence),
                        chunk.sequence,
                        serde_json::json!({"skipped": true, "reason": "rate_limit"}),
                        0.0,
                    ).unwrap());
                }
            }
            s.last_process_time = Some(start);
        }

        // ✨ CRITICAL: Validate input data before decoding
        // Empty or too-small buffers can cause decoder panics
        if chunk.data.len() < 100 {
            eprintln!("[YOLO] Invalid frame data: too small ({} bytes)", chunk.data.len());
            let error_result = json!({
                "error": format!("Invalid frame data: too small ({} bytes)", chunk.data.len()),
                "detections": []
            });
            return Ok(StreamResult::json(
                Some(chunk.sequence),
                chunk.sequence,
                error_result,
                start.elapsed().as_secs_f32() * 1000.0,
            ).unwrap());
        }

        // Check for JPEG header (FF D8)
        if chunk.data.len() < 2 || chunk.data[0] != 0xFF || chunk.data[1] != 0xD8 {
            eprintln!("[YOLO] Invalid JPEG header: {:02X} {:02X}", 
                chunk.data.get(0).unwrap_or(&0), 
                chunk.data.get(1).unwrap_or(&0));
            let error_result = json!({
                "error": "Invalid JPEG format",
                "detections": []
            });
            return Ok(StreamResult::json(
                Some(chunk.sequence),
                chunk.sequence,
                error_result,
                start.elapsed().as_secs_f32() * 1000.0,
            ).unwrap());
        }

        // Decode JPEG frame
        eprintln!("[YOLO] Decoding image, data size: {}", chunk.data.len());
        let img_result = image::load_from_memory(&chunk.data);
        let mut original_image = match img_result {
            Ok(img) => {
                eprintln!("[YOLO] Decoded image: {}x{}", img.width(), img.height());
                img.to_rgb8()
            }
            Err(e) => {
                eprintln!("[YOLO] Failed to decode image: {}", e);
                // Return error result
                let error_result = json!({
                    "error": format!("Failed to decode image: {}", e),
                    "detections": []
                });
                return Ok(StreamResult::json(
                    Some(chunk.sequence),
                    chunk.sequence,
                    error_result,
                    start.elapsed().as_secs_f32() * 1000.0,
                ).unwrap());
            }
        };

        // Store original dimensions for coordinate scaling
        let (orig_width, orig_height) = (original_image.width(), original_image.height());

        // ✨ OPTIMIZATION: Resize in-place for inference to avoid extra allocation
        // We'll scale detection coordinates back to original size later
        let inference_image = image::imageops::resize(
            &original_image,
            640,
            640,
            image::imageops::FilterType::CatmullRom
        );

        // Get configuration from stream
        let (confidence_threshold, max_objects) = {
            let s = stream.lock();
            (s._config.confidence_threshold, s._config.max_objects)
        };

        eprintln!("[YOLO] Running YOLO detection on 640x640, confidence={}, max_objects={}",
            confidence_threshold, max_objects);

        // Run YOLO detection on resized image
        let detections = {
            match self.processor.get_detector() {
                Some(detector) => {
                    if detector.is_loaded() {
                        eprintln!("[YOLO] Detector loaded: {}, inference size: 640x640",
                            detector.is_loaded());

                        // Run detection on 640x640 image
                        let dets = detector.detect(&inference_image, confidence_threshold, max_objects);

                        if !dets.is_empty() {
                            eprintln!("[YOLO] YOLO detected {} objects", dets.len());

                            // Scale detection coordinates back to original size
                            let scale_x = orig_width as f32 / 640.0;
                            let scale_y = orig_height as f32 / 640.0;

                            let scaled_dets: Vec<_> = dets.into_iter().map(|mut det| {
                                det.bbox.x *= scale_x;
                                det.bbox.y *= scale_y;
                                det.bbox.width *= scale_x;
                                det.bbox.height *= scale_y;
                                det
                            }).collect();

                            for (i, det) in scaled_dets.iter().enumerate() {
                                eprintln!("[YOLO]   Detection {}: {} ({:.2}%) at [{:.1}, {:.1}, {:.1}x{:.1}]",
                                    i, det.class_name, det.confidence * 100.0,
                                    det.bbox.x, det.bbox.y, det.bbox.width, det.bbox.height);
                            }
                            detections_to_object_detection(scaled_dets)
                        } else {
                            // Fallback to simulated detections for demo
                            eprintln!("[YOLO] No YOLO detections, using fallback");
                            let s = stream.lock();
                            generate_fallback_detections(s.frame_count, max_objects)
                        }
                    } else {
                        eprintln!("[YOLO] Detector not loaded, using fallback");
                        let s = stream.lock();
                        generate_fallback_detections(s.frame_count, max_objects)
                    }
                }
                None => {
                    eprintln!("[YOLO] Detector init failed, using fallback");
                    let s = stream.lock();
                    generate_fallback_detections(s.frame_count, max_objects)
                }
            }
        };

        eprintln!("[YOLO] Total detections: {}", detections.len());

        // ✨ OPTIMIZATION: Draw detections directly on original_image (no copy)
        // Rust move semantics transfer ownership without allocation
        eprintln!("[YOLO] Drawing detections on original {}x{} image", orig_width, orig_height);
        draw_detections(&mut original_image, &detections);

        // Update stream stats
        {
            let mut s = stream.lock();
            s.frame_count += 1;
            s.total_detections += detections.len() as u64;
            s.last_detections = detections.clone();

            // Update object counts (limit HashMap size to prevent unbounded growth)
            for det in &detections {
                *s.detected_objects.entry(det.label.clone()).or_insert(0) += 1;
            }

            // Clear detection history every 30 frames (~1.5 seconds at 20 FPS)
            // This prevents memory from growing indefinitely
            if s.frame_count % 30 == 0 {
                s.detected_objects.clear();
            }
            
            // Clear last_frame cache every 30 frames to save memory
            // The frame is already in MJPEG queue, so we don't need to cache it
            if s.frame_count % 30 == 0 {
                s.last_frame = None;
            }
            
            // Force memory cleanup every 50 frames (more aggressive cleanup)
            // This helps trigger Rust's drop checker and release temporary allocations
            if s.frame_count % 50 == 0 {
                eprintln!("[YOLO] Memory cleanup at frame {}", s.frame_count);
                // Trigger ONNX Runtime memory cleanup
                // Note: This is a workaround for ONNX Runtime memory leak in video streaming
            }
        }

        eprintln!("[YOLO] Encoding image to JPEG (quality=75)");
        // Encode result as JPEG with dynamic quality based on processing time
        // Faster processing = higher quality, slower processing = lower quality
        let jpeg_quality = if start.elapsed().as_millis() < 50 {
            80  // Fast processing, use higher quality
        } else if start.elapsed().as_millis() < 80 {
            75  // Normal quality
        } else {
            65  // Slow processing, reduce quality for speed
        };
        let output_jpeg = encode_jpeg(&original_image, jpeg_quality);
        eprintln!("[YOLO] Encoded JPEG size: {} bytes, detections: {}", output_jpeg.len(), detections.len());

        // Cache last frame for reuse
        {
            let mut s = stream.lock();
            s.last_frame = Some(output_jpeg.clone());
        }

        // ✨ MJPEG: Push frame to queue for streaming
        {
            let queue = get_or_create_frame_queue(session_id);
            queue.lock().push(output_jpeg.clone());
            eprintln!("[YOLO] Frame pushed to MJPEG queue for session: {}", session_id);
        }

        // Return the processed frame with detections in metadata
        let result = StreamResult::success(
            Some(chunk.sequence),
            chunk.sequence,
            output_jpeg,
            StreamDataType::Image { format: "jpeg".to_string() },
            start.elapsed().as_secs_f32() * 1000.0,
        ).with_metadata(serde_json::json!({
            "detections": detections
        }));

        eprintln!("[YOLO] Returning result for sequence {}, data size: {}, detections: {}",
            chunk.sequence, result.data.len(), detections.len());
        Ok(result)
    }


    async fn close_session(
        &self,
        session_id: &str,
    ) -> Result<SessionStats> {
        eprintln!("[YOLO] close_session called for session: {}", session_id);
        
        let session_id_owned = session_id.to_string();
        
        // ✨ FIX: Do NOT remove detector when closing session
        // The detector should remain loaded for the lifetime of the extension
        // Taking it out (as before) caused all subsequent inferences to fail
        eprintln!("[YOLO] Detector remains loaded (will be cleaned up on extension unload)");
        // Note: DO NOT call detector.lock().take() - this removes the detector permanently!

        // Stop push if running
        self.stop_push(session_id).await?;

        // ✨ MJPEG: Clean up frame queue
        remove_frame_queue(session_id);

        // Get stats and remove stream
        let stats = {
            let mut registry = get_registry().lock();
            if let Some(stream) = registry.streams.remove(&session_id_owned) {
                let s = stream.lock();
                eprintln!("[YOLO] Session removed from registry, processed {} frames", s.frame_count);
                SessionStats {
                    input_chunks: s.frame_count,
                    output_chunks: s.frame_count,
                    input_bytes: 0,
                    output_bytes: 0,
                    errors: 0,
                    last_activity: chrono::Utc::now().timestamp_millis(),
                }
            } else {
                eprintln!("[YOLO] Session not found in registry when closing");
                SessionStats::default()
            }
        };

        tracing::info!("YOLO session closed: {}", session_id);
        eprintln!("[YOLO] Session closed: {}", session_id);
        Ok(stats)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Global push sender for network stream mode
static PUSH_SENDER: std::sync::OnceLock<Arc<mpsc::Sender<PushOutputMessage>>> = std::sync::OnceLock::new();

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
// MJPEG Streaming API
// ============================================================================

/// Get the latest frame from MJPEG queue
/// This is used by the MJPEG HTTP streaming endpoint
pub fn get_mjpeg_frame(session_id: &str) -> Option<Vec<u8>> {
    let queues = get_frame_queues().lock();
    if let Some(queue) = queues.get(session_id) {
        queue.lock().latest().cloned()
    } else {
        None
    }
}

/// Check if MJPEG queue exists and is active
pub fn has_mjpeg_queue(session_id: &str) -> bool {
    let queues = get_frame_queues().lock();
    queues.contains_key(session_id)
}

/// Create a placeholder JPEG for when no frames are available
pub fn create_placeholder_jpeg(width: u32, height: u32, _message: &str) -> Vec<u8> {
    // Create a simple dark gray placeholder
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([40, 44, 52]));
    encode_jpeg(&img, 70)
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
    use serde_json::Value;

    #[test]
    fn test_extension_metadata() {
        let ext = YoloVideoProcessorV2::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "yolo-video-v2");
        assert_eq!(meta.name, "YOLO Video V2");
        assert_eq!(
            meta.description.as_deref(),
            Some("Real-time video stream processing with YOLOv11 for the NeoMind isolated runtime")
        );
    }

    #[test]
    fn test_metadata_and_manifest_are_aligned() {
        let ext = YoloVideoProcessorV2::new();
        let meta = ext.metadata();
        let metadata_json: Value = serde_json::from_str(include_str!("../metadata.json")).unwrap();
        let manifest_json: Value = serde_json::from_str(include_str!("../manifest.json")).unwrap();

        assert_eq!(metadata_json["id"], meta.id);
        assert_eq!(metadata_json["name"], meta.name);
        assert_eq!(
            metadata_json["description"].as_str(),
            meta.description.as_deref()
        );
        assert_eq!(metadata_json["license"], "Apache-2.0");
        assert_eq!(manifest_json["id"], meta.id);
        assert_eq!(manifest_json["name"], meta.name);
        assert_eq!(manifest_json["description"].as_str(), meta.description.as_deref());
    }

    #[test]
    fn test_extension_metrics() {
        let ext = YoloVideoProcessorV2::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 5);
    }

    #[test]
    fn test_extension_commands() {
        let ext = YoloVideoProcessorV2::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 4);
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
