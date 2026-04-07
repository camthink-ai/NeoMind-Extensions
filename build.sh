#!/bin/bash
# NeoMind Extensions Build Script
# Unified build script for all extensions
#
# Usage:
#   ./build.sh                    # Build all, create packages
#   ./build.sh --dev              # Dev build, install to NeoMind
#   ./build.sh --release 2.4.0    # Release build with version
#   ./build.sh --single yolo-video-v2  # Build single extension
#
# For release: ./build.sh --release VERSION

set -e

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Default values
AUTO_INSTALL=false
SKIP_INSTALL=false
BUILD_FRONTEND=true
BUILD_TYPE="release"
SKIP_PACKAGE=false
DEV_MODE=false
SINGLE_EXT=""
MARKET_VERSION=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --yes|-y)
            AUTO_INSTALL=true
            shift
            ;;
        --skip-install)
            SKIP_INSTALL=true
            shift
            ;;
        --skip-frontend)
            BUILD_FRONTEND=false
            shift
            ;;
        --skip-package)
            SKIP_PACKAGE=true
            shift
            ;;
        --debug)
            BUILD_TYPE="debug"
            shift
            ;;
        --dev)
            DEV_MODE=true
            AUTO_INSTALL=true
            SKIP_PACKAGE=true
            shift
            ;;
        --release)
            BUILD_TYPE="release"
            shift
            if [[ -n "$1" && ! "$1" =~ ^- ]]; then
                MARKET_VERSION="$1"
                shift
            fi
            ;;
        --single)
            shift
            SINGLE_EXT="$1"
            shift
            ;;
        --help|-h)
            echo "NeoMind Extensions Build Script"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --yes, -y          Auto-install without prompting"
            echo "  --skip-install     Build only, skip installation"
            echo "  --skip-frontend    Skip building frontend components"
            echo "  --skip-package     Skip creating .nep packages"
            echo "  --debug            Build in debug mode"
            echo "  --dev              Dev mode: build + install to NeoMind"
            echo "  --release [VER]    Release mode, optional version for filenames"
            echo "  --single <ext>     Build single extension only"
            echo "  --help, -h         Show this help message"
            echo ""
            echo "Examples:"
            echo "  ./build.sh                           # Build all, create packages"
            echo "  ./build.sh --dev                     # Dev build, auto-install"
            echo "  ./build.sh --release 2.4.0           # Release with version"
            echo "  ./build.sh --single weather-forecast-v2  # Single extension"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

echo "======================================"
echo "NeoMind Extensions Build"
echo "======================================"
echo ""

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
        if [ "$ARCH" = "i686" ] || [ "$ARCH" = "i386" ]; then
            PLATFORM="windows_x86"
        else
            PLATFORM="windows_amd64"
        fi
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
    "yolo-device-inference"
    "ocr-device-inference"
    "wasm-demo"
)

# Filter to single extension if specified
if [ -n "$SINGLE_EXT" ]; then
    if [[ " ${V2_EXTENSIONS[@]} " =~ " ${SINGLE_EXT} " ]]; then
        V2_EXTENSIONS=("$SINGLE_EXT")
        echo -e "${BLUE}Building single extension: $SINGLE_EXT${NC}"
    else
        echo -e "${RED}Error: Unknown extension '$SINGLE_EXT'${NC}"
        echo "Available: ${V2_EXTENSIONS[*]}"
        exit 1
    fi
fi

# Build Rust extensions
echo ""
echo -e "${BLUE}Building extensions for runtime protocol v3...${NC}"

# Detect WASM extensions and build them
WASM_EXTENSIONS=()
NATIVE_EXTENSIONS=()

for ext in "${V2_EXTENSIONS[@]}"; do
    EXT_DIR="extensions/$ext"

    # Check if this is a WASM extension by reading metadata.json
    EXT_TYPE="native"
    if [ -f "$EXT_DIR/metadata.json" ]; then
        EXT_TYPE=$(jq -r '.type // "native"' "$EXT_DIR/metadata.json" 2>/dev/null)
    fi

    if [ "$EXT_TYPE" = "wasm" ]; then
        WASM_EXTENSIONS+=("$ext")
    else
        NATIVE_EXTENSIONS+=("$ext")
    fi
