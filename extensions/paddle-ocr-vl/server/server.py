#!/usr/bin/env python3
"""
PaddleOCR-VL inference service for the paddle-ocr-vl NeoMind extension.

Endpoints (consumed by the Rust extension's HTTP client):
  GET  /health          → service liveness probe
  POST /ocr             → text OCR with bbox (text_blocks)
  POST /table           → table structure recognition (HTML)
  POST /kie             → key information extraction (best-effort)
  POST /markdown        → full document parsing (Markdown)

Quick start:
  pip install paddlepaddle==3.2.1   # CPU; use paddlepaddle-gpu for CUDA
  pip install -r requirements.txt
  ./download_models.sh              # optional: pre-warm model cache
  python3 server.py                 # → listens on 127.0.0.1:8000
"""

from __future__ import annotations

import base64
import io
import logging
import os
import tempfile
import time
from typing import Any

# -----------------------------------------------------------------------
# Lazy imports — `paddleocr` and `paddlepaddle` are heavy. Import on startup
# so the service can still report /health even if deps are missing.
# -----------------------------------------------------------------------

logger = logging.getLogger("paddle-ocr-vl-server")
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(levelname)-7s  %(name)s  %(message)s",
)

try:
    from fastapi import FastAPI, HTTPException
    from fastapi.responses import JSONResponse
    from pydantic import BaseModel
except ImportError as e:
    raise SystemExit(f"Missing fastapi/uvicorn/pydantic: {e}. Run: pip install -r requirements.txt")

try:
    from PIL import Image
except ImportError:
    Image = None  # Soft dep — only needed for base64 decoding

# Lazy-loaded pipeline cache keyed on (rotate, dewarp).
# PaddleOCRVL instantiates doc-preprocessor models only when the corresponding
# flag is True at construction time — per-call predict() kwargs can disable
# them, but cannot enable what was never loaded. So we maintain one pipeline
# per unique flag combo, evicting the oldest when MAX_PIPELINES is exceeded.
_PIPELINE_CACHE: dict[tuple[bool, bool], Any] = {}
_PIPELINE_ERRORS: dict[tuple[bool, bool], Exception] = {}
MAX_PIPELINES = 2


def get_pipeline(
    use_doc_orientation_classify: bool = False,
    use_doc_unwarping: bool = False,
):
    """Lazily instantiate PaddleOCRVL for the given preprocessor combo.

    The model is large (~1-2 GB); loading happens once per combo and is reused.
    When MAX_PIPELINES is exceeded the oldest entry is evicted (FIFO).
    """
    key = (bool(use_doc_orientation_classify), bool(use_doc_unwarping))
    if key in _PIPELINE_CACHE:
        return _PIPELINE_CACHE[key]
    if key in _PIPELINE_ERRORS:
        raise RuntimeError(f"PaddleOCRVL {key} failed to load: {_PIPELINE_ERRORS[key]}")

    try:
        from paddleocr import PaddleOCRVL
        logger.info(
            "Loading PaddleOCRVL pipeline (v1.6, rotate=%s, dewarp=%s)…",
            key[0], key[1],
        )
        t0 = time.perf_counter()
        pipeline = PaddleOCRVL(
            pipeline_version="v1.6",
            use_doc_orientation_classify=key[0],
            use_doc_unwarping=key[1],
            device=os.environ.get("PADDLE_DEVICE", "cpu"),
        )
        logger.info("Pipeline loaded in %.2fs", time.perf_counter() - t0)
        _PIPELINE_CACHE[key] = pipeline
        # Evict oldest entry if over limit.
        while len(_PIPELINE_CACHE) > MAX_PIPELINES:
            oldest = next(iter(_PIPELINE_CACHE))
            if oldest != key:
                _PIPELINE_CACHE.pop(oldest, None)
                logger.info("Evicted pipeline %s from cache", oldest)
            else:
                break
        return pipeline
    except Exception as e:
        _PIPELINE_ERRORS[key] = e
        logger.exception("Failed to load PaddleOCRVL %s", key)
        raise


def any_pipeline_loaded() -> bool:
    return bool(_PIPELINE_CACHE)


def latest_load_error() -> str | None:
    if not _PIPELINE_ERRORS:
        return None
    k, v = next(iter(_PIPELINE_ERRORS.items()))
    return f"{k}: {v}"


