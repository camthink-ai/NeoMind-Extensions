# Image Analyzer V2

Standalone image object detection using YOLOv11 with ONNX Runtime, CoreML/CUDA acceleration, and annotated bounding box visualization.

## Features

- Real-time object detection powered by YOLOv8 (via usls)
- Auto-detection of best inference device: CoreML on macOS, CUDA on Linux, CPU fallback
- Lazy model loading with automatic ONNX Runtime library path resolution
- Base64 image input with bounding box coordinates in detection results
- Fallback analysis when YOLO model is unavailable (image format detection)
- Configurable confidence threshold and NMS IoU threshold
- 80-class COCO dataset support (person, car, dog, etc.)

## Installation

```bash
# Build from repository root
./build.sh --single image-analyzer-v2

# Or build with Cargo directly
cargo build --release -p image-analyzer-v2
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `analyze_image` | Analyze an image and return detected objects with bounding boxes | `image` (string, required) - Base64 encoded image data |
| `reset_stats` | Reset all processing statistics | None |
| `get_status` | Get current model loading status and configuration | None |
| `reload_model` | Reload YOLO model with current configuration | None |

## Metrics

| Metric | Display Name | Type | Unit |
|--------|-------------|------|------|
| `images_processed` | Images Processed | Integer | count |
| `avg_processing_time_ms` | Avg Processing Time | Float | ms |
| `total_detections` | Total Detections | Integer | count |

## Frontend Component

**ImageAnalyzer** widget - Drag-and-drop image upload with live detection results display. Supports configurable confidence threshold, display variant (default/compact), and metrics visibility toggle.

## Requirements

- YOLOv8 ONNX model file: `models/yolov8n.onnx` (included in extension package)
- ONNX Runtime native library (bundled or in system library path)
- CoreML framework (macOS, optional) or CUDA toolkit (Linux, optional) for hardware acceleration

## License

Apache-2.0
