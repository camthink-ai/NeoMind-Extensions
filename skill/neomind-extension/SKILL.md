---
name: neomind-extension
description: |
  Comprehensive guide for creating NeoMind Edge AI Platform extensions for the isolated NeoMind runtime.

  Use this skill when:
  - Creating new NeoMind extensions from scratch
  - Implementing extension commands and metrics
  - Adding React frontend components
  - Building cross-platform extension packages
  - Implementing ML/AI extensions with YOLO models
  - Setting up video streaming extensions
  - Creating device inference extensions

  This skill teaches:
  - Current runtime patterns based on real extension implementations
  - ExtensionMetadata::new() builder pattern
  - FFI exports using neomind_export!() macro
  - ML model lifecycle management (lazy loading, keep loaded across sessions)
  - Process isolation architecture
  - Cross-platform building for 6 platforms
  - React frontend development with Vite

  Based on actual extensions: weather-forecast-v2, image-analyzer-v2, yolo-video-v2, yolo-device-inference

version: 2.0.0
argument-hint: "[extension-name]"
allowed-tools: [Read, Write, Edit, Bash, Glob, Grep, mcp__serena_serena__find_symbol, mcp__serena_serena__get_symbols_overview]
---

# NeoMind Extension Development Guide

Learn to create production-ready extensions for the NeoMind Edge AI Platform using the isolated NeoMind extension runtime.

## Quick Start

**Create a new extension:**
```bash
cd NeoMind-Extensions
cp -r extensions/weather-forecast-v2 extensions/my-extension-v2
cd extensions/my-extension-v2
# Update Cargo.toml with your extension name
```

**Build and test:**
```bash
# Dev build + auto-install
./build.sh --dev --single my-extension-v2

# Or manual build
cargo build --release
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_my_extension_v2.dylib ~/.neomind/extensions/
```

**Build all & package:**
```bash
./build.sh                           # Build all, create packages
./build.sh --release 2.4.0           # Release with version
./release.sh 2.4.0                   # Same as above
```

---

## Essential Extension Structure

```
extensions/your-extension-v2/
├── Cargo.toml              # Project configuration
├── src/
│   └── lib.rs              # Extension implementation
├── frontend/               # Optional React components
│   ├── src/index.tsx
│   ├── package.json
│   ├── vite.config.ts
│   └── frontend.json
└── README.md
```

---

## Step 1: Configure Cargo.toml

**Required setup:**

```toml
[package]
name = "your-extension-v2"
version = "2.0.0"
edition = "2021"

[lib]
name = "neomind_extension_your_extension_v2"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../../path/to/NeoMind/crates/neomind-extension-sdk" }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = "0.1"
tokio = { version = "1", features = ["rt", "sync"] }
semver = "1"
parking_lot = "0.12"  # For efficient locks
```

**CRITICAL SAFETY REQUIREMENT:**
- `panic = "unwind"` is **mandatory** - using `panic = "abort"` will crash the entire NeoMind server on any panic
- If the extension lives in a Cargo workspace, put `[profile.release]` in the workspace root `Cargo.toml`, not the member crate

---

## Step 2: Basic Extension Template

**File: `src/lib.rs`**

