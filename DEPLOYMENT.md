# NeoMind Extension Development and Deployment Guide

This document describes the complete NeoMind extension development and deployment workflow, ensuring unified extension paths and avoiding version conflicts.

## Directory Structure

```
NeoMindProject/
├── NeoMind/                    # Main project
│   ├── data/
│   │   └── extensions/         # ← Unified extension installation directory
│   │       ├── yolo-video-v2/
│   │       ├── image-analyzer-v2/
│   │       └── ...
│   └── ...
│
└── NeoMind-Extension/          # Extension development repository
    ├── extensions/
    │   ├── yolo-video-v2/
    │   │   ├── src/
    │   │   ├── models/
    │   │   ├── frontend/
    │   │   └── metadata.json
    │   └── ...
    ├── build.sh --dev            # Development build script
    └── build.sh --single        # Production packaging script
```

## Core Principle

**Important: Extensions are loaded only from `NeoMind/data/extensions/`**

This avoids:
- Conflicts between development and runtime directories
- Multiple versions of the same extension coexisting
- Frontend upload still loading old versions

---

## Development Workflow

### 1. Modify Extension Code

Modify code in `NeoMind-Extension/extensions/<extension-name>/`.

### 2. Build and Deploy to NeoMind

```bash
cd NeoMind-Extensions
./build.sh --dev <extension-name>
```

Example:
```bash
./build.sh --dev yolo-video-v2
```

This script will:
1. Build the extension (release mode)
2. Copy binary to `NeoMind/data/extensions/<extension-name>/extension.dylib`
3. Copy model files to `NeoMind/data/extensions/<extension-name>/models/`
4. Copy frontend files to `NeoMind/data/extensions/<extension-name>/frontend/`
5. Copy manifest.json

### 3. Restart NeoMind or Reload Extension

**Option A: Restart NeoMind (recommended)**
```bash
# Restart NeoMind service
```

**Option B: Call reload API (if extension supports)**
```bash
curl -X POST http://localhost:9375/api/extensions/<extension-id>/reload
```

---

## Production Deployment Workflow

### 1. Package Extension

```bash
cd NeoMind-Extensions
./build.sh --single <extension-name> ./dist
```

This generates `<extension-name>.nep` file (essentially a zip format).

### 2. Upload Extension via Frontend

1. Open NeoMind frontend
2. Go to Extension Management page
3. Click "Upload Extension"
4. Select the generated `.nep` file
5. Wait for installation to complete

### 3. Verify Installation

Check the extension list in frontend to confirm the new extension is installed and enabled.

---

## Extension Update Workflow

### Option 1: Frontend Upload Update (Recommended)

1. **Uninstall old version**
   - Frontend calls `/api/extensions/<id>/uninstall`
   - System will: stop process → delete package files → clean registration (the extension-private `data/` dir is PRESERVED — see NEOMIND_EXTENSION_DATA_DIR)

2. **Upload new version**
   - Frontend calls `/api/extensions/upload`
   - Upload new `.nep` package

3. **Verify**
   - Check extension version is correctly updated

### Option 2: Development Overwrite Update

```bash
# Direct compile overwrite
./build.sh --dev <extension-name>

# Restart NeoMind
```

---

## Model File Path Explanation

All extension model file path lookup logic is unified to:

### Priority 1: `NEOMIND_EXTENSION_DIR` (Runtime)
```
$NEOMIND_EXTENSION_DIR/models/<model-file>
```
This is the standard path, automatically set by the extension runner.

### Priority 2: `CARGO_MANIFEST_DIR` (Development)
```
$CARGO_MANIFEST_DIR/models/<model-file>
```
Used for running tests directly during development.

### No Longer Supported Paths
- ❌ Relative paths (`./models/`)
- ❌ Hardcoded paths
- ❌ Multi-level fallback paths

---

## Frequently Asked Questions

### Q: Why is my extension still the old version after modifying code?

**A:** Ensure you use the `./build.sh --dev` script to deploy to `NeoMind/data/extensions/`, not just compile in `NeoMind-Extension/target/`.

### Q: Extension not updated after frontend upload?

**A:** First completely uninstall the old version, then upload the new version:
```bash
# 1. Uninstall
curl -X DELETE http://localhost:9375/api/extensions/<id>

# 2. Upload
curl -X POST http://localhost:9375/api/extensions/upload \
  -H "Content-Type: application/octet-stream" \
  --data-binary @extension.nep
```

### Q: Extension process still running, file deletion failed?

**A:** This is normal. The uninstall API will stop the process first before deleting files. If manually deleting, ensure to stop NeoMind service first.

### Q: How to confirm extension is loading from the correct path?

**A:** Check NeoMind logs, you should see something like:
```
Added extension discovery directory: "data/extensions"
Loading extension in ISOLATED mode
NEOMIND_EXTENSION_DIR=/path/to/data/extensions/<extension-id>
```

---

## Script Reference

### build.sh --dev

```bash
# Usage
./build.sh --dev <extension-name> [neomind-root]

# Examples
./build.sh --dev yolo-video-v2
./build.sh --dev image-analyzer-v2 /path/to/NeoMind
```

### build.sh --single

```bash
# Usage
./build.sh --single <extension-name> [output-dir]

# Examples
./build.sh --single yolo-video-v2 ./dist
```

---

## Extension Metadata

Each extension requires a `metadata.json` file:

```json
{
  "id": "yolo-video-v2",
  "name": "YOLO Video V2",
  "version": "2.0.0",
  "description": "Real-time video stream processing",
  "author": "Your Name",
  "license": "MIT",
  "type": "native"
}
```

During installation, it will be automatically converted to `manifest.json`.

---

## Summary

| Operation | Command/Method | Output Location |
|-----------|----------------|-----------------|
| Development build | `./build.sh --dev <name>` | `NeoMind/data/extensions/<name>/` |
| Production package | `./build.sh --single <name>` | `./dist/<name>.nep` |
| Frontend upload | Upload `.nep` file | `NeoMind/data/extensions/<name>/` |
| Uninstall extension | `DELETE /api/extensions/:id` | Delete package files (data/ preserved) |

**Key: All paths unified to `NeoMind/data/extensions/`, ensuring version consistency.**
