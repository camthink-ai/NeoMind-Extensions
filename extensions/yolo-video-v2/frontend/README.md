# YOLO Video V2 Frontend

Built for the NeoMind extension runtime.

## Quick Start

```bash
# Install dependencies
npm install

# Build for production
npm run build
```

## Output

- `dist/yolo-video-v2-components.umd.js` - UMD bundle for dashboard

## Component

### YoloVideoDisplay

Real-time video stream display with YOLOv11 object detection.

**Props:**
- `title` - Component title (default: "YOLO Video V2")
- `dataSource` - Data source configuration with `extensionId`
- `sourceUrl` - Video source URL (camera://0, rtsp://..., hls://...)
- `videoSource` - Source type: "camera", "rtsp", "hls", "rtmp" (default: "camera")
- `confidenceThreshold` - Min confidence for detections (default: 0.5)
- `maxObjects` - Max objects per frame (default: 20)
- `fps` - Target FPS (default: 15)
- `drawBoxes` - Draw bounding boxes (default: true)
- `showStats` - Show statistics bar (default: true)
- `variant` - Display variant: "default" or "compact" (default: "default")

## Usage

```tsx
import { YoloVideoDisplay } from '@neomind/yolo-video-v2-frontend'

<YoloVideoDisplay
  dataSource={{ extensionId: 'yolo-video-v2' }}
  sourceUrl="camera://0"
  confidenceThreshold={0.6}
  fps={30}
  showStats={true}
/>
```

## Features

- Zero-processing frontend (MJPEG stream display)
- Support for multiple video sources (Camera, RTSP, HLS, RTMP)
- Real-time detection statistics
- Live indicator with session time
- Detected objects summary
- Compact mode for small containers
- Dark mode support via CSS variables

## API

The component uses standard SDK API helpers:

### `executeExtensionCommand<T>(extensionId, command, args)`

```typescript
// Start stream
const streamInfo = await executeExtensionCommand<StreamInfo>(
  'yolo-video-v2',
  'start_stream',
  {
    source_url: 'camera://0',
    confidence_threshold: 0.5,
    max_objects: 20,
    target_fps: 15,
    draw_boxes: true
  }
)

// Stop stream
await executeExtensionCommand(
  'yolo-video-v2',
  'stop_stream',
  { stream_id: 'stream-uuid' }
)

// Get stream stats
const stats = await executeExtensionCommand<StreamStats>(
  'yolo-video-v2',
  'get_stream_stats',
  { stream_id: 'stream-uuid' }
)
```
