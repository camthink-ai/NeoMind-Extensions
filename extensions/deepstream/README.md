# DeepStream

Multi-stream RTSP video inference on NVIDIA Jetson via the NVIDIA DeepStream SDK. Decodes, infers, tracks, and analyzes up to 32 RTSP cameras on a single Jetson Orin NX, then re-publishes annotated streams as standard RTSP URLs and pushes business events (line crossing, ROI intrusion, counting) into the NeoMind EventBus.

## Features

- **Multi-stream RTSP** — 2–32 concurrent 720p/1080p cameras per Orin NX 8G
- **Hardware pipeline** — full NVDEC → TensorRT → NvDCF/NvSORT → nvdsanalytics → OSD → NVENC chain, zero-copy buffers
- **Object detection** — preset TrafficCam/YOLOv8 models, swappable via `register_model` (etlt/onnx)
- **Object tracking** — NvDCF (low light) or NvSORT (high accuracy)
- **Business analytics** — line crossing, ROI intrusion, bidirectional counting, dwell time
- **Annotated RTSP output** — `rtsp://<host>:8554/ds/<stream_id>`, h264/h265, OSD overlay
- **MJPEG preview** — browser-friendly thumbnails via `mjpeg_server.py`
- **Snapshot endpoint** — `http://<host>:8555/snapshot/<stream_id>.jpg`
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
| mediamtx | any recent release | RTSP relay for output streams |
| ffmpeg | 4.x+ | MJPEG preview + test source |

Non-Jetson hosts (macOS, x86 Linux dev boxes) can build the Rust crate but cannot run the Python sidecar — DeepStream is Jetson-only.

---

## Quick Start

This guide covers the **most common deployment**: NeoMind runs on your Mac/PC, the DeepStream sidecar runs on a Jetson via the remote bridge.

> For building the Docker image from scratch, see **[INSTALL.md](./INSTALL.md)** first.

### Step 1 — On the Jetson: start auxiliary services

Four services need to run on the Jetson. Create a start script:

```bash
#!/bin/bash
# ~/ds-deps/start_all.sh
set -e
cd ~/ds-deps

# 1. mediamtx — RTSP relay (receives DeepStream output + serves RTSP to clients)
pkill -x mediamtx 2>/dev/null || true
./mediamtx > /tmp/mediamtx.log 2>&1 &
sleep 1

# 2. ffmpeg test source — loop a sample video as an RTSP stream
#    (replace sample.mp4 with your own video, or use an IP camera URL instead)
pkill -f "ffmpeg.*stream_loop" 2>/dev/null || true
ffmpeg -re -stream_loop -1 -i sample.mp4 -c copy \
    -f rtsp -rtsp_transport tcp rtsp://127.0.0.1:8554/sample \
    > /tmp/ffmpeg.log 2>&1 &
sleep 2

# 3. MJPEG preview server — converts RTSP output to Motion-JPEG for browser thumbnails
pkill -f mjpeg_server 2>/dev/null || true
python3 sidecar/mjpeg_server.py --port 8090 > /tmp/mjpeg_server.log 2>&1 &

# 4. Sidecar bridge — TCP relay between NeoMind and the Docker sidecar
export SIDECAR_SPAWN_CMD="docker run --rm -i --runtime=nvidia --network=host \
    -v ~/ds-deps/sidecar:/srv/sidecar \
    -v ~/ds-engines:/engines \
    ds:7.1-pyds-gi \
    python3 -u /srv/sidecar/deepstream_runner.py"
pkill -f sidecar_bridge 2>/dev/null || true
cd ~/ds-deps/sidecar
python3 -u sidecar_bridge.py --port 9556 --log-level debug > bridge.log 2>&1 &

echo "All services started. Logs in /tmp/"
echo "mediamtx      : RTSP  :8554"
echo "MJPEG preview : HTTP  :8090"
echo "Sidecar bridge: TCP   :9556"
echo "Snapshot      : HTTP  :8555 (inside Docker, started on demand)"
```

Verify:

