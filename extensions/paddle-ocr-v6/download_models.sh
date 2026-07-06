#!/bin/bash
# Download PP-OCRv6 ONNX models for the paddle-ocr-v6 extension.
#
# Usage:
#   ./download_models.sh              # download tiny tier (default, shipped in .nep)
#   ./download_models.sh small        # download small tier
#   ./download_models.sh medium       # download medium tier
#   ./download_models.sh all          # download all three tiers
#
# Models cache to ./models/. Tiny tier (~6 MB total) is bundled into
# the .nep package by build.sh. Small/medium are lazy-downloaded at
# runtime by the extension's downloader.rs module.

set -e

MODELS_DIR="$(dirname "$0")/models"
mkdir -p "$MODELS_DIR"

TIER="${1:-tiny}"

HF_BASE="https://huggingface.co/PaddlePaddle/PP-OCRv6"
DICT_BASE="https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/dict"

download_tier() {
    local tier=$1
    echo "=== Downloading PP-OCRv6 ${tier} tier ==="

    local det_file="ppocr-v6-${tier}-det.onnx"
    local rec_file="ppocr-v6-${tier}-rec.onnx"
    local dict_file
    if [ "$tier" = "tiny" ]; then
        dict_file="ppocrv6_tiny_dict.txt"
    else
        dict_file="ppocrv6_dict.txt"
    fi

    if [ ! -f "$MODELS_DIR/$det_file" ]; then
        echo "  Downloading $det_file ..."
        curl -L --fail -o "$MODELS_DIR/$det_file" \
            "${HF_BASE}_${tier}_det_onnx/resolve/main/inference.onnx"
    else
        echo "  ✓ $det_file already present"
    fi

    if [ ! -f "$MODELS_DIR/$rec_file" ]; then
        echo "  Downloading $rec_file ..."
        curl -L --fail -o "$MODELS_DIR/$rec_file" \
            "${HF_BASE}_${tier}_rec_onnx/resolve/main/inference.onnx"
    else
        echo "  ✓ $rec_file already present"
    fi

    if [ ! -f "$MODELS_DIR/$dict_file" ]; then
        echo "  Downloading $dict_file ..."
        curl -L --fail -o "$MODELS_DIR/$dict_file" "${DICT_BASE}/${dict_file}"
    else
        echo "  ✓ $dict_file already present"
    fi

    echo "  ✓ ${tier} tier ready:"
    ls -lh "$MODELS_DIR/$det_file" "$MODELS_DIR/$rec_file" "$MODELS_DIR/$dict_file" \
        | awk '{print "    "$5"  "$9}'
}

case "$TIER" in
    tiny|small|medium)
        download_tier "$TIER"
        ;;
    all)
        download_tier tiny
        download_tier small
        download_tier medium
        ;;
    *)
        echo "Unknown tier: $TIER"
        echo "Usage: $0 [tiny|small|medium|all]"
        exit 1
        ;;
esac

echo ""
echo "Done. Models cached in: $MODELS_DIR"
