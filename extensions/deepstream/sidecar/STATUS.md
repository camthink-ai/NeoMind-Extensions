# DeepStream Sidecar — Status

Phase 8.2–8.6b draft. Code written on macOS; **smoke testing happens on
Jetson Orin NX 8G in the `ds:7.1-pyds` container.** No tests have been
run yet — see "Verification" at the bottom for what passed locally.

## Module status

| Module                | Status         | Notes |
|-----------------------|----------------|-------|
| `protocol.py`         | drafted        | Round-trips `Ready`, `Stats`, control messages against the Rust host. Stdlib-only — imports on macOS. |
| `config.py`           | drafted        | Parses `StreamConfig` + all nested types from raw JSON. Stdlib-only — imports on macOS. |
| `pipeline_builder.py` | drafted        | Per-stream Gst.Pipeline; uridecodebin → nvstreammux → nvinfer → nvtracker → nvdsanalytics → encoder → rtspclientsink. Snapshot appsink branch scaffolded but tee wiring incomplete (see TODOs). |
| `analytics.py`        | drafted        | Probe walks NvDsFrameMeta → typed events; LC/ROI status iteration. Hot-update via `set_property("config", ...)` while PLAYING. |
| `snapshot_server.py`  | drafted        | stdlib `http.server.ThreadingHTTPServer`; per-stream token gate; constant-time compare. Daemon thread. |
| `glib_bridge.py`      | drafted        | GLib MainLoop in dedicated thread; `call_from_glib` + `call_from_asyncio` helpers. |
| `deepstream_runner.py`| drafted        | Handshake (ready → hello → hello_ack), control dispatch, SIGTERM graceful shutdown, Bye emission. |
| `__init__.py`         | drafted        | Package marker only. |
| `requirements-sidecar.txt` | drafted   | Documents the no-runtime-deps invariant. |
| `README.md`           | drafted        | Operator runbook. |

## Open design decisions

### 1. RTSP output strategy — chose option B (rtspclientsink + mediamtx)

Two options were considered for publishing the per-stream RTSP feed:

- **(A) GstRtspServer in-process:** the sidecar runs an
  `RTSPServer` listening on `rtsp_port` (8554), each stream gets a
  factory mounted at `/ds/<stream_id>` that feeds the encoder output
  via an `appsrc` or direct pad-block.
- **(B) rtspclientsink + external `mediamtx`:** the sidecar emits
  `rtsp://<host>:8554/ds/<stream_id>` via `rtspclientsink`; the
  operator runs `mediamtx` separately on the same host.

**Choice: (B).** Rationale documented in `pipeline_builder.py`
docstring:

- In-process GstRtspServer is fiddly to wire (factory + mount-points +
  appsrc interop); debugging encoder-tee issues inside the same process
  on first integration is painful.
- `mediamtx` is a single static binary, trivially deployable, with
  battle-tested RTSP//WebRTC/HLS muxing. Letting it own the
  network-facing surface keeps the sidecar single-responsibility.
- Cost: one extra systemd service on the Jetson.

**Migration to (A)** is a tracked follow-up after Phase 8 smoke
testing confirms the inference pipeline is sound.

### 2. Snapshot branch wiring — incomplete

`pipeline_builder._make_snapshot_branch` creates the appsink +
nvjpegenc + queue elements but does NOT yet insert the `tee` element
into the encoder input. The encoder input currently flows
`nvvideoconvert → encoder → parser → rtspclientsink` directly.

To complete: add a `tee` between `converter` and `encoder`, request a
src pad on the tee, and link to the snapshot queue. The appsink
callback wiring (`_wire_snapshot_callback` in deepstream_runner) is
already complete — once the tee exists, snapshots will flow.

**Phase 8.3e will complete this.** Without it, the snapshot HTTP
server returns 404 for every stream (token matches but bytes slot is
empty). Detection / analytics / RTSP output are unaffected.

### 3. nvdsanalytics ROI/LC config translation — needs validation

The `analytics_roi_config` / `analytics_line_config` helpers in
`pipeline_builder.py` build the dict-of-dicts expected by
`nvdsanalytics.set_property("config", ...)`. The exact key names
(`ROI-<id>`, `line-crossing-<id>`, `extended`, `mode`) follow the
NVIDIA reference but **have NOT been smoke-tested against a real
nvdsanalytics instance.** Phase 8.3d will validate.

### 4. `set_threshold` is informational only

nvinfer's confidence threshold is baked into the engine at compile
time; live threshold changes require an engine rebuild (10-60s). The
handler currently logs and no-ops. A future improvement: stash a
mutable filter object that the analytics probe reads to filter at the
metadata level (fast, no rebuild).

### 5. GPU info probing — best-effort

`_probe_gpu` in deepstream_runner reads `/proc/device-tree/model` and
`/proc/meminfo` for the GPU name and memory. This works on Jetson
(shared CPU/GPU memory) but is not accurate on dGPU setups. The
informational-only contract on the host side means this is fine for
the first cut. If we need accurate GPU memory on dGPU, pull in
`pynvml` (optional dependency).

### 6. Health/stats emission cadence — not implemented

The protocol defines a `Stats` event but this draft does not yet emit
it on a timer. The Rust host will see `Ready` / `HelloAck` /
`StreamAdded` etc. but no periodic `Stats`. Phase 8.6b follow-up:
schedule a 1 Hz asyncio task that queries each pipeline's
`Gst.CLOCK_TIME_NONE`-derived FPS + nvml GPU utilization.

## Verification (run on macOS — pre-commit)

```
python3 -c 'import sys; sys.path.insert(0, "extensions/deepstream/sidecar"); import protocol, config'
python3 extensions/deepstream/sidecar/protocol.py < /dev/null
python3 -m py_compile extensions/deepstream/sidecar/*.py
ruff check extensions/deepstream/sidecar/
```

All four pass on macOS with Python 3.12. The pyds-dependent modules
(`pipeline_builder`, `analytics`, `glib_bridge`, `deepstream_runner`)
fail to import on macOS — that's expected, they only run inside the
ds:7.1-pyds container.

## Not yet implemented (Phase 8.7+)

- Stats periodic emission (1 Hz)
- Snapshot tee wiring (see design decision #2)
- Live threshold filter propagation (see #4)
- Jetson end-to-end integration tests
- `mediamtx` deployment artifacts (systemd unit)
- Frontend UI integration (NeoMind main repo)
