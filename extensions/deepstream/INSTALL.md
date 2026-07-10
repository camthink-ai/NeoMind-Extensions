# DeepStream 7.1 on CamThink Jetson — Installation Guide

> **Target audience:** Engineers deploying DeepStream 7.1 + this extension's sidecar from scratch on a CamThink NG4500 / Orin NX 8GB device.
>
> This document captures every pitfall hit on customer hardware. Follow the steps in order to get a working pipeline. **No sudo-to-root required — the ordinary `box` user suffices (except for the Docker daemon config).**

---

## 0. Prerequisites

| Item | Requirement |
|------|-------------|
| Device | CamThink NG4500-CB01 (Jetson Orin NX 8GB) |
| JetPack | R36.4.3 / JetPack 6.1 GA |
| L4T | 36.4.3 |
| Free disk | **≥ 20 GB** (image 5.5 GB + engine + cache) |
| Network | Access to `nvcr.io` (NGC) and PyPI |
| NGC account | Register at https://ngc.nvidia.com and generate an API key |

Verify:

```bash
cat /etc/nv_tegra_release         # should show R36.4.3
df -h /var/lib                    # free ≥ 20 GB
uname -r                          # 5.15.x-tegra
```

---

## 1. Docker Configuration (one-time)

### 1.1 Switch to `vfs` storage driver (**critical, otherwise disk explodes**)

The Orin NX kernel's overlayfs repeatedly fails when creating whiteout files inside the container, causing Docker daemon to retry indefinitely and bloat logs. You must use `vfs`:

```bash
sudo tee /etc/docker/daemon.json <<'EOF'
{
  "storage-driver": "vfs",
  "default-runtime": "nvidia",
  "runtimes": {
    "nvidia": {
      "path": "nvidia-container-runtime",
      "runtimeArgs": []
    }
  }
}
EOF
sudo systemctl restart docker
```

> **Cost:** vfs does no block-level deduplication; every layer is copied in full. Run `docker image prune -f` frequently.

### 1.2 Disable iptables (kernel has no `iptables_raw` module)

JetPack 6's tegra kernel doesn't compile `iptable_raw`, so Docker fails when creating the default bridge network:

```
iptables v1.8.7 (legacy): can't initialize iptables table `raw': Table does not exist
```

Fix: **add `--network=host` to every `docker run`**. This extension's sidecar, tests, and engine-build scripts all use host networking.

### 1.3 Log in to NGC

Generate an API key (base64 string) at https://org.ngc.nvidia.com/setup/api-key.

```bash
echo "$YOUR_NGC_KEY" | docker login nvcr.io -u '$oauthtoken' --password-stdin
```

> ⚠️ The username must literally be `$oauthtoken` (including the `$`). Use single quotes in the shell to prevent variable expansion.

---

## 2. Pull the DeepStream Image

```bash
docker pull nvcr.io/nvidia/deepstream:7.1-samples-multiarch
docker tag nvcr.io/nvidia/deepstream:7.1-samples-multiarch ds:7.1-base
```

> Image is ~5.5 GB. Under the vfs storage driver, the extracted size approaches 11 GB.
>
> Do NOT use `:7.1` (data-center variant) — it lacks Jetson libraries. **You must use the `multiarch` / `samples` tag.**

---

## 3. Build the Sidecar Image (layer pyds + GI on top of base)

### 3.1 Prepare the pyds Wheel

DeepStream 7.1 corresponds to `pyds-1.2.0`. Download from NGC or the NVIDIA GitHub release:

```bash
# From the host
wget https://github.com/NVIDIA-AI-IOT/deepstream_python_apps/releases/download/v1.2.0/pyds-1.2.0-cp310-cp310-linux_aarch64.whl
```

> ⚠️ **Gotcha:** the NGC direct-download `.whl` filename is `pyds-1.2.0-cp310-linux_aarch64.whl` (missing the ABI tag), and pip will refuse to install it (PEP 427). **Manually add the cp310 ABI tag:**
> ```bash
> cp pyds-1.2.0-cp310-linux_aarch64.whl pyds-1.2.0-cp310-cp310-linux_aarch64.whl
> ```

### 3.2 Dockerfile

```dockerfile
FROM nvcr.io/nvidia/deepstream:7.1-samples-multiarch