```bash
# RTSP source works
ffprobe -v error rtsp://127.0.0.1:8554/sample  # should print "h264"

# Bridge is listening
ss -tlnp | grep 9556

# MJPEG responds (will be 200 even if no DeepStream stream exists yet)
curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8090/mjpeg/test
```

### Step 2 — Copy sidecar code to the Jetson

```bash
# From the NeoMind-Extensions repo root, copy sidecar scripts to the Jetson
scp -r extensions/deepstream/sidecar box@<jetson-ip>:~/ds-deps/sidecar
```

This includes `deepstream_runner.py` (the sidecar), `sidecar_bridge.py` (the TCP bridge), and `mjpeg_server.py` (the preview server).

### Step 3 — Build & install the extension

```bash
# On your Mac/PC (from the NeoMind-Extensions repo root)
./build.sh --dev --single deepstream
# This builds the Rust dylib, frontend UMD bundle, and auto-installs to NeoMind
```

Restart NeoMind (or reload the dashboard) so the extension loads.

### Step 4 — Configure the extension in NeoMind

Open the NeoMind dashboard → Settings → Extensions → DeepStream, and set:

| Parameter | Value | Example |
|-----------|-------|---------|
| `sidecar_mode` | `remote` | — |
| `sidecar_host` | Jetson's IP | `192.168.93.20` |
| `sidecar_port` | `9556` | — |
| `server_host` | Jetson's IP (same as sidecar_host) | `192.168.93.20` |
| `rtsp_port` | `8554` | — |
| `snapshot_port` | `8555` | — |

> `server_host` is used by the frontend to build MJPEG/snapshot URLs. It must be the Jetson's IP as seen from the browser.

### Step 5 — Add a stream

In the dashboard, click **Add** on the DeepStream card and enter:

| Field | Value |
|-------|-------|
| Stream ID | `cam1` (any short alphanumeric name) |
| Source URL | `rtsp://127.0.0.1:8554/sample` |
| Model | `Primary_Detector` (default) |

> **Important:** the source URL must be reachable **from inside the Docker container**. Since the container uses `--network=host`, `127.0.0.1` refers to the Jetson itself. For IP cameras, use the camera's RTSP URL directly (e.g. `rtsp://admin:pass@10.0.0.10/Streaming/Channels/101`).

Within ~5 seconds you should see:
- The thumbnail tile switch from black to live video with bounding boxes
- Stats chips (FPS, GPU, stream count) update every 5s
- Detection events appear in the EventBus

### Troubleshooting

| Symptom | Check |
|---------|-------|
| Black thumbnail | Source RTSP URL wrong/unreachable → `frame_count=0` in stats. Verify with `ffprobe rtsp://127.0.0.1:8554/sample` on the Jetson |
| "Broken pipe" on add_stream | Bridge died or rejected duplicate connection. Restart bridge: `pkill -f sidecar_bridge && ~/ds-deps/start_all.sh` |
| Stats show "—" | Extension not connected to sidecar. Check bridge log: `tail ~/ds-deps/sidecar/bridge.log` |
| FPS = 0 but status = playing | Source RTSP URL is valid but unreachable from Docker. Check firewall / use `--network=host` |
| Dashboard can't reach MJPEG | `server_host` config wrong. Verify: `curl http://<jetson-ip>:8090/mjpeg/<stream_id>` from your browser's machine |

---

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
│          └─ TCP :9556 ─────────────────┐                    │
│                                        ▼                    │
│                           sidecar_bridge.py                  │
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

### Service Topology

```
┌─────────────────────┐         ┌──────────────────────────────────────┐
│ Browser (dashboard) │         │ Jetson Orin NX                       │
│                     │         │                                      │
│  ManagerCard        │─MJPEG──→│  mjpeg_server.py :8090               │
│   ├─ StatsBar       │─HTTP───→│  (ffmpeg RTSP→MJPEG conversion)      │
│   ├─ StreamGrid     │         │                                      │
│   └─ DetailDrawer   │─HLS/RTSP│  mediamtx :8554                      │
│                     │────────→│  ├─ input:  ffmpeg test source       │
│                     │         │  └─ output: ds/<id> from DeepStream  │
│  NeoMind API        │─REST───→│                                      │
│                     │         │  sidecar_bridge.py :9556             │
│  extension-runner   │─TCP────→│  └─ Docker: ds:7.1-pyds-gi           │
│   └─ deepstream.so  │ JSONL   │      └─ deepstream_runner.py         │
│                     │         │          ├─ DeepStream pipeline      │
│                     │         │          ├─ snapshot_server :8555    │
│                     │         │          └─ RTSP sink → mediamtx     │
└─────────────────────┘         └──────────────────────────────────────┘
```

