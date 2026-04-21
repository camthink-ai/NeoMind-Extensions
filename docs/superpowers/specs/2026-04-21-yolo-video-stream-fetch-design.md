# YOLO Video V2 - Backend Stream Fetching Design

## Problem

The `yolo-video-v2` extension claims to support network streams (RTSP/RTMP/HLS) but the backend `start_push` task generates **simulated gradient frames** instead of actually fetching frames from the stream URL. The frontend cannot pass a stream address for real inference.

## Solution

Replace the fake frame generation with real stream fetching using `ffmpeg-next`. The backend opens the stream URL, decodes frames in a loop, runs YOLO inference, and pushes results to the frontend via the existing WebSocket `push_output` channel. The frontend only displays results.

## Architecture

```
User clicks Start (network mode)
  → Frontend sends WebSocket init { source_url: "rtsp://..." }
  → Backend creates StreamFetcher with ffmpeg-next
  → Backend loop: decode frame → YOLO infer → draw boxes → encode JPEG
  → Backend pushes push_output { data: base64_jpeg, metadata: { detections } }
  → Frontend displays frame + detections
```

## Backend Changes (`lib.rs`)

### 1. New Dependency: `ffmpeg-next`

Add to `Cargo.toml`:
```toml
ffmpeg-next = { version = "7", default-features = false, features = ["codec", "format", "software-scaling"] }
```

### 2. Implement Existing `VideoSource` Trait in `video_source.rs`

The project already has `video_source.rs` with a `VideoSource` trait, `SourceType` enum, and `SourceFactory` — but all implementations are stubs. We implement them using FFmpeg:

```rust
/// FFmpeg-based stream fetcher implementing VideoSource
pub struct FfmpegVideoSource {
    info: SourceInfo,
    input: ffmpeg_next::format::context::Input,
    decoder: ffmpeg_next::decoder::Video,
    scaler: ffmpeg_next::software::scaling::Context,
    stream_index: usize,
    active: bool,
}

impl FfmpegVideoSource {
    pub fn new(source_type: &SourceType) -> Result<Self>;  // Open stream based on SourceType config
    pub fn next_frame(&mut self) -> FrameResult;            // Decode next frame → FrameResult
}

impl VideoSource for FfmpegVideoSource {
    fn info(&self) -> &SourceInfo;
    fn is_active(&self) -> bool;
}
```

Then implement `SourceFactory::create()`:
```rust
impl SourceFactory {
    pub async fn create(source_type: SourceType) -> Result<Box<dyn VideoSource>, String> {
        match source_type {
            SourceType::RTSP { .. } | SourceType::RTMP { .. } | SourceType::HLS { .. } |
            SourceType::File { .. } => {
                Ok(Box::new(FfmpegVideoSource::new(&source_type)?))
            }
            SourceType::Camera { .. } => Err("Camera source uses frontend capture".to_string()),
            SourceType::Screen { .. } => Err("Screen capture not yet supported".to_string()),
        }
    }
}
```

Key behaviors:
- Opens any URL FFmpeg supports (RTSP, RTMP, HLS, HTTP-FLV, local files)
- Uses `software::scaling` to convert to RGB24
- Returns frames via existing `FrameResult` enum
- Non-blocking: returns `FrameResult::NotReady` when no frame is available yet

### 3. Modify `start_push` Network Stream Path

Replace the simulated frame generation (lines 1215-1228) with:

```rust
// Parse source URL using existing parse_source_url()
let source_type = parse_source_url(&source_url)?;

// Create video source via existing SourceFactory
let mut video_source = match SourceFactory::create(source_type).await {
    Ok(s) => s,
    Err(e) => {
        sender.send(PushOutputMessage { type: "error", ... }).await;
        return;
    }
};

loop {
    // Check if stream still running
    if !is_running(&sid) { break; }

    // Fetch real frame using VideoSource trait
    let frame = match video_source.next_frame() {
        FrameResult::Frame(f) => /* convert data to RgbImage */,
        FrameResult::NotReady => {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
        FrameResult::EndOfStream => break,
        FrameResult::Error(e) => { /* handle reconnect */ }
    };

    // Run YOLO inference (existing code)
    let detections = processor.get_detector()
        .and_then(|d| if d.is_loaded() { Some(d) } else { None })
        .map(|d| detections_to_object_detection(d.detect(&frame, confidence, max_obj)))
        .unwrap_or_default();

    // Draw boxes + encode JPEG (existing code)
    let mut output = frame;
    draw_detections(&mut output, &detections);
    let jpeg = encode_jpeg(&output, 85);
    let base64_data = base64::encode(&jpeg);

    // Push to frontend
    sender.send(PushOutputMessage {
        type: "push_output",
        data: base64_data,
        data_type: "image/jpeg",
        metadata: { detections },
        sequence: sequence += 1,
    }).await;
}
```