# -----------------------------------------------------------------------
# API server
# -----------------------------------------------------------------------

app = FastAPI(title="PaddleOCR-VL Service", version="1.6.0")


# ---- Request schemas ---------------------------------------------------

class ImageRequest(BaseModel):
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


# ---- Helpers -----------------------------------------------------------

def _save_image_to_temp(image_base64: str | None, image_url: str | None) -> tuple[str, bool]:
    """Resolve the image to a local temp file path. Returns (path, is_temp).

    PaddleOCRVL.predict() needs a file path or URL — it doesn't accept raw
    bytes. We decode base64 → temp PNG; URLs are passed through.
    """
    if image_url:
        return image_url, False  # Let PaddleOCR fetch it

    if not image_base64:
        raise HTTPException(status_code=400, detail="Either image_base64 or image_url required")

    try:
        raw = base64.b64decode(image_base64)
    except Exception as e:
        raise HTTPException(status_code=400, detail=f"Invalid base64: {e}")

    suffix = ".png"
    if Image is not None:
        try:
            im = Image.open(io.BytesIO(raw))
            if im.format == "JPEG":
                suffix = ".jpg"
            elif im.format == "WEBP":
                suffix = ".webp"
        except Exception:
            pass  # Trust the bytes; PIL failed

    fd, path = tempfile.mkstemp(suffix=suffix)
    with os.fdopen(fd, "wb") as f:
        f.write(raw)
    return path, True


def _image_dimensions(image_base64: str | None, image_url: str | None) -> tuple[int, int]:
    """Best-effort width/height for bbox normalization."""
    if Image is None:
        return 0, 0
    try:
        if image_base64:
            raw = base64.b64decode(image_base64)
            im = Image.open(io.BytesIO(raw))
            return im.width, im.height
        if image_url:
            import urllib.request
            with urllib.request.urlopen(image_url, timeout=10) as r:
                im = Image.open(io.BytesIO(r.read()))
                return im.width, im.height
    except Exception:
        return 0, 0
    return 0, 0


def _run_predict(
    image_base64: str | None,
    image_url: str | None,
    use_doc_orientation_classify: bool = False,
    use_doc_unwarping: bool = False,
):
    """Run PaddleOCRVL.predict() and return the first result object.

    The flags select which cached pipeline to use. Each unique (rotate, dewarp)
    combo instantiates a separate pipeline on first use (~10-30s on CPU).
    """
    path, is_temp = _save_image_to_temp(image_base64, image_url)
    try:
        pipeline = get_pipeline(
            use_doc_orientation_classify=use_doc_orientation_classify,
            use_doc_unwarping=use_doc_unwarping,
        )
        outputs = list(pipeline.predict(path))
        if not outputs:
            return None
        return outputs[0]
    finally:
        if is_temp:
            try:
                os.unlink(path)
            except OSError:
                pass


