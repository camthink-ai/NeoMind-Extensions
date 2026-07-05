# Stream Player

Universal video player supporting RTSP, RTMP, HLS, local files via FFmpeg transcoding and JPEG frame rendering.

## Features

- Multi-protocol support: RTSP, RTMP, HLS, HTTP, and local file playback
- FFmpeg-based decoding with RGB24 scaling and JPEG encoding
- Push streaming via WebSocket to frontend canvas rendering
- Configurable target FPS, output resolution, and quality
- Auto-reconnect for network sources with exponential backoff (up to 3 retries)
- Loop playback for local file sources
- Frame skipping to recover from latency spikes
- Concurrent session support (up to 4 simultaneous streams)

## Installation

```bash
# Build this extension only
./build.sh --single stream-player

# Dev build with auto-install to NeoMind
./build.sh --dev --single stream-player

# Release build with versioned package
./build.sh --release 2.6.0 --single stream-player
```

**Runtime dependency:** FFmpeg libraries must be installed on the host system.

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `list_sources` | List supported video source formats and example URLs | None |
| `get_player_info` | Get current player status, active sessions, and stream stats | None |
| `extract_frame` | Open a video URL one-shot and return the latest frame as JPEG | See below |

### `extract_frame` — One-shot Frame Extraction

Opens a video URL, decodes the **first available frame** (= "latest" for live sources, = first frame for files), encodes as JPEG, and returns. No running stream session needed.

**Parameters:**

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `url` | string | ✅ | — | Video source (RTSP/RTMP/HLS/HTTP/file) |
| `output` | string | ❌ | `"base64"` | `"base64"` or `"file"` |
| `output_path` | string | ❌ | auto temp | Where to save when `output="file"` |
| `width` | integer | ❌ | source width | Must specify with `height` |
| `height` | integer | ❌ | source height | Must specify with `width` |
| `quality` | integer | ❌ | `85` | JPEG quality 1-100 |

**Examples:**

```jsonc
// Default: base64 from live RTSP
{ "url": "rtsp://host:554/stream" }

// Save to file
{ "url": "rtsp://host/stream", "output": "file" }

// Custom size + quality
{ "url": "/tmp/clip.mp4", "output": "base64", "width": 320, "height": 240, "quality": 70 }
```

**Response (base64):**
```json
{ "success": true, "width": 640, "height": 480, "mime": "image/jpeg", "size_bytes": 45321, "data": "<base64...>" }
```

**Response (file):**
```json
{ "success": true, "width": 640, "height": 480, "path": "/tmp/neomind-frame-xxx.jpg", "size_bytes": 45321 }
```

Errors: `ExecutionFailed` with descriptive message on open failure, decode timeout (15s), or invalid params.


## Metrics

| Metric | Display Name | Type | Unit | Description |
|--------|--------------|------|------|-------------|
| `active_streams` | Active Streams | Integer | count | Number of currently active stream sessions |
| `total_frames` | Total Frames | Integer | frames | Cumulative frames decoded and pushed across all sessions |
| `total_bytes_sent` | Total Bytes Sent | Integer | bytes | Cumulative bytes of JPEG data pushed to frontends |

## Stream Configuration

Sessions are configured via `PlayerConfig` (passed as session config):

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `source_url` | string | (required) | Video source URL (RTSP/RTMP/HLS/HTTP/file) |
| `target_fps` | integer | 24 | Target frame rate |
| `output_width` | integer | 640 | Output frame width in pixels |
| `output_height` | integer | 480 | Output frame height in pixels |
| `video_bitrate` | integer | 1500 | Video bitrate (kbps) |
| `loop_file` | boolean | true | Loop playback for file sources |

## Frontend Component

**StreamPlayerCard** - A panel component that renders the live video stream on an HTML canvas. Supports configurable default source URL and target FPS. Built as a UMD bundle (`stream-player-components.umd.cjs`).

## License

Apache-2.0
