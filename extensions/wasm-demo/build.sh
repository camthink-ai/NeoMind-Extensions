#!/bin/bash
# Build script for WASM Demo Extension
#
# This script builds the WASM extension for the wasm32-unknown-unknown target.
#
# Prerequisites:
#   rustup target add wasm32-unknown-unknown
#
# Usage:
#   ./build.sh [--release]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Default to release build
BUILD_TYPE="${1:-release}"

echo "Building WASM Demo Extension..."
echo "Build type: $BUILD_TYPE"

# Check if wasm target is installed
if ! rustup target list | grep -q "wasm32-unknown-unknown (installed)"; then
    echo "Installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi

# Build the extension
if [ "$BUILD_TYPE" = "--debug" ]; then
    echo "Building debug version..."
    cargo build --target wasm32-unknown-unknown
    WASM_FILE="target/wasm32-unknown-unknown/debug/neomind_extension_wasm_demo.wasm"
else
    echo "Building release version..."
    cargo build --target wasm32-unknown-unknown --release
    WASM_FILE="target/wasm32-unknown-unknown/release/neomind_extension_wasm_demo.wasm"
fi

# Check if build succeeded
if [ -f "$WASM_FILE" ]; then
    WASM_SIZE=$(ls -lh "$WASM_FILE" | awk '{print $5}')
    echo ""
    echo "✅ Build successful!"
    echo "   Output: $WASM_FILE"
    echo "   Size: $WASM_SIZE"
    echo ""
    echo "To install:"
    echo "   cp $WASM_FILE /path/to/neomind/extensions/wasm-demo/"
    echo "   cp metadata.json /path/to/neomind/extensions/wasm-demo/"
else
    echo "❌ Build failed!"
    exit 1
fi