```rust
use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};
use semver::Version;

// ============================================================================
// Extension Struct
// ============================================================================

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

impl Default for YourExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for YourExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "your-extension-v2",
                "Your Extension",
                Version::parse("2.0.0").unwrap()
            )
            .with_description("Description of what your extension does")
            .with_author("Your Name")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "counter".to_string(),
                display_name: "Counter".to_string(),
                data_type: MetricDataType::Integer,
                unit: "count".to_string(),
                min: Some(0.0),
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "your_command".to_string(),
                display_name: "Your Command".to_string(),
                description: "What this command does".to_string(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "param1".to_string(),
                        display_name: "Parameter 1".to_string(),
                        description: "Description of parameter".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({"param1": "example"})],
                parameter_groups: vec![],
            },
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "your_command" => {
                let param1 = args.get("param1")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing param1".to_string()))?;

                // Your logic here
                self.counter.fetch_add(1, Ordering::SeqCst);

                Ok(json!({
                    "result": "success",
                    "param1": param1,
                    "count": self.counter.load(Ordering::SeqCst),
                }))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        // NOTE: This is SYNCHRONOUS - NO .await allowed!
        let now = chrono::Utc::now().timestamp_millis();

        Ok(vec![
            ExtensionMetricValue {
                name: "counter".to_string(),
                value: ParamMetricValue::Integer(self.counter.load(Ordering::SeqCst)),
                timestamp: now,
            },
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// FFI Export
// ============================================================================

// This single macro generates all required FFI exports
neomind_extension_sdk::neomind_export!(YourExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = YourExtension::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "your-extension-v2");
        assert_eq!(meta.name, "Your Extension");
    }

    #[test]
    fn test_extension_commands() {
        let ext = YourExtension::new();
        let commands = ext.commands();
        assert!(!commands.is_empty());
        assert_eq!(commands[0].name, "your_command");
    }
}
```

**Key Points:**
- ExtensionMetadata uses builder pattern: `ExtensionMetadata::new().with_description().with_author()`
- `metrics()` and `commands()` return `Vec<>`, not slices
- `produce_metrics()` is **synchronous** - no `.await` allowed
- `execute_command()` is `async` - can use `.await`
- FFI export is just one line: `neomind_export!(YourExtension)`

---

## Step 3: Extension ID Convention

Follow this pattern:
```
{category}-{feature}-v{major}

Examples:
✅ weather-forecast-v2
✅ image-analyzer-v2
✅ yolo-video-v2
✅ yolo-device-inference

❌ weather_forecast (use hyphens, not underscores)
❌ weather-forecast (missing version suffix)
```

**Why `-v2` suffix?**
- Indicates the current generation of the isolated runtime protocol
- Distinguishes from legacy extensions
- Prevents conflicts with old extensions

---

## Step 4: Implementing Commands

### Simple Command (No Parameters)

```rust
ExtensionCommand {
    name: "refresh".to_string(),
    display_name: "Refresh Data".to_string(),
    description: "Refresh cached data".to_string(),
    payload_template: String::new(),
    parameters: vec![],
    fixed_values: Default::default(),
    samples: vec![json!({})],
    parameter_groups: vec![],
}
```

### Command with Required Parameters

```rust
ExtensionCommand {
    name: "get_weather".to_string(),
    display_name: "Get Weather".to_string(),
    description: "Get current weather for a city".to_string(),
    payload_template: String::new(),
    parameters: vec![
        ParameterDefinition {
            name: "city".to_string(),
            display_name: "City".to_string(),
            description: "City name".to_string(),
            param_type: MetricDataType::String,
            required: true,
            default_value: None,
            min: None,
            max: None,
            options: vec![
                "Beijing".to_string(),
                "Shanghai".to_string(),
                "New York".to_string(),
            ],
        },
    ],
    fixed_values: Default::default(),
    samples: vec![json!({"city": "Beijing"})],
    parameter_groups: vec![],
}
```

### Command with Optional Parameters

```rust
ParameterDefinition {
    name: "threshold".to_string(),
    display_name: "Confidence Threshold".to_string(),
    description: "Minimum confidence for detections".to_string(),
    param_type: MetricDataType::Float,
    required: false,
    default_value: Some(ParamMetricValue::Float(0.5)),
    min: Some(0.0),
    max: Some(1.0),
    options: vec![],
},
```

---

## Step 5: Error Handling

**Use the `?` operator for clean error propagation:**

```rust
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    match command {
        "process_data" => {
            let data = self.fetch_data()
                .await
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Fetch failed: {}", e)))?;

            let result = self.parse_data(&data)
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Parse failed: {}", e)))?;

            Ok(json!({"result": result}))
        }
        _ => Err(ExtensionError::CommandNotFound(command.to_string())),
    }
}
```