done

# Build native extensions
if [ ${#NATIVE_EXTENSIONS[@]} -gt 0 ]; then
    echo ""
    echo -e "${BLUE}Building Native Extensions...${NC}"

    if [ "$BUILD_TYPE" = "release" ]; then
        for ext in "${NATIVE_EXTENSIONS[@]}"; do
            echo -e "  ${BLUE}Building${NC} $ext..."
            if ! cargo build --release -p "$ext" 2>&1; then
                echo -e "  ${RED}✗${NC} $ext build failed"
            fi
        done
    else
        for ext in "${NATIVE_EXTENSIONS[@]}"; do
            echo -e "  ${BLUE}Building${NC} $ext..."
            if ! cargo build -p "$ext" 2>&1; then
                echo -e "  ${RED}✗${NC} $ext build failed"
            fi
        done
    fi
fi

# Build WASM extensions
if [ ${#WASM_EXTENSIONS[@]} -gt 0 ]; then
    echo ""
    echo -e "${BLUE}Building WASM Extensions...${NC}"

    # Check if wasm32 target is installed
    if ! rustup target list | grep -q "wasm32-unknown-unknown"; then
        echo -e "${YELLOW}Installing wasm32-unknown-unknown target...${NC}"
        rustup target add wasm32-unknown-unknown
    fi

    for ext in "${WASM_EXTENSIONS[@]}"; do
        echo -e "  ${BLUE}Building${NC} $ext (WASM)..."

        if [ "$BUILD_TYPE" = "release" ]; then
            cargo build --release -p "$ext" --target wasm32-unknown-unknown 2>/dev/null || true
        else
            cargo build -p "$ext" --target wasm32-unknown-unknown 2>/dev/null || true
        fi
    done
fi

# Find built extensions
BUILD_DIR="target/$BUILD_TYPE"
echo ""
echo -e "${BLUE}Built extensions:${NC}"

BUILT_EXTENSIONS=()

# Check native extensions
for ext in "${NATIVE_EXTENSIONS[@]}"; do
    LIB_NAME=$(echo "$ext" | tr '-' '_')
    
    # On Windows, DLL files don't have 'lib' prefix
    if [ "$LIB_EXT" = "dll" ]; then
        LIB_FILE="$BUILD_DIR/neomind_extension_${LIB_NAME}.${LIB_EXT}"
    else
        LIB_FILE="$BUILD_DIR/libneomind_extension_${LIB_NAME}.${LIB_EXT}"
    fi

    if [ -f "$LIB_FILE" ]; then
        echo -e "  ${GREEN}✓${NC} $ext -> $(basename $LIB_FILE) [native]"
        BUILT_EXTENSIONS+=("$ext")
    else
        echo -e "  ${YELLOW}⚠${NC} $ext (not found: $LIB_FILE)"
    fi
done

# Check WASM extensions
for ext in "${WASM_EXTENSIONS[@]}"; do
    LIB_NAME=$(echo "$ext" | tr '-' '_')
    # WASM files are in target/wasm32-unknown-unknown/release/ not target/release/wasm32-unknown-unknown/release/
    WASM_FILE="target/wasm32-unknown-unknown/${BUILD_TYPE}/neomind_extension_${LIB_NAME}.wasm"

    if [ -f "$WASM_FILE" ]; then
        echo -e "  ${GREEN}✓${NC} $ext -> neomind_extension_${LIB_NAME}.wasm [wasm]"
        BUILT_EXTENSIONS+=("$ext")
    else
        echo -e "  ${YELLOW}⚠${NC} $ext (not found: $WASM_FILE)"
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

        # Check if this is a WASM extension
        # WASM files are in target/wasm32-unknown-unknown/release/
        WASM_FILE="target/wasm32-unknown-unknown/${BUILD_TYPE}/neomind_extension_${LIB_NAME}.wasm"
        
        # On Windows, DLL files don't have 'lib' prefix
        if [ "$LIB_EXT" = "dll" ]; then
            NATIVE_LIB_FILE="$BUILD_DIR/neomind_extension_${LIB_NAME}.${LIB_EXT}"
        else
            NATIVE_LIB_FILE="$BUILD_DIR/libneomind_extension_${LIB_NAME}.${LIB_EXT}"
        fi

        IS_WASM=false
        if [ -f "$WASM_FILE" ]; then
            IS_WASM=true
            LIB_FILE="$WASM_FILE"
            EXT_TYPE="wasm"
            BINARY_NAME="extension.wasm"
        else
            LIB_FILE="$NATIVE_LIB_FILE"
            EXT_TYPE="native"
            BINARY_NAME="extension.${LIB_EXT}"
        fi

        # Get version from Cargo.toml (or use MARKET_VERSION for filename)
        EXT_VERSION=$(grep -m1 'version = ' "$EXT_DIR/Cargo.toml" 2>/dev/null | sed 's/.*version = "\([^"]*\)".*/\1/' || echo "0.1.0")
        # Use MARKET_VERSION for filename if provided (for releases)
        PACKAGE_VERSION="${MARKET_VERSION:-$EXT_VERSION}"

        if [ ! -f "$LIB_FILE" ]; then
            echo -e "  ${YELLOW}⚠${NC} $ext: no binary found"
            continue
        fi

        # Create temp package directory
        TEMP_DIR=$(mktemp -d)
        PACKAGE_DIR="$TEMP_DIR/$ext"

        if [ "$IS_WASM" = true ]; then
            # WASM extension - no platform-specific directory
            mkdir -p "$PACKAGE_DIR/binaries"
            mkdir -p "$PACKAGE_DIR/frontend"
        else
            # Native extension - platform-specific directory
            mkdir -p "$PACKAGE_DIR/binaries/$PLATFORM"
            mkdir -p "$PACKAGE_DIR/frontend"
        fi
        mkdir -p "$PACKAGE_DIR/models"

        # Copy binary
        if [ "$IS_WASM" = true ]; then
            cp "$LIB_FILE" "$PACKAGE_DIR/binaries/$BINARY_NAME"
        else
            cp "$LIB_FILE" "$PACKAGE_DIR/binaries/$PLATFORM/$BINARY_NAME"
        fi

        # Copy ONNX Runtime library for native extensions using ort
        if [ "$IS_WASM" = false ]; then
            ORT_LIB=""
            BINARY_DIR="$PACKAGE_DIR/binaries/$PLATFORM"

            # Check common locations for ONNX Runtime library
            if [ -n "$ORT_LIB_PATH" ] && [ -d "$ORT_LIB_PATH" ]; then
                # Use ORT_LIB_PATH if set
                # IMPORTANT: Exclude dSYM directories - they contain debug symbols, not the actual library
                if [ "$LIB_EXT" = "dylib" ]; then
                    # Prefer the unversioned symlink (libonnxruntime.dylib), fall back to versioned
                    if [ -f "$ORT_LIB_PATH/libonnxruntime.dylib" ]; then
                        ORT_LIB="$ORT_LIB_PATH/libonnxruntime.dylib"
                    else
                        ORT_LIB=$(find "$ORT_LIB_PATH" -maxdepth 1 -name "libonnxruntime*.dylib" -not -path "*/dSYM/*" 2>/dev/null | head -1)
                    fi
                elif [ "$LIB_EXT" = "so" ]; then
                    ORT_LIB=$(find "$ORT_LIB_PATH" -maxdepth 1 -name "libonnxruntime.so*" 2>/dev/null | head -1)
                elif [ "$LIB_EXT" = "dll" ]; then
                    ORT_LIB=$(find "$ORT_LIB_PATH" -maxdepth 1 -name "onnxruntime*.dll" 2>/dev/null | head -1)
                fi
            fi

            # Also check LD_LIBRARY_PATH
            if [ -z "$ORT_LIB" ] && [ -n "$LD_LIBRARY_PATH" ]; then
                IFS=':' read -ra PATHS <<< "$LD_LIBRARY_PATH"
                for p in "${PATHS[@]}"; do
                    if [ -d "$p" ]; then
                        if [ "$LIB_EXT" = "so" ]; then
                            ORT_LIB=$(find "$p" -maxdepth 1 -name "libonnxruntime.so*" 2>/dev/null | head -1)
                        fi
                        [ -n "$ORT_LIB" ] && break
                    fi
                done
            fi

            if [ -n "$ORT_LIB" ] && [ -f "$ORT_LIB" ]; then
                cp "$ORT_LIB" "$BINARY_DIR/"
                echo -e "    ${GREEN}→${NC} Bundled ONNX Runtime: $(basename $ORT_LIB)"
            fi
        fi

        # Fix the binary's LC_ID_DYLIB to use @executable_path instead of absolute path
        # This is critical for Rust cdylib which sets LC_ID_DYLIB to absolute build path
        if [ "$IS_WASM" = false ] && [ "$OS" = "Darwin" ]; then
            echo -e "    ${BLUE}→${NC} Fixing library ID for macOS..."
            
            # Get the binary path
            BINARY_PATH="$PACKAGE_DIR/binaries/$PLATFORM/$BINARY_NAME"
            
            # Get the current library ID
            CURRENT_ID=$(otool -D "$BINARY_PATH" 2>/dev/null | tail -n 1)
            
            # Check if it's an absolute path (starts with /)
            if [[ "$CURRENT_ID" == /* ]]; then
                # Extract library name from absolute path
                LIB_BASENAME=$(basename "$CURRENT_ID")
                NEW_ID="@rpath/extension.dylib"
                
                # Change the library ID
                install_name_tool -id "$NEW_ID" "$BINARY_PATH" 2>/dev/null
                
                # Re-sign the library with ad-hoc signature
                codesign --force --sign - "$BINARY_PATH" 2>/dev/null
                
                echo -e "    ${GREEN}✓${NC} Changed LC_ID_DYLIB: $CURRENT_ID"
                echo -e "    ${GREEN}✓${NC} To: $NEW_ID"
                echo -e "    ${GREEN}✓${NC} Re-signed library with ad-hoc signature"
            else
                echo -e "    ${YELLOW}⚠${NC} Library ID already uses relative path: $CURRENT_ID"
            fi
        fi


        # Fix dynamic library dependencies for portability (native only)
        # Solution: Copy self-referenced dependency libraries to the package
        if [ "$IS_WASM" = false ] && [ "$OS" = "Darwin" ]; then
            echo -e "    ${BLUE}→${NC} Fixing dynamic library dependencies..."
            
            # Get the binary path
            if [ "$IS_WASM" = true ]; then
                BINARY_PATH="$PACKAGE_DIR/binaries/$BINARY_NAME"
                BINARY_DIR="$PACKAGE_DIR/binaries"
            else
                BINARY_PATH="$PACKAGE_DIR/binaries/$PLATFORM/$BINARY_NAME"
                BINARY_DIR="$PACKAGE_DIR/binaries/$PLATFORM"
            fi
            
            # Get all dependent dylibs with absolute paths
            DEPS=$(otool -L "$BINARY_PATH" 2>/dev/null | \
                   grep -oE "/Users/[^ ]+\.dylib" || true)
            
            if [ -n "$DEPS" ]; then
                # Add @executable_path to rpath
                install_name_tool -add_rpath "@executable_path" \
                    "$BINARY_PATH" 2>/dev/null || true
                
                # Calculate hash of source library
                SOURCE_HASH=$(shasum -a 256 "$LIB_FILE" | cut -d' ' -f1)
                
                echo "$DEPS" | while read -r dep; do
                    if [ -f "$dep" ]; then
                        LIB_NAME=$(basename "$dep")
                        DEP_HASH=$(shasum -a 256 "$dep" | cut -d' ' -f1)
                        
                        if [ "$SOURCE_HASH" == "$DEP_HASH" ]; then
                            # Self-reference - copy to package and fix reference
                            cp "$dep" "$BINARY_DIR/$LIB_NAME"
                            install_name_tool -change "$dep" "@executable_path/$LIB_NAME" \
                                "$BINARY_PATH" 2>/dev/null && \
                                echo -e "    ${GREEN}→${NC} Copied and fixed: $LIB_NAME"
                        else
                            # Different library - copy to package
                            cp "$dep" "$BINARY_DIR/$LIB_NAME"
                            install_name_tool -change "$dep" "@executable_path/$LIB_NAME" \
                                "$BINARY_PATH" 2>/dev/null && \
                                echo -e "    ${GREEN}→${NC} Copied dependency: $LIB_NAME"
                        fi
                    fi
                done
            fi
        fi


        # Fix dynamic library dependencies for Linux (set rpath for bundled libraries)
        if [ "$IS_WASM" = false ] && [ "$OS" = "Linux" ]; then
            BINARY_PATH="$PACKAGE_DIR/binaries/$PLATFORM/$BINARY_NAME"
            BINARY_DIR="$PACKAGE_DIR/binaries/$PLATFORM"

            # Check if patchelf is available
            if command -v patchelf &> /dev/null; then
                # Set rpath to $ORIGIN (current directory) so the binary can find bundled libraries
                echo -e "    ${BLUE}→${NC} Setting rpath for Linux..."
                patchelf --set-rpath '$ORIGIN' "$BINARY_PATH" 2>/dev/null && \
                    echo -e "    ${GREEN}✓${NC} Set rpath to \$ORIGIN" || \
                    echo -e "    ${YELLOW}⚠${NC} Could not set rpath (may already be correct)"
            fi
        fi


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

        # Check if models are included
        HAS_MODELS="false"
        if [ -d "$EXT_DIR/models" ] && ls "$EXT_DIR/models"/*.onnx 1> /dev/null 2>&1; then
            HAS_MODELS="true"
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

            # Read global_name from vite.config.ts (the name field in build.lib)
            GLOBAL_NAME=""
            if [ -f "$EXT_DIR/frontend/vite.config.ts" ]; then
                GLOBAL_NAME=$(grep -o "name: *'[^']*'" "$EXT_DIR/frontend/vite.config.ts" 2>/dev/null | head -1 | sed "s/name: *'\\([^']*\\)'/\\1/")
                if [ -z "$GLOBAL_NAME" ]; then
                    GLOBAL_NAME=$(grep -o 'name: *"[^"]*"' "$EXT_DIR/frontend/vite.config.ts" 2>/dev/null | head -1 | sed 's/name: *"\([^"]*\)"/\1/')
                fi
            fi

            # Generate component type from extension ID
            # Use full extension ID (with hyphens converted) to ensure uniqueness
            # e.g., yolo-device-inference -> yolo-device-inference-card
            # e.g., yolo-video-v2 -> yolo-video-card (remove -v2 suffix for cleaner names)
            COMPONENT_TYPE=$(echo "$ext" | sed 's/-v2$//' | sed 's/-v1$//')"-card"

            # Convert components to dashboard_components format
            # Note: category must be one of: chart, metric, table, control, media, custom, other
            if [ -n "$GLOBAL_NAME" ]; then
                DASHBOARD_COMPONENTS=$(jq -c --arg entrypoint "$ACTUAL_ENTRYPOINT" --arg component_type "$COMPONENT_TYPE" --arg global_name "$GLOBAL_NAME" '
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
                        "global_name": $global_name,
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
                echo -e "    ${BLUE}→${NC} Global name: $GLOBAL_NAME"
            else
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
                echo -e "    ${YELLOW}⚠${NC} No global_name found in vite.config.ts"
            fi

            if [ -z "$DASHBOARD_COMPONENTS" ] || [ "$DASHBOARD_COMPONENTS" = "null" ]; then
                DASHBOARD_COMPONENTS="[]"
            fi

            echo -e "    ${BLUE}→${NC} Generated dashboard_components"
        fi

        # Build manifest JSON using jq for proper escaping
        if [ "$IS_WASM" = true ]; then
            # WASM extension - single binary, no platform directory
            MANIFEST_JSON=$(jq -n \
                --arg format "neomind-extension-package" \
                --arg format_version "2.0" \
                --argjson abi_version 3 \
                --arg id "$ext" \
                --arg name "$(echo $ext | sed 's/-v2$//' | sed 's/-/ /g')" \
                --arg version "$EXT_VERSION" \
                --arg sdk_version "2.0.0" \
                --arg type "wasm" \
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
                    binaries: { "wasm": "binaries/extension.wasm" },
                    frontend: {
                        "components": $dashboard_components
                    }
                } | if $has_models then . + {"models": "models/"} else . end')
        else
            # Native extension - platform-specific binary
            MANIFEST_JSON=$(jq -n \
                --arg format "neomind-extension-package" \
                --arg format_version "2.0" \
                --argjson abi_version 3 \
                --arg id "$ext" \
                --arg name "$(echo $ext | sed 's/-v2$//' | sed 's/-/ /g')" \
                --arg version "$EXT_VERSION" \
                --arg sdk_version "2.0.0" \
                --arg type "native" \
                --arg platform "$PLATFORM" \
                --arg lib_ext "$LIB_EXT" \
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
                    binaries: { ($platform): ("binaries/" + $platform + "/extension." + $lib_ext) },
                    frontend: {
                        "components": $dashboard_components
                    }
                } | if $has_models then . + {"models": "models/"} else . end')
        fi

        echo "$MANIFEST_JSON" > "$PACKAGE_DIR/manifest.json"

        # Create .nep package with platform suffix for native extensions
        if [ "$IS_WASM" = true ]; then
            # WASM is cross-platform, no platform suffix needed
            OUTPUT_FILE="dist/${ext}-${PACKAGE_VERSION}.nep"
        else
            # Native extensions need platform suffix
            OUTPUT_FILE="dist/${ext}-${PACKAGE_VERSION}-${PLATFORM}.nep"
        fi
        cd "$PACKAGE_DIR"

        # Use Python zipfile for reliable CRC handling
        # macOS zip command has a known bug producing incorrect CRC32 for large files
        if command -v python3 &> /dev/null; then
            python3 -c "
import zipfile, os, sys
output = '$OLDPWD/$OUTPUT_FILE'
with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as zf:
    for root, dirs, files in os.walk('.'):
        for f in sorted(files + dirs):
            fp = os.path.join(root, f)
            arcname = fp[2:]  # strip './'
            if os.path.isdir(fp):
                zf.write(fp, arcname + '/')
            else:
                zf.write(fp, arcname)
# Verify
with zipfile.ZipFile(output, 'r') as zf:
    bad = zf.testzip()
    if bad is not None:
        print(f'ERROR: CRC check failed for: {bad}', file=sys.stderr)
        sys.exit(1)
"
        elif command -v zip &> /dev/null; then
            zip -r -q "$OLDPWD/$OUTPUT_FILE" .
        elif command -v pwsh &> /dev/null; then
            # Windows: use PowerShell Compress-Archive
            pwsh -Command "Compress-Archive -Path '*' -DestinationPath '$OLDPWD/$OUTPUT_FILE' -Force"
        elif command -v powershell &> /dev/null; then
            powershell -Command "Compress-Archive -Path '*' -DestinationPath '$OLDPWD/$OUTPUT_FILE' -Force"
        else
            echo -e "${RED}Error: No zip utility available${NC}"
            exit 1
        fi
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

        echo -e "  ${GREEN}✓${NC} $ext -> dist/$(basename $OUTPUT_FILE)"
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
            # On Windows, DLL files don't have 'lib' prefix
            if [ "$LIB_EXT" = "dll" ]; then
                LIB_FILE="$BUILD_DIR/neomind_extension_${LIB_NAME}.${LIB_EXT}"
            else
                LIB_FILE="$BUILD_DIR/libneomind_extension_${LIB_NAME}.${LIB_EXT}"
            fi
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
