//! YOLO Video Processor Extension
//!
//! A stateful extension that processes video streams with object detection.
//! Demonstrates the Stateful streaming mode for session-based processing.
//!
//! # Streaming Mode
//!
//! - **Mode**: Stateful (maintains session context)
//! - **Direction**: Upload (client → extension)
//! - **Supported Types**: JPEG, PNG images, H264/H265 video
//! - **Max Chunk Size**: 5MB per frame
//! - **Max Concurrent Sessions**: 5
//!
//! # Usage
//!
//! Build the extension:
//! ```bash
//! cd /Users/shenmingming/NeoMind-Extension
//! cargo build --release -p neomind-yolo-video
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;

use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, MetricDefinition, ExtensionCommand,
    ExtensionMetricValue, ParamMetricValue, MetricDataType, CommandDefinition,
    CExtensionMetadata, ABI_VERSION, Result,
};
use neomind_core::extension::{
    StreamCapability, StreamMode, StreamDirection, StreamDataType, DataChunk, StreamResult,
    StreamSession, SessionStats,
};

use async_trait::async_trait;
use serde_json::Value;
use semver::Version;

// ============================================================================
// Types
// ============================================================================

/// Object detection result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ObjectDetection {
    id: u32,
    label: String,
    confidence: f32,
    bbox: BoundingBox,
    class_id: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BoundingBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Frame detection result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct FrameResult {
    frame_number: u64,
    timestamp_ms: i64,
    detections: Vec<ObjectDetection>,
    fps: f32,
    processing_time_ms: u64,
}

/// Session state for video processing
#[derive(Debug)]
struct VideoSession {
    id: String,
    created_at: i64,
    frame_count: u64,
    total_processing_time_ms: u64,
    total_detections: u64,
    last_frame_time: Option<i64>,
    config: VideoConfig,
    detected_objects: HashMap<String, u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct VideoConfig {
    confidence_threshold: f32,
    max_objects: u32,
    target_fps: u32,
    enable_tracking: bool,
}

/// Global statistics
#[derive(Debug, Default)]
struct GlobalStats {
    sessions_created: u64,
    active_sessions: u64,
    total_frames_processed: u64,
    total_detections: u64,
}

// ============================================================================
// Static Metrics and Commands
// ============================================================================

static METRICS: Lazy<[MetricDefinition; 3]> = Lazy::new(|| [
    MetricDefinition {
        name: "active_sessions".to_string(),
        display_name: "Active Sessions".to_string(),
        data_type: MetricDataType::Integer,
        unit: "count".to_string(),
        min: Some(0.0),
        max: None,
        required: false,
    },
    MetricDefinition {
        name: "total_frames_processed".to_string(),
        display_name: "Total Frames Processed".to_string(),
        data_type: MetricDataType::Integer,
        unit: "frames".to_string(),
        min: Some(0.0),
        max: None,
        required: false,
    },
    MetricDefinition {
        name: "avg_fps".to_string(),
        display_name: "Average FPS".to_string(),
        data_type: MetricDataType::Float,
        unit: "fps".to_string(),
        min: Some(0.0),
        max: None,
        required: false,
    },
]);

static COMMANDS: Lazy<[CommandDefinition; 1]> = Lazy::new(|| [
    CommandDefinition {
        name: "get_session_info".to_string(),
        display_name: "Get Session Info".to_string(),
        payload_template: r#"{"session_id": ""}"#.to_string(),
        parameters: vec![],
        fixed_values: HashMap::new(),
        samples: vec![],
        llm_hints: "Get information about an active processing session".to_string(),
        parameter_groups: vec![],
    },
]);

// ============================================================================
// YOLO Video Processor Extension
// ============================================================================

pub struct YoloVideoProcessor {
    metadata: ExtensionMetadata,
    sessions: Arc<Mutex<HashMap<String, VideoSession>>>,
    stats: Arc<Mutex<GlobalStats>>,
}

impl YoloVideoProcessor {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata::new(
            "yolo-video",
            "YOLO Video Processor",
            Version::new(1, 0, 0),
        )
        .with_description("Stateful video stream processing with YOLO object detection")
        .with_author("NeoMind Team");

