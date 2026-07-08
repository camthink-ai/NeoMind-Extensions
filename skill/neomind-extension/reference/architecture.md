# Architecture Reference

## Process Isolation Architecture

NeoMind runs each extension in its own process. The main server never loads extension
code directly — it spawns a **runner process** per extension, and the runner dlopens the
cdylib.

```
┌──────────────────────────────────────────────────────────────┐
│                    NeoMind Main Process                       │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  UnifiedExtensionService                                │  │
│  │   • spawns one runner process per loaded extension     │  │
│  │   • routes ExecuteCommand / ProduceMetrics / events    │  │
│  │   • injects capability bridge fn ptrs into runner      │  │
│  │   • restarts runner on crash (panic=unwind lets it)    │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
              │ IPC (JSON over stdin/stdout + EventPush channel)
              ▼
┌──────────────────────────────────┐      ┌──────────────────────────────────┐
│  Extension Runner Process        │      │  Extension Runner Process        │
│   • dlopens libneomind_ext_*.so │      │   • loads .wasm via wasmtime     │
│   • calls neomind_export! FFI    │      │   • same IPC protocol            │
│   • hosts Tokio runtime          │      │   • sandboxed                    │
│   • delivers events to ext       │      │                                  │
└──────────────────────────────────┘      └──────────────────────────────────┘
```

### Benefits

1. **Crash isolation** — extension panics (caught by `panic = "unwind"`) never abort the
   server. The runner can call `neomind_extension_reset_instance` to drop & recreate the
   instance.
2. **Memory isolation** — each extension has its own address space. Leaks don't poison
   the host.
3. **Independent lifecycle** — extensions can be reloaded, upgraded, or restarted without
   bouncing the platform.
4. **Capability gating** — the host injects an `invoke_capability` fn pointer via
   `neomind_extension_set_capability_bridge`. Without it, capabilities silently no-op.

## IPC protocol

JSON over stdin/stdout. The host also pushes events asynchronously via an
`EventPush` channel (not request/response).

```jsonc
// Host → Runner
{ "ExecuteCommand": { "command": "analyze", "args": {...}, "request_id": 1 } }
{ "ProduceMetrics": { "request_id": 2 } }
{ "Configure":      { "config": {...}, "request_id": 3 } }
{ "EventPush":      { "event_type": "AgentStreamChunk", "payload": {...}, "timestamp": ... } }
{ "HealthCheck":    { "request_id": 4 } }

// Runner → Host
{ "success": true, "data": {...}, "request_id": 1 }
{ "metrics": [{ "name": "x", "value": 42, "timestamp": 1709481600000 }], "request_id": 2 }
```

The runner translates these into calls on your `Extension` trait methods:
- `ExecuteCommand` → `execute_command(...).await`
- `ProduceMetrics` → `produce_metrics()`  (sync)
- `Configure` → `configure(...).await`
- `EventPush` → routed through `EventDispatcher`, which filters by
  `event_subscriptions()` and calls `handle_event(...)` (sync)
- `HealthCheck` → `health_check().await`

## ABI v3 — what `neomind_export!` generates

Every native extension must export a set of C-ABI symbols. **Never hand-write them** —
use the `neomind_export!` macro. Old ABI v2 symbols (`_create` / `_destroy`) are
deprecated and will crash the runner.

