#!/bin/bash
# NeoMind Extensions JSON Generator
# Generates metadata.json for each extension and updates index.json
#
# Usage: ./scripts/update-versions.sh [VERSION] [--bump-extensions]
# Example: ./scripts/update-versions.sh 2.7.0
#          ./scripts/update-versions.sh 2.7.0 --bump-extensions
#
# Options:
#   --bump-extensions   Also update version in all Cargo.toml files
#                       (use this for normal releases where all extensions share the same version)
#   --check             Only verify version consistency, don't generate files

set -e

# Parse arguments
MARKET_VERSION=""
BUMP_EXTENSIONS=false
CHECK_ONLY=false
for arg in "$@"; do
    case "$arg" in
        --bump-extensions) BUMP_EXTENSIONS=true ;;
        --check) CHECK_ONLY=true ;;
        -*)
            echo "Unknown option: $arg"
            echo "Usage: $0 [VERSION] [--bump-extensions] [--check]"
            exit 1
            ;;
        *)
            if [ -z "$MARKET_VERSION" ]; then
                MARKET_VERSION="$arg"
            else
                echo "Error: Multiple version arguments"
                exit 1
            fi
            ;;
    esac
done

# Configuration
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXTENSIONS_DIR="$(dirname "$SCRIPT_DIR")/extensions"
GITHUB_REPO="camthink-ai/NeoMind-Extensions"

if [ -z "$MARKET_VERSION" ]; then
    # Read from VERSION file
    VERSION_FILE="$(dirname "$SCRIPT_DIR")/VERSION"
    if [ -f "$VERSION_FILE" ]; then
        MARKET_VERSION=$(cat "$VERSION_FILE" | tr -d '[:space:]')
        echo "Using version from VERSION file: $MARKET_VERSION"
    else
        echo "Error: No version provided and VERSION file not found"
        echo "Usage: $0 [VERSION] [--bump-extensions] [--check]"
        exit 1
    fi
fi

REPO_ROOT="$(dirname "$SCRIPT_DIR")"

# ============================================================================
# Version consistency check
# ============================================================================
check_versions() {
    echo "========================================"
    echo "Version Consistency Check"
    echo "========================================"
    echo "Target version: $MARKET_VERSION"
    echo ""

    local all_ok=true
    local mismatches=""

    for ext_dir in "$EXTENSIONS_DIR"/*/; do
        ext_id=$(basename "$ext_dir")
        cargo_toml="$ext_dir/Cargo.toml"

        [ ! -f "$cargo_toml" ] && continue

        ext_version=$(grep -E "^version" "$cargo_toml" | head -1 | sed 's/.*=.*"\(.*\)"/\1/')

        if [ "$ext_version" != "$MARKET_VERSION" ]; then
            echo "  MISMATCH: $ext_id Cargo.toml=$ext_version (expected $MARKET_VERSION)"
            mismatches="$mismatches  $ext_id: $ext_version\n"
            all_ok=false
        fi
    done

    # Check VERSION file
    if [ -f "$REPO_ROOT/VERSION" ]; then
        file_version=$(cat "$REPO_ROOT/VERSION" | tr -d '[:space:]')
        if [ "$file_version" != "$MARKET_VERSION" ]; then
            echo "  MISMATCH: VERSION file=$file_version (expected $MARKET_VERSION)"
            all_ok=false
        fi
    fi

    if [ "$all_ok" = true ]; then
        echo "  All versions consistent!"
        return 0
    else
        echo ""
        echo "  Run with --bump-extensions to fix:"
        echo "    ./scripts/update-versions.sh $MARKET_VERSION --bump-extensions"
        return 1
    fi
}

# ============================================================================
# Bump extension versions in Cargo.toml
# ============================================================================
bump_extensions() {
    echo ""
    echo "Bumping extension versions to $MARKET_VERSION..."
    echo "=============================================="

    local count=0
    for ext_dir in "$EXTENSIONS_DIR"/*/; do
        ext_id=$(basename "$ext_dir")
        cargo_toml="$ext_dir/Cargo.toml"

        [ ! -f "$cargo_toml" ] && continue

        old_version=$(grep -E "^version" "$cargo_toml" | head -1 | sed 's/.*=.*"\(.*\)"/\1/')

        if [ "$old_version" != "$MARKET_VERSION" ]; then
            # Use sed to replace the version line
            if [[ "$OSTYPE" == "darwin"* ]]; then
                sed -i '' "s/^version = \"$old_version\"/version = \"$MARKET_VERSION\"/" "$cargo_toml"
            else
                sed -i "s/^version = \"$old_version\"/version = \"$MARKET_VERSION\"/" "$cargo_toml"
            fi
            echo "  Updated: $ext_id $old_version -> $MARKET_VERSION"
            count=$((count + 1))
        else
            echo "  Already: $ext_id $MARKET_VERSION"
        fi
    done

    # Also update VERSION file
    echo "$MARKET_VERSION" > "$REPO_ROOT/VERSION"
    echo "  Updated: VERSION file -> $MARKET_VERSION"

    echo ""
    echo "  $count extension(s) updated."
}

