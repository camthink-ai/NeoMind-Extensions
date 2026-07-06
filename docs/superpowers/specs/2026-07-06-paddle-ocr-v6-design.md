# PP-OCRv6 端侧 OCR 扩展 (`paddle-ocr-v6`) 设计文档

- **作者**: NeoMind Team
- **日期**: 2026-07-06
- **状态**: Draft v2 (after first spec review — critical issues C1/C2/C3 fixed)
- **Review 历史**: round 1 found 3 critical + 5 important issues; see §15 changelog
- **关联**: 替代方案讨论的产物；与现有 `ocr-device-inference` (v4) 和 `paddle-ocr-vl` (VLM) 共存

---

## 1. 背景与动机

### 1.1 现状

NeoMind 当前有两个 OCR 扩展：

| 扩展 | 定位 | 模型 | 体积 | 状态 |
|---|---|---|---|---|
| `ocr-device-inference` | 端侧原生 | PP-OCRv4 (DB + SVTR) | ~30 MB | 稳定，生产可用 |
| `paddle-ocr-vl` | 服务器 VLM | PaddleOCR-VL | ~MB (HTTP client) | 新增 |

PP-OCRv4 模型发布于 2024 年，精度和语种覆盖（仅中/英）已落后于 SOTA。PaddlePaddle 于 2025-06 发布了 **PP-OCRv6**，带来：

- 单模型覆盖 **50 语言**（中、繁、英、日 + 46 拉丁语系）
- 比 v5_server：识别准确率 +5.1%、检测 Hmean +4.6%、GPU 推理 2.37× 加速
- 三档 tier（tiny / small / medium），参数 1.5M–34.5M，覆盖 IoT 到服务器
- 新架构：PPLCNetV4 统一骨干 + RepLKFPN 检测 neck + EncoderWithLightSVTR 识别 neck

### 1.2 为什么不原地升级

原地升级 `ocr-device-inference` 到 v6 涉及：

- 破坏性 API 变更（双 recognizer → 单 recognizer，移除 `language` 参数）
- 模型替换的回归风险（v6 架构变了，预处理常量需重新验证）
- 影响已上线用户的稳定版

**结论**：新增独立扩展 `paddle-ocr-v6`，让 v4 保持稳定、v6 独立激进演进。这符合 NeoMind 现有的"多 OCR 扩展共存"模式（ne101_camera 的 `processingExtensionId` 下拉已支持多选）。

### 1.3 命名

`paddle-ocr-v6` —— 直接体现模型版本。和 `paddle-ocr-vl`（VLM 大模型版）的区分：
- `paddle-ocr-vl`：`VL` = Vision-Language（大模型）
- `paddle-ocr-v6`：`v6` = 模型版本号（端侧轻量）

display name: **PaddleOCR v6**，前端组件名: **PaddleOcrV6Card**。

---

## 2. 设计目标

1. **零破坏性**：不影响 `ocr-device-inference` (v4)、不影响 `paddle-ocr-vl`、不影响 NeoMind 主平台
2. **Multi-tier 支持**：tiny / small / medium 三档，按设备能力自动选择或运行时切换
3. **开箱即用**：默认 ship tiny (6 MB)，无网络环境也能跑
4. **端侧原生**：纯 Rust + ONNX Runtime，保留 CoreML/CUDA 加速能力，无 Python 依赖
5. **演进友好**：新代码不带 v4 历史包袱，单 recognizer，干净 API

---

## 3. 架构

### 3.1 项目结构

```
extensions/paddle-ocr-v6/
├── Cargo.toml                          # name = "paddle-ocr-v6", lib = "neomind_extension_paddle_ocr_v6"
├── README.md
├── download_models.sh                  # 拉 HuggingFace PaddlePaddle（替代 jamjamjon/assets）
├── models/                             # 默认 ship tiny (6 MB)
│   ├── ppocr-v6-tiny-det.onnx          # 1.7 MB
│   ├── ppocr-v6-tiny-rec.onnx          # 4.3 MB
│   └── ppocrv6_tiny_dict.txt           # 6,904 行
├── src/
│   ├── lib.rs                          # 主体（精简自 v1，单 recognizer）
│   ├── preset.rs                       # v6 preset 构造器（自维护，不依赖 usls 上游）
│   ├── downloader.rs                   # HuggingFace lazy download
│   └── tier.rs                         # Tier 枚举 + device-aware 自动选择
├── frontend/
│   ├── frontend.json                   # id: "paddle-ocr-v6"
│   ├── src/
│   │   └── PaddleOcrV6Card.tsx         # 复制自 OcrDeviceCard，加 tier dropdown + 进度条
│   └── dist/                           # UMD bundle
└── tests/
    └── fixtures/
        └── test.png                    # 集成测试样例
```

