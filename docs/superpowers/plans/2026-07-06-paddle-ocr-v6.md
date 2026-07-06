# paddle-ocr-v6 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增独立扩展 `paddle-ocr-v6`，端侧原生 PP-OCRv6 ONNX 推理，三档 tier（tiny/small/medium），默认 ship tiny，运行时可切换并 lazy-download 其他 tier。

**Architecture:** 复制 `ocr-device-inference` 作为起点；在 `patches/usls` fork 加 2 个 builder method（`with_db_unclip_ratio` + `with_swap_rgb`）；新模块 `tier.rs` / `preset.rs` / `downloader.rs` 处理 tier 选择、v6 preset 构造、HuggingFace 下载；单 recognizer 取代 v1 双 recognizer 结构（v6 单模型已 multilingual）。

**Tech Stack:** Rust + neomind-extension-sdk + usls (forked) + ONNX Runtime + ureq + React/TypeScript (frontend)

**Spec:** `docs/superpowers/specs/2026-07-06-paddle-ocr-v6-design.md`

---

## File Structure

### 新建文件

| 路径 | 职责 |
|---|---|
| `extensions/paddle-ocr-v6/Cargo.toml` | 包元数据 |
| `extensions/paddle-ocr-v6/README.md` | 用户文档 |
| `extensions/paddle-ocr-v6/download_models.sh` | 模型下载脚本（HuggingFace） |
| `extensions/paddle-ocr-v6/src/lib.rs` | 扩展主体（精简自 v1，单 recognizer） |
| `extensions/paddle-ocr-v6/src/tier.rs` | Tier 枚举 + device-aware 自动选择 |
| `extensions/paddle-ocr-v6/src/preset.rs` | v6 usls::Config 构造器（含 critical override） |
| `extensions/paddle-ocr-v6/src/downloader.rs` | HuggingFace 模型下载器 |
| `extensions/paddle-ocr-v6/frontend/frontend.json` | 前端组件元数据 |
| `extensions/paddle-ocr-v6/frontend/src/PaddleOcrV6Card.tsx` | React 组件 |
| `extensions/paddle-ocr-v6/frontend/src/main.tsx` | UMD entry |
| `extensions/paddle-ocr-v6/frontend/package.json` | npm 元数据 |
| `extensions/paddle-ocr-v6/frontend/vite.config.ts` | Vite UMD 构建配置 |
| `extensions/paddle-ocr-v6/frontend/tsconfig.json` | TS 配置 |
| `extensions/paddle-ocr-v6/tests/fixtures/test.png` | 集成测试图（含 "Hello" + "你好"） |
| `extensions/paddle-ocr-v6/tests/integration_test.rs` | 集成测试 |
| `extensions/paddle-ocr-v6/models/.gitkeep` | 模型目录占位（实际模型 gitignored，build 时打包） |

### 修改文件

| 路径 | 改动 |
|---|---|
| `Cargo.toml` (workspace) | members 加 `extensions/paddle-ocr-v6` |
| `patches/usls/src/core/config.rs` | 加 `swap_rgb: Option<bool>` 字段 + Default |
| `patches/usls/src/core/image.rs` | 5 处 `to_rgb8()` 调用点条件分支（基于 Config.swap_rgb） |
| `.github/workflows/build-extension.yml` | 加 `cargo test -p paddle-ocr-v6` 步骤 |
| `extensions/index.json` | 注册新扩展（update-versions.sh 自动生成） |

### 不修改的文件（重要）

- `extensions/ocr-device-inference/**` — v1 保持稳定不动
- `extensions/paddle-ocr-vl/**` — VLM 扩展独立
- `patches/usls/src/models/db/config.rs` — db() preset 不动（避免影响 yolo-device-inference 等其他依赖）

---

## Phase 0: Pre-flight Checks

### Task 0.1: 验证工作环境

**Files:** 无修改

- [ ] **Step 1: 确认当前分支和工作树状态**

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind-Extensions"
git status
git branch --show-current
```

Expected: 在 `main` 分支；`paddle-ocr-vl/` untracked（这是预期状态，不要清理）。

- [ ] **Step 2: 确认 usls fork 编译通过（基线）**

```bash
cargo check -p usls 2>&1 | tail -5
```

Expected: `Finished` 无错误。如果失败，**停止**——先修复 workspace 基线。

- [ ] **Step 3: 确认 v1 (ocr-device-inference) 测试通过**

```bash
cargo test -p ocr-device-inference --lib 2>&1 | tail -10
```

Expected: 所有测试 PASS。记录测试数作为基线。

- [ ] **Step 4: 创建 worktree（推荐，可选）**

参考 @superpowers:using-git-worktrees。如果不想用 worktree，跳过。

---

## Phase 1: patches/usls fork 扩展

### Task 1.1: 验证 `with_db_unclip_ratio` 是否已被 aksr::Builder 宏自动生成

**背景**: `Config` struct 使用 `#[derive(aksr::Builder)]`，宏会为每个 `pub` 字段自动生成 `with_<field>` 方法。`db_unclip_ratio: Option<f32>` 字段已存在（`core/config.rs:64`），所以方法**可能已存在**（grep 看不到因为宏展开）。

**Files:** 临时测试文件，验证后删除

- [ ] **Step 1: 写一个临时测试调用 with_db_unclip_ratio**

修改 `extensions/ocr-device-inference/src/lib.rs` 临时加一行（最后会 revert）：

在 `try_load_detector` 函数（约 line 597）的 `let config = ...` 后加：

```rust
let _test = config.clone().with_db_unclip_ratio(Some(1.4));
```

- [ ] **Step 2: 编译检查**

```bash
cargo check -p ocr-device-inference 2>&1 | tail -15
```

- [ ] **Step 3: 根据结果决定路径**

  - **如果编译通过**：宏已自动生成 → 跳过 Task 1.2，直接 Task 1.3。Revert Step 1 的临时改动。
  - **如果编译失败**（error: no method named `with_db_unclip_ratio`）：需要手动加 → 执行 Task 1.2。

- [ ] **Step 4: Revert 临时改动**

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind-Extensions"
git diff extensions/ocr-device-inference/src/lib.rs  # 确认只有那一行
git checkout extensions/ocr-device-inference/src/lib.rs
```

### Task 1.2（条件执行）: 手动添加 `with_db_unclip_ratio` builder

**只在 Task 1.1 Step 3 编译失败时执行。**

**Files:**
- Modify: `patches/usls/src/models/db/config.rs`

- [ ] **Step 1: 在 `impl crate::Config` 块末尾（`db_resnet50_u8` 之后、闭合 `}` 之前）添加**

```rust
    /// Override the DB unclip ratio (default 1.5).
    /// v6 YAML uses 1.4; v4/v5 tolerate the default.
    pub fn with_db_unclip_ratio(mut self, ratio: impl Into<Option<f32>>) -> Self {
        self.db_unclip_ratio = ratio.into();
        self
    }
```

- [ ] **Step 2: 编译验证**

```bash
cargo check -p usls 2>&1 | tail -5
```

Expected: `Finished`

- [ ] **Step 3: 单元测试验证**

写一个临时测试：

```bash
cat > /tmp/usls_test.rs <<'EOF'
#[test]
fn test_with_db_unclip_ratio() {
    let cfg = usls::Config::db().with_db_unclip_ratio(1.4_f32);
    assert_eq!(cfg.db_unclip_ratio, Some(1.4));
}
EOF
# 拷到 usls tests 目录测试
cp /tmp/usls_test.rs patches/usls/tests/zzz_db_ratio_test.rs
cargo test -p usls --test zzz_db_ratio_test 2>&1 | tail -10
rm patches/usls/tests/zzz_db_ratio_test.rs
```

Expected: 1 test passed.

- [ ] **Step 4: Commit**

```bash
git add patches/usls/src/models/db/config.rs
git commit -m "feat(usls): add with_db_unclip_ratio builder for PP-OCRv6 support"
```

### Task 1.3: 添加 `swap_rgb` 字段到 Config

**Files:**
- Modify: `patches/usls/src/core/config.rs`

- [ ] **Step 1: 在 `db_unclip_ratio` 字段旁加 `swap_rgb`**

定位到 `patches/usls/src/core/config.rs:64`（`pub db_unclip_ratio: Option<f32>,` 那一行），下一行加：

```rust
    pub swap_rgb: Option<bool>,
```

- [ ] **Step 2: 在 Default 实现里初始化**

定位到 `patches/usls/src/core/config.rs:98`（`db_unclip_ratio: Some(1.5),` 那一行），下一行加：

```rust
            swap_rgb: None,
