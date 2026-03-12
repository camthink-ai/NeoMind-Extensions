#!/bin/bash
# build-dev.sh - Development build script
# Compiles extensions and outputs directly to NeoMind/data/extensions/ directory
#
# Usage: ./build-dev.sh <extension-name> [neomind-root]
# Example: ./build-dev.sh yolo-video-v2 /path/to/NeoMind

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# NeoMind root directory - can be specified, defaults to finding NeoMind subdirectory
if [ -n "$2" ]; then
    NEOMIND_ROOT="$2"
else
    # Try 1: NeoMind subdirectory in current directory
    if [ -d "$PROJECT_ROOT/NeoMind" ]; then
        NEOMIND_ROOT="$PROJECT_ROOT/NeoMind"
    # Try 2: Parent directory
    elif [ -d "$(dirname "$PROJECT_ROOT")/NeoMind" ]; then
        NEOMIND_ROOT="$(dirname "$PROJECT_ROOT")/NeoMind"
    # Try 3: Use parent directory directly (backward compatibility)
    else
        NEOMIND_ROOT="$(dirname "$PROJECT_ROOT")"
    fi
fi

# Target directory
TARGET_DIR="$NEOMIND_ROOT/data/extensions"

echo "=========================================="
echo "  NeoMind Extension Development Build"
echo "=========================================="
echo ""
echo "Project Root: $PROJECT_ROOT"
echo "NeoMind Root: $NEOMIND_ROOT"
echo "Target Directory: $TARGET_DIR"
echo ""

# Get extension name
EXTENSION_NAME="$1"
if [ -z "$EXTENSION_NAME" ]; then
    echo "Usage: $0 <extension-name> [neomind-root]"
    echo "Example: $0 yolo-video-v2"
    echo ""
    echo "Available extensions:"
    ls -1 extensions/
    exit 1
fi

# Check if extension exists
if [ ! -d "extensions/$EXTENSION_NAME" ]; then
    echo "Error: Extension '$EXTENSION_NAME' does not exist"
    echo "Available extensions:"
    ls -1 extensions/
    exit 1
fi

# Check if NeoMind directory exists
if [ ! -d "$NEOMIND_ROOT" ]; then
    echo "Error: NeoMind root directory does not exist: $NEOMIND_ROOT"
    exit 1
fi

echo "Building extension: $EXTENSION_NAME"
echo ""

# Build extension (release mode)
cd "$PROJECT_ROOT"
cargo build --package "$EXTENSION_NAME" --release

# Get build artifact path
if [[ "$OSTYPE" == "darwin"* ]]; then
    LIB_EXT="dylib"
elif [[ "$OSTYPE" == "linux"* ]]; then
    LIB_EXT="so"
else
    LIB_EXT="dll"
fi

LIB_NAME="libneomind_extension_${EXTENSION_NAME//-/_}.$LIB_EXT"
SOURCE_LIB="$PROJECT_ROOT/target/release/$LIB_NAME"
TARGET_EXT_DIR="$TARGET_DIR/$EXTENSION_NAME"

# Check if build artifact exists
if [ ! -f "$SOURCE_LIB" ]; then
    echo "Error: Build artifact does not exist: $SOURCE_LIB"
    echo "Please check if build succeeded"
    exit 1
fi

# Create target directory
mkdir -p "$TARGET_EXT_DIR"

echo ""
echo "Deploying to: $TARGET_EXT_DIR"
echo "----------------------------------------"

# Copy build artifact
cp "$SOURCE_LIB" "$TARGET_EXT_DIR/extension.dylib"
echo "✓ Copied extension.dylib"

# Copy model files (if exist)
if [ -d "extensions/$EXTENSION_NAME/models" ]; then
    mkdir -p "$TARGET_EXT_DIR/models"
    cp -r "extensions/$EXTENSION_NAME/models/"* "$TARGET_EXT_DIR/models/" 2>/dev/null || true
    if [ "$(ls -A "$TARGET_EXT_DIR/models" 2>/dev/null)" ]; then
        echo "✓ Copied model files"
    fi
fi

# Copy frontend build artifacts (NOT source code)
# Frontend components should be in dist/ directory after building
# Copy dist contents directly to frontend/ for consistency with other extensions
if [ -d "extensions/$EXTENSION_NAME/frontend/dist" ]; then
    mkdir -p "$TARGET_EXT_DIR/frontend"
    cp -r "extensions/$EXTENSION_NAME/frontend/dist/"* "$TARGET_EXT_DIR/frontend/" 2>/dev/null || true
    if [ "$(ls -A "$TARGET_EXT_DIR/frontend" 2>/dev/null)" ]; then
        echo "✓ Copied frontend build artifacts"
    fi
fi

# Copy frontend.json (component manifest)
if [ -f "extensions/$EXTENSION_NAME/frontend/frontend.json" ]; then
    cp "extensions/$EXTENSION_NAME/frontend/frontend.json" "$TARGET_EXT_DIR/frontend.json"
    echo "✓ Copied frontend.json"
fi

# Copy/generate manifest.json
if [ -f "extensions/$EXTENSION_NAME/metadata.json" ]; then
    cp "extensions/$EXTENSION_NAME/metadata.json" "$TARGET_EXT_DIR/manifest.json"
    echo "✓ Copied manifest.json"
elif [ -f "$TARGET_EXT_DIR/manifest.json" ]; then
    echo "✓ Kept existing manifest.json"
else
    echo "⚠ Warning: No metadata.json or manifest.json found"
fi

# Copy fonts directory (if exist)
if [ -d "extensions/$EXTENSION_NAME/fonts" ]; then
    mkdir -p "$TARGET_EXT_DIR/fonts"
    cp -r "extensions/$EXTENSION_NAME/fonts/"* "$TARGET_EXT_DIR/fonts/" 2>/dev/null || true
    if [ "$(ls -A "$TARGET_EXT_DIR/fonts" 2>/dev/null)" ]; then
        echo "✓ Copied fonts files"
    fi
fi

echo "----------------------------------------"
echo ""
echo "=========================================="
echo "  Build Complete!"
echo "=========================================="
echo ""
echo "Extension deployed to: $TARGET_EXT_DIR"
echo ""
echo "Next steps:"
echo "  1. Restart NeoMind service to load the new extension"
echo "  2. Or call reload extension API in frontend (if supported)"
echo ""
echo "Tip: Add --restart parameter to auto-restart NeoMind"
echo ""
