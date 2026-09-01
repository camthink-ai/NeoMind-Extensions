//! Video Source Abstraction Layer (V2)
//!
//! Supports multiple video stream protocols through a unified interface.
//! FFmpeg-backed sources handle RTSP/RTMP/HLS/File decoding on dedicated threads.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;

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
    if url.starts_with("camera://") || url.starts_with("usb://") || url.starts_with("v4l2://") {
        // Map to the raw UVC device node (bypasses Rockchip ISP).
        // Use /dev/video25 (first UVC node); support camera://N for index.
        let parts = url.split("://").nth(1).unwrap_or("0");
        let device_index = parts.split('?').next()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);
        // UVC nodes are typically video25+ on RK3576 (video0-11 are ISP)
        let device = if device_index > 0 {
            format!("/dev/video{}", 25 + device_index)
        } else {
            "/dev/video25".to_string()
        };
        // Treat as a File source — input_args_for adds -f v4l2 for device paths
        Ok(SourceType::File {
            path: device,
            loop_: false,
            start_time_secs: 0.0,
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
// ---------------------------------------------------------------------------
// FFmpeg-based video source
// ---------------------------------------------------------------------------

/// FFmpeg-based video source for network streams and local files.
///
/// Uses the system `ffmpeg`/`ffprobe` binaries as subprocesses instead of the
/// `ffmpeg-next` crate — this avoids the libclang/bindgen build-time dependency
/// that makes that crate painful to compile on constrained boards. Raw RGB24
/// frames are piped out of the child (`-f rawvideo -pix_fmt rgb24`).
///
/// **Thread safety**: `next_frame()` is blocking I/O. Must be called from a
/// dedicated OS thread, NOT from inside a tokio async context.
pub struct FfmpegVideoSource {
    info: SourceInfo,
    cmd_args: Vec<String>,
    child: Option<Child>,
    reader: Option<ChildStdout>,
    active: bool,
    frame_count: u64,
    frame_size: usize, // 0 for JPEG mode (variable-size frames)
    is_jpeg: bool,     // true = pipe carries MJPEG frames (hardware pipeline)
    jpeg_buf: Vec<u8>, // accumulation buffer for JPEG SOI/EOI parsing
    started: std::time::Instant,
    stderr_tail: Arc<parking_lot::Mutex<String>>,
}

/// Locate a binary by trying a few well-known absolute paths, falling back to
/// PATH lookup (so the runner's PATH doesn't have to include ffmpeg's dir).
fn find_binary(name: &str) -> String {
    for dir in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
        let candidate = format!("{}/{}", dir, name);
        if std::path::Path::new(&candidate).exists() {
            return candidate;
        }
    }
    name.to_string()
}

/// Whether the system ffmpeg build exposes Rockchip MPP hardware decode
/// (RK3576/RK3588 boards). Detection is cached; force-disable with
/// `VIDEO_VLM_HW_DECODE=0` if a particular stream misbehaves under rkmpp.
fn rkmpp_supported() -> bool {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHE.get_or_init(|| {
        if std::env::var("VIDEO_VLM_HW_DECODE")
            .map(|v| v == "0" || v == "false")
            .unwrap_or(false)
        {
            return false;
        }
        // Only Rockchip Linux boards carry rkmpp; skip the probe elsewhere.
        if !(cfg!(target_os = "linux") && cfg!(target_arch = "aarch64")) {
            return false;
        }
        Command::new(find_binary("ffmpeg"))
            .arg("-hwaccels")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("rkmpp"))
            .unwrap_or(false)
    })
}

/// FFmpeg input-side options, mirroring the old ffmpeg-next `Dictionary` setup.
fn input_args_for(source_type: &SourceType, url: &str) -> Vec<String> {
    let mut args = Vec::new();
    // Rockchip MPP hardware decode: offloads H.264/HEVC from the CPU to the
    // SoC video decoder. `-hwaccel_output_format nv12` forces a system-memory
    // nv12 transfer so the downstream scale/rawvideo filters work unchanged.
    // Only added when the system ffmpeg actually reports rkmpp support.
    // v4l2 live MJPEG inputs are EXCLUDED: the rkmpp decoder hangs on them
    // (0 frames) — software MJPEG decode at 720p is cheap on RK3576.
    if rkmpp_supported() && !url.starts_with("/dev/video") {
        args.push("-hwaccel".to_string());
        args.push("rkmpp".to_string());
        args.push("-hwaccel_output_format".to_string());
        args.push("nv12".to_string());
    }
    if matches!(source_type, SourceType::RTSP { .. }) {
        args.push("-rtsp_transport".to_string());
        args.push("tcp".to_string());
        args.push("-stimeout".to_string());
        args.push("5000000".to_string()); // 5s in microseconds
    }
    if matches!(
        source_type,
        SourceType::RTSP { .. } | SourceType::RTMP { .. } | SourceType::HLS { .. }
    ) {
        args.push("-analyzeduration".to_string());
        args.push("2000000".to_string());
        args.push("-probesize".to_string());
        args.push("1000000".to_string());
    }
    // V4L2 USB camera: use raw UVC node with MJPEG input
    if url.starts_with("/dev/video") {
        args.push("-f".to_string());
        args.push("v4l2".to_string());
        args.push("-input_format".to_string());
        args.push("mjpeg".to_string());
        args.push("-video_size".to_string());
        args.push("1280x720".to_string());
    }

    // File sources: read at native frame rate (-re)
    if matches!(source_type, SourceType::File { path, .. } if !path.starts_with("/dev/")) {
        args.push("-re".to_string());
    }
    args.push("-i".to_string());
    args.push(url.to_string());
    args
}