### 3.2 模块职责

- **`tier.rs`**: `Tier` 枚举（Tiny/Small/Medium/Auto），`resolve()` 根据 device + RAM 选具体 tier
- **`preset.rs`**: `ppocr_det_v6(tier)` / `ppocr_rec_v6(tier)` 返回 `usls::Config`（基于 `db()` / `svtr()` 基础 preset 加 v6 特定覆盖）
- **`downloader.rs`**: `ensure_models(tier, models_dir)` 检查文件存在性，缺失时从 HuggingFace 流式下载
- **`lib.rs`**: `PaddleOcrV6Extension` 实现 `Extension` trait；`OcrEngine` 内部持有 detector + 单 recognizer

### 3.3 与 `usls` 的关系

**复用 workspace 现有的 `patches/usls` fork**。workspace `Cargo.toml` 已经有 `[patch.crates-io] usls = { path = "patches/usls" }`，是为了 CUDA `is_available()` 检查问题打的补丁。本扩展继承这个 fork，**并在 fork 里追加 v6 必需的 builder methods**（详见 §8.1）。

依赖：workspace 统一 `usls = "0.1.11"`（通过 `[patch]` 解析到本地 fork）。

复用 usls 提供的：
- `Device` 自动检测（CoreML/CUDA/CPU）
- `Image` 处理 pipeline
- `DB` / `SVTR` 模型 wrapper
- `Config` fluent builder

需要在 `patches/usls` fork 里新增的 builder methods（~40 LOC）：
- `Config::with_db_unclip_ratio(f32)` — 覆盖硬编码的 `unwrap_or(1.5)` 默认值（v6 要 1.4）。在 `core/config.rs` 加字段（已存在）+ builder method。~5 LOC。
- `Config::with_swap_rgb(bool)` 或等价 hook — 让 det/rec 拿到 BGR 输入而不是 RGB（v6 训练用 BGR）。`core/image.rs` 里 `to_rgb8()` 在 **5 处调用点**（L83/92/110/211/139 间接），需要条件分支或封装一个 helper。~30-35 LOC。

**为什么不向上游提 PR**：preset 是薄层但 v6 需要的 builder method 涉及 usls 内部 image pipeline 和 db postprocess，需要 fork 改动。本仓库已经在 fork usls，继续在 fork 里维护更一致。如果上游接受 PR，未来可以移除 fork 里这部分改动。

---

## 4. 三 Tier 模型清单

### 4.1 模型文件

| Tier | det 文件 | rec 文件 | dict 文件 | 总体积 |
|---|---|---|---|---|
| **tiny** | `ppocr-v6-tiny-det.onnx` (1.7 MB) | `ppocr-v6-tiny-rec.onnx` (4.3 MB) | `ppocrv6_tiny_dict.txt` (6,904 行) | **6 MB** |
| **small** | `ppocr-v6-small-det.onnx` (9.4 MB) | `ppocr-v6-small-rec.onnx` (20.2 MB) | `ppocrv6_dict.txt` (18,708 行) | **30 MB** |
| **medium** | `ppocr-v6-medium-det.onnx` (59 MB) | `ppocr-v6-medium-rec.onnx` (73 MB) | `ppocrv6_dict.txt` (18,708 行) | **132 MB** |

### 4.2 下载 URL

**ONNX 模型**（HuggingFace，三档全部官方直出）：
```
https://huggingface.co/PaddlePaddle/PP-OCRv6_{tier}_{task}_onnx/resolve/main/inference.onnx
```
其中 `{tier} ∈ {tiny, small, medium}`，`{task} ∈ {det, rec}`。

**字典文件**（PaddleOCR GitHub raw）：
```
https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/master/ppocr/utils/dict/ppocrv6_dict.txt        # small/medium 共用
https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/master/ppocr/utils/dict/ppocrv6_tiny_dict.txt    # tiny 专用（去掉日文假名）
```

