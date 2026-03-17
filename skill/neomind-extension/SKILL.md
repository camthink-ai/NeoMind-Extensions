---
name: neomind-extension
description: Guide for creating NeoMind Edge AI Platform extensions using SDK V2 with ABI Version 3. Use when creating extensions, implementing commands, adding frontend components, or building NeoMind extensions. Helps with Rust backend implementation, React frontend components, and extension deployment.
argument-hint: [extension-name]
allowed-tools: Read, Write, Edit, Bash, Glob, Grep
---

# NeoMind Extension Development Skill

Guide for creating extensions for the NeoMind Edge AI Platform using the Extension SDK V2.

## Quick Commands

**Create new extension:**
```bash
# Copy from template
cp -r extensions/weather-forecast-v2 extensions/$ARGUMENTS
cd extensions/$ARGUMENTS
sed -i '' "s/weather-forecast-v2/$ARGUMENTS/g" Cargo.toml
```

**Build extension:**
```bash
cargo build --release -p neomind-$ARGUMENTS
```

**Install extension:**
```bash
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_$ARGUMENTS.dylib ~/.neomind/extensions/
```

---

## Extension Structure

```
extensions/your-extension-v2/
├── Cargo.toml              # Rust project configuration
├── src/
│   └── lib.rs              # Extension implementation
├── frontend/               # React components (optional)
│   ├── src/
│   │   └── index.tsx       # Component implementation
│   ├── package.json        # npm dependencies
│   ├── vite.config.ts      # Vite build config
│   └── frontend.json       # Component manifest
├── metadata.json           # Extension metadata
└── README.md               # Documentation
```

---

## Step-by-Step Workflow

### 1. Create Extension Project

**Choose the right template:**
- Simple data fetching: `weather-forecast-v2`
- Image processing: `image-analyzer-v2`
- Video processing: `yolo-video-v2`
- Device inference: `yolo-device-inference`

**Steps:**
1. Copy template: `cp -r extensions/weather-forecast-v2 extensions/your-extension-v2`
2. Rename directory to match your extension ID
3. Update all references in `Cargo.toml`, `src/lib.rs`, and `frontend/`

**Extension ID Convention:**
```
{category}-{feature}-v{major}

Examples:
- weather-forecast-v2 (weather data)
- image-analyzer-v2 (image AI)
- yolo-video-v2 (video AI)
- yolo-device-inference (device AI)
```

### 2. Configure Cargo.toml

**Required configuration:**

```toml
[package]
name = "your-extension-v2"
version = "2.0.0"
edition = "2021"

[lib]
name = "neomind_extension_your_extension_v2"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../../../NeoMind/crates/neomind-extension-sdk" }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = "0.1"
tokio = { version = "1", features = ["rt", "sync"] }
semver = "1"

[profile.release]
panic = "unwind"  # CRITICAL: Required for safety!
opt-level = 3
lto = "thin"
```

### 3. Implement Extension (src/lib.rs)

**Minimal template:**

```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};

// Extension struct
pub struct YourExtension {
    counter: AtomicI64,
}

impl YourExtension {
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }
}

// Implement Extension trait
#[async_trait]
impl Extension for YourExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "your-extension-v2".to_string(),
                name: "Your Extension".to_string(),
                version: Version::parse("2.0.0").unwrap(),
                description: Some("Your extension description".to_string()),
                author: Some("Your Name".to_string()),
                homepage: None,
                license: Some("MIT".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "counter".to_string(),
                    display_name: "Counter".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: String::new(),
                    min: None,
                    max: None,
                    required: false,
                },
            ]
        })
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "your_command".to_string(),
                    display_name: "Your Command".to_string(),
                    payload_template: String::new(),
                    parameters: vec![],
                    fixed_values: std::collections::HashMap::new(),
                    samples: vec![json!({})],
                    llm_hints: "Execute your command".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "your_command" => {
                // Implement your command logic
                Ok(json!({ "result": "success" }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }
}

// Export FFI - Just one line!
neomind_extension_sdk::neomind_export!(YourExtension);
```

### 4. Build & Test

```bash
# Build
cargo build --release

# Check output
ls -lh target/release/libneomind_extension_*.dylib

# Install
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_your_extension_v2.dylib ~/.neomind/extensions/
```

