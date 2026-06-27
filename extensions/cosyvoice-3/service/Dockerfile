# CosyVoice 3 HTTP service container (Linux / CUDA).
#
# On macOS development, run service/start.sh directly with PyTorch + MPS —
# this Dockerfile is for Linux server / Jetson Orin (CUDA) deployments.
#
# Build:
#   docker build -t neomind/cosyvoice-3-service extensions/cosyvoice-3/service
#
# Run (GPU):
#   docker run --rm --gpus all -p 9385:9385 \
#     -v "$HOME/.cache/modelscope:/root/.cache/modelscope" \
#     neomind/cosyvoice-3-service
#
# Run (CPU):
#   docker run --rm -p 9385:9385 \
#     -v "$HOME/.cache/modelscope:/root/.cache/modelscope" \
#     neomind/cosyvoice-3-service
#
# First run downloads ~2GB model into the mounted modelscope cache volume.

FROM nvidia/cuda:12.4.1-cudnn-devel-ubuntu22.04

ENV DEBIAN_FRONTEND=noninteractive \
    PYTHONUNBUFFERED=1 \
    PYTORCH_ENABLE_MPS_FALLBACK=1

RUN apt-get update && apt-get install -y --no-install-recommends \
        python3.11 python3-pip python3.11-venv \
        git build-essential ffmpeg \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/bin/python3.11 /usr/bin/python

WORKDIR /opt/service

COPY requirements.txt server.py /opt/service/
RUN pip install --no-cache-dir -r /opt/service/requirements.txt

ENV COSYVOICE_MODEL_DIR=FunAudioLLM/Fun-CosyVoice3-0.5B-2512

EXPOSE 9385
CMD ["python", "/opt/service/server.py", "--host", "0.0.0.0", "--port", "9385"]
