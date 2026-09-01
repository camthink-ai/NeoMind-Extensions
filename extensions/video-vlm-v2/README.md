# YOLO Video V2

Real-time video stream object detection with YOLOv11, RTSP/camera support, ROI analytics, line crossing, smart capture rules, and MJPEG streaming.

## Features

- YOLOv11 object detection (COCO 80 classes) via ONNX Runtime
- Multiple video sources: local camera, RTSP, HLS, RTMP
- Region of Interest (ROI) polygon zones with per-class counting
- Line crossing detection with forward/backward direction tracking
- Smart capture rules: threshold, presence, and absence triggers with cooldown
- Push-mode MJPEG streaming with detection overlays
- Hot-update ROI, line, and capture rule configuration without restarting streams
- Base64 JPEG frame snapshots on demand

## Installation

```bash
# Build the extension
./build.sh --single yolo-video-v2

# Or build all extensions
./build.sh
```

## Commands

| Command | Description | Key Parameters |
|---------|-------------|----------------|
| `start_stream` | Start a new video detection stream | `source_url` (camera://0, rtsp://...) |
| `stop_stream` | Stop an active stream | `stream_id` |
| `get_stream_stats` | Get statistics for an active stream | `stream_id` |
| `get_frame` | Get current frame as base64 JPEG | `stream_id` |
| `update_stream_config` | Hot-update ROI/line/capture rules | `stream_id`, `rois`, `lines`, `capture_rules` |
| `gc_memory` | Trigger memory cleanup | - |

## Metrics

| Metric | Type | Unit | Description |
|--------|------|------|-------------|
| `active_streams` | Integer | count | Number of currently active streams |
| `total_frames_processed` | Integer | frames | Total frames processed across all streams |
| `total_detections` | Integer | count | Total objects detected across all streams |
| `total_roi_alerts` | Integer | count | Total ROI threshold/alert events |
| `latest_capture` | String | - | JSON of the most recent capture event |

## Frontend Component

**YoloVideoDisplay** - A panel component for real-time object detection visualization on video streams. Supports configurable confidence threshold, max objects, FPS, bounding box rendering, and display stats toggle. Available in `default` and `compact` variants.

## Streaming

Supports push-mode streaming via WebSocket. Clients connect and send an `init` message to receive a live MJPEG stream with detection overlays rendered on each frame. Video processing runs on a dedicated thread per stream with configurable frame rate.

## Requirements

- ONNX Runtime (bundled via `usls` crate)
- YOLOv11n ONNX model (`yolo11n.onnx` in `models/` directory)
- FFmpeg libraries (for RTSP/HLS/RTMP decoding)
- **Note:** This extension is marked HIGH-RISK due to AI inference and multi-threaded video processing. Process isolation is recommended for production deployments.

## License

Apache-2.0
