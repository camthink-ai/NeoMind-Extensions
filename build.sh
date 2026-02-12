#!/bin/bash
# NeoMind Extensions Build Script
# Builds all extensions in the workspace and installs them

set -e

echo "Building NeoMind Extensions..."

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Parse arguments
AUTO_INSTALL=false
SKIP_INSTALL=false
for arg in "$@"; do
    case "$arg" in
        --yes|-y)
            AUTO_INSTALL=true
            ;;
        --skip-install)
            SKIP_INSTALL=true
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --yes, -y         Auto-install without prompting"
            echo "  --skip-install    Build only, skip installation"
            echo "  --help, -h        Show this help message"
            exit 0
            ;;
    esac
done

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

echo -e "${BLUE}Platform: $OS $ARCH${NC}"

# Get the library extension
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
        echo "Unknown OS: $CURRENT_OS"
        exit 1
        ;;
esac

# Build all extensions
echo ""
echo -e "${BLUE}Building all extensions...${NC}"
cargo build --release

# Find all built extensions
echo ""
echo -e "${BLUE}Built extensions:${NC}"
BUILT_EXTENSIONS=()
for lib in target/release/libneomind_extension_*."$EXT"; do
    if [ -f "$lib" ]; then
        basename "$lib"
        BUILT_EXTENSIONS+=("$lib")
    fi
done

if [ ${#BUILT_EXTENSIONS[@]} -eq 0 ]; then
    echo "Error: No extensions were built"
    exit 1
fi

echo ""
echo -e "${GREEN}Build successful!${NC}"
echo "Built ${#BUILT_EXTENSIONS[@]} extension(s)"

# Optionally install
if [ "$SKIP_INSTALL" = true ]; then
    echo ""
    echo -e "${YELLOW}Skipping installation (use --yes to auto-install)${NC}"
    exit 0
fi

if [ "$AUTO_INSTALL" = true ] || [ -n "$CI" ]; then
    # Auto-install in CI mode or when --yes is specified
    mkdir -p ~/.neomind/extensions

    echo ""
    echo -e "${BLUE}Installing extensions...${NC}"
    for lib in "${BUILT_EXTENSIONS[@]}"; do
        cp "$lib" ~/.neomind/extensions/
        echo "  ✓ $(basename "$lib")"
    done

    echo ""
    echo -e "${GREEN}Installed ${#BUILT_EXTENSIONS[@]} extension(s) to ~/.neomind/extensions/${NC}"
    echo ""
    echo -e "${YELLOW}Restart NeoMind to load the new extensions.${NC}"
else
    # Interactive prompt
    echo ""
    read -p "Install to ~/.neomind/extensions/? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        mkdir -p ~/.neomind/extensions

        echo ""
        echo -e "${BLUE}Installing extensions...${NC}"
        for lib in "${BUILT_EXTENSIONS[@]}"; do
            cp "$lib" ~/.neomind/extensions/
            echo "  ✓ $(basename "$lib")"
        done

        echo ""
        echo -e "${GREEN}Installed ${#BUILT_EXTENSIONS[@]} extension(s) to ~/.neomind/extensions/${NC}"
        echo ""
        echo -e "${YELLOW}Restart NeoMind to load the new extensions.${NC}"
    fi
fi
