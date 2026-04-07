#!/bin/bash
# NeoMind Extensions Release Script
#
# Single source of truth: VERSION file at repo root
#
# VERSION ──→ Cargo.toml ──→ .nep filename (via build.sh)
#         └──→ metadata.json (download URLs)
#         └──→ index.json (market_version + URLs)
#
# This guarantees: marketplace URLs always match actual .nep packages
#
# Usage:
#   ./release.sh 2.5.0              # Sync all versions + regenerate JSON
#   ./release.sh 2.5.0 --commit     # + git commit + tag v2.5.0
#   ./release.sh 2.5.0 --publish    # + push (triggers CI build)
#   ./release.sh --check            # Verify all versions consistent
#
set -euo pipefail

GITHUB_REPO="camthink-ai/NeoMind-Extensions"
PLATFORMS=(darwin-aarch64 darwin-x86_64 linux-x86_64 linux-aarch64 windows-x86_64)
ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXT_DIR="$ROOT_DIR/extensions"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
step()  { echo -e "\n${BLUE}[$1]${NC} $2"; }
ok()    { echo -e "  ${GREEN}✓${NC} $1"; }
warn()  { echo -e "  ${YELLOW}⚠${NC} $1"; }
fail()  { echo -e "  ${RED}✗${NC} $1"; }

# ─── Helpers ───

platform_suffix() {
    case "$1" in
        linux-x86_64)  echo "linux_amd64" ;;
        linux-aarch64) echo "linux_arm64" ;;
        *)             echo "${1//-/_}" ;;
    esac
}

infer_categories() {
    case "$1" in
        *yolo*|*image-analyzer*) echo '["ai", "vision", "detection"]' ;;
        *weather*)               echo '["weather"]' ;;
        *video*)                 echo '["ai", "vision", "detection"]' ;;
        *ocr*)                   echo '["ai", "computer-vision", "device-integration"]' ;;
        *device*)                echo '["ai", "computer-vision", "device-integration"]' ;;
        *)                       echo '["utility"]' ;;
    esac
}

cargo_version() {
    grep -m1 '^version = ' "$1" | sed 's/.*"\([^"]*\)".*/\1/'
}

sed_inplace() {
    if [[ "$(uname -s)" == "Darwin" ]]; then
        sed -i '' -E "$@"
    else
        sed -i -E "$@"
    fi
}

# ─── Check: verify all versions are consistent ───