**Error types:**
- `ExtensionError::CommandNotFound(name)` - Command doesn't exist
- `ExtensionError::InvalidArguments(msg)` - Bad parameters
- `ExtensionError::ExecutionFailed(msg)` - Command execution failed
- `ExtensionError::NotSupported(msg)` - Feature not supported

---

## Step 6: Metrics Production

**Synchronous metrics (cached data):**

```rust
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    // NO .await allowed - must be synchronous
    let now = chrono::Utc::now().timestamp_millis();

    Ok(vec![
        ExtensionMetricValue {
            name: "temperature_c".to_string(),
            value: ParamMetricValue::Float(self.last_temp_c.load(Ordering::SeqCst) as f64 / 100.0),
            timestamp: now,
        },
        ExtensionMetricValue {
            name: "humidity_percent".to_string(),
            value: ParamMetricValue::Integer(self.last_humidity.load(Ordering::SeqCst)),
            timestamp: now,
        },
    ])
}
```

**Pattern: Cache async results in atomic types for synchronous access**

From weather-forecast-v2:
```rust
pub struct WeatherExtension {
    last_temperature_c: AtomicI64,  // Store temperature * 100
    last_humidity_percent: AtomicI64,
    last_update_ts: AtomicI64,
}

// In async command, update the cache
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    match command {
        "get_weather" => {
            let weather = self.fetch_weather_sync(city)?;

            // Update cache
            self.last_temperature_c.store((weather.temperature_c * 100.0) as i64, Ordering::SeqCst);
            self.last_humidity_percent.store(weather.humidity_percent as i64, Ordering::SeqCst);

            Ok(json!(weather))
        }
        // ...
    }
}

// In produce_metrics, read from cache
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let now = chrono::Utc::now().timestamp_millis();
    Ok(vec![
        ExtensionMetricValue {
            name: "temperature_c".to_string(),
            value: ParamMetricValue::Float(self.last_temperature_c.load(Ordering::SeqCst) as f64 / 100.0),
            timestamp: now,
        },
    ])
}
```

---

## Step 7: ML Model Extensions (YOLO)

### Model Lifecycle Management

**CRITICAL: Keep models loaded across sessions**

From yolo-video-v2 (fixed version):

```rust
pub struct YoloVideoExtension {
    detector: Arc<Mutex<Option<YoloDetector>>>,  // Lazy loading
    sessions: Arc<Mutex<HashMap<String, SessionData>>>,
}

impl YoloVideoExtension {
    pub fn new() -> Self {
        Self {
            detector: Arc::new(Mutex::new(None)),  // Don't load yet
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Load on first use
    async fn ensure_detector_loaded(&self) -> Result<()> {
        let mut detector = self.detector.lock().await;
        if detector.is_none() {
            *detector = Some(YoloDetector::new()?);
            eprintln!("[YOLO] Model loaded successfully");
        }
        Ok(())
    }
}

// ✅ CORRECT: Keep detector when closing session
async fn close_session(&self, session_id: &str) -> Result<()> {
    self.sessions.remove(session_id);
    // DO NOT remove detector - keep it loaded!
    eprintln!("[YOLO] Detector remains loaded for reuse");
    Ok(())
}

// ❌ WRONG: Don't do this!
async fn close_session(&self, session_id: &str) -> Result<()> {
    self.detector.lock().take();  // This removes the model!
    Ok(())
}
```

**Why keep models loaded?**
- Models are 200MB+ and take time to load
- Loading once and reusing is much more efficient
- Extension runner handles cleanup on process termination
- OS reclaims all resources when extension process exits

### Model Loading Pattern

From image-analyzer-v2:

