# DeepStream 7.1 on CamThink Jetson — 安装指南

> 目标读者：在 CamThink NG4500 / Orin NX 8GB 设备上从零部署 DeepStream 7.1 + 本扩展 sidecar 的工程师。
>
> 本文档总结了在客户设备上踩过的所有坑，按步骤复刻即可成功跑通。**全程不需要 sudo 进 root，普通 `box` 用户即可（除 docker daemon 配置外）。**

---

## 0. 环境前置

| 项 | 要求 |
|----|------|
| 设备 | CamThink NG4500-CB01（Jetson Orin NX 8GB） |
| JetPack | R36.4.3 / JetPack 6.1 GA |
| L4T | 36.4.3 |
| 磁盘可用空间 | **≥ 20 GB**（镜像 5.5 GB + engine + 缓存） |
| 网络 | 能访问 `nvcr.io`（NGC） 和 PyPI |
| NGC 账号 | 注册 https://ngc.nvidia.com  拿一把 API key |

校验：

```bash
cat /etc/nv_tegra_release         # 应该显示 R36.4.3
df -h /var/lib                    # 剩余 ≥ 20 GB
uname -r                          # 5.15.x-tegra
```

---

## 1. Docker 配置（一次性）

### 1.1 换 vfs 存储驱动（**关键，否则空间爆掉**）

Orin NX 内核的 overlayfs 在容器内创建白屏文件时反复失败，Docker daemon 会无限重试并膨胀日志。必须改用 `vfs`：

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

> **代价**：vfs 不做块级去重，每个 layer 完整复制。`docker image prune -f` 要勤跑。

### 1.2 关闭 iptables（内核无 iptables_raw 模块）

JetPack 6 的 tegra 内核没编 `iptable_raw`，Docker 默认创建 bridge 网络时会失败：

```
iptables v1.8.7 (legacy): can't initialize iptables table `raw': Table does not exist
```

解决：**所有 `docker run` 加 `--network=host`**。本扩展的 sidecar、test、engine build 脚本都用 host 网络。

### 1.3 登录 NGC

去 https://org.ngc.nvidia.com/setup/api-key  生成一把 API key（base64 串）。

```bash
echo "$YOUR_NGC_KEY" | docker login nvcr.io -u '$oauthtoken' --password-stdin
```

> ⚠️ 用户名必须字面写 `$oauthtoken`（含 `$`）。在 shell 里用单引号包住防止变量展开。

---

## 2. 拉取 DeepStream 镜像

```bash
docker pull nvcr.io/nvidia/deepstream:7.1-samples-multiarch
docker tag nvcr.io/nvidia/deepstream:7.1-samples-multiarch ds:7.1-base
```

> 镜像 ~5.5 GB。vfs 存储驱动下，解压后实际占用接近 11 GB。
>
> 不要用 `:7.1`（数据中心版），它不带 Jetson 相关库。**必须 multiarch / samples 标签。**

---

## 3. 构建 sidecar 镜像（在 base 上叠 pyds + GI）

### 3.1 准备 pyds wheel

DeepStream 7.1 对应 `pyds-1.2.0`。从 NGC 或 NVIDIA GitHub release 下载：

```bash
# 从 host 端
wget https://github.com/NVIDIA-AI-IOT/deepstream_python_apps/releases/download/v1.2.0/pyds-1.2.0-cp310-cp310-linux_aarch64.whl
```

> ⚠️ **坑**：NGC 直链下载 `.whl` 文件名是 `pyds-1.2.0-cp310-linux_aarch64.whl`（缺 ABI tag），pip 会拒绝安装（PEP 427）。**手动补一个 cp310**：
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

构建：

```bash
docker build --network=host -t ds:7.1-pyds-gi -f Dockerfile.sidecar .
```

校验：

```bash
docker run --rm --runtime=nvidia --network=host ds:7.1-pyds-gi \
    python3 -c "import pyds; print('pyds', pyds.__version__)"
```

---

## 4. 预构建 TensorRT 引擎（避免运行时爆炸）

Jetson Orin NX 8GB 内存吃紧，让 nvinfer 在 pipeline 启动时**直接用 INT8** 构建 engine 会因 tactic 选择阶段显存不够失败：

```
Tactic Device request: 538MB Available: 196MB
build engine file failed
```

> 教训：**永远用 `trtexec` 提前把 FP16 engine 构建好**，运行时 nvinfer 直接 deserialize。

### 4.1 build-engine.sh

```bash
#!/bin/bash
# build-engine.sh — 预构建 TrafficCam FP16 engine
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

> ⚠️ TensorRT 10.3+：`--workspace` 已废弃，必须用 `--memPoolSize=workspace:1024`。

