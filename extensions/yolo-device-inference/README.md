# YOLO Device Inference Extension

This extension provides automatic YOLOv8 object detection on device image data sources.

## Features

- **Device Binding**: Bind devices with image data sources for automatic inference
- **Automatic Inference**: Run YOLO detection when image data is updated
- **Virtual Metrics**: Store detection results as virtual metrics on devices
- **Real-time Monitoring**: Monitor inference statistics and device status
- **Frontend Component**: React component for managing device bindings

## Installation

1. Build the extension:
   ```bash
   cd NeoMind-Extension
   ./build.sh
   ```

2. Install via NeoMind Web UI or copy the `.nep` file to `~/.neomind/extensions/`

## Usage

### Binding a Device

```json
{
  "command": "bind_device",
  "args": {
    "device_id": "camera-01",
    "image_metric": "snapshot",
    "result_metric_prefix": "yolo_",
    "confidence_threshold": 0.25,
    "draw_boxes": true
  }
}
```

### Commands

| Command | Description |
|---------|-------------|
| `bind_device` | Bind a device for automatic inference |
| `unbind_device` | Remove a device binding |
| `get_bindings` | Get all device bindings and status |
| `analyze_image` | Manually analyze an image (base64) |
| `get_status` | Get extension status |
| `toggle_binding` | Pause/resume a binding |

### Metrics

| Metric | Description |
|--------|-------------|
| `bound_devices` | Number of devices bound |
| `total_inferences` | Total inference count |
| `total_detections` | Total objects detected |
| `total_errors` | Total errors |

## Frontend Component

The extension includes a React component `DeviceBindingCard` for managing bindings:

```tsx
import { DeviceBindingCard } from '@neomind/yolo-device-inference-frontend';

<DeviceBindingCard
  executeCommand={handleCommand}
  devices={deviceList}
  onBindingChange={handleBindingChange}
/>
```

## Requirements

- YOLOv8 ONNX model file (`yolov8n.onnx`) in the `models/` directory
- Native platform (not WASM compatible)

## Model Download

```bash
# Download YOLOv8 nano model
wget https://github.com/ultralytics/assets/releases/download/v0.0.0/yolov8n.onnx
mv yolov8n.onnx extensions/yolo-device-inference/models/
```

## License

MIT