```

- [ ] **Step 3: 编译检查**

```bash
cargo check -p usls 2>&1 | tail -5
```

Expected: `Finished`（aksr::Builder 会自动生成 `with_swap_rgb(Option<bool>)`）

- [ ] **Step 4: 验证 builder 已生成**

```bash
# 写一个临时测试调用 with_swap_rgb
cat > patches/usls/tests/zzz_swap_rgb_test.rs <<'EOF'
#[test]
fn test_with_swap_rgb_generated() {
    let cfg = usls::Config::db().with_swap_rgb(Some(true));
    assert_eq!(cfg.swap_rgb, Some(true));
}
EOF
cargo test -p usls --test zzz_swap_rgb_test 2>&1 | tail -10
rm patches/usls/tests/zzz_swap_rgb_test.rs
```

Expected: 1 test passed. 如果失败（method not found），需要手动加 builder method（参考 Task 1.2 模式）。

- [ ] **Step 5: Commit**

```bash
git add patches/usls/src/core/config.rs
git commit -m "feat(usls): add swap_rgb Config field for BGR input support"
```

### Task 1.4: 在 image pipeline 5 处 `to_rgb8()` 调用点应用 swap_rgb

**Files:**
- Modify: `patches/usls/src/core/image.rs`

**关键事实**：`to_rgb8()` 在 5 处调用（L83, L92, L110, L139, L211）。我们需要在每处根据 Config.swap_rgb 决定是 RGB 还是 BGR。

**实现策略**：加一个 `Image::to_rgb_or_bgr8(swap: bool)` helper，封装条件逻辑。然后 5 处调用点改成传 swap 参数。

- [ ] **Step 1: 先读完整 image.rs 找到所有调用点的上下文**

```bash
sed -n '75,145p' patches/usls/src/core/image.rs
sed -n '200,225p' patches/usls/src/core/image.rs
```

- [ ] **Step 2: 在 image.rs 末尾加 helper method**

在 `impl Image { ... }` 块里（找 `pub fn to_rgb8(&self) -> RgbImage` 那个 impl 块），加：

```rust
    /// Convert to RGB8 or BGR8 based on `swap_rgb` flag.
    /// PP-OCRv6 det/rec train with BGR input; usls default path forces RGB.
    pub fn to_rgb_or_bgr8(&self, swap_rgb: bool) -> RgbImage {
        let rgb = self.image.to_rgb8();
        if !swap_rgb {
            return rgb;
        }
        // Swap R and B channels in-place
        let mut bgr = rgb;
        for pixel in bgr.pixels_mut() {
            let r = pixel[0];
            pixel[0] = pixel[2];
            pixel[2] = r;
        }
        bgr
    }