跑一次：3 秒，输出 4 MB 的 `.engine` 文件。

### 4.2 重要：engine batch-size 必须 == pipeline batch-size

```bash
gstnvtracker: Loading low-level lib at (null)
NvDsInferContext[UID 1]: deserialize engine ... maxBatchSize 1 whereas 30 has been requested
```

意思是 nvinfer 看到的 engine 是 `batch=1`（trtexec 默认），但 nvstreammux 给的是 30。会触发**自动重建**，又掉进 INT8 失败的坑。

**修复**：sidecar 在生成 nvinfer 配置时强制 `batch-size=1`。本扩展的 `pipeline_builder.py` 已经这样做，自己改其它配置要记得同步。

---

## 5. 启动 RTSP 测试源

sidecar 需要 RTSP 输入。最方便用 mediamtx + ffmpeg 循环推一路：

```bash
# mediamtx (https://github.com/bluenviron/mediamtx/releases)
tar xf mediamtx_v*.linux_arm64v8.tar.gz
./mediamtx &

# 4 路循环样本
for i in 1 2 3 4; do
    ffmpeg -re -stream_loop -1 -i sample.mp4 -c copy \
        -f rtsp rtsp://localhost:8554/in/stream$i &
done
```

校验：

```bash
docker run --rm --network=host ds:7.1-pyds-gi \
    gst-launch-1.0 rtspsrc location=rtsp://localhost:8554/in/stream1 \
        latency=200 protocols=tcp ! fakesink sync=false
# 看到 "Pipeline is PREROLLED" 就 OK
```

> ⚠️ **必须用 TCP（`protocols=tcp`）**：JetPack 内核的 `multiudpsink` 在 IPv6 解析 `localhost` 时报 `Invalid address family (got 10)`，UDP RTSP 完全跑不通。

---

## 6. 启动 sidecar

### 6.1 拷贝 sidecar 源码

```bash
# 从 NeoMind-Extensions 仓库的 extensions/deepstream/sidecar/ 整目录拷到 Jetson
scp -r extensions/deepstream/sidecar box@<jetson>:~/ds-deps/
```

### 6.2 Hello + add_stream 测试输入

```bash
cat > data-plane.jsonl <<'EOF'
{"id":"0","type":"hello","version":"1.0","capabilities":["streams","events"],"pid":12345,"rtsp_port":8554,"snapshot_port":8555,"log_level":"info","models_dir":"/opt/nvidia/deepstream/deepstream/samples/models","max_streams":4,"snapshot_bind_addr":"0.0.0.0"}
{"id":"1","type":"add_stream","config":{"stream_id":"test-1","source":{"type":"rtsp","url":"rtsp://localhost:8554/in/stream1","rtsp_transport":"tcp","latency_ms":200},"model":"Primary_Detector"}}
EOF
```

> ⚠️ `models_dir` **末尾不能有 `/`**！sidecar 用 `os.path.dirname()` 反推 base 路径，带斜杠只会 strip 掉那个斜杠而不是上跳一级，导致相对路径解析错误。

### 6.3 跑

```bash
docker run --rm -i --runtime=nvidia --network=host \
    -v ~/ds-deps/sidecar:/srv/sidecar:ro \
    -v ~/ds-deps/data-plane.jsonl:/srv/data-plane.jsonl:ro \
    -v ~/ds-engines:/engines:ro \
    ds:7.1-pyds-gi \
    bash -c "(cat /srv/data-plane.jsonl; sleep 60; echo '{\"id\":\"2\",\"type\":\"shutdown\",\"graceful_secs\":3}') | timeout 90 python3 /srv/sidecar/deepstream_runner.py"
```

期望看到：

```jsonl
{"type":"hello_ack","max_streams":4,"rtsp_url_prefix":"rtsp://0.0.0.0:8554/ds/",...}
{"type":"stream_added","id":"1","stream_id":"test-1","rtsp_url":"rtsp://0.0.0.0:8554/ds/test-1"}
{"type":"Detection","stream_id":"test-1",...}
{"type":"Detection",...}
```

---

## 7. 已知坑汇总（按踩坑顺序）

