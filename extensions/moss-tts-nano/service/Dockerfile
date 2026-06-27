# MOSS-TTS-Nano HTTP service container.
#
# Build:
#   docker build -t neomind/moss-tts-nano-service extensions/moss-tts-nano/service
#
# Run:
#   docker run --rm -p 9382:9382 \
#     -v "$HOME/MOSS-TTS-Nano/models:/models" \
#     -e MOSS_TTS_MODEL_DIR=/models \
#     neomind/moss-tts-nano-service
#
# First run downloads ~200 MB of ONNX weights into the mounted /models volume.

FROM python:3.12-slim AS base

RUN apt-get update && apt-get install -y --no-install-recommends \
        git build-essential ffmpeg \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt

# Clone + install MOSS-TTS-Nano (editable so server.py can import its modules).
ARG MOSS_TTS_NANO_REF=master
RUN git clone --depth 1 --branch "${MOSS_TTS_NANO_REF}" \
        https://github.com/OpenMOSS/MOSS-TTS-Nano.git /opt/MOSS-TTS-Nano

WORKDIR /opt/MOSS-TTS-Nano
# pynini/WeTextProcessing need conda on most platforms. We try the
# community wheel route here; on failure, see repo README for conda setup.
RUN pip install --no-cache-dir \
        "pynini==2.1.6.post1; platform_system == 'Linux'" \
    || echo "[warn] pynini wheel install failed, continuing without it"
RUN pip install --no-cache-dir -r requirements.txt \
    && pip install --no-cache-dir -e .

# Copy our service in.
WORKDIR /opt/service
COPY requirements.txt server.py /opt/service/
RUN pip install --no-cache-dir -r /opt/service/requirements.txt

ENV MOSS_TTS_NANO_REPO=/opt/MOSS-TTS-Nano \
    MOSS_TTS_MODEL_DIR=/opt/MOSS-TTS-Nano/models \
    MOSS_TTS_CPU_THREADS=4

EXPOSE 9382
CMD ["python", "/opt/service/server.py", "--host", "0.0.0.0", "--port", "9382"]
