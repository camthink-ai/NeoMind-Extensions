# Vision Hub

NeoMind 的统一视觉扩展 —— 一个扩展覆盖全部视觉任务,一条任务 schema,一份 pipeline 配置,零 usls 依赖(直接对接 ONNX Runtime)。

**当前批次(detect)**:目标检测端到端可用 —— `analyze` 一次性分析、设备帧 pipeline 持续分析、虚拟指标/事件/抓拍出口、硬件加速自动选择。后续批次按同一 schema 增加 OCR / 人脸 / 定位 / VLM,不改配置格式。

## 为什么需要它

此前的视觉能力分散在 7 个扩展里(yolo-device-inference、ocr-device-inference、face-recognition、image-analyzer-v2、locate-anything-v2、stream-player、yolo-video-v2),约 3 万行代码、30–40% 互相复制:6 份相同的 ORT 引导、5 份设备选择、5 份画框、3 份设备绑定框架。Vision Hub 把公共层收进 `crates/vision-common`,对用户意味着:

- **装一个扩展**就有视觉能力,不用按任务挑扩展
- **一条命令面**(`analyze` / pipeline CRUD),LLM 工具调用不再需要在 40 个命名各异的命令里找
- **统一结果 schema**:任何任务都返回 `{task, detections[{label, confidence, bbox}], text?, inference_ms, accelerator}`
- **硬件加速可见**:`get_status` 显示每台硬件上每个模型实际跑在哪个加速器(CoreML/CUDA/CPU)以及决策原因

## 硬件加速矩阵

vision-common 的 `accel` 模块是唯一注册执行提供者(EP)的地方:探测一次硬件(`HardwareProfile::probe`),每个模型决策一次(`plan()` → `DevicePlan`),会话构建时应用 EP 约束(CUDA/TRT ⇒ GraphOpt Level 1 + 禁用 GeluFusion 系优化器 —— 这两个约束编码为数据,调用点无法漏配)。

| 平台 | 加速器 | 探测依据 | 获取方式 |
|---|---|---|---|
| macOS (Apple Silicon) | CoreML (ComputeUnits::All) | 编译期 `target_os` | 默认 .nep(捆绑 CoreML 版 ORT dylib) |
| Linux x86_64 + NVIDIA | CUDA | `/dev/nvidia0` + `nvidia-smi` 空闲显存 ≥ 2GB | `linux-x86_64-cuda` 变体 .nep |
| Jetson (Linux aarch64) | CUDA / TensorRT(预留 feature) | `/proc/device-tree/model` 板型识别 | `linux-aarch64-jetson` 变体(Jetson Zoo CUDA 版 ORT dylib)|
| Rockchip (RK3576 等) | 远程引擎(板端 rkllm3-server / 自建服务) | 板型识别(辅助) | VLM 批次接入 `RemoteEngine` |
| Windows | CPU(DirectML 预留,未启用) | — | 默认 .nep |
| 内存受限进程 | 强制 CPU | RLIMIT_AS ≤ 4GB(自动尝试抬升) | — |

**回退链**:计划加速器注册失败(dylib 不含该 EP)→ 自动 CPU,`get_status` 的 `loaded_tasks[].accelerator` 可见真实落点。

### 手动覆盖

任务级 `"device"` 参数 / pipeline 任务里 `"device": "cpu" | "cuda" | "coreml" | "tensorrt"`,以及 `analyze` 的顶层 `device` 参数。

## 命令

| 命令 | 说明 |
|---|---|
| `analyze` | `{image: base64或data-URL, tasks: [{type: detect, params: {confidence, iou, labels[], model, layout, input_size}}], include_annotated?}` → 统一 schema;agent 以 `vision-hub:analyze` 工具调用 |
| `get_status` | 硬件画像、每模型加速器与决策注记、pipeline 统计、模型缓存清单 |
| `create_pipeline` | `{pipeline: {id, source, tasks, sinks, schedule}}`(upsert 语义) |
| `list_pipelines` | 配置 + 运行态(最近结果/快照/错误) |
| `delete_pipeline` | `{id}` |
| `reload_models` | 丢弃全部模型会话,按当前配置懒加载重建 |
| `model_cache` | 缓存目录与文件清单 |

## Pipeline

```json
{
  "id": "gate-camera",
  "enabled": true,
  "source": {"type": "device", "device_id": "NE301-0001", "metric": "image"},
  "tasks": [{"type": "detect", "params": {"confidence": 0.5, "labels": ["person"]}}],
  "sinks": {
    "virtual_metrics": true,
    "event": true,
    "snapshot": true,
    "capture": {"kind": "presence", "labels": ["person"], "cooldown_secs": 60}
  },
  "schedule": {"on_frame": {"cooldown_secs": 2}},
  "draw": true
}
```

