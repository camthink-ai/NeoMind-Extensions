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

### 2. New `StreamFetcher` Module

Encapsulates FFmpeg stream decoding:

```rust
struct StreamFetcher {
    decoder: ffmpeg_next::decoder::Video,
    scaler: ffmpeg_next::software::scaling::Context,
    stream_index: usize,
    input: ffmpeg_next::format::context::Input,
}

impl StreamFetcher {
    fn new(url: &str) -> Result<Self>;      // Open stream, find video stream, init decoder
    fn next_frame(&mut self) -> Option<RgbImage>;  // Decode next frame → RgbImage
    fn close(&mut self);                     // Clean up resources
}
```

Key behaviors:
- Opens any URL FFmpeg supports (RTSP, RTMP, HLS, HTTP-FLV, local files)
- Uses `software::scaling` to convert to RGB24
- Returns `RgbImage` compatible with existing YOLO inference pipeline
- Non-blocking: returns `None` when no frame is available yet

### 3. Modify `start_push` Network Stream Path

Replace the simulated frame generation (lines 1215-1228) with:

```rust
// Create StreamFetcher
let mut fetcher = match StreamFetcher::new(&source_url) {
    Ok(f) => f,
    Err(e) => {
        sender.send(PushOutputMessage { type: "error", ... }).await;
        return;
    }
};

loop {
    // Check if stream still running
    if !is_running(&sid) { break; }

    // Fetch real frame
    let frame = match fetcher.next_frame() {
        Some(f) => f,
        None => {
            // Stream ended or temporary no-data
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
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
