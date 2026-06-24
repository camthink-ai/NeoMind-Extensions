# voice-edge-tts — Linux ARM64 / AMD64 deploy image.
#
# Build (from repo root):
#   docker build -t voice-edge-tts:latest -f extensions/voice-edge-tts/service/Dockerfile .
# Run:
#   docker run -p 9386:9386 voice-edge-tts:latest
#
# First container start downloads ~150MB ZipVoice model into ~/.cache/sherpa-onnx.
# For faster cold starts, uncomment the pre-download RUN below.
FROM python:3.11-slim

WORKDIR /app

# Audio libs needed by soundfile + ca-certificates for model download.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libsndfile1 \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*

# Python deps first (better layer caching).
COPY extensions/voice-edge-tts/service/requirements.txt /app/service/requirements.txt
RUN pip install --no-cache-dir -r /app/service/requirements.txt

# Service code + bundled assets.
COPY extensions/voice-edge-tts/service/ /app/service/

# Pre-download the model into the image so first start is fast.
# Uncomment for production builds (adds ~150MB to image).
# RUN cd /app/service && python -c "import server; server._model_dir(); print('model cached')"

EXPOSE 9386

WORKDIR /app/service
CMD ["python", "server.py", "--host", "0.0.0.0", "--port", "9386"]