```rust
struct YOLODetector {
    model: Option<Runtime<YOLO>>,
    load_error: Option<String>,
}

impl YOLODetector {
    fn new(conf: f32, iou: f32, version: &str, scale: &str) -> Self {
        match Self::try_load_model(conf, iou, version, scale) {
            Ok(model) => {
                tracing::info!("[YOLO] Model loaded");
                Self {
                    model: Some(model),
                    load_error: None,
                }
            }
            Err(e) => {
                tracing::error!("[YOLO] Failed to load: {}", e);
                Self {
                    model: None,
                    load_error: Some(e),
                }
            }
        }
    }

    fn try_load_model(conf: f32, iou: f32, version: &str, scale: &str) -> std::result::Result<Runtime<YOLO>, String> {
        // Load model from disk
        let model_path = std::path::PathBuf::from("models").join("yolov8n.onnx");

        let model_data = std::fs::read(&model_path)
            .map_err(|e| format!("Failed to read model: {}", e))?;

        // Create YOLO model
        let config = Config::yolo_detect()
            .with_class_confs(&[conf])
            .with_iou(iou);

        let model = YOLO::new(config)
            .map_err(|e| format!("Failed to create YOLO: {:?}", e))?;

        Ok(model)
    }
}
```

---

## Step 8: Video Streaming Extensions

### Stream Capability Implementation

From yolo-video-v2:

```rust
use neomind_extension_sdk::prelude::{
    StreamCapability, StreamMode, StreamDirection, StreamDataType,
    StreamSession, SessionStats, PushOutputMessage,
};

impl StreamCapability for YoloVideoExtension {
    fn supported_modes(&self) -> Vec<StreamMode> {
        vec![
            StreamMode::Push(StreamDirection::Output),  // Push frames to client
        ]
    }

    fn supported_data_types(&self) -> Vec<StreamDataType> {
        vec![
            StreamDataType::MJPEG,  // Motion JPEG video
        ]
    }

    async fn create_session(&self, session_id: String, config: &serde_json::Value) -> Result<Box<dyn StreamSession>> {
        // Ensure detector is loaded
        self.ensure_detector_loaded().await?;

        // Create session
        let session = VideoSession::new(
            session_id.clone(),
            self.detector.clone(),
            config,
        )?;

        // Store session
        self.sessions.lock().insert(session_id.clone(), session.get_stats());

        Ok(Box::new(session))
    }

    async fn close_session(&self, session_id: &str) -> Result<()> {
        // Remove session but keep detector!
        self.sessions.remove(session_id);
        eprintln!("[YOLO] Session {} closed, detector remains loaded", session_id);
        Ok(())
    }

    fn get_session_stats(&self) -> Result<Vec<SessionStats>> {
        let sessions = self.sessions.lock();
        Ok(sessions.values().cloned().collect())
    }
}
```

---

## Step 9: Device Integration Extensions

### Event Handling Pattern

From yolo-device-inference:

```rust
use neomind_extension_sdk::capabilities::device;

#[async_trait]
impl Extension for YoloDeviceInference {
    // ... standard methods ...

    fn event_subscriptions(&self) -> Vec<device::EventSubscription> {
        vec![
            device::EventSubscription {
                device_id: None,  // All devices
                event_type: device::EventType::DataUpdated,
            },
        ]
    }

    async fn handle_event(&self, event: &device::DeviceEvent) -> Result<()> {
        match &event.event_type {
            device::EventType::DataUpdated => {
                // Run inference on device image data
                if let Some(image_data) = event.data.get("image") {
                    let detections = self.run_inference(image_data).await?;
                    self.store_detection_results(event.device_id.clone(), detections).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
```

---

## Step 10: Frontend Components (Optional)

### Component Structure

**File: `frontend/src/index.tsx`**

