# DeepStream

Multi-stream RTSP video inference on NVIDIA Jetson via the NVIDIA DeepStream SDK. Decodes, infers, tracks, and analyzes up to 32 RTSP cameras on a single Jetson Orin NX, then re-publishes annotated streams as standard RTSP URLs and pushes business events (line crossing, ROI intrusion, counting) into the NeoMind EventBus.

## Features

- **Multi-stream RTSP** — 2–32 concurrent 720p/1080p cameras per Orin NX 8G
- **Hardware pipeline** — full NVDEC → TensorRT → NvDCF/NvSORT → nvdsanalytics → OSD → NVENC chain, zero-copy buffers
- **Object detection** — preset YOLOv8n/s models, swappable via `register_model` (etlt/onnx)
- **Object tracking** — NvDCF (low light) or NvSORT (high accuracy)
- **Business analytics** — line crossing, ROI intrusion, bidirectional counting, dwell time
- **Annotated RTSP output** — `rtsp://<host>:8554/ds/<stream_id>`, h264/h265, OSD overlay
- **Snapshot endpoint** — `http://<host>:8555/snapshot/<stream_id>.jpg`, MJPEG low-fps
- **Event-driven** — Detection / LineCross / ROIIntrusion / AnalyticsSnapshot published to NeoMind EventBus with per-stream rate limiting
- **Crash resilient** — sidecar supervisor with sliding-window backoff (1s→2s→5s→10s→30s) and graceful shutdown sequence (bye → SIGTERM → SIGKILL)
- **Heartbeat watchdog** — 10s ping + 5s pong window on a dedicated priority channel so event floods cannot starve it

## Requirements

| Component | Minimum | Notes |
|-----------|---------|-------|
| Platform | Jetson Orin NX / Nano / AGX | JetPack 6.x (BSP kernel + L4T) |
| DeepStream SDK | 7.0+ (validated on 7.1) | `dpkg -l | grep deepstream` |
| Python bindings | `pyds >= 1.1.0` | ships with DeepStream SDK |
| GStreamer plugins | `nvvideo4linux2`, `nvv4l2decoder`, `nvv4l2h264enc`, `nvtracker` | `gst-inspect-1.0` |
| NeoMind | host build with `neomind-extension-sdk 0.6+` | |

Non-Jetson hosts (macOS, x86 Linux dev boxes) can build the Rust crate but cannot run the Python sidecar — DeepStream is Jetson-only.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ NeoMind Server (host)                                       │
│  └─ extension-runner (isolated process)                     │
│      └─ libneomind_extension_deepstream.{dylib,so} (Rust)    │
│          │                                                  │
│          ├─ Extension trait impl                            │
│          ├─ StreamManager (per-stream state, authoritative)  │
│          ├─ SidecarSupervisor (spawn + restart + shutdown)   │
│          ├─ EventBus publisher                              │
│          │                                                  │
│          └─ spawn ─────────────────┐                        │
│                                    ▼                        │
│                           deepstream_runner.py               │
│                          (single sidecar instance)           │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  stdin  ← JSONL control (add/del/update/shutdown)    │   │
│  │  stdout → JSONL event stream (Detection, LineCross…) │   │
│  │  stderr → raw logs (Rust forwards as ext_debug!)     │   │
│  │  HTTP   → /snapshot/{stream_id}.jpg on port 8555     │   │
│  │  RTSP   → rtsp://host:8554/ds/{stream_id}            │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

The Rust side is the **authoritative state holder** — stream configs, ordering, and supervisor state live in Rust memory. The Python sidecar is ephemeral: if it crashes, Rust respawns it and replays all stored configs. This design choice avoids NVIDIA's `pyds`/GStreamer FFI surface in unsafe Rust and aligns with NVIDIA's official Python sample path.

## Deployment Topology

```
┌──────────────────────────────┐        ┌──────────────────────────────────┐
│ User's machine (browser)     │        │ Jetson Orin NX                   │
│  └─ NeoMind dashboard        │ ──HTTP─│  ├─ NeoMind server + API         │
│     - commands via /api      │  POST  │  │   └─ extension-runner          │
│     - snapshots via :8555    │        │  │       └─ deepstream.{so,dylib}│
│     - RTSP via rtsp://:8554  │ ──HTTP─│  │           └─ deepstream_runner│
│                              │  GET   │  │               .py (sidecar)    │
└──────────────────────────────┘        │  ├─ snapshot HTTP :8555          │
                                        │  └─ RTSP        :8554            │
                                        └──────────────────────────────────┘
```