```

需要 `use image::RgbImage;`（应该已在文件顶部 import）。

- [ ] **Step 3: 改 5 处调用点**

每处 `image.to_rgb8()` 或 `image.into_rgb8()` 改成 `image.to_rgb_or_bgr8(swap_rgb)`，并在函数签名里加 `swap_rgb: bool` 参数。

具体定位（行号可能漂移，按内容匹配）：

L83 附近（`From<DynamicImage>`）：
```rust
impl From<DynamicImage> for Image {
    fn from(image: DynamicImage) -> Self {
        Self {
            image: image.to_rgb8(),  // ← 这行
            ..Default::default()
        }
    }
}
```

**问题**：`From` trait 不能传 swap_rgb 参数。这两个 trait impl 必须保持 RGB（默认行为）。

**修正策略**：**不改 trait impl**（保持 RGB 默认）。改在 processor pipeline 里：在 image 转成 usls::Image 之后、送进 ONNX 之前，根据 Config.swap_rgb 做一次通道翻转。

- [ ] **Step 4: 重新规划——在 processor.rs 加翻转步骤**

放弃改 image.rs，改在 `patches/usls/src/core/processor.rs` 的处理流程里加翻转。

读 processor.rs：

```bash
grep -n "fn process\|to_rgb\|swap" patches/usls/src/core/processor.rs | head -20
```

- [ ] **Step 5: 实现 swap_rgb 在 processor**

参考 processor.rs 实际代码（运行 Step 4 后看输出）。预期改动：在 `process()` 函数末尾、return tensor 之前，如果 `cfg.swap_rgb == Some(true)`，对 tensor 的 channel 维做 `[R,G,B] → [B,G,R]` 翻转。

**由于这一步依赖 processor.rs 实际结构**，实施时根据 Step 4 输出调整。如果 processor 改动复杂（>50 LOC），降级方案：在 `preset.rs` 不用 swap_rgb，而是在 `paddle-ocr-v6/src/lib.rs` 里加载图像后手动翻转通道再喂给 usls。

- [ ] **Step 6: 编译 + 测试**

```bash
cargo check -p usls 2>&1 | tail -5
cargo test -p usls --lib 2>&1 | tail -10
```

Expected: 所有现有测试仍 PASS（不能破坏 usls 现有功能）。

- [ ] **Step 7: Commit**

```bash
git add patches/usls/
git commit -m "feat(usls): apply swap_rgb in image processor for BGR input"
```

---

## Phase 2: 脚手架

### Task 2.1: 复制 v1 作为起点

**Files:**
- Create: `extensions/paddle-ocr-v6/**`（整目录）

- [ ] **Step 1: 整目录复制**

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind-Extensions"
cp -R extensions/ocr-device-inference extensions/paddle-ocr-v6
```

- [ ] **Step 2: 清理无关文件**

```bash
cd extensions/paddle-ocr-v6
rm -rf target node_modules
rm -f Cargo.lock
# 删 v1 的旧模型（v6 会用新的）
rm -f models/*.onnx models/*.txt
touch models/.gitkeep
```

### Task 2.2: 改 Cargo.toml

**Files:**
- Modify: `extensions/paddle-ocr-v6/Cargo.toml`

- [ ] **Step 1: Read 当前 Cargo.toml**

```bash
cat extensions/paddle-ocr-v6/Cargo.toml
```

- [ ] **Step 2: 替换为 v6 版本**

完整替换文件内容为：

```toml
[package]
name = "paddle-ocr-v6"
version = "2.7.7"
edition = "2021"
authors = ["NeoMind Team"]
license = "Apache-2.0"
description = "Edge-native PP-OCRv6 OCR extension with multi-tier model support (tiny/small/medium)"

[lib]
name = "neomind_extension_paddle_ocr_v6"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
chrono = "0.4"
base64 = "0.22"
image = "0.25"
imageproc = "0.24"
ab_glyph = "0.2"
parking_lot = "0.12"
tracing = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time"] }
uuid = { version = "1.0", features = ["v4"] }
ureq = { version = "2", features = ["json"] }
sysinfo = "0.30"

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ort = { workspace = true }
usls = { workspace = true }

[features]
default = []

[dev-dependencies]
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "test-util"] }
```

注意：相比 v1 新增 `ureq`（HTTP 下载）、`sysinfo`（RAM 检测）。

- [ ] **Step 3: 加入 workspace**

修改根 `Cargo.toml`，在 `members = [...]` 列表末尾加：

```toml
    "extensions/paddle-ocr-v6",
```

- [ ] **Step 4: 验证加入 workspace**

```bash
cargo check -p paddle-ocr-v6 2>&1 | tail -10
```

Expected: 编译错误（因为 lib.rs 里还是 v1 的内容，FFI export 名字不对），但能识别这个 package。

### Task 2.3: 占位 main.rs 让 workspace 编译通过

为了让后续任务能逐步改 lib.rs 而不阻塞 workspace 编译，先把 lib.rs 简化成空壳。

**Files:**
- Modify: `extensions/paddle-ocr-v6/src/lib.rs`

- [ ] **Step 1: 把 lib.rs 替换成最小骨架**

完整替换为：

```rust
//! paddle-ocr-v6 — NeoMind Edge PP-OCRv6 OCR extension.
//! Independent of ocr-device-inference (PP-OCRv4) and paddle-ocr-vl (VLM).
//! See docs/superpowers/specs/2026-07-06-paddle-ocr-v6-design.md

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, Result,
};
use serde_json::Value;

pub struct PaddleOcrV6Extension;

#[async_trait]
impl Extension for PaddleOcrV6Extension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("paddle-ocr-v6", "PaddleOCR v6", env!("CARGO_PKG_VERSION"))
                .with_description("Edge-native PP-OCRv6 OCR with multi-tier model support")
                .with_author("NeoMind Team")
        })
    }

    async fn execute_command(&self, _command: &str, _args: &Value) -> Result<Value> {
        Err(neomind_extension_sdk::ExtensionError::ExecutionFailed(
            "paddle-ocr-v6 not yet implemented".into(),
        ))
    }
}

impl Default for PaddleOcrV6Extension {
    fn default() -> Self {
        Self
    }
}

neomind_extension_sdk::neomind_export!(PaddleOcrV6Extension);
```

- [ ] **Step 2: 编译验证**

```bash
cargo check -p paddle-ocr-v6 2>&1 | tail -5
```

Expected: `Finished`

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml extensions/paddle-ocr-v6/
git commit -m "feat(paddle-ocr-v6): scaffold extension skeleton"
```

---

## Phase 3: 模型下载

### Task 3.1: 下载 tiny tier 三件套

**Files:**
- Create: `extensions/paddle-ocr-v6/models/ppocr-v6-tiny-det.onnx`
- Create: `extensions/paddle-ocr-v6/models/ppocr-v6-tiny-rec.onnx`
- Create: `extensions/paddle-ocr-v6/models/ppocrv6_tiny_dict.txt`

- [ ] **Step 1: 下载 det 模型（1.7 MB）**

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind-Extensions/extensions/paddle-ocr-v6/models"
curl -L -o ppocr-v6-tiny-det.onnx \
  "https://huggingface.co/PaddlePaddle/PP-OCRv6_tiny_det_onnx/resolve/main/inference.onnx"
ls -lh ppocr-v6-tiny-det.onnx
```

Expected: ~1.7 MB。

- [ ] **Step 2: 下载 rec 模型（4.3 MB）**

```bash
curl -L -o ppocr-v6-tiny-rec.onnx \
  "https://huggingface.co/PaddlePaddle/PP-OCRv6_tiny_rec_onnx/resolve/main/inference.onnx"
ls -lh ppocr-v6-tiny-rec.onnx
```

Expected: ~4.3 MB。

- [ ] **Step 3: 下载字典（6904 行）**

```bash
curl -L -o ppocrv6_tiny_dict.txt \
  "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/master/ppocr/utils/dict/ppocrv6_tiny_dict.txt"
wc -l ppocrv6_tiny_dict.txt
```

Expected: 6904 lines。

- [ ] **Step 4: 加 .gitignore（模型不入 git，由 build.sh 打包）**

```bash
cat > ../.gitignore.models <<'EOF'
# Don't commit models — they're bundled by build.sh
*.onnx
!*.gitkeep
EOF
# 合并到现有 .gitignore 或保留独立
```

实际操作：检查仓库根 `.gitignore`，如果没有 `*.onnx` 规则就加上。模型通过 build.sh 在 .nep 里打包。

### Task 3.2: 改写 download_models.sh

**Files:**
- Modify: `extensions/paddle-ocr-v6/download_models.sh`

- [ ] **Step 1: 完整替换 download_models.sh**

```bash
#!/bin/bash
# Download PP-OCRv6 ONNX models for paddle-ocr-v6 extension.
# Usage:
#   ./download_models.sh              # download tiny (default shipped)
#   ./download_models.sh small        # download small tier
#   ./download_models.sh medium       # download medium tier
#   ./download_models.sh all          # download all three tiers

set -e

MODELS_DIR="$(dirname "$0")/models"
mkdir -p "$MODELS_DIR"

TIER="${1:-tiny}"

# HF model URLs
DET_URL_BASE="https://huggingface.co/PaddlePaddle/PP-OCRv6"
REC_URL_BASE="https://huggingface.co/PaddlePaddle/PP-OCRv6"
DICT_BASE="https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/master/ppocr/utils/dict"

download_tier() {
    local tier=$1
    echo "=== Downloading PP-OCRv6 ${tier} ==="

    local det_file="ppocr-v6-${tier}-det.onnx"
    local rec_file="ppocr-v6-${tier}-rec.onnx"
    local dict_file
    if [ "$tier" = "tiny" ]; then
        dict_file="ppocrv6_tiny_dict.txt"
    else
        dict_file="ppocrv6_dict.txt"
    fi

    # det
    if [ ! -f "$MODELS_DIR/$det_file" ]; then
        echo " Downloading $det_file ..."
        curl -L -o "$MODELS_DIR/$det_file" \
            "${DET_URL_BASE}_${tier}_det_onnx/resolve/main/inference.onnx"
    fi

    # rec
    if [ ! -f "$MODELS_DIR/$rec_file" ]; then
        echo "  Downloading $rec_file ..."
        curl -L -o "$MODELS_DIR/$rec_file" \
            "${REC_URL_BASE}_${tier}_rec_onnx/resolve/main/inference.onnx"
    fi

    # dict
    if [ ! -f "$MODELS_DIR/$dict_file" ]; then
        echo "  Downloading $dict_file ..."
        curl -L -o "$MODELS_DIR/$dict_file" "${DICT_BASE}/${dict_file}"
    fi

    echo "  ✓ ${tier} ready"
    ls -lh "$MODELS_DIR/$det_file" "$MODELS_DIR/$rec_file" "$MODELS_DIR/$dict_file"
}

case "$TIER" in
    tiny|small|medium)
        download_tier "$TIER"
        ;;
    all)
        download_tier tiny
        download_tier small
        download_tier medium
        ;;
    *)
        echo "Unknown tier: $TIER"
        echo "Usage: $0 [tiny|small|medium|all]"
        exit 1
        ;;
esac

echo ""
echo "Done. Models in: $MODELS_DIR"
```

- [ ] **Step 2: 加可执行权限 + 验证**

```bash
chmod +x extensions/paddle-ocr-v6/download_models.sh
# 验证语法
bash -n extensions/paddle-ocr-v6/download_models.sh && echo "syntax OK"
```

- [ ] **Step 3: Commit**

```bash
git add extensions/paddle-ocr-v6/download_models.sh extensions/paddle-ocr-v6/models/
git commit -m "feat(paddle-ocr-v6): bundle tiny tier models + download script"
```

---

## Phase 4: 核心模块（TDD）

### Task 4.1: 实现 `tier.rs`（Tier 枚举 + device-aware 选择）

**Files:**
- Create: `extensions/paddle-ocr-v6/src/tier.rs`
- Modify: `extensions/paddle-ocr-v6/src/lib.rs`（加 `mod tier;`）

- [ ] **Step 1: 在 lib.rs 顶部加 mod 声明**

```rust
mod tier;
```

放在文件顶部 doc comment 之后。

- [ ] **Step 2: 写 tier.rs 的失败测试**

创建 `extensions/paddle-ocr-v6/src/tier.rs`：

```rust
//! Tier selection for PP-OCRv6 models.

use neomind_extension_sdk::ExtensionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Tiny,
    Small,
    Medium,
    Auto,
}

impl Tier {
    /// Parse from a config string. Case-insensitive.
    pub fn from_str(s: &str) -> Result<Self, ExtensionError> {
        match s.to_lowercase().as_str() {
            "tiny" => Ok(Tier::Tiny),
            "small" => Ok(Tier::Small),
            "medium" => Ok(Tier::Medium),
            "auto" => Ok(Tier::Auto),
            _ => Err(ExtensionError::InvalidArguments(format!(
                "Unknown tier: '{}'. Expected: tiny|small|medium|auto",
                s
            ))),
        }
    }

    /// Concrete tier (resolves Auto → specific).
    pub fn resolve(self, has_cuda: bool, has_coreml: bool, ram_gb: u64) -> Tier {
        match self {
            Tier::Tiny | Tier::Small | Tier::Medium => self,
            Tier::Auto => {
                if has_cuda && ram_gb >= 16 {
                    Tier::Medium
                } else if has_cuda || has_coreml {
                    Tier::Small
                } else {
                    Tier::Tiny
                }
            }
        }
    }

    /// Filename segment used in model files: "tiny" / "small" / "medium".
    /// Panics on Auto (must resolve first).
    pub fn filename_segment(&self) -> &'static str {
        match self {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Medium => "medium",
            Tier::Auto => panic!("Tier::Auto has no filename; call resolve() first"),
        }
    }

    /// Display string for metrics.
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Medium => "medium",
            Tier::Auto => "auto",
        }
    }
}

impl Default for Tier {
    fn default() -> Self {
        Tier::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_valid() {
        assert_eq!(Tier::from_str("tiny").unwrap(), Tier::Tiny);
        assert_eq!(Tier::from_str("SMALL").unwrap(), Tier::Small);
        assert_eq!(Tier::from_str("Medium").unwrap(), Tier::Medium);
        assert_eq!(Tier::from_str("auto").unwrap(), Tier::Auto);
    }

    #[test]
    fn test_from_str_invalid() {
        assert!(Tier::from_str("huge").is_err());
        assert!(Tier::from_str("").is_err());
    }

    #[test]
    fn test_resolve_auto_cpu_only() {
        assert_eq!(Tier::Auto.resolve(false, false, 4), Tier::Tiny);
        assert_eq!(Tier::Auto.resolve(false, false, 32), Tier::Tiny);
    }

    #[test]
    fn test_resolve_auto_coreml() {
        assert_eq!(Tier::Auto.resolve(false, true, 8), Tier::Small);
        assert_eq!(Tier::Auto.resolve(false, true, 32), Tier::Small);
    }

    #[test]
    fn test_resolve_auto_cuda() {
        assert_eq!(Tier::Auto.resolve(true, false, 8), Tier::Small);
        assert_eq!(Tier::Auto.resolve(true, false, 16), Tier::Medium);
        assert_eq!(Tier::Auto.resolve(true, false, 64), Tier::Medium);
    }

    #[test]
    fn test_resolve_explicit_passthrough() {
        assert_eq!(Tier::Tiny.resolve(true, true, 64), Tier::Tiny);
        assert_eq!(Tier::Medium.resolve(false, false, 2), Tier::Medium);
    }

    #[test]
    fn test_filename_segment() {
        assert_eq!(Tier::Tiny.filename_segment(), "tiny");
        assert_eq!(Tier::Small.filename_segment(), "small");
        assert_eq!(Tier::Medium.filename_segment(), "medium");
    }

    #[test]
    #[should_panic(expected = "Tier::Auto has no filename")]
    fn test_filename_segment_auto_panics() {
        let _ = Tier::Auto.filename_segment();
    }
}
```

- [ ] **Step 3: 跑测试**

```bash
cargo test -p paddle-ocr-v6 --lib tier 2>&1 | tail -20
```

Expected: 8 tests passed。

- [ ] **Step 4: Commit**

```bash
git add extensions/paddle-ocr-v6/src/tier.rs extensions/paddle-ocr-v6/src/lib.rs
git commit -m "feat(paddle-ocr-v6): add Tier enum with device-aware resolution"
```

### Task 4.2: 实现 `preset.rs`（v6 usls Config 构造器）

**Files:**
- Create: `extensions/paddle-ocr-v6/src/preset.rs`
- Modify: `extensions/paddle-ocr-v6/src/lib.rs`（加 `mod preset;`）

- [ ] **Step 1: 写 preset.rs**

```rust
//! PP-OCRv6 preset builders — self-maintained alternatives to usls's
//! `ppocr_det_v5_mobile()` / `ppocr_rec_v5_mobile()`.
//!
//! Critical overrides vs usls defaults:
//! - det: box_thresh (0.40 tiny / 0.45 others), unclip_ratio=1.4, swap_rgb=true
//! - rec: normalize=false (v6 trained on [0,255] raw pixels), swap_rgb=true,
//!        width opt=320

use crate::tier::Tier;
use usls::Config;

/// Det model filename for a given tier, e.g. "ppocr-v6-tiny-det.onnx".
pub fn det_filename(tier: Tier) -> String {
    format!("ppocr-v6-{}-det.onnx", tier.filename_segment())
}

/// Rec model filename for a given tier.
pub fn rec_filename(tier: Tier) -> String {
    format!("ppocr-v6-{}-rec.onnx", tier.filename_segment())
}

/// Dictionary filename for a given tier.
/// Tiny uses a separate (smaller, no Japanese) dictionary.
pub fn dict_filename(tier: Tier) -> &'static str {
    match tier {
        Tier::Tiny => "ppocrv6_tiny_dict.txt",
        _ => "ppocrv6_dict.txt",
    }
}

/// Build a `usls::Config` for PP-OCRv6 detection.
pub fn ppocr_det_v6(tier: Tier, models_dir: &std::path::Path) -> Config {
    let box_thresh: f32 = match tier {
        Tier::Tiny => 0.40,
        _ => 0.45,
    };
    let det_path = models_dir.join(det_filename(tier));

    Config::db()
        .with_model_file(det_path.to_string_lossy().to_string())
        .with_class_confs(&[box_thresh])
        .with_db_unclip_ratio(Some(1.4))
        .with_swap_rgb(Some(true))
}

/// Build a `usls::Config` for PP-OCRv6 recognition.
pub fn ppocr_rec_v6(tier: Tier, models_dir: &std::path::Path) -> Config {
    let rec_path = models_dir.join(rec_filename(tier));
    let dict_path = models_dir.join(dict_filename(tier));

    Config::svtr()
        .with_model_file(rec_path.to_string_lossy().to_string())
        .with_vocab_txt(dict_path.to_string_lossy().to_string())
        .with_model_ixx(0, 3, (320, 960, 3200))
        .with_normalize(false)
        .with_swap_rgb(Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filenames() {
        assert_eq!(det_filename(Tier::Tiny), "ppocr-v6-tiny-det.onnx");
        assert_eq!(det_filename(Tier::Medium), "ppocr-v6-medium-det.onnx");
        assert_eq!(rec_filename(Tier::Small), "ppocr-v6-small-rec.onnx");
    }

    #[test]
    fn test_dict_filename() {
        assert_eq!(dict_filename(Tier::Tiny), "ppocrv6_tiny_dict.txt");
        assert_eq!(dict_filename(Tier::Small), "ppocrv6_dict.txt");
        assert_eq!(dict_filename(Tier::Medium), "ppocrv6_dict.txt");
    }

    #[test]
    fn test_det_preset_constructs() {
        // 不实际加载模型，只验证 Config 能构造
        let cfg = ppocr_det_v6(Tier::Tiny, std::path::Path::new("/tmp"));
        // 验证关键字段
        assert_eq!(cfg.class_confs, vec![0.40_f32.to_string()].first().cloned().unwrap_or_default().parse::<f32>().unwrap_or(0.0), 0.40);
        assert_eq!(cfg.db_unclip_ratio, Some(1.4));
        assert_eq!(cfg.swap_rgb, Some(true));
    }

    #[test]
    fn test_rec_preset_constructs() {
        let cfg = ppocr_rec_v6(Tier::Small, std::path::Path::new("/tmp"));
        // 验证 normalize=false 已覆盖 svtr() 默认的 true
        // (processor config 是嵌套的，验证可能需要更深的访问路径)
        assert_eq!(cfg.swap_rgb, Some(true));
    }
}
```

**注意**：Step 3 的测试可能需要根据 usls Config 实际字段类型调整。`class_confs` 字段类型可能是 `Vec<String>` 或 `Vec<f32>`，跑测试时按编译错误调整。

- [ ] **Step 2: 加 mod 声明**

在 `lib.rs` 顶部加：

```rust
mod preset;
```

- [ ] **Step 3: 跑测试**

```bash
cargo test -p paddle-ocr-v6 --lib preset 2>&1 | tail -30
```

Expected: 测试 PASS。如果有编译错误，按错误信息调整（特别是字段类型不匹配的情况）。

- [ ] **Step 4: Commit**

```bash
git add extensions/paddle-ocr-v6/src/preset.rs extensions/paddle-ocr-v6/src/lib.rs
git commit -m "feat(paddle-ocr-v6): add v6 preset with critical overrides (unclip/normalize/swap_rgb)"
```

### Task 4.3: 实现 `downloader.rs`（HuggingFace lazy download）

**Files:**
- Create: `extensions/paddle-ocr-v6/src/downloader.rs`
- Modify: `extensions/paddle-ocr-v6/src/lib.rs`（加 `mod downloader;`）

- [ ] **Step 1: 写 downloader.rs**

```rust
//! Lazy model downloader for PP-OCRv6 tiers.
//!
//! Downloads ONNX + dict from HuggingFace PaddlePaddle/PP-OCRv6_*_onnx
//! on demand when switching to a non-default tier.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use neomind_extension_sdk::{ExtensionError, Result};

use crate::preset::{det_filename, dict_filename, rec_filename};
use crate::tier::Tier;

const HF_BASE: &str = "https://huggingface.co/PaddlePaddle/PP-OCRv6";
const DICT_BASE: &str =
    "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/master/ppocr/utils/dict";

pub struct Downloader {
    /// Progress 0..=1000 (per-mille). Read by metrics producer.
    pub progress: std::sync::Arc<AtomicU64>,
}

impl Default for Downloader {
    fn default() -> Self {
        Self {
            progress: std::sync::Arc::new(AtomicU64::new(1000)),
        }
    }
}

impl Downloader {
    /// Ensure all three model files for `tier` exist in `models_dir`.
    /// Downloads missing files. Idempotent.
    pub fn ensure_models(&self, tier: Tier, models_dir: &Path) -> Result<()> {
        let files = required_files(tier, models_dir);
        let missing: Vec<_> = files
            .iter()
            .filter(|(_, path)| !path.exists())
            .collect();
        if missing.is_empty() {
            self.progress.store(1000, Ordering::SeqCst);
            return Ok(());
        }

        let total = missing.len() as u64;
        for (i, (url, target)) in missing.iter().enumerate() {
            self.download_with_retry(url, target, 3)?;
            let pct = ((i + 1) as u64 * 1000) / (total * 1000 / total.max(1));
            self.progress.store(pct, Ordering::SeqCst);
        }
        self.progress.store(1000, Ordering::SeqCst);
        Ok(())
    }

    fn download_with_retry(&self, url: &str, target: &Path, retries: u32) -> Result<()> {
        let tmp = target.with_extension("part");
        let mut last_err = None;
        for attempt in 0..retries {
            match self.download_once(url, &tmp) {
                Ok(_) => {
                    std::fs::rename(&tmp, target).map_err(|e| {
                        ExtensionError::ExecutionFailed(format!("rename failed: {}", e))
                    })?;
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(
                        "Download attempt {}/{} failed for {}: {}",
                        attempt + 1,
                        retries,
                        url,
                        e
                    );
                    last_err = Some(e);
                    let _ = std::fs::remove_file(&tmp);
                    std::thread::sleep(Duration::from_secs(1u64 << attempt));
                }
            }
        }
        Err(ExtensionError::ExecutionFailed(format!(
            "Download failed after {} attempts: {} (last error: {})",
            retries,
            url,
            last_err.unwrap_or_else(|| "unknown".into())
        )))
    }

    fn download_once(&self, url: &str, target: &Path) -> Result<()> {
        let resp = ureq::get(url)
            .timeout(Duration::from_secs(300))
            .call()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("HTTP: {}", e)))?;

        let expected_len = resp
            .header("Content-Length")
            .and_then(|s| s.parse::<u64>().ok());

        if let Some(code) = resp.status() {
            if code >= 400 {
                return Err(ExtensionError::ExecutionFailed(format!(
                    "HTTP {} for {}",
                    code, url
                )));
            }
        }

        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(target).map_err(|e| {
            ExtensionError::ExecutionFailed(format!("create file failed: {}", e))
        })?;
        std::io::copy(&mut reader, &mut file).map_err(|e| {
            ExtensionError::ExecutionFailed(format!("write failed: {}", e))
        })?;

        // Verify size if Content-Length was provided
        if let Some(expected) = expected_len {
            let actual = std::fs::metadata(target)
                .map(|m| m.len())
                .unwrap_or(0);
            if actual != expected {
                return Err(ExtensionError::ExecutionFailed(format!(
                    "Size mismatch: expected {} bytes, got {}",
                    expected, actual
                )));
            }
        }

        Ok(())
    }
}

/// Return [(url, local_path)] for the three files of a tier.
fn required_files(tier: Tier, models_dir: &Path) -> Vec<(String, PathBuf)> {
    let det = det_filename(tier);
    let rec = rec_filename(tier);
    let dict = dict_filename(tier);

    vec![
        (
            format!("{}/PP-OCRv6_{}_det_onnx/resolve/main/inference.onnx", HF_BASE, tier.filename_segment().to_uppercase()),
            models_dir.join(&det),
        ),
        (
            format!("{}/PP-OCRv6_{}_rec_onnx/resolve/main/inference.onnx", HF_BASE, tier.filename_segment().to_uppercase()),
            models_dir.join(&rec),
        ),
        (
            format!("{}/{}", DICT_BASE, dict),
            models_dir.join(dict),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_files_count() {
        let files = required_files(Tier::Tiny, Path::new("/tmp"));
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn test_urls_contain_correct_tier() {
        let files = required_files(Tier::Medium, Path::new("/tmp"));
        let urls: Vec<_> = files.iter().map(|(u, _)| u.as_str()).collect();
        assert!(urls[0].contains("PP-OCRv6_MEDIUM_det_onnx"));
        assert!(urls[1].contains("PP-OCRv6_MEDIUM_rec_onnx"));
    }

    #[test]
    fn test_dict_url_uses_tiny_for_tiny_tier() {
        let files = required_files(Tier::Tiny, Path::new("/tmp"));
        let dict_url = &files[2].0;
        assert!(dict_url.ends_with("ppocrv6_tiny_dict.txt"));
    }

    #[test]
    fn test_dict_url_uses_full_for_non_tiny() {
        let files = required_files(Tier::Small, Path::new("/tmp"));
        let dict_url = &files[2].0;
        assert!(dict_url.ends_with("ppocrv6_dict.txt"));
    }

    #[test]
    fn test_ensure_models_noop_when_files_exist() {
        // Create tmp dir with the three tiny files
        let tmp = tempfile_dir();
        for fname in &["ppocr-v6-tiny-det.onnx", "ppocr-v6-tiny-rec.onnx", "ppocrv6_tiny_dict.txt"] {
            std::fs::write(tmp.join(fname), "fake").unwrap();
        }
        let dl = Downloader::default();
        let result = dl.ensure_models(Tier::Tiny, &tmp);
        assert!(result.is_ok());
        // Progress should be 1000 (full)
        assert_eq!(dl.progress.load(Ordering::SeqCst), 1000);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("paddle-ocr-v6-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
```

- [ ] **Step 2: 加 mod 声明 + uuid 已在 Cargo.toml**

在 `lib.rs` 顶部加：

```rust
mod downloader;
```

- [ ] **Step 3: 跑测试**

```bash
cargo test -p paddle-ocr-v6 --lib downloader 2>&1 | tail -15
```

Expected: 5 tests passed。

- [ ] **Step 4: Commit**

```bash
git add extensions/paddle-ocr-v6/src/downloader.rs extensions/paddle-ocr-v6/src/lib.rs
git commit -m "feat(paddle-ocr-v6): add HuggingFace lazy downloader with retry"
```

---

## Phase 5: Python 对比验证（手动 gate）

> **关键检查点**：在写完 preset 后、改造 lib.rs 之前，必须用 Python ground truth 验证 v6 ONNX + preset 配置确实能正确识别。

### Task 5.1: 准备测试图

**Files:**
- Create: `extensions/paddle-ocr-v6/tests/fixtures/test.png`

- [ ] **Step 1: 找一张含 "Hello" + "你好" 的图**

可以用任意现有图，或用 PIL 生成：

```bash
mkdir -p extensions/paddle-ocr-v6/tests/fixtures
python3 -c "
from PIL import Image, ImageDraw, ImageFont
img = Image.new('RGB', (640, 200), 'white')
d = ImageDraw.Draw(img)
# 用系统字体，没有就 fallback
try:
    font_en = ImageFont.truetype('/System/Library/Fonts/Helvetica.ttc', 48)
    font_ch = ImageFont.truetype('/System/Library/Fonts/PingFang.ttc', 48)
except Exception:
    font_en = ImageFont.load_default()
    font_ch = font_en
d.text((20, 30), 'Hello World', fill='black', font=font_en)
d.text((20, 110), '你好世界', fill='black', font=font_ch)
img.save('extensions/paddle-ocr-v6/tests/fixtures/test.png')
print('created test.png')
"
ls -lh extensions/paddle-ocr-v6/tests/fixtures/test.png
```

### Task 5.2: Python paddleocr ground truth

- [ ] **Step 1: 安装 paddleocr（在 venv 里）**

```bash
python3 -m venv /tmp/paddleocr-venv
source /tmp/paddleocr-venv/bin/activate
pip install paddleocr onnxruntime
```

- [ ] **Step 2: 跑 Python 推理记录 ground truth**

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind-Extensions"
paddleocr ocr \
    -i extensions/paddle-ocr-v6/tests/fixtures/test.png \
    --text_detection_model_name PP-OCRv6_tiny_det \
    --text_recognition_model_name PP-OCRv6_tiny_rec \
    --engine onnxruntime \
    --use_doc_orientation_classify False \
    --use_doc_unwarping False \
    --use_textline_orientation False 2>&1 | tee /tmp/python_ground_truth.txt
```

记录：
- 检测到几个 text region？
- 每个 region 的 rec_text 是什么？
- bbox 坐标（pixel）
- confidence

- [ ] **Step 3: 保存 ground truth**

```bash
cp /tmp/python_ground_truth.txt extensions/paddle-ocr-v6/tests/fixtures/python_ground_truth.txt
```

这个文件作为后续 Rust 集成测试的断言依据。

---

## Phase 6: lib.rs 改造

### Task 6.1: 实现 OcrEngine（detector + 单 recognizer）

**Files:**
- Modify: `extensions/paddle-ocr-v6/src/lib.rs`

参考 v1 的 `OcrEngine` 结构（约 line 484-700），但：
- 删除 `recognizer_chinese: Option<SVTR>` + `recognizer_english: Option<SVTR>`
- 加 `recognizer: Option<SVTR>`（单）
- 删除 `Language` 枚举
- 加 `tier: Tier` + `downloader: Downloader` 字段

- [ ] **Step 1: Read v1 OcrEngine 实现**

```bash
sed -n '480,870p' extensions/ocr-device-inference/src/lib.rs > /tmp/v1_engine.rs
wc -l /tmp/v1_engine.rs
```

通读一遍，理解 `OcrEngine`、`ensure_loaded`、`try_load_detector`、`load_recognizer`、`recognize`、`crop_polygon_static` 等方法。

- [ ] **Step 2: 在 paddle-ocr-v6 lib.rs 写 OcrEngine 结构**

在 `PaddleOcrV6Extension` struct 之前加：

```rust
/// Internal OCR engine holding detector + single multilingual recognizer.
pub struct OcrEngine {
    pub detector: Option<usls::models::DB>,
    pub recognizer: Option<usls::models::SVTR>,
    pub tier: Tier,
    pub downloader: Downloader,
    pub models_dir: std::path::PathBuf,
    pub loaded: bool,
    pub load_error: Option<String>,
}

impl OcrEngine {
    pub fn new() -> Self {
        Self {
            detector: None,
            recognizer: None,
            tier: Tier::default(),
            downloader: Downloader::default(),
            models_dir: default_models_dir(),
            loaded: false,
            load_error: None,
        }
    }

    pub fn ensure_loaded(&mut self, tier: Tier, inference_device_hint: Option<&str>) {
        if self.loaded && self.tier == tier {
            return;
        }
        // Resolve Auto → concrete
        let resolved = tier.resolve(
            has_cuda(inference_device_hint),
            has_coreml(inference_device_hint),
            total_ram_gb(),
        );
        self.tier = resolved;

        // Lazy download if missing
        if let Err(e) = self.downloader.ensure_models(resolved, &self.models_dir) {
            self.loaded = false;
            self.load_error = Some(format!("download failed: {}", e));
            return;
        }

        // Load detector + recognizer using preset
        match self.try_load_detector(resolved) {
            Ok(d) => self.detector = Some(d),
            Err(e) => {
                self.loaded = false;
                self.load_error = Some(format!("detector load failed: {}", e));
                return;
            }
        }
        match self.try_load_recognizer(resolved) {
            Ok(r) => self.recognizer = Some(r),
            Err(e) => {
                self.loaded = false;
                self.load_error = Some(format!("recognizer load failed: {}", e));
                return;
            }
        }
        self.loaded = true;
        self.load_error = None;
    }

    fn try_load_detector(&mut self, tier: Tier) -> std::result::Result<usls::models::DB, String> {
        let cfg = crate::preset::ppocr_det_v6(tier, &self.models_dir);
        let cfg = cfg
            .with_device_all(auto_device())
            .commit()
            .map_err(|e| format!("config commit failed: {}", e))?;
        usls::models::DB::new(cfg).map_err(|e| format!("DB init failed: {}", e))
    }

    fn try_load_recognizer(&mut self, tier: Tier) -> std::result::Result<usls::models::SVTR, String> {
        let cfg = crate::preset::ppocr_rec_v6(tier, &self.models_dir);
        let cfg = cfg
            .with_device_all(auto_device())
            .commit()
            .map_err(|e| format!("config commit failed: {}", e))?;
        usls::models::SVTR::new(cfg).map_err(|e| format!("SVTR init failed: {}", e))
    }
}

fn default_models_dir() -> std::path::PathBuf {
    // Resolve relative to the extension binary at runtime.
    // NeoMind installs .nep with models/ next to the .dylib/.so.
    std::env::current_dir().unwrap_or_default().join("models")
}

fn has_cuda(hint: Option<&str>) -> bool {
    if let Some(h) = hint {
        return h.eq_ignore_ascii_case("cuda");
    }
    // Best-effort: probe via usls::Device
    // (Skip actual CUDA check in tests; rely on platform)
    cfg!(target_os = "linux") && std::env::var("CUDA_VISIBLE_DEVICES").is_ok()
}

fn has_coreml(hint: Option<&str>) -> bool {
    if let Some(h) = hint {
        return h.eq_ignore_ascii_case("coreml");
    }
    cfg!(target_os = "macos")
}

fn total_ram_gb() -> u64 {
    // Use sysinfo; fallback to 8 if probe fails
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_memory();
    (sys.total_memory() / (1024 * 1024 * 1024)).max(0) as u64
}

fn auto_device() -> usls::Device {
    // 复用 v1 的 auto_device 逻辑
    #[cfg(target_os = "macos")]
    {
        // CoreML preferred on Apple Silicon
        return usls::Device::CoreMl;
    }
    #[cfg(not(target_os = "macos"))]
    {
        if has_cuda(None) {
            return usls::Device::Cuda(0);
        }
        return usls::Device::Cpu(0);
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo check -p paddle-ocr-v6 2>&1 | tail -15
```

Expected: `Finished`（如果 usls Device API 有差异，按编译错误调整）

- [ ] **Step 4: Commit**

```bash
git add extensions/paddle-ocr-v6/src/lib.rs
git commit -m "feat(paddle-ocr-v6): add OcrEngine with tier-aware loading"
```

### Task 6.2: 实现 `recognize` 推理流程

**Files:**
- Modify: `extensions/paddle-ocr-v6/src/lib.rs`

参考 v1 `recognize` 方法（约 line 677-870），改成单 recognizer。

- [ ] **Step 1: 在 OcrEngine impl 块加 recognize 方法**

参考 v1，但移除 language 切换分支：

```rust
impl OcrEngine {
    pub fn recognize(
        &mut self,
        image_data: &[u8],
        device_id: &str,
        roi_regions: &[RoiPolygon],
        roi_overlap_threshold: f32,
    ) -> Result<OcrResult> {
        let start = std::time::Instant::now();
        if !self.loaded {
            return Err(ExtensionError::ExecutionFailed(
                self.load_error.clone().unwrap_or_else(|| "Models not loaded".into()),
            ));
        }

        // Load image using image crate, convert to usls::Image
        let dyn_img = image::load_from_memory(image_data)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("image load failed: {}", e)))?;
        let img: usls::Image = dyn_img.into();
        let (img_w, img_h) = (dyn_img.width(), dyn_img.height());

        // Detection
        let det_results = if let Some(ref mut detector) = self.detector {
            detector.forward(&img).map_err(|e| {
                ExtensionError::ExecutionFailed(format!("det forward failed: {}", e))
            })?
        } else {
            return Err(ExtensionError::ExecutionFailed("detector not loaded".into()));
        };

        // Crop polygons
        let mut crops_with_data: Vec<(usls::Image, BoundingBox, Option<Vec<[f32; 2]>>)> = Vec::new();
        if let Some(det_result) = det_results.first() {
            for polygon in &det_result.polygons {
                let bbox = polygon_to_bbox_static(polygon, img_w, img_h);
                // ROI filter
                if !roi_regions.is_empty()
                    && !bbox_in_roi(&bbox, roi_regions, roi_overlap_threshold)
                {
                    continue;
                }
                if let Some(crop) = crop_polygon_static(&img, polygon) {
                    crops_with_data.push((crop, bbox, None));
                }
            }
        }
        if crops_with_data.is_empty() {
            return Ok(OcrResult {
                text_blocks: vec![],
                full_text: String::new(),
                processing_time_ms: start.elapsed().as_millis() as f64,
            });
        }

        // Recognition (single recognizer for all languages)
        let crop_images: Vec<usls::Image> = crops_with_data.iter().map(|(c, _, _)| c.clone()).collect();
        let rec_results = if let Some(ref mut rec) = self.recognizer {
            rec.forward(&crop_images).map_err(|e| {
                ExtensionError::ExecutionFailed(format!("rec forward failed: {}", e))
            })?
        } else {
            return Err(ExtensionError::ExecutionFailed("recognizer not loaded".into()));
        };

        // Assemble results
        let mut blocks = Vec::with_capacity(crops_with_data.len());
        let mut full_text_parts = Vec::with_capacity(crops_with_data.len());
        for (i, rec_result) in rec_results.iter().enumerate() {
            let (_, bbox, _) = &crops_with_data[i];
            if let Some(text_obj) = rec_result.texts.first() {
                let text = text_obj.label.clone();
                let confidence = text_obj.confidence.unwrap_or(1.0);
                full_text_parts.push(text.clone());
                blocks.push(TextBlock {
                    text,
                    confidence,
                    bbox: bbox.clone(),
                });
            }
        }

        Ok(OcrResult {
            text_blocks: blocks,
            full_text: full_text_parts.join("\n"),
            processing_time_ms: start.elapsed().as_millis() as f64,
        })
    }
}
```

**注意**：`OcrResult`、`TextBlock`、`BoundingBox`、`RoiPolygon` 等结构定义要从 v1 复制过来（参考 v1 lib.rs L192-330）。`crop_polygon_static` / `polygon_to_bbox_static` / `bbox_in_roi` helper 也要复制（v1 lib.rs L873-960）。

- [ ] **Step 2: 复制必要的 helper 函数和 struct**

从 v1 lib.rs 复制（参考对应行号）：
- `RoiPolygon` (L192-200)
- `BoundingBox` (L237-295)
- `TextBlock` (L293-303)
- `OcrResult` (L303-318)
- `crop_polygon_static` (L873-)
- `polygon_to_bbox_static` (L903-)
- `bbox_in_roi` （在 v1 找一下，可能在 ROI 处理附近）

- [ ] **Step 3: 编译验证**

```bash
cargo check -p paddle-ocr-v6 2>&1 | tail -20
```

Expected: `Finished`。可能有多处编译错误（字段名、API 差异），按错误逐个修。

- [ ] **Step 4: Commit**

```bash
git add extensions/paddle-ocr-v6/src/lib.rs
git commit -m "feat(paddle-ocr-v6): implement recognize pipeline (single recognizer)"
```

### Task 6.3: 实现 Extension trait（commands、metrics、configure）

**Files:**
- Modify: `extensions/paddle-ocr-v6/src/lib.rs`

参考 v1 的 `impl Extension for OcrDeviceInference`（L1292-1956），改成 `PaddleOcrV6Extension`。

- [ ] **Step 1: 完整 Extension trait impl**

替换 `PaddleOcrV6Extension` 的占位 impl（Task 2.3 写的最小骨架）为完整实现：

```rust
pub struct PaddleOcrV6Extension {
    engine: parking_lot::Mutex<OcrEngine>,
    config: parking_lot::RwLock<RuntimeConfig>,
    request_count: std::sync::atomic::AtomicI64,
    success_count: std::sync::atomic::AtomicI64,
    failure_count: std::sync::atomic::AtomicI64,
    total_text_blocks: std::sync::atomic::AtomicI64,
    last_inference_ms: std::sync::atomic::AtomicI64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct RuntimeConfig {
    tier: String,        // "auto" / "tiny" / "small" / "medium"
    inference_device: Option<String>,
    draw_boxes: bool,
    roi_regions: Vec<RoiPolygon>,
    roi_overlap_threshold: f32,
}

impl Default for PaddleOcrV6Extension {
    fn default() -> Self {
        Self::new()
    }
}

impl PaddleOcrV6Extension {
    pub fn new() -> Self {
        Self {
            engine: parking_lot::Mutex::new(OcrEngine::new()),
            config: parking_lot::RwLock::new(RuntimeConfig {
                tier: "auto".into(),
                inference_device: None,
                draw_boxes: true,
                roi_regions: vec![],
                roi_overlap_threshold: 0.5,
            }),
            request_count: Default::default(),
            success_count: Default::default(),
            failure_count: Default::default(),
            total_text_blocks: Default::default(),
            last_inference_ms: Default::default(),
        }
    }
}

#[async_trait]
impl Extension for PaddleOcrV6Extension {
    fn metadata(&self) -> &ExtensionMetadata {
        // (复制 Task 2.3 的 metadata 块，添加 config_parameters)
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        // bind_device, unbind_device, toggle_binding, get_bindings,
        // recognize_image, get_status, update_roi, switch_tier
        // 参考 v1 commands 列表，但:
        // - 移除所有 language 参数
        // - 加 switch_tier command (parameter: tier enum)
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        // 5 个继承 + 3 个新增 (model_tier, model_loaded, download_progress)
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "recognize_image" => { /* 调 engine.recognize */ }
            "switch_tier" => {
                let tier_str = args.get("tier").and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("tier required".into()))?;
                let new_tier = Tier::from_str(tier_str)?;
                let mut engine = self.engine.lock();
                // 下载不持有 Mutex 太久——这里简化为同步；如需异步见 spec §6.1
                engine.ensure_loaded(new_tier, self.config.read().inference_device.as_deref());
                Ok(serde_json::json!({
                    "tier": engine.tier.as_str(),
                    "loaded": engine.loaded,
                    "error": engine.load_error,
                }))
            }
            "get_status" => {
                let engine = self.engine.lock();
                Ok(serde_json::json!({
                    "tier": engine.tier.as_str(),
                    "loaded": engine.loaded,
                    "download_progress": engine.downloader.progress.load(Ordering::SeqCst) as f64 / 1000.0,
                    "request_count": self.request_count.load(Ordering::SeqCst),
                }))
            }
            // bind_device / unbind_device / etc 参考 v1，移除 language
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    async fn configure(&mut self, config: &Value) -> Result<()> {
        // 只更新 RuntimeConfig，不触发模型重载
        // (spec §5.1)
        let mut current = self.config.read().clone();
        if let Some(t) = config.get("tier").and_then(|v| v.as_str()) {
            Tier::from_str(t)?; // 验证合法性
            current.tier = t.to_string();
        }
        if let Some(d) = config.get("inference_device").and_then(|v| v.as_str()) {
            current.inference_device = Some(d.to_string());
        }
        if let Some(b) = config.get("draw_boxes").and_then(|v| v.as_bool()) {
            current.draw_boxes = b;
        }
        // roi_regions / threshold 同 v1
        *self.config.write() = current;
        Ok(())
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // 5 继承 + 3 新增
    }
}
```

- [ ] **Step 2: 编译**

```bash
cargo check -p paddle-ocr-v6 2>&1 | tail -20
```

- [ ] **Step 3: Commit**

```bash
git add extensions/paddle-ocr-v6/src/lib.rs
git commit -m "feat(paddle-ocr-v6): implement Extension trait with switch_tier"
```

---

## Phase 7: 集成测试 + Python 对比

### Task 7.1: 集成测试

**Files:**
- Create: `extensions/paddle-ocr-v6/tests/integration_test.rs`

- [ ] **Step 1: 写 integration_test.rs**

```rust
//! Integration test: load tiny tier models + run OCR on test.png.
//! Requires `tests/fixtures/test.png` (committed) and tiny tier models
//! in `extensions/paddle-ocr-v6/models/` (downloaded by build.sh).