### 5. Create Frontend Component (Optional)

**Component structure:**

```tsx
// frontend/src/index.tsx
import { forwardRef, useState } from 'react'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
}

export interface DataSource {
  type: string
  extensionId?: string
  [key: string]: any
}

const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>
): Promise<{ success: boolean; data?: T; error?: string }> {
  const response = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args })
  })
  return response.json()
}

export const YourCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function YourCard(props, ref) {
    const { title = 'Your Extension', dataSource, className = '' } = props
    const extensionId = dataSource?.extensionId || 'your-extension-v2'

    return (
      <div ref={ref} className={`your-card ${className}`}>
        <style>{`
          .your-card {
            --ext-bg: rgba(255, 255, 255, 0.25);
            --ext-fg: hsl(240 10% 10%);
            --ext-border: rgba(255, 255, 255, 0.5);
            --ext-accent: hsl(221 83% 53%);
          }
          .dark .your-card {
            --ext-bg: rgba(30, 30, 30, 0.4);
            --ext-fg: hsl(0 0% 95%);
          }
        `}</style>
        <h3>{title}</h3>
        {/* Your component content */}
      </div>
    )
  }
)

export default { YourCard }
```

**Frontend manifest (frontend/frontend.json):**

```json
{
  "id": "your-extension-v2",
  "version": "2.0.0",
  "entrypoint": "your-extension-v2-components.umd.js",
  "components": [
    {
      "name": "YourCard",
      "type": "card",
      "displayName": "Your Extension Card",
      "description": "Displays data from your extension",
      "defaultSize": { "width": 300, "height": 200 },
      "refreshable": true,
      "refreshInterval": 5000
    }
  ],
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

---

## Key Concepts

### Process Isolation Architecture

All V2 extensions run in isolated processes by default:

- **Crash Safety**: Extension crashes don't affect main process
- **Memory Isolation**: Each extension has its own memory space
- **Resource Limits**: CPU and memory can be limited per extension

### ABI Version 3

New interface providing:
- Improved panic handling
- Frontend component support
- Standardized metric types
- Better error handling

### Extension ID Convention

```
{category}-{name}-v{major}

Examples:
- weather-forecast-v2
- image-analyzer-v2
- yolo-video-v2
```

---

## Common Patterns

### Async Command Execution

```rust
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    match command {
        "fetch_data" => {
            // Can use .await here
            let data = self.fetch_from_api().await?;
            Ok(json!({ "data": data }))
        }
        _ => Err(ExtensionError::CommandNotFound(command.to_string())),
    }
}
```

### Synchronous Metrics Production

```rust
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    // NO .await allowed here - must be synchronous
    let cached_data = self.cached_data.lock().unwrap().clone();
    let now = chrono::Utc::now().timestamp_millis();

    Ok(vec![
        ExtensionMetricValue {
            name: "metric_name".to_string(),
            value: ParamMetricValue::Integer(cached_data),
            timestamp: now,
        },
    ])
}
```

### Error Handling Best Practices

```rust
// ✅ Good: Use ? operator
fn process_data(&self) -> Result<Data> {
    let value = self.get_value()?;
    Ok(Data::new(value))
}

// ✅ Good: Use unwrap_or for defaults
let count = args.get("count")
    .and_then(|v| v.as_i64())
    .unwrap_or(1);

// ❌ Avoid: Direct unwrap may cause process exit
let value = some_option.unwrap();  // Not recommended
```

---

## Safety Requirements

### CRITICAL: Panic Configuration

**Always set `panic = "unwind"` in Cargo.toml:**

```toml
[profile.release]
panic = "unwind"  # REQUIRED!
opt-level = 3
lto = "thin"
```

Using `panic = "abort"` will cause process crashes to affect the extension runner.

### Async Runtime Rules

- `produce_metrics()`: **Synchronous** - NO `.await`
- `execute_command()`: **Async** - CAN use `.await`

---

## AI/ML Extension Special Considerations

### Model Lifecycle Management

**CRITICAL for YOLO and other ML models:**

When creating extensions that load heavy ML models:

```rust
pub struct YourMLExtension {
    detector: Arc<Mutex<Option<Detector>>>,  // Use Arc<Mutex<Option<T>> for lazy loading
}

