# NeoMind 扩展 - 用户指南

本指南解释如何查找、安装和使用 NeoMind 扩展。

[中文指南](USER_GUIDE.zh.md) | [English Documentation](USER_GUIDE.md)

---

## 目录

1. [什么是扩展？](#什么是扩展)
2. [查找扩展](#查找扩展)
3. [安装扩展](#安装扩展)
4. [使用扩展](#使用扩展)
5. [管理扩展](#管理扩展)
6. [故障排除](#故障排除)

---

## 什么是扩展？

NeoMind 扩展是扩展平台功能的插件。扩展有 **两种类型**：

| 类型 | 格式 | 描述 |
|------|--------|-------------|
| **原生扩展** | `.dylib` / `.so` / `.dll` | 平台特定，最高性能 |
| **WASM 扩展** | `.wasm` + `.json` | 跨平台，所有平台的单个二进制文件 |

每个扩展可以提供：

| 类型 | 描述 | 示例 |
|------|-------------|---------|
| **指标** | 随时间产生值的数据流 | 温度、湿度、股价 |
| **命令** | 可以执行的操作 | 查询天气、发送通知 |
| **工具** | AI 智能体可以调用的函数 | 获取数据、执行计算 |

---

## 查找扩展

### 通过 NeoMind Web UI

1. 打开 NeoMind Web UI（通常是 `http://localhost:9375`）
2. 导航到 **扩展** → **市场**
3. 按类别浏览可用扩展：
   - 天气
   - 数据
   - 自动化
   - 集成
   - 设备

### 通过 GitHub 仓库

访问 [https://github.com/camthink-ai/NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) 查看所有可用扩展。

---

## 安装扩展

### 方法 1：市场（推荐）

1. 在 NeoMind Web UI 中，转到 **扩展** → **市场**
2. 找到您想要的扩展
3. 点击 **安装**
4. 等待下载和安装完成
5. 扩展将出现在 **我的扩展** 中

### 方法 2：手动安装

#### 步骤 1：下载扩展

从 [Releases](https://github.com/camthink-ai/NeoMind-Extensions/releases) 下载适合您平台的文件：

**原生扩展：**
| 平台 | 文件扩展名 |
|----------|----------------|
| macOS | `.dylib` |
| Linux | `.so` |
| Windows | `.dll` |

**WASM 扩展：**
| 文件 | 描述 |
|------|-------------|
| `.wasm` | WebAssembly 模块（适用于所有平台） |
| `.json` | 元数据 sidecar 文件 |

#### 步骤 2：安装扩展

**原生扩展：**
```bash
# 如果不存在则创建扩展目录
mkdir -p ~/.neomind/extensions

# 复制下载的扩展
cp ~/Downloads/libneomind_extension_*.dylib ~/.neomind/extensions/
```

**WASM 扩展：**
```bash
# 如果不存在则创建扩展目录
mkdir -p ~/.neomind/extensions

# 复制两个文件（必需！）
cp ~/Downloads/my-extension.wasm ~/.neomind/extensions/
cp ~/Downloads/my-extension.json ~/.neomind/extensions/
```

#### 步骤 3：验证安装

```bash
# 通过 API 列出已安装的扩展
curl http://localhost:9375/api/extensions

# 或在 NeoMind Web UI 中的 扩展 → 我的扩展 查看
```

### 方法 3：从源代码构建

**原生扩展：**
```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions

# 构建扩展
cargo build --release -p neomind-weather-forecast

# 安装
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_weather_forecast.dylib ~/.neomind/extensions/
```

**WASM 扩展（Rust）：**
```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions/extensions/wasm-hello

# 安装 WASM 目标（一次性设置）
rustup target add wasm32-wasi

# 构建 WASM 扩展
cargo build --release --target wasm32-wasi

# 安装（需要两个文件！）
mkdir -p ~/.neomind/extensions
cp target/wasm32-wasi/release/wasm_hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/wasm-hello.json
```

**WASM 扩展（AssemblyScript）：**
```bash
# 克隆仓库
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions/extensions/as-hello

# 安装依赖（一次性设置）
npm install

# 构建 WASM 扩展（约 1 秒！）
npm run build

# 安装（需要两个文件！）
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json
```

---

## 使用扩展

### 通过 AI 智能体

扩展自动将其命令注册为 AI 智能体可以使用的工具：

```
用户：东京的天气怎么样？
智能体：[调用 query_weather 工具] 东京目前：18°C，晴，湿度：45%
```

### 通过 API

```bash
# 执行命令
curl -X POST http://localhost:9375/api/extensions/weather-forecast/command \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{
    "command": "query_weather",
    "args": {"city": "北京"}
  }'
```

### 通过 Web UI

1. 转到 **扩展** → **我的扩展**
2. 点击扩展
3. 使用 **执行命令** 按钮
4. 输入参数并点击 **运行**

---

## 管理扩展

### 查看已安装的扩展

```bash
# 通过 API
curl http://localhost:9375/api/extensions

# 通过 Web UI
# 扩展 → 我的扩展
```

### 检查扩展健康状态

```bash
# 通过 API
curl http://localhost:9375/api/extensions/weather-forecast/health

# 响应：{"healthy": true, "message": "Extension is running"}
```

### 查看扩展指标

```bash
# 从扩展获取当前指标
curl http://localhost:9375/api/extensions/weather-forecast/metrics
```

### 卸载扩展

```bash
# 删除扩展文件
rm ~/.neomind/extensions/libneomind_extension_weather_forecast.dylib

# 或通过 Web UI
# 扩展 → 我的扩展 → 点击扩展 → 卸载
```

---

## 扩展配置

某些扩展接受配置：

```bash
# 通过 API 配置扩展
curl -X PUT http://localhost:9375/api/extensions/weather-forecast/config \
  -H "Content-Type: application/json" \
  -d '{
    "default_city": "上海"
  }'
```

---

## 故障排除

### 扩展未加载

**问题**：扩展未出现在列表中

**解决方案**：
1. 检查文件是否在 `~/.neomind/extensions/` 中
2. 验证文件扩展名与您的平台匹配（`.dylib`、`.so`、`.dll`）
3. 检查 NeoMind 服务器日志：`journalctl -u neomind -f`
4. 验证 ABI 版本兼容性（需要 NeoMind 0.5.8+）

### 扩展显示错误状态

**问题**：扩展处于"错误"状态

**解决方案**：
1. 检查健康端点：`curl /api/extensions/{id}/health`
2. 在 NeoMind 服务器日志中查看扩展日志
3. 尝试重启 NeoMind
4. 检查是否满足扩展依赖项（网络、API 密钥等）

### 命令未找到

**问题**：AI 智能体找不到扩展命令

**解决方案**：
1. 验证扩展已加载：`curl /api/extensions`
2. 检查扩展的命令：`curl /api/extensions/{id}`
3. 重启 AI 智能体会话

### 权限被拒绝

**问题**：无法将扩展复制到 `~/.neomind/extensions/`

**解决方案**：
```bash
# 修复权限
sudo chown -R $USER:$USER ~/.neomind/

# 或先创建目录
mkdir -p ~/.neomind/extensions
```

### 架构不匹配

**问题**：扩展加载失败，显示架构错误

**解决方案**：
1. 检查您的系统架构：`uname -m`
2. 下载正确的二进制文件：
   - Apple Silicon Mac: `darwin-aarch64`
   - Intel Mac: `darwin-x86_64`
   - Linux PC: `linux-x86_64`
   - Windows PC: `windows-x86_64`

### WASM 扩展未加载

**问题**：WASM 扩展未出现在列表中

**解决方案**：
1. 确保 **两个** 文件 `.wasm` 和 `.json` 都存在
2. 验证 JSON 文件与 WASM 文件具有相同的基本文件名
3. 检查 JSON 是否有效（使用 `jq` 或 JSON 验证器）
4. 验证 `data_type` 值：`integer`、`float`、`string` 或 `boolean`

### WASM 构建失败

**问题**：`cargo build --target wasm32-wasi` 失败

**解决方案**：
1. 安装 WASM 目标：`rustup target add wasm32-wasi`
2. 某些 crate 不支持 WASM - 检查依赖项
3. 对 WASM 扩展使用最小依赖项

---

## 安全注意事项

### 已验证的扩展

仅从以下来源安装扩展：
- 官方 [NeoMind-Extensions 仓库](https://github.com/camthink-ai/NeoMind-Extensions)
- 受信任的来源

### 扩展权限

扩展在以下安全限制下运行：
- **熔断器**：5 次连续失败 → 禁用
- **超时**：每个命令 30 秒（可配置）
- **内存**：限制为 100MB（可配置）
- **Panic 隔离**：崩溃不会导致 NeoMind 崩溃

### 审查扩展代码

所有扩展源代码公开可用：
```
https://github.com/camthink-ai/NeoMind-Extensions/tree/main/extensions/{extension-name}/src/lib.rs
```

---

## 获取帮助

- **文档**：开发者请参阅 [EXTENSION_GUIDE.zh.md](EXTENSION_GUIDE.zh.md)
- **问题**：[GitHub Issues](https://github.com/camthink-ai/NeoMind-Extensions/issues)
- **社区**：[Discussions](https://github.com/camthink-ai/NeoMind-Extensions/discussions)

---

## 可用扩展

### WASM Hello（Rust）

- **ID**：`wasm-hello`
- **类型**：WASM（跨平台）
- **语言**：Rust
- **描述**：用 Rust 编写的简单 WASM 扩展，展示跨平台兼容性
- **命令**：`get_counter`、`increment_counter`、`get_temperature`、`get_humidity`、`hello`
- **指标**：`counter`、`temperature`、`humidity`

此扩展可在 **所有平台** 上运行，无需重新编译！

### WASM Hello（AssemblyScript）

- **ID**：`as-hello`
- **类型**：WASM（跨平台）
- **语言**：AssemblyScript（类 TypeScript）
- **描述**：用 AssemblyScript 编写的 WASM 扩展，编译快（~1s）且二进制文件小（~15 KB）
- **命令**：`get_counter`、`increment_counter`、`reset_counter`、`get_temperature`、`set_temperature`、`get_humidity`、`hello`、`get_all_metrics`
- **指标**：`counter`、`temperature`、`humidity`

非常适合希望创建 WASM 扩展的 JavaScript/TypeScript 开发者！

### 天气预报

- **ID**：`weather-forecast`
- **描述**：全球天气数据和预报
- **命令**：`query_weather`、`refresh`
- **指标**：`temperature_c`、`humidity_percent`、`wind_speed_kmph`、`cloud_cover_percent`

### 更多扩展即将推出

我们正在积极开发更多扩展。请查看仓库获取更新！