### 4.3 Tier 选择对照

| Tier | 适用场景 | RAM 预估 | CPU 推理时延（参考） |
|---|---|---|---|
| tiny | IoT / 浏览器 / 默认 ship | ~100 MB | 极快 |
| small | 桌面 / 移动 / Apple Silicon | ~300 MB | 中等 |
| medium | 服务器 / 工作站 / 有 GPU | ~500 MB+ | 慢（CPU），GPU 上快 |

---

## 5. 配置参数

### 5.1 启动时 config

通过 `configure()` 注入：

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `tier` | string enum | `"auto"` | `"tiny"` / `"small"` / `"medium"` / `"auto"` |
| `inference_device` | string | auto-detected | `"cpu"` / `"coreml"` / `"cuda"` 手动覆盖（注意：字段名是 `inference_device` 而不是 `device`，避免和 `bind_device.device_id` 概念碰撞） |
| `models_dir` | path | `<ext_dir>/models` | 模型存储目录 |
| `draw_boxes` | bool | `true` | 在 annotated image 上画 bbox |
| `roi_regions` | array | `[]` | 多边形 ROI 过滤（继承 v1） |
| `roi_overlap_threshold` | float | `0.5` | （继承 v1） |

**`configure()` 不触发模型重载**：`configure()` 只更新 `RwLock<Config>`，下次 `ensure_loaded()` / 帧处理时按需加载。要立即切换 tier，调用 `switch_tier` command（详见 §6.1）。这避免 `configure()` 阻塞数秒做模型重载。

### 5.2 `tier = auto` 解析规则

`tier.rs` 中的 `Tier::resolve(device, ram_gb)` 逻辑：

```
if device == CUDA && ram_gb >= 16:    → Medium
elif device == CoreML:                → Small      # Apple Silicon 默认
elif device == Cuda && ram_gb < 16:   → Small
else (CPU):                            → Tiny
```

**理由**：
- CUDA + 大内存：服务器场景，medium 收益最大
- Apple Silicon：CoreML 加速好，small 平衡精度/内存
- CPU/嵌入式：tiny 保响应速度
- 16 GB 阈值：参考当代笔记本/工作站主流配置，保守不冒进

**`auto` 解析时机**：在 `ensure_loaded()` 首次加载时解析一次；之后 `configure()` 改 `tier=auto` 不会自动重算（避免无谓重载）。用户想强制重新解析 → 调 `switch_tier { tier: "auto" }`。

---

## 6. Commands

| Command | 说明 | 主要参数 | 破坏性 |
|---|---|---|---|
| `bind_device` | 绑定设备图像流，每帧自动 OCR | `device_id`, `device_name`, `image_metric`, `draw_boxes` | 无（继承 v1 概念，去掉 `language`） |
| `unbind_device` | 解绑 | `device_id` | 无 |
| `toggle_binding` | 启停 | `device_id`, `active` | 无 |
| `get_bindings` | 列出绑定 | — | 无 |
| `recognize_image` | 单次 OCR（base64 输入） | `image` (base64) | 移除 `language` 参数（v6 单模型 multilingual） |
| `get_status` | 状态 + 当前 tier + 模型加载状态 + 下载进度 | — | 新增 `tier` / `model_loaded` / `download_progress` 字段 |
| `update_roi` | 设置 ROI 多边形 | `device_id`, `roi_regions`, `roi_overlap_threshold` | 无 |
| `configure` | 加载持久化配置（host 调用） | config JSON | 无 |
| **`switch_tier`** | **新**：运行时切换 tier，触发 lazy download + 模型重载 | `tier` | 新增 |

### 6.1 `switch_tier` 行为细节

```
1. 校验 tier 合法性
2. 如果新 tier != 当前 tier:
   a. 调用 downloader.ensure_models(new_tier, models_dir)
      - 文件已存在: 跳过下载
      - 缺失: 流式下载（带进度回调到 AtomicU64，不持有 OcrEngine Mutex）
   b. acquire OcrEngine Mutex
   c. 释放旧 detector / recognizer
   d. 加载新 tier 的 detector / recognizer
   e. 更新 metrics.model_tier
   f. release Mutex
3. 失败回退: 保留旧 tier 模型继续工作，返回警告
```

