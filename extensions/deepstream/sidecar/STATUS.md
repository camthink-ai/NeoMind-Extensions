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

### 2. Snapshot — on-demand via GStreamer (tee approach abandoned)

The original plan used a `tee` element to fan out from the converter
to both the encoder (RTSP) and an `nvjpegenc → appsink` branch. This
**stalled the pipeline** (0 Detection events) even with queues between
every element and `allow-not-linked=True` + `alloc-pad` on the tee.
Root cause: likely caps negotiation conflict from the extra
`nvvideoconvert` in the snapshot branch.

**Current approach (working):** the snapshot HTTP handler creates a
**one-shot GStreamer pipeline** on demand that reads one frame from
the stream's RTSP output (`rtspsrc ! rtph264depay ! nvv4l2decoder !
nvvideoconvert ! nvjpegenc ! appsink`). The JPEG is returned inline.
Verified on Jetson: HTTP 200, 95KB JPEG, 1920x1080, ~8s latency
(RTSP connection + decode + encode).

This approach is robust (no effect on the main pipeline) but adds
~8s latency per snapshot request. For lower latency, a future
improvement could cache the last decoded frame via a pad probe on
the main pipeline's converter.

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

## Jetson smoke test status (Phase 8.7)

### Control-plane handshake — verified end-to-end on Jetson (commit 72e3209)

The full handshake + control message dispatch loop was validated on
the Jetson Orin NX 8G inside the `ds:7.1-pyds-gi` container (samples
image + pyds 1.2.0 + python3-gi + python3-gst-1.0 + libpython3.10):

```
host → sidecar: hello    {rtsp_port:8554, snapshot_port:8555, ...}
sidecar → host: ready    {ds_ver:7.1.0, pyds_ver:1.2.0, protocol_ver:1, gpu_info:{name,model}}
sidecar → host: hello_ack {max_streams, rtsp_url_prefix, models_loaded:[...]}
host → sidecar: add_stream {stream_id:stream1, source:{type:rtsp,url:...}, model:Primary_Detector}
sidecar → host: stream_added {id, stream_id, rtsp_url}
host → sidecar: shutdown
sidecar → host: bye {reason, exit_code:0}
```

Three runtime bugs were caught by the smoke test and fixed in 72e3209:

1. `from .X import` → `from X import` (13 occurrences across 3 files).
   The sidecar is invoked as `python3 deepstream_runner.py` (top-level
   script), not as a package — relative imports broke at runtime even
   though ruff accepted them.
2. `loop.connect_read_pipe(sys.stdin)` replaced with
   `loop.run_in_executor(None, sys.stdin.readline)` in both `_read_hello`
   and `_control_loop`. asyncio's pipe transport doesn't support regular
   files; when stdin is redirected from a file or in some TTY-less
   container configs, `connect_read_pipe` either raises
   `OSError: [Errno 22]` or hangs forever. The executor pattern works
   across pipe/file/pty.
3. `GstRtspServer` namespace import removed. The samples image doesn't
   ship `gir1.2-gst-rtsp-server-1.0`; option B (rtspclientsink + external
   mediamtx) doesn't need it anyway.
4. `resolve_model_preset()` rewritten to probe 5 candidate .txt locations
   (both user-laid-out `<models_dir>/<model>/config_infer_primary_<model>.txt`
   AND the NVIDIA samples split layout under
   `<ds_root>/samples/configs/deepstream-app/`).

### Data-plane inference — VERIFIED end-to-end on Jetson (RTSP source)

The full data plane is now working: a single RTSP source
(`rtsp://localhost:8554/in/stream1` fed by `mediamtx` + `ffmpeg -re
-stream_loop -1 -i sample.mp4 -c copy -f rtsp`) flowing through the
sidecar produces ~4400 `Detection` events over a 180s run on Jetson
Orin NX 8G.

```
rtspsrc ! rtph264depay ! nvv4l2decoder ! mux.sink_0
mux ! nvinfer ! nvtracker ! nvdsanalytics ! nvvideoconvert ! nvv4l2h264enc ! h264parse ! fakesink
```

Six root causes were caught and fixed end-to-end (see commit history):

1. **`link_chain(*conv_chain)` only linked converter onward**, leaving
   `mux.src` dangling. Pipeline reached PLAYING but no buffer ever
   reached nvinfer, so zero Detection events. Fixed: link the full
   `mux → nvinfer → tracker → analytics → converter → [osd?] → encoder
   → parser → rtsp_sink` chain.
2. **h264parse between rtph264depay and nvv4l2decoder fails AVCC→byte-stream
   conversion** despite advertising byte-stream caps — buffers stay
   length-prefixed, decoder opens but never produces output. Fixed:
   drop h264parse entirely; mediamtx SDP carries SPS/PPS via
   sprop-parameter-sets so depay+decoder handle AVCC natively.
3. **nvstreammux on Jetson requires explicit `mux.sink_N` pad naming** —
   auto-link via `pad.link()` against a static sink pad fails. Fixed:
   `mux.get_request_pad("sink_0")` + `decoder.src.link(mux_sink)`.
4. **capsfilter between decoder and mux blocks buffer flow** even though
   caps negotiation succeeds. Fixed: direct `decoder.src → mux.sink_0`
   link, no capsfilter.
5. **Back-pressure stall**: without queues between transform elements,
   the pipeline froze after exactly 5 buffers (encoder pool exhausted).
   Fixed: insert a `queue` with unlimited size between every pair of
   elements in the chain.
6. **TRT 10.x (in DS 7.1) drops legacy `output-blob-names` tensor names**
   that the sample configs hardcode. Fixed: strip `output-blob-names`
   so nvinfer discovers outputs from the ONNX directly.

**Pre-built engine:** `trafficcam_fp16.engine` (3.1MB, FP16,
batch-size=1) is mounted at `/engines/trafficcam_fp16.engine` inside
the container. `write_model_config` injects it via `model-engine-file`
override and forces `network-mode=2` (FP16) + `batch-size=1` to match
the trtexec-built engine. Without this, nvinfer tries to rebuild the
engine at startup and OOMs on Jetson Orin NX 8G.

**Reproducing:** see `~/ds-deps/run_test.sh` on the Jetson box. The
script pipes `data-plane-long.jsonl` (hello + add_stream) into the
sidecar container, waits 180s, then sends shutdown and greps stdout
for `"type":"detection"` events.

**Known limitations of current state:**
- `rtspclientsink` restored — RTSP output publishing to mediamtx works
  (verified: mediamtx logs "is publishing to path 'ds/test-1'").
- Snapshot branch disabled (`snapshot_enabled=False` default) — tee
  insertion stalls the pipeline (see design decision #2). Deferred to
  follow-up; will use probe-based snapshot instead of tee.
- Stats periodic emission **implemented and verified** (5s interval,
  per-stream FPS from frame_count deltas, pipeline status, frame_count,
  object_count). See `deepstream_runner._stats_loop`.

## Verified — Stats emission (2026-07-08)

60s run on Jetson Orin NX 8G → **11 Stats events** (5s interval) +
**1340 Detection events**. Sample Stats:

```json
{"type":"stats","ts":1783476707269,"global_fps":25.17,
 "per_stream":[{"stream_id":"test-1","fps":25.17,
 "frame_count":174,"object_count":3140,"status":"playing"}]}
```

FPS=25.17 matches the 25fps source video exactly. `gpu_utilization_percent`
and `gpu_memory_used_mb` report 0.0 (nvml/pynvml not wired; see design
decision #5 — informational only).

## Not yet implemented (follow-ups)

- Snapshot latency optimization (currently ~8s via one-shot RTSP grab;
  could cache last frame via pad probe for <100ms)
- Live threshold filter propagation (see #4)
- GPU utilization/memory via pynvml (currently 0.0 — informational only)
- `mediamtx` deployment artifacts (systemd unit)
- Frontend UI integration (NeoMind main repo)