```tsx
import { forwardRef, useState, useEffect } from 'react'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: {
    type: string
    extensionId?: string
    config?: Record<string, any>
  }
  className?: string
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

export const YourExtensionCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function YourExtensionCard(props, ref) {
    const { title = 'Your Extension', dataSource, className = '' } = props
    const [data, setData] = useState<any>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const extensionId = dataSource?.extensionId || 'your-extension-v2'

    const fetchData = async () => {
      setLoading(true)
      setError(null)
      const result = await executeExtensionCommand(
        extensionId,
        'your_command',
        { param1: 'value' }
      )
      if (result.success) {
        setData(result.data)
      } else {
        setError(result.error || 'Unknown error')
      }
      setLoading(false)
    }

    useEffect(() => {
      fetchData()
    }, [extensionId])

    return (
      <div ref={ref} className={`extension-card ${className}`}>
        <style>{`
          .extension-card {
            --ext-bg: rgba(255, 255, 255, 0.25);
            --ext-fg: hsl(240 10% 10%);
            --ext-muted: hsl(240 5% 40%);
            --ext-border: rgba(255, 255, 255, 0.5);
            --ext-accent: hsl(221 83% 53%);
            padding: 16px;
            border-radius: 8px;
            background: var(--ext-bg);
            color: var(--ext-fg);
          }
          .dark .extension-card {
            --ext-bg: rgba(30, 30, 30, 0.4);
            --ext-fg: hsl(0 0% 95%);
            --ext-muted: hsl(0 0% 65%);
          }
        `}</style>
        <h3>{title}</h3>
        {loading && <div>Loading...</div>}
        {error && <div>Error: {error}</div>}
        {data && <div>{JSON.stringify(data, null, 2)}</div>}
      </div>
    )
  }
)

export default { YourExtensionCard }
```

### Frontend Manifest

**File: `frontend/frontend.json`**

```json
{
  "id": "your-extension-v2",
  "version": "2.0.0",
  "entrypoint": "your-extension-v2-components.umd.js",
  "components": [
    {
      "name": "YourExtensionCard",
      "type": "card",
      "displayName": "Your Extension Card",
      "description": "Displays data from your extension",
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

**File: `frontend/vite.config.ts`**

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

## Building & Packaging

### Unified Build Script

**Main build script:**
```bash
./build.sh                           # Build all, create packages
./build.sh --dev                     # Dev build + auto-install
./build.sh --release 2.4.0           # Release with version in filenames
./build.sh --single your-extension-v2 # Build single extension
./build.sh --skip-frontend           # Skip frontend builds
./build.sh --debug                   # Debug build
./build.sh --help                    # Show all options
```

**Release helper:**
```bash
./release.sh 2.4.0                   # Build + package for release
```

### Development Workflow

```bash
# Create new extension
cp -r extensions/weather-forecast-v2 extensions/my-extension-v2
cd extensions/my-extension-v2
# Update Cargo.toml

# Dev build + auto-install
./build.sh --dev --single my-extension-v2

# Test in NeoMind
# Extensions are installed to ~/.neomind/extensions/
```

### Creating .nep Package

```bash
# Package all extensions
./build.sh

# Package with version
./release.sh 2.4.0