**并发设计**：
- 下载阶段（可能持续数十秒下载 medium）**不持有** `OcrEngine` Mutex，期间推理继续用旧 tier
- 只有最终的重载阶段（释放旧 + 加载新，约 1–3 秒）持有 Mutex
- 这避免 `switch_tier` 期间所有推理被阻塞数分钟

---

## 7. Metrics

继承 v1 的 5 个核心 metrics + 新增 3 个：

| Metric | Type | Unit | 说明 |
|---|---|---|---|
| `bound_devices` | Integer | count | （继承 v1） |
| `total_inferences` | Integer | count | （继承 v1） |
| `total_text_blocks` | Integer | count | （继承 v1） |
| `total_errors` | Integer | count | （继承 v1） |
| `last_inference_ms` | Integer | ms | （继承 v1） |
| **`model_tier`** | String | — | 当前生效 tier (`"tiny"` / `"small"` / `"medium"`) |
| **`model_loaded`** | Boolean | — | 模型是否就绪 |
| **`download_progress`** | Float | ratio | lazy download 进度 0.0–1.0（空闲时为 1.0） |

虚指标（per-device JSON / text）保持 v1 的 `virtual.ocr.text` / `virtual.ocr.full_text` / `virtual.ocr.count` / `virtual.ocr.confidence` 不变。

---

## 8. 关键实现细节

### 8.1 `preset.rs`（~120 LOC）

v6 preset 不是薄壳——它必须在 4 个地方有意识地偏离 `usls::Config::db()` / `svtr()` 的默认值。下面每个 override 都对应一个验证过的 v6 YAML 字段：

```rust
use crate::tier::Tier;
use usls::Config;

pub fn ppocr_det_v6(tier: Tier) -> Config {
    let box_thresh = match tier {
        Tier::Tiny => 0.40,        // v6 tiny YAML: box_thresh=0.4
        _ => 0.45,                  // v6 small/medium: box_thresh=0.45
    };
    Config::db()
        .with_model_file(det_filename(tier))     // ppocr-v6-{tier}-det.onnx
        .with_class_confs(&[box_thresh])         // 覆盖 db() 默认 0.35
        .with_db_unclip_ratio(1.4)               // 覆盖 impl.rs unwrap_or(1.5)；v6 YAML 全档 1.4
        .with_swap_rgb(true)                     // v6 det YAML: img_mode=BGR；usls Image 默认 RGB
        // 保持 db() 默认（已对齐 v6）:
        //   - image_mean/std = ImageNet (0.485/0.456/0.406 + 0.229/0.224/0.225) ✓ v6 det NormalizeImage
        //   - db_binary_thresh = 0.2 ✓ v6 YAML thresh=0.2
        //   - 输入动态范围 (608, 960, 1600) ✓ v6 limit_side_len=960
}

pub fn ppocr_rec_v6(tier: Tier) -> Config {
    Config::svtr()
        .with_model_file(rec_filename(tier))     // ppocr-v6-{tier}-rec.onnx
        .with_vocab_txt(dict_filename(tier))     // ppocrv6{?_tiny}_dict.txt
        .with_model_ixx(0, 3, (320, 960, 3200))  // width opt 改 320（v6 官方推理默认）
        .with_normalize(false)                   // ⚠️ v6 rec YAML 完全没有 NormalizeImage 操作
        //                                      //   模型期望 [0,255] 原始像素，svtr() 默认 true 会归一化到 [0,1]
        //                                      //   不覆盖会导致 rec 输出乱码（critical bug）
        .with_swap_rgb(true)                     // v6 rec YAML: img_mode=BGR
        // height=48 已是 usls svtr() 默认（v6 三档统一到 48）
}
```

**两个待加 builder method 在 `patches/usls` fork 里**：

1. **`with_db_unclip_ratio(f32)`** — 在 `core/config.rs` 加 builder；`db/impl.rs:40` 已经会读 `config.db_unclip_ratio()`，目前 `unwrap_or(1.5)` 是 fallback，不需要改 impl。
2. **`with_swap_rgb(bool)`** — 在 `core/config.rs` 加字段 + builder；在 `core/image.rs:83` 的 `to_rgb8()` 调用处条件分支（如果 `swap_rgb=true`，跳过 RGB 转换，保留 BGR）。或者更简洁的实现：在 image processor 阶段把 RGB→BGR 翻转一次。