RUN apt-get update && apt-get install -y --no-install-recommends \
        python3-gi python3-gst-1.0 libpython3.10 python3-pip \
    && rm -rf /var/lib/apt/lists/*

COPY pyds-1.2.0-cp310-cp310-linux_aarch64.whl /tmp/
RUN pip3 install --no-cache-dir /tmp/pyds-1.2.0-cp310-cp310-linux_aarch64.whl \
    && rm /tmp/*.whl

WORKDIR /srv/sidecar
```

Build:

```bash
docker build --network=host -t ds:7.1-pyds-gi -f Dockerfile.sidecar .
```

Verify:

```bash
docker run --rm --runtime=nvidia --network=host ds:7.1-pyds-gi \
    python3 -c "import pyds; print('pyds', pyds.__version__)"
```

---

## 4. Pre-build the TensorRT Engine (avoid runtime OOM)

Jetson Orin NX 8GB memory is tight. Letting `nvinfer` build an INT8 engine at pipeline startup fails during tactic selection due to insufficient VRAM:

```
Tactic Device request: 538MB Available: 196MB
build engine file failed
```

> Lesson: **always pre-build the FP16 engine with `trtexec`**; at runtime `nvinfer` just deserializes it.

### 4.1 build-engine.sh

```bash
#!/bin/bash
# build-engine.sh — pre-build the TrafficCam FP16 engine
set -e

mkdir -p ~/ds-engines

docker run --rm --runtime=nvidia --network=host \
    -v ~/ds-engines:/engines \
    ds:7.1-pyds-gi \
    trtexec \
        --onnx=/opt/nvidia/deepstream/deepstream/samples/models/Primary_Detector/resnet18_trafficcamnet_pruned.onnx \
        --saveEngine=/engines/trafficcam_fp16.engine \
        --fp16 \
        --memPoolSize=workspace:1024
```

> ⚠️ TensorRT 10.3+: `--workspace` is deprecated; use `--memPoolSize=workspace:1024`.

Run once: ~3 seconds, produces a 4 MB `.engine` file.

### 4.2 Important: engine batch-size MUST match pipeline batch-size

```
gstnvtracker: Loading low-level lib at (null)
NvDsInferContext[UID 1]: deserialize engine ... maxBatchSize 1 whereas 30 has been requested
```

This means `nvinfer` sees an engine with `batch=1` (trtexec default), but `nvstreammux` provides 30. This triggers an **automatic rebuild**, which falls back into the INT8-OOM trap.

**Fix:** the sidecar forces `batch-size=1` when generating the nvinfer config. This extension's `pipeline_builder.py` already does this — if you customize other configs, remember to keep them in sync.

---

## 5. Start an RTSP Test Source

The sidecar needs RTSP input. The easiest path is mediamtx + ffmpeg looping a single stream:

```bash
# mediamtx (https://github.com/bluenviron/mediamtx/releases)
tar xf mediamtx_v*.linux_arm64v8.tar.gz
./mediamtx &

# 4 looped sample streams
for i in 1 2 3 4; do
    ffmpeg -re -stream_loop -1 -i sample.mp4 -c copy \
        -f rtsp rtsp://localhost:8554/in/stream$i &
done
```

Verify:

```bash
docker run --rm --network=host ds:7.1-pyds-gi \
    gst-launch-1.0 rtspsrc location=rtsp://localhost:8554/in/stream1 \
        latency=200 protocols=tcp ! fakesink sync=false
# "Pipeline is PREROLLED" means OK
```

> ⚠️ **You must use TCP (`protocols=tcp`):** the JetPack kernel's `multiudpsink` fails to resolve `localhost` over IPv6 with `Invalid address family (got 10)`, so UDP RTSP does not work at all.

---

## 6. Start the Sidecar

### 6.1 Copy the Sidecar Source

```bash
# From the NeoMind-Extensions repo, copy extensions/deepstream/sidecar/ to the Jetson
scp -r extensions/deepstream/sidecar box@<jetson>:~/ds-deps/
```

> In production, the `.nep` package installs the sidecar automatically under `~/.neomind/extensions/deepstream/sidecar/` and the extension finds it via `NEOMIND_EXTENSION_DIR`. The manual `scp` here is only for standalone sidecar debugging.

### 6.2 Hello + add_stream Test Input

```bash
cat > data-plane.jsonl <<'EOF'
{"id":"0","type":"hello","version":"1.0","capabilities":["streams","events"],"pid":12345,"rtsp_port":8554,"snapshot_port":8555,"log_level":"info","models_dir":"/opt/nvidia/deepstream/deepstream/samples/models","max_streams":4,"snapshot_bind_addr":"0.0.0.0"}
{"id":"1","type":"add_stream","config":{"stream_id":"test-1","source":{"type":"rtsp","url":"rtsp://localhost:8554/in/stream1","rtsp_transport":"tcp","latency_ms":200},"model":"Primary_Detector"}}
EOF
```

> ⚠️ `models_dir` **must not end with `/`**! The sidecar uses `os.path.dirname()` to derive the base path; a trailing slash only strips that slash instead of going up one level, breaking relative-path resolution.

### 6.3 Run

```bash
docker run --rm -i --runtime=nvidia --network=host \
    -v ~/ds-deps/sidecar:/srv/sidecar:ro \
    -v ~/ds-deps/data-plane.jsonl:/srv/data-plane.jsonl:ro \
    -v ~/ds-engines:/engines:ro \
    ds:7.1-pyds-gi \
    bash -c "(cat /srv/data-plane.jsonl; sleep 60; echo '{\"id\":\"2\",\"type\":\"shutdown\",\"graceful_secs\":3}') | timeout 90 python3 /srv/sidecar/deepstream_runner.py"
```

Expected output:

```jsonl
{"type":"hello_ack","max_streams":4,"rtsp_url_prefix":"rtsp://0.0.0.0:8554/ds/",...}
{"type":"stream_added","id":"1","stream_id":"test-1","rtsp_url":"rtsp://0.0.0.0:8554/ds/test-1"}
{"type":"Detection","stream_id":"test-1",...}
{"type":"Detection",...}
```

---

## 7. Known Pitfalls Summary (in order encountered)

| # | Symptom | Root Cause | Fix |
|---|---------|------------|-----|
| 1 | `docker pull` 401 | NGC username must be `$oauthtoken` (literal), not `$NGC_API_KEY` | Use single quotes around the username |
| 2 | `iptables table 'raw' does not exist` | JetPack kernel doesn't compile `iptable_raw` | Add `--network=host` to every docker run |
| 3 | `/var/lib/docker` balloons to 100% | overlayfs retries infinitely on whiteout files | Change `daemon.json` to `"storage-driver": "vfs"` |
| 4 | `pyds wheel ... is not a valid wheel filename` | NGC-provided wheel filename missing ABI tag | Add `cp310` when copying: `pyds-1.2.0-cp310-cp310-...` |
| 5 | `gir1.0-gst-rtsp-server-1.0` apt not found | Ubuntu ports repo has no such package | Remove it; the sidecar uses an external mediamtx |
| 6 | `cannot build Hello: missing 6 arguments` | Hello protocol upgraded with new fields | Fill in `rtsp_port`/`snapshot_port`/`log_level`/`models_dir`/`max_streams`/`snapshot_bind_addr` |
| 7 | nvinfer INT8 build fails `Tactic ... Available: 196MB` | Orin NX 8GB insufficient for tactic selection | Pre-build with `trtexec --fp16` |
| 8 | `--workspace` not recognized | TRT 10.3+ deprecated it | Use `--memPoolSize=workspace:1024` |
| 9 | `Backend maxBatchSize 1 whereas 30 has been requested` | trtexec default batch=1, streammux batch=30 | Sidecar overrides `batch-size=1` |
| 10 | `gstnvtracker: Loading low-level lib at (null)` | `_apply_tracker_props` early-returns when tracker_cfg=None, never sets `ll-lib-file` | Force-set the NvDCF default lib even when tracker is disabled |
| 11 | `libnvds_nvdcdcf_tracker.so: cannot open shared object file` | DS 7.1 ships only the combined `libnvds_nvmultiobjecttracker.so` | `_ll_lib_for` prefers the combined lib |
| 12 | `Could not open labels file:/tmp/ds_model_X/../../models/...` | Generated config copied to /tmp breaks relative paths | Convert every path key (including `-path` suffix variants like `labelfile-path`) to absolute when writing config |
| 13 | `gstnvdsanalytics: Configuration file not provided` | `_apply_analytics_props` early-returns when cfg=None | Write a minimal `[property]\nenable=1\n...` to /tmp then `set_property("config-file", path)` |
| 14 | `Invalid address family (got 10)` — UDP RTSP won't start | multiudpsink IPv6 fails to resolve localhost | uridecodebin hooks into `source-setup` signal and forces `protocols=tcp(4)` on the inner rtspsrc |
| 15 | `Could not find output layer 'conv2d_bbox' in engine` | TRT 10.x no longer uses ONNX output tensor names | Do **not** set `output-blob-names` in the config; let nvinfer infer |
| 16 | `object of type GstNvTracker does not have property enable-batch-process` | DS 7.1 GstNvTracker removed this property | Sidecar patch drops this line |

---

## 8. Disk Cleanup

The vfs driver produces redundant layers on every build. Clean periodically:

```bash
docker container prune -f
docker image prune -f
docker builder prune -f
# Emergency:
# sudo rm -rf /var/lib/docker && sudo systemctl restart docker
#  ↑ deletes all images/containers/volumes — use with caution
```

---

## 9. Next Step: Run YOLOv8n

The default TrafficCam model is for entry-level validation. For production with YOLOv8n:

1. **Export ONNX** (on a PC):
   ```bash
   yolo export model=yolov8n.pt format=onnx opset=12 simplify
   ```
2. **Copy to Jetson:** `/srv/models/yolov8n.onnx`
3. **Pre-build FP16 engine:**
   ```bash
   trtexec --onnx=/srv/models/yolov8n.onnx --saveEngine=/engines/yolov8n_fp16.engine \
           --fp16 --memPoolSize=workspace:1024
   ```
4. **Write the nvinfer config:** refer to the `yolov8` sample `config.txt` in NVIDIA's `deepstream-python-apps` repo, paired with the custom parser `libnvds_infercustomparser_yolov8.so`.
5. **`register_model` on the sidecar:** fill in `model-engine-file`, `labelfile-path`, `parse-bbox-func-name`, `custom-lib-path`.

---

## 10. References

- NVIDIA DeepStream 7.1 Release Notes: https://docs.nvidia.com/metropolis/deepstream/dev-guide/
- pyds Python bindings: https://github.com/NVIDIA-AI-IOT/deepstream_python_apps
- CamThink device software guide: https://wiki.camthink.ai/docs/neoedge-ng4500-series/ng4500-cb01-development-board/software-guide/software-frameworks-and-tools/deepstream
- NeoMind extension development: see `CLAUDE.md` and `EXTENSION_GUIDE.md` in this repo

---

**Last updated:** 2026-07-08