**The extension MUST be installed on the Jetson itself** — the Rust crate spawns the Python sidecar as a **local child process** communicating over stdin/stdout JSONL (not a network socket). There is no way to run the extension on a separate host and point it at a remote sidecar.

The dashboard, however, can run anywhere: commands (`add_stream`, `list_streams`, …) travel over the NeoMind REST API, while media (snapshots, annotated RTSP output) is fetched directly from the Jetson's `:8555` / `:8554` ports. Set the `serverHost` field in the frontend card config to the Jetson's IP when the dashboard is not on the Jetson itself.

## Installation

```bash
# Build from repository root. Builds on any host; RUNTIME requires Jetson.
./build.sh --single deepstream

# Or build with Cargo directly
cargo build --release -p deepstream

# Dev build + auto-install to ~/.neomind/extensions/ (on the Jetson)
./build.sh --dev --single deepstream
```

> 📺 **Deploying DeepStream 7.1 + sidecar from scratch?** See **[INSTALL.md](./INSTALL.md)** — 16 gotchas encountered on CamThink NG4500 hardware (Docker vfs / NGC auth / pyds wheel / INT8 memory / IPv6 RTSP / TRT 10.x tensor names, etc.).

## Commands

| Command | Frequency | Purpose |
|---------|-----------|---------|
| `add_stream` | Medium | Add an RTSP/file/camera source with model + analytics config |
| `remove_stream` | Medium | Stop and remove a stream |
| `list_streams` | High | Return all streams with current status |
| `get_stream_info` | High | Detailed config + live stats for one stream |
| `update_analytics` | Medium | Hot-update line-crossing rules / ROI polygons (no pipeline restart) |
| `set_threshold` | Low | Hot-update confidence / IoU thresholds |
| `list_models` | Low | List preset + user-registered models |
| `register_model` | Low | Register a user model (etlt/onnx + labels + shape) |
| `restart_sidecar` | Rare | Manual recovery when auto-restart exhausted |
| `diagnose` | Rare | One-shot diagnostic dump for support tickets |

### Minimal `add_stream`

```json
{
  "stream_id": "cam_front",
  "source": {"type": "rtsp", "url": "rtsp://admin:pass@10.0.0.10/Streaming/Channels/101"},
  "model": "yolov8n-coco"
}
```

All other fields (tracker, analytics, output encoder, event rate) default to documented values. See the design spec for the full payload schema.

## Metrics

### Global (7)

| Metric | Type | Unit |
|--------|------|------|
| `active_stream_count` | int | count |
| `total_throughput_fps` | float | fps |
| `gpu_utilization_percent` | float | % |
| `gpu_memory_used_mb` | float | MB |
| `sidecar_status` | string | — |
| `sidecar_uptime_secs` | int | s |
| `restart_count` | int | count |

### Per-stream (9, dynamic, named `<base>.<stream_id>`)

| Base metric | Type | Unit |
|-------------|------|------|
| `stream_fps` | float | fps |
| `stream_latency_ms` | float | ms |
| `stream_status` | string | — |
| `stream_detection_count` | int | count |
| `stream_person_count` | int | count |
| `stream_vehicle_count` | int | count |
| `stream_line_cross_events` | int | count |
| `stream_roi_intrusion_events` | int | count |
| `stream_error_count` | int | count |

## Events

All events publish to the NeoMind EventBus.

| Event type | Trigger | Priority |
|------------|---------|----------|
| `DeepStreamReady` | sidecar booted | high |
| `StreamAdded` | RTSP URL available | high |
| `StreamRemoved` | cleanup done | high |
| `StreamError` | decode/connect failure | high |
| `Detection` | rate-limited snapshot (default 1 Hz/stream) | low |
| `LineCross` | line crossing detected | high |
| `ROIIntrusion` | ROI entry/exit | high |
| `AnalyticsSnapshot` | 5s periodic counting/dwell | medium |
| `StreamStalled` | FPS below threshold for N seconds | medium |
| `SidecarCrashed` | watchdog triggered | high |

**Rate limiting** keeps the EventBus healthy at scale: per-frame detection × 30fps × 32 streams would otherwise produce 960 events/sec. Detection defaults to 1 Hz per stream; LineCross/ROIIntrusion are always emitted but deduplicated by `track_id` within a 3-second window; AnalyticsSnapshot fires every 5s. Worst-case ceiling: < 200 events/sec.