这两个改动各 ~10 LOC，是 fork 维护成本的一部分。如果未来上游接受 PR，可以从 fork 移除。

**关键验证步骤**（实施时第一时间做）：
1. 先实现 det preset + fork 改动，加载 tiny 模型，跑 `tests/fixtures/test.png`
2. 对比 Python `paddleocr ocr --image test.png --engine onnxruntime` 的输出
3. bbox 数 / 文本内容一致 → preset 正确；不一致 → 调试 BGR/normalize/unclip
4. 只有 det 验证通过后才动 rec；rec 同样做对比验证

这个验证必须在集成测试之前手动做，因为 Python 对比是 ground truth。

### 8.2 `tier.rs`（~80 LOC）

```rust
pub enum Tier { Tiny, Small, Medium, Auto }

impl Tier {
    pub fn from_str(s: &str) -> Result<Self> { ... }
    pub fn filename_segment(&self) -> &'static str { "tiny" | "small" | "medium" }
    pub fn resolve(device: usls::Device, ram_gb: u64) -> Self {
        match (device, ram_gb) {
            (usls::Device::Cuda(_), n) if n >= 16 => Tier::Medium,
            (usls::Device::Cuda(_), _)             => Tier::Small,
            (usls::Device::CoreMl, _)              => Tier::Small,
            _                                       => Tier::Tiny,
        }
    }
}
```

**RAM 检测**：`sysinfo` crate 不在当前 workspace 依赖树里（已验证 `Cargo.lock` 无此包），需在 `paddle-ocr-v6/Cargo.toml` 新增 `sysinfo = "0.30"` 直接依赖。MVP 可先 hard-code 假设 8 GB（保守走 tiny/small），后续加精确检测。

### 8.3 `downloader.rs`（~150 LOC）

```rust
pub struct Downloader {
    progress: Arc<AtomicU64>,  // 0..=1000 (千分比避免浮点)
}

impl Downloader {
    pub fn ensure_models(&self, tier: Tier, models_dir: &Path) -> Result<()> {
        for (filename, url) in model_urls(tier) {
            let target = models_dir.join(filename);
            if target.exists() { continue; }
            self.download_with_retry(&url, &target, 3)?;
        }
        Ok(())
    }

    fn download_with_retry(&self, url: &str, target: &Path, retries: u32) -> Result<()> {
        let tmp = target.with_extension("part");
        for attempt in 0..retries {
            match self.download_once(url, &tmp) {
                Ok(_) => {
                    std::fs::rename(&tmp, target)?;
                    self.progress.store(1000, SeqCst);
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Download attempt {} failed: {}", attempt + 1, e);
                    std::thread::sleep(Duration::from_secs(1 << attempt));
                }
            }
        }
        Err(ExtensionError::ExecutionFailed(format!(
            "Download failed after {} attempts: {}", retries, url
        )))
    }

    fn download_once(&self, url: &str, target: &Path) -> Result<()> {
        // ureq streaming + Content-Length 校验 + 进度回调
    }
}
```

**关键约束**：
- `ureq` 流式下载（避免加载到内存）
- 校验 `Content-Length` 防止半包
- `*.part` 临时文件 + 原子 rename
- 失败重试 3 次，指数退避（1s, 2s, 4s）
- 进度写入 `AtomicU64`（千分比），metrics 暴露为 `download_progress = progress / 1000.0`

### 8.4 `lib.rs` 主要变化（相对 v1）

| 模块 | v1 (ocr-device-inference) | v2 (paddle-ocr-v6) |
|---|---|---|
| Recognizer 字段 | `recognizer_chinese: Option<SVTR>` + `recognizer_english: Option<SVTR>` | `recognizer: Option<SVTR>`（单） |
| 语言枚举 | `enum Language { Chinese, English }` | 删除（v6 单模型 multilingual） |
| Preset 调用 | `usls::Config::ppocr_det_v5_mobile()` | `crate::preset::ppocr_det_v6(tier)` |
| Tier 字段 | 无 | `tier: Tier` |
| 加载流程 | `ensure_loaded()` 直接读模型 | `ensure_loaded()` → `downloader.ensure_models()` → 读模型 |
| 新 command | — | `switch_tier` |

### 8.5 前端 (`PaddleOcrV6Card`)