/// FFmpeg output-side options.
///
/// On Rockchip boards with RGA + mjpeg_rkmpp available, use the fully
/// hardware-accelerated pipeline: RGA scale/convert + hardware MJPEG encode,
/// piping pre-compressed JPEG frames (~30KB each vs ~690KB rawvideo). The
/// Rust side then does zero JPEG encoding — frames are forwarded as-is.
///
/// Falls back to CPU rawvideo on platforms without Rockchip hardware.
/// Cap for the ffmpeg-side `fps` filter (push display rate). Dropping frames
/// in ffmpeg keeps the decode loop's frame grid aligned with the WS push
/// throttle — a skip-style throttle alone quantizes 24fps to 12/8/6..., it
/// can never deliver e.g. 20fps from a 24fps grid.
pub const PUSH_FPS_CAP: f64 = 20.0;

fn fps_filter_suffix(src_fps: f64) -> String {
    if src_fps > PUSH_FPS_CAP + 0.5 {
        format!(",fps={}", PUSH_FPS_CAP as u32)
    } else {
        String::new()
    }
}

fn output_args_hw(out_w: u32, out_h: u32, src_fps: f64) -> Option<Vec<String>> {
    if !rkmpp_supported() {
        return None;
    }
    // Check for RGA + mjpeg_rkmpp support at runtime (cached)
    static HW_ENC: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let supported = *HW_ENC.get_or_init(|| {
        let has_rga = Command::new(find_binary("ffmpeg"))
            .arg("-filters")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("scale_rkrga"))
            .unwrap_or(false);
        let has_mjpeg = Command::new(find_binary("ffmpeg"))
            .arg("-encoders")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("mjpeg_rkmpp"))
            .unwrap_or(false);
        has_rga && has_mjpeg
    });
    if !supported {
        return None;
    }
    Some(vec![
        "-an".to_string(),
        "-sn".to_string(),
        "-dn".to_string(),
        // CPU scale (RGA can't negotiate with rkmpp output formats on this
        // FFmpeg build), but hardware MJPEG encode eliminates the biggest
        // CPU cost. Output is JPEG frames (~30KB vs 690KB rawvideo).
        "-vf".to_string(),
        format!("scale={}:{}:flags=fast_bilinear{}", out_w, out_h, fps_filter_suffix(src_fps)),
        "-pix_fmt".to_string(),
        "yuvj420p".to_string(),
        "-c:v".to_string(),
        "mjpeg_rkmpp".to_string(),
        "-f".to_string(),
        "mjpeg".to_string(),
        "pipe:1".to_string(),
    ])
}

/// CPU fallback: raw RGB24 frames.
fn output_args(out_w: u32, out_h: u32, src_fps: f64) -> Vec<String> {
    vec![
        "-an".to_string(), // drop audio
        "-sn".to_string(), // drop subtitles
        "-dn".to_string(), // drop data
        "-vf".to_string(),
        format!("scale={}:{}:flags=fast_bilinear{}", out_w, out_h, fps_filter_suffix(src_fps)),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        "rgb24".to_string(),
        "pipe:1".to_string(),
    ]
}

