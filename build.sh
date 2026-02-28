#!/bin/bash
# NeoMind Extensions V2 Build Script
# Builds all V2 extensions and packages them into .nep files

set -e

echo "NeoMind Extensions V2 Build"
echo "============================"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Parse arguments
AUTO_INSTALL=false
SKIP_INSTALL=false
BUILD_FRONTEND=true
BUILD_TYPE="release"
SKIP_PACKAGE=false

for arg in "$@"; do
    case "$arg" in
        --yes|-y)
            AUTO_INSTALL=true
            ;;
        --skip-install)
            SKIP_INSTALL=true
            ;;
        --skip-frontend)
            BUILD_FRONTEND=false
            ;;
        --skip-package)
            SKIP_PACKAGE=true
            ;;
        --debug)
            BUILD_TYPE="debug"
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --yes, -y          Auto-install without prompting"
            echo "  --skip-install     Build only, skip installation"
            echo "  --skip-frontend    Skip building frontend components"
            echo "  --skip-package     Skip creating .nep packages"
            echo "  --debug            Build in debug mode"
            echo "  --help, -h         Show this help message"
            exit 0
            ;;
    esac
done

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

echo -e "${BLUE}Platform: $OS $ARCH${NC}"
echo -e "${BLUE}Build Type: $BUILD_TYPE${NC}"

# Get the library extension and platform string
case "$OS" in
    Darwin)
        if [ "$ARCH" = "arm64" ]; then
            PLATFORM="darwin_aarch64"
        else
            PLATFORM="darwin_x86_64"
        fi
        LIB_EXT="dylib"
        ;;
    Linux)
        if [ "$ARCH" = "aarch64" ]; then
            PLATFORM="linux_arm64"
        else
            PLATFORM="linux_amd64"
        fi
        LIB_EXT="so"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        PLATFORM="windows_amd64"
        LIB_EXT="dll"
        ;;
    *)
        echo -e "${RED}Unknown OS: $OS${NC}"
        exit 1
        ;;
esac

# V2 Extensions list
V2_EXTENSIONS=(
    "weather-forecast-v2"
    "image-analyzer-v2"
    "yolo-video-v2"
)

# Build Rust extensions
echo ""
echo -e "${BLUE}Building V2 Extensions (ABI Version 3)...${NC}"

if [ "$BUILD_TYPE" = "release" ]; then
    cargo build --release
else
    cargo build
fi

# Find built extensions
BUILD_DIR="target/$BUILD_TYPE"
echo ""
echo -e "${BLUE}Built extensions:${NC}"

BUILT_EXTENSIONS=()
for ext in "${V2_EXTENSIONS[@]}"; do
    LIB_NAME=$(echo "$ext" | tr '-' '_')
    LIB_FILE="$BUILD_DIR/libneomind_extension_${LIB_NAME}.${LIB_EXT}"

    if [ -f "$LIB_FILE" ]; then
        echo -e "  ${GREEN}✓${NC} $ext -> $(basename $LIB_FILE)"
        BUILT_EXTENSIONS+=("$ext")
    else
        echo -e "  ${YELLOW}⚠${NC} $ext (not found: $LIB_FILE)"
    fi
done

# Build frontend components
if [ "$BUILD_FRONTEND" = true ]; then
    echo ""
    echo -e "${BLUE}Building Frontend Components...${NC}"

    for ext in "${V2_EXTENSIONS[@]}"; do
        FRONTEND_DIR="extensions/$ext/frontend"

        if [ -d "$FRONTEND_DIR" ] && [ -f "$FRONTEND_DIR/package.json" ]; then
            echo -e "  ${BLUE}Building${NC} $ext frontend..."

            cd "$FRONTEND_DIR"

            if [ ! -d "node_modules" ]; then
                npm install --silent 2>/dev/null || {
                    echo -e "  ${YELLOW}⚠${NC} $ext frontend: npm install failed"
                    cd - > /dev/null
                    continue
                }
            fi

            npm run build 2>/dev/null && {
                echo -e "  ${GREEN}✓${NC} $ext frontend built"
            } || {
                echo -e "  ${YELLOW}⚠${NC} $ext frontend: build failed"
            }

            cd - > /dev/null
        else
            echo -e "  ${YELLOW}⚠${NC} $ext: no frontend"
        fi
    done
fi