use paddle_ocr_v6::PaddleOcrV6Extension;
// 或者直接测 OcrEngine pub API

#[test]
fn test_load_tiny_and_recognize() {
    // 这个测试需要真模型，跳过条件：CI 环境无模型时
    let models_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
    let det = models_dir.join("ppocr-v6-tiny-det.onnx");
    if !det.exists() {
        eprintln!("Skipping: tiny models not present at {:?}", models_dir);
        return;
    }

    // 加载 + 推理
    // 参考 Task 6.1 的 OcrEngine API
    let test_img = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/test.png");
    let img_bytes = std::fs::read(&test_img).expect("test.png must exist");

    // ... 实例化 OcrEngine, 调 ensure_loaded(Tier::Tiny, None), 调 recognize
    // 断言：
    // - 检测到 >= 2 个 text_blocks
    // - 至少一个 text 包含 "Hello" 子串
    // - 至少一个 text 包含 "你好" 子串
    // - 平均 confidence > 0.5
}
```

- [ ] **Step 2: 跑集成测试**

```bash
cargo test -p paddle-ocr-v6 --test integration_test -- --nocapture 2>&1 | tail -30
```

Expected: PASS。如果 OCR 输出和 Python ground truth 不一致：
- 检查 swap_rgb 是否真的生效（用 tracing 加日志看进入 ONNX 的 tensor 前几个像素值）
- 检查 unclip_ratio 是否生效
- 检查 normalize=false 是否生效

调试方法：在 preset.rs 临时加 `println!("{:?}", cfg)` 看 Config 字段是否对。

- [ ] **Step 3: 与 Python ground truth 对比**

把 Rust 测试的输出和 `tests/fixtures/python_ground_truth.txt` 对比：
- text region 数量一致？
- 文本内容相似度 > 80%？
- bbox 大致对应（IoU > 0.7）？

如果差异大，回到 Phase 1 检查 fork 改动是否真的生效。

- [ ] **Step 4: Commit**

```bash
git add extensions/paddle-ocr-v6/tests/integration_test.rs
git commit -m "test(paddle-ocr-v6): add integration test with python ground truth"
```

---

## Phase 8: 前端

### Task 8.1: 复制 + 改造前端组件

**Files:**
- Create: `extensions/paddle-ocr-v6/frontend/frontend.json`
- Create: `extensions/paddle-ocr-v6/frontend/src/PaddleOcrV6Card.tsx`
- Create: `extensions/paddle-ocr-v6/frontend/src/main.tsx`
- Create: `extensions/paddle-ocr-v6/frontend/package.json`
- Create: `extensions/paddle-ocr-v6/frontend/vite.config.ts`
- Create: `extensions/paddle-ocr-v6/frontend/tsconfig.json`

- [ ] **Step 1: 复制 v1 frontend 整目录**

```bash
cp -R extensions/ocr-device-inference/frontend extensions/paddle-ocr-v6/frontend
rm -rf extensions/paddle-ocr-v6/frontend/node_modules extensions/paddle-ocr-v6/frontend/dist
```

- [ ] **Step 2: 改 frontend.json**

```json
{
  "id": "paddle-ocr-v6",
  "version": "2.7.7",
  "entrypoint": "paddle-ocr-v6-components.umd.cjs",
  "components": [
    {
      "name": "PaddleOcrV6Card",
      "type": "card",
      "displayName": "PaddleOCR v6",
      "description": "Edge-native PP-OCRv6 OCR with multi-tier model support",
      "icon": "scan-text",
      "defaultSize": { "width": 360, "height": 400 },
      "minSize": { "width": 280, "height": 320 },
      "maxSize": { "width": 600, "height": 600 },
      "refreshable": true,
      "refreshInterval": 1000,
      "hasDataSource": true,
      "dataSourceAllowedTypes": ["device", "device-metric"],
      "configSchema": {
        "tier": {
          "type": "string",
          "title": "Model Tier",
          "description": "tiny=fast/light, small=balanced, medium=accurate/heavy, auto=device-aware",
          "enum": ["auto", "tiny", "small", "medium"],
          "default": "auto"
        },
        "drawBoxes": {
          "type": "boolean",
          "title": "Draw Bounding Boxes",
          "default": true
        }
      }
    }
  ]
}
```

- [ ] **Step 3: 改 package.json 的 name + entrypoint**

```bash
# 改 name: ocr-device-inference-components → paddle-ocr-v6-components
sed -i.bak 's/ocr-device-inference-components/paddle-ocr-v6-components/g' \
    extensions/paddle-ocr-v6/frontend/package.json
