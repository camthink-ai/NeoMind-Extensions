#!/bin/bash
# NeoMind Extensions Release Script
# Builds extensions and creates .nep packages for GitHub release

set -e

echo "======================================"
echo "NeoMind Extensions Release Builder"
echo "======================================"
echo ""

# Version from workspace
VERSION="2.2.0"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m'

# Detect current platform
OS=$(uname -s)
ARCH=$(uname -m)

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

echo -e "${BLUE}Platform: $OS $ARCH ($PLATFORM)${NC}"
echo -e "${BLUE}Version: $VERSION${NC}"
echo ""

# Check for required tools
command -v cargo >/dev/null 2>&1 || { echo -e "${RED}Error: cargo not found${NC}"; exit 1; }
command -v gh >/dev/null 2>&1 || echo -e "${YELLOW}Warning: gh CLI not found${NC}"

# V2 Extensions list
V2_EXTENSIONS=(
    "weather-forecast-v2"
    "image-analyzer-v2"
    "yolo-video-v2"
    "yolo-device-inference"
)

# Clean build artifacts
echo -e "${BLUE}Step 1: Cleaning previous builds...${NC}"
cargo clean 2>/dev/null || true
rm -rf dist/
mkdir -p dist
echo -e "${GREEN}✓ Clean completed${NC}"
echo ""

# Build Rust extensions
echo -e "${BLUE}Step 2: Building Rust extensions...${NC}"
cargo build --release
echo -e "${GREEN}✓ Rust build completed${NC}"
echo ""

# Build frontend components
echo -e "${BLUE}Step 3: Building frontend components...${NC}"
for ext in "${V2_EXTENSIONS[@]}"; do
    FRONTEND_DIR="extensions/$ext/frontend"
    if [ -d "$FRONTEND_DIR" ] && [ -f "$FRONTEND_DIR/package.json" ]; then
        echo -e "  ${BLUE}Building${NC} $ext frontend..."
        cd "$FRONTEND_DIR"
        npm install --silent 2>/dev/null || true
        npm run build 2>/dev/null && echo -e "  ${GREEN}✓${NC} $ext frontend" || echo -e "  ${YELLOW}⚠${NC} $ext frontend failed"
        cd - > /dev/null
    fi
done
echo -e "${GREEN}✓ Frontend build completed${NC}"
echo ""

# Create .nep packages
echo -e "${BLUE}Step 4: Creating .nep packages...${NC}"

