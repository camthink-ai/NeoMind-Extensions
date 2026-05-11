# Face Recognition

Real-time face recognition with ArcFace embeddings, device camera binding, face registration gallery, and identity matching for NeoMind Edge AI.

## Features

- **SCRFD Face Detection** — Detect faces in images using the SCRFD (Sample and Computation Redistribution for Face Detection) ONNX model
- **ArcFace Embeddings** — Extract 512-dimensional face feature vectors with ArcFace for high-accuracy identity matching
- **Device Binding** — Bind camera devices for automatic, event-driven face recognition on image updates
- **Face Gallery** — Register, list, and delete named faces for persistent identity recognition
- **Configurable Thresholds** — Adjust recognition similarity threshold and max face count at runtime
- **Real-time Metrics** — Track bound devices, total inferences, recognized faces, and unknown detections

## Installation

Build from the repository root:

```bash
# Build this extension only
./build.sh --single face-recognition

# Dev build with auto-install to NeoMind
./build.sh --dev --single face-recognition

# Build all extensions
./build.sh
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `bind_device` | Bind a device for automatic face recognition | `device_id` (string), `metric_name` (string, default: `"image"`) |
| `unbind_device` | Unbind a device from face recognition | `device_id` (string) |
| `toggle_binding` | Toggle a device binding active/inactive | `device_id` (string), `active` (boolean) |
| `get_bindings` | List all device bindings and their status | — |
| `register_face` | Register a face with a name | `name` (string), `image` (base64 string) |
| `delete_face` | Delete a registered face | `face_id` (string) |
| `list_faces` | List all registered faces | — |
| `get_status` | Get extension status, model info, and statistics | — |
| `configure` | Update extension configuration | `config` (JSON: `recognition_threshold`, `max_faces`) |
| `get_config` | Get current extension configuration | — |

## Metrics

| Metric | Type | Unit | Description |
|--------|------|------|-------------|
| `bound_devices` | Integer | count | Number of currently bound devices |
| `total_inferences` | Integer | count | Total face detection/recognition inferences performed |
| `total_recognized` | Integer | count | Total faces successfully recognized |
| `total_unknown` | Integer | count | Total faces detected but not matched to a known identity |

## Frontend Component

**FaceRecognitionCard** — A widget component for real-time face recognition display. Provides device binding management, face registration gallery, and live recognition results with annotated images. Supports English and Chinese locales.

## Requirements

- **ONNX Models** — Requires SCRFD detection model (`det_10g.onnx`) and ArcFace recognition model placed in the `models/` directory. Models are lazy-loaded on first inference.
- **ONNX Runtime** — Provided via the `ort` crate (configured in workspace dependencies)

## License

Apache-2.0
