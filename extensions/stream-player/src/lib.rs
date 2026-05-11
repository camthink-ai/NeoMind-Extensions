//! Stream Player Extension
//!
//! Universal video player for NeoMind that supports RTSP, RTMP, HLS, local files,
//! and HTTP video sources. Uses FFmpeg to decode any source, transcodes to JPEG frames,
//! and pushes them to the frontend via the SDK's push streaming infrastructure.
//!
//! Architecture:
//!   FFmpeg decode → RGB24 scale → JPEG encode → send_push_output → WebSocket → Canvas

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ffmpeg_next as ff;
use neomind_extension_sdk::{
    send_push_output, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, MetricDescriptor, MetricDataType, ParamMetricValue, Result,
};
use neomind_extension_sdk::prelude::{
    FlowControl, PushOutputMessage, SessionStats, StreamCapability, StreamDataType,
    StreamDirection, StreamMode, StreamResult, StreamSession,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ============================================================================
// Video Source Types
// ============================================================================

/// Supported source types
#[derive(Debug, Clone, PartialEq)]
enum SourceType {
    RTSP { url: String },
    RTMP { url: String },
    HLS { url: String },
    File { path: String, loop_: bool },
    Http { url: String },
}

/// Parse source URL into source type
fn parse_source_url(url: &str) -> std::result::Result<SourceType, String> {
    if url.starts_with("rtsp://") {
        Ok(SourceType::RTSP { url: url.to_string() })
    } else if url.starts_with("rtmp://") {
        Ok(SourceType::RTMP { url: url.to_string() })
    } else if url.starts_with("hls://") || url.contains(".m3u8") {
        Ok(SourceType::HLS { url: url.to_string() })
    } else if url.starts_with("file://") {
        Ok(SourceType::File {
            path: url[7..].to_string(),
            loop_: true,
        })
    } else if url.starts_with("http://") || url.starts_with("https://") {
        Ok(SourceType::Http { url: url.to_string() })
    } else if url.starts_with('/') || url.ends_with(".mp4") || url.ends_with(".avi")
        || url.ends_with(".mkv") || url.ends_with(".mov")
    {
        Ok(SourceType::File {
            path: url.to_string(),
            loop_: true,
        })
    } else {
        Err(format!("Unsupported source URL: {}", url))
    }
}

fn is_file_source(url: &str) -> bool {
    url.starts_with('/') || url.ends_with(".mp4") || url.ends_with(".avi")
        || url.ends_with(".mkv") || url.ends_with(".mov") || url.starts_with("file://")
}

// ============================================================================
// FFmpeg Video Decoder
// ============================================================================

/// FFmpeg video decoder — opens a source and yields decoded RGB24 frames.
///
/// **Thread safety**: `next_frame()` is blocking I/O. Must be called from a
/// dedicated OS thread, NOT from inside a tokio async context.
struct FfmpegDecoder {
    input_ctx: ff::format::context::Input,
    decoder: ff::decoder::Video,
    scaler: ff::software::scaling::Context,
    stream_index: usize,
    frame_count: u64,
    source_type: SourceType,
}

/// Decoded video frame (RGB24)
struct DecodedFrame {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

/// Result of frame decode
enum FrameResult {
    Frame(DecodedFrame),
    EndOfStream,
    Error(String),
}

impl FfmpegDecoder {
    fn new(source_type: &SourceType, output_width: u32, output_height: u32) -> std::result::Result<Self, String> {
        let url = match source_type {
            SourceType::RTSP { url } => url.as_str(),
            SourceType::RTMP { url } => url.as_str(),
            SourceType::HLS { url } => url.as_str(),
            SourceType::File { path, .. } => path.as_str(),
            SourceType::Http { url } => url.as_str(),
        };

        let mut input_opts = ff::Dictionary::new();
        if matches!(source_type, SourceType::RTSP { .. }) {
            input_opts.set("rtsp_transport", "tcp");
            input_opts.set("stimeout", "5000000");     // 5s socket timeout
            input_opts.set("rw_timeout", "10000000");   // 10s read/write timeout
        }
        if matches!(source_type, SourceType::RTMP { .. }) {
            input_opts.set("rw_timeout", "10000000"); // 10s read/write timeout
            input_opts.set("timeout", "10000000");     // 10s connection timeout
        }
        if matches!(source_type, SourceType::RTSP { .. } | SourceType::RTMP { .. } | SourceType::HLS { .. }) {
            input_opts.set("analyzeduration", "2000000");
            input_opts.set("probesize", "1000000");
        }

        let input_ctx = ff::format::input_with_dictionary(url, input_opts)
            .map_err(|e| format!("Failed to open '{}': {}", url, e))?;

        let stream = input_ctx.streams().best(ff::media::Type::Video)
            .ok_or("No video stream found")?;
        let stream_index = stream.index();

        let context = ff::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| format!("Codec context failed: {}", e))?;
        let decoder = context.decoder().video()
            .map_err(|e| format!("Video decoder failed: {}", e))?;

        let width = decoder.width();
        let height = decoder.height();

        // Scale to output size, convert to RGB24
        let scaler = ff::software::scaling::Context::get(
            decoder.format(),
            width, height,
            ff::format::Pixel::RGB24,
            output_width, output_height,
            ff::software::scaling::flag::Flags::BILINEAR,
        ).map_err(|e| format!("Scaler failed: {}", e))?;

        Ok(Self {
            input_ctx,
            decoder,
            scaler,
            stream_index,
            frame_count: 0,
            source_type: source_type.clone(),
        })
    }

    /// Decode next frame → RGB24 at output resolution. **BLOCKING**.
    fn next_frame(&mut self) -> FrameResult {
        let mut decoded = ff::frame::Video::empty();

        while let Some((stream, pkt)) = self.input_ctx.packets().next() {
            if stream.index() != self.stream_index {
                continue;
            }
            if self.decoder.send_packet(&pkt).is_err() {
                continue;
            }
            while self.decoder.receive_frame(&mut decoded).is_ok() {
                let mut rgb_frame = ff::frame::Video::empty();
                if self.scaler.run(&decoded, &mut rgb_frame).is_err() {
                    continue;
                }
                self.frame_count += 1;

                let width = rgb_frame.width();
                let height = rgb_frame.height();
                let stride = rgb_frame.stride(0);
                let row_bytes = (width as usize) * 3;

                // Strip row padding
                let data = if stride == row_bytes {
                    rgb_frame.data(0).to_vec()
                } else {
                    let raw = rgb_frame.data(0);
                    let mut buf = Vec::with_capacity(row_bytes * height as usize);
                    for row in 0..height as usize {
                        let start = row * stride;
                        buf.extend_from_slice(&raw[start..start + row_bytes]);
                    }
                    buf
                };

                return FrameResult::Frame(DecodedFrame { data, width, height });
            }
        }
        FrameResult::EndOfStream
    }

    fn reconnect(&mut self, output_width: u32, output_height: u32) -> std::result::Result<(), String> {
        let new = Self::new(&self.source_type, output_width, output_height)?;
        *self = new;
        Ok(())
    }
}

// Safety: Used from a single dedicated OS thread only.
unsafe impl Send for FfmpegDecoder {}

// ============================================================================
// JPEG Encoder (using `image` crate — fast, pure Rust)
// ============================================================================

/// Encode RGB24 data to JPEG using the `image` crate (same as yolo-video-v2).
/// Much faster than ffmpeg MJPEG encoder — no color space conversion needed.
fn encode_jpeg(rgb_data: &[u8], width: u32, height: u32, quality: u8) -> Vec<u8> {
    let img = match image::RgbImage::from_raw(width, height, rgb_data.to_vec()) {
        Some(img) => img,
        None => return Vec::new(),
    };
    let mut buffer = Vec::with_capacity((width * height) as usize / 4);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, quality);
    let _ = encoder.encode(
        img.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    );
    buffer
}