复制 `OcrDeviceCard` 后：
- 改 component name 和 css class 前缀（`ocr-` → `paddle-ocr-v6-`）
- **移除 language 切换 UI**（v6 multilingual）
- **加 tier dropdown**：Auto / Tiny / Small / Medium（说明体积）
- **加下载进度条**：当 `download_progress < 1.0` 时显示
- 加状态徽章：当前 tier / 模型加载状态

---

## 9. 错误处理与回退

| 场景 | 处理 |
|---|---|
| Lazy download 网络失败 | 重试 3 次；失败后保留已下载部分，下次启动续传；3 次后报错并指引 `./download_models.sh <tier>` 手动拉 |
| ONNX 加载失败 | device fallback (CUDA → CoreML → CPU)；仍失败报错并附上 ORT 错误细节 |
| `switch_tier` 失败 | 保留旧 tier 继续工作；不强制重载；返回警告给 host |
| 首次启动无网络 | tiny 已 ship，开箱可用；切 small/medium 才需要网络 |
| 模型文件损坏（半包） | `download_once` 校验 `Content-Length`；不一致则删除重下 |

---

## 10. 测试策略

### 10.1 单元测试（不需真模型）

- `preset.rs`: `ppocr_det_v6(tier)` / `ppocr_rec_v6(tier)` 构造出的 `Config` 字段正确（model_file、box_thresh、ixx 范围）
- `tier.rs`: `Tier::resolve()` 在各种 device + RAM 组合下返回预期 tier
- `tier.rs`: `Tier::from_str()` 接受合法值、拒绝非法值
- `downloader.rs`: URL 拼接（mock tier）
- `downloader.rs`: 重试逻辑（mock HTTP 500/超时）
- `lib.rs`: metadata / commands / metrics descriptors 完整性
- `lib.rs`: `configure()` 更新 tier 后 `get_status` 反映新值

### 10.2 集成测试（需真模型，本地验证 + 待加 CI）

> **注意**：当前 `.github/workflows/build-extension.yml` 只跑构建，不跑 `cargo test`。本扩展的集成测试默认是**本地手动验证**。在 §12 步骤 7 里要把 `cargo test -p paddle-ocr-v6` 加入 CI workflow。

- 加载 tiny tier（默认 ship，测试不需联网下载）
- 对 `tests/fixtures/test.png`（含 "Hello" + "你好" 双语文本，**已提交到仓库**）跑 `recognize_image`
- 断言：返回 ≥ 2 个 text_blocks，至少一个包含 "Hello" 子串，至少一个包含 "你好" 子串
- 断言：平均 confidence > 0.5
- 断言：bbox 在 `[0, 1]` 归一化范围内
- 断言：processing_time_ms > 0

### 10.2.5 Python 对比验证（实施前必做，§8.1 已强调）

在写集成测试之前，**手动**做一次 Python 对比：

```bash
# Python side (ground truth)
pip install paddleocr onnxruntime
paddleocr ocr --image tests/fixtures/test.png \
    --text_detection_model_name PP-OCRv6_tiny_det \
    --text_recognition_model_name PP-OCRv6_tiny_rec \
    --engine onnxruntime

# Rust side
cargo test -p paddle-ocr-v6 --test integration_test
```

对比两边的 text_blocks 文本、bbox 数、confidence。一致才能继续。这是 §11 critical 风险（BGR / unclip / normalize）的最后一道防线。

### 10.3 回归测试

v1 (`ocr-device-inference`) 完全不动，独立测试套件不受影响。

### 10.4 手动验证清单

- macOS Apple Silicon：tiny / small / medium 三档都能加载，CoreML 加速生效
- Linux CUDA：medium tier GPU 推理
- Linux CPU only：tiny tier 性能可接受（< 1 s/帧）
- Windows：CPU 推理路径通畅
- Lazy download：删除 small 模型文件，配置 `tier=small`，验证自动下载并加载

---

## 11. 风险与待二次确认

### 11.1 Critical（已识别缓解方案，实施时第一时间验证）

1. **BGR vs RGB 通道顺序**（det + rec 都受影响）
   - **事实**：v6 det YAML (`img_mode: BGR`) + v6 rec YAML (`img_mode: BGR`) 都用 BGR；usls `core/image.rs:83` 无条件 `to_rgb8()`
   - **缓解**：在 `patches/usls` fork 加 `Config::with_swap_rgb(bool)` builder + image pipeline 条件分支（~10 LOC）
   - **验证**：§10.2.5 Python 对比测试

