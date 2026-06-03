"""
LocateAnything FastAPI Service
Wraps nvidia/LocateAnything-3B model as an HTTP API.
Runs on Mac (MPS) or GPU (CUDA).
"""
import os
import sys
import io
import re
import json
import base64
import time
import argparse
import asyncio
from concurrent.futures import ThreadPoolExecutor
from functools import partial
from pathlib import Path

# Add Embodied dir to path if available
eagle_embodied = os.environ.get("EAGLE_EMBODIED_PATH", "")
if eagle_embodied and os.path.isdir(eagle_embodied):
    sys.path.insert(0, eagle_embodied)

import types
import importlib.util

# Pre-register mock modules for optional deps that the model's trust_remote_code
# may import at top level (decord) but aren't needed for image inference.
for _mod_name in ("decord",):
    if _mod_name not in sys.modules:
        _mock = types.ModuleType(_mod_name)
        _mock.VideoReader = type("VideoReader", (), {})
        # Set __spec__ so import checks pass
        _spec = importlib.util.spec_from_loader(_mod_name, loader=None)
        _mock.__spec__ = _spec
        _mock.__path__ = []
        _mock.__file__ = None
        sys.modules[_mod_name] = _mock

import torch
from PIL import Image
from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field
from typing import Optional
from contextlib import asynccontextmanager

# ---------------------------------------------------------------------------
# Global worker (loaded once)
# ---------------------------------------------------------------------------
worker = None
executor = ThreadPoolExecutor(max_workers=2)


def get_device() -> str:
    if torch.cuda.is_available():
        return "cuda"
    elif hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def get_dtype(device: str) -> torch.dtype:
    if device == "mps":
        return torch.float16  # MPS doesn't fully support bfloat16
    return torch.bfloat16


@asynccontextmanager
async def lifespan(app: FastAPI):
    global worker
    model_path = os.environ.get("LOCATE_ANYTHING_MODEL", "nvidia/LocateAnything-3B")
    device = get_device()
    dtype = get_dtype(device)

    print(f"[LocateAnything] Loading model: {model_path}")
    print(f"[LocateAnything] Device: {device}, dtype: {dtype}")

    try:
        from locateanything_worker import LocateAnythingWorker
        worker = LocateAnythingWorker(model_path, device=device, dtype=dtype)
        print("[LocateAnything] Model loaded successfully")
    except Exception as e:
        print(f"[LocateAnything] Failed to load model: {e}")
        print(f"[LocateAnything] Service will start but inference will fail until model is available")

    yield
    print("[LocateAnything] Shutting down")


app = FastAPI(title="LocateAnything Service", lifespan=lifespan)
app.add_middleware(CORSMiddleware, allow_origins=["*"], allow_methods=["*"], allow_headers=["*"])


# ---------------------------------------------------------------------------
# Request / Response models
# ---------------------------------------------------------------------------
class ImageRequest(BaseModel):
    image_base64: Optional[str] = Field(None, description="Base64-encoded image (JPEG/PNG)")
    image_url: Optional[str] = Field(None, description="Image URL to fetch")
    image_path: Optional[str] = Field(None, description="Local file path")


class DetectRequest(ImageRequest):
    categories: list[str] = Field(..., description="Object categories to detect")
    generation_mode: str = Field("hybrid", description="fast|slow|hybrid")
    max_new_tokens: int = Field(2048)


class GroundRequest(ImageRequest):
    phrase: str = Field(..., description="Text description to ground")
    mode: str = Field("multi", description="single|multi")
    generation_mode: str = Field("hybrid")
    max_new_tokens: int = Field(2048)


class DetectTextRequest(ImageRequest):
    generation_mode: str = Field("hybrid")
    max_new_tokens: int = Field(2048)


class GuiGroundRequest(ImageRequest):
    phrase: str = Field(..., description="UI element description")
    output_type: str = Field("box", description="box|point")
    generation_mode: str = Field("hybrid")
    max_new_tokens: int = Field(2048)


class PointRequest(ImageRequest):
    phrase: str = Field(..., description="Object description to point to")
    generation_mode: str = Field("hybrid")
    max_new_tokens: int = Field(2048)


class InferenceResponse(BaseModel):
    success: bool
    answer: str = ""
    boxes: list[dict] = []
    points: list[dict] = []
    inference_time_ms: float = 0
    error: Optional[str] = None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
