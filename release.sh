#!/bin/bash
# NeoMind Extensions Release Script
# Builds extensions for multiple platforms and prepares for GitHub release

set -e

echo "======================================"
echo "NeoMind Extensions Release Builder"
echo "======================================"
echo ""

# Version from workspace
VERSION="0.1.0"
REPO_NAME="NeoMind-Extensions"
GITHUB_ORG="camthink-ai"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m'

# Detect current platform
CURRENT_OS=$(uname -s)
CURRENT_ARCH=$(uname -m)

echo -e "${BLUE}Current platform: $CURRENT_OS $CURRENT_ARCH${NC}"
echo -e "${BLUE}Target version: $VERSION${NC}"
echo ""

# Check for required tools
command -v cargo >/dev/null 2>&1 || { echo -e "${RED}Error: cargo not found${NC}"; exit 1; }
command -v gh >/dev/null 2>&1 || { echo -e "${YELLOW}Warning: gh CLI not found (needed for GitHub release)${NC}"; }

# Clean build artifacts
echo -e "${BLUE}Step 1: Cleaning previous builds...${NC}"
cargo clean
rm -rf dist/
mkdir -p dist
echo -e "${GREEN}✓ Clean completed${NC}"
echo ""

# Build for current platform
echo -e "${BLUE}Step 2: Building extensions for $CURRENT_OS $CURRENT_ARCH...${NC}"
cargo build --release

# Determine library extension
case "$CURRENT_OS" in
    Darwin)
        EXT="dylib"
        PLATFORM_NAME="darwin-$CURRENT_ARCH"
        ;;
    Linux)
        EXT="so"
        PLATFORM_NAME="linux-$CURRENT_ARCH"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        EXT="dll"
        PLATFORM_NAME="windows-$CURRENT_ARCH"
        ;;
    *)
        echo -e "${RED}Unknown OS: $CURRENT_OS${NC}"
        exit 1
        ;;
esac

# Copy built extensions to dist/
echo ""
echo -e "${BLUE}Step 3: Organizing built files...${NC}"
BUILT_COUNT=0
for lib in target/release/libneomind_extension_*."$EXT"; do
    if [ -f "$lib" ]; then
        basename_lib=$(basename "$lib")
        platform_lib="dist/${basename_lib%.*}-$PLATFORM_NAME.$EXT"

        cp "$lib" "$platform_lib"

        # Calculate SHA256
        if command -v shasum >/dev/null 2>&1; then
            SHA256=$(shasum -a 256 "$platform_lib" | awk '{print $1}')
            SIZE=$(stat -f%z "$platform_lib" 2>/dev/null || stat -c%s "$platform_lib" 2>/dev/null)

            echo "  ✓ $basename_lib"
            echo "    SHA256: $SHA256"
            echo "    Size: $SIZE bytes"

            # Save checksum info
            echo "$basename_lib|$PLATFORM_NAME|$SHA256|$SIZE" >> dist/checksums.txt
        fi

        BUILT_COUNT=$((BUILT_COUNT + 1))
    fi
done

echo ""
echo -e "${GREEN}✓ Built $BUILT_COUNT extension(s)${NC}"
echo ""

# Show what's built
echo -e "${BLUE}Built files:${NC}"
ls -lh dist/
echo ""

# Instructions for multi-platform build
echo -e "${YELLOW}======================================"
echo "Multi-Platform Build Instructions"
echo -e "======================================${NC}"
echo ""
echo "To build for all platforms, you need to:"
echo ""
echo "1. macOS (Apple Silicon):"
echo "   Run this script on macOS ARM64"
echo ""
echo "2. macOS (Intel):"
echo "   Run this script on macOS x86_64"
echo "   or use: rustup target add x86_64-apple-darwin"
echo ""
echo "3. Linux (x86_64):"
echo "   - Use Docker: docker run --rm -v \$(pwd):/work -w /work rust:latest cargo build --release"
echo "   - Or use GitHub Actions CI/CD"
echo ""
echo "4. Windows (x86_64):"
echo "   Run this script on Windows"
echo ""

# GitHub Release Instructions
echo -e "${YELLOW}======================================"
echo "GitHub Release Instructions"
echo -e "======================================${NC}"
echo ""
echo "After building for all platforms:"
echo ""
echo "1. Update metadata.json with SHA256 checksums:"
echo "   - Edit extensions/*/metadata.json"
echo "   - Update builds.{platform}.sha256"
echo "   - Update builds.{platform}.size"
echo ""
echo "2. Commit changes:"
echo "   git add ."
echo "   git commit -m \"Release v$VERSION\""
echo "   git push origin main"
echo ""
echo "3. Create GitHub Release:"
echo "   gh release create v$VERSION \\"
echo "     --title \"NeoMind Extensions v$VERSION\" \\"
echo "     --notes \"See README.md for installation instructions\" \\"
echo "     dist/*"
echo ""

# Quick release command (if checksums.txt exists)
if [ -f "dist/checksums.txt" ]; then
    echo -e "${BLUE}Checksum summary for metadata.json:${NC}"
    echo ""
    while IFS='|' read -r file platform sha256 size; do
        ext_id=$(echo "$file" | sed 's/libneomind_extension_//' | sed 's/\.[^.]*$//')
        echo "  $ext_id ($platform):"
        echo "    sha256: \"$sha256\""
        echo "    size: $size"
    done < dist/checksums.txt
    echo ""
fi

echo -e "${GREEN}======================================"
echo "Build complete!"
echo -e "======================================${NC}"