# ============================================================================
# Main
# ============================================================================

if [ "$CHECK_ONLY" = true ]; then
    check_versions
    exit $?
fi

if [ "$BUMP_EXTENSIONS" = true ]; then
    bump_extensions
fi

echo ""
echo "NeoMind Extensions JSON Generator"
echo "=================================="
echo "Market Version: $MARKET_VERSION"
echo ""

# Generate metadata.json for each extension
for ext_dir in "$EXTENSIONS_DIR"/*/; do
    ext_id=$(basename "$ext_dir")
    cargo_toml="$ext_dir/Cargo.toml"

    # Skip if no Cargo.toml
    if [ ! -f "$cargo_toml" ]; then
        continue
    fi

    echo "Processing $ext_id..."

    # Extract info from Cargo.toml
    version=$(grep -E "^version" "$cargo_toml" | head -1 | sed 's/.*=.*"\(.*\)"/\1/')
    description=$(grep -E "^description" "$cargo_toml" | head -1 | sed 's/.*=.*"\(.*\)"/\1/')

    # Read frontend.json if exists
    frontend_json="$ext_dir/frontend/frontend.json"
    frontend_section="null"
    if [ -f "$frontend_json" ]; then
        # Extract component names only (not full objects)
        components=$(jq -c '[.components[].name]' "$frontend_json" 2>/dev/null || echo "[]")
        entrypoint=$(jq -r '.entrypoint // ""' "$frontend_json" 2>/dev/null || echo "")

        if [ -n "$entrypoint" ]; then
            frontend_section=$(cat <<EOF
{
  "components": $components,
  "entrypoint": "$entrypoint"
}
EOF
)
        fi
    fi

    # Infer categories
    categories='["utility"]'
    if [[ "$ext_id" == *"yolo"* ]] || [[ "$ext_id" == *"image"* ]]; then
        categories='["ai", "vision", "detection"]'
    elif [[ "$ext_id" == *"weather"* ]]; then
        categories='["weather"]'
    elif [[ "$ext_id" == *"video"* ]]; then
        categories='["video", "streaming", "detection"]'
    elif [[ "$ext_id" == *"device"* ]]; then
        categories='["ai", "computer-vision", "device-integration"]'
    fi

    # Build platforms for builds field
    platforms='darwin-aarch64 darwin-x86_64 linux-x86_64 linux-aarch64 windows-x86_64'
    builds_json="{"
    first=true
    for platform in $platforms; do
        if [ "$platform" = "linux-x86_64" ]; then
            platform_suffix="linux_amd64"
        elif [ "$platform" = "linux-aarch64" ]; then
            platform_suffix="linux_arm64"
        elif [ "$platform" = "windows-x86_64" ]; then
            platform_suffix="windows_amd64"
        else
            platform_suffix=$(echo $platform | sed 's/-/_/')
        fi

        # Use extension version from Cargo.toml for filename
        url="https://github.com/$GITHUB_REPO/releases/download/v$MARKET_VERSION/${ext_id}-${version}-${platform_suffix}.nep"
        if [ "$first" = true ]; then
            builds_json+="\"$platform\":{\"url\":\"$url\"}"
            first=false
        else
            builds_json+=",\"$platform\":{\"url\":\"$url\"}"
        fi
    done
    builds_json+="}"

    # Generate base metadata.json with builds field
    cat > "$ext_dir/metadata.json" <<EOF
{
  "id": "$ext_id",
  "name": "$(echo $ext_id | sed 's/-v2$//' | sed 's/-/ /g' | sed 's/\b\(.\)/\u\1/g')",
  "version": "$version",
  "description": "$description",
  "author": "NeoMind Team",
  "license": "Apache-2.0",
  "type": "native",
  "categories": $categories,
  "homepage": "https://github.com/$GITHUB_REPO/tree/main/extensions/$ext_id",
  "builds": $builds_json
}
EOF

    # Add frontend field if exists
    if [ "$frontend_section" != "null" ]; then
        jq --argjson frontend "$frontend_section" '. + {frontend: $frontend}' \
            "$ext_dir/metadata.json" > "$ext_dir/metadata.json.tmp"
        mv "$ext_dir/metadata.json.tmp" "$ext_dir/metadata.json"
        echo "  ✓ Generated metadata.json with builds and frontend"
    else
        echo "  ✓ Generated metadata.json with builds"
    fi
done

echo ""
echo "Generating index.json..."

# Generate index.json using jq
jq -n \
    --arg version "$MARKET_VERSION" \
    --arg market_version "$MARKET_VERSION" \
    '{
        version: $version,
        market_version: $market_version,
        extensions: []
    }' > "$EXTENSIONS_DIR/index.json.tmp"

# Add each extension to index.json
for ext_dir in "$EXTENSIONS_DIR"/*/; do
    ext_id=$(basename "$ext_dir")
    metadata="$ext_dir/metadata.json"
    frontend_json="$ext_dir/frontend/frontend.json"

    if [ ! -f "$metadata" ]; then
        continue
    fi

    # Get extension version (from Cargo.toml, default to 2.0.0)
    ext_version=$(grep -E "^version" "$ext_dir/Cargo.toml" 2>/dev/null | head -1 | sed 's/.*=.*"\(.*\)"/\1/' || echo "2.0.0")

    # Build platforms
    platforms='darwin-aarch64 darwin-x86_64 linux-x86_64 linux-aarch64 windows-x86_64'
    builds="{}"
    for platform in $platforms; do
        platform_underscore=$(echo $platform | sed 's/-/_/')
        if [ "$platform" = "linux-x86_64" ]; then
            platform_suffix="linux_amd64"
        elif [ "$platform" = "linux-aarch64" ]; then
            platform_suffix="linux_arm64"
        elif [ "$platform" = "windows-x86_64" ]; then
            platform_suffix="windows_amd64"
        else
            platform_suffix=$(echo $platform | sed 's/-/_/')
        fi

        url="https://github.com/$GITHUB_REPO/releases/download/v$MARKET_VERSION/${ext_id}-${ext_version}-${platform_suffix}.nep"
        builds=$(echo "$builds" | jq --arg p "$platform" --arg u "$url" '. + {($p): {url: $u}}')
    done

    # Create extension entry
    entry=$(jq -c \
        --argjson builds "$builds" \
        --arg metadata_url "https://raw.githubusercontent.com/$GITHUB_REPO/main/extensions/$ext_id/metadata.json" \
        '
        . + {
            metadata_url: $metadata_url,
            builds: $builds
        }
        ' "$metadata")

    # Add frontend info if exists
    # API expects: { components: ["ComponentName1", "ComponentName2"], entrypoint: "file.js" }
    if [ -f "$frontend_json" ]; then
        # Extract just component names (not full objects)
        frontend_info=$(jq -c '{components: [.components[].name], entrypoint: .entrypoint}' "$frontend_json")
        entry=$(echo "$entry" | jq --argjson frontend "$frontend_info" '. + {frontend: $frontend}')
    fi

    # Append to index
    jq --argjson entry "$entry" '.extensions += [$entry]' "$EXTENSIONS_DIR/index.json.tmp" > "$EXTENSIONS_DIR/index.json.tmp2"
    mv "$EXTENSIONS_DIR/index.json.tmp2" "$EXTENSIONS_DIR/index.json.tmp"
done

mv "$EXTENSIONS_DIR/index.json.tmp" "$EXTENSIONS_DIR/index.json"
echo "✓ Generated index.json"
echo ""
echo "Done! Version: $MARKET_VERSION"