        Self {
            metadata,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(GlobalStats::default())),
        }
    }

    /// Process a video frame
    fn process_frame(&self, session: &mut VideoSession, _data: &[u8], sequence: u64) -> Result<FrameResult> {
        let start = std::time::Instant::now();

        // Simulate YOLO detection
        let detections = self.run_yolo_detection(session)?;

        let processing_time = start.elapsed().as_millis() as u64;

        // Update session stats
        session.frame_count += 1;
        session.total_processing_time_ms += processing_time;
        session.total_detections += detections.len() as u64;
        session.last_frame_time = Some(chrono::Utc::now().timestamp_millis());

        // Track object frequency
        for detection in &detections {
            *session.detected_objects.entry(detection.label.clone()).or_insert(0) += 1;
        }

        // Calculate current FPS
        let elapsed_sec = session.total_processing_time_ms as f32 / 1000.0;
        let fps = if elapsed_sec > 0.0 {
            session.frame_count as f32 / elapsed_sec
        } else {
            session.config.target_fps as f32
        };

        Ok(FrameResult {
            frame_number: sequence,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            detections,
            fps,
            processing_time_ms: processing_time,
        })
    }

    /// Run YOLO detection (simulated for demo)
    fn run_yolo_detection(&self, session: &VideoSession) -> Result<Vec<ObjectDetection>> {
        let mut detections = Vec::new();

        // Simulate detecting common objects
        let common_objects = vec![
            ("person", 0),
            ("car", 1),
            ("truck", 2),
            ("bus", 3),
            ("bicycle", 4),
            ("dog", 5),
            ("cat", 6),
        ];

        // Randomly generate some detections
        let num_detections = (session.frame_count % 5) + 1;
        for i in 0..num_detections {
            let (label, class_id) = common_objects[i as usize % common_objects.len()];
            detections.push(ObjectDetection {
                id: i as u32,
                label: label.to_string(),
                confidence: 0.6 + (i as f32 * 0.05).min(0.35),
                bbox: BoundingBox {
                    x: 100.0 + i as f32 * 50.0,
                    y: 100.0 + i as f32 * 30.0,
                    width: 200.0,
                    height: 150.0,
                },
                class_id,
            });
        }

        // Filter by confidence threshold
        detections.retain(|d| d.confidence >= session.config.confidence_threshold);

        // Limit to max_objects
        detections.truncate(session.config.max_objects as usize);

        Ok(detections)
    }

    fn get_session_info(&self, session_id: &str) -> Result<Value> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions.get(session_id)
            .ok_or_else(|| ExtensionError::SessionNotFound(session_id.to_string()))?;

        Ok(serde_json::json!({
            "session_id": session.id,
            "frame_count": session.frame_count,
            "total_processing_time_ms": session.total_processing_time_ms,
            "total_detections": session.total_detections,
            "detected_objects": session.detected_objects,
            "config": session.config,
        }))
    }
}

#[async_trait::async_trait]
impl Extension for YoloVideoProcessor {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    fn metrics(&self) -> &[MetricDefinition] {
        &*METRICS
    }

    fn commands(&self) -> &[CommandDefinition] {
        &*COMMANDS
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &Value,
    ) -> Result<Value> {
        match command {
            "get_session_info" => {
                let session_id = args.get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing session_id".to_string()))?;
                self.get_session_info(session_id)
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let stats = self.stats.lock().unwrap();
        let sessions = self.sessions.lock().unwrap();
        let total_frames = stats.total_frames_processed;

        // Calculate average FPS across all sessions
        let total_time: u64 = sessions.values()
            .map(|s| s.total_processing_time_ms)
            .sum();
        let avg_fps = if total_time > 0 {
            total_frames as f32 / (total_time as f32 / 1000.0)
        } else {
            0.0
        };

        Ok(vec![
            ExtensionMetricValue::new(
                "active_sessions",
                ParamMetricValue::Integer(sessions.len() as i64),
            ),
            ExtensionMetricValue::new(
                "total_frames_processed",
                ParamMetricValue::Integer(total_frames as i64),
            ),
            ExtensionMetricValue::new(
                "avg_fps",
                ParamMetricValue::Float(avg_fps as f64),
            ),
        ])
    }

    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability {
            direction: StreamDirection::Upload,
            mode: StreamMode::Stateful,
            supported_data_types: vec![
                StreamDataType::Image { format: "jpeg".to_string() },
                StreamDataType::Image { format: "png".to_string() },
                StreamDataType::Video {
                    codec: "h264".to_string(),
                    width: 1920,
                    height: 1080,
                    fps: 30,
                },
                StreamDataType::Video {
                    codec: "h265".to_string(),
                    width: 1920,
                    height: 1080,
                    fps: 30,
                },
            ],
            max_chunk_size: 5 * 1024 * 1024, // 5MB per frame
            preferred_chunk_size: 1024 * 1024, // 1MB
            max_concurrent_sessions: 5,
            flow_control: Default::default(),
            config_schema: None,
        })
    }