# Packages created in dist/
ls -lh dist/*.nep
```

**Package structure:**
```
your-extension-v2-2.0.0.nep  (ZIP archive)
├── manifest.json           # Extension metadata
├── binaries/
│   ├── darwin_aarch64/    # macOS ARM64
│   │   └── libneomind_extension_your_extension_v2.dylib
│   ├── darwin_x86_64/     # macOS Intel
│   ├── linux_amd64/       # Linux x86_64
│   ├── linux_arm64/       # Linux ARM64
│   ├── windows_amd64/     # Windows 64-bit
│   └── windows_x86/       # Windows 32-bit
└── frontend/              # Optional
    └── your-extension-v2-components.umd.cjs
```

### Cross-Platform Building

**Supported platforms:**
- `darwin_aarch64` - macOS ARM64 (Apple Silicon)
- `darwin_x86_64` - macOS Intel
- `linux_amd64` - Linux x86_64
- `linux_arm64` - Linux ARM64
- `windows_amd64` - Windows 64-bit
- `windows_x86` - Windows 32-bit

**Using GitHub Actions:**

The workflow `.github/workflows/build-nep-packages.yml` builds all 6 platforms automatically.

```yaml
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

---

## Key Concepts

### Runtime Protocol v3

Current extensions use runtime protocol v3:
- **Improved safety**: Better panic handling with `panic = "unwind"`
- **Frontend support**: Built-in React component integration
- **Standardized metrics**: Unified metric types across extensions
- **Better error handling**: Structured error responses

### Process Isolation Architecture

All V2 extensions run in isolated processes:
- **Crash Safety**: Extension crashes don't affect the main process
- **Memory Isolation**: Each extension has its own memory space
- **Resource Limits**: CPU and memory can be limited per extension
- **Automatic Restart**: Extensions can be restarted on failure

### Sync vs Async Methods

**Synchronous (NO `.await`):**
- `metadata()` - Returns extension metadata
- `metrics()` - Returns metric descriptors
- `produce_metrics()` - Produces metric values
- `commands()` - Returns command descriptors

**Asynchronous (CAN use `.await`):**
- `execute_command()` - Executes extension commands
- `configure()` - Applies configuration changes
- Event handlers - Process device events

**Pattern:** Cache async results in atomic types for synchronous `produce_metrics()` access.

---

## Common Patterns

### Command with File Processing

```rust
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    match command {
        "process_file" => {
            let file_data = args.get("data")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExtensionError::InvalidArguments("Missing data".to_string()))?;

            let decoded = base64::engine::general_purpose::STANDARD.decode(file_data)
                .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid base64: {}", e)))?;

            let result = self.process_bytes(&decoded)?;
            Ok(json!({"result": result}))
        }
        _ => Err(ExtensionError::CommandNotFound(command.to_string())),
    }
}
```

### Metrics with Conditional Values

```rust
fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut metrics = Vec::new();

    // Always include count
    metrics.push(ExtensionMetricValue {
        name: "request_count".to_string(),
        value: ParamMetricValue::Integer(self.count.load(Ordering::SeqCst)),
        timestamp: now,
    });

    // Include data only if available
    if self.has_data.load(Ordering::SeqCst) {
        metrics.push(ExtensionMetricValue {
            name: "temperature".to_string(),
            value: ParamMetricValue::Float(self.last_temp.load(Ordering::SeqCst) as f64 / 100.0),
            timestamp: now,
        });
    }

    Ok(metrics)
}
```

---

## Safety Requirements

### CRITICAL: Panic Configuration

**Always set `panic = "unwind"` in the workspace root Cargo.toml (or the crate root if it is standalone):**

```toml
[profile.release]
panic = "unwind"  # REQUIRED!
opt-level = 3
lto = "thin"
```

Using `panic = "abort"` will cause extension panics to crash the entire NeoMind server.

### Avoid Blocking Operations

**Don't block in async functions:**
```rust
// ❌ Bad: Blocks executor
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    std::thread::sleep(std::time::Duration::from_secs(5));  // Don't do this!
    Ok(json!({}))
}

// ✅ Good: Uses async sleep
async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    tokio::time::sleep(Duration::from_secs(5)).await;  // OK
    Ok(json!({}))
}
```

### Thread Safety

Use `Arc` for shared state across async tasks:

```rust
pub struct MyExtension {
    shared_state: Arc<Mutex<SharedState>>,
}

async fn process_concurrent(&self) -> Result<()> {
    let state = self.shared_state.clone();

    // Spawn multiple tasks sharing state
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let state = state.clone();
            tokio::spawn(async move {
                let mut data = state.lock().await;
                // Process data
            })
        })
        .collect();

    // Wait for all tasks
    for handle in handles {
        handle.await?;
    }

    Ok(())
}
```

---

## Marketplace Release Checklist (CRITICAL)

Before releasing extensions to the marketplace, verify ALL of the following:

### 1. Component Type MUST Be Unique

**Problem:** If two extensions have the same `type` in their dashboard component, only one will appear in the UI.

```bash
# ❌ WRONG: Both use "yolo-card" - conflict!
yolo-device-inference -> type: "yolo-card"
yolo-video-v2          -> type: "yolo-card"

# ✅ CORRECT: Each has unique type
yolo-device-inference -> type: "yolo-device-inference-card"
yolo-video-v2          -> type: "yolo-video-card"
```

**The component type is auto-generated by build.sh:**
```bash
# Format: {extension-name-without-v2}-card
COMPONENT_TYPE=$(echo "$ext" | sed 's/-v2$//' | sed 's/-v1//')"-card"
```

**Verification:**
```bash
# Check manifest.json in the .nep package
unzip -p dist/your-extension-2.3.1-darwin_aarch64.nep manifest.json | jq '.frontend.components[].type'

# Should output unique type like: "your-extension-card"
```

### 2. Frontend Field Format in metadata.json

**Problem:** `metadata.json` on GitHub marketplace must use correct format for `frontend.components`.

```json
// ❌ WRONG: Full component objects (causes "Failed to load details")
{
  "frontend": {
    "components": [
      { "name": "MyCard", "type": "card", ... }
    ]
  }
}

// ✅ CORRECT: Array of component names only
{
  "frontend": {
    "components": ["MyCard"],
    "entrypoint": "my-extension-components.umd.cjs"
  }
}
```

**Why:** The `FrontendInfo` struct in NeoMind expects `Vec<String>`, not `Vec<Object>`.

### 3. Version Consistency in Builds URLs

**Problem:** The builds URLs must use the **market version** (release version), NOT the extension's internal version.

```json
// ❌ WRONG: Uses extension version 2.0.0 in URL
{
  "builds": {
    "darwin-aarch64": {
      "url": "https://github.com/.../v2.3.0/my-extension-2.0.0-darwin_aarch64.nep"
    }
  }
}

// ✅ CORRECT: Uses market version 2.3.0 in URL
{
  "builds": {
    "darwin-aarch64": {
      "url": "https://github.com/.../v2.3.0/my-extension-2.3.0-darwin_aarch64.nep"
    }
  }
}
```

**Why:** GitHub release files are named with the market version, not individual extension versions.

### Pre-Release Verification Script

```bash
#!/bin/bash
# Run this before creating a GitHub release

VERSION="2.3.1"

echo "=== Checking metadata.json files ==="
for ext_dir in extensions/*/; do
    ext_id=$(basename "$ext_dir")
    if [ -f "$ext_dir/metadata.json" ]; then
        echo "Checking $ext_id..."

        # Check frontend format
        components=$(jq '.frontend.components[0]' "$ext_dir/metadata.json" 2>/dev/null)
        if [ "$components" != "null" ] && echo "$components" | jq -e 'type == "object"' >/dev/null 2>&1; then
            echo "  ❌ ERROR: frontend.components should be string array, not objects"
        fi

        # Check version in URL
        url=$(jq -r '.builds["darwin-aarch64"].url' "$ext_dir/metadata.json" 2>/dev/null)
        if [[ "$url" == *"-2.0.0-"* ]] && [ "$VERSION" != "2.0.0" ]; then
            echo "  ❌ ERROR: URL contains wrong version (2.0.0 instead of $VERSION)"
        fi
    fi
