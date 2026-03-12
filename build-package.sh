#!/bin/bash
# build-package.sh - Production packaging script
# Packages extensions into .nep (NeoMind Extension Package) files
#
# Usage: ./build-package.sh <extension-name> [output-dir]
# Example: ./build-package.sh yolo-video-v2 ./dist

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# Get extension name
EXTENSION_NAME="$1"
if [ -z "$EXTENSION_NAME" ]; then
    echo "Usage: $0 <extension-name> [output-dir]"
    echo "Example: $0 yolo-video-v2 ./dist"
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

# Output directory
OUTPUT_DIR="${2:-./dist}"
# Convert to absolute path
case "$OUTPUT_DIR" in
    /*) ;;  # Already absolute
    *) OUTPUT_DIR="$(pwd)/$OUTPUT_DIR" ;;
esac
mkdir -p "$OUTPUT_DIR"

# Temporary packaging directory
TEMP_DIR=$(mktemp -d)
PACKAGE_DIR="$TEMP_DIR/$EXTENSION_NAME"

echo "=========================================="
echo "  NeoMind Extension Production Packaging"
echo "=========================================="
echo ""
echo "Extension Name: $EXTENSION_NAME"
echo "Output Directory: $OUTPUT_DIR"
echo "Temporary Directory: $PACKAGE_DIR"
echo ""

# Create package directory structure
mkdir -p "$PACKAGE_DIR"

echo "Building extension..."
echo ""

# Build extension (release mode)
cd "$PROJECT_ROOT"
cargo build --package "$EXTENSION_NAME" --release

# Get build artifact path
if [[ "$OSTYPE" == "darwin"* ]]; then
    LIB_EXT="dylib"
    PLATFORM="darwin"
    if [[ "$(uname -m)" == "arm64" ]]; then
        PLATFORM_ARCH="darwin-aarch64"
    else
        PLATFORM_ARCH="darwin-x86_64"
    fi
elif [[ "$OSTYPE" == "linux"* ]]; then
    LIB_EXT="so"
    PLATFORM="linux"
    if [[ "$(uname -m)" == "aarch64" ]]; then
        PLATFORM_ARCH="linux-aarch64"
    else
        PLATFORM_ARCH="linux-x86_64"
    fi
else
    echo "Error: Unsupported operating system"
    exit 1
fi

LIB_NAME="libneomind_extension_${EXTENSION_NAME//-/_}.$LIB_EXT"
SOURCE_LIB="$PROJECT_ROOT/target/release/$LIB_NAME"

# Check if build artifact exists
if [ ! -f "$SOURCE_LIB" ]; then
    echo "Error: Build artifact does not exist: $SOURCE_LIB"
    exit 1
fi

echo "Assembling extension package..."
echo "----------------------------------------"

# Copy binary
mkdir -p "$PACKAGE_DIR/binaries"
cp "$SOURCE_LIB" "$PACKAGE_DIR/binaries/extension.$LIB_EXT"
echo "✓ Copied binary file"

# Copy model files (if exist)
if [ -d "extensions/$EXTENSION_NAME/models" ]; then
    mkdir -p "$PACKAGE_DIR/models"
    cp -r "extensions/$EXTENSION_NAME/models/"* "$PACKAGE_DIR/models/" 2>/dev/null || true
    if [ "$(ls -A "$PACKAGE_DIR/models" 2>/dev/null)" ]; then
        echo "✓ Copied model files"
    fi
fi

# Copy frontend build artifacts (NOT source code)
# Frontend components should be in dist/ directory after building
# Copy dist contents directly to frontend/ for consistency with other extensions
if [ -d "extensions/$EXTENSION_NAME/frontend/dist" ]; then
    mkdir -p "$PACKAGE_DIR/frontend"
    cp -r "extensions/$EXTENSION_NAME/frontend/dist/"* "$PACKAGE_DIR/frontend/" 2>/dev/null || true
    if [ "$(ls -A "$PACKAGE_DIR/frontend" 2>/dev/null)" ]; then
        echo "✓ Copied frontend build artifacts"
    fi
fi

# Copy frontend.json (component manifest)
if [ -f "extensions/$EXTENSION_NAME/frontend/frontend.json" ]; then
    cp "extensions/$EXTENSION_NAME/frontend/frontend.json" "$PACKAGE_DIR/frontend.json"
    echo "✓ Copied frontend.json"
fi

# Copy fonts directory (if exist)
if [ -d "extensions/$EXTENSION_NAME/fonts" ]; then
    mkdir -p "$PACKAGE_DIR/fonts"
    cp -r "extensions/$EXTENSION_NAME/fonts/"* "$PACKAGE_DIR/fonts/" 2>/dev/null || true
    if [ "$(ls -A "$PACKAGE_DIR/fonts" 2>/dev/null)" ]; then
        echo "✓ Copied fonts files"
    fi
fi

# Copy and convert metadata.json -> manifest.json
if [ -f "extensions/$EXTENSION_NAME/metadata.json" ]; then
    cp "extensions/$EXTENSION_NAME/metadata.json" "$PACKAGE_DIR/manifest.json"
    echo "✓ Copied manifest.json"
else
    echo "⚠ Warning: No metadata.json found"
fi

# Copy README (if exist)
if [ -f "extensions/$EXTENSION_NAME/README.md" ]; then
    cp "extensions/$EXTENSION_NAME/README.md" "$PACKAGE_DIR/README.md"
    echo "✓ Copied README.md"
fi

# Create package.json (package metadata)
cat > "$PACKAGE_DIR/package.json" << EOF
{
    "name": "$EXTENSION_NAME",
    "version": "$(date +%Y%m%d%H%M%S)",
    "platform": "$PLATFORM_ARCH",
    "build_time": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "source": "NeoMind-Extension"
}
EOF
echo "✓ Created package.json"

echo "----------------------------------------"
echo ""

# Calculate checksum
CHECKSUM=$(shasum -a 256 "$SOURCE_LIB" | cut -d' ' -f1)
echo "Binary SHA256: $CHECKSUM"

# Package into .nep file (actually zip)
NEP_FILE="$OUTPUT_DIR/${EXTENSION_NAME}.nep"
echo ""
echo "Creating .nep package..."
cd "$TEMP_DIR"
if zip -r "$NEP_FILE" "$EXTENSION_NAME" > /dev/null 2>&1; then
    echo "✓ Created .nep package"
else
    echo "Error: Failed to create .nep package"
    rm -rf "$TEMP_DIR"
    exit 1
fi

# Clean up temporary directory
rm -rf "$TEMP_DIR"

echo ""
echo "=========================================="
echo "  Packaging Complete!"
echo "=========================================="
echo ""
echo "Extension package created: $NEP_FILE"
echo ""
echo "Next steps:"
echo "  1. Upload .nep file to NeoMind via frontend"
echo "  2. Or manually copy to NeoMind/data/extensions/ and extract"
echo ""
echo "Extract command (manual installation):"
echo "  unzip -o $NEP_FILE -d NeoMind/data/extensions/"
echo ""