| Symbol | Direction | Purpose |
|---|---|---|
| `neomind_extension_abi_version` | → host | Returns `3` |
| `neomind_extension_metadata` | → host | Legacy C-struct metadata fast path |
| `neomind_extension_descriptor_json` | → host | Full descriptor (commands/metrics/capabilities) as JSON |
| `neomind_extension_execute_command_json` | bidir | `{"command","args"}` → `{"success","result"}` |
| `neomind_extension_produce_metrics_json` | → host | Current metric values |
| `neomind_extension_stats_json` | → host | `get_stats()` |
| `neomind_extension_health_check_json` | → host | `health_check()` |
| `neomind_extension_configure_json` | bidir | Apply config |
| `neomind_extension_event_subscriptions_json` | → host | `{"event_types": [...]}` |
| `neomind_extension_init_session_json` | bidir | Start streaming session |
| `neomind_extension_process_session_chunk_json` | bidir | Push a chunk into a session |
| `neomind_extension_close_session_json` | bidir | End a session |
| `neomind_extension_process_chunk_json` | bidir | Stateless chunk processing |
| `neomind_extension_stream_capability_json` | → host | StreamCapability descriptor |
| `neomind_extension_start_push_json` / `stop_push_json` | bidir | Push-mode lifecycle |
| `neomind_extension_reset_instance` | → host | Drop & recreate instance (panic recovery) |
| `neomind_extension_free_string` | ← host | Free a `*mut c_char` returned by any of the above |
| `neomind_extension_set_capability_bridge` | ← host | Host injects `invoke` + `free` fn ptrs |

### Legacy CExtensionMetadata (still used by the fast path)

```rust
#[repr(C)]
pub struct CExtensionMetadata {
    pub abi_version: u32,
    pub id: *const c_char,
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub author: *const c_char,
    pub metric_count: usize,
    pub command_count: usize,
}
```

## Extension lifecycle

```
1. Host spawns runner process
       ↓
2. Runner dlopens the cdylib, calls neomind_extension_set_capability_bridge
       ↓
3. neomind_extension_descriptor_json → reads metadata/commands/metrics/capabilities
       ↓
4. Runner constructs the instance (via the constructor wired by neomind_export!)
       ↓
5. init() → start()
       ↓
6. neomind_extension_event_subscriptions_json → runner tells host which events to forward
       ↓
7. Idle loop:
   • ExecuteCommand  → execute_command().await
   • ProduceMetrics  → produce_metrics()         (sync)
   • Configure       → configure().await
   • EventPush       → handle_event()            (sync)
   • HealthCheck     → health_check().await
       ↓
8. stop() → on_unload()
       ↓
9. neomind_extension_reset_instance (or process exit)
```

If a panic unwinds through any handler, the runner catches it (thanks to
`panic = "unwind"`), logs it, and either continues with the same instance (if state is
still consistent) or calls `reset_instance` to start fresh.

## Native vs WASM

| Feature | Native cdylib | WASM |
|---|---|---|
| Performance | Maximum | Good |
| Platform | Per-platform binary (6 targets) | Universal `.wasm` |
| File extension | `.dylib` / `.so` / `.dll` | `.wasm` |
| System access | Full (fs, net, GPU) | Sandboxed |
| Allowed crates | Any Rust crate | WASM-compatible only |
| Tokio runtime | Hosted by runner | Limited |
| Capabilities | Via injected fn ptrs | Via host imports |
| Typical use | ML inference, bridges, voice | Lightweight utilities |

This skill focuses on native cdylib extensions — that's what 22 of the 23 current
extensions are. WASM (see `extensions/wasm-demo/`) uses the same `Extension` trait via
the SDK's `wasm/` feature gate but is otherwise out of scope here.

## Where to look in the platform source

| File | What's there |
|---|---|
| `NeoMind/crates/neomind-extension-sdk/src/host.rs` | Extension trait, CapabilityContext, ExtensionCapability enum |
| `NeoMind/crates/neomind-extension-sdk/src/ipc_types.rs` | All IPC structs (metadata, errors, commands, params) |
| `NeoMind/crates/neomind-extension-sdk/src/macros.rs` | neomind_export!, builders, metric/log macros |
| `NeoMind/crates/neomind-extension-sdk/src/capabilities/` | Per-capability client helpers (chat, device, ...) |
| `NeoMind/crates/neomind-extension-runner/src/main.rs` | Runner process — IPC, event dispatch, `ALLOWED_CAPABILITIES` |
| `NeoMind/crates/neomind-core/src/event.rs` | `NeoMindEvent` enum incl. `AgentStreamChunk` / `AgentStreamEnd` |
