# NeoMind-Extensions - Extension Marketplace Repository

Official extension marketplace for NeoMind Edge AI Platform. Contains native extensions built with the NeoMind Extension SDK.

## Tech Stack
- **Backend**: Rust (native cdylib extensions)
- **Frontend**: React 18 + TypeScript + Vite (UMD bundles)
- **SDK**: neomind-extension-sdk v0.6 (crates.io)
- **ABI Version**: 3

## Development Commands

```bash
# Build all extensions (release)
cargo build --release

# Build single extension
cargo build --release -p weather-forecast-v2

# Test all extensions
cargo test

# Generate JSON files (metadata.json, index.json)
./scripts/update-versions.sh 2.4.0

# Build .nep packages (uses Cargo.toml versions)
./build.sh

# Build with specific version for filenames
./build.sh --release 2.4.0

# Dev build + auto-install to NeoMind
./build.sh --dev

# Build single extension
./build.sh --single weather-forecast-v2
```

## Scripts Reference

### Main Scripts

| Script | Purpose | Usage |
|--------|---------|-------|
| `./build.sh` | **Main build script** | All-in-one build, package, install |
| `./release.sh [VERSION]` | Release helper | Wrapper for `./build.sh --release` |
| `./scripts/update-versions.sh [VERSION]` | Generate JSON files | Must pass version arg |

### build.sh Options

```bash
./build.sh                           # Build all, create packages
./build.sh --dev                     # Dev build, auto-install to NeoMind
./build.sh --release 2.4.0           # Release with version in filenames
./build.sh --single weather-forecast-v2  # Single extension
./build.sh --skip-frontend           # Skip frontend builds
./build.sh --skip-package            # Skip .nep creation
./build.sh --debug                   # Debug build
./build.sh --help                    # Show all options
```

### Release Process

#### Version Model

本仓库有三个层级的版本号，发布时**必须全部一致**（除非有明确理由保持不同）：

| 文件 | 版本含义 | 示例 |
|------|---------|------|
| `VERSION` | 市场发布版本 | `2.7.0` |
| `extensions/index.json` → `version` | 市场发布版本 | `2.7.0` |
| `extensions/*/Cargo.toml` → `version` | 扩展自身版本（影响包文件名） | `2.7.0` |
| `extensions/*/metadata.json` → `version` | 自动从 Cargo.toml 读取 | `2.7.0` |

**常见错误**：只更新了 `VERSION` 和 `index.json`，但忘了更新各扩展的 `Cargo.toml`。这会导致：
- 包文件名是旧版本：`xxx-2.6.0-windows_amd64.nep`
- GitHub Release 标题是 v2.7.0，但里面的包都是 2.6.0
- 用户体验混乱

#### 发布步骤

```bash
VERSION=2.7.0

# Step 1: 一步到位 — 同步 Cargo.toml + VERSION + 生成 JSON
./scripts/update-versions.sh $VERSION --bump-extensions

# Step 2: 验证版本一致性（必须通过！）
./scripts/update-versions.sh $VERSION --check

# Step 3: 提交版本变更
git add . && git commit -m "chore: bump to v$VERSION"

# Step 4: 构建和打包
./build.sh --release $VERSION

# Step 5: 验证包文件名版本一致
ls dist/*.nep
# 确认所有文件名包含正确版本号，如：weather-forecast-v2-2.7.0-darwin_aarch64.nep

# Step 6: Tag 和发布
git tag v$VERSION
git push origin main --tags
gh release create v$VERSION ./dist/*.nep --title "v$VERSION"
```

#### update-versions.sh 用法

```bash
# 完整更新：同步 Cargo.toml + VERSION + 生成 JSON（推荐）
./scripts/update-versions.sh 2.7.0 --bump-extensions

# 仅生成 JSON 文件（不修改 Cargo.toml）
./scripts/update-versions.sh 2.7.0

# 仅检查版本一致性
./scripts/update-versions.sh 2.7.0 --check

# 不传版本参数时从 VERSION 文件读取
./scripts/update-versions.sh --check
```

#### 发布前检查清单