## Sidecar Protocol

JSON Lines over the child process's stdin/stdout (4 MiB per-line cap). The protocol is documented in `src/protocol.rs` and consists of:

- **8 control messages** (Rust → Python): `Hello`, `AddStream`, `RemoveStream`, `UpdateAnalytics`, `SetThreshold`, `ListState`, `HealthCheck`, `Shutdown`
- **13 event variants** (Python → Rust): `Ready`, `HelloAck`, `StreamAdded`, `StreamRemoved`, `StreamError`, `Detection`, `LineCross`, `ROIIntrusion`, `AnalyticsSnapshot`, `Stats`, `Pong`, `ErrorResponse`, `Bye`

Reliability features:

- **Handshake** — Python emits `ready` on boot; Rust replies with `hello`; Python acks. 10s timeout on either side.
- **Heartbeat** — Rust sends `health_check` every 10s, expects `pong` within 5s. Pongs arrive on a dedicated priority channel so they cannot be starved by event floods.
- **Crash recovery** — supervisor respawns with backoff `[1, 2, 5, 10, 30]`s capped at 30s. Sliding window of 5 restarts per 60s; beyond that, marks `Failed` and waits for manual `restart_sidecar`.
- **Graceful shutdown** — `shutdown {graceful_secs: 5}` → wait for `bye` (5s) → close stdin → SIGTERM → wait 2s → SIGKILL.

## Configuration

Defaults work for most deployments. Override via `~/.neomind/extensions/deepstream.toml` (loaded by NeoMind host config):

| Key | Default | Description |
|------|---------|-------------|
| `max_streams` | 32 | Hard cap on concurrent streams; Python enforces with GPU memory check |
| `rtsp_port` | 8554 | RTSP server port (annotated output) |
| `snapshot_port` | 8555 | HTTP port for snapshot JPEGs |
| `python_bin` | auto-detected | Interpreter that has `pyds` installed |
| `models_dir` | `./models` | Where preset + user-registered model files live |
| `log_level` | `info` | Sidecar log level |

## Development Status

**Production-ready.** Verified end-to-end on Jetson Orin NX 8G (2026-07-08):

- **RTSP source pipeline** — NVDEC → TensorRT (FP16) → NvDCF tracker → nvdsanalytics → OSD → NVENC → `rtspclientsink` to mediamtx
- **Detection events** — ~1000 events/60s with rich object data (class, track_id, confidence, bbox)
- **Stats events** — per-stream FPS / frame_count / object_count every 5s
- **Snapshot endpoint** — on-demand GStreamer one-shot pipeline, ~8s latency, 1920×1080 JPEG
- **Annotated RTSP output** — `rtsp://<jetson>:8554/ds/<stream_id>`
- **Frontend** — 3 card components: system stats, stream overview grid, single-stream detail
- **SidecarSupervisor** — crash recovery with backoff `[1, 2, 5, 10, 30]`s + sliding-window rate limit (5 restarts/60s)
- **restart_sidecar** — full manual recovery: shutdown → respawn → replay all active stream configs
- **Crash-recovery replay** — `StreamManager::replay_to()` re-sends AddStream for all non-Stopped streams after a respawn

See `docs/superpowers/specs/2026-07-06-deepstream-extension-design.md` for the design spec and `sidecar/STATUS.md` for the sidecar implementation log.

## Testing

```bash
cargo test -p deepstream
```

Tests are split into unit tests (protocol, lib) and integration tests that spawn a mock sidecar (`tests/mock_sidecar.py`, pure stdlib, no DeepStream dependency) so they run on any host — including macOS dev machines.

| Test file | What it covers |
|-----------|----------------|
| `src/lib.rs` (unit) | Extension metadata, panic-unwind invariant |
| `src/protocol.rs` (unit) | JSONL serialize/parse, 4 MiB cap, round-trip |
| `tests/sidecar_supervisor_test.rs` | Spawn + handshake through real pipes |
| `tests/heartbeat_test.rs` | 10s ping / 5s pong window / timeout (uses tokio mock clock) |
| `tests/supervisor_test.rs` | Crash respawn + backoff escalation (real wall clock) |
| `tests/shutdown_test.rs` | bye → SIGTERM → SIGKILL escalation |

## License

Apache-2.0