- **源**(本批次):设备 metric 帧(`DeviceMetric` 事件,支持嵌套路径 `image.frame` 与 data-URL/base64/MetricValue 包装)。stream(rtsp/文件/USB)与 url 源随 source 批次开放。
- **出口**:
  - 虚拟指标:`virtual.vision.<pipeline>.detections / inference_ms / labels` 写回绑定设备 —— 规则引擎、仪表盘 data-source、设备详情页直接可用
  - 事件:`vision.result`(payload 含 pipeline、results、时间戳)上 EventBus,`Custom` 通道
  - 快照:标注帧存运行态,`list_pipelines` 轮询(前端组件批次接入)
  - 抓拍:`capture` 规则(presence/absence/threshold × labels × 冷却)
- **持久化**:`$NEOMIND_EXTENSION_DIR/config.json`,进程重启自动恢复。

## 模型

- 捆绑:`models/yolo11n.onnx`(COCO 80 类)。v8/v9/v11/v12/v13 检测布局自动识别(按文件名或 `layout` 参数),v5/v6/v7、v10(NMS-free)同样支持。
- 自定义模型:放 `$NEOMIND_EXTENSION_DIR/models/`,`"model": "my-model.onnx"` + 可选 `"layout": "v8"`。
- 大模型按需下载(ModelManager:sha256 校验、断点重试、进度)在后续批次为 OCR/face 启用。

## 架构(给开发者)

```
crates/vision-common            # 共享运行时(不是扩展,发布脚本不会把它当扩展)
├── accel        # 硬件探测 + DevicePlan 纯逻辑 + Tier(移植自 paddle-ocr-v6)
├── engine       # OrtSession:唯一 EP 注册点,应用 DevicePlan 约束
├── models/yolo  # usls 检测路径移植(MIT,© Jamjamjon):letterbox/解码/类别无关 NMS
├── types        # Detection/BBox/VisionResult/ImageSource 统一 schema
├── image        # 解码编码/data-URL/设备值提取/FitAdaptive letterbox
├── native       # ORT dylib 引导(ORT_DYLIB_PATH/DYLD/符号链接)
├── draw         # 画框+标签(一份字体,支持 CJK 字体覆盖)
├── model        # ModelSpec + ModelManager(下载/校验/清单)
└── remote       # OpenAI 兼容远程引擎(VLM 批次使用)

extensions/vision-hub            # 本扩展
├── src/task.rs     # VisionTask trait + TaskRegistry(懒加载/按 model+device 复用)
├── src/detect.rs   # detect 任务(YoloDetector 包装)
├── src/pipeline.rs # pipeline 引擎(设备绑定→任务→出口)
└── src/lib.rs      # Extension 实现:命令/指标/事件订阅
```

**为什么脱离 usls**:fork(patches/usls)近 2 万行只为 3 个模型族,EP 注册被它的 feature 门控包住、每次加速策略调整都要改 fork;直接用 ort 后 EP 链、provider options、图优化约束都在自己代码里,RKNN 等非 ORT 运行时也有了接入位置(InferenceEngine 扩展点)。移植的解码逻辑与 usls 行为对齐(含类别无关 NMS),golden 回归测试随任务批次落地。

## 路线图(同一 schema,分批交付)

| 批次 | 内容 |
|---|---|
| **detect(本版)** | 检测端到端 + pipeline 引擎 + accel 层 + ModelManager |
| ocr | PP-OCR det+rec 移植(db/svtr),Tier 档位,ROI |
| face | SCRFD+ArcFace 移植(补上 EP 注册,修复旧实现纯 CPU 的缺口) |
| ground / vlm | RemoteEngine 接入(locate-anything / OpenAI 兼容) |
| source | RTSP/文件/USB 流源 + FFmpeg + NVDEC + 快照命令 |
| frontend | VisionPanel / VisionConsole / PipelineEditor / FaceLibrary 组件 |
| Stage B | 退役 7 个旧扩展 + 删除 patches/usls(见迁移表) |

## 旧扩展迁移对照

| 旧扩展 | vision-hub 对应 |
|---|---|
| yolo-device-inference bind_device | `create_pipeline`(source=device) |
| image-analyzer-v2 analyze_image | `analyze`(tasks=[detect]) |
| yolo-video-v2(流+ROI/越线)| source 批次(pipeline source=stream)|
| ocr-device-inference / face-recognition | ocr / face 批次 |
| locate-anything-v2 / video-vlm-v2 | ground / vlm 批次 |
| stream-player | source 批次(快照命令)|

过渡期旧扩展全部保留可用;Stage B 删除前会在 marketplace 公告。

## 开发

```bash
cargo test -p vision-common --features engine-ort   # 56 单测
cargo test -p vision-hub                            # 19 单测
./build.sh --dev --single vision-hub --skip-frontend # 本地安装
cargo run -p neomind-cli -- serve                    # 起平台,GET /api/docs 调命令
```

需要真实 ORT dylib 的推理验证(`#[ignore]` 测试)本地跑:`cargo test -p vision-hub -- --ignored`。