    async fn init_session(&self, session: &StreamSession) -> Result<()> {
        let config: VideoConfig = serde_json::from_value(session.config.clone())
            .unwrap_or_default();

        let video_session = VideoSession {
            id: session.id.clone(),
            created_at: chrono::Utc::now().timestamp_millis(),
            frame_count: 0,
            total_processing_time_ms: 0,
            total_detections: 0,
            last_frame_time: None,
            config,
            detected_objects: HashMap::new(),
        };

        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(&session.id) {
            return Err(ExtensionError::SessionAlreadyExists(session.id.clone()));
        }
        sessions.insert(session.id.clone(), video_session);

        // Update global stats
        let mut stats = self.stats.lock().unwrap();
        stats.sessions_created += 1;
        stats.active_sessions = sessions.len() as u64;

        Ok(())
    }

    async fn process_session_chunk(
        &self,
        session_id: &str,
        chunk: DataChunk,
    ) -> Result<StreamResult> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(session_id)
            .ok_or_else(|| ExtensionError::SessionNotFound(session_id.to_string()))?;

        // Process the frame
        let result = self.process_frame(session, &chunk.data, chunk.sequence)?;

        // Update global stats
        drop(sessions);
        let mut stats = self.stats.lock().unwrap();
        stats.total_frames_processed += 1;
        stats.total_detections += result.detections.len() as u64;

        // Serialize result
        let output_data = serde_json::to_vec(&result)
            .map_err(|e| ExtensionError::InvalidStreamData(e.to_string()))?;

        Ok(StreamResult {
            input_sequence: Some(chunk.sequence),
            output_sequence: result.frame_number,
            data: output_data,
            data_type: StreamDataType::Json,
            processing_ms: result.processing_time_ms as f32,
            metadata: Some(serde_json::json!({
                "fps": result.fps,
                "detections": result.detections.len(),
                "processing_time_ms": result.processing_time_ms,
            })),
            error: None,
        })
    }

    async fn close_session(&self, session_id: &str) -> Result<SessionStats> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.remove(session_id)
            .ok_or_else(|| ExtensionError::SessionNotFound(session_id.to_string()))?;

        // Update global stats
        let mut stats = self.stats.lock().unwrap();
        stats.active_sessions = sessions.len() as u64;

        let session_stats = SessionStats {
            input_chunks: session.frame_count,
            output_chunks: session.frame_count,
            input_bytes: session.frame_count * 1024,
            output_bytes: session.total_detections * 100,
            errors: 0,
            last_activity: chrono::Utc::now().timestamp_millis(),
        };

        Ok(session_stats)
    }
}

// ============================================================================
// Global Extension Instance
// ============================================================================

static EXTENSION_INSTANCE: Lazy<YoloVideoProcessor> = Lazy::new(|| YoloVideoProcessor::new());

// ============================================================================
// FFI Exports
// ============================================================================

use tokio::sync::RwLock;

#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    ABI_VERSION
}

#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> CExtensionMetadata {
    use std::ffi::CStr;

    // Use static CStr references to avoid dangling pointers
    let id = CStr::from_bytes_with_nul(b"yolo-video\0").unwrap();
    let name = CStr::from_bytes_with_nul(b"YOLO Video Processor\0").unwrap();
    let version = CStr::from_bytes_with_nul(b"1.0.0\0").unwrap();
    let description = CStr::from_bytes_with_nul(b"Stateful video stream processing with YOLO\0").unwrap();
    let author = CStr::from_bytes_with_nul(b"NeoMind Team\0").unwrap();

    CExtensionMetadata {
        abi_version: ABI_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        version: version.as_ptr(),
        description: description.as_ptr(),
        author: author.as_ptr(),
        metric_count: 3,
        command_count: 1,
    }
}

#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Extension>> {
    use std::sync::Arc;

    // Parse config (ignored for this extension)
    let _config = if config_json.is_null() || config_len == 0 {
        serde_json::json!({})
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(config_json, config_len);
            let s = std::str::from_utf8_unchecked(slice);
            serde_json::from_str(s).unwrap_or(serde_json::json!({}))
        }
    };

    let extension = YoloVideoProcessor::new();
    Box::into_raw(Box::new(RwLock::new(Box::new(extension))))
}

#[no_mangle]
pub extern "C" fn neomind_extension_destroy(ptr: *mut RwLock<Box<dyn Extension>>) {
    if !ptr.is_null() {
        unsafe {
            let _ = Box::from_raw(ptr);
        }
    }
}
