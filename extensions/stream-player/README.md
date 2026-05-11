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
