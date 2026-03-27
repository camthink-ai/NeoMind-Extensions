#!/bin/bash
# Download OCR models for NeoMind OCR Device Inference Extension
# Models from usls project: https://github.com/jamjamjon/assets

MODELS_DIR="$(dirname "$0")/models"
mkdir -p "$MODELS_DIR"

# PP-OCR ONNX models from usls GitHub releases
DET_MODEL="https://github.com/jamjamjon/assets/releases/download/db/ppocr-v4-ch.onnx"
REC_MODEL_CH="https://github.com/jamjamjon/assets/releases/download/svtr/ppocr-v4-ch.onnx"
REC_MODEL_EN="https://github.com/jamjamjon/assets/releases/download/svtr/ppocr-v4-en.onnx"

echo "Downloading OCR models to $MODELS_DIR..."

# Download detection model (DB - Differentiable Binarization, language-agnostic)
if [ ! -f "$MODELS_DIR/det_mv3_db.onnx" ]; then
    echo "Downloading text detection model (DB - PP-OCRv4)..."
    curl -L -o "$MODELS_DIR/det_mv3_db.onnx" "$DET_MODEL"
fi

# Download Chinese recognition model (SVTR - Scene Text Recognition)
if [ ! -f "$MODELS_DIR/rec_svtr.onnx" ]; then
    echo "Downloading Chinese text recognition model (SVTR - PP-OCRv4)..."
    curl -L -o "$MODELS_DIR/rec_svtr.onnx" "$REC_MODEL_CH"
fi

# Download English recognition model
if [ ! -f "$MODELS_DIR/rec_en.onnx" ]; then
    echo "Downloading English text recognition model (SVTR - PP-OCRv4)..."
    curl -L -o "$MODELS_DIR/rec_en.onnx" "$REC_MODEL_EN"
fi

# Download character dictionary
if [ ! -f "$MODELS_DIR/vocab.txt" ]; then
    echo "Downloading character dictionary..."
    curl -L -o "$MODELS_DIR/vocab.txt" "https://github.com/jamjamjon/assets/releases/download/svtr/vocab-v1-ppocr-rec-ch.txt"
fi

echo "OCR models downloaded successfully!"
ls -lh "$MODELS_DIR/"
