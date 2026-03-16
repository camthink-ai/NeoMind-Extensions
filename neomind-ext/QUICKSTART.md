# NeoMind Extension CLI - 快速开始

## 安装

```bash
cd /Users/shenmingming/NeoMindProject/NeoMind-Extension
cargo install --path neomind-ext
```

## 使用示例

### 1. 创建新扩展

```bash
# 创建基础扩展
neomind-ext new my-cool-extension

# 创建带前端的扩展
neomind-ext new my-cool-extension --with-frontend

# 创建 AI 类型扩展
neomind-ext new image-analyzer --type ai
```

### 2. 构建扩展

```bash
cd my-cool-extension
neomind-ext build

# 发布构建
neomind-ext build --release
```

### 3. 打包扩展

```bash
# 打包成 .nep 文件
neomind-ext package

# 包含前端
neomind-ext package --with-frontend
```

### 4. 验证扩展

```bash
# 验证当前扩展
neomind-ext validate

# 验证 .nep 包
neomind-ext validate dist/my-cool-extension-1.0.0.nep
```

### 5. 测试扩展

```bash
# 运行测试
neomind-ext test

# 详细输出
neomind-ext test --verbose
```

## 与现有工作流对比

### 传统方式

```bash
# 1. 手动复制目录
cp -r extensions/weather-forecast-v2 extensions/my-extension
cd extensions/my-extension

# 2. 手动编辑多个文件
vim Cargo.toml
vim src/lib.rs
vim metadata.json

# 3. 构建
cargo build --release

# 4. 手动打包
cd ../..
bash scripts/package.sh -d extensions/my-extension
```

### 使用 neomind-ext

```bash
# 1. 一条命令创建
neomind-ext new my-extension --with-frontend

# 2. 自动构建
cd my-extension
neomind-ext build --release

# 3. 自动打包
neomind-ext package --with-frontend
```

**节省时间：70%+**

## 核心优势

1. **快速启动** - 3 分钟从零到运行
2. **自动化** - 减少手动操作和错误
3. **一致性** - 统一的项目结构和规范
4. **内置最佳实践** - 模板包含安全配置和代码规范
5. **开发友好** - 监视模式、自动重建、快速测试

## 开发工作流

```bash
# 创建项目
neomind-ext new my-extension --with-frontend
cd my-extension

# 开发（自动重建）
neomind-ext watch

# 在另一个终端测试
neomind-ext test --verbose

# 准备发布
neomind-ext build --release
neomind-ext package --with-frontend
neomind-ext validate dist/*.nep

# 安装到 NeoMind
neomind extension install dist/*.nep
```

## 高级用法

### 批量操作

```bash
# 在 NeoMind-Extension 根目录
neomind-ext build --all
neomind-ext package --all
neomind-ext validate --all
```

### 交叉编译

```bash
# 为 Linux 构建（从 macOS）
neomind-ext build --target x86_64-unknown-linux-gnu --release

# 打包多平台
neomind-ext package --platforms darwin-aarch64,linux-amd64
```

## 下一步

- 查看 [README.md](./README.md) 了解所有命令
- 查看 [templates/](./templates/) 了解可用的模板
- 查看现有扩展作为参考：`extensions/` 目录
