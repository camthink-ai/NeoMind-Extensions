# AI 扩展硬件加速指南

NeoMind 的 AI 扩展（yolo-video-v2、yolo-device-inference、image-analyzer-v2、face-recognition）使用 ONNX Runtime 做推理。本文说明各平台如何启用 GPU / NPU 加速。

## 现状总览

| 平台 | 推理设备 | 开箱即用？ | 说明 |
|------|---------|-----------|------|
| **macOS**（Apple Silicon / Intel） | CoreML | ✅ 是 | 走 Apple Neural Engine / GPU，CoreML 自动路由 |
| **Linux x86_64 + NVIDIA GPU** | CUDA | ⚠️ 需装 CUDA toolkit + CUDA 版 ORT | 驱动和库就位后自动启用 |
| **NVIDIA Jetson**（Linux aarch64） | CUDA | ⚠️ **必须在 Jetson 上重新编译扩展** | 见下文，GitHub Release 的预编译包走 CPU |
| **Linux 无 GPU** | CPU | ✅ 是 | 自动 fallback |
| **Windows** | CPU | ✅（仅 CPU） | 当前不支持 Windows GPU（无 DirectML 分支） |

## 工作原理

扩展代码（`detector.rs`）按编译目标的 `target_os` 选择默认设备，运行时失败自动回退 CPU：

```rust
fn auto_device() -> Device {
    #[cfg(target_os = "macos")]       { Device::CoreMl }    // macOS → CoreML
    #[cfg(target_os = "linux")]       { Device::Cuda(0) }   // Linux → CUDA (GPU 0)
    #[cfg(not(...))]                  { Device::Cpu(0) }    // 其他 → CPU
}
```

加载流程：
1. `auto_device()` 返回目标设备
2. 尝试用该设备构建 ORT session
3. 失败（EP 不可用 / CUDA 库缺失）→ **fallback 到 CPU**，不会崩溃

**关键**：`Device::Cuda(0)` 只是「请求 CUDA」，真正决定能不能用 GPU 的是 **ORT 运行时库是否包含 CUDA Execution Provider**（`libonnxruntime_providers_cuda.so`）。官方 aarch64 Linux 的 ORT release **不含 CUDA EP**，这是 Jetson 需要特殊处理的原因。

---

## macOS（CoreML）

**开箱即用，无需任何配置。**

- `usls` 编译时启用 `coreml` feature
- CoreML framework 是 macOS 自带的
- CoreML 自动选择最快的执行单元（ANE > GPU > CPU）
- Apple Silicon 上 YOLO 通常跑在 Neural Engine 上

验证：推流时看后端日志，应有：
```
[HW] Trying device: CoreMl
[HW] Model loaded with device: CoreMl
```

---

## NVIDIA Jetson（重点）

**直接装 GitHub Release 的 `linux_arm64.nep` 会走 CPU**，因为 CI 构建时打包的是 CPU-only ORT。要在 Jetson 上启用 CUDA，必须在本机重新编译。

### Step 1：确认 JetPack / CUDA 版本

```bash
cat /etc/nv_tegra/release          # JetPack 版本
nvcc --version                     # CUDA 版本
# JetPack 5.x → CUDA 11.4
# JetPack 6.x → CUDA 12.2
```

### Step 2：安装 Jetson 专用的 CUDA 版 ORT

官方 `pip install onnxruntime` 在 aarch64 上只有 CPU 版。用 NVIDIA 提供的 Jetson CUDA wheel：

- 下载页：https://elinux.org/Jetson_Zoo#ONNX_Runtime
- 选**对应 JetPack/CUDA 版本**的 wheel（版本不匹配会加载失败）

```bash
# 例：JetPack 5（CUDA 11.4）
pip3 install onnxruntime_gpu-1.17.1-cp38-cp38-linux_aarch64.whl

# 验证 CUDA EP 在
python3 -c "import onnxruntime as ort; print([p for p in ort.get_available_providers()])"
# 输出必须包含 'CUDAExecutionProvider'

# 找到库路径
python3 -c "import onnxruntime, os; print(os.path.dirname(onnxruntime.__file__))"
# 记下这个路径，里面有 libonnxruntime.so + libonnxruntime_providers_cuda.so
```

