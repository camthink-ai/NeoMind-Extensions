# Template Extension (Native)

This is a minimal template extension for NeoMind using the **Native** extension type (platform-specific dynamic library).

> **Note**: NeoMind supports two extension types:
> - **Native** (.dylib/.so/.dll): This template - maximum performance, platform-specific
> - **WASM** (.wasm + .json): Cross-platform, single binary for all platforms
>
> For cross-platform extensions, use the `wasm-hello` template instead.

## How to Use This Template

1. **Copy the template folder:**
   ```bash
   cd ~/NeoMind-Extension/extensions
   cp -r template my-extension
   cd my-extension
   ```

2. **Update `Cargo.toml`:**
   ```toml
   [package]
   name = "neomind-my-extension"  # Change this
   description = "My custom extension"  # Change this
   ```

3. **Update `src/lib.rs`:**
   - Change the extension ID (e.g., `"com.example.template"` → `"com.mycompany.myextension"`)
   - Change metadata (name, description, author, homepage)
   - Implement your extension logic in `execute_command()`
   - Define your metrics in the `METRICS` static
   - Define your commands in the `COMMANDS` static
   - Update the `metric_count` and `command_count` in `neomind_extension_metadata()`

4. **Build and test:**
   ```bash
   cargo build --release
   cargo test
   ```

5. **Install:**
   ```bash
   cp target/release/libneomind_extension_my_extension.* ~/.neomind/extensions/
   ```

## Extension Structure

```
my-extension/
├── Cargo.toml          # Package configuration
├── README.md           # Extension documentation
└── src/
    └── lib.rs          # Extension implementation
```

## Key Components

### 1. State
```rust
struct MyState {
    // Your internal state here
}
```

### 2. Static Metrics
```rust
static METRICS: Lazy<[MetricDescriptor; N]> = Lazy::new(|| [
    MetricDescriptor {
        name: "my_metric".to_string(),
        display_name: "My Metric".to_string(),
        // ...
    },
    // ... more metrics
]);
```

### 3. Static Commands
```rust
static COMMANDS: Lazy<[ExtensionCommand; N]> = Lazy::new(|| [
    ExtensionCommand {
        name: "my_command".to_string(),
        display_name: "My Command".to_string(),
        // ...
    },
    // ... more commands
]);
```

### 4. Command Execution
```rust
async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
    match command {
        "my_command" => {
            // Your logic here
            Ok(json!({"result": "success"}))
        }
        _ => Err(ExtensionError::CommandNotFound(command.to_string())),
    }
}
```

## Further Reading

See [EXTENSION_GUIDE.md](../../EXTENSION_GUIDE.md) for complete documentation, including:
- Native extension development (this template)
- WASM extension development (see `../wasm-hello/`)
- API reference and best practices

## Alternative: WASM Template

For cross-platform extensions that work on all platforms without recompilation, see the WASM template:

```bash
cp -r ../wasm-hello my-wasm-extension
```

WASM extensions offer:
- Single binary for all platforms
- Sandboxed execution
- Easier distribution

## License

MIT