do_check() {
    local errors=0

    step "CHECK" "Verifying version consistency...\n"

    local ver_file=""
    if [ -f "$ROOT_DIR/VERSION" ]; then
        ver_file=$(tr -d '[:space:]' < "$ROOT_DIR/VERSION")
    fi

    for d in "$EXT_DIR"/*/; do
        [ -f "$d/Cargo.toml" ] || continue
        local id; id=$(basename "$d")
        local cv; cv=$(cargo_version "$d/Cargo.toml")
        local mv; mv=$(jq -r '.version // "MISSING"' "$d/metadata.json" 2>/dev/null || echo "MISSING")
        local iv; iv=$(jq -r --arg id "$id" '.extensions[] | select(.id == $id) | .version // "MISSING"' "$EXT_DIR/index.json" 2>/dev/null || echo "MISSING")

        # Check that URL release tag == URL filename version
        local url_sample; url_sample=$(jq -r --arg id "$id" \
            '.extensions[] | select(.id == $id) | .builds["darwin-aarch64"].url // ""' \
            "$EXT_DIR/index.json" 2>/dev/null || echo "")
        local url_tag=""; local url_fn_ver=""
        if [ -n "$url_sample" ]; then
            # Extract tag: .../download/v2.5.0/... → 2.5.0
            url_tag=$(echo "$url_sample" | grep -oE 'download/v[^/]+' | sed 's|download/v||')
            # Extract filename version: ext-2.5.0-platform.nep → 2.5.0
            url_fn_ver=$(echo "$url_sample" | grep -oE '/[^/]+\.nep' | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
        fi

        local bad=false
        if [ -n "$ver_file" ] && [ "$cv" != "$ver_file" ]; then
            fail "$id: VERSION=$ver_file  Cargo.toml=$cv"; bad=true
        fi
        if [ "$cv" != "$mv" ]; then
            fail "$id: Cargo.toml=$cv  metadata.json=$mv"; bad=true
        fi
        if [ "$cv" != "$iv" ]; then
            fail "$id: Cargo.toml=$cv  index.json=$iv"; bad=true
        fi
        if [ -n "$url_tag" ] && [ "$url_tag" != "$url_fn_ver" ]; then
            fail "$id: URL tag=$url_tag  filename=$url_fn_ver (TAG ≠ FILENAME)"; bad=true
        fi
        if [ -n "$url_tag" ] && [ "$url_tag" != "$cv" ]; then
            fail "$id: URL tag=$url_tag  Cargo.toml=$cv (TAG ≠ VERSION)"; bad=true
        fi

        if ! $bad; then
            ok "$id: $cv  (tag=$url_tag  file=$url_fn_ver)"
        else
            errors=$((errors + 1))
        fi
    done

    local mkt; mkt=$(jq -r '.market_version // "MISSING"' "$EXT_DIR/index.json" 2>/dev/null)
    echo ""
    echo "  VERSION file:    ${ver_file:-(not found)}"
    echo "  market_version:  $mkt"

    echo ""
    if [ $errors -gt 0 ]; then
        fail "$errors extension(s) with mismatches"
        echo "  Fix: $0 <version>"
        return 1
    fi
    ok "All versions consistent"
    return 0
}

# ─── Sync: update every version source ───

do_sync() {
    local VERSION="$1"

    echo "======================================"
    echo "NeoMind Extensions Release v$VERSION"
    echo "======================================"

    # ── 1. VERSION file ──
    step "1/4" "Updating VERSION file"
    echo "$VERSION" > "$ROOT_DIR/VERSION"
    ok "VERSION → $VERSION"

    # ── 2. Cargo.toml ──
    step "2/4" "Syncing Cargo.toml versions"
    for d in "$EXT_DIR"/*/; do
        [ -f "$d/Cargo.toml" ] || continue
        local id; id=$(basename "$d")
        local old; old=$(cargo_version "$d/Cargo.toml")

        if [ "$old" = "$VERSION" ]; then
            ok "$id: already $VERSION"
        else
            sed_inplace "s/^(version = \")[^\"]*(\".*)/\\1${VERSION}\\2/" "$d/Cargo.toml"
            ok "$id: $old → $VERSION"
        fi
    done

    # ── 3. metadata.json per extension ──
    step "3/4" "Generating metadata.json"
    for d in "$EXT_DIR"/*/; do
        [ -f "$d/Cargo.toml" ] || continue
        local id; id=$(basename "$d")
        local desc; desc=$(grep -E "^description" "$d/Cargo.toml" | head -1 | sed 's/.*=.*"\(.*\)"/\1/')
        local etype; etype=$(jq -r '.type // "native"' "$d/metadata.json" 2>/dev/null || echo "native")
        local cats; cats=$(infer_categories "$id")

        # Build download URLs — tag and filename both use $VERSION
        local builds="{}"
        for p in "${PLATFORMS[@]}"; do
            local ps; ps=$(platform_suffix "$p")
            local url
            if [ "$etype" = "wasm" ]; then
                url="https://github.com/$GITHUB_REPO/releases/download/v$VERSION/${id}-${VERSION}.nep"
            else
                url="https://github.com/$GITHUB_REPO/releases/download/v$VERSION/${id}-${VERSION}-${ps}.nep"
            fi
            builds=$(echo "$builds" | jq --arg p "$p" --arg u "$url" '. + {($p): {url: $u}}')
        done

        # Frontend info
        local fj="$d/frontend/frontend.json"
        local metadata
        metadata=$(jq -c -n \
            --arg id "$id" \
            --arg name "$(echo "$id" | sed 's/-v2$//' | sed 's/-/ /g')" \
            --arg version "$VERSION" \
            --arg desc "$desc" \
            --argjson cats "$cats" \
            --arg home "https://github.com/$GITHUB_REPO/tree/main/extensions/$id" \
            --argjson builds "$builds" \
            '{id:$id, name:$name, version:$version, description:$desc,
              author:"NeoMind Team", license:"Apache-2.0", type:"native",
              categories:$cats, homepage:$home, builds:$builds}')

        if [ -f "$fj" ]; then
            local comps; comps=$(jq -c '[.components[].name]' "$fj" 2>/dev/null || echo "[]")
            local ep;    ep=$(jq -r '.entrypoint // ""' "$fj" 2>/dev/null || echo "")
            if [ -n "$ep" ] && [ "$comps" != "[]" ]; then
                metadata=$(echo "$metadata" | jq -c --argjson c "$comps" --arg e "$ep" \
                    '. + {frontend: {components: $c, entrypoint: $e}}')
            fi
        fi

        echo "$metadata" | jq '.' > "$d/metadata.json"
        ok "$id"
    done

    # ── 4. index.json ──
    step "4/4" "Generating index.json"
    local index
    index=$(jq -c -n --arg v "$VERSION" '{version:$v, market_version:$v, extensions:[]}')

    for d in "$EXT_DIR"/*/; do
        [ -f "$d/metadata.json" ] || continue
        local id; id=$(basename "$d")
        local murl="https://raw.githubusercontent.com/$GITHUB_REPO/main/extensions/$id/metadata.json"
        local entry; entry=$(jq -c --arg murl "$murl" '. + {metadata_url: $murl}' "$d/metadata.json")
        index=$(echo "$index" | jq --argjson e "$entry" '.extensions += [$e]')
    done

    echo "$index" | jq '.' > "$EXT_DIR/index.json"
    local cnt; cnt=$(jq '.extensions | length' "$EXT_DIR/index.json")
    ok "index.json: $cnt extensions, v$VERSION"

    echo ""
    echo "======================================"
    echo "Sync complete: v$VERSION"
    echo "======================================"
}

