# NeoMind Extensions - User Guide

This guide explains how to find, install, and use NeoMind extensions.

**[中文指南](USER_GUIDE.zh.md)** | English Documentation

---

## Table of Contents

1. [What are Extensions?](#what-are-extensions)
2. [Finding Extensions](#finding-extensions)
3. [Installing Extensions](#installing-extensions)
4. [Using Extensions](#using-extensions)
5. [Managing Extensions](#managing-extensions)
6. [Troubleshooting](#troubleshooting)

---

## What are Extensions?

NeoMind extensions are add-ons that extend the platform's capabilities. Extensions come in **two types**:

| Type | Format | Description |
|------|--------|-------------|
| **Native** | `.dylib` / `.so` / `.dll` | Platform-specific for maximum performance |
| **WASM** | `.wasm` + `.json` | Cross-platform, single binary for all platforms |

Each extension can provide:

| Type | Description | Example |
|------|-------------|---------|
| **Metrics** | Data streams that produce values over time | Temperature, humidity, stock prices |
| **Commands** | Operations that can be executed | Query weather, send notification |
| **Tools** | Functions that AI agents can call | Fetch data, perform calculations |

---

## Finding Extensions

### Via NeoMind Web UI

1. Open NeoMind Web UI (usually `http://localhost:9375`)
2. Navigate to **Extensions** → **Marketplace**
3. Browse available extensions by category:
   - Weather
   - Data
   - Automation
   - Integration
   - Device

### Via GitHub Repository

Visit [https://github.com/camthink-ai/NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions) to see all available extensions.

---

## Installing Extensions

### Method 1: Marketplace (Recommended)

1. In NeoMind Web UI, go to **Extensions** → **Marketplace**
2. Find the extension you want
3. Click **Install**
4. Wait for download and installation to complete
5. The extension will appear in **My Extensions**

### Method 2: Manual Installation

#### Step 1: Download the Extension

Download the appropriate files for your platform from [Releases](https://github.com/camthink-ai/NeoMind-Extensions/releases):

**Native Extensions:**
| Platform | File Extension |
|----------|----------------|
| macOS | `.dylib` |
| Linux | `.so` |
| Windows | `.dll` |

**WASM Extensions:**
| File | Description |
|------|-------------|
| `.wasm` | WebAssembly module (works on all platforms) |
| `.json` | Metadata sidecar file |

#### Step 2: Install the Extension

**Native Extension:**
```bash
# Create extensions directory if it doesn't exist
mkdir -p ~/.neomind/extensions

# Copy the downloaded extension
cp ~/Downloads/libneomind_extension_*.dylib ~/.neomind/extensions/
```

**WASM Extension:**
```bash
# Create extensions directory if it doesn't exist
mkdir -p ~/.neomind/extensions

# Copy BOTH files (required!)
cp ~/Downloads/my-extension.wasm ~/.neomind/extensions/
cp ~/Downloads/my-extension.json ~/.neomind/extensions/
```

#### Step 3: Verify Installation

```bash
# List installed extensions via API
curl http://localhost:9375/api/extensions

# Or check in NeoMind Web UI under Extensions → My Extensions
```

### Method 3: Build from Source

**Native Extension:**
```bash
# Clone the repository
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions

# Build the extension
cargo build --release -p neomind-weather-forecast

# Install
mkdir -p ~/.neomind/extensions
cp target/release/libneomind_extension_weather_forecast.dylib ~/.neomind/extensions/
```

**WASM Extension (Rust):**
```bash
# Clone the repository
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions/extensions/wasm-hello

# Install WASM target (one-time setup)
rustup target add wasm32-wasi

# Build the WASM extension
cargo build --release --target wasm32-wasi

# Install (both files required!)
mkdir -p ~/.neomind/extensions
cp target/wasm32-wasi/release/wasm_hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/wasm-hello.json
```

**WASM Extension (AssemblyScript):**
```bash
# Clone the repository
git clone https://github.com/camthink-ai/NeoMind-Extensions.git
cd NeoMind-Extensions/extensions/as-hello

# Install dependencies (one-time setup)
npm install

# Build the WASM extension (~1 second!)
npm run build

# Install (both files required!)
mkdir -p ~/.neomind/extensions
cp build/as-hello.wasm ~/.neomind/extensions/
cp metadata.json ~/.neomind/extensions/as-hello.json
```

---

## Using Extensions

### Via AI Agent

Extensions automatically register their commands as tools that AI agents can use:

```
User: What's the weather in Tokyo?
Agent: [Calls query_weather tool] Currently in Tokyo: 18°C, Clear, Humidity: 45%
```

### Via API

```bash
# Execute a command
curl -X POST http://localhost:9375/api/extensions/weather-forecast/command \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{
    "command": "query_weather",
    "args": {"city": "Beijing"}
  }'
```

### Via Web UI

1. Go to **Extensions** → **My Extensions**
2. Click on an extension
3. Use the **Execute Command** button
4. Enter parameters and click **Run**

---

## Managing Extensions

### Viewing Installed Extensions

```bash
# Via API
curl http://localhost:9375/api/extensions

# Via Web UI
# Extensions → My Extensions
```

### Checking Extension Health

```bash
# Via API
curl http://localhost:9375/api/extensions/weather-forecast/health

# Response: {"healthy": true, "message": "Extension is running"}
```

### Viewing Extension Metrics

```bash
# Get current metrics from an extension
curl http://localhost:9375/api/extensions/weather-forecast/metrics
```

### Uninstalling an Extension

```bash
# Remove the extension file
rm ~/.neomind/extensions/libneomind_extension_weather_forecast.dylib

# Or via Web UI
# Extensions → My Extensions → Click extension → Uninstall
```

---

## Extension Configuration

Some extensions accept configuration:

```bash
# Configure extension via API
curl -X PUT http://localhost:9375/api/extensions/weather-forecast/config \
  -H "Content-Type: application/json" \
  -d '{
    "default_city": "Shanghai"
  }'
```

---

## Troubleshooting

### Extension Not Loading

**Problem**: Extension doesn't appear in the list

**Solutions**:
1. Check the file is in `~/.neomind/extensions/`
2. Verify the file extension matches your platform (`.dylib`, `.so`, `.dll`)
3. Check NeoMind server logs: `journalctl -u neomind -f`
4. Verify ABI version compatibility (requires NeoMind 0.5.8+)

### Extension Shows Error Status

**Problem**: Extension is in "Error" state

**Solutions**:
1. Check health endpoint: `curl /api/extensions/{id}/health`
2. View extension logs in NeoMind server logs
3. Try restarting NeoMind
4. Check if extension dependencies are met (network, API keys, etc.)

### Command Not Found

**Problem**: AI agent can't find the extension command

**Solutions**:
1. Verify extension is loaded: `curl /api/extensions`
2. Check extension's commands: `curl /api/extensions/{id}`
3. Restart the AI agent session

### Permission Denied

**Problem**: Can't copy extension to `~/.neomind/extensions/`

**Solutions**:
```bash
# Fix permissions
sudo chown -R $USER:$USER ~/.neomind/

# Or create directory first
mkdir -p ~/.neomind/extensions
```

### Wrong Architecture

**Problem**: Extension fails to load with architecture error

**Solutions**:
1. Check your system architecture: `uname -m`
2. Download the correct binary:
   - Apple Silicon Mac: `darwin-aarch64`
   - Intel Mac: `darwin-x86_64`
   - Linux PC: `linux-x86_64`
   - Windows PC: `windows-x86_64`

### WASM Extension Not Loading

**Problem**: WASM extension doesn't appear in the list

**Solutions**:
1. Ensure **both** `.wasm` and `.json` files are present
2. Verify the JSON file has the same base name as the WASM file
3. Check the JSON is valid (use `jq` or a JSON validator)
4. Verify `data_type` values: `integer`, `float`, `string`, or `boolean`

### WASM Build Fails

**Problem**: `cargo build --target wasm32-wasi` fails

**Solutions**:
1. Install WASM target: `rustup target add wasm32-wasi`
2. Some crates don't support WASM - check dependencies
3. Use minimal dependencies for WASM extensions

---

## Security Considerations

### Verified Extensions

Only install extensions from:
- Official [NeoMind-Extensions repository](https://github.com/camthink-ai/NeoMind-Extensions)
- Trusted sources

### Extension Permissions

Extensions run with these safety limits:
- **Circuit Breaker**: 5 consecutive failures → disabled
- **Timeout**: 30 seconds per command (configurable)
- **Memory**: Limited to 100MB (configurable)
- **Panic Isolation**: Crashes won't crash NeoMind

### Reviewing Extension Code

All extension source code is publicly available:
```
https://github.com/camthink-ai/NeoMind-Extensions/tree/main/extensions/{extension-name}/src/lib.rs
```

---

## Getting Help

- **Documentation**: [EXTENSION_GUIDE.md](EXTENSION_GUIDE.md) for developers
- **Issues**: [GitHub Issues](https://github.com/camthink-ai/NeoMind-Extensions/issues)
- **Community**: [Discussions](https://github.com/camthink-ai/NeoMind-Extensions/discussions)

---

## Available Extensions

### WASM Hello (Rust)

- **ID**: `wasm-hello`
- **Type**: WASM (cross-platform)
- **Language**: Rust
- **Description**: Simple WASM extension written in Rust demonstrating cross-platform compatibility
- **Commands**: `get_counter`, `increment_counter`, `get_temperature`, `get_humidity`, `hello`
- **Metrics**: `counter`, `temperature`, `humidity`

This extension works on **all platforms** without recompilation!

### WASM Hello (AssemblyScript)

- **ID**: `as-hello`
- **Type**: WASM (cross-platform)
- **Language**: AssemblyScript (TypeScript-like)
- **Description**: WASM extension written in AssemblyScript with fast compile times (~1s) and small binary size (~15 KB)
- **Commands**: `get_counter`, `increment_counter`, `reset_counter`, `get_temperature`, `set_temperature`, `get_humidity`, `hello`, `get_all_metrics`
- **Metrics**: `counter`, `temperature`, `humidity`

Ideal for JavaScript/TypeScript developers wanting to create WASM extensions!

### Weather Forecast

- **ID**: `weather-forecast`
- **Description**: Global weather data and forecasts
- **Commands**: `query_weather`, `refresh`
- **Metrics**: `temperature_c`, `humidity_percent`, `wind_speed_kmph`, `cloud_cover_percent`

### More Coming Soon

We're actively developing more extensions. Check the repository for updates!