- [ ] `VERSION` 文件版本正确
- [ ] `./scripts/update-versions.sh $VERSION --check` 通过
- [ ] 所有 `Cargo.toml` 版本与市场版本一致
- [ ] `dist/*.nep` 包文件名版本一致
- [ ] `index.json` 中每个扩展的 `version` 和 `builds` URL 版本一致

### Legacy Scripts (Removed)

These scripts have been consolidated into `build.sh`:
- ~~`build-package.sh`~~ - Use `./build.sh --single <ext>`
- ~~`build-dev.sh`~~ - Use `./build.sh --dev`
- ~~`build-all-platforms.sh`~~ - Use `./build.sh`

## Project Structure

```
NeoMind-Extensions/
├── extensions/              # All extension projects
│   ├── index.json         # Marketplace index (auto-generated)
│   ├── weather-forecast-v2/
│   │   ├── Cargo.toml     # Extension metadata (version, description)
│   │   ├── src/lib.rs     # Extension implementation
│   │   ├── metadata.json  # Full metadata (auto-generated)
│   │   ├── frontend/      # Frontend components
│   │   │   ├── frontend.json    # Component definitions
│   │   │   ├── src/            # React source
│   │   │   └── dist/           # Built UMD bundle
│   │   └── models/       # ML models (optional, .onnx)
│   ├── image-analyzer-v2/
│   ├── yolo-video-v2/
│   ├── yolo-device-inference/
│   └── wasm-demo/
├── scripts/
│   ├── update-versions.sh  # Generate all JSON files
│   └── generate-json.ts    # Alternative TypeScript generator
├── release.sh              # Build .nep packages
└── Cargo.toml              # Workspace configuration
```

## Extension Anatomy

### 1. Cargo.toml (Single Source of Truth)
```toml
[package]
name = "weather-forecast-v2"
version = "2.0.0"
description = "Weather forecast extension..."
authors = ["NeoMind Team"]
license = "Apache-2.0"

[lib]
name = "neomind_extension_weather_forecast_v2"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
```

### 2. src/lib.rs (Extension Implementation)
```rust
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionCommand, ExtensionError,
    MetricDescriptor, MetricDataType, ParameterDefinition, Result,
};

pub struct MyExtension { /* ... */ }

#[async_trait]
impl Extension for MyExtension {
    fn metadata(&self) -> &ExtensionMetadata { /* ... */ }
    fn commands(&self) -> Vec<ExtensionCommand> { /* ... */ }
    fn metrics(&self) -> Vec<MetricDescriptor> { /* ... */ }
    async fn execute_command(&self, cmd: &str, args: &Value) -> Result<Value> { /* ... */ }
}

// FFI export - generates all required symbols
neomind_extension_sdk::neomind_export!(MyExtension);
```

### 3. frontend/frontend.json (Component Definitions)
```json
{
  "id": "weather-forecast-v2",
  "version": "2.0.0",
  "entrypoint": "weather-forecast-v2-components.umd.cjs",
  "components": [
    {
      "name": "WeatherCard",
      "type": "card",
      "displayName": "Weather Forecast",
      "description": "Real-time weather display",
      "icon": "cloud-sun",
      "defaultSize": { "width": 340, "height": 320 },
      "minSize": { "width": 240, "height": 260 },
      "maxSize": { "width": 480, "height": 400 },
      "refreshable": true,
      "refreshInterval": 30000,
      "hasDataSource": true,
      "dataSourceAllowedTypes": ["device"],
      "configSchema": {
        "contentType": {
          "type": "string",
          "title": "Content Type",
          "description": "Type of content",
          "enum": ["none", "text", "markdown", "html", "image-url"],
          "enumTitles": ["None", "Plain Text", "Markdown", "HTML", "Image URL"],
          "default": "none"
        },
        "textContent": {
          "type": "string",
          "title": "Text Content",
          "description": "Content for text/markdown/html mode"
        },
        "imageUrl": {
          "type": "string",
          "title": "Image URL",
          "description": "Image URL for image-url mode"
        }
      },
      "uiHints": {
        "fieldOrder": ["contentType", "textContent", "imageUrl"],
        "visibilityRules": [
          { "field": "contentType", "condition": "equals", "value": "text", "thenShow": ["textContent"] },
          { "field": "contentType", "condition": "equals", "value": "image-url", "thenShow": ["imageUrl"] }
        ]
      }
    }
  ]
}
```

