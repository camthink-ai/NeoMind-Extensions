# Quick Start Example: Using the Claude Code Skill

This guide shows you how to use the NeoMind Extension Development skill in Claude Code.

## Installation

```bash
cd NeoMind-Extension
./install-skill.sh
```

## Example 1: Create a New Extension

**What you type in Claude Code:**
```
Create a temperature monitoring extension for NeoMind that reads CPU temperature
```

**What Claude will provide:**

1. **Project Setup**
```bash
cp -r extensions/weather-forecast-v2 extensions/temperature-monitor-v2
cd extensions/temperature-monitor-v2
```

2. **Cargo.toml Configuration**
```toml
[package]
name = "temperature-monitor-v2"
version = "2.0.0"
edition = "2021"

[lib]
name = "neomind_extension_temperature_monitor_v2"
crate-type = ["cdylib", "rlib"]

[dependencies]
neomind-extension-sdk = { path = "../../../NeoMind/crates/neomind-extension-sdk" }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = "0.1"
tokio = { version = "1", features = ["rt", "sync"] }
semver = "1"
chrono = "0.4"
sysinfo = "0.30"  # For reading CPU temperature
```

If this extension lives inside a Cargo workspace, define `[profile.release]` only in the workspace root `Cargo.toml`.

3. **Complete Rust Implementation**
```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;
use sysinfo::{System, SystemExt, ComponentExt};
use std::sync::Mutex;

pub struct TemperatureMonitorExtension {
    system: Mutex<System>,
}

impl TemperatureMonitorExtension {
    pub fn new() -> Self {
        Self {
            system: Mutex::new(System::new_all()),
        }
    }
}

#[async_trait]
impl Extension for TemperatureMonitorExtension {
    // ... complete implementation provided by Claude
}

neomind_extension_sdk::neomind_export!(TemperatureMonitorExtension);
```

4. **Build & Install Commands**
```bash
cargo build --release
cp target/release/libneomind_extension_temperature_monitor_v2.dylib ~/.neomind/extensions/
```

## Example 2: Add a Frontend Component

**What you type:**
```
Add a frontend component that displays the temperature with a gauge
```

**What Claude will provide:**

1. **React Component with Gauge**
```tsx
import { forwardRef, useState, useEffect } from 'react'

export const TemperatureGauge = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function TemperatureGauge(props, ref) {
    const [temp, setTemp] = useState(0)

    // Complete component implementation with:
    // - Circular gauge visualization
    // - Color coding (green/yellow/red)
    // - Real-time updates
    // - Error handling

    return <div>...</div>
  }
)
```

2. **Build Configuration**
```json
{
  "id": "temperature-monitor-v2",
  "components": [{
    "name": "TemperatureGauge",
    "type": "card",
    "displayName": "CPU Temperature Gauge"
  }]
}
```

3. **Build & Install**
```bash
cd frontend
npm install
npm run build
cp -r dist ~/.neomind/extensions/temperature-monitor-v2/frontend/
```

## Example 3: Troubleshooting

**What you type:**
```
My extension crashes with "panic in extension" error
```

**What Claude will provide:**

1. **Diagnosis Steps**
```bash
# Check panic configuration
grep "panic" Cargo.toml

# Should see:
# panic = "unwind"  # ✓ Correct
# If you see panic = "abort", that's the problem!
```

2. **Common Fixes**
- Check for `.unwrap()` calls → Replace with `?` operator
- Verify async/sync rules (no `.await` in `produce_metrics`)
- Add proper error handling

3. **Testing Commands**
```bash
# Test the extension
curl -X POST http://localhost:9375/api/extensions/temperature-monitor-v2/command \
  -H "Content-Type: application/json" \
  -d '{"command": "get_temperature", "args": {}}'
```

## Example 4: Understanding Architecture

**What you type:**
```
How does process isolation work in NeoMind extensions?
```

**What Claude will provide:**

Detailed explanation with diagrams:
```
┌─────────────────────────────┐
│   NeoMind Main Process      │
│   - Stable and protected    │
└─────────────┬───────────────┘
              │ IPC (JSON)
              ▼
┌─────────────────────────────┐
│  Extension Process          │
│  - Your extension runs here │
│  - Crashes are isolated     │
└─────────────────────────────┘
```

Plus:
- How IPC communication works
- What happens when extension crashes
- How to configure resource limits

## Example 5: Quick Reference

**What you type:**
```
Show me the minimal code for a NeoMind extension
```

**What Claude will provide:**

Complete minimal template:
```rust
use async_trait::async_trait;
use neomind_extension_sdk::prelude::*;

pub struct MinimalExtension;

#[async_trait]
impl Extension for MinimalExtension {
    fn metadata(&self) -> &ExtensionMetadata { /* ... */ }
    fn metrics(&self) -> &[MetricDescriptor] { /* ... */ }
    fn commands(&self) -> &[ExtensionCommand] { /* ... */ }
    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> { /* ... */ }
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { /* ... */ }
}

neomind_extension_sdk::neomind_export!(MinimalExtension);
```

## Tips for Best Results

### 1. Be Specific
✅ **Good**: "Create a weather extension that fetches data from OpenWeatherMap API"
❌ **Bad**: "Make an extension"

### 2. Ask Follow-up Questions
- "How do I add error handling to this?"
- "Can you add logging to this extension?"
- "What's the best way to cache API responses?"

### 3. Request Examples
- "Show me an example of using async HTTP requests"
- "Give me an example of using atomic counters for metrics"

### 4. Get Architecture Guidance
- "Should I use a separate thread for video processing?"
- "How do I handle long-running operations?"
- "What's the best way to manage state?"

## Common Workflows

### Starting a New Extension
```
1. "Create a [type] extension that [does X]"
2. Review and customize the generated code
3. "Add [feature Y] to this extension"
4. Build and test
5. "Create a frontend component for this"
```

### Debugging Issues
```
1. Describe the error or behavior
2. Claude analyzes and suggests fixes
3. "Show me how to add better error handling"
4. Test the fixes
5. "How do I add logging for debugging?"
```

### Learning Extension Development
```
1. "Explain how [concept] works in NeoMind extensions"
2. "Show me an example of [pattern]"
3. "What are the best practices for [task]?"
4. "Compare [approach A] vs [approach B]"
```

## Skill Features You Get

✅ **Automatic Context**: Claude knows about:
- NeoMind architecture
- extension runtime API
- Process isolation model
- runtime protocol v3 requirements
- Best practices and patterns

✅ **Code Generation**:
- Complete extension implementations
- Frontend components
- Configuration files
- Build scripts

✅ **Documentation**:
- Architecture explanations
- API references
- Working examples
- Troubleshooting guides

✅ **Interactive Help**:
- Answer questions
- Suggest improvements
- Debug problems
- Optimize code

## Next Steps

1. **Install the skill** (if not already done)
   ```bash
   ./install-skill.sh
   ```

2. **Start Claude Code** in your project

3. **Try asking**:
   - "How do I create a NeoMind extension?"
   - "Show me an example extension"
   - "Create a [your idea] extension"

4. **Explore documentation**:
   - Read SKILL_GUIDE.md for complete details
   - Check reference/ for deep dives
   - Review examples/ for working code

## Feedback

Found an issue or have suggestions? The skill is open source and located at:
```
.claude/skills/neomind-extension/
```

Edit the files and reinstall to see changes immediately!

---

Happy coding with AI assistance! 🚀
