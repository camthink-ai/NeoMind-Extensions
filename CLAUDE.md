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

```bash
# 1. Update JSON files
./scripts/update-versions.sh 2.4.0

# 2. Commit version bump
git add . && git commit -m "chore: bump to v2.4.0"

# 3. Build and package
./release.sh 2.4.0
# or: ./build.sh --release 2.4.0

# 4. Verify packages
ls -la dist/*.nep

# 5. Tag and release
git tag v2.4.0
git push origin main --tags
gh release create v2.4.0 ./dist/*.nep --title "v2.4.0"
```

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
      "configSchema": {
        "defaultCity": {
          "type": "string",
          "default": "Beijing",
          "description": "Default city"
        }
      }
    }
  ]
}
```

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

1. **Update versions:**
   ```bash
   ./scripts/update-versions.sh 2.4.0
   git add . && git commit -m "chore: bump to v2.4.0"
   ```

2. **Build packages:**
   ```bash
   ./release.sh  # or ./build-all-platforms.sh
   ```

3. **Create GitHub release:**
   ```bash
   git tag v2.4.0
   git push origin main --tags
   gh release create v2.4.0 ./dist/*.nep --title "v2.4.0"
   ```

## Important Rules

### HTTP Client in Extensions
- **Use sync client (ureq)** in extensions to avoid Tokio runtime issues
- Async clients can cause panics when loaded as dynamic libraries
- The SDK requires Tokio only for `RwLock` wrapper

### Panic Handling
- `panic = "unwind"` is REQUIRED in Cargo.toml profiles
- This enables safe extension unloading and panic recovery

### Frontend Components
- Build to UMD format (`.umd.cjs`) for browser compatibility
- Component names in `index.json` must be string array, not objects
- Entry point file must match `frontend.json` entrypoint

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

- `EXTENSION_GUIDE.md` - Detailed extension development guide
- `EXTENSION_GUIDE.zh.md` - Chinese version
- `QUICKSTART.md` - Quick start guide
- `DEPLOYMENT.md` - Deployment documentation
