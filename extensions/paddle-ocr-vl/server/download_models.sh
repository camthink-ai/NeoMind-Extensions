#!/usr/bin/env bash
# Pre-download PaddleOCR-VL 1.6 models so the server doesn't have to on first request.
#
# Run AFTER installing paddleocr + paddlepaddle:
#   pip install -r requirements.txt
#   ./download_models.sh
#
# This invokes the official PaddleOCR-VL pipeline once on a demo image — the
# pipeline auto-downloads all model weights (~1-2 GB total) into the local
# PaddleOCR cache directory. Subsequent server starts will load from cache.

set -euo pipefail

DEMO_URL="https://paddle-model-ecology.bj.bcebos.com/paddlex/imgs/demo_image/paddleocr_vl_demo.png"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "==> Pre-downloading PaddleOCR-VL 1.6 models..."
echo "    Target: \$HOME/.paddlex (~1-2 GB)"
echo "    Demo image: $DEMO_URL"
echo

python3 - <<PYEOF
from paddleocr import PaddleOCRVL

print("Instantiating PaddleOCRVL (pipeline_version='v1.6')...")
pipeline = PaddleOCRVL(pipeline_version="v1.6")

print("Running one inference to trigger model download...")
output = pipeline.predict("$DEMO_URL")
for res in output:
    n = len(getattr(res, "parsing_res_list", []) or [])
    print(f"  -> OK: {n} layout elements recognized")

print("\nModels cached. You can now start the server:")
print("  python3 server.py")
PYEOF

echo
echo "==> Done."