def load_image_from_request(req: ImageRequest) -> tuple[Image.Image, tuple[int, int]]:
    """Load and optionally resize image. Returns (resized_image, original_size)."""
    img = None
    if req.image_base64:
        data = base64.b64decode(req.image_base64)
        img = Image.open(io.BytesIO(data)).convert("RGB")
    elif req.image_url:
        import requests
        resp = requests.get(req.image_url, timeout=30)
        resp.raise_for_status()
        img = Image.open(io.BytesIO(resp.content)).convert("RGB")
    elif req.image_path:
        img = Image.open(req.image_path).convert("RGB")
    else:
        raise HTTPException(400, "Must provide image_base64, image_url, or image_path")

    # Save original dimensions before resizing
    original_size = img.size  # (width, height)

    # Downscale large images to speed up inference (max 512px on longest side)
    max_size = 512
    if max(img.size) > max_size:
        ratio = max_size / max(img.size)
        new_size = (int(img.width * ratio), int(img.height * ratio))
        img = img.resize(new_size, Image.LANCZOS)
    return img, original_size


def run_inference_sync(predict_fn) -> InferenceResponse:
    if worker is None:
        return InferenceResponse(success=False, error="Model not loaded")

    t0 = time.time()
    try:
        result = predict_fn()
        elapsed = (time.time() - t0) * 1000

        answer = result.get("answer", "")
        return InferenceResponse(
            success=True,
            answer=answer,
            inference_time_ms=round(elapsed, 1),
        )
    except Exception as e:
        elapsed = (time.time() - t0) * 1000
        return InferenceResponse(success=False, error=str(e), inference_time_ms=round(elapsed, 1))


async def run_inference(predict_fn) -> InferenceResponse:
    """Run inference in a thread pool to avoid blocking the event loop."""
    loop = asyncio.get_event_loop()
    return await loop.run_in_executor(executor, partial(run_inference_sync, predict_fn))


def parse_response(response: InferenceResponse, original_size: tuple[int, int]) -> InferenceResponse:
    """Parse boxes/points from the raw answer text using original image dimensions."""
    from locateanything_worker import LocateAnythingWorker
    w, h = original_size
    response.boxes = LocateAnythingWorker.parse_boxes(response.answer, w, h)
    response.points = LocateAnythingWorker.parse_points(response.answer, w, h)
    return response


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------
@app.get("/health")
async def health():
    return {
        "status": "ok" if worker is not None else "model_not_loaded",
        "model": os.environ.get("LOCATE_ANYTHING_MODEL", "nvidia/LocateAnything-3B"),
        "device": get_device(),
    }


@app.post("/detect", response_model=InferenceResponse)
async def detect(req: DetectRequest):
    img, orig_size = load_image_from_request(req)
    resp = await run_inference(lambda: worker.detect(img, req.categories, generation_mode=req.generation_mode, max_new_tokens=req.max_new_tokens))
    return parse_response(resp, orig_size)


@app.post("/ground", response_model=InferenceResponse)
async def ground(req: GroundRequest):
    img, orig_size = load_image_from_request(req)
    fn = worker.ground_multi if req.mode == "multi" else worker.ground_single
    resp = await run_inference(lambda: fn(img, req.phrase, generation_mode=req.generation_mode, max_new_tokens=req.max_new_tokens))
    return parse_response(resp, orig_size)


@app.post("/detect_text", response_model=InferenceResponse)
async def detect_text(req: DetectTextRequest):
    img, orig_size = load_image_from_request(req)
    resp = await run_inference(lambda: worker.detect_text(img, generation_mode=req.generation_mode, max_new_tokens=req.max_new_tokens))
    return parse_response(resp, orig_size)


@app.post("/ground_gui", response_model=InferenceResponse)
async def ground_gui(req: GuiGroundRequest):
    img, orig_size = load_image_from_request(req)
    resp = await run_inference(lambda: worker.ground_gui(img, req.phrase, output_type=req.output_type, generation_mode=req.generation_mode, max_new_tokens=req.max_new_tokens))
    return parse_response(resp, orig_size)


@app.post("/point", response_model=InferenceResponse)
async def point(req: PointRequest):
    img, orig_size = load_image_from_request(req)
    resp = await run_inference(lambda: worker.point(img, req.phrase, generation_mode=req.generation_mode, max_new_tokens=req.max_new_tokens))
    return parse_response(resp, orig_size)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="LocateAnything API Server")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=9380)
    parser.add_argument("--model", default=None, help="Model path or HuggingFace ID")
    args = parser.parse_args()

    if args.model:
        os.environ["LOCATE_ANYTHING_MODEL"] = args.model

    import uvicorn
    uvicorn.run(app, host=args.host, port=args.port)