impl YourMLExtension {
    pub fn new() -> Self {
        Self {
            detector: Arc::new(Mutex::new(None)),  // Don't load model in new()
        }
    }
    
    // Load model on first use
    async fn ensure_detector_loaded(&self) -> Result<()> {
        let mut detector_guard = self.detector.lock().await;
        if detector_guard.is_none() {
            // Load model here
            *detector_guard = Some(Detector::new()?);
        }
        Ok(())
    }
}

// ⚠️ IMPORTANT: Do NOT call detector.shutdown() in close_session
// The model should remain loaded for the extension's lifetime
```

**Why this matters:**
- Models can be 200MB+ and take time to load
- Loading once and reusing is more efficient
- Extension runner handles cleanup on process termination
- OS reclaims all resources when extension process exits

### Session Management

For video/streaming extensions:

```rust
// ✅ Good: Keep detector across sessions
async fn close_session(&self, session_id: &str) -> Result<()> {
    // Stop the session processing
    self.sessions.remove(session_id);
    
    // DO NOT remove detector - keep it loaded!
    eprintln!("[Extension] Detector remains loaded for reuse");
    Ok(())
}

// ❌ Bad: Removing detector causes all future inferences to fail
async fn close_session(&self, session_id: &str) -> Result<()> {
    self.detector.lock().take();  // Don't do this!
    Ok(())
}
```

---



## Reference Files

For detailed information, see:
- [Extension Architecture](reference/architecture.md) - Process isolation details
- [SDK API Reference](reference/sdk-api.md) - Complete SDK documentation
- [Frontend Guide](reference/frontend.md) - React component development
- [Examples](examples/) - Working extension examples

---

## Building & Packaging

### Local Build

```bash
# Build all extensions
./build.sh --yes

# Build without installing
./build.sh --skip-install

# Debug build
./build.sh --debug

# Build single extension
cargo build --release -p your-extension-v2
```

### Creating .nep Package

```bash
# Package all extensions
./build.sh --yes --skip-install

