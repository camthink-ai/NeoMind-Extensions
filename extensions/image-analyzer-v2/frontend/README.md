# Image Analyzer V2 Frontend

Built for the NeoMind extension runtime.

## Quick Start

```bash
# Install dependencies
npm install

# Build for production
npm run build
```

## Output

- `dist/image-analyzer-v2-components.umd.js` - UMD bundle for dashboard

## Component

### ImageAnalyzer

AI-powered image analysis with YOLOv8 object detection.

**Props:**
- `title` - Component title (default: "Image Analyzer V2")
- `dataSource` - Data source configuration with `extensionId`
- `showMetrics` - Show detection metrics (default: true)
- `maxImageSize` - Max file size in bytes (default: 10485760)
- `confidenceThreshold` - Min confidence for detections (default: 0.5)
- `variant` - Display variant: "default" or "compact" (default: "default")

## Usage

```tsx
import { ImageAnalyzer } from '@neomind/image-analyzer-v2-frontend'

<ImageAnalyzer
  dataSource={{ extensionId: 'image-analyzer-v2' }}
  confidenceThreshold={0.6}
  showMetrics={true}
/>
```

## Features

- Drag-and-drop image upload
- Real-time object detection with bounding boxes
- Detection statistics display
- Compact mode for small containers
- Dark mode support via CSS variables

## API

The component uses the standard SDK API helpers:

### `executeExtensionCommand<T>(extensionId, command, args)`

```typescript
const result = await executeExtensionCommand<AnalysisResult>(
  'image-analyzer-v2',
  'analyze_image',
  { image: base64ImageData }
)
```

### `getExtensionMetrics(extensionId)`

```typescript
const metrics = await getExtensionMetrics('image-analyzer-v2')
// Returns: { images_processed, avg_processing_time_ms, total_detections }
```
