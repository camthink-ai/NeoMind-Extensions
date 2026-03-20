# NeoMind Extensions - User Guide

This guide explains how to find, install, and use NeoMind extensions.

[中文指南](USER_GUIDE.zh.md)

---

## Table of Contents

1. [What are NeoMind Extensions?](#what-are-neomind-extensions)
2. [Installing Extensions](#installing-extensions)
3. [Available Extensions](#available-extensions)
4. [Using Extensions](#using-extensions)
5. [Troubleshooting](#troubleshooting)

---

## What are NeoMind Extensions?

NeoMind extensions run on the shared extension runtime using the isolated runtime protocol.

### Key Features

| Feature | Description |
|---------|-------------|
| **Shared Runtime Model** | Single runtime model for Native and WASM targets |
| **Runtime Protocol v3** | Isolated extension protocol with improved safety |
| **Frontend Components** | React-based dashboard widgets |
| **CSS Theming** | Light/dark mode support |

### Extension Types

| Type | File Extension | Performance | Safety |
|------|---------------|-------------|--------|
| **Native** | `.dylib` / `.so` / `.dll` | Maximum | Standard |
| **WASM** | `.wasm` | 90-95% | Sandboxed |

---

## Installing Extensions

### Method 1: Copy to Extensions Directory

```bash
# Build extensions
make build

# Install to NeoMind
make install

# Or manually
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_*.dylib ~/.neomind/extensions/
```

### Method 2: Using the Build Script

```bash
# Build and auto-install
./build.sh --yes

# Build only (skip installation)
./build.sh --skip-install

# Build with frontend
./build.sh

# Build without frontend
./build.sh --skip-frontend
```

### Method 3: Package Installation

```bash
# Package a specific extension
bash scripts/package.sh -d extensions/weather-forecast-v2

# Install the .nep package via NeoMind Web UI
# Extensions → Add Extension → File Mode → Upload
```

---

## Available Extensions

### Weather Forecast V2

**ID**: `weather-forecast-v2`

Real-time weather data using Open-Meteo API.

| Command | Description |
|---------|-------------|
| `get_weather` | Get current weather for any city |

| Metric | Description |
|--------|-------------|
| `temperature_c` | Temperature in Celsius |
| `humidity_percent` | Relative humidity |
| `wind_speed_kmph` | Wind speed in km/h |

**Frontend**: WeatherCard - Beautiful weather display

```bash
# Build
cargo build --release -p neomind-weather-forecast-v2
```

---

### Image Analyzer V2

**ID**: `image-analyzer-v2`

AI-powered image analysis using YOLOv8.

| Command | Description |
|---------|-------------|
| `analyze_image` | Analyze image for objects |

| Metric | Description |
|--------|-------------|
| `images_processed` | Total images processed |
| `total_detections` | Objects detected |
| `avg_processing_time_ms` | Average processing time |

**Frontend**: ImageAnalyzer - Drag-drop upload with detection boxes

```bash
# Build
cargo build --release -p neomind-image-analyzer-v2
```

---

### YOLO Video V2

**ID**: `yolo-video-v2`

Real-time video processing with YOLOv11.

| Command | Description |
|---------|-------------|
| `start_stream` | Start video stream processing |
| `stop_stream` | Stop video stream |
| `get_stream_stats` | Get stream statistics |

| Metric | Description |
|--------|-------------|
| `active_streams` | Number of active streams |
| `total_frames_processed` | Total frames processed |
| `avg_fps` | Average frames per second |

**Frontend**: YoloVideoDisplay - MJPEG stream with real-time detection

```bash
# Build
cargo build --release -p neomind-yolo-video-v2
```

---

## Using Extensions

### Via NeoMind Web UI

1. Open NeoMind Web UI (default: `http://localhost:9375`)
2. Navigate to **Extensions**
3. View installed extensions and their status
4. Add dashboard widgets from extension components

### Via API

```bash
# List extensions
curl http://localhost:9375/api/extensions

# Execute extension command
curl -X POST http://localhost:9375/api/extensions/weather-forecast-v2/command \
  -H "Content-Type: application/json" \
  -d '{"command": "get_weather", "args": {"city": "Beijing"}}'

# Get extension metrics
curl http://localhost:9375/api/extensions/image-analyzer-v2/metrics
```

### Via Dashboard

V2 extensions provide React components for the dashboard:

1. Go to **Dashboard**
2. Click **Add Widget**
3. Select extension component (e.g., "Weather Card")
4. Configure and save

---

## Troubleshooting

### Extension Not Loading

**Symptom**: Extension shows as "Failed to load"

**Solutions**:
1. Check runtime protocol compatibility: extension packages must target runtime protocol v3
2. Verify binary format matches platform
3. Check NeoMind logs: `tail -f ~/.neomind/logs/extension.log`

### Frontend Component Not Displaying

**Symptom**: Dashboard widget shows blank or error

**Solutions**:
1. Verify frontend files exist in extension directory
2. Check browser console for errors
3. Rebuild frontend: `./build.sh`

### Performance Issues

**Symptom**: Extension runs slowly

**Solutions**:
1. Use Native instead of WASM for compute-heavy tasks
2. Enable process isolation for AI extensions
3. Check system resources

---

## Build Commands Summary

```bash
# Build all extensions
make build

# Build specific extension
cargo build --release -p neomind-weather-forecast-v2

# Build and install
./build.sh --yes

# Clean build artifacts
make clean

# Run tests
make test

# Format code
make fmt
```

---

## Support

- **Documentation**: [EXTENSION_GUIDE.md](EXTENSION_GUIDE.md)
- **Issues**: GitHub Issues
- **License**: MIT
