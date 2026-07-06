#!/usr/bin/env python3
"""
Mock PaddleOCR-VL server — used to verify the paddle-ocr-vl extension's
HTTP wiring WITHOUT deploying the real multi-GB PaddleOCR-VL model.

Returns canned OCR/table/KIE responses so the frontend card can render
end-to-end. Replace with the real PaddleOCRVL service for production.

Run:
    pip install fastapi uvicorn
    python3 mock_server.py
    # → listens on http://127.0.0.1:8000

Then point the extension's `endpoint` config at this URL (default already is).
"""

from __future__ import annotations

import base64
import io
import time
from typing import Any

try:
    from fastapi import FastAPI
    from fastapi.responses import JSONResponse
    from pydantic import BaseModel
except ImportError:
    raise SystemExit(
        "Missing deps. Install:\n  pip install fastapi uvicorn pydantic"
    )

try:
    from PIL import Image
except ImportError:
    Image = None  # Image dimension sniffing is optional


app = FastAPI(title="Mock PaddleOCR-VL", version="0.0.1-mock")


# ---------------------------------------------------------------------------
# Request schemas (mirror the Rust extension's wire format)
# ---------------------------------------------------------------------------

class OcrRequest(BaseModel):
    image_base64: str | None = None
    image_url: str | None = None
    language: str = "ch"
    use_doc_orientation_classify: bool = False
    use_doc_unwarping: bool = False


class TableRequest(BaseModel):
    image_base64: str | None = None
    image_url: str | None = None


class KieRequest(BaseModel):
    image_base64: str | None = None
    image_url: str | None = None
    schema: dict | None = None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _decode_image(b64: str | None) -> tuple[bytes, int, int]:
    """Decode base64 image. Returns (bytes, width, height)."""
    if not b64:
        return b"", 1920, 1080
    try:
        raw = base64.b64decode(b64)
    except Exception:
        return b"", 1920, 1080

    if Image is not None:
        try:
            im = Image.open(io.BytesIO(raw))
            return raw, im.width, im.height
        except Exception:
            pass
    return raw, 1920, 1080  # Fallback dims


def _ms_since(t0: float) -> float:
    return round((time.perf_counter() - t0) * 1000.0, 2)


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@app.get("/health")
def health():
    return {
        "status": "ok",
        "version": "1.6-mock",
        "model_loaded": True,
    }


@app.post("/ocr")
def ocr(req: OcrRequest):
    t0 = time.perf_counter()
    _, w, h = _decode_image(req.image_base64)

    # Canned text blocks (normalized bbox 0..1)
    items = [
        {
            "rec_text": "Hello, PaddleOCR-VL!",
            "rec_score": 0.982,
            "dt_polynomial": [
                [w * 0.05, h * 0.10],
                [w * 0.55, h * 0.10],
                [w * 0.55, h * 0.18],
                [w * 0.05, h * 0.18],
            ],
        },
        {
            "rec_text": ".invoice_no  INV-2026-0001",
            "rec_score": 0.964,
            "dt_polynomial": [
                [w * 0.05, h * 0.25],
                [w * 0.65, h * 0.25],
                [w * 0.65, h * 0.32],
                [w * 0.05, h * 0.32],
            ],
        },
        {
            "rec_text": ".date        2026-07-06",
            "rec_score": 0.958,
            "dt_polynomial": [
                [w * 0.05, h * 0.35],
                [w * 0.65, h * 0.35],
                [w * 0.65, h * 0.42],
                [w * 0.05, h * 0.42],
            ],
        },
        {
            "rec_text": "中文识别测试 你好世界",
            "rec_score": 0.971,
            "dt_polynomial": [
                [w * 0.05, h * 0.50],
                [w * 0.70, h * 0.50],
                [w * 0.70, h * 0.58],
                [w * 0.05, h * 0.58],
            ],
        },
    ]

    return {
        "results": items,
        "processing_time_ms": _ms_since(t0) + 15.0,
    }


@app.post("/table")
def table(req: TableRequest):
    t0 = time.perf_counter()
    return {
        "html": (
            "<table border='1' cellspacing='0' cellpadding='4'>"
            "<thead><tr><th>Item</th><th>Qty</th><th>Price</th></tr></thead>"
            "<tbody>"
            "<tr><td>Widget</td><td>3</td><td>$12.50</td></tr>"
            "<tr><td>Gadget</td><td>1</td><td>$49.00</td></tr>"
            "<tr><td>Total</td><td>4</td><td>$86.50</td></tr>"
            "</tbody></table>"
        ),
        "processing_time_ms": _ms_since(t0) + 22.0,
    }


@app.post("/kie")
def kie(req: KieRequest):
    t0 = time.perf_counter()
    requested_fields: list[str] = []
    if req.schema and isinstance(req.schema, dict):
        fields = req.schema.get("fields")
        if isinstance(fields, list):
            requested_fields = [str(f) for f in fields]

    canned: dict[str, Any] = {
        "invoice_no": "INV-2026-0001",
        "date": "2026-07-06",
        "total": "$86.50",
        "vendor": "Acme Corp",
        "customer": "NeoMind",
    }

    if requested_fields:
        out = {k: canned.get(k, "<not found>") for k in requested_fields}
    else:
        out = canned

    return {
        "fields": out,
        "processing_time_ms": _ms_since(t0) + 30.0,
    }


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import uvicorn

    print("=" * 60)
    print("  Mock PaddleOCR-VL server")
    print("  Listening on: http://127.0.0.1:8000")
    print("  Endpoints:")
    print("    GET  /health")
    print("    POST /ocr")
    print("    POST /table")
    print("    POST /kie")
    print()
    print("  Ctrl+C to stop")
    print("=" * 60)
    uvicorn.run(app, host="127.0.0.1", port=8000, log_level="info")