### Auxiliary Services Explained

| Service | Port | Purpose |
|---------|------|---------|
| **mediamtx** | 8554 (RTSP) | RTSP relay. Receives DeepStream annotated output via `rtspclientsink`, serves it to MJPEG server and external RTSP clients |
| **mjpeg_server.py** | 8090 (HTTP) | Converts RTSP → Motion-JPEG for browser `<img>` tags. One ffmpeg per client |
| **sidecar_bridge.py** | 9556 (TCP) | Pipes JSONL between NeoMind and the Docker sidecar. Spawns/kills Docker container on connect/disconnect |
| **snapshot_server** | 8555 (HTTP) | On-demand JPEG snapshots. Runs inside the Docker container, started by `deepstream_runner.py` |
| **ffmpeg test source** | — | Loops `sample.mp4` to `rtsp://127.0.0.1:8554/sample` for testing. Replace with real cameras in production |

---

## Deployment Topology

The extension supports two transport modes for the sidecar:

### Mode A — Local (NeoMind runs on the Jetson itself)

Everything runs on the Jetson. The Rust extension spawns the Python sidecar as a child process communicating over stdin/stdout JSONL.

### Mode B — Remote bridge (NeoMind off-device, recommended for Mac/PC users)

NeoMind runs on your Mac/PC. Only the sidecar + bridge run on the Jetson. Set `sidecar_mode = "remote"` in the extension config and install `sidecar/sidecar_bridge.py` as a daemon on the Jetson.

The bridge daemon is **Python 3 stdlib only** — no pip install needed on the Jetson.

```bash
# On the Jetson
export SIDECAR_SPAWN_CMD="docker run --rm -i --runtime=nvidia --network=host \
    -v ~/ds-deps/sidecar:/srv/sidecar \
    -v ~/ds-engines:/engines \
    ds:7.1-pyds-gi \
    python3 -u /srv/sidecar/deepstream_runner.py"
python3 sidecar/sidecar_bridge.py --port 9556
```

### Choosing between modes

| Need | Mode |
|------|------|
| All-in-one Jetson appliance, NeoMind runs locally | `local` (default) |
| NeoMind runs on your Mac / a server, sidecar on a LAN Jetson | `remote` |
| Multi-tenant — multiple NeoMind instances sharing one Jetson | not supported (bridge accepts one client) |

---

## Installation

```bash
# Build from repository root. Builds on any host; RUNTIME requires Jetson.
./build.sh --single deepstream

# Or build with Cargo directly
cargo build --release -p deepstream

# Dev build + auto-install to NeoMind extensions directory
./build.sh --dev --single deepstream
```

> 📺 **Deploying DeepStream 7.1 + sidecar from scratch?** See **[INSTALL.md](./INSTALL.md)** — 16 gotchas encountered on CamThink NG4500 hardware (Docker vfs / NGC auth / pyds wheel / INT8 memory / IPv6 RTSP / TRT 10.x tensor names, etc.).

