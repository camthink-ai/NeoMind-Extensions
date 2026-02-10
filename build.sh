#!/bin/bash
# NeoMind Extension Build Script

set -e

echo "Building NeoMind Weather Extension..."

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

echo -e "${BLUE}Platform: $OS $ARCH${NC}"

# Build release
cargo build --release

# Get the output filename
case "$OS" in
    Darwin)
        EXT="dylib"
        ;;
    Linux)
        EXT="so"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        EXT="dll"
        ;;
    *)
        echo "Unknown OS: $OS"
        exit 1
        ;;
esac

LIB_NAME="libneomind_extension_weather_forecast.$EXT"
SOURCE_PATH="target/release/$LIB_NAME"

if [ ! -f "$SOURCE_PATH" ]; then
    echo "Error: Build failed, $SOURCE_NAME not found"
    exit 1
fi

echo -e "${GREEN}Build successful!${NC}"
echo "Output: $SOURCE_PATH"

# Optionally install
echo ""
read -p "Install to ~/.neomind/extensions/? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    mkdir -p ~/.neomind/extensions
    cp "$SOURCE_PATH" ~/.neomind/extensions/
    echo -e "${GREEN}Installed to ~/.neomind/extensions/${NC}"
fi
