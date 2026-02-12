# NeoMind 扩展

NeoMind 边缘 AI 平台的官方扩展集合。

**中文文档** | **[English Documentation](README.md)**

## 概述

此仓库包含多个扩展 NeoMind 功能的扩展。NeoMind 支持 **两种类型的扩展**：

| 类型 | 文件格式 | 描述 | 最适合 |
|------|-------------|-------------|----------|
| **原生扩展** | `.dylib` / `.so` / `.dll` | 通过 FFI 加载的平台特定动态库 | 最高性能，完整系统访问 |
| **WASM 扩展** | `.wasm` + `.json` | 在沙箱环境中运行的 WebAssembly 模块 | 跨平台分发，安全执行 |

### 为什么选择 WASM 扩展？

**原生扩展**（`.dylib`/`.so`/`.dll`）：
- **优点**：最高性能，完整系统访问，通过 C FFI 支持多种语言
- **缺点**：必须为每个平台分别编译（macOS ARM64、macOS x64、Linux、Windows）

**WASM 扩展**（`.wasm`）：
- **优点**：一次编写，到处运行；沙箱执行；小文件体积（<100KB）；多语言支持（Rust、AssemblyScript、Go 等）
- **缺点**：约 10-30% 的性能开销；系统访问受限（仅通过主机 API）

> **提示**：选择 WASM 以便于分发和跨平台兼容性。选择原生扩展以获得性能关键型扩展所需的直接系统访问。

---

## 用户指南：安装扩展

### 通过 NeoMind 扩展市场（推荐）

最简单的安装扩展方式是通过 NeoMind 内置的市场：

1. 打开 NeoMind Web UI
2. 导航到 **扩展** → **市场**
3. 浏览可用扩展
4. 点击任意扩展的 **安装** 按钮
5. 扩展将自动下载并安装

扩展获取来源：
- **索引**：https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions/main/extensions/index.json
- **元数据**：https://raw.githubusercontent.com/camthink-ai/NeoMind-Extensions/main/extensions/{id}/metadata.json
- **二进制文件**：https://github.com/camthink-ai/NeoMind-Extensions/releases

### 手动安装

#### 预编译二进制文件