2. **v6 rec 模型不归一化**（critical，spec review round 1 发现）
   - **事实**：v6 medium rec YAML **完全没有 NormalizeImage 操作**，模型期望 [0, 255] 原始像素；usls `svtr()` 默认 `with_normalize(true)` 会归一化到 [0, 1]，结果是乱码
   - **缓解**：`ppocr_rec_v6` 加 `.with_normalize(false)`
   - **影响**：仅 preset.rs 一行；但这是必须做的，遗漏会让 rec 输出全错
   - **注意**：v1 (`ocr-device-inference`) 也可能有这个 bug，但 v4 模型也许碰巧用归一化训练；v6 肯定没

3. **`unclip_ratio` 硬编码 fallback**（critical，spec review round 1 发现）
   - **事实**：v6 det YAML 全档 `unclip_ratio=1.4`；usls `db/impl.rs:40` 是 `config.db_unclip_ratio().unwrap_or(1.5)`，db() preset 没设置此字段
   - **缓解**：在 `patches/usls` fork 加 `Config::with_db_unclip_ratio(f32)` builder（~10 LOC，core/config.rs 加字段+builder method）
   - **影响**：不覆盖会让多边形过度扩张，crop 区域偏大，rec 准确率掉点

### 11.2 High（验证过的低风险）

4. **`box_thresh` 通过 `with_class_confs` 覆盖**
   - **已验证（spec review）**：`patches/usls/src/models/db/impl.rs:38` 和 L134 确认 `class_confs` 通过 `DynConf::new_or_default(config.class_confs(), 1)` 真的映射到 `box_thresh`
   - 风险等级：低。`.with_class_confs(&[0.45])` 可行

### 11.3 Medium

5. **CoreML 上 medium 性能未实测**
   - PP-OCRv6 medium 在 Apple Silicon 上的 ONNX Runtime CoreML EP 性能未知
   - **缓解**：`auto` 规则默认 Apple Silicon → small，避免冒进
   - **fallback**：必要时把 medium 单独转 `.mlmodel`（额外工作，不在 MVP 范围）

6. **`sysinfo` RAM 检测跨平台一致性**
   - 不同平台返回值可能有偏差（容器内、虚拟机内）
   - **缓解**：MVP 先用保守阈值（16 GB），后续根据反馈调整；提供 `inference_device` config 手动覆盖

### 11.4 Low

7. **medium det ONNX 体积 59 MB**
   - GitHub release 单 asset 无大小限制（< 2 GB），但用户体感差
   - **缓解**：`auto` 规则默认不选 medium，需要显式配置

8. **字典文件一致性**
   - HuggingFace `inference.yml` 里内嵌的 `character_dict` 和 GitHub raw 的 `ppocrv6_dict.txt` 是否完全一致未确认
   - **缓解**：以 GitHub raw 的 `ppocrv6_dict.txt` 为准；实施时加 SHA256 校验（错误字典会静默产生乱码而不是报错）

---

## 12. 实施步骤概要

详细任务计划由后续 `superpowers:writing-plans` skill 生成。高层步骤：

1. **fork usls 扩展**（§3.3、§8.1）：在 `patches/usls` 加 `with_db_unclip_ratio`（~5 LOC）+ `with_swap_rgb`（~30-35 LOC，涉及 `core/image.rs` 多处 `to_rgb8()` 调用点）
2. **脚手架**：复制 `ocr-device-inference/` → `paddle-ocr-v6/`；改 Cargo.toml、metadata、lib name
3. **模型准备**：下载 tiny 三件套到 `models/`；改写 `download_models.sh` 拉 HuggingFace
4. **核心模块**：实现 `tier.rs`、`preset.rs`（含 §11.1 三个 critical override）、`downloader.rs`
5. **Python 对比验证**（§10.2.5）：手动跑一次，确认 det/rec 输出和 paddleocr 一致，再继续
6. **lib.rs 改造**：单 recognizer、删 Language、加 `switch_tier` command、加新 metrics
7. **前端**：复制 `OcrDeviceCard` → `PaddleOcrV6Card`；加 tier UI 和进度条
8. **测试**：单元测试 + 集成测试（含 test.png fixture）
9. **CI 接入**：在 `.github/workflows/build-extension.yml` 加 `cargo test -p paddle-ocr-v6` 步骤
10. **文档**：README + frontend.json + 加入 workspace `Cargo.toml`
11. **构建验证**：`./build.sh --single paddle-ocr-v6`，确认 `.nep` 包内含 `models/`
12. **市场注册**：`./scripts/update-versions.sh` 生成 metadata.json、加入 index.json
13. **手动验证清单**：第 10.4 节