BUILT_COUNT=0
for ext in "${V2_EXTENSIONS[@]}"; do
    EXT_DIR="extensions/$ext"
    LIB_NAME=$(echo "$ext" | tr '-' '_')
    LIB_FILE="target/release/libneomind_extension_${LIB_NAME}.${LIB_EXT}"

    if [ ! -f "$LIB_FILE" ]; then
        echo -e "  ${YELLOW}⚠${NC} $ext: binary not found"
        continue
    fi

    # Create temp package directory
    TEMP_DIR=$(mktemp -d)
    PACKAGE_DIR="$TEMP_DIR/$ext"
    mkdir -p "$PACKAGE_DIR/binaries/$PLATFORM"
    mkdir -p "$PACKAGE_DIR/frontend"

    # Copy binary
    cp "$LIB_FILE" "$PACKAGE_DIR/binaries/$PLATFORM/"

    # Copy frontend
    if [ -d "$EXT_DIR/frontend/dist" ]; then
        cp -r "$EXT_DIR/frontend/dist"/* "$PACKAGE_DIR/frontend/" 2>/dev/null || true
    fi

    # Copy frontend.json
    if [ -f "$EXT_DIR/frontend/frontend.json" ]; then
        cp "$EXT_DIR/frontend/frontend.json" "$PACKAGE_DIR/"
    fi

    # Check if models are included
    HAS_MODELS="false"
    if [ -d "$EXT_DIR/models" ] && ls "$EXT_DIR/models"/*.onnx 1> /dev/null 2>&1; then
        HAS_MODELS="true"
        mkdir -p "$PACKAGE_DIR/models"
        for model_file in "$EXT_DIR/models"/*.onnx; do
            if [ -f "$model_file" ]; then
                cp "$model_file" "$PACKAGE_DIR/models/"
                echo -e "    ${BLUE}→${NC} Including $(basename $model_file)"
            fi
        done
    fi

    # Generate dashboard_components from frontend.json
    DASHBOARD_COMPONENTS="[]"
    if [ -f "$EXT_DIR/frontend/frontend.json" ] && command -v jq &> /dev/null; then
        FRONTEND_JSON="$EXT_DIR/frontend/frontend.json"

        # Read entrypoint from frontend.json and resolve actual file
        ENTRYPOINT=$(jq -r '.entrypoint // ""' "$FRONTEND_JSON" 2>/dev/null)

        # Check if the entrypoint file exists, try alternate extensions if not
        ACTUAL_ENTRYPOINT="$ENTRYPOINT"
        if [ ! -f "$EXT_DIR/frontend/dist/$ENTRYPOINT" ]; then
            # Try .umd.cjs instead of .umd.js
            if [ -f "$EXT_DIR/frontend/dist/${ENTRYPOINT%.umd.js}.umd.cjs" ]; then
                ACTUAL_ENTRYPOINT="${ENTRYPOINT%.umd.js}.umd.cjs"
            fi
        fi

        # Generate component type from extension ID (e.g., weather-forecast-v2 -> weather-card)
        COMPONENT_TYPE=$(echo "$ext" | sed 's/-v2$//' | sed 's/-.*$//')"-card"

        # Convert components to dashboard_components format
        DASHBOARD_COMPONENTS=$(jq -c --arg entrypoint "$ACTUAL_ENTRYPOINT" --arg component_type "$COMPONENT_TYPE" '
            [.components[] | {
                "type": $component_type,
                "name": .displayName,
                "description": .description,
                "category": (if .type == "card" then "custom"
                             elif .type == "widget" then "custom"
                             elif .type == "panel" then "custom"
                             elif .type == "chart" then "chart"
                             elif .type == "metric" then "metric"
                             elif .type == "table" then "table"
                             elif .type == "control" then "control"
                             elif .type == "media" then "media"
                             else "other" end),
                "icon": .icon,
                "bundle_path": ("frontend/" + $entrypoint),
                "export_name": .name,
                "size_constraints": {
                    "min_w": (.minSize.width // 200),
                    "min_h": (.minSize.height // 150),
                    "default_w": (.defaultSize.width // 300),
                    "default_h": (.defaultSize.height // 200),
                    "max_w": (.maxSize.width // 800),
                    "max_h": (.maxSize.height // 600)
                },
                "has_data_source": false,
                "has_display_config": true,
                "has_actions": false,
                "max_data_sources": 0,
                "config_schema": (if .configSchema then
                    {
                        "type": "object",
                        "properties": (.configSchema | to_entries | map({
                            (.key): {
                                "type": (if .value.type == "string" then "string"
                                         elif .value.type == "number" then "number"
                                         elif .value.type == "boolean" then "boolean"
                                         else "string" end),
                                "description": .value.description,
                                "default": .value.default
                            }
                        }) | add // {})
                    }
                else null end),
                "default_config": (if .configSchema then
                    (.configSchema | to_entries | map(select(.value.default != null)) | map({
                        (.key): .value.default
                    }) | add // {})
                else null end),
                "variants": []
            }]
        ' "$FRONTEND_JSON" 2>/dev/null)

        if [ -z "$DASHBOARD_COMPONENTS" ] || [ "$DASHBOARD_COMPONENTS" = "null" ]; then
            DASHBOARD_COMPONENTS="[]"
        fi

        echo -e "    ${BLUE}→${NC} Generated dashboard_components"
    fi

    # Create manifest.json using jq for proper JSON generation
    MANIFEST_JSON=$(jq -n \
        --arg format "neomind-extension-package" \
        --arg format_version "2.0" \
        --argjson abi_version 3 \
        --arg id "$ext" \
        --arg name "$(echo $ext | sed 's/-v2$//' | sed 's/-/ /g')" \
        --arg version "$VERSION" \
        --arg sdk_version "2.0.0" \
        --arg type "native" \
        --arg platform "$PLATFORM" \
        --arg binary_name "$(basename $LIB_FILE)" \
        --argjson has_models "$HAS_MODELS" \
        --argjson dashboard_components "$DASHBOARD_COMPONENTS" \
        '{
            format: $format,
            format_version: $format_version,
            abi_version: $abi_version,
            id: $id,
            name: $name,
            version: $version,
            sdk_version: $sdk_version,
            type: $type,
            binaries: { ($platform): ("binaries/" + $platform + "/" + $binary_name) },
            frontend: {
                "components": $dashboard_components
            }
        } | if $has_models then . + {"models": "models/"} else . end')

    echo "$MANIFEST_JSON" > "$PACKAGE_DIR/manifest.json"

    # Create .nep package (without top-level directory prefix)
    OUTPUT_FILE="dist/${ext}-${VERSION}-${PLATFORM}.nep"
    cd "$PACKAGE_DIR"
    zip -r -q "$OLDPWD/$OUTPUT_FILE" .
    cd - > /dev/null

    # Calculate checksum
    if command -v sha256sum &> /dev/null; then
        CHECKSUM=$(sha256sum "$OUTPUT_FILE" | cut -d' ' -f1)
    else
        CHECKSUM=$(shasum -a 256 "$OUTPUT_FILE" | cut -d' ' -f1)
    fi
    SIZE=$(stat -f%z "$OUTPUT_FILE" 2>/dev/null || stat -c%s "$OUTPUT_FILE" 2>/dev/null)

    echo "$CHECKSUM  $(basename $OUTPUT_FILE)" >> dist/checksums.txt

    # Cleanup
    rm -rf "$TEMP_DIR"

    echo -e "  ${GREEN}✓${NC} $ext -> ${ext}-${VERSION}-${PLATFORM}.nep"
    echo "    SHA256: $CHECKSUM"
    echo "    Size: $SIZE bytes"

    BUILT_COUNT=$((BUILT_COUNT + 1))
done

echo ""
echo -e "${GREEN}✓ Created $BUILT_COUNT .nep package(s)${NC}"
echo ""

# Summary
echo -e "${BLUE}======================================"
echo "Release Summary"
echo -e "======================================${NC}"
echo ""
ls -lh dist/
echo ""

# GitHub Release Instructions
echo -e "${YELLOW}======================================"
echo "GitHub Release Instructions"
echo -e "======================================${NC}"
echo ""
echo "1. Commit and tag:"
echo "   git add . && git commit -m \"Release v$VERSION\""
echo "   git tag v$VERSION"
echo "   git push origin main --tags"
echo ""
echo "2. Create GitHub Release:"
echo "   gh release create v$VERSION \\"
echo "     --title \"NeoMind Extensions v$VERSION\" \\"
echo "     --notes \"## Downloads\n\n| File | Platform |\n|------|----------|\n$(for f in dist/*.nep; do echo "| \`$(basename $f)\` | $(echo $f | grep -o 'darwin\|linux\|windows') |"; done)\" \\"
echo "     dist/*.nep dist/checksums.txt"
echo ""

echo -e "${GREEN}Build complete!${NC}"