// ============================================================================
// Types
// ============================================================================

/// Player configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct PlayerConfig {
    source_url: String,
    target_fps: u32,
    output_width: u32,
    output_height: u32,
    video_bitrate: u32,
    loop_file: bool,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            source_url: String::new(),
            target_fps: 24,
            output_width: 640,
            output_height: 480,
            video_bitrate: 1500,
            loop_file: true,
        }
    }
}

/// Active stream state
struct ActiveStream {
    config: PlayerConfig,
    running: bool,
    frame_count: u64,
    fps: f32,
    bytes_sent: u64,
    started_at: Instant,
    push_task: Option<std::thread::JoinHandle<()>>,
}

// ============================================================================
// Stream Registry
// ============================================================================

struct StreamRegistry {
    streams: HashMap<String, Arc<Mutex<ActiveStream>>>,
}

impl StreamRegistry {
    fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }
}

static REGISTRY: std::sync::OnceLock<Mutex<StreamRegistry>> = std::sync::OnceLock::new();

fn get_registry() -> &'static Mutex<StreamRegistry> {
    REGISTRY.get_or_init(|| Mutex::new(StreamRegistry::new()))
}

// ============================================================================
// Extension
// ============================================================================

pub struct StreamPlayerExtension;

impl StreamPlayerExtension {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StreamPlayerExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for StreamPlayerExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "stream-player",
                "Stream Player",
                "2.0.0",
            )
            .with_description("Universal video player for RTSP, RTMP, HLS, and local file playback")
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
                name: "total_frames".to_string(),
                display_name: "Total Frames".to_string(),
                data_type: MetricDataType::Integer,
                unit: "frames".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_bytes_sent".to_string(),
                display_name: "Total Bytes Sent".to_string(),
                data_type: MetricDataType::Integer,
                unit: "bytes".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "list_sources".to_string(),
                display_name: "List Sources".to_string(),
                description: "List supported video source formats".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![json!({})],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "get_player_info".to_string(),
                display_name: "Get Player Info".to_string(),
                description: "Get current player status and configuration".to_string(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![json!({})],
                parameter_groups: vec![],
            },
        ]
    }

    async fn execute_command(&self, command: &str, _args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "list_sources" => {
                Ok(json!({
                    "supported": [
                        {"protocol": "RTSP", "example": "rtsp://host:554/stream"},
                        {"protocol": "RTMP", "example": "rtmp://host/live/stream"},
                        {"protocol": "HLS", "example": "hls://host/live/stream.m3u8"},
                        {"protocol": "HTTP", "example": "http://host/video.mp4"},
                        {"protocol": "File", "example": "file:///path/to/video.mp4"},
                    ]
                }))
            }
            "get_player_info" => {
                let registry = get_registry().lock();
                let streams: Vec<_> = registry.streams.iter().map(|(id, s)| {
                    let stream = s.lock();
                    json!({
                        "session_id": id,
                        "source_url": stream.config.source_url,
                        "running": stream.running,
                        "frame_count": stream.frame_count,
                        "fps": (stream.fps as u32),
                        "bytes_sent": stream.bytes_sent,
                    })
                }).collect();
                Ok(json!({
                    "active_count": registry.streams.len(),
                    "streams": streams,
                }))
            }
            "configure" => {
                // Accept config silently - can be extended for real config handling
                Ok(json!({"status": "ok"}))
            }

            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let registry = get_registry().lock();

        let mut total_frames: i64 = 0;
        let mut total_bytes: i64 = 0;
        for stream_arc in registry.streams.values() {
            let s = stream_arc.lock();
            total_frames += s.frame_count as i64;
            total_bytes += s.bytes_sent as i64;
        }

        Ok(vec![
            ExtensionMetricValue {
                name: "active_streams".to_string(),
                value: ParamMetricValue::Integer(registry.streams.len() as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_frames".to_string(),
                value: ParamMetricValue::Integer(total_frames),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_bytes_sent".to_string(),
                value: ParamMetricValue::Integer(total_bytes),
                timestamp: now,
            },
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    // ========================================================================
    // Push Mode Streaming
    // ========================================================================

    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability {
            direction: StreamDirection::Bidirectional,
            mode: StreamMode::Push,
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
        let config: PlayerConfig = serde_json::from_value(session.config.clone())
            .unwrap_or_default();

        if config.source_url.is_empty() {
            return Err(ExtensionError::InvalidArguments("source_url is required".to_string()));
        }

        let session_id = session.id.clone();

        // Clean up existing session if any
        {
            let registry = get_registry().lock();
            if let Some(old) = registry.streams.get(&session_id) {
                old.lock().running = false;
            }
        }

        let stream = ActiveStream {
            config,
            running: true,
            frame_count: 0,
            fps: 0.0,
            bytes_sent: 0,
            started_at: Instant::now(),
            push_task: None,
        };

        {
            let mut registry = get_registry().lock();
            registry.streams.insert(session_id.clone(), Arc::new(Mutex::new(stream)));
        }

        tracing::info!("[StreamPlayer] Session initialized: {}", session_id);
        Ok(())
    }

    fn set_output_sender(&self, _sender: Arc<tokio::sync::mpsc::Sender<PushOutputMessage>>) {
        // No-op: Push mode uses send_push_output() directly via FFI
    }

    async fn start_push(&self, session_id: &str) -> Result<()> {
        // Check if already running
        {
            let registry = get_registry().lock();
            if let Some(stream) = registry.streams.get(session_id) {
                let s = stream.lock();
                if s.push_task.is_some() {
                    return Ok(());
                }
            }
        }

        let config = {
            let registry = get_registry().lock();
            registry.streams.get(session_id)
                .map(|s| s.lock().config.clone())
        };

        let config = match config {
            Some(c) => c,
            None => return Err(ExtensionError::SessionNotFound(session_id.to_string())),
        };

        let source_url = config.source_url.clone();

        // Parse source URL
        let source_type = match parse_source_url(&source_url) {
            Ok(st) => st,
            Err(e) => {
                return Err(ExtensionError::ExecutionFailed(format!("Invalid source URL: {}", e)));
            }
        };

        let sid = session_id.to_string();
        let target_fps = config.target_fps.max(1);
        let output_width = config.output_width;
        let output_height = config.output_height;
        let loop_file = config.loop_file && is_file_source(&source_url);

        tracing::info!("[StreamPlayer] Starting push: {} ({})", sid, source_url);

        let task_handle = std::thread::spawn(move || {
            let mut sequence = 0u64;
            let frame_duration = Duration::from_millis(1000 / target_fps as u64);
            let mut reconnect_count = 0u32;
            const MAX_RECONNECT: u32 = 3;

            // Frame skipping: if we fall behind, skip decode frames to catch up
            let mut frames_to_skip = 0u32;
            let mut last_push_time = Instant::now();

            // Open decoder
            let mut decoder = match FfmpegDecoder::new(&source_type, output_width, output_height) {
                Ok(d) => {
                    tracing::info!("[StreamPlayer] Connected: {}", source_url);
                    let _ = send_push_output(
                        &PushOutputMessage::json(&sid, sequence, json!({
                            "type": "status", "status": "streaming",
                            "width": output_width, "height": output_height,
                        })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                    );
                    d
                }
                Err(e) => {
                    tracing::error!("[StreamPlayer] Connection failed: {}", e);
                    let _ = send_push_output(
                        &PushOutputMessage::json(&sid, sequence, json!({
                            "type": "error", "message": format!("Connection failed: {}", e)
                        })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                    );
                    return;
                }
            };

            loop {
                // Check if still running
                let should_continue = {
                    let registry = get_registry().lock();
                    registry.streams.get(&sid).map_or(false, |s| s.lock().running)
                };
                if !should_continue {
                    break;
                }

                let frame_start = Instant::now();

                // Decode next frame (blocking)
                match decoder.next_frame() {
                    FrameResult::Frame(decoded) => {
                        reconnect_count = 0;

                        // Skip frames if we're behind schedule
                        if frames_to_skip > 0 {
                            frames_to_skip -= 1;
                            continue;
                        }

                        // Encode to JPEG (pure Rust, fast)
                        let encode_start = Instant::now();
                        let jpeg_data = encode_jpeg(&decoded.data, decoded.width, decoded.height, 55);
                        let encode_ms = encode_start.elapsed().as_millis();
                        if jpeg_data.is_empty() {
                            tracing::warn!("[StreamPlayer] JPEG encode produced empty output");
                            continue;
                        }

                        if sequence < 5 || sequence % 100 == 0 {
                            eprintln!("[StreamPlayer] frame {} encode={}ms jpeg={}KB", sequence, encode_ms, jpeg_data.len() / 1024);
                        }

                        // Update stats
                        let data_len = jpeg_data.len() as u64;
                        {
                            let registry = get_registry().lock();
                            if let Some(stream) = registry.streams.get(&sid) {
                                let mut s = stream.lock();
                                s.frame_count += 1;
                                s.bytes_sent += data_len;
                                let elapsed = s.started_at.elapsed().as_secs_f32();
                                if elapsed > 0.0 {
                                    s.fps = s.frame_count as f32 / elapsed;
                                }
                            }
                        }

                        // Push JPEG frame to frontend (no metadata to reduce JSON overhead)
                        let output = PushOutputMessage::image_jpeg(&sid, sequence, jpeg_data);
                        let push_start = Instant::now();

                        match send_push_output(&output) {
                            Ok(_) => {
                                let push_ms = push_start.elapsed().as_millis();
                                if sequence < 5 || sequence % 100 == 0 {
                                    eprintln!("[StreamPlayer] frame {} push={}ms total={}ms", sequence, push_ms, frame_start.elapsed().as_millis());
                                }
                                sequence += 1;
                                // Detect if we're falling behind
                                let push_elapsed = last_push_time.elapsed();
                                last_push_time = Instant::now();
                                if push_elapsed > frame_duration * 3 {
                                    // Behind by 3+ frame durations — skip next N frames to catch up
                                    let skip = (push_elapsed.as_millis() / frame_duration.as_millis()).min(10) as u32;
                                    tracing::warn!(
                                        "[StreamPlayer] Behind by {:?}, skipping {} frames",
                                        push_elapsed, skip
                                    );
                                    frames_to_skip = skip;
                                }
                            }
                            Err(e) => {
                                tracing::warn!("[StreamPlayer] Push failed: {}", e);
                                break;
                            }
                        }

                        // Frame rate throttling
                        let elapsed = frame_start.elapsed();
                        if elapsed < frame_duration {
                            std::thread::sleep(frame_duration - elapsed);
                        }
                    }
                    FrameResult::EndOfStream => {
                        // File sources: loop playback
                        if loop_file {
                            tracing::info!("[StreamPlayer] File ended, looping: {}", sid);
                            match decoder.reconnect(output_width, output_height) {
                                Ok(()) => {
                                    let _ = send_push_output(
                                        &PushOutputMessage::json(&sid, sequence, json!({
                                            "type": "status", "status": "looping"
                                        })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                                    );
                                    continue;
                                }
                                Err(e) => {
                                    tracing::error!("[StreamPlayer] Loop reconnect failed: {}", e);
                                }
                            }
                        }

                        // Network sources (RTSP/RTMP/HLS): auto-reconnect
                        if !loop_file {
                            reconnect_count += 1;
                            if reconnect_count <= MAX_RECONNECT {
                                let backoff = Duration::from_secs(1 << (reconnect_count - 1).min(3));
                                tracing::warn!(
                                    "[StreamPlayer] Stream ended, reconnecting in {:?} ({}/{})",
                                    backoff, reconnect_count, MAX_RECONNECT
                                );
                                let _ = send_push_output(
                                    &PushOutputMessage::json(&sid, sequence, json!({
                                        "type": "status", "status": "reconnecting"
                                    })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                                );
                                std::thread::sleep(backoff);
                                match decoder.reconnect(output_width, output_height) {
                                    Ok(()) => {
                                        tracing::info!("[StreamPlayer] Reconnected: {}", sid);
                                        let _ = send_push_output(
                                            &PushOutputMessage::json(&sid, sequence, json!({
                                                "type": "status", "status": "streaming"
                                            })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                                        );
                                        continue;
                                    }
                                    Err(e) => {
                                        tracing::error!("[StreamPlayer] Reconnect failed: {}", e);
                                    }
                                }
                            }
                        }

                        let _ = send_push_output(
                            &PushOutputMessage::json(&sid, sequence, json!({
                                "type": "status", "status": "ended"
                            })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                        );
                        tracing::info!("[StreamPlayer] Stream ended: {}", sid);
                        break;
                    }
                    FrameResult::Error(e) => {
                        tracing::error!("[StreamPlayer] Frame error: {}", e);
                        reconnect_count += 1;
                        if reconnect_count > MAX_RECONNECT {
                            let _ = send_push_output(
                                &PushOutputMessage::json(&sid, sequence, json!({
                                    "type": "error",
                                    "message": format!("Stream error after {} retries: {}", MAX_RECONNECT, e)
                                })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                            );
                            break;
                        }
                        let backoff = Duration::from_secs(1 << (reconnect_count - 1));
                        tracing::info!("[StreamPlayer] Reconnecting in {:?} ({}/{})", backoff, reconnect_count, MAX_RECONNECT);
                        let _ = send_push_output(
                            &PushOutputMessage::json(&sid, sequence, json!({
                                "type": "status", "status": "reconnecting"
                            })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                        );
                        std::thread::sleep(backoff);

                        match decoder.reconnect(output_width, output_height) {
                            Ok(()) => {
                                tracing::info!("[StreamPlayer] Reconnected: {}", sid);
                                let _ = send_push_output(
                                    &PushOutputMessage::json(&sid, sequence, json!({
                                        "type": "status", "status": "streaming"
                                    })).unwrap_or_else(|_| PushOutputMessage::image_jpeg(&sid, sequence, vec![]))
                                );
                            }
                            Err(re) => {
                                tracing::error!("[StreamPlayer] Reconnect failed: {}", re);
                            }
                        }
                    }
                }
            }

            tracing::info!("[StreamPlayer] Push task ended. Frames: {}", sequence);
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

        if let Some(handle) = task_handle {
            drop(handle);
            tracing::info!("[StreamPlayer] Push task stopping: {}", session_id);
        }

        Ok(())
    }

    async fn process_session_chunk(
        &self,
        _session_id: &str,
        _chunk: neomind_extension_sdk::DataChunk,
    ) -> Result<StreamResult> {
        Err(ExtensionError::NotSupported("process_session_chunk not used".to_string()))
    }

    async fn close_session(&self, session_id: &str) -> Result<SessionStats> {
        {
            let mut registry = get_registry().lock();
            if let Some(stream) = registry.streams.remove(session_id) {
                stream.lock().running = false;
            }
        }
        tracing::info!("[StreamPlayer] Session closed: {}", session_id);
        Ok(SessionStats::default())
    }
}

// FFI export
neomind_extension_sdk::neomind_export!(StreamPlayerExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rtsp() {
        let st = parse_source_url("rtsp://192.168.1.100:554/stream").unwrap();
        assert!(matches!(st, SourceType::RTSP { .. }));
    }

    #[test]
    fn test_parse_rtmp() {
        let st = parse_source_url("rtmp://host/live/stream").unwrap();
        assert!(matches!(st, SourceType::RTMP { .. }));
    }

    #[test]
    fn test_parse_hls() {
        let st = parse_source_url("hls://example.com/live/stream.m3u8").unwrap();
        assert!(matches!(st, SourceType::HLS { .. }));
    }

    #[test]
    fn test_parse_file() {
        let st = parse_source_url("file:///path/to/video.mp4").unwrap();
        assert!(matches!(st, SourceType::File { .. }));
    }

    #[test]
    fn test_parse_http() {
        let st = parse_source_url("http://example.com/video.mp4").unwrap();
        assert!(matches!(st, SourceType::Http { .. }));
    }

    #[test]
    fn test_parse_unsupported() {
        assert!(parse_source_url("ftp://example.com/video").is_err());
    }

    #[test]
    fn test_extension_metadata() {
        let ext = StreamPlayerExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "stream-player");
        assert_eq!(meta.name, "Stream Player");
    }

    #[test]
    fn test_default_config() {
        let config = PlayerConfig::default();
        assert_eq!(config.target_fps, 24);
        assert_eq!(config.output_width, 640);
        assert_eq!(config.output_height, 480);
        assert_eq!(config.video_bitrate, 1500);
        assert!(config.loop_file);
    }
}