---

## 13. 不在范围内（Non-Goals）

- **不修改 v1 (`ocr-device-inference`)**：v4 保持稳定不动
- **不做 ocr-core 共享 crate 重构**：第二阶段视维护痛点再决定
- **不支持 NRTR 解码分支**：v6 rec 训练时用 CTC+NRTR 双头，推理只用 CTC（NRTR 是训练辅助）
- **不做模型微调/训练支持**：纯推理扩展
- **不做 PP-StructureV3 表格识别**：那是 `paddle-ocr-vl` 的领域
- **不做 Android/iOS 移动端原生构建**：NeoMind 当前 6 平台已足够

---

## 14. 参考资料

- [PP-OCRv6 HuggingFace Collection](https://huggingface.co/collections/PaddlePaddle/pp-ocrv6)
- [PaddleOCR configs/det/PP-OCRv6/](https://github.com/PaddlePaddle/PaddleOCR/tree/master/configs/det/PP-OCRv6)
- [PaddleOCR configs/rec/PP-OCRv6/](https://github.com/PaddlePaddle/PaddleOCR/tree/master/configs/rec/PP-OCRv6)
- [ppocrv6_dict.txt](https://github.com/PaddlePaddle/PaddleOCR/blob/master/ppocr/utils/dict/ppocrv6_dict.txt)
- [ppocrv6_tiny_dict.txt](https://github.com/PaddlePaddle/PaddleOCR/blob/master/ppocr/utils/dict/ppocrv6_tiny_dict.txt)
- [PP-OCRv6 算法文档](https://www.paddleocr.ai/main/en/version3.x/algorithm/PP-OCRv6/PP-OCRv6.html)
- [usls repo](https://github.com/jamjamjon/usls)
- [PP-OCRv6 arXiv 论文](https://arxiv.org/abs/2606.13108)

---

## 15. Spec Changelog

### v2 (2026-07-06, after round 1 review)

**Critical 修复**：
- **C1**: §3.3 改写——之前说"不 fork usls"是错的，workspace 已有 `patches/usls` fork（`Cargo.toml` L66-67 `[patch.crates-io]` 块）。新表述承认复用现有 fork，并明确要在 fork 里加 `with_db_unclip_ratio` 和 `with_swap_rgb` builder methods。
- **C2**: §8.1 加 `.with_db_unclip_ratio(1.4)`——v6 YAML 全档 1.4，usls 默认 1.5。之前 spec 完全没提。
- **C3**: §8.1 加 `.with_normalize(false)` for rec——v6 rec YAML 完全没有 NormalizeImage，模型期望 [0,255] 原始像素；usls `svtr()` 默认 `with_normalize(true)` 会归一化导致乱码。之前 spec 完全漏掉这个风险。

**Important 修复**：
- §5.1: `device` 字段改名为 `inference_device`（避免和 `bind_device.device_id` 概念碰撞）
- §5.1: 明确 `configure()` 不触发模型重载（避免阻塞数秒）
- §6.1: `switch_tier` 并发模型细化——下载阶段不持有 `OcrEngine` Mutex
- §8.2: `sysinfo` 不在依赖树，需新增直接依赖
- §10.2: CI 不跑 `cargo test`，集成测试默认本地验证；§12 加 CI 接入步骤
- §10.2.5 新增：实施前必做的 Python 对比验证（det + rec 各一次）

**Minor 改进**：
- §8.1 LOC 预算从 ~80 调到 ~120
- §11 重构为 Critical / High / Medium / Low 四档；§11.1 项 #2 (`box_thresh`) 已验证低风险
- §11.4 项 #8 加 SHA256 字典校验建议
- §12 步骤从 10 步扩到 13 步（加 fork 扩展、Python 验证、CI 接入）
