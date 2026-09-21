//! Video Source Abstraction Layer (V2)
//!
//! Supports multiple video stream protocols through a unified interface.
//! FFmpeg-backed sources handle RTSP/RTMP/HLS/File decoding on dedicated threads.

use ffmpeg_next as ff;

/// Video source information
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub codec: String,
    pub is_live: bool,
}

/// Frame from video source
#[derive(Debug)]
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: i64,
    pub frame_number: u64,
}

/// Result of frame read operation
pub enum FrameResult {
    Frame(VideoFrame),
    EndOfStream,
    NotReady,
    Error(String),
}

/// Video source trait
pub trait VideoSource {
    fn info(&self) -> &SourceInfo;
    fn is_active(&self) -> bool;
}

/// Parse source URL into source type
pub fn parse_source_url(url: &str) -> Result<SourceType, String> {
    if url.starts_with("camera://") {
        let parts = url.split("://").nth(1).unwrap_or("0");
        let device_index = parts.split('?').next()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        Ok(SourceType::Camera {
            device_index,
            width: 640,
            height: 480,
            fps: 30,
        })
    } else if url.starts_with("rtsp://") {
        Ok(SourceType::RTSP {
            url: url.to_string(),
            transport: RtspTransport::Tcp,
            timeout_secs: 10,
        })
    } else if url.starts_with("rtmp://") {
        Ok(SourceType::RTMP {
            url: url.to_string(),
            app: "live".to_string(),
            stream_key: "stream".to_string(),
        })
    } else if url.starts_with("hls://") || url.contains(".m3u8") {
        Ok(SourceType::HLS {
            url: url.to_string(),
            playlist_reload_secs: 5,
        })
    } else if url.starts_with("file://") {
        let path = url[7..].to_string();
        Ok(SourceType::File {
            path,
            loop_: false,
            start_time_secs: 0.0,
        })
    } else if url.starts_with("screen://") {
        let display = url.split("://").nth(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        Ok(SourceType::Screen {
            display,
            width: 1920,
            height: 1080,
        })
    } else if url.starts_with("http://") || url.starts_with("https://") {
        // HTTP/HTTPS URLs: MP4, MKV, FLV, M3U8, etc. — FFmpeg handles them all
        Ok(SourceType::File {
            path: url.to_string(),
            loop_: false,
            start_time_secs: 0.0,
        })
    } else {
        // Default to camera
        Ok(SourceType::Camera {
            device_index: 0,
            width: 640,
            height: 480,
            fps: 30,
        })
    }
}

/// Supported source types
#[derive(Debug, Clone, PartialEq)]
pub enum SourceType {
    Camera { device_index: i32, width: u32, height: u32, fps: u32 },
    RTSP { url: String, transport: RtspTransport, timeout_secs: u64 },
    RTMP { url: String, app: String, stream_key: String },
    HLS { url: String, playlist_reload_secs: u64 },
    File { path: String, loop_: bool, start_time_secs: f32 },
    Screen { display: u32, width: u32, height: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RtspTransport {
    Tcp,
    Udp,
    Auto,
}

// ---------------------------------------------------------------------------
// FFmpeg-based video source
// ---------------------------------------------------------------------------

/// FFmpeg-based video source for network streams and local files.
///
/// **Thread safety**: `next_frame()` is blocking I/O. Must be called from a
/// dedicated OS thread, NOT from inside a tokio async context.
pub struct FfmpegVideoSource {
    info: SourceInfo,
    input_ctx: ff::format::context::Input,
    decoder: ff::decoder::Video,
    scaler: ff::software::scaling::Context,
    stream_index: usize,
    active: bool,
    frame_count: u64,
    source_type: SourceType,
}

impl FfmpegVideoSource {
    pub fn new(source_type: &SourceType) -> Result<Self, String> {
        let url = match source_type {
            SourceType::RTSP { url, .. } => url.as_str(),
            SourceType::RTMP { url, .. } => url.as_str(),
            SourceType::HLS { url, .. } => url.as_str(),
            SourceType::File { path, .. } => path.as_str(),
            _ => return Err("Unsupported source type for FFmpeg".to_string()),
        };

        // Build FFmpeg options for network streams
        let mut input_opts = ff::Dictionary::new();
        if matches!(source_type, SourceType::RTSP { .. }) {
            input_opts.set("rtsp_transport", "tcp");
            input_opts.set("stimeout", "5000000"); // 5s in microseconds
        }
        if matches!(source_type, SourceType::RTSP { .. } | SourceType::RTMP { .. } | SourceType::HLS { .. }) {
            input_opts.set("analyzeduration", "2000000");
            input_opts.set("probesize", "1000000");
        }

        let input_ctx = ff::format::input_with_dictionary(&url, input_opts)
            .map_err(|e| format!("Failed to open stream '{}': {}", url, e))?;

        // Find best video stream
        let stream = input_ctx.streams().best(ff::media::Type::Video)
            .ok_or("No video stream found")?;
        let stream_index = stream.index();

        let context = ff::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| format!("Failed to create codec context: {}", e))?;
        let decoder = context.decoder().video()
            .map_err(|e| format!("Failed to open video decoder: {}", e))?;

        let width = decoder.width();
        let height = decoder.height();
        let fps = stream.avg_frame_rate();
        let fps_float = if fps.numerator() > 0 && fps.denominator() > 0 {
            fps.numerator() as f32 / fps.denominator() as f32
        } else {
            25.0
        };

        // Cap output resolution at 960x540 (16:9). Decoding + YUV→RGB conversion is one of
        // the biggest CPU costs in the pipeline; doing the downscale inside FFmpeg's scaler
        // avoids a second CPU resize later and keeps the rest of the pipeline working on a
        // small image. If the source is already smaller, leave it untouched.
        const OUT_W: u32 = 960;
        const OUT_H: u32 = 540;
        let (out_w, out_h) = if width > OUT_W || height > OUT_H {
            (OUT_W, OUT_H)
        } else {
            (width, height)
        };

        // Create scaler: decode format → RGB24 (with optional downscale)
        let scaler = ff::software::scaling::Context::get(
            decoder.format(),
            width, height,
            ff::format::Pixel::RGB24,
            out_w, out_h,
            ff::software::scaling::flag::Flags::BILINEAR,
        ).map_err(|e| format!("Failed to create scaler: {}", e))?;

        let codec_name = stream.parameters().id().name().to_string();

        Ok(Self {
            info: SourceInfo {
                width: out_w,
                height: out_h,
                fps: fps_float,
                codec: codec_name,
                is_live: !matches!(source_type, SourceType::File { .. }),
            },
            input_ctx,
            decoder,
            scaler,
            stream_index,
            active: true,
            frame_count: 0,
            source_type: source_type.clone(),
        })
    }

    /// Decode next frame. **BLOCKING** — call from a dedicated thread only.
    pub fn next_frame(&mut self) -> FrameResult {
        let mut decoded = ff::frame::Video::empty();

        while let Some((stream, pkt)) = self.input_ctx.packets().next() {
            if stream.index() != self.stream_index {
                continue;
            }

            if self.decoder.send_packet(&pkt).is_err() {
                continue;
            }

            while self.decoder.receive_frame(&mut decoded).is_ok() {
                // Scale decoded frame to RGB24
                let mut rgb_frame = ff::frame::Video::empty();
                if self.scaler.run(&decoded, &mut rgb_frame).is_err() {
                    continue;
                }

                self.frame_count += 1;

                let width = rgb_frame.width();
                let height = rgb_frame.height();
                let stride = rgb_frame.stride(0);
                let row_bytes = (width as usize) * 3;

                // Strip row padding if stride != width*3
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

                return FrameResult::Frame(VideoFrame {
                    data,
                    width,
                    height,
                    timestamp: decoded.timestamp().unwrap_or(0),
                    frame_number: self.frame_count,
                });
            }
        }

        // Iterator ended — either EOF or read error
        self.active = false;
        FrameResult::EndOfStream
    }

    /// Close and reopen the stream (for reconnection).
    pub fn reconnect(&mut self) -> Result<(), String> {
        self.active = false;
        let new_source = Self::new(&self.source_type)?;
        *self = new_source;
        Ok(())
    }

    /// Seek back to the start of the source for seamless looping.
    ///
    /// Unlike `reconnect()`, this does NOT reopen the file / re-download the
    /// HTTP resource — it reuses the already-open input context, decoder, and
    /// scaler, so it completes in milliseconds instead of 15-30s.
    ///
    /// Works for local files and most HTTP sources (when the server supports
    /// Range requests). Live streams (RTSP/RTMP/HLS) don't support seeking;
    /// caller should fall back to `reconnect()` on `Err`.
    pub fn seek_to_start(&mut self) -> Result<(), String> {
        // Flush decoder buffers — otherwise stale frames buffered before EOF
        // would be emitted first after seeking, causing a brief glitch.
        let _ = self.decoder.flush();
        // Seek to timestamp 0 (beginning of stream). ffmpeg-next's `seek`
        // takes a Range<i64> as the second arg — `..` means accept any
        // resulting position (no strict boundary).
        self.input_ctx
            .seek(0, ..)
            .map_err(|e| format!("seek to start failed: {}", e))?;
        self.frame_count = 0;
        self.active = true;
        Ok(())
    }

    pub fn close(&mut self) {
        self.active = false;
    }
}

impl VideoSource for FfmpegVideoSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

// Safety: FfmpegVideoSource is used from a single dedicated OS thread.
// FFmpeg contexts are not thread-safe and must not be shared across threads.
// The `Send` impl allows moving the source to the dedicated thread at creation time.
unsafe impl Send for FfmpegVideoSource {}

impl VideoFrame {
    /// Convert raw RGB24 data to `image::RgbImage`.
    /// Returns `None` if data length doesn't match `width * height * 3`.
    pub fn to_rgb_image(self) -> Option<image::RgbImage> {
        image::RgbImage::from_raw(self.width, self.height, self.data)
    }
}

// ---------------------------------------------------------------------------
// GStreamer NVDEC hardware decode (Linux/Jetson only)
// ---------------------------------------------------------------------------

#[cfg(feature = "nvdec")]
pub mod nvdec {
    use super::{FrameResult, SourceInfo, SourceType, VideoFrame};
    use gstreamer as gst;
    use gstreamer_app as gst_app;
    use gstreamer_app::prelude::*;

    /// GStreamer-backed hardware decoder using Jetson's `nvv4l2decoder` (NVDEC)
    /// + `nvvidconv` (hardware colorspace conversion / scaling).
    ///
    /// Falls back automatically if `nvv4l2decoder` is not installed — the
    /// caller should check `nvdec_available()` first.
    pub struct GstreamerNvdecSource {
        pipeline: gst::Pipeline,
        appsink: gst_app::AppSink,
        info: SourceInfo,
        frame_count: u64,
        active: bool,
    }

    /// Returns true if both `nvv4l2decoder` and `nvvidconv` are installed.
    pub fn nvdec_available() -> bool {
        if gst::init().is_err() {
            return false;
        }
        gst::ElementFactory::find("nvv4l2decoder").is_some()
            && gst::ElementFactory::find("nvvidconv").is_some()
    }

    impl GstreamerNvdecSource {
        pub fn new(source_type: &SourceType) -> Result<Self, String> {
            gst::init().map_err(|e| format!("GStreamer init: {}", e))?;

            // Boost nvv4l2decoder rank so uridecodebin prefers it over software decoders
            if let Some(factory) = gst::ElementFactory::find("nvv4l2decoder") {
                factory.set_rank(gst::Rank::Primary + 1);
            }

            let (uri, is_live) = match source_type {
                SourceType::File { path, .. } => {
                    let uri = if path.starts_with("http")
                        || path.starts_with("file://")
                        || path.starts_with("rtsp://")
                    {
                        path.clone()
                    } else {
                        format!("file://{}", path)
                    };
                    (uri, false)
                }
                SourceType::RTSP { url, .. }
                | SourceType::HLS { url, .. }
                | SourceType::RTMP { url, .. } => (url.clone(), true),
                _ => return Err("Unsupported source type for NVDEC".to_string()),
            };

            const OUT_W: i32 = 960;
            const OUT_H: i32 = 540;
            // uridecodebin handles any container (mp4/mkv/ts/http/rtsp).
            // nvvidconv: NVMM → system memory + hardware scale to 960x540.
            // videoconvert: fast SIMD I420→RGB (nvvidconv can't do plain RGB from NVMM).
            let pipeline_str = format!(
                "uridecodebin uri={uri} name=src ! \
                 nvvidconv ! video/x-raw,format=I420,width={w},height={h} ! \
                 videoconvert ! video/x-raw,format=RGB ! \
                 appsink name=sink sync=false max-buffers=2 drop=true",
                uri = uri,
                w = OUT_W,
                h = OUT_H,
            );

            let pipeline = gst::parse_launch(&pipeline_str)
                .map_err(|e| format!("Pipeline creation failed: {}", e))?;
            let pipeline = pipeline
                .downcast::<gst::Pipeline>()
                .map_err(|_| "Failed to downcast to Pipeline".to_string())?;

            let appsink = pipeline
                .by_name("sink")
                .ok_or("appsink 'sink' not found in pipeline")?
                .dynamic_cast::<gst_app::AppSink>()
                .map_err(|_| "sink is not an AppSink".to_string())?;

            // Wait for pipeline to preroll (up to 10s) — needed for RTSP/HTTP
            pipeline
                .set_state(gst::State::Playing)
                .map_err(|e| format!("Failed to start pipeline: {:?}", e))?;

            Ok(Self {
                pipeline,
                appsink,
                info: SourceInfo {
                    width: OUT_W as u32,
                    height: OUT_H as u32,
                    fps: 25.0, // updated on first frame from caps
                    codec: "NVDEC".to_string(),
                    is_live,
                },
                frame_count: 0,
                active: true,
            })
        }

        /// Blocking frame pull from appsink. Returns RGB24 data.
        pub fn next_frame(&mut self) -> FrameResult {
            let sample = if self.frame_count == 0 {
                self.appsink.pull_preroll()
            } else {
                self.appsink.pull_sample()
            };

            match sample {
                Ok(s) => {
                    let caps = match s.caps().and_then(|c| c.structure(0)) {
                        Some(st) => st,
                        None => return FrameResult::Error("No caps in sample".to_string()),
                    };
                    let width = caps.get::<i32>("width").unwrap_or(960) as u32;
                    let height = caps.get::<i32>("height").unwrap_or(540) as u32;

                    let buffer = match s.buffer() {
                        Some(b) => b,
                        None => return FrameResult::Error("No buffer in sample".to_string()),
                    };

                    let map = match buffer.map_readable() {
                        Ok(m) => m,
                        Err(e) => {
                            return FrameResult::Error(format!("Failed to map buffer: {:?}", e))
                        }
                    };

                    self.frame_count += 1;
                    FrameResult::Frame(VideoFrame {
                        data: map.to_vec(),
                        width,
                        height,
                        timestamp: buffer.pts().map(|t| t.useconds() as i64).unwrap_or(0),
                        frame_number: self.frame_count,
                    })
                }
                Err(_) => {
                    // pull_sample returns Err when no buffer is available (EOS or not yet ready)
                    if self.appsink.is_eos() {
                        self.active = false;
                        FrameResult::EndOfStream
                    } else {
                        FrameResult::NotReady
                    }
                }
            }
        }

        pub fn close(&mut self) {
            if self.active {
                self.active = false;
                let _ = self.pipeline.set_state(gst::State::Null);
            }
        }

        pub fn info(&self) -> &SourceInfo {
            &self.info
        }
    }

    impl Drop for GstreamerNvdecSource {
        fn drop(&mut self) {
            self.close();
        }
    }

    // Safety: GstreamerNvdecSource is used from a single dedicated OS thread.
    // GStreamer elements are not thread-safe; the Send impl allows moving the
    // source to the dedicated thread at creation time.
    unsafe impl Send for GstreamerNvdecSource {}
}

/// Factory for creating video sources
pub struct SourceFactory;

impl SourceFactory {
    pub fn create(source_type: &SourceType) -> Result<Box<dyn VideoSource>, String> {
        match source_type {
            SourceType::RTSP { .. } | SourceType::RTMP { .. } | SourceType::HLS { .. } |
            SourceType::File { .. } => {
                let source = FfmpegVideoSource::new(source_type)?;
                Ok(Box::new(source))
            }
            SourceType::Camera { .. } => {
                Err("Camera source uses frontend capture, not FFmpeg".to_string())
            }
            SourceType::Screen { .. } => {
                Err("Screen capture not yet supported".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_camera_url() {
        let result = parse_source_url("camera://0");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::Camera { device_index, .. } => {
                assert_eq!(device_index, 0);
            }
            _ => panic!("Expected Camera type"),
        }
    }

    #[test]
    fn test_parse_rtsp_url() {
        let result = parse_source_url("rtsp://192.168.1.100:554/stream");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::RTSP { url, .. } => {
                assert_eq!(url, "rtsp://192.168.1.100:554/stream");
            }
            _ => panic!("Expected RTSP type"),
        }
    }

    #[test]
    fn test_parse_hls_url() {
        let result = parse_source_url("hls://example.com/live/stream.m3u8");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::HLS { url, .. } => {
                assert_eq!(url, "hls://example.com/live/stream.m3u8");
            }
            _ => panic!("Expected HLS type"),
        }
    }

    #[test]
    fn test_parse_file_url() {
        let result = parse_source_url("file:///path/to/video.mp4");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::File { path, .. } => {
                assert_eq!(path, "/path/to/video.mp4");
            }
            _ => panic!("Expected File type"),
        }
    }

    #[test]
    fn test_parse_unknown_url_defaults_to_camera() {
        let result = parse_source_url("unknown://test");
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::Camera { .. } => {}
            _ => panic!("Expected Camera type as default"),
        }
    }
}
