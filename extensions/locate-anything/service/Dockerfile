# LocateAnything Inference Service - GPU (CUDA)
# Supports: NVIDIA T4, V100, A100, etc.
#
# Build:  docker build -t locate-anything-service .
# Run:    docker run -d --gpus all -p 9380:9380 -v locate-models:/models locate-anything-service

FROM pytorch/pytorch:2.6.0-cuda12.4-cudnn9-devel

WORKDIR /app

# System deps for building Python packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    git libgl1-mesa-glx libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*

# Python dependencies (CUDA-specific)
COPY requirements-docker.txt requirements.txt
RUN pip install --no-cache-dir -r requirements.txt

# Copy service code
COPY server.py .
COPY locateanything_worker.py .

# Model cache directory (persist via volume)
ENV HF_HOME=/models/huggingface
RUN mkdir -p /models/huggingface

# Pre-download model at build time (optional, comment out for faster builds)
# Model is ~6GB; if skipped, it downloads on first startup
ARG DOWNLOAD_MODEL=0
RUN if [ "$DOWNLOAD_MODEL" = "1" ]; then \
        python -c "from transformers import AutoModel, AutoTokenizer, AutoProcessor; \
        AutoTokenizer.from_pretrained('nvidia/LocateAnything-3B', trust_remote_code=True); \
        AutoProcessor.from_pretrained('nvidia/LocateAnything-3B', trust_remote_code=True); \
        AutoModel.from_pretrained('nvidia/LocateAnything-3B', trust_remote_code=True, torch_dtype='auto')" \
    ; fi

EXPOSE 9380

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=300s --retries=3 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:9380/health')" || exit 1

# Start the service
# Use 0.0.0.0 to accept external connections (not just localhost)
ENV LOCATE_ANYTHING_MODEL=nvidia/LocateAnything-3B
CMD ["python", "server.py", "--host", "0.0.0.0", "--port", "9380"]