#### Component Config Fields

| Field | Type | Description |
|-------|------|-------------|
| `refreshable` | boolean | Show refresh button on card |
| `refreshInterval` | number | Auto-refresh interval in ms |
| `hasDataSource` | boolean | Enable Data Source tab in config dialog |
| `dataSourceAllowedTypes` | string[] | Allowed data source types: `"device"`, `"device-metric"`, `"extension"`, `"extension-command"`, `"system"`, `"ai-metric"`, `"transform"` |
| `configSchema` | object | Form fields for config dialog |
| `uiHints` | object | UI behavior hints for config form |

#### configSchema Field Properties

Each field in `configSchema` supports:
- `type`: `"string"`, `"number"`, `"integer"`, `"boolean"`
- `title`: Display label (shown as field label)
- `description`: Help text / placeholder
- `default`: Default value
- `enum`: Array of allowed values → renders as dropdown select
- `enumTitles`: Display labels for enum values (parallel array)

#### uiHints (Conditional Field Visibility)

```json
"uiHints": {
  "fieldOrder": ["field1", "field2", "field3"],
  "visibilityRules": [
    {
      "field": "controlField",
      "condition": "equals",
      "value": "someValue",
      "thenShow": ["dependentField1", "dependentField2"]
    }
  ]
}
```

**Supported conditions:** `equals`, `not_equals`, `contains`, `empty`, `not_empty`

**Behavior:** Fields listed in `thenShow` are **hidden by default**, and only shown when the rule matches. Fields NOT in any `thenShow` rule are always visible.

#### Data Source Binding

When `hasDataSource: true`, the config dialog shows a Data Source tab. The bound data source is passed to the component as `props.dataSource`:

```typescript
export interface ExtensionComponentProps {
  dataSource?: {
    type: string
    deviceId?: string
    device_id?: string
    extensionId?: string
    command?: string
    [key: string]: any
  }
  config?: Record<string, any>
  className?: string
}
```

Use `dataSourceAllowedTypes` to control what types users can select:
- `["device"]` — only device selection (for device-targeting components)
- `["device-metric", "extension"]` — metric and extension data
- Default (unset): `["device-metric", "extension", "extension-command"]`

## JSON File Generation

**Run after changing Cargo.toml or frontend.json:**
```bash
./scripts/update-versions.sh 2.3.0
```

This generates:
- `extensions/*/metadata.json` - Extension metadata
- `extensions/index.json` - Marketplace index

### index.json Format
```json
{
  "version": "2.3.0",
  "market_version": "2.3.0",
  "extensions": [
    {
      "id": "weather-forecast-v2",
      "name": "Weather Forecast V2",
      "version": "2.0.0",
      "description": "...",
      "metadata_url": "https://raw.githubusercontent.com/.../metadata.json",
      "frontend": {
        "components": ["WeatherCard"],  // Array of strings, NOT objects!
        "entrypoint": "weather-forecast-v2-components.umd.cjs"
      },
      "builds": {
        "darwin-aarch64": { "url": "https://.../weather-forecast-v2-2.0.0-darwin_aarch64.nep" },
        "linux-x86_64": { "url": "..." }
      }
    }
  ]
}
```

## .nep Package Format

NeoMind Extension Package (ZIP):
```
extension-id-2.0.0-darwin_aarch64.nep
├── manifest.json           # Package manifest
├── binaries/
│   └── darwin_aarch64/
│       └── libneomind_extension_xxx.dylib
├── frontend/
│   └── xxx-components.umd.cjs
└── models/                 # Optional ONNX models
    └── model.onnx
```

## FFI Interface (ABI Version 3)

**Required exports (generated by `neomind_export!` macro):**
```c
// Descriptor JSON (replaces old create/destroy)
const char* _neomind_extension_descriptor_json();

// Command execution
const char* _neomind_extension_execute_command_json(const char* cmd, const char* args);

// Metrics
const char* _neomind_extension_produce_metrics_json();

// Lifecycle
int _neomind_extension_initialize(const char* config);
int _neomind_extension_shutdown();
```