# ─── Argument parsing ───

VERSION=""
ACTION="sync"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --check)   ACTION="check";  shift ;;
        --commit)  ACTION="commit"; shift ;;
        --publish) ACTION="publish"; shift ;;
        --help|-h)
            echo "NeoMind Extensions Release"
            echo ""
            echo "Usage:"
            echo "  $0 <version>              Sync + regenerate JSON (preview)"
            echo "  $0 <version> --commit     Commit + tag"
            echo "  $0 <version> --publish    Push to remote"
            echo "  $0 --check                Verify consistency"
            exit 0 ;;
        -*) echo "Unknown: $1" >&2; exit 1 ;;
        *)  VERSION="$1"; shift ;;
    esac
done

if [ "$ACTION" = "check" ]; then
    do_check
    exit $?
fi

if [ -z "$VERSION" ]; then
    echo "ERROR: version required" >&2
    echo "Usage: $0 <version> [--commit|--publish]" >&2
    exit 1
fi
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    echo "ERROR: invalid version: $VERSION (expected X.Y.Z)" >&2
    exit 1
fi

do_sync "$VERSION"

case "$ACTION" in
    sync)
        echo ""
        echo "Preview — files updated but NOT committed."
        echo "Next: $0 $VERSION --commit"
        ;;
    commit)
        cd "$ROOT_DIR"
        git add VERSION extensions/*/Cargo.toml extensions/*/metadata.json extensions/index.json
        git commit -m "release: v$VERSION"
        git tag -f "v$VERSION"
        echo ""
        ok "Committed + tagged v$VERSION"
        echo "Push: git push origin main --tags"
        ;;
    publish)
        cd "$ROOT_DIR"
        git add VERSION extensions/*/Cargo.toml extensions/*/metadata.json extensions/index.json
        git commit -m "release: v$VERSION"
        git tag -f "v$VERSION"
        git push origin main
        git push origin "v$VERSION" --force
        echo ""
        ok "Pushed v$VERSION — CI will build + create GitHub Release"
        ;;
esac