# Package into .nep files
if [ "$SKIP_PACKAGE" = false ] && [ "$BUILD_TYPE" = "release" ]; then
    echo ""
    echo -e "${BLUE}Creating .nep Packages...${NC}"

    mkdir -p dist
    rm -f dist/*.nep dist/checksums.txt

    for ext in "${BUILT_EXTENSIONS[@]}"; do
        EXT_DIR="extensions/$ext"
        LIB_NAME=$(echo "$ext" | tr '-' '_')
        LIB_FILE="$BUILD_DIR/libneomind_extension_${LIB_NAME}.${LIB_EXT}"

        # Get version from Cargo.toml
        VERSION=$(grep -m1 'version = ' "$EXT_DIR/Cargo.toml" 2>/dev/null | sed 's/.*version = "\([^"]*\)".*/\1/' || echo "0.1.0")

        # Create temp package directory
        TEMP_DIR=$(mktemp -d)
        PACKAGE_DIR="$TEMP_DIR/$ext"
        mkdir -p "$PACKAGE_DIR/binaries/$PLATFORM"
        mkdir -p "$PACKAGE_DIR/frontend"
        mkdir -p "$PACKAGE_DIR/models"

        # Copy binary (rename to extension.{ext} for NeoMind loader)
        BINARY_NAME="extension.${LIB_EXT}"
        cp "$LIB_FILE" "$PACKAGE_DIR/binaries/$PLATFORM/$BINARY_NAME"

        # Copy frontend
        FRONTEND_DIST="$EXT_DIR/frontend/dist"
        if [ -d "$FRONTEND_DIST" ]; then
            cp -r "$FRONTEND_DIST"/* "$PACKAGE_DIR/frontend/" 2>/dev/null || true
        fi

        # Copy models from extension directory if available
        EXT_MODELS_DIR="$EXT_DIR/models"
        if [ -d "$EXT_MODELS_DIR" ]; then
            for model_file in "$EXT_MODELS_DIR"/*.onnx; do
                if [ -f "$model_file" ]; then
                    cp "$model_file" "$PACKAGE_DIR/models/"
                    echo -e "    ${BLUE}→${NC} Including $(basename $model_file)"
                fi
            done
        fi

        # Copy frontend.json
        if [ -f "$EXT_DIR/frontend/frontend.json" ]; then
            cp "$EXT_DIR/frontend/frontend.json" "$PACKAGE_DIR/"
        fi

        # Create manifest.json
        # Check if models are included
        HAS_MODELS="false"
        if [ -d "$EXT_DIR/models" ] && ls "$EXT_DIR/models"/*.onnx 1> /dev/null 2>&1; then
            HAS_MODELS="true"
        fi

        MANIFEST_EXTRA=""
        if [ "$HAS_MODELS" = "true" ]; then
            MANIFEST_EXTRA=',"models": "models/"'
        fi

        cat > "$PACKAGE_DIR/manifest.json" << EOF
{
  "format": "neomind-extension-package",
  "format_version": "2.0",
  "abi_version": 3,
  "id": "$ext",
  "name": "$(echo $ext | sed 's/-v2$//' | sed 's/-/ /g')",
  "version": "$VERSION",
  "sdk_version": "2.0.0",
  "type": "native",
  "binaries": {
    "$PLATFORM": "binaries/$PLATFORM/extension.${LIB_EXT}"
  },
  "frontend": "frontend/"$MANIFEST_EXTRA
}
EOF

        # Create .nep package (without top-level directory prefix)
        OUTPUT_FILE="dist/${ext}-${VERSION}.nep"
        cd "$PACKAGE_DIR"
        zip -r -q "$OLDPWD/$OUTPUT_FILE" .
        cd - > /dev/null

        # Calculate checksum
        if command -v sha256sum &> /dev/null; then
            CHECKSUM=$(sha256sum "$OUTPUT_FILE" | cut -d' ' -f1)
        else
            CHECKSUM=$(shasum -a 256 "$OUTPUT_FILE" | cut -d' ' -f1)
        fi
        echo "$CHECKSUM  $(basename $OUTPUT_FILE)" >> dist/checksums.txt

        # Cleanup
        rm -rf "$TEMP_DIR"

        echo -e "  ${GREEN}✓${NC} $ext -> dist/${ext}-${VERSION}.nep"
    done

    echo ""
    echo -e "${GREEN}Packages created in dist/${NC}"
fi

echo ""
echo -e "${GREEN}Build complete!${NC}"
echo "Built ${#BUILT_EXTENSIONS[@]} extension(s)"

# Installation
if [ "$SKIP_INSTALL" = true ]; then
    echo ""
    echo -e "${YELLOW}Skipping installation${NC}"
    exit 0
fi

INSTALL_DIR="$HOME/.neomind/extensions"

if [ "$AUTO_INSTALL" = true ]; then
    mkdir -p "$INSTALL_DIR"

    echo ""
    echo -e "${BLUE}Installing extensions to $INSTALL_DIR...${NC}"

    # Install from .nep packages if available
    if [ -d "dist" ] && ls dist/*.nep 1> /dev/null 2>&1; then
        for nep in dist/*.nep; do
            EXT_NAME=$(basename "$nep" .nep | sed 's/-[0-9].*//')
            EXT_INSTALL_DIR="$INSTALL_DIR/$EXT_NAME"
            mkdir -p "$EXT_INSTALL_DIR"
            
            # Extract .nep
            unzip -q -o "$nep" -d "$EXT_INSTALL_DIR"
            echo -e "  ${GREEN}✓${NC} Installed $EXT_NAME"
        done
    else
        # Fallback: copy raw binaries
        for ext in "${BUILT_EXTENSIONS[@]}"; do
            LIB_NAME=$(echo "$ext" | tr '-' '_')
            LIB_FILE="$BUILD_DIR/libneomind_extension_${LIB_NAME}.${LIB_EXT}"
            cp "$LIB_FILE" "$INSTALL_DIR/"
            echo -e "  ${GREEN}✓${NC} Installed $(basename $LIB_FILE)"
        done
    fi

    echo ""
    echo -e "${GREEN}Installation complete!${NC}"
    echo "Extensions installed to: $INSTALL_DIR"
else
    echo ""
    echo -e "${YELLOW}To install extensions, run:${NC}"
    echo "  $0 --yes"
    echo ""
    echo "Or use the .nep packages:"
    echo "  NeoMind Web UI → Extensions → Add Extension → File Mode"
fi