# Packages are created in dist/
ls -lh dist/*.nep
```

**Package structure:**
```
extension-name-version-platform.nep
├── manifest.json           # Metadata
├── binaries/
│   └── darwin_aarch64/
│       └── extension.dylib
└── frontend/               # Optional
    └── *.umd.cjs
```

### Cross-Platform Building

**Supported platforms:**
- macOS ARM64 (aarch64-apple-darwin)
- macOS x86_64 (x86_64-apple-darwin) - Cross-compile on ARM
- Linux AMD64 (x86_64-unknown-linux-gnu)
- Linux ARM64 (aarch64-unknown-linux-gnu)
- Windows x86_64 (x86_64-pc-windows-msvc)
- Windows x86 (i686-pc-windows-msvc)

**Using GitHub Actions:**

```yaml
# .github/workflows/build-extension.yml
strategy:
  matrix:
    include:
      - os: macos-latest
        platform: darwin_aarch64
        target: aarch64-apple-darwin
      - os: macos-latest
        platform: darwin_x86_64
        target: x86_64-apple-darwin
      - os: ubuntu-latest
        platform: linux_amd64
        target: x86_64-unknown-linux-gnu
      - os: ubuntu-22.04-arm
        platform: linux_arm64
        target: aarch64-unknown-linux-gnu
      - os: windows-latest
        platform: windows_amd64
        target: x86_64-pc-windows-msvc
      - os: windows-latest
        platform: windows_x86
        target: i686-pc-windows-msvc
```

**Platform Identifiers:**
- `darwin_aarch64` - macOS ARM64 (Apple Silicon)
- `darwin_x86_64` - macOS Intel
- `linux_amd64` - Linux x86_64
- `linux_arm64` - Linux ARM64
- `windows_amd64` - Windows 64-bit
- `windows_x86` - Windows 32-bit

---

## Frontend Development Guide

### Component Template

**File: frontend/src/index.tsx**

```tsx
import { forwardRef, useState, useEffect } from 'react'
import { executeExtensionCommand } from './api'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: {
    type: string
    extensionId?: string
    config?: Record<string, any>
  }
  className?: string
}

export const YourExtensionCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function YourExtensionCard(props, ref) {
    const { title = 'Your Extension', dataSource, className = '' } = props
    const [data, setData] = useState<any>(null)
    const [loading, setLoading] = useState(false)
    const extensionId = dataSource?.extensionId || 'your-extension-v2'

    // Fetch data on mount
    useEffect(() => {
      const fetchData = async () => {
        setLoading(true)
        const result = await executeExtensionCommand(
          extensionId,
          'your_command',
          {}
        )
        if (result.success) {
          setData(result.data)
        }
        setLoading(false)
      }
      fetchData()
    }, [extensionId])

    if (loading) return <div>Loading...</div>

    return (
      <div ref={ref} className={`extension-card ${className}`}>
        <style>{`
          .extension-card {
            --ext-bg: rgba(255, 255, 255, 0.25);
            --ext-fg: hsl(240 10% 10%);
            --ext-border: rgba(255, 255, 255, 0.5);
            --ext-accent: hsl(221 83% 53%);
            padding: 16px;
            border-radius: 8px;
          }
          .dark .extension-card {
            --ext-bg: rgba(30, 30, 30, 0.4);
            --ext-fg: hsl(0 0% 95%);
          }
        `}</style>
        <h3>{title}</h3>
        {data && <div>{JSON.stringify(data, null, 2)}</div>}
      </div>
    )
  }
)
```

**File: frontend/src/api.ts**

```typescript
const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

export async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>
): Promise<{ success: boolean; data?: T; error?: string }> {
  const response = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args })
  })
  return response.json()
}

export async function getExtensionMetrics(
  extensionId: string
): Promise<{ success: boolean; data?: any[]; error?: string }> {
  const response = await fetch(`${getApiBase()}/extensions/${extensionId}/metrics`)
  return response.json()
}
```

### Frontend Manifest

**File: frontend/frontend.json**

```json
{
  "id": "your-extension-v2",
  "version": "2.0.0",
  "entrypoint": "your-extension-v2-components.umd.js",
  "components": [
    {
      "name": "YourExtensionCard",
      "type": "card",
      "displayName": "Your Extension",
      "description": "Display data from your extension",
      "defaultSize": { "width": 300, "height": 200 },
      "minSize": { "width": 200, "height": 150 },
      "maxSize": { "width": 800, "height": 600 },
      "refreshable": true,
      "refreshInterval": 5000,
      "icon": "cpu",
      "configSchema": {
        "updateInterval": {
          "type": "number",
          "description": "Update interval in milliseconds",
          "default": 5000
        }
      }
    }
  ],
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

### Vite Build Config

**File: frontend/vite.config.ts**

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: 'src/index.tsx',
      name: 'YourExtensionV2Components',
      formats: ['umd', 'cjs'],
      fileName: (format) => `your-extension-v2-components.${format === 'umd' ? 'umd.js' : 'umd.cjs'}`
    },
    rollupOptions: {
      external: ['react', 'react-dom'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM'
        }
      }
    }
  }
})
```

---

## Troubleshooting

**Extension not loading:**
1. Check ABI version returns 3
2. Verify binary format matches platform
3. Check extension runner logs

**Process crashes:**
1. Check for `unwrap()` or `expect()` calls
2. Review error handling in commands
3. Monitor memory usage

**Frontend not displaying:**
1. Verify `frontend.json` exists
2. Check component name matches
3. Verify UMD build output exists

---

## Platform Support

| Platform | Architecture | Binary Extension |
|----------|--------------|------------------|
| macOS    | ARM64        | `*.dylib`        |
| macOS    | x86_64       | `*.dylib`        |
| Linux    | x86_64       | `*.so`           |
| Linux    | ARM64        | `*.so`           |
| Windows  | x86_64, x86 (32-bit) | `*.dll`          |
| Cross-platform | Any    | `*.wasm`         |

**Latest Release (v2.0.0)**: 6 platforms, 27 extension packages

---

## Quick Reference: File Locations

- **NeoMind repo**: `~/NeoMindProject/NeoMind`
- **Extension SDK**: `~/NeoMindProject/NeoMind/crates/neomind-extension-sdk`
- **Extensions repo**: `~/NeoMindProject/NeoMind-Extension`
- **Extension templates**: `~/NeoMindProject/NeoMind-Extension/extensions/`
- **Install location**: `~/.neomind/extensions/`

---

## License

MIT License