### 4. Error Handling & Reconnection

- On stream error: attempt reconnect up to 3 times with exponential backoff (1s, 2s, 4s)
- Send `error` message to frontend on each failure
- On final failure: send `session_closed` and clean up
- Send `status` messages (`connecting`, `streaming`, `reconnecting`) to frontend

### 5. Frame Rate Control

- Use existing FPS configuration from `StreamConfig`
- Throttle frame decoding to match `target_fps` (skip frames if needed)
- Default: 15 FPS for network streams

## Frontend Changes (`index.tsx`)

### 1. Network Mode: Remove Camera Logic

In `connectWebSocket` → `onopen` handler, for network mode:
- Send init message with `source_url` (unchanged)
- Do NOT start `frameTimerRef` capture loop
- Just wait for `push_output` messages from backend

### 2. Add Connection Status Display

New state: `streamStatus: 'idle' | 'connecting' | 'streaming' | 'reconnecting' | 'error'`

Handle new message types:
```typescript
case 'status':
    setStreamStatus(msg.status)  // 'connecting' | 'streaming' | 'reconnecting'
    break
```

Display status in header (replaces the static mode label):
- `connecting` → "Connecting..." (yellow dot)
- `streaming` → "RTSP" / "RTMP" / "HLS" (green dot)
- `reconnecting` → "Reconnecting..." (yellow dot, animated)
- `error` → "Error" (red dot)

### 3. Keep Camera Mode Unchanged

All `camera://` mode logic stays exactly the same. The `if (mode === 'camera')` branches remain.

## Packaging Changes

### 1. Bundle FFmpeg Libraries in .nep

New `.nep` structure:
```
yolo-video-v2-X.X.X-darwin_aarch64.nep
├── manifest.json
├── binaries/
│   └── darwin_aarch64/
│       └── libneomind_extension_yolo_video_v2.dylib
├── libs/                          ← NEW
│   └── darwin_aarch64/
│       ├── libavcodec.dylib
│       ├── libavformat.dylib
│       ├── libavutil.dylib
│       ├── libswscale.dylib
│       └── libswresample.dylib
├── frontend/
│   └── yolo-video-v2-components.umd.cjs
└── models/
```

### 2. Build Script Changes (`build.sh`)

Add a `bundle_ffmpeg()` function:
- Detect platform from build target
- Locate system FFmpeg libraries (via `brew` on macOS, `apt` on Linux)
- Copy required dylibs/so files to `libs/<platform>/` in the .nep package
- Use `install_name_tool` (macOS) or `patchelf` (Linux) to fix library paths

### 3. Runtime Library Loading

In `lib.rs` startup, add library path resolution:
```rust
fn ensure_ffmpeg_libs() {
    // Add extension's libs/ directory to dyld search path
    // This is done before any ffmpeg-next calls
}
```

## Supported Protocols

| Protocol | Format | Status |
|----------|--------|--------|
| RTSP | `rtsp://host:port/path` | Primary target |
| RTMP | `rtmp://host:port/path` | Supported via FFmpeg |
| HLS | `http://host/path.m3u8` | Supported via FFmpeg |
| HTTP-FLV | `http://host/path.flv` | Supported via FFmpeg |
| Local file | `/path/to/video.mp4` | Supported via FFmpeg |

## Non-Goals

- No changes to camera:// mode
- No changes to WebSocket protocol (reuses push_output)
- No shared FFmpeg resource between extensions (future work)
- No GPU-accelerated decoding (future work)

## Testing Plan

1. Unit test: `StreamFetcher` with local test video file
2. Integration test: RTSP stream from test camera
3. Frontend test: Verify push_output display works with new backend
4. Package test: Verify .nep bundles FFmpeg libs correctly on macOS/Linux