**Old FFI (deprecated, causes crashes):**
- `_neomind_extension_create` - DO NOT USE
- `_neomind_extension_destroy` - DO NOT USE

## Release Process

> 详细发布步骤和版本规范见上方 [Release Process](#release-process-1) 章节。

## Important Rules

### HTTP Client in Extensions
- **Use sync client (ureq)** in extensions to avoid Tokio runtime issues
- Async clients can cause panics when loaded as dynamic libraries
- The SDK requires Tokio only for `RwLock` wrapper

### Panic Handling
- `panic = "unwind"` is REQUIRED in Cargo.toml profiles
- This enables safe extension unloading and panic recovery

### Frontend Components

> **完整设计规范：** [`EXTENSION_FRONTEND_DESIGN_GUIDE.md`](EXTENSION_FRONTEND_DESIGN_GUIDE.md) — 修改扩展前端前必须阅读。

Key rules:
- Build to UMD format (`.umd.cjs`) for browser compatibility
- React/ReactDOM are external — provided by host app, NOT bundled
- **NEVER use Tailwind CSS** — extensions don't have Tailwind. Use NeoMind CSS variables (`var(--foreground)`, `var(--card)`, etc.) for all colors
- **NEVER hardcode colors** (`#fff`, `rgb(...)`) — use CSS variables for automatic light/dark mode support
- **主按钮文本必须用 `var(--{prefix}-on-primary)`**，不能直接写 `var(--primary-foreground)` 或 `#fff` — 详见设计规范第 5.1 节
- Component names in `index.json` must be string array, not objects
- Entry point file must match `frontend.json` entrypoint
- Every component must use `forwardRef` and handle loading/error/empty states
- Use scoped CSS with extension-prefixed class names (e.g., `.weather-`, `.yolo-`)
- Use inline SVG icons, not icon libraries

### CDN Caching
- Main project uses timestamp-based cache-busting (`?t=timestamp`)
- No version sync needed between repos
- GitHub CDN may cache briefly (few minutes)

## Common Issues

### Script: jq not found
**Symptom:** `update-versions.sh` fails with "jq: command not found"
**Solution:**
```bash
brew install jq  # macOS
apt install jq   # Linux
```

### Script: frontend build fails
**Symptom:** `build.sh` shows "⚠ frontend failed"
**Solution:**
```bash
cd extensions/xxx/frontend
rm -rf node_modules package-lock.json
npm install
npm run build
```

### Script: Version in filename vs manifest
- **Package filename**: Uses `--release VERSION` param or Cargo.toml version
- **manifest.json version**: Always from Cargo.toml
- This allows consistent filenames for releases while keeping accurate extension versions

### Extension not loading (FFI error)
- Ensure extension uses SDK v0.6+ with ABI version 3
- Old extensions using `_neomind_extension_create` will crash
- Solution: Delete old extension, reinstall from marketplace

### Frontend components missing
- Ensure `frontend/dist/` exists with UMD bundle
- Check `frontend.json` entrypoint matches built file
- Verify `index.json` has `frontend.components` as string array

### Marketplace parse_error
- Check `index.json` syntax with `jq . extensions/index.json`
- Ensure `components` is `["WeatherCard"]` not `[{...}]`

## Related Projects

- **NeoMind Main**: `../NeoMind` - Core platform
- **SDK Source**: `../NeoMind/crates/neomind-extension-sdk`
- **Extension Runner**: `../NeoMind/crates/neomind-extension-runner`

## Documentation

- **`EXTENSION_FRONTEND_DESIGN_GUIDE.md`** - Extension 前端设计规范（CSS 变量、组件模板、暗色模式、fallback 模式）
- `EXTENSION_GUIDE.md` - Detailed extension development guide
- `EXTENSION_GUIDE.zh.md` - Chinese version
- `QUICKSTART.md` - Quick start guide
- `DEPLOYMENT.md` - Deployment documentation
- **NeoMind Design Spec**: `../NeoMind/web/DESIGN_SPEC.md` - Main platform design system
