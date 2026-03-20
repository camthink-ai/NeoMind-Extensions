# NeoMind 扩展开发 - Claude Code 技能

为创建 NeoMind 边缘 AI 平台扩展提供 AI 驱动的开发辅助。

[中文文档](README.zh.md) | [English Documentation](README.md)

## 📚 快速链接

| 文档 | 说明 |
|------|------|
| **[安装说明](#安装)** | 如何安装技能 |
| **[GUIDE.md](GUIDE.md)** | 完整技能文档（英文）|
| **[QUICK_START.md](QUICK_START.md)** | 实用示例和教程（英文）|
| **[neomind-extension/](neomind-extension/)** | 技能包（指令、参考、示例）|

---

## 🎯 功能说明

这个 Claude Code 技能为您提供开发 NeoMind 扩展的 AI 辅助：

### ✨ 特性

- 🤖 **自动指导**：讨论扩展开发时，Claude 会自动提供帮助
- 💻 **代码生成**：生成完整的 Rust 后端和 React 前端代码
- 📚 **按需文档**：渐进式披露架构、SDK 和模式
- 🔧 **故障排查**：AI 辅助调试和问题解决
- 💡 **最佳实践**：内置 NeoMind 模式和标准知识

### 📦 包含内容

```
skill/
├── install.sh                      # 安装脚本
├── README.md                       # 本文件（英文）
├── README.zh.md                    # 本文件（中文）
├── GUIDE.md                        # 完整技能文档
├── QUICK_START.md                  # 快速开始示例
└── neomind-extension/              # 技能包
    ├── SKILL.md                    # 主技能指令
    ├── reference/                  # 详细参考文档
    │   ├── architecture.md         # 进程隔离和 IPC
    │   ├── sdk-api.md              # 完整 SDK API
    │   └── frontend.md             # React 组件
    └── examples/                   # 工作示例
        └── simple-counter.md       # 完整计数器扩展
```

---

## 🚀 安装

### 一键安装

从仓库根目录：

```bash
cd NeoMind-Extension
./skill/install.sh
```

脚本将会：
- ✅ 复制技能到 `~/.claude/skills/neomind-extension/`
- ✅ 验证安装
- ✅ 显示使用说明

### 手动安装

```bash
# 创建技能目录
mkdir -p ~/.claude/skills

# 复制技能包
cp -r skill/neomind-extension ~/.claude/skills/

# 验证
ls ~/.claude/skills/neomind-extension/SKILL.md
```

---

## 💡 使用方法

### 方式 1：自然对话

向 Claude 自然提问：

```
"如何创建 NeoMind 扩展？"
"创建一个温度监控扩展"
"添加一个带仪表盘的前端组件"
"execute_command 和 produce_metrics 有什么区别？"
```

Claude 会自动使用技能提供指导。

### 方式 2：直接调用

使用斜杠命令：

```bash
/neomind-extension                    # 通用指导
/neomind-extension weather-sensor     # 创建特定扩展
```

### 方式 3：具体帮助

询问针对性问题：

```
"展示 Extension trait 的结构"
"如何实现异步命令？"
"生成一个 React 组件模板"
```

---

## 📖 文档

### [GUIDE.md](GUIDE.md) - 完整文档（英文）

全面的指南涵盖：
- 安装和设置
- 所有功能和能力
- 使用示例
- 架构概述
- 故障排查
- 团队入门

**从这里开始全面了解。**

### [QUICK_START.md](QUICK_START.md) - 实用示例（英文）

交互式示例展示：
- 创建扩展（带代码）
- 添加前端组件
- 调试和故障排查
- 实际工作流程
- 常见模式

**从这里开始通过示例学习。**

### [neomind-extension/](neomind-extension/) - 技能包

Claude 使用的实际技能文件：
- `SKILL.md` - 主要指令
- `reference/` - 深入文档
- `examples/` - 工作代码示例

**这是 Claude 读取的内容。**

---

## 🎓 快速示例

### 示例 1：创建扩展

**您：**
```
创建一个 CPU 温度监控扩展
```

**Claude 提供：**
- 完整的项目设置
- 使用 sysinfo crate 的 Rust 实现
- 命令和指标定义
- 构建和安装指令

### 示例 2：添加前端

**您：**
```
添加一个仪表盘组件来显示温度
```

**Claude 提供：**
- 带圆形仪表的 React 组件
- 颜色编码可视化（绿/黄/红）
- API 集成代码
- 构建配置

### 示例 3：调试问题

**您：**
```
我的扩展崩溃并出现 panic 错误
```

**Claude 提供：**
- 诊断步骤
- 常见修复（panic = "unwind"、错误处理）
- 测试命令
- 预防技巧

---

## 🏗️ 可以构建什么

使用这个技能，您可以创建：

### 传感器扩展
- 温度监控器
- 系统指标
- 环境传感器
- 自定义硬件集成

### AI 扩展
- 图像分析
- 视频处理
- 自然语言处理
- 自定义机器学习模型

### 集成扩展
- API 客户端
- 数据库连接器
- 外部服务集成
- 协议实现

### 实用扩展
- 数据处理器
- 格式转换器
- 聚合器
- 自动化工具

---

## 📊 技能内容

### 主技能 (neomind-extension/)

**SKILL.md** (12KB)
- 快速命令
- 分步工作流
- 代码模板
- 常见模式
- 安全要求

**reference/** (20KB)
- `architecture.md` - 进程隔离、IPC 协议
- `sdk-api.md` - 完整 SDK 参考
- `frontend.md` - React 组件指南

**examples/** (7KB)
- `simple-counter.md` - 完整工作扩展

### 文档 (30KB)

**GUIDE.md** (11KB)
- 完整技能文档
- 安装指南
- 使用示例
- 故障排查

**QUICK_START.md** (8KB)
- 交互式示例
- 实际工作流程
- 常见模式
- 技巧和窍门

---

## 🔄 更新

### 更新技能

拉取最新更改后：

```bash
# 重新安装
./skill/install.sh

# 或手动更新
cp -r skill/neomind-extension ~/.claude/skills/
```

更改在 Claude Code 中立即生效（无需重启）。

### 贡献改进

改进技能：

1. 编辑 `skill/neomind-extension/` 中的文件
2. 使用 `./skill/install.sh` 测试
3. 在 Claude Code 中验证
4. 提交 PR

---

## 🆘 支持

### 文档
- **完整指南**：[GUIDE.md](GUIDE.md)
- **快速开始**：[QUICK_START.md](QUICK_START.md)
- **技能包**：[neomind-extension/](neomind-extension/)

### 安装问题

**找不到技能：**
```bash
# 检查安装
ls ~/.claude/skills/neomind-extension/SKILL.md

# 重新安装
./skill/install.sh
```

**技能未激活：**
```bash
# 尝试手动调用
/neomind-extension

# 检查 YAML 前置内容
head -10 ~/.claude/skills/neomind-extension/SKILL.md
```

### 获取帮助

1. 查看 [GUIDE.md](GUIDE.md) 了解常见问题
2. 查看 [QUICK_START.md](QUICK_START.md) 了解示例
3. 询问 Claude："我在使用 neomind-extension 技能时遇到问题"

---

## 📈 优势

### 对于个人开发者
- ⚡ **更快开发**：AI 生成样板代码
- 📚 **持续学习**：按需交互式文档
- 🛠️ **更少错误**：内置最佳实践
- 🎯 **专注工作**：减少查文档时间，增加功能开发时间

### 对于团队
- 🤝 **一致代码**：共享标准和模式
- 📖 **轻松入门**：新开发者自助服务
- 🚀 **更高生产力**：减少提问和阻塞
- 🏆 **更好质量**：自动最佳实践

---

## 📋 系统要求

- **Claude Code**：支持技能的任何版本
- **NeoMind**：隔离扩展运行时
- **平台**：macOS、Linux、Windows

---

## 📄 许可证

MIT 许可证 - 与 NeoMind-Extension 仓库相同

---

## 🎉 开始使用

1. **安装**：`./skill/install.sh`
2. **阅读**：[QUICK_START.md](QUICK_START.md)
3. **构建**：让 Claude 帮您创建第一个扩展！

---

**版本**：1.0.0
**创建时间**：2026年3月3日
**维护者**：NeoMind 团队
