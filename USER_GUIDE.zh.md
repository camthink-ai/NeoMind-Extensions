# NeoMind 扩展 V2 - 用户指南

本指南介绍如何安装和使用 NeoMind V2 扩展。

[English Guide](USER_GUIDE.md)

---

## 目录

1. [什么是 V2 扩展？](#什么是-v2-扩展)
2. [安装扩展](#安装扩展)
3. [可用扩展](#可用扩展)
4. [使用扩展](#使用扩展)
5. [故障排除](#故障排除)

---

## 什么是 V2 扩展？

NeoMind V2 扩展使用 **统一的 Extension SDK V2** 构建，使用 **ABI 版本 3**。

### 核心特性

| 特性 | 描述 |
|-----|------|
| **统一 SDK** | Native 和 WASM 单一 SDK |
| **ABI 版本 3** | 新的扩展接口，改进安全性 |
| **前端组件** | 基于 React 的仪表板小部件 |
| **CSS 主题** | 明暗模式支持 |

### 扩展类型

| 类型 | 文件扩展名 | 性能 | 安全性 |
|-----|-----------|------|--------|
| **Native** | `.dylib` / `.so` / `.dll` | 最高 | 标准 |
| **WASM** | `.wasm` | 90-95% | 沙箱 |

---

## 安装扩展

### 方法 1：复制到扩展目录

```bash
# 构建扩展
make build

# 安装到 NeoMind
make install

# 或手动安装
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_*.dylib ~/.neomind/extensions/
```

### 方法 2：使用构建脚本

```bash
# 构建并自动安装
./build.sh --yes

# 仅构建（跳过安装）
./build.sh --skip-install

# 构建包含前端
./build.sh

# 构建不含前端
./build.sh --skip-frontend
```

### 方法 3：打包安装

```bash
# 打包特定扩展
bash scripts/package.sh -d extensions/weather-forecast-v2

# 通过 NeoMind Web UI 安装 .nep 包
# 扩展 → 添加扩展 → 文件模式 → 上传
```

---

## 可用扩展

### 天气预报 V2

**ID**: `weather-forecast-v2`

使用 Open-Meteo API 的实时天气数据。

| 命令 | 描述 |
|-----|------|
| `get_weather` | 获取任意城市的当前天气 |

| 指标 | 描述 |
|-----|------|
| `temperature_c` | 温度（摄氏度） |
| `humidity_percent` | 相对湿度 |
| `wind_speed_kmph` | 风速（公里/小时） |

**前端组件**: WeatherCard - 美观的天气显示卡片

```bash
# 构建
cargo build --release -p neomind-weather-forecast-v2
```

---

### 图像分析器 V2

**ID**: `image-analyzer-v2`

使用 YOLOv8 的 AI 图像分析。

| 命令 | 描述 |
|-----|------|
| `analyze_image` | 分析图像中的物体 |

| 指标 | 描述 |
|-----|------|
| `images_processed` | 已处理图像总数 |
| `total_detections` | 检测到的物体数 |
| `avg_processing_time_ms` | 平均处理时间 |

**前端组件**: ImageAnalyzer - 拖放上传，带检测框显示

```bash
# 构建
cargo build --release -p neomind-image-analyzer-v2
```

---

### YOLO 视频 V2

**ID**: `yolo-video-v2`

使用 YOLOv11 的实时视频流处理。

| 命令 | 描述 |
|-----|------|
| `start_stream` | 启动视频流处理 |
| `stop_stream` | 停止视频流 |
| `get_stream_stats` | 获取流统计信息 |

| 指标 | 描述 |
|-----|------|
| `active_streams` | 活跃流数量 |
| `total_frames_processed` | 已处理总帧数 |
| `avg_fps` | 平均帧率 |

**前端组件**: YoloVideoDisplay - MJPEG 流实时检测显示

```bash
# 构建
cargo build --release -p neomind-yolo-video-v2
```

---

## 使用扩展

### 通过 NeoMind Web UI

1. 打开 NeoMind Web UI（默认：`http://localhost:9375`）
2. 进入 **扩展** 页面
3. 查看已安装的扩展和状态
4. 从扩展组件添加仪表板小部件

### 通过 API

```bash
# 列出扩展
curl http://localhost:9375/api/extensions

# 执行扩展命令
curl -X POST http://localhost:9375/api/extensions/weather-forecast-v2/command \
  -H "Content-Type: application/json" \
  -d '{"command": "get_weather", "args": {"city": "北京"}}'

# 获取扩展指标
curl http://localhost:9375/api/extensions/image-analyzer-v2/metrics
```

### 通过仪表板

V2 扩展为仪表板提供 React 组件：

1. 进入 **仪表板**
2. 点击 **添加小部件**
3. 选择扩展组件（如"Weather Card"）
4. 配置并保存

---

## 故障排除

### 扩展无法加载

**症状**: 扩展显示"加载失败"

**解决方案**:
1. 检查 ABI 版本：扩展必须使用 ABI 版本 3
2. 验证二进制格式匹配平台
3. 查看 NeoMind 日志：`tail -f ~/.neomind/logs/extension.log`

### 前端组件不显示

**症状**: 仪表板小部件显示空白或错误

**解决方案**:
1. 验证扩展目录中存在前端文件
2. 检查浏览器控制台错误
3. 重新构建前端：`./build.sh`

### 性能问题

**症状**: 扩展运行缓慢

**解决方案**:
1. 对于计算密集型任务使用 Native 而非 WASM
2. 为 AI 扩展启用进程隔离
3. 检查系统资源

---

## 构建命令总结

```bash
# 构建所有扩展
make build

# 构建特定扩展
cargo build --release -p neomind-weather-forecast-v2

# 构建并安装
./build.sh --yes

# 清理构建产物
make clean

# 运行测试
make test

# 格式化代码
make fmt
```

---

## 支持

- **文档**: [EXTENSION_GUIDE.zh.md](EXTENSION_GUIDE.zh.md)
- **问题**: GitHub Issues
- **许可证**: MIT