rm extensions/paddle-ocr-v6/frontend/package.json.bak
```

- [ ] **Step 4: 改 vite.config.ts 的 output filename**

```bash
sed -i.bak 's/ocr-device-inference-components/paddle-ocr-v6-components/g' \
    extensions/paddle-ocr-v6/frontend/vite.config.ts
rm extensions/paddle-ocr-v6/frontend/vite.config.ts.bak
```

- [ ] **Step 5: 改组件类名 + 加 tier UI**

把 `OcrDeviceCard.tsx` 重命名为 `PaddleOcrV6Card.tsx`：

```bash
cd extensions/paddle-ocr-v6/frontend/src
mv OcrDeviceCard.tsx PaddleOcrV6Card.tsx 2>/dev/null || true
ls
```

在 PaddleOcrV6Card.tsx 里：
- 改 component 名 `OcrDeviceCard` → `PaddleOcrV6Card`
- 改 css class 前缀 `ocr-device-` → `paddle-ocr-v6-`
- 移除 language 切换 UI
- 加 tier 下拉选择（绑定到 configSchema.tier）
- 加下载进度条（从 get_status 的 download_progress 读取）

具体 JSX 改动参考 v1 同名组件 + 加：

```tsx
// Tier selector
<select
  value={config?.tier ?? 'auto'}
  onChange={(e) => onConfigChange({ ...config, tier: e.target.value })}
  className="paddle-ocr-v6-tier-select"
