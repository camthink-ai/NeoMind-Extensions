# YOLO Video V2 Backend Stream Fetching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace fake frame generation in yolo-video-v2 with real RTSP/RTMP/HLS stream fetching via ffmpeg-next, so the backend pulls frames and the frontend only displays results.

**Architecture:** Implement the existing `VideoSource` trait in `video_source.rs` using `ffmpeg-next` for stream decoding. Run FFmpeg decode loop on a dedicated OS thread (avoiding blocking the tokio runtime). The decoded frames go through the same YOLO inference pipeline: resize to 640x640 → detect → scale coordinates back → draw on original → encode JPEG → push via WebSocket.

**Tech Stack:** Rust, ffmpeg-next (FFmpeg bindings), image crate (existing), tokio (existing), crossbeam-channel (existing)

**Spec:** `docs/superpowers/specs/2026-04-21-yolo-video-stream-fetch-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `extensions/yolo-video-v2/Cargo.toml` | Modify | Add ffmpeg-next dependency |
| `extensions/yolo-video-v2/src/video_source.rs` | Modify | Implement FfmpegVideoSource with real FFmpeg decoding |
| `extensions/yolo-video-v2/src/lib.rs` | Modify | Replace fake frame generation in start_push with FfmpegVideoSource |
| `extensions/yolo-video-v2/frontend/src/index.tsx` | Modify | Handle network stream status messages |
| `build.sh` | May modify | FFmpeg lib bundling if auto-discovery misses them |

## Image Pipeline Reference (existing, unchanged)

```
FFmpeg decoded frame (raw RGB24 bytes, stride-aware)
  → strip padding, build image::RgbImage::from_raw(w, h, data)
  → image::imageops::resize(&original, 640, 640, CatmullRom)  // for inference
  → detector.detect(&inference_image, confidence, max_objects)
  → scale coordinates: det.bbox.x *= orig_width / 640.0
  → draw_detections(&mut original_image, &detections)  // on ORIGINAL size
  → encode_jpeg(&original_image, quality)
  → PushOutputMessage::image_jpeg(session_id, sequence, jpeg_data)
```

Key: inference runs on 640x640, drawing runs on original resolution, coordinates must be scaled.

---

### Task 1: Add ffmpeg-next Dependency

**Files:**
- Modify: `extensions/yolo-video-v2/Cargo.toml`

- [ ] **Step 1: Check installed FFmpeg version**

Run: `ffmpeg -version 2>&1 | head -1`
Note the version number. `ffmpeg-next` crate version must match: FFmpeg 7.x → `ffmpeg-next = "7"`, FFmpeg 8.x → `ffmpeg-next = "8"`.

- [ ] **Step 2: Add ffmpeg-next to Cargo.toml**

Add to `[dependencies]` section:

```toml
ffmpeg-next = { version = "7", default-features = false, features = ["codec", "format", "software-scaling"] }
```

Adjust version to `"8"` if the system has FFmpeg 8.x. We only need `codec` (decoding), `format` (demuxing), and `software-scaling` (color conversion). No encoding, no filtering, no audio.

- [ ] **Step 3: Verify compilation**

Run: `cd extensions/yolo-video-v2 && cargo check 2>&1 | tail -20`
Expected: Compiles with no new errors. FFmpeg dev libraries must be installed (`brew install ffmpeg` on macOS).

- [ ] **Step 4: Commit**

```bash
git add extensions/yolo-video-v2/Cargo.toml
git commit -m "feat(yolo-video-v2): add ffmpeg-next dependency for stream fetching"
```

---

### Task 2: Implement FfmpegVideoSource

**Files:**
- Modify: `extensions/yolo-video-v2/src/video_source.rs`

This is the core task. Implement real stream decoding using FFmpeg, fitting the existing `VideoSource` trait.

**Critical design note:** FFmpeg calls (`send_packet`, `receive_frame`, `input_with_dictionary`) are **synchronous blocking I/O**. This code is designed to run on a dedicated OS thread (via `std::thread::spawn`), NOT inside `tokio::spawn`. The `next_frame()` method is intentionally synchronous.

- [ ] **Step 1: Add FfmpegVideoSource struct and constructor**

Add to `video_source.rs` after the existing code:

```rust
use ffmpeg_next as ff;