| # | 现象 | 根因 | 解决 |
|---|------|------|------|
| 1 | `docker pull` 401 | NGC 用户名要写 `$oauthtoken`（字面），不是 `$NGC_API_KEY` | 用单引号包用户名 |
| 2 | `iptables table 'raw' does not exist` | JetPack 内核没编 `iptable_raw` 模块 | 所有 docker run 加 `--network=host` |
| 3 | `/var/lib/docker` 暴涨到 100% | overlayfs 在白屏文件上无限重试 | `daemon.json` 改 `"storage-driver": "vfs"` |
| 4 | `pyds wheel ... is not a valid wheel filename` | NGC 提供的 wheel 文件名缺 ABI tag | 复制时补一个 `cp310`：`pyds-1.2.0-cp310-cp310-...` |
| 5 | `gir1.0-gst-rtsp-server-1.0` apt 找不到 | ubuntu ports 仓库无此包 | 删掉，sidecar 用外部 mediamtx |
| 6 | `cannot build Hello: missing 6 arguments` | hello 协议升级加了字段 | 补齐 `rtsp_port`/`snapshot_port`/`log_level`/`models_dir`/`max_streams`/`snapshot_bind_addr` |
| 7 | nvinfer INT8 build 失败 `Tactic ... Available: 196MB` | Orin NX 8GB 内存不够 tactic 选择 | 用 `trtexec --fp16` 预构建 |
| 8 | `--workspace` 不识别 | TRT 10.3+ 已废弃 | 换 `--memPoolSize=workspace:1024` |
| 9 | `Backend maxBatchSize 1 whereas 30 has been requested` | trtexec 默认 batch=1，streammux batch=30 | sidecar override `batch-size=1` |
| 10 | `gstnvtracker: Loading low-level lib at (null)` | `_apply_tracker_props` 在 tracker_cfg=None 时早退，没设 `ll-lib-file` | 即使 tracker 禁用也强制设 NvDCF 默认 lib |
| 11 | `libnvds_nvdcdcf_tracker.so: cannot open shared object file` | DS 7.1 只剩 `libnvds_nvmultiobjecttracker.so` 一个组合 lib | `_ll_lib_for` 优先返回组合 lib |
| 12 | `Could not open labels file:/tmp/ds_model_X/../../models/...` | 生成 config 拷贝到 /tmp 后相对路径失效 | 写 config 时把所有路径 key（含 `-path` 后缀变体如 `labelfile-path`）转绝对 |
| 13 | `gstnvdsanalytics: Configuration file not provided` | `_apply_analytics_props` 在 cfg=None 时早退 | 写最小 `[property]\nenable=1\n...` 到 /tmp 再 `set_property("config-file", path)` |
| 14 | `Invalid address family (got 10)` UDP RTSP 起不来 | multiudpsink IPv6 解析 localhost 失败 | uridecodebin 接 `source-setup` 信号，对内部 rtspsrc 强制 `protocols=tcp(4)` |
| 15 | `Could not find output layer 'conv2d_bbox' in engine` | TRT 10.x 不再用 ONNX 的输出 tensor 名 | 配置文件**不要写 `output-blob-names`**，让 nvinfer 自己推断 |
| 16 | `object of type GstNvTracker does not have property enable-batch-process` | DS 7.1 GstNvTracker 移除了该属性 | sidecar patch 中删掉这一行 |

---

## 8. 清理空间

vfs 驱动每次构建都会产生冗余 layer，定期清：

```bash
docker container prune -f
docker image prune -f
docker builder prune -f
# 紧急情况下：
# sudo rm -rf /var/lib/docker && sudo systemctl restart docker
#  ↑ 会删所有镜像/容器/卷，慎用
```

---

## 9. 下一步：跑通 YOLOv8n

默认 TrafficCam 模型是入门验证。生产用 YOLOv8n 需要：

1. **导出 ONNX**（在 PC 上）：
   ```bash
   yolo export model=yolov8n.pt format=onnx opset=12 simplify
   ```
2. **拷到 Jetson**：`/srv/models/yolov8n.onnx`
3. **预构建 FP16 engine**：
   ```bash
   trtexec --onnx=/srv/models/yolov8n.onnx --saveEngine=/engines/yolov8n_fp16.engine \
           --fp16 --memPoolSize=workspace:1024
   ```
4. **写 nvinfer 配置**：参考 NVIDIA `deepstream-python-apps` 仓库的 `yolov8` 示例 `config.txt`，配合自定义 parser `libnvds_infercustomparser_yolov8.so`。
5. **sidecar 端 `register_model`**：把 `model-engine-file`、`labelfile-path`、`parse-bbox-func-name`、`custom-lib-path` 都填好。

---

## 10. 参考

- NVIDIA DeepStream 7.1 Release Notes: https://docs.nvidia.com/metropolis/deepstream/dev-guide/
- pyds Python bindings: https://github.com/NVIDIA-AI-IOT/deepstream_python_apps
- CamThink 设备软件指南: https://wiki.camthink.ai/docs/neoedge-ng4500-series/ng4500-cb01-development-board/software-guide/software-frameworks-and-tools/deepstream
- NeoMind 扩展开发：本仓库 `CLAUDE.md` 与 `EXTENSION_GUIDE.md`

---

**最后更新**：2026-07-07