从 [Releases](https://github.com/camthink-ai/NeoMind-Extensions/releases) 下载预编译二进制文件：

**原生扩展（.dylib / .so / .dll）**：
```bash
# 下载后，复制到扩展目录
mkdir -p ~/.neomind/extensions
cp ~/Downloads/libneomind_extension_weather_forecast.dylib ~/.neomind/extensions/

# 重启 NeoMind
```

**WASM 扩展（.wasm）**：
```bash
# 下载两个文件：
# - my-extension.wasm（WebAssembly 模块）
# - my-extension.json（元数据文件）

mkdir -p ~/.neomind/extensions
cp ~/Downloads/my-extension.wasm ~/.neomind/extensions/
cp ~/Downloads/my-extension.json ~/.neomind/extensions/

# 重启 NeoMind
```

#### 从源代码构建

**原生扩展**：
```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions

# 构建所有扩展
cargo build --release

# 复制到扩展目录
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_*.dylib ~/.neomind/extensions/
```

**WASM 扩展（Rust）**：
```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions/extensions/wasm-hello

# 安装 WASM 目标
rustup target add wasm32-wasi

# 构建 WASM 扩展
cargo build --release --target wasm32-wasi

# 复制两个文件到扩展目录
mkdir -p ~/.neomind/extensions
cp target/wasm32-wasi/release/wasm_hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/wasm-hello.json
```

**WASM 扩展（AssemblyScript）**：
```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions/extensions/as-hello

# 安装依赖
npm install

# 构建 WASM 扩展（约 1 秒！）
npm run build

# 复制两个文件到扩展目录
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json
```

---

## 可用扩展

### [wasm-hello](extensions/wasm-hello/) - WASM 示例（Rust）
一个用 Rust 编写的简单 WASM 扩展，展示跨平台兼容性。

| 功能 | 类型 | 描述 |
|-----------|------|-------------|
| `get_counter` | 命令 | 获取当前计数器值 |
| `increment_counter` | 命令 | 增加计数器 |
| `get_temperature` | 命令 | 获取温度读数（模拟） |
| `get_humidity` | 命令 | 获取湿度读数（模拟） |
| `hello` | 命令 | 从 WASM 打招呼 |

**指标**：counter、temperature、humidity

**安装**：
```bash
# 构建 WASM 扩展
cd extensions/wasm-hello
rustup target add wasm32-wasi
cargo build --release --target wasm32-wasi

# 安装（需要两个文件）
cp target/wasm32-wasi/release/wasm_hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/wasm-hello.json
```

### [as-hello](extensions/as-hello/) - WASM 示例（AssemblyScript/TypeScript）
一个用 AssemblyScript（类 TypeScript 语言）编写的 WASM 扩展，编译快、体积小。

| 功能 | 类型 | 描述 |
|-----------|------|-------------|
| `get_counter` | 命令 | 获取当前计数器值 |
| `increment_counter` | 命令 | 增加计数器 |
| `reset_counter` | 命令 | 重置计数器为默认值 |
| `get_temperature` | 命令 | 获取温度读数（模拟） |
| `set_temperature` | 命令 | 设置温度值（用于测试） |
| `get_humidity` | 命令 | 获取湿度读数（模拟） |
| `hello` | 命令 | 从 AssemblyScript 打招呼 |
| `get_all_metrics` | 命令 | 获取所有指标（带变化） |

**指标**：counter、temperature、humidity

**安装**：
```bash
# 构建 AssemblyScript WASM 扩展
cd extensions/as-hello
npm install
npm run build

# 安装（需要两个文件）
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json
```

**为什么选择 AssemblyScript？**
- 类 TypeScript 语法（适合 JS/TS 开发者）
- 编译非常快（~1秒 vs Rust WASM 的 ~5秒）
- 二进制文件小（~15 KB vs Rust WASM 的 ~50 KB）
- 单个 `.wasm` 文件适用于所有平台

### [template](extensions/template/) - 原生扩展模板
用于创建原生扩展的模板。

### [weather-forecast](extensions/weather-forecast/)
全球城市的天气数据和预报。

| 功能 | 类型 | 描述 |
|-----------|------|-------------|
| `query_weather` | 命令 | 获取任意城市的当前天气 |
| `refresh` | 命令 | 强制刷新缓存数据 |

**指标**：temperature_c、humidity_percent、wind_speed_kmph、cloud_cover_percent

**安装**：
```bash
# 通过市场（在 NeoMind UI 中）
# 或手动安装：
cp target/release/libneomind_extension_weather_forecast.dylib ~/.neomind/extensions/
```

---

## 仓库结构

```
NeoMind-Extensions/
├── extensions/
│   ├── index.json              # 主市场索引
│   │   # 列出所有可用扩展及其元数据 URL
│   ├── wasm-hello/             # WASM 扩展示例（Rust）
│   │   ├── Cargo.toml          # 包配置
│   │   ├── metadata.json       # 元数据（sidecar 文件）
│   │   ├── README.md           # 扩展文档
│   │   └── src/lib.rs          # 源代码（wasm32-wasi 目标）
│   ├── as-hello/               # WASM 扩展示例（AssemblyScript）
│   │   ├── package.json        # npm 依赖
│   │   ├── asconfig.json       # AssemblyScript 编译器配置
│   │   ├── metadata.json       # 扩展元数据（市场用）
│   │   ├── README.md           # 扩展文档
│   │   ├── README.zh.md        # 扩展中文文档
│   │   └── assembly/extension.ts  # 源代码
│   ├── weather-forecast/       # 原生扩展
│   │   ├── metadata.json       # 扩展元数据（市场用）
│   │   ├── Cargo.toml          # 包配置
│   │   ├── README.md           # 扩展文档
│   │   └── src/lib.rs          # 源代码
│   └── template/               # 原生扩展模板
│       ├── Cargo.toml
│       ├── README.md
│       └── src/lib.rs
├── EXTENSION_GUIDE.md          # 开发者指南
├── EXTENSION_GUIDE.zh.md       # 开发者指南（中文）
├── USER_GUIDE.md               # 用户指南
├── USER_GUIDE.zh.md            # 用户指南（中文）
├── README.md                   # 本文件（英文）
├── README.zh.md                # 主 README（中文，本文件）
├── Cargo.toml                  # 工作区配置
└── build.sh                    # 构建脚本
```

---

## 开发者指南：创建扩展

详见 [EXTENSION_GUIDE.zh.md](EXTENSION_GUIDE.zh.md) 获取完整文档。

### 快速开始

**选择扩展类型**：

| 目标 | 推荐类型 |
|------|------------------|
| 无需重新编译的跨平台 | WASM |
| 最高性能 | 原生 |
| 学习/开发 | 原生（template）或 WASM（as-hello 适合 JS/TS 开发者） |
| 生产分发 | WASM |
| 快速迭代/原型开发 | WASM（AssemblyScript - ~1秒编译） |

**原生扩展（从 template）**：
```bash
cd extensions
cp -r template my-extension
cd my-extension

# 更新 Cargo.toml 中的扩展名称
# 更新 src/lib.rs 中的实现
# 创建 metadata.json 用于市场列表

# 构建
cargo build --release
```

**WASM 扩展（从 wasm-hello - Rust）**：
```bash
cd extensions
cp -r wasm-hello my-wasm-extension
cd my-wasm-extension

# 安装 WASM 目标
rustup target add wasm32-wasi

# 更新 Cargo.toml 中的扩展名称
# 更新 src/lib.rs 中的实现
# 更新 my-wasm-extension.json 元数据

# 构建（单个二进制文件适用于所有平台！）
cargo build --release --target wasm32-wasi
```

**WASM 扩展（从 as-hello - AssemblyScript/TypeScript）**：
```bash
cd extensions
cp -r as-hello my-as-extension
cd my-as-extension

# 安装依赖
npm install

# 更新 package.json 中的扩展名称
# 更新 assembly/extension.ts 中的实现
# 更新 my-extension.json 元数据
# 如需更改输出文件名，更新 asconfig.json

# 构建（非常快 ~1秒，单个二进制适用于所有平台！）
npm run build
```

### 提交到市场

1. Fork 此仓库
2. 在 `extensions/your-extension/` 中创建扩展
3. 按照上述格式添加 metadata.json
4. 将扩展添加到 `extensions/index.json`
5. 提交 Pull Request

PR 合并后：
1. 构建多平台二进制文件
2. 创建 GitHub Release
3. 上传二进制文件到 Release
4. 使用 SHA256 校验和更新 metadata.json

---

## 发布流程

准备发布时：

```bash
# 1. 更新版本号
# - 在每个扩展的 Cargo.toml 中
# - 在每个扩展的 metadata.json 中
# - 在 extensions/index.json 中

# 2. 构建所有平台
./build.sh --all-platforms

# 3. 计算 SHA256
shasum -a 256 target/release/libneomind_extension_*

# 4. 创建 GitHub Release
gh release create v0.1.0 \
  target/release/*.dylib \
  target/release/*.so \
  target/release/*.dll

# 5. 使用校验和更新 metadata.json
# 6. 提交并推送
git add .
git commit -m "Release v0.1.0"
git push origin main
```

---

## 平台支持

| 平台 | 架构 | 原生二进制 | WASM 二进制 |
|----------|--------------|---------------|-------------|
| macOS | ARM64 (Apple Silicon) | `libneomind_extension_*.dylib` | `*.wasm`（通用） |
| macOS | x86_64 (Intel) | `libneomind_extension_*.dylib` | `*.wasm`（通用） |
| Linux | x86_64 | `libneomind_extension_*.so` | `*.wasm`（通用） |
| Linux | ARM64 | `libneomind_extension_*.so` | `*.wasm`（通用） |
| Windows | x86_64 | `neomind_extension_*.dll` | `*.wasm`（通用） |

> **注意**：WASM 扩展无需重新编译即可在所有平台上运行——同一个 `.wasm` 文件可以在任何地方运行！

---

## 许可证

MIT License - 详见 [LICENSE](LICENSE) 文件。

---

## 作者

CamThink

仓库地址：https://github.com/camthink-ai/NeoMind-Extensions
