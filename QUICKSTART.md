# NeoMind 扩展仓库 - 快速开始指南

## V2 扩展 (ABI Version 3)

本仓库使用统一的 NeoMind Extension SDK V2，支持 ABI Version 3。

### 可用扩展

| 扩展 ID | 类型 | 描述 |
|---------|------|------|
| `weather-forecast-v2` | Native | 天气预报扩展 |
| `image-analyzer-v2` | Native | 图像分析扩展 (YOLOv8) |
| `yolo-video-v2` | Native | 视频处理扩展 (YOLOv11) |
| `yolo-device-inference` | Native | 设备推理扩展 (YOLOv8) |
| `wasm-demo` | WASM | WebAssembly 演示扩展 |

## .nep 包格式

### 目录结构

```
extension-name-version.nep  (ZIP archive)
├── manifest.json           # 扩展元数据
├── binaries/               # 二进制文件
│   └── darwin_aarch64/    # 或 linux_amd64/, windows_amd64/, windows_x86/ 等
│       └── libneomind_extension_*.dylib
└── frontend/              # 可选：前端组件
    └── *-components.umd.cjs
```

### manifest.json 字段

```json
{
  "format": "neomind-extension-package",
  "format_version": "2.0",
  "abi_version": 3,
  "id": "weather-forecast-v2",
  "name": "Weather Forecast",
  "version": "2.0.0",
  "sdk_version": "2.0.0",
  "type": "native",
  "binaries": {
    "darwin_aarch64": "binaries/darwin_aarch64/libneomind_extension_weather_forecast_v2.dylib",
    "darwin_x86_64": "binaries/darwin_x86_64/libneomind_extension_weather_forecast_v2.dylib",
    "linux_amd64": "binaries/linux_amd64/libneomind_extension_weather_forecast_v2.so",
    "linux_arm64": "binaries/linux_arm64/libneomind_extension_weather_forecast_v2.so",
    "windows_amd64": "binaries/windows_amd64/libneomind_extension_weather_forecast_v2.dll",
    "windows_x86": "binaries/windows_x86/libneomind_extension_weather_forecast_v2.dll"
  },
  "frontend": "frontend/",
  "permissions": [],
  "config_parameters": [],
  "metrics": [],
  "commands": []
}
```

## 构建命令

### 构建所有扩展
```bash
./build.sh --yes
```

### 构建单个扩展
```bash
bash scripts/package.sh -d extensions/weather-forecast-v2
bash scripts/package.sh -d extensions/image-analyzer-v2
bash scripts/package.sh -d extensions/yolo-video-v2
bash scripts/package.sh -d extensions/yolo-device-inference
```

### 其他命令
```bash
./build.sh --help         # 查看帮助
./build.sh --skip-install # 仅构建，不安装
./build.sh --debug        # Debug 模式构建
make list                 # 列出所有扩展
make clean                # 清理构建产物
```

### 测试 .nep 包
```bash
python3 scripts/test_nep.py dist/weather-forecast-v2-2.0.0.nep
```


## macOS dylib 依赖修复

在 macOS 上，如果扩展的 dylib 文件有自引用依赖（通过 `otool -L` 可以看到），需要使用 `@executable_path` 或 `@rpath` 来修复：

### 问题诊断

```bash
# 检查 dylib 依赖
otool -L target/release/libneomind_extension_my_extension.dylib
```

如果输出中包含绝对路径或相对路径的自引用（如 `libneomind_extension_my_extension.dylib`），则需要修复。

### 修复方法

```bash
# 使用 install_name_tool 修复自引用
install_name_tool -id /usr/local/lib/neomind/extensions/libneomind_extension_my_extension.dylib \
  target/release/libneomind_extension_my_extension.dylib
```

或者在构建脚本中自动处理：

```bash
# scripts/fix_dylib.sh
#!/bin/bash

DYLIB_PATH="$1"
DYLIB_NAME=$(basename "$DYLIB_PATH")

# 修复自引用为绝对路径
install_name_tool -id /usr/local/lib/neomind/extensions/$DYLIB_NAME "$DYLIB_PATH"

echo "Fixed dylib: $DYLIB_PATH"
```

### 验证修复

```bash
# 再次检查依赖
otool -L target/release/libneomind_extension_my_extension.dylib
```

现在自引用应该显示为 `/usr/local/lib/neomind/extensions/libneomind_extension_my_extension.dylib`。

---

## 安装方式

### 1. 使用 NeoMind CLI（推荐）

```bash
# 检查系统健康
neomind health

# 查看扩展日志
neomind logs --extension weather-forecast-v2

# 列出已安装扩展
neomind extension list

# 安装 .nep 包
neomind extension install weather-forecast-v2-2.0.0.nep

# 卸载扩展
neomind extension uninstall weather-forecast-v2

# 验证包
neomind extension validate weather-forecast-v2-2.0.0.nep
```

### 2. 本地开发安装
```bash
./build.sh --yes
# 扩展安装到 ~/.neomind/extensions/
```

### 3. Web UI 上传
1. 下载 `.nep` 文件
2. NeoMind Web UI → 扩展 → 添加扩展 → 文件模式
3. 拖放文件并上传

### 3. API 上传
```bash
curl -X POST http://localhost:9375/api/extensions/upload/file \
  -H "Content-Type: application/octet-stream" \
  --data-binary @extension-name.nep
```

## 文件过滤

以下文件已从 Git 中排除（`.gitignore`）：
- `dist/` - 构建的 .nep 包
- `*.dylib`, `*.so`, `*.dll`, `*.wasm` - 二进制文件
- `target/` - Rust 构建目录
- `node_modules/` - Node.js 依赖
- `models/` - 模型文件（太大）

## CI/CD

GitHub Actions 自动构建：
- 推送到 `main` 分支触发构建
- 手动触发可构建指定扩展
- 成功后创建 Release 并上传 .nep 包

## 依赖关系

扩展仓库依赖 NeoMind 主项目的 SDK：

```
NeoMind-Extension/
├── extensions/
│   └── */Cargo.toml → neomind-extension-sdk = { path = "../../../NeoMind/crates/neomind-extension-sdk" }
└── Cargo.toml (workspace)
```

确保 NeoMind 主项目位于 `../NeoMind/` 目录。

## ⚠️ 内存限制警告

**大型扩展（如 YOLO 扩展）在服务启动时不会自动加载**，以防止系统 OOM（Out of Memory）。

### 受影响的扩展

- `yolo-device-inference` - 38MB+ 二进制 + 200MB+ 模型文件
- `yolo-video-v2` - 视频处理扩展

### 手动加载方法

对于这些大型扩展，请在服务启动后通过以下方式手动加载：

```bash
# 方法 1: 使用 CLI
neomind extension install yolo-device-inference-1.0.0.nep

# 方法 2: 使用 Web UI
# 访问 http://localhost:9375 → 扩展 → 添加扩展 → 选择 .nep 文件

# 方法 3: 使用 API
curl -X POST http://localhost:9375/api/extensions/upload/file \
  -H "Content-Type: application/octet-stream" \
  --data-binary @yolo-device-inference-1.0.0.nep
```

### 为什么需要手动加载？

YOLO 扩展包含：
- 大型二进制文件（38MB+）
- 深度学习模型（200MB+）
- 如果在服务启动时自动加载，会导致系统内存不足

解决方案是延迟加载：仅在需要时通过用户显式操作加载。
