# NeoMind 扩展仓库 - 快速开始指南

## V2 扩展 (ABI Version 3)

本仓库使用统一的 NeoMind Extension SDK V2，支持 ABI Version 3。

### 可用扩展

| 扩展 ID | 类型 | 描述 |
|---------|------|------|
| `weather-forecast-v2` | Native | 天气预报扩展 |
| `image-analyzer-v2` | Native | 图像分析扩展 (YOLOv8) |
| `yolo-video-v2` | Native | 视频处理扩展 (YOLOv11) |

## .nep 包格式

### 目录结构

```
extension-name-version.nep  (ZIP archive)
├── manifest.json           # 扩展元数据
├── binaries/               # 二进制文件
│   └── darwin_aarch64/    # 或 linux_amd64/, windows_amd64/
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
    "darwin_aarch64": "binaries/darwin_aarch64/libneomind_extension_weather_forecast_v2.dylib"
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

## 安装方式

### 1. 本地开发安装
```bash
./build.sh --yes
# 扩展安装到 ~/.neomind/extensions/
```

### 2. Web UI 上传
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