**关键检查**：上一步目录里**必须有** `libonnxruntime_providers_cuda.so`（或带版本号的同名文件）。没有就是装错了 wheel。

### Step 3：在 Jetson 上编译扩展

```bash
# 在 Jetson 上（必须本机编译，不能交叉编译）
git clone https://github.com/camthink-ai/NeoMind-Extensions
cd NeoMind-Extensions

# 指向 Step 2 装的 CUDA 版 ORT
export ORT_LIB_PATH=/usr/lib/python3.8/site-packages/onnxruntime/capi

# 确认 CUDA 环境（Jetson 默认已配）
echo $CUDA_HOME    # 应为 /usr/local/cuda
echo $LD_LIBRARY_PATH  # 应包含 /usr/local/cuda/lib64

# 编译打包单个扩展
./build.sh --single yolo-video-v2

# 装到本机 NeoMind
./build.sh --yes
```

`build.sh` 会把 `ORT_LIB_PATH` 里的 ORT 库（含 CUDA EP）复制进 `.nep` 包。扩展运行时通过 `setup_native_lib_paths()`（detector.rs）加入搜索路径。

### Step 4：验证加速生效

启动推流，看后端日志，**关键三行**：

```
[HW] Trying device: Cuda(0)              ← 请求 CUDA
[HW] Model loaded with device: Cuda(0)   ← ✅ 成功，正在用 GPU
```

如果看到这行就是 **fallback 到 CPU 了**（排查 ORT wheel 版本 / `LD_LIBRARY_PATH`）：
```
[HW] Cuda(0) failed (...), falling back to CPU
```

还可以看 GPU 占用确认：
```bash
tegrastats   # 看 GR3D_FREQ 非 0 说明 GPU 在干活
```

### Jetson 常见坑

| 现象 | 原因 | 解决 |
|------|------|------|
| `Cuda failed, falling back to CPU` | ORT wheel 版本和 JetPack CUDA 不匹配 | 装对应 JetPack 的 wheel |
| `libonnxruntime_providers_cuda.so: cannot open` | CUDA EP 库没打包或路径不对 | 确认 `ORT_LIB_PATH` 指对，重 build |
| `libcudnn.so: cannot open` | cuDNN 缺失 | JetPack 重装时勾选 cuDNN 组件 |
| 编译时报 CUDA 链接错误 | `usls` cuda feature 找不到 CUDA toolkit | `export CUDA_HOME=/usr/local/cuda` |

---

## Linux x86_64 + NVIDIA GPU

和 Jetson 类似，需要 CUDA 版 ORT：

```bash
# x86_64 上可以直接装 CUDA 版 ORT（官方支持）
pip3 install onnxruntime-gpu

# 同样要在本机编译扩展，让 build.sh 打包 CUDA EP
export ORT_LIB_PATH=$(python3 -c "import onnxruntime, os; print(os.path.dirname(onnxruntime.__file__)+'/capi')")
./build.sh --single yolo-video-v2
```

---

## Windows

**当前不支持 GPU 加速**。`auto_device()` 对 Windows 直接返回 `Device::Cpu(0)`。

原因：`usls` 的 features 只启用了 `coreml` 和 `cuda`，没有 DirectML；Windows 上的 CUDA 也需要单独适配。如需 Windows GPU 支持，要改 `auto_device()` 加 Windows 分支 + 评估 usls 的 DirectML 支持。

---

## 故障排查通用

无论哪个平台，验证设备选择的最直接方式是看日志：

```bash
# 启动推流后，grep 这几行
grep -E "\[HW\]" <后端日志>
```

- `Model loaded with device: X` → 成功
- `X failed (...), falling back to CPU` → 该设备不可用，查错误信息

如果完全没看到 `[HW]` 日志，说明模型没加载（检查 model 路径 / session 创建）。

---

## 相关代码

- 设备选择：`extensions/yolo-video-v2/src/detector.rs` → `auto_device()` + `with_device_fallback()`
- ORT 库搜索路径：`extensions/yolo-video-v2/src/detector.rs` → `setup_native_lib_paths()`
- ORT 打包逻辑：`build.sh` L396-448（`ORT_LIB_PATH` 优先，其次 `LD_LIBRARY_PATH`）
- usls features：各扩展 `Cargo.toml` → `usls = { features = ["yolo", "ort-load-dynamic", "coreml", "cuda"] }`