/// FFmpeg-based video source for network streams
pub struct FfmpegVideoSource {
    info: SourceInfo,
    input_ctx: ff::format::context::Input,
    decoder: ff::decoder::Video,
    scaler: ff::software::scaling::Context,
    stream_index: usize,
    active: bool,
    frame_count: u64,
}

impl FfmpegVideoSource {
    pub fn new(source_type: &SourceType) -> Result<Self, String> {
        // Initialize FFmpeg (safe to call multiple times)
        ff::init().map_err(|e| format!("FFmpeg init failed: {}", e))?;

        let url = match source_type {
            SourceType::RTSP { url, .. } => url.as_str(),
            SourceType::RTMP { url, .. } => url.as_str(),
            SourceType::HLS { url, .. } => url.as_str(),
            SourceType::File { path, .. } => path.as_str(),
            _ => return Err("Unsupported source type for FFmpeg".to_string()),
        };

        // Open input with timeout options for network streams
        let mut input_opts = ff::Dictionary::new();
        if matches!(source_type, SourceType::RTSP { .. }) {
            input_opts.set("rtsp_transport", "tcp");
            input_opts.set("stimeout", "5000000"); // 5 seconds in microseconds
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
        let mut decoder = context.decoder().video()
            .map_err(|e| format!("Failed to open video decoder: {}", e))?;

        // Get stream info
        let width = decoder.width();
        let height = decoder.height();
        let fps = stream.avg_frame_rate();
        let fps_float = if fps.numerator() > 0 && fps.denominator() > 0 {
            fps.numerator() as f32 / fps.denominator() as f32
        } else {
            25.0
        };

        // Create scaler to convert decoded frames to RGB24
        let scaler = ff::software::scaling::Context::get(
            decoder.format(),
            width, height,
            ff::format::Pixel::RGB24,
            width, height,
            ff::software::scaling::flag::Flags::BILINEAR,
        ).map_err(|e| format!("Failed to create scaler: {}", e))?;

        Ok(Self {
            info: SourceInfo {
                width,
                height,
                fps: fps_float,
                codec: stream.parameters().id().map(|id| id.name().to_string()).unwrap_or_default(),
                is_live: !matches!(source_type, SourceType::File { .. }),
            },
            input_ctx,
            decoder,
            scaler,
            stream_index,
            active: true,
            frame_count: 0,
        })
    }

    /// Decode next frame. BLOCKING — call from a dedicated thread, not tokio.
    /// Returns raw RGB24 data with dimensions.
    pub fn next_frame(&mut self) -> FrameResult {
        let mut decoded = ff::frame::Video::empty();

        // Read packets until we get a decoded video frame
        for packet_result in self.input_ctx.packets() {
            let (stream, pkt) = match packet_result {
                Ok(p) => p,
                Err(e) => {
                    self.active = false;
                    return FrameResult::Error(format!("Read error: {}", e));
                }
            };

            if stream.index() != self.stream_index {
                continue;
            }

            // Send packet to decoder
            if self.decoder.send_packet(&pkt).is_err() {
                continue;
            }

            // Receive decoded frame
            while self.decoder.receive_frame(&mut decoded).is_ok() {
                // Scale to RGB24
                let mut rgb_frame = ff::frame::Video::empty();
                if self.scaler.run(&decoded, &mut rgb_frame).is_err() {
                    continue;
                }

                self.frame_count += 1;

                // Extract RGB24 data, handling stride/padding
                // FFmpeg frames may have row padding (stride > width * 3)
                let width = rgb_frame.width();
                let height = rgb_frame.height();
                let stride = rgb_frame.stride(0) as usize;
                let row_bytes = (width * 3) as usize;

                let data = if stride == row_bytes {
                    // No padding — use data directly
                    rgb_frame.data(0).to_vec()
                } else {
                    // Strip padding: copy row by row
                    let raw = rgb_frame.data(0);
                    let mut stripped = Vec::with_capacity(row_bytes * height as usize);
                    for row in 0..height as usize {
                        let start = row * stride;
                        stripped.extend_from_slice(&raw[start..start + row_bytes]);
                    }
                    stripped
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

        // End of stream
        self.active = false;
        FrameResult::EndOfStream
    }

    /// Reconnect to the stream
    pub fn reconnect(&mut self, source_type: &SourceType) -> Result<(), String> {
        self.close();
        let new_source = Self::new(source_type)?;
        self.info = new_source.info;
        self.input_ctx = new_source.input_ctx;
        self.decoder = new_source.decoder;
        self.scaler = new_source.scaler;
        self.stream_index = new_source.stream_index;
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
```

- [ ] **Step 2: Implement SourceFactory with real FFmpeg creation**

Replace the stub `SourceFactory::create`:

```rust
impl SourceFactory {
    pub async fn create(source_type: SourceType) -> Result<Box<dyn VideoSource>, String> {
        match source_type {
            SourceType::RTSP { .. } | SourceType::RTMP { .. } | SourceType::HLS { .. } |
            SourceType::File { .. } => {
                let source = FfmpegVideoSource::new(&source_type)?;
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
```

- [ ] **Step 3: Add conversion from VideoFrame to RgbImage**

Add a helper method to `VideoFrame`:

```rust
impl VideoFrame {
    /// Convert raw RGB24 data to image::RgbImage.
    /// Returns None if data length doesn't match width * height * 3.
    pub fn to_rgb_image(&self) -> Option<image::RgbImage> {
        image::RgbImage::from_raw(self.width, self.height, std::mem::take(&mut self.data))
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd extensions/yolo-video-v2 && cargo check 2>&1 | tail -20`
Expected: Compiles with no errors. May have warnings about unused imports.

- [ ] **Step 5: Run existing tests**

Run: `cd extensions/yolo-video-v2 && cargo test --lib video_source 2>&1`
Expected: All existing tests pass (test_parse_camera_url, test_parse_rtsp_url, etc.)

- [ ] **Step 6: Commit**

```bash
git add extensions/yolo-video-v2/src/video_source.rs
git commit -m "feat(yolo-video-v2): implement FfmpegVideoSource for real stream decoding"
```

---

### Task 3: Replace Fake Frame Generation in start_push

**Files:**
- Modify: `extensions/yolo-video-v2/src/lib.rs` (lines ~1130-1317, the `start_push` method)

This is where the real integration happens. Replace the simulated frame loop with FfmpegVideoSource.

**Architecture decision:** The FFmpeg decode + YOLO inference loop runs on a **dedicated OS thread** (via `std::thread::spawn`), NOT inside `tokio::spawn`. This avoids blocking the async runtime with synchronous FFmpeg I/O. Communication with the async world uses the existing `mpsc::Sender<PushOutputMessage>` channel (which is async-safe via `try_send`).

- [ ] **Step 1: Add import for video_source module**

At the top of `lib.rs`, ensure the module is imported. Find the existing `mod` declarations and add if missing:

```rust
mod video_source;
use video_source::{FfmpegVideoSource, SourceType, FrameResult, parse_source_url};
```

- [ ] **Step 2: Replace the simulated frame generation in start_push**

The key section to replace is inside the `tokio::spawn` block (around lines 1183-1303). Replace the **entire tokio::spawn closure** with a `std::thread::spawn`:

Find the block starting with `let task_handle = tokio::spawn(async move {` (around line 1183) through its closing `});` (around line 1306). Replace with:

```rust
        // Run on dedicated OS thread — FFmpeg decode is blocking I/O
        let task_handle = std::thread::spawn(move || {
            let mut sequence = 0u64;
            let frame_duration = std::time::Duration::from_millis(1000 / config.target_fps.max(1) as u64);
            let mut reconnect_count = 0u32;
            const MAX_RECONNECT: u32 = 3;

            // Parse source URL and create FFmpeg video source
            let source_type = match parse_source_url(&source_url) {
                Ok(st) => st,
                Err(e) => {
                    tracing::error!("[Stream {}] Invalid source URL: {}", sid, e);
                    let _ = sender.try_send(
                        PushOutputMessage::json(&sid, sequence, serde_json::json!({
                            "type": "error",
                            "message": format!("Invalid source URL: {}", e)
                        })).unwrap_or_else(|_| {
                            PushOutputMessage::image_jpeg(&sid, sequence, vec![])
                        })
                    );
                    return;
                }
            };

            // Create the video source (this opens the stream connection)
            let mut video_source = match FfmpegVideoSource::new(&source_type) {
                Ok(vs) => {
                    tracing::info!("[Stream {}] Connected to: {}", sid, source_url);
                    let _ = sender.try_send(
                        PushOutputMessage::json(&sid, sequence, serde_json::json!({
                            "type": "status",
                            "status": "streaming"
                        })).unwrap_or_else(|_| {
                            PushOutputMessage::image_jpeg(&sid, sequence, vec![])
                        })
                    );
                    vs
                }
                Err(e) => {
                    tracing::error!("[Stream {}] Failed to connect: {}", sid, e);
                    let _ = sender.try_send(
                        PushOutputMessage::json(&sid, sequence, serde_json::json!({
                            "type": "error",
                            "message": format!("Failed to connect: {}", e)
                        })).unwrap_or_else(|_| {
                            PushOutputMessage::image_jpeg(&sid, sequence, vec![])
                        })
                    );
                    return;
                }
            };

            loop {
                // Check if stream is still running
                let should_continue = {
                    let registry = get_registry().lock();
                    registry.streams.get(&sid).map_or(false, |s| s.lock().running)
                };
                if !should_continue {
                    break;
                }

                let frame_start = std::time::Instant::now();

                // Fetch next frame from FFmpeg (BLOCKING call — OK on dedicated thread)
                let frame_result = video_source.next_frame();

                match frame_result {
                    FrameResult::Frame(mut video_frame) => {
                        reconnect_count = 0;

                        // Convert FFmpeg RGB24 data to RgbImage
                        let original_image = match video_frame.to_rgb_image() {
                            Some(img) => img,
                            None => {
                                tracing::warn!("[Stream {}] Failed to convert frame to RgbImage (data size mismatch)", sid);
                                continue;
                            }
                        };

                        let (orig_width, orig_height) = (original_image.width(), original_image.height());

                        // Resize to 640x640 for YOLO inference
                        let inference_image = image::imageops::resize(
                            &original_image,
                            640, 640,
                            image::imageops::FilterType::CatmullRom,
                        );

                        // Run YOLO detection on resized image
                        let detections = {
                            match processor.get_detector() {
                                Some(detector) if detector.is_loaded() => {
                                    let dets = detector.detect(&inference_image, confidence, max_obj);
                                    if !dets.is_empty() {
                                        // Scale coordinates from 640x640 back to original dimensions
                                        let scale_x = orig_width as f32 / 640.0;
                                        let scale_y = orig_height as f32 / 640.0;
                                        let scaled: Vec<_> = dets.into_iter().map(|mut d| {
                                            d.bbox.x *= scale_x;
                                            d.bbox.y *= scale_y;
                                            d.bbox.width *= scale_x;
                                            d.bbox.height *= scale_y;
                                            d
                                        }).collect();
                                        detections_to_object_detection(scaled)
                                    } else {
                                        vec![]
                                    }
                                }
                                _ => vec![],
                            }
                        };

                        // Draw detections on original-resolution image
                        let mut output_image = original_image;
                        draw_detections(&mut output_image, &detections);

                        // Encode to JPEG
                        let jpeg_data = encode_jpeg(&output_image, 75);

                        // Update stream statistics (quick lock)
                        {
                            let registry = get_registry().lock();
                            if let Some(stream) = registry.streams.get(&sid) {
                                let mut s = stream.lock();
                                s.frame_count += 1;
                                s.total_detections += detections.len() as u64;
                                s.last_detections = detections.clone();
                                s.last_frame = Some(jpeg_data.clone());
                                s.last_frame_time = Some(std::time::Instant::now());
                                let elapsed = s.started_at.elapsed().as_secs_f32();
                                if elapsed > 0.0 {
                                    s.fps = s.frame_count as f32 / elapsed;
                                }
                                // Periodic cleanup to prevent memory growth
                                if s.frame_count % 30 == 0 {
                                    s.detected_objects.clear();
                                    s.last_frame = None;
                                }
                            }
                        }

                        // Push to frontend via WebSocket channel
                        let output = PushOutputMessage::image_jpeg(&sid, sequence, jpeg_data)
                            .with_metadata(serde_json::json!({
                                "detections": detections
                            }));

                        match sender.try_send(output) {
                            Ok(_) => sequence += 1,
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                tracing::debug!("[Stream {}] Channel full, dropping frame {}", sid, sequence);
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                tracing::warn!("[Stream {}] Channel closed", sid);
                                break;
                            }
                        }

                        // Frame rate throttling: sleep remaining time to hit target FPS
                        let elapsed = frame_start.elapsed();
                        if elapsed < frame_duration {
                            std::thread::sleep(frame_duration - elapsed);
                        }
                    }
                    FrameResult::NotReady => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    FrameResult::EndOfStream => {
                        tracing::warn!("[Stream {}] Stream ended", sid);
                        break;
                    }
                    FrameResult::Error(e) => {
                        tracing::error!("[Stream {}] Frame error: {}", sid, e);
                        reconnect_count += 1;
                        if reconnect_count > MAX_RECONNECT {
                            tracing::error!("[Stream {}] Max reconnect attempts reached", sid);
                            let _ = sender.try_send(
                                PushOutputMessage::json(&sid, sequence, serde_json::json!({
                                    "type": "error",
                                    "message": format!("Stream error after {} retries: {}", MAX_RECONNECT, e)
                                })).unwrap_or_else(|_| {
                                    PushOutputMessage::image_jpeg(&sid, sequence, vec![])
                                })
                            );
                            break;
                        }
                        // Exponential backoff: 1s, 2s, 4s
                        let backoff = std::time::Duration::from_secs(1 << (reconnect_count - 1));
                        tracing::info!("[Stream {}] Reconnecting in {:?} (attempt {}/{})", sid, backoff, reconnect_count, MAX_RECONNECT);
                        let _ = sender.try_send(
                            PushOutputMessage::json(&sid, sequence, serde_json::json!({
                                "type": "status",
                                "status": "reconnecting"
                            })).unwrap_or_else(|_| {
                                PushOutputMessage::image_jpeg(&sid, sequence, vec![])
                            })
                        );
                        std::thread::sleep(backoff);

                        match video_source.reconnect(&source_type) {
                            Ok(()) => {
                                tracing::info!("[Stream {}] Reconnected", sid);
                                let _ = sender.try_send(
                                    PushOutputMessage::json(&sid, sequence, serde_json::json!({
                                        "type": "status",
                                        "status": "streaming"
                                    })).unwrap_or_else(|_| {
                                        PushOutputMessage::image_jpeg(&sid, sequence, vec![])
                                    })
                                );
                            }
                            Err(re) => {
                                tracing::error!("[Stream {}] Reconnect failed: {}", sid, re);
                            }
                        }
                    }
                }
            }

            tracing::info!("[Stream {}] Push task ended. Total frames: {}", sid, sequence);
        });
```

Note: We use `std::thread::sleep` here because we're on a dedicated OS thread, not inside tokio. This is correct — blocking is fine on a dedicated thread.

- [ ] **Step 3: Fix the push_task type**

The existing `push_task` field is `Option<tokio::task::JoinHandle<()>>`. Since we changed from `tokio::spawn` to `std::thread::spawn`, update the field type in `ActiveStream` struct:

```rust
push_task: Option<std::thread::JoinHandle<()>>,
```

Update all places where `push_task` is set/used (there are ~4 occurrences in lib.rs — search for `push_task`).

- [ ] **Step 4: Fix the task abort on stop**

The old code used `task.abort()` (tokio feature) to cancel the push task. With `std::thread::JoinHandle`, there's no `abort()`. Instead, rely on the `running` flag check in the loop — setting `running = false` will cause the thread to exit on the next iteration. Remove the `task.abort()` call or replace with:

```rust
if let Some(task) = old.push_task.take() {
    // Thread will exit on next loop iteration when running=false
    // Don't join — that would block
    drop(task);
}
```

- [ ] **Step 5: Verify compilation**

Run: `cd extensions/yolo-video-v2 && cargo check 2>&1 | tail -30`
Expected: Compiles. Fix any type mismatches.

- [ ] **Step 6: Commit**

```bash
git add extensions/yolo-video-v2/src/lib.rs
git commit -m "feat(yolo-video-v2): replace fake frames with real ffmpeg stream fetching"
```

---

### Task 4: Update Frontend for Backend Stream Fetching

**Files:**
- Modify: `extensions/yolo-video-v2/frontend/src/index.tsx`

The frontend needs minor changes to display connection status from the backend.

**Protocol for status messages:** The backend sends JSON messages via `PushOutputMessage::json()`:
```json
{"type": "status", "status": "streaming"}
{"type": "status", "status": "reconnecting"}
{"type": "error", "message": "Failed to connect: ..."}
```

These arrive as regular `push_output` messages with JSON data (not image/jpeg).

- [ ] **Step 1: Add stream status state**

Find the state declarations (around line 369) and add:

```typescript
const [streamStatus, setStreamStatus] = useState<'idle' | 'connecting' | 'streaming' | 'reconnecting' | 'error'>('idle')
```

- [ ] **Step 2: Handle status messages in push_output handler**

In the `ws.onmessage` handler, update the `push_output` case (around line 576) to detect status messages:

```typescript
case 'push_output':
  // Check if this is a status/error message (JSON, not image)
  if (msg.data_type === 'application/json' && msg.data) {
    try {
      const statusData = typeof msg.data === 'string' ? JSON.parse(msg.data) : msg.data
      if (statusData.type === 'status' && statusData.status) {
        setStreamStatus(statusData.status as any)
      } else if (statusData.type === 'error') {
        setStreamStatus('error')
        setError(statusData.message || 'Stream error')
      }
    } catch { /* ignore parse errors */ }
    break
  }
  // Image frame
  if (msg.data && msg.data_type === 'image/jpeg') {
    setStreamStatus('streaming')
    setFrameData(msg.data)
    updateFps()
    if (msg.metadata?.detections) {
      setDetections(msg.metadata.detections)
    }
  }
  break
```

- [ ] **Step 3: Update status display**

Replace the `getModeLabel()` function (around line 745):

```typescript
const getModeLabel = () => {
  if (mode === 'network') {
    // Show connection status for active streams
    if (isRunning && streamStatus === 'reconnecting') return 'Reconnecting...'
    if (isRunning && streamStatus === 'error') return 'Error'
    // Show protocol when streaming or idle
    if (sourceUrl.startsWith('rtsp://')) return 'RTSP'
    if (sourceUrl.startsWith('rtmp://')) return 'RTMP'
    if (sourceUrl.startsWith('hls://') || sourceUrl.includes('.m3u8')) return 'HLS'
    return 'Network'
  }
  return 'CAM'
}
```

Update the status dot (around line 768):

```typescript
<span className={`yolo-status-dot${
  streamStatus === 'reconnecting' ? ' yolo-status-warning' :
  streamStatus === 'error' ? ' yolo-status-error' : ''
}`} />
```

Add CSS in the STYLES string (after the existing `.yolo-status-dot` rule):

```css
.yolo-status-dot.yolo-status-warning { background: var(--yolo-warning); animation: yolo-blink 1s infinite; }
.yolo-status-dot.yolo-status-error { background: #ef4444; }
@keyframes yolo-blink { 0%,100% { opacity: 1; } 50% { opacity: 0.3; } }
```

- [ ] **Step 4: Reset streamStatus on disconnect**

In `disconnectWebSocket` and `stopStream`, add:
```typescript
setStreamStatus('idle')
```

Also reset in `ws.onclose` handler:
```typescript
setStreamStatus('idle')
```

- [ ] **Step 5: Build frontend and verify**

Run: `cd extensions/yolo-video-v2/frontend && npm run build`
Expected: Build succeeds with no TypeScript errors.

- [ ] **Step 6: Commit**

```bash
git add extensions/yolo-video-v2/frontend/
git commit -m "feat(yolo-video-v2): frontend stream status display for backend-fetched streams"
```

---

### Task 5: Integration Test with Local Video File

**Files:**
- No new files (manual testing)

Test the full pipeline end-to-end using a local video file (FFmpeg supports `file://` URLs).

- [ ] **Step 1: Build the extension**

Run: `./build.sh --single yolo-video-v2`
Expected: Build succeeds.

- [ ] **Step 2: Test with a local video file**

Use a test video file URL in the frontend config:
```
sourceUrl: "file:///path/to/test_video.mp4"
```

Expected behavior:
1. Extension starts, opens video file via FFmpeg
2. Frames are decoded (stride-aware RGB24 extraction)
3. Resized to 640x640 for YOLO inference
4. Coordinates scaled back to original resolution
5. Detections drawn on original image
6. JPEG frames pushed to frontend via WebSocket
7. Frontend displays video with bounding boxes

- [ ] **Step 3: Test RTSP stream (if available)**

Use an RTSP camera or test stream:
```
sourceUrl: "rtsp://192.168.1.100:554/stream"
```

Expected: Same pipeline as file test, but with live stream.

- [ ] **Step 4: Test reconnection**

Disconnect the stream source mid-stream and verify:
- Backend sends "reconnecting" status via JSON message
- Reconnects automatically (up to 3 times)
- Frontend shows "Reconnecting..." with blinking yellow dot

- [ ] **Step 5: Commit any fixes**

```bash
git add -A
git commit -m "fix(yolo-video-v2): integration test fixes"
```

---

### Task 6: Build Script Verification (No Code Changes Expected)

**Files:**
- Verify: `build.sh`

The existing `build.sh` already handles dynamic library bundling:
- **macOS** (lines 471-539): Uses `otool -L` to find dependencies, copies them, fixes paths with `install_name_tool`
- **Linux** (lines 542-555): Uses `patchelf` to set `$ORIGIN` rpath

FFmpeg dylibs linked by the extension will be automatically discovered and bundled.

- [ ] **Step 1: Verify FFmpeg libs are auto-bundled on macOS**

Run: `./build.sh --single yolo-video-v2 && unzip -l dist/yolo-video-v2-*.nep | grep -i "av\|ffmpeg"`
Expected: Shows libavcodec, libavformat, libavutil, libswscale in the binaries directory.

- [ ] **Step 2: If not auto-bundled, add explicit FFmpeg handling**

If the auto-discovery misses FFmpeg libs (e.g., they use system paths like `/opt/homebrew/lib/` which are filtered out by the `grep -oE "/Users/"` pattern in the existing script), add explicit handling similar to the ORT bundling:

In `build.sh`, after the ORT bundling section (~line 435), add:

```bash
# Bundle FFmpeg libraries for extensions using ffmpeg-next
if [ "$EXT_NAME" = "yolo-video-v2" ] && [ "$IS_WASM" = false ]; then
    echo -e "    ${BLUE}→${NC} Bundling FFmpeg libraries..."
    FFMPEG_LIBS="libavcodec libavformat libavutil libswscale libswresample"
    for lib in $FFMPEG_LIBS; do
        for search_path in /usr/local/lib /opt/homebrew/lib /usr/lib/x86_64-linux-gnu /usr/lib/aarch64-linux-gnu; do
            if [ "$LIB_EXT" = "dylib" ]; then
                FFILE=$(find "$search_path" -maxdepth 1 -name "${lib}*.dylib" -not -path "*/dSYM/*" 2>/dev/null | head -1)
            elif [ "$LIB_EXT" = "so" ]; then
                FFILE=$(find "$search_path" -maxdepth 1 -name "${lib}.so*" 2>/dev/null | head -1)
            fi
            if [ -n "$FFILE" ] && [ -f "$FFILE" ]; then
                cp "$FFILE" "$BINARY_DIR/"
                echo -e "    ${GREEN}✓${NC} Bundled: $(basename $FFILE)"
                break
            fi
        done
    done
fi
```

Also update the `otool -L` dependency fix (line 486) to handle `/opt/homebrew/` and `/usr/local/` paths, not just `/Users/`:

```bash
# Original: only matches /Users/ paths
# DEPS=$(otool -L "$BINARY_PATH" 2>/dev/null | grep -oE "/Users/[^ ]+\.dylib" || true)
# Fixed: also match /opt/homebrew/ and /usr/local/
DEPS=$(otool -L "$BINARY_PATH" 2>/dev/null | \
       grep -oE "(/Users/|/opt/homebrew/|/usr/local/)[^ ]+\.dylib" || true)
```

- [ ] **Step 3: Commit if build.sh was modified**

```bash
git add build.sh
git commit -m "fix(build): add explicit FFmpeg lib bundling for yolo-video-v2"
```