>
  <option value="auto">Auto (device-aware)</option>
  <option value="tiny">Tiny (6 MB, fastest)</option>
  <option value="small">Small (30 MB, balanced)</option>
  <option value="medium">Medium (132 MB, accurate)</option>
</select>
```

- [ ] **Step 6: 改 main.tsx 导出名**

```bash
# 看现有 main.tsx
cat extensions/paddle-ocr-v6/frontend/src/main.tsx
# 改导出 OcrDeviceCard → PaddleOcrV6Card
```

- [ ] **Step 7: 安装依赖 + 构建**

```bash
cd extensions/paddle-ocr-v6/frontend
npm install
npm run build
ls dist/
```

Expected: `dist/paddle-ocr-v6-components.umd.cjs` 生成。

- [ ] **Step 8: Commit**

```bash
git add extensions/paddle-ocr-v6/frontend/
git commit -m "feat(paddle-ocr-v6): add PaddleOcrV6Card with tier selector"
```

---

## Phase 9: CI + Workspace 集成

### Task 9.1: CI 加 cargo test 步骤

**Files:**
- Modify: `.github/workflows/build-extension.yml`

- [ ] **Step 1: Read 现有 workflow**

```bash
cat .github/workflows/build-extension.yml
```

- [ ] **Step 2: 在 build job 后加 test step**

参考结构，加：

```yaml
      - name: Run paddle-ocr-v6 tests
        working-directory: .
        run: |
          cargo test -p paddle-ocr-v6 --lib 2>&1 | tail -20
          # 集成测试需要 models/，跳过如果 CI 不 ship 模型
          # cargo test -p paddle-ocr-v6 --test integration_test || echo "integration test skipped (no models)"
        env:
          RUST_BACKTRACE: 1
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/build-extension.yml
git commit -m "ci: add paddle-ocr-v6 unit tests"
```

### Task 9.2: README + metadata

**Files:**
- Create: `extensions/paddle-ocr-v6/README.md`

- [ ] **Step 1: 写 README**

参考 v1 README 结构，改成 v6 内容（含 tier 说明、命令清单、配置参数等）。

- [ ] **Step 2: Commit**

```bash
git add extensions/paddle-ocr-v6/README.md
git commit -m "docs(paddle-ocr-v6): add README"
```

---

## Phase 10: Build & Marketplace 注册

### Task 10.1: 构建验证

- [ ] **Step 1: 单扩展构建**

```bash
./build.sh --single paddle-ocr-v6 2>&1 | tail -30
```

Expected: 成功生成 `.nep` 包。

- [ ] **Step 2: 检查 .nep 内容**

```bash
unzip -l dist/paddle-ocr-v6-*.nep | head -20
```

Expected: 包含 `models/ppocr-v6-tiny-det.onnx`、`models/ppocr-v6-tiny-rec.onnx`、`models/ppocrv6_tiny_dict.txt`、`frontend/paddle-ocr-v6-components.umd.cjs`、`binaries/...`。

### Task 10.2: 注册到 marketplace

- [ ] **Step 1: 跑 update-versions.sh**

```bash
./scripts/update-versions.sh 2.7.7 --bump-extensions
./scripts/update-versions.sh 2.7.7 --check
```

Expected: `--check` 通过，新扩展出现在 `extensions/index.json`。

- [ ] **Step 2: 验证 index.json**

```bash
jq '.extensions[] | select(.id == "paddle-ocr-v6")' extensions/index.json
```

Expected: 完整的扩展条目，含 builds URL。

### Task 10.3: 手动验证清单（参考 spec §10.4）

- [ ] macOS Apple Silicon：`./build.sh --dev --single paddle-ocr-v6`，在 NeoMind 里测试
  - [ ] tiny tier 默认加载，OCR 能识别 "Hello" + "你好"
  - [ ] 切到 small tier，自动下载 + 重载
  - [ ] CoreML 加速生效（看日志）
- [ ] Linux CPU：tiny tier 性能 < 1s/帧
- [ ] ne101_camera 的 processingExtensionId 下拉里出现 paddle-ocr-v6

---

## Definition of Done

- [ ] 所有单元测试 PASS（tier、preset、downloader）
- [ ] 集成测试 PASS（tiny tier + test.png + python ground truth 对比一致）
- [ ] `.nep` 包含 models/ 三件套 + frontend bundle + binary
- [ ] workspace 全部扩展编译通过（v1 不受影响）
- [ ] CI workflow 跑 `cargo test -p paddle-ocr-v6`
- [ ] marketplace index.json 含新扩展
- [ ] ne101_camera 下拉可选 paddle-ocr-v6

---

## 风险应急

**如果集成测试 OCR 输出和 Python 不一致**：
1. 在 preset.rs 加 `tracing::info!("{:?}", cfg)` 看 Config 实际值
2. 在 recognize 入口加日志打印前几个像素值，对比 Python 看是否 BGR/RGB 颠倒
3. 临时关掉 `with_normalize(false)` / `with_swap_rgb` / `with_db_unclip_ratio` 一个个测，找出哪个 override 起决定作用
4. 如果 usls `Image::from(DynamicImage)` 强制 RGB 是瓶颈，考虑在 lib.rs 加载图像后手动翻通道再喂给 usls

**如果 usls builder method 不存在**（Task 1.1 / 1.3 失败）：
- 手动在 patches/usls 加 builder method（参考 Task 1.2）
- 或者直接在 lib.rs 用 `cfg.db_unclip_ratio = Some(1.4);` mutate pub 字段（aksr::Builder 字段都是 pub）

**如果 usls Device API 在 paddle-ocr-v6 上下文调用失败**：
- 参考 v1 lib.rs `auto_device()` 函数（line 36-65）完整复制

---

## 不在本计划范围

- v1 (ocr-device-inference) 的任何改动
- paddle-ocr-vl 任何改动
- PP-StructureV3 表格识别
- NRTR 解码分支（v6 rec 推理只用 CTC）
- Android/iOS 原生构建
- ocr-core 共享 crate 重构（第二阶段视痛点决定）