def _normalize_bbox(bbox: list[float], w: int, h: int) -> dict:
    """Convert a pixel-coordinate polygon/bbox into normalized 0..1 dict."""
    if not bbox:
        return {"x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0}

    # bbox may be [x1, y1, x2, y2] or [[x, y], [x, y], ...]
    pts: list[tuple[float, float]] = []
    if isinstance(bbox[0], (list, tuple)):
        for pt in bbox:
            if len(pt) >= 2:
                pts.append((float(pt[0]), float(pt[1])))
    else:
        # Flat [x1, y1, x2, y2, ...] — group into pairs
        flat = [float(v) for v in bbox]
        for i in range(0, len(flat) - 1, 2):
            pts.append((flat[i], flat[i + 1]))

    if not pts or w <= 0 or h <= 0:
        return {"x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0}

    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    min_x, max_x = min(xs), max(xs)
    min_y, max_y = min(ys), max(ys)

    def clamp01(v: float) -> float:
        return max(0.0, min(1.0, v))

    return {
        "x": clamp01(min_x / w),
        "y": clamp01(min_y / h),
        "width": clamp01((max_x - min_x) / w),
        "height": clamp01((max_y - min_y) / h),
    }


def _get_markdown(res: Any) -> str:
    """Safely extract markdown text from a PaddleOCRVL result as a plain string.

    PaddleOCRVL's `res.markdown` may be a custom Markdown object (not a str),
    which FastAPI's jsonable_encoder cannot serialize. Always coerce to str.
    """
    md = getattr(res, "markdown", None)
    if md is None and isinstance(res, dict):
        md = res.get("markdown")
    if md is None:
        return ""
    # Some PaddleX versions wrap markdown in a custom type with .raw_text or .text
    if isinstance(md, str):
        return md
    for attr in ("markdown_texts", "raw_text", "text", "markdown_text"):
        v = getattr(md, attr, None)
        if isinstance(v, str):
            return v
    # PaddleX Markdown object: __str__ returns dict repr; dig into common dict shapes
    if isinstance(md, dict):
        for k in ("markdown_texts", "raw_text", "text", "markdown_text"):
            v = md.get(k)
            if isinstance(v, str):
                return v
    return str(md)


def _strip_markdown(md: str) -> str:
    """Strip simple markdown formatting for the full_text field."""
    import re
    md = re.sub(r"^#+\s*", "", md, flags=re.MULTILINE)  # headings
    md = re.sub(r"\*\*([^*]+)\*\*", r"\1", md)           # bold
    md = re.sub(r"\*([^*]+)\*", r"\1", md)                # italic
    md = re.sub(r"`([^`]+)`", r"\1", md)                  # inline code
    return md.strip()


def _extract_text_blocks(res: Any, w: int, h: int) -> tuple[list[dict], str]:
    """Pull text blocks + bbox from a PaddleOCRVL result.

    PaddleOCRVL's doc_parser result exposes `parsing_res_list` — one entry per
    detected layout element. Each entry has `layout_bbox` (pixel polygon) and
    `content` (Markdown text for that element).

    Returns (blocks, full_text).
    """
    blocks: list[dict] = []
    full_parts: list[str] = []

    parsing_list = getattr(res, "parsing_res_list", None)
    if parsing_list is None and isinstance(res, dict):
        parsing_list = res.get("parsing_res_list", [])
    if parsing_list is None:
        # Fallback: take the markdown output whole
        md = _get_markdown(res)
        return [], _strip_markdown(md)

    for entry in parsing_list:
        content = ""
        bbox_raw: list[float] = []
        if isinstance(entry, dict):
            content = entry.get("content", "") or entry.get("text", "") or ""
            bbox_raw = entry.get("layout_bbox") or entry.get("bbox") or []
        else:
            content = getattr(entry, "content", "") or getattr(entry, "text", "") or ""
            bbox_raw = getattr(entry, "layout_bbox", None) or getattr(entry, "bbox", []) or []

        content_str = str(content).strip()
        if not content_str:
            continue

        normalized = _normalize_bbox(bbox_raw, w, h)
        blocks.append({
            "text": content_str,
            "confidence": 1.0,  # PaddleOCRVL doesn't emit per-element confidence
            "bbox": normalized,
        })
        full_parts.append(content_str)

    return blocks, "\n".join(full_parts)


def _extract_table_html(res: Any) -> str:
    """Find the largest table-shaped element in the parsed result."""
    parsing_list = getattr(res, "parsing_res_list", None)
    if parsing_list is None and isinstance(res, dict):
        parsing_list = res.get("parsing_res_list", [])
    if parsing_list is None:
        return ""

    best_html = ""
    best_len = 0
    for entry in parsing_list:
        content = ""
        label = ""
        if isinstance(entry, dict):
            content = entry.get("content", "") or ""
            label = entry.get("layout_label", "") or entry.get("label", "") or ""
        else:
            content = getattr(entry, "content", "") or ""
            label = getattr(entry, "layout_label", "") or getattr(entry, "label", "") or ""

        if "table" in label.lower() or "<table" in content.lower():
            if len(content) > best_len:
                best_html = content
                best_len = len(content)

    return best_html


# ---- Endpoints ---------------------------------------------------------

@app.get("/health")
def health():
    """Liveness probe — doesn't load the model."""
    loaded = any_pipeline_loaded()
    err = latest_load_error()
    return {
        "status": "ok" if not err else "degraded",
        "version": "1.6",
        "model_loaded": loaded,
        "cached_pipelines": list(_PIPELINE_CACHE.keys()),
        "load_error": err,
    }


@app.post("/ocr")
def ocr(req: ImageRequest):
    """Text OCR — returns text_blocks with normalized bbox + full_text."""
    t0 = time.perf_counter()
    w, h = _image_dimensions(req.image_base64, req.image_url)

    try:
        res = _run_predict(
            req.image_base64,
            req.image_url,
            use_doc_orientation_classify=req.use_doc_orientation_classify,
            use_doc_unwarping=req.use_doc_unwarping,
        )
    except Exception as e:
        logger.exception("OCR inference failed")
        raise HTTPException(status_code=500, detail=str(e))

    if res is None:
        return {"results": [], "processing_time_ms": _ms(t0)}

    blocks, full_text = _extract_text_blocks(res, w, h)
    return {
        "results": [
            {
                "rec_text": b["text"],
                "rec_score": b["confidence"],
                "dt_polynomial": _denormalize_bbox(b["bbox"], w, h),
            }
            for b in blocks
        ],
        "full_text": full_text,
        "processing_time_ms": _ms(t0),
        "image_width": w,
        "image_height": h,
    }


@app.post("/markdown")
def markdown(req: ImageRequest):
    """Full document parsing — returns Markdown + layout elements."""
    t0 = time.perf_counter()
    w, h = _image_dimensions(req.image_base64, req.image_url)

    try:
        res = _run_predict(req.image_base64, req.image_url)
    except Exception as e:
        logger.exception("Markdown inference failed")
        raise HTTPException(status_code=500, detail=str(e))

    if res is None:
        return {"markdown": "", "processing_time_ms": _ms(t0)}

    md = _get_markdown(res)
    blocks, _ = _extract_text_blocks(res, w, h)
    return {
        "markdown": md,
        "text_blocks": blocks,
        "processing_time_ms": _ms(t0),
    }


@app.post("/table")
def table(req: TableRequest):
    """Table extraction — returns HTML of the largest table found."""
    t0 = time.perf_counter()
    try:
        res = _run_predict(req.image_base64, req.image_url)
    except Exception as e:
        logger.exception("Table inference failed")
        raise HTTPException(status_code=500, detail=str(e))

    html = _extract_table_html(res) if res is not None else ""
    return {
        "html": html,
        "processing_time_ms": _ms(t0),
    }


@app.post("/kie")
def kie(req: KieRequest):
    """Best-effort key information extraction.

    PaddleOCR-VL is a document parser, not a KIE model — so we parse the
    document to Markdown and return it as a single field. For real KIE, layer
    an LLM call on top of the parsed text.
    """
    t0 = time.perf_counter()
    try:
        res = _run_predict(req.image_base64, req.image_url)
    except Exception as e:
        logger.exception("KIE inference failed")
        raise HTTPException(status_code=500, detail=str(e))

    md = ""
    if res is not None:
        md = _get_markdown(res)

    fields = {"markdown": md}
    if req.schema and isinstance(req.schema, dict):
        wanted = req.schema.get("fields") or []
        for name in wanted:
            fields[name] = "<requires LLM post-processing>"

    return {
        "fields": fields,
        "processing_time_ms": _ms(t0),
    }


def _ms(t0: float) -> float:
    return round((time.perf_counter() - t0) * 1000.0, 2)


def _denormalize_bbox(norm: dict, w: int, h: int) -> list[list[float]]:
    """Convert a normalized bbox dict back into a pixel polygon for the wire
    format (the Rust extension expects pixel coords; it re-normalizes)."""
    if w <= 0 or h <= 0:
        return []
    x = norm.get("x", 0.0) * w
    y = norm.get("y", 0.0) * h
    width = norm.get("width", 0.0) * w
    height = norm.get("height", 0.0) * h
    return [
        [x, y],
        [x + width, y],
        [x + width, y + height],
        [x, y + height],
    ]


# -----------------------------------------------------------------------
# Entry point
# -----------------------------------------------------------------------

if __name__ == "__main__":
    import uvicorn

    host = os.environ.get("HOST", "0.0.0.0")
    port = int(os.environ.get("PORT", "8000"))

    print("=" * 60)
    print("  PaddleOCR-VL Inference Service")
    print(f"  Listening on: http://{host}:{port}")
    print("  Endpoints:")
    print("    GET  /health")
    print("    POST /ocr        — text OCR")
    print("    POST /markdown   — full document parse")
    print("    POST /table      — table extraction")
    print("    POST /kie        — best-effort KIE")
    print()
    print("  Ctrl+C to stop")
    print("=" * 60)

    uvicorn.run(app, host=host, port=port, log_level="info")
