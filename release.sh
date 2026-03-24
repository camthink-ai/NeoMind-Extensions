#!/bin/bash
# NeoMind Extensions Release Script
# Simple wrapper for build.sh with release settings
#
# Usage: ./release.sh [VERSION]
# Example: ./release.sh 2.4.0

set -e

# Default version from Cargo.toml workspace
DEFAULT_VERSION="2.3.0"
VERSION="${1:-$DEFAULT_VERSION}"

echo "======================================"
echo "NeoMind Extensions Release v$VERSION"
echo "======================================"
echo ""

# Run build.sh with release settings
./build.sh --release "$VERSION" --skip-install

echo ""
echo "======================================"
echo "Release Summary"
echo "======================================"
echo ""
ls -lh dist/*.nep 2>/dev/null || echo "No packages created"
echo ""

echo "Next steps:"
echo "  1. git add . && git commit -m 'Release v$VERSION'"
echo "  2. git tag v$VERSION"
echo "  3. git push origin main --tags"
echo "  4. gh release create v$VERSION ./dist/*.nep ./dist/checksums.txt --title \"v$VERSION\""
