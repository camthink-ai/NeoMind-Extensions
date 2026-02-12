# AssemblyScript 扩展示例

一个使用 [AssemblyScript](https://www.assemblyscript.org/) 编写的 NeoMind WASM 扩展——AssemblyScript 是一种类似 TypeScript 的语言，可以编译为 WebAssembly。

**中文文档** | **[English Documentation](README.md)**

## 为什么选择 AssemblyScript？

| 特性 | AssemblyScript | Rust WASM | 原生 Rust |
|------|-----------------|-----------|-------------|
| **语法** | 类 TypeScript | Rust | Rust |
| **学习曲线** | 低（适合 JS/TS 开发者） | 高 | 高 |
| **编译时间** | 非常快（~1秒） | 快（~5秒） | 快（~10秒） |
| **二进制大小** | ~15 KB | ~50 KB | ~100 KB |
| **性能** | 高（原生性能的 90-95%） | 高（原生性能的 85-90%） | 100% |
| **跨平台** | ✅ 单个 .wasm 文件 | ✅ 单个 .wasm 文件 | ❌ 平台特定 |

## 功能特性

- **指标（Metrics）**：计数器（counter）、温度（temperature）、湿度（humidity）
- **命令（Commands）**：get_counter、increment_counter、reset_counter、get_temperature、set_temperature、get_humidity、hello、get_all_metrics
- **跨平台**：单个 `.wasm` 文件可在所有平台运行
- **小体积**：编译后的二进制文件约 15 KB
- **类型安全**：AssemblyScript 的严格 TypeScript 子集

## 前置要求

```bash
# 安装 Node.js（如果尚未安装）
# macOS:
brew install node

# 安装 npm 依赖
cd ~/NeoMind-Extension/extensions/as-hello
npm install
```

## 构建

```bash
cd ~/NeoMind-Extension/extensions/as-hello

# 安装依赖（仅首次需要）
npm install

# 构建扩展
npm run build

# 输出：build/as-hello.wasm
```

## 安装

```bash
# 复制两个文件到 NeoMind 扩展目录
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json

# 重启 NeoMind 或触发扩展发现
```

## 使用方法

### 通过 NeoMind API

```bash
# 列出扩展
curl http://localhost:9375/api/extensions

# 执行命令
curl -X POST http://localhost:9375/api/extensions/as-hello/command/hello \
  -H "Content-Type: application/json" \
  -d '{}'

# 获取计数器
curl -X POST http://localhost:9375/api/extensions/as-hello/command/get_counter \
  -H "Content-Type: application/json" \
  -d '{}'

# 增加计数器
curl -X POST http://localhost:9375/api/extensions/as-hello/command/increment_counter \
  -H "Content-Type: application/json" \
  -d '{}'
```

### 可用命令

| 命令 | 描述 | 示例 |
|---------|-------------|---------|
| `get_counter` | 获取当前计数器值 | `{}` |
| `increment_counter` | 计数器加 1 | `{}` |
| `reset_counter` | 重置计数器为 42 | `{}` |
| `get_temperature` | 获取温度读数 | `{}` |
| `set_temperature` | 设置新温度值 | `{"temperature": 25.5}` |
| `get_humidity` | 获取湿度读数 | `{}` |
| `hello` | 从 AS 扩展打招呼 | `{}` |
| `get_all_metrics` | 获取所有指标（带变化） | `{}` |

## 指标

| 指标 | 类型 | 单位 | 范围 | 描述 |
|--------|------|------|-------|-------------|
| `counter` | 整数 | count | 0-1000+ | 简单计数器 |
| `temperature` | 浮点数 | °C | -20 到 50 | 模拟温度 |
| `humidity` | 浮点数 | % | 0-100 | 模拟湿度 |

## 开发

### 项目结构

```
as-hello/
├── package.json          # npm 依赖
├── asconfig.json          # AssemblyScript 编译器配置
├── metadata.json          # 扩展元数据（NeoMind 使用）
├── README.md              # 英文文档
├── README.zh.md           # 中文文档（本文件）
├── assembly/
│   └── extension.ts      # 主扩展实现
├── build/                 # 编译输出（自动生成）
│   └── as-hello.wasm     # 编译的 WASM 模块
└── tests/
    └── test.js           # Node.js 测试
```

### 核心实现要点

**内存布局**：AssemblyScript 使用线性内存作为 WASM。字符串以指针形式传递，以 null 结尾。

**命令模式**：主入口点是 `neomind_execute()`，接收：
- `command_ptr`：指向命令字符串的指针
- `args_ptr`：指向参数 JSON 字符串的指针
- `result_buf_ptr`：指向结果缓冲区的指针
- `result_buf_len`：结果缓冲区的最大长度

**类型安全**：AssemblyScript 在编译到高效 WASM 代码的同时提供编译时类型检查。

## AssemblyScript 基础

### 类型定义

```typescript
// 基本类型
let counter: i32 = 42;
let temperature: f64 = 23.5;
let name: string = "as-hello";

// 数组
let values: f64[] = [1.0, 2.0, 3.0];

// 常量
const ABI_VERSION: u32 = 2;
```

### 导出函数（WASM）

```typescript
// 导出函数供 WASM 主机调用
export function get_counter(): i32 {
  return counter;
}
```

### 内存操作

```typescript
// 从内存读取
let byte = load<u8>(ptr);

// 向内存写入
store<u8>(ptr, 42);

// 复制内存
memory.copy(dest, source, length);
```

## 故障排除

### 构建错误

**"asc: command not found"（asc 命令未找到）**
```bash
npm install
```

**"Module not found"（模块未找到）**
```bash
# 确保你在 as-hello 目录中
cd ~/NeoMind-Extension/extensions/as-hello
```

### 运行时错误

**"Extension not loading"（扩展未加载）**
- 确保两个文件（`.wasm` 和 `.json`）都在扩展目录中
- 检查 JSON 文件名与 WASM 文件名匹配

**"Command not found"（命令未找到）**
- 检查命令名称拼写是否正确
- 验证命令是否在元数据中列出

## 与其他扩展的对比

| 方面 | as-hello (AS) | wasm-hello (Rust) | template (原生) |
|--------|----------------|-------------------|------------------|
| **语言** | 类 TypeScript | Rust | Rust |
| **构建时间** | ~1s | ~5s | ~10s |
| **二进制大小** | ~15 KB | ~50 KB | ~100 KB |
| **类型安全** | 编译时 | 编译时 | 编译时 |
| **调试难度** | 中等 | 困难 | 容易 |
| **JS 开发者** | 容易 | 困难 | 困难 |

## 资源链接

- [AssemblyScript 官网](https://www.assemblyscript.org/)
- [AssemblyScript GitHub](https://github.com/AssemblyScript/assemblyscript)
- [AssemblyScript 文档](https://www.assemblyscript.org/introduction.html)
- [WebAssembly 规范](https://webassembly.github.io/spec/)

## 许可证

MIT License - 详见 [NeoMind-Extensions LICENSE](../../../LICENSE)