/// Probe a source with `ffprobe` to learn geometry / framerate / codec.
fn probe_stream(source_type: &SourceType, url: &str) -> Result<(u32, u32, f32, String), String> {
    let mut args = vec![
        "-v".to_string(),
        "error".to_string(),
        "-select_streams".to_string(),
        "v:0".to_string(),
        "-show_entries".to_string(),
        "stream=width,height,avg_frame_rate,codec_name".to_string(),
        "-of".to_string(),
        "default=noprint_wrappers=1".to_string(),
    ];
    // RTSP needs the same transport options as the decode process.
    if matches!(source_type, SourceType::RTSP { .. }) {
        args.push("-rtsp_transport".to_string());
        args.push("tcp".to_string());
        args.push("-stimeout".to_string());
        args.push("5000000".to_string());
    }
    // v4l2 devices need an explicit format hint — without it ffprobe falls
    // back to negotiating rawvideo (YUYV), which is slow to read and reports
    // the wrong codec. Mirrors the capture args in input_args_for.
    if url.starts_with("/dev/video") {
        args.push("-f".to_string());
        args.push("v4l2".to_string());
        args.push("-input_format".to_string());
        args.push("mjpeg".to_string());
        args.push("-video_size".to_string());
        args.push("1280x720".to_string());
    }
    args.push(url.to_string());

    let out = Command::new(find_binary("ffprobe"))
        .args(&args)
        .output()
        .map_err(|e| format!("ffprobe failed to launch: {}", e))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        let msg = msg.trim();
        return Err(if msg.is_empty() {
            format!(
                "ffprobe could not open '{}' (exit {:?})",
                url,
                out.status.code()
            )
        } else {
            format!("ffprobe failed: {}", msg)
        });
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 25.0f32;
    let mut codec = "unknown".to_string();
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("width=") {
            width = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("height=") {
            height = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("codec_name=") {
            codec = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("avg_frame_rate=") {
            let v = v.trim();
            if let Some((n, d)) = v.split_once('/') {
                let n: f32 = n.trim().parse().unwrap_or(0.0);
                let d: f32 = d.trim().parse().unwrap_or(0.0);
                if n > 0.0 && d > 0.0 {
                    fps = n / d;
                }
            } else if let Ok(x) = v.parse::<f32>() {
                if x > 0.0 {
                    fps = x;
                }
            }
        }
    }
    if width == 0 || height == 0 {
        return Err(format!(
            "no video stream found in '{}' (ffprobe stdout: {})",
            url,
            text.trim()
        ));
    }
    Ok((width, height, fps, codec))
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

        // Probe first: we need the output geometry before we know the per-frame
        // byte count to read off the pipe.
        let (width, height, fps, codec) = probe_stream(source_type, url)?;

        // Fit the source inside 960x540 PRESERVING aspect ratio (even dims —
        // yuv420 needs them). Forcing 16:9 here used to squash portrait/long
        // videos: the display looked distorted and the VLM misread the scene.
        const OUT_W: u32 = 960;
        const OUT_H: u32 = 540;
        let scale = (OUT_W as f64 / width.max(1) as f64)
            .min(OUT_H as f64 / height.max(1) as f64)
            .min(1.0);
        let out_w = ((width as f64 * scale).round() as u32).max(2) & !1;
        let out_h = ((height as f64 * scale).round() as u32).max(2) & !1;

        let mut cmd_args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-nostdin".to_string(),
        ];
        cmd_args.extend(input_args_for(source_type, url));

        // Prefer hardware pipeline (RGA scale + mjpeg_rkmpp encode → JPEG pipe)
        let hw_args = output_args_hw(out_w, out_h, fps as f64);
        let (is_jpeg, frame_size) = match &hw_args {
            Some(args) => {
                cmd_args.extend(args.clone());
                (true, 0usize)
            }
            None => {
                cmd_args.extend(output_args(out_w, out_h, fps as f64));
                (false, (out_w as usize) * (out_h as usize) * 3)
            }
        };

        let mut src = Self {
            info: SourceInfo {
                width: out_w,
                height: out_h,
                fps,
                codec,
                is_live: !matches!(source_type, SourceType::File { .. }),
            },
            cmd_args,
            child: None,
            reader: None,
            active: false,
            frame_count: 0,
            frame_size,
            is_jpeg,
            jpeg_buf: Vec::with_capacity(64 * 1024),
            started: std::time::Instant::now(),
            stderr_tail: Arc::new(parking_lot::Mutex::new(String::new())),
        };
        src.spawn_child()?;
        src.active = true;
        Ok(src)
    }

    /// Start the ffmpeg child and hand back its piped stdout.
    fn spawn_child(&mut self) -> Result<(), String> {
        let mut cmd = Command::new(find_binary("ffmpeg"));
        cmd.args(&self.cmd_args);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn ffmpeg: {}", e))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "no stdout on ffmpeg child".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "no stderr on ffmpeg child".to_string())?;

        // Drain stderr into a rolling buffer so error causes can be surfaced in
        // reconnect/log messages. Without a reader the pipe buffer fills and
        // stalls ffmpeg (deadlock), even at -loglevel error.
        let tail = self.stderr_tail.clone();
        *tail.lock() = String::new();
        std::thread::Builder::new()
            .name("ffmpeg-stderr".to_string())
            .spawn(move || {
                let mut reader = stderr;
                let mut tmp = [0u8; 4096];
                loop {
                    match reader.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut t = tail.lock();
                            t.push_str(&String::from_utf8_lossy(&tmp[..n]));
                            let keep = t.len().saturating_sub(8192);
                            if keep > 0 {
                                t.drain(..keep);
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| format!("failed to spawn stderr reader thread: {}", e))?;

        self.child = Some(child);
        self.reader = Some(stdout);
        self.started = std::time::Instant::now();
        Ok(())
    }

    /// Kill the child and close the pipe. Idempotent.
    fn kill_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait(); // reap to avoid a zombie
        }
        self.reader = None;
    }

    /// Decode next frame. **BLOCKING** — call from a dedicated thread only.
    ///
    /// JPEG mode: reads variable-size MJPEG frames by scanning for SOI (FFD8)
    /// and EOI (FFD9) markers. Rawvideo mode: reads fixed-size RGB24 frames.
    pub fn next_frame(&mut self) -> FrameResult {
        if self.is_jpeg {
            let mut chunk = [0u8; 16384];
            loop {
                if let Some(frame) = Self::extract_jpeg(&mut self.jpeg_buf) {
                    self.frame_count += 1;
                    self.active = true;
                    return FrameResult::Frame(VideoFrame {
                        data: frame,
                        width: self.info.width,
                        height: self.info.height,
                        timestamp: self.started.elapsed().as_micros() as i64,
                        frame_number: self.frame_count,
                    });
                }
                if self.jpeg_buf.len() > 2 * 1024 * 1024 {
                    self.jpeg_buf.clear();
                }
                let reader = match self.reader.as_mut() {
                    Some(r) => r,
                    None => return FrameResult::Error("video source not started".to_string()),
                };
                match reader.read(&mut chunk) {
                    Ok(0) => {
                        return self.handle_read_err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "pipe closed",
                        ));
                    }
                    Ok(n) => self.jpeg_buf.extend_from_slice(&chunk[..n]),
                    Err(e) => return self.handle_read_err(e),
                }
            }
        }

        let reader = match self.reader.as_mut() {
            Some(r) => r,
            None => return FrameResult::Error("video source not started".to_string()),
        };
        let mut buf = vec![0u8; self.frame_size];
        match reader.read_exact(&mut buf) {
            Ok(_) => {
                self.frame_count += 1;
                self.active = true;
                FrameResult::Frame(VideoFrame {
                    data: buf,
                    width: self.info.width,
                    height: self.info.height,
                    timestamp: self.started.elapsed().as_micros() as i64,
                    frame_number: self.frame_count,
                })
            }
            Err(e) => self.handle_read_err(e),
        }
    }

    /// Extract the first complete JPEG frame (SOI→EOI) from the buffer.
    /// Returns the frame bytes and removes them (plus any preceding garbage)
    /// from the buffer.
    fn extract_jpeg(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
        if buf.len() < 4 {
            return None;
        }
        // Find SOI
        let soi = buf.windows(2).position(|w| w == [0xFF, 0xD8])?;
        // Find EOI after SOI
        let eoi = buf[soi + 2..]
            .windows(2)
            .position(|w| w == [0xFF, 0xD9])?
            + soi
            + 2;
        let frame = buf[soi..eoi + 2].to_vec();
        buf.drain(..eoi + 2);
        Some(frame)
    }

    fn handle_read_err(&mut self, e: std::io::Error) -> FrameResult {
        self.active = false;
        if let Some(status) = self
            .child
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten())
        {
            if !status.success() {
                tracing::warn!(
                    "ffmpeg exited with {}: {}",
                    status,
                    self.stderr_tail.lock().trim()
                );
            }
        }
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            FrameResult::EndOfStream
        } else {
            FrameResult::Error(format!("reading frame from ffmpeg failed: {}", e))
        }
    }

    /// Close and reopen the stream (for reconnection).
    pub fn reconnect(&mut self) -> Result<(), String> {
        self.kill_child();
        self.spawn_child()?;
        self.frame_count = 0;
        self.active = true;
        Ok(())
    }

    /// Restart the source from the beginning for seamless looping.
    ///
    /// The rawvideo pipe can't be seeked inside ffmpeg, so this restarts the
    /// child. For local files that's milliseconds; for HTTP sources it
    /// re-downloads the resource (a known tradeoff vs. the previous in-context
    /// seek, which only worked when the server honored Range requests).
    pub fn seek_to_start(&mut self) -> Result<(), String> {
        self.reconnect()
    }

    pub fn close(&mut self) {
        self.active = false;
        self.kill_child();
    }

    /// PID of the live ffmpeg child (for forced external kill when the owner
    /// thread is stuck inside a blocking `next_frame` and can't Drop normally).
    pub fn child_pid(&self) -> Option<i32> {
        self.child.as_ref().map(|c| c.id() as i32)
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

impl Drop for FfmpegVideoSource {
    fn drop(&mut self) {
        self.close();
    }
}

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