---

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
  "model": "Primary_Detector"
}
```

All other fields (tracker, analytics, output encoder, event rate) default to documented values.

---

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

---

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

---

## Sidecar Protocol

JSON Lines over the child process's stdin/stdout (4 MiB per-line cap). The protocol is documented in `src/protocol.rs` and consists of:

- **8 control messages** (Rust → Python): `Hello`, `AddStream`, `RemoveStream`, `UpdateAnalytics`, `SetThreshold`, `ListState`, `HealthCheck`, `Shutdown`
- **13 event variants** (Python → Rust): `Ready`, `HelloAck`, `StreamAdded`, `StreamRemoved`, `StreamError`, `Detection`, `LineCross`, `ROIIntrusion`, `AnalyticsSnapshot`, `Stats`, `Pong`, `ErrorResponse`, `Bye`

Reliability features:

- **Handshake** — Python emits `ready` on boot; Rust replies with `hello`; Python acks. 10s timeout on either side.
- **Heartbeat** — Rust sends `health_check` every 10s, expects `pong` within 5s. Pongs arrive on a dedicated priority channel so they cannot be starved by event floods.
- **Crash recovery** — supervisor respawns with backoff `[1, 2, 5, 10, 30]`s capped at 30s. Sliding window of 5 restarts per 60s; beyond that, marks `Failed` and waits for manual `restart_sidecar`.
- **Graceful shutdown** — `shutdown {graceful_secs: 5}` → wait for `bye` (5s) → close stdin → SIGTERM → wait 2s → SIGKILL.

---

## Configuration

The extension declares config parameters via `with_config_parameters()` in its SDK metadata. NeoMind's host applies them through the `configure()` lifecycle hook (typically from the dashboard's extension settings dialog). The first six are sent to the sidecar via the `Hello` handshake message on every spawn; the last four shape the transport / frontend URLs and never reach the sidecar process itself.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `rtsp_port` | integer | `8554` | RTSP server port for annotated output streams (1–65535) |
| `snapshot_port` | integer | `8555` | HTTP port for snapshot JPEGs (1–65535) |
| `snapshot_bind_addr` | string | `0.0.0.0` | Bind address for the snapshot HTTP server |
| `log_level` | enum | `info` | Sidecar Python log level (`debug` / `info` / `warning` / `error`) |
| `models_dir` | string | `/opt/nvidia/deepstream/deepstream/samples/models` | Where preset + user-registered model files live |
| `max_streams` | integer | `32` | Hard cap on concurrent streams (1–64) |
| `server_host` | string | _(empty)_ | Frontend-facing Jetson IP for building MJPEG/snapshot/RTSP URLs. Empty = derive from dashboard hostname |
| `sidecar_mode` | enum | `local` | `local` (child process) or `remote` (TCP to bridge daemon) |
| `sidecar_host` | string | _(empty)_ | When `remote`: IP of the Jetson running `sidecar_bridge.py` |
| `sidecar_port` | integer | `9556` | When `remote`: TCP port of the bridge daemon |

Notes:
- Changing config on an already-running sidecar has no immediate effect — changes take effect on the **next spawn**. Call `restart_sidecar` to force a respawn.
- `server_host` takes effect immediately — surfaced to the frontend on the next poll (≤3s), no restart needed.
- When switching from `local` → `remote`, start `sidecar_bridge.py` on the Jetson **first**.

---

## Development Status

**Production-ready.** Verified end-to-end on Jetson Orin NX 8G (2026-07-08):

- **RTSP source pipeline** — NVDEC → TensorRT (FP16) → NvDCF tracker → nvdsanalytics → OSD → NVENC → `rtspclientsink` to mediamtx
- **Detection events** — ~1000 events/60s with rich object data (class, track_id, confidence, bbox)
- **Stats events** — per-stream FPS / frame_count / object_count every 5s
- **Snapshot endpoint** — on-demand GStreamer one-shot pipeline, ~8s latency, 1920×1080 JPEG
- **Annotated RTSP output** — `rtsp://<jetson>:8554/ds/<stream_id>`
- **MJPEG preview** — live thumbnails in dashboard grid via `mjpeg_server.py`
- **Frontend** — ManagerCard with stats bar, stream grid, detail drawer
- **SidecarSupervisor** — crash recovery with backoff `[1, 2, 5, 10, 30]`s + sliding-window rate limit
- **restart_sidecar** — full manual recovery: shutdown → respawn → replay all active stream configs

---

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
| `tests/remote_bridge_test.rs` | Remote bridge TCP connection + handshake |
| `tests/command_test.rs` | Command routing + stream manager |
| `tests/replay_test.rs` | Crash-recovery stream replay |

---

## License

Apache-2.0
