# NeoMind Extension CLI

快速开发、构建和打包 NeoMind 扩展的命令行工具。

## 功能特性

- 🔧 **脚手架生成** - 一键创建新扩展项目
- 🏗️ **快速构建** - 增量构建和交叉编译
- 📦 **自动打包** - 生成 .nep 包（支持多平台）
- ✅ **规范验证** - 检查扩展是否符合规范
- 🧪 **本地测试** - 在开发环境中快速测试扩展
- 📄 **文档生成** - 自动生成扩展文档

## 安装

```bash
# 从 NeoMind-Extension 仓库根目录安装
cargo install --path neomind-ext
```

## 快速开始

### 创建新扩展

```bash
# 创建基础扩展
neomind-ext new my-extension

# 创建带前端的扩展
neomind-ext new my-extension --with-frontend

# 创建特定类型的扩展
neomind-ext new sensor-temperature --type device
neomind-ext new ai-analyzer --type ai
```

### 构建扩展

```bash
# 构建当前目录的扩展
neomind-ext build

# 构建指定扩展
neomind-ext build extensions/my-extension

# 发布构建（优化）
neomind-ext build --release

# 交叉编译
neomind-ext build --target aarch64-unknown-linux-gnu
```

### 打包扩展

```bash
# 打包为 .nep 文件
neomind-ext package

# 打包并包含前端
neomind-ext package --with-frontend

# 打包指定扩展
neomind-ext package extensions/my-extension

# 打包多个平台
neomind-ext package --platforms darwin-aarch64,linux-amd64
```

### 验证扩展

```bash
# 验证扩展规范
neomind-ext validate

# 验证 .nep 包
neomind-ext validate my-extension-1.0.0.nep

# 详细验证输出
neomind-ext validate --verbose
```

### 测试扩展

```bash
# 运行扩展测试
neomind-ext test

# 运行并显示输出
neomind-ext test --verbose

# 测试特定扩展
neomind-ext test extensions/my-extension
```

### 开发辅助

```bash
# 监视文件变化并自动构建
neomind-ext watch

# 清理构建产物
neomind-ext clean

# 检查 SDK 版本兼容性
neomind-ext check-sdk

# 生成扩展文档
neomind-ext docs

# 列出所有可用命令
neomind-ext help
```

## 项目结构

```
neomind-ext/
├── src/
│   ├── main.rs              # CLI 入口
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── new.rs           # 创建新扩展
│   │   ├── build.rs         # 构建扩展
│   │   ├── package.rs       # 打包扩展
│   │   ├── validate.rs      # 验证扩展
│   │   ├── test.rs          # 测试扩展
│   │   └── watch.rs         # 监视模式
│   ├── templates/
│   │   ├── mod.rs
│   │   ├── basic.rs         # 基础模板
│   │   ├── device.rs        # 设备扩展模板
│   │   ├── ai.rs            # AI 扩展模板
│   │   └── frontend.rs      # 前端模板
│   └── utils/
│       ├── mod.rs
│       ├── cargo.rs         # Cargo 操作
│       ├── manifest.rs      # manifest.json 生成
│       └── zip.rs           # ZIP 打包
├── templates/
│   ├── extension-basic/     # 基础扩展模板
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── metadata.json
│   ├── extension-device/    # 设备扩展模板
│   ├── extension-ai/        # AI 扩展模板
│   └── extension-with-frontend/  # 带前端的扩展
└── Cargo.toml
```

## 配置文件

创建 `neomind-ext.toml` 配置文件：

```toml
[extension]
id = "my-extension"
name = "My Extension"
version = "1.0.0"
author = "Your Name"
description = "My awesome extension"

[build]
release = false
targets = ["darwin-aarch64", "linux-amd64"]

[package]
include_frontend = true
include_models = false
output_dir = "dist"

[dev]
auto_reload = true
watch_paths = ["src", "frontend/src"]
```

## 高级用法

### 批量操作

```bash
# 构建所有扩展
neomind-ext build --all

# 打包所有扩展
neomind-ext package --all

# 验证所有扩展
neomind-ext validate --all
```

### CI/CD 集成

```yaml
# .github/workflows/build-extension.yml
name: Build Extension

on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Install neomind-ext
        run: cargo install --path neomind-ext
      - name: Build extension
        run: neomind-ext build --release
      - name: Package extension
        run: neomind-ext package
      - name: Validate package
        run: neomind-ext validate dist/*.nep
```

### 自定义模板

```bash
# 创建自定义模板
neomind-ext new my-extension --template /path/to/template

# 从远程模板创建
neomind-ext new my-extension --template https://github.com/user/neomind-extension-template
```

## 与现有脚本对比

| 功能 | 现有脚本 | neomind-ext |
|------|---------|-------------|
| 创建扩展 | 手动复制目录 | `neomind-ext new` ⚡ |
| 构建扩展 | `cargo build` | `neomind-ext build` + 增量构建 |
| 打包扩展 | `scripts/package.sh` | `neomind-ext package` 自动化 |
| 验证扩展 | `scripts/test_nep.py` | `neomind-ext validate` 原生 |
| 监视模式 | ❌ 不支持 | `neomind-ext watch` ✅ |
| 多平台打包 | 手动操作 | `neomind-ext package --platforms` ⚡ |
| 文档生成 | ❌ 不支持 | `neomind-ext docs` ✅ |

## 贡献

欢迎贡献！请查看 [CONTRIBUTING.md](../../CONTRIBUTING.md) 了解详情。

## 许可证

MIT License