done

echo ""
echo "=== Checking .nep package manifests ==="
for nep in dist/*.nep; do
    if [ -f "$nep" ]; then
        ext_id=$(basename "$nep" | sed 's/-[0-9].*//')
        component_type=$(unzip -p "$nep" manifest.json | jq -r '.frontend.components[0].type' 2>/dev/null)
        echo "$ext_id: type=$component_type"
    fi
done
```

---

## Troubleshooting

### Extension Not Loading

1. Check ABI version returns 3:
   ```rust
   #[no_mangle]
   pub extern "C" fn neomind_extension_abi_version() -> u32 {
       3
   }
   ```

2. Verify binary format matches platform
3. Check extension runner logs: `neomind logs --extension your-extension`

### Process Crashes

1. Check for `unwrap()` or `expect()` calls that could panic
2. Review error handling in commands
3. Monitor memory usage
4. Ensure `panic = "unwind"` is set

### Frontend Not Displaying

1. Verify `frontend.json` exists and is valid
2. Check component name matches manifest
3. Verify UMD build output exists in `frontend/dist/`
4. Check browser console for errors
5. **Check component type is unique** - duplicate types cause conflicts
6. **Verify metadata.json format** - `frontend.components` must be `["ComponentName"]` not objects
7. **Check builds URLs** - must use market version, not extension version

### "Failed to load details" in Marketplace

Common causes:
1. `metadata.json` has wrong format for `frontend` field
2. JSON parsing error due to invalid structure
3. GitHub CDN serving cached old version (wait 5-10 minutes)

**Fix:** Run `./scripts/update-versions.sh 2.3.1` to regenerate correct metadata.json

### Model Loading Issues

1. Check model files exist in `models/` directory
2. Verify model path is correct
3. Check NEOMIND_EXTENSION_DIR environment variable
4. Review error logs for model loading failures

---

## Real Extension Examples

### Weather Extension (Simple API Client)
- **Path:** `extensions/weather-forecast-v2/src/lib.rs`
- **Features:** Sync HTTP client, metric caching, configuration
- **Key Patterns:**
  - `ExtensionMetadata::new().with_config_parameters()`
  - Atomic metrics for cached data
  - Sync HTTP with ureq

### Image Analyzer (ML Inference)
- **Path:** `extensions/image-analyzer-v2/src/lib.rs`
- **Features:** YOLOv8 object detection, base64 image handling
- **Key Patterns:**
  - Lazy model loading with `Arc<Mutex<Option<Detector>>>`
  - Fallback analysis when model unavailable
  - Model status diagnostics

### YOLO Video (Streaming)
- **Path:** `extensions/yolo-video-v2/src/lib.rs`
- **Features:** Real-time video streaming, session management
- **Key Patterns:**
  - `StreamCapability` trait implementation
  - Keep model loaded across sessions (CRITICAL)
  - MJPEG push streaming

### YOLO Device Inference (Event-Driven)
- **Path:** `extensions/yolo-device-inference/src/lib.rs`
- **Features:** Device event handling, virtual metrics
- **Key Patterns:**
  - `event_subscriptions()` and `handle_event()`
  - Device binding/unbinding
  - Automatic inference on data updates

---

## Quick Reference

### File Locations

- **Extensions**: `NeoMind-Extensions/extensions/`
- **SDK**: `NeoMind/crates/neomind-extension-sdk`
- **Install location**: `~/.neomind/extensions/`

### Essential Commands

```bash
# Dev build + auto-install
./build.sh --dev --single your-extension-v2

# Or manual build
cargo build --release -p your-extension-v2
cp target/release/libneomind_extension_*.dylib ~/.neomind/extensions/

# Build all & package
./build.sh                    # Build all, create packages
./release.sh 2.4.0            # Release with version

# View logs
neomind logs --extension your-extension-v2

# List extensions
neomind extension list

# Generate JSON files
./scripts/update-versions.sh 2.4.0
```

### Build Script Options

```bash
./build.sh                           # Build all, create packages
./build.sh --dev                     # Dev build + auto-install
./build.sh --release 2.4.0           # Release with version
./build.sh --single weather-forecast-v2  # Single extension
./build.sh --skip-frontend           # Skip frontend builds
./build.sh --debug                   # Debug build
```

### Platform Matrix

| Platform | Architecture | Binary | Target |
|----------|--------------|--------|--------|
| macOS | ARM64 | `*.dylib` | `aarch64-apple-darwin` |
| macOS | x86_64 | `*.dylib` | `x86_64-apple-darwin` |
| Linux | x86_64 | `*.so` | `x86_64-unknown-linux-gnu` |
| Linux | ARM64 | `*.so` | `aarch64-unknown-linux-gnu` |
| Windows | x86_64 | `*.dll` | `x86_64-pc-windows-msvc` |
| Windows | x86 | `*.dll` | `i686-pc-windows-msvc` |

---

## License

MIT License
