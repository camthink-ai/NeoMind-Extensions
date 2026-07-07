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

### Data-plane inference — NOT YET verified (blocked on Jetson disk state)

The next milestone is a 90s multi-stream inference test using
`/home box/ds-deps/sample.mp4` published via mediamtx as 4 RTSP streams
(`rtsp://localhost:8554/in/stream{1..4}`) and consumed by the sidecar.
Expected output: ongoing `Detection` events on stdout.

**Current blocker (operational, not code):** The Jetson dev box uses
the Docker `vfs` storage driver (overlayfs is broken on this board's
kernel). vfs duplicates layer data aggressively — every container start
eats ~500MB of irrecoverable layer metadata. Disk fills to 100% within
a few hours of iterative testing. A `docker system prune -a --volumes`
is required to recover, which wipes the 5.36GB ds:7.1-pyds-gi image.

**To unblock:** Re-pull `nvcr.io/nvidia/deepstream:7.1-samples-multiarch`
(~5 min on this network) and rebuild `ds:7.1-pyds-gi`:

```bash
# On Jetson:
docker pull nvcr.io/nvidia/deepstream:7.1-samples-multiarch
docker run -d --name ds-tmp --network=host --runtime=nvidia \
  nvcr.io/nvidia/deepstream:7.1-samples-multiarch sleep 600
docker exec ds-tmp bash -c 'apt-get update && apt-get install -y \
  python3-gi python3-gst-1.0 libpython3.10 && \
  pip3 install /root/pyds-1.2.0-cp310-linux_aarch64.whl'
docker cp ~/ds-deps/pyds-1.2.0-cp310-linux_aarch64.whl ds-tmp:/root/
docker commit ds-tmp ds:7.1-pyds-gi
docker rm -f ds-tmp

# Then re-run the smoke test:
(cat ~/ds-deps/test-long.jsonl; timeout 90 tail -f /dev/null) | \
  docker run --rm -i --network=host --runtime=nvidia \
  -v ~/ds-deps/sidecar:/sidecar -w /sidecar -e PYTHONPATH=/sidecar \
  ds:7.1-pyds-gi python3 -u deepstream_runner.py
```

If the test produces 0 `Detection` events despite a successful
`stream_added`, suspect (in order): (a) pipeline state change to PLAYING
never invoked — check `set_state` log on stderr; (b) buffer probe
attached to wrong element — verify `analytics_elem` in
`build_pipeline()` return; (c) GLib MainLoop not running — Bridge.start
must be called before any `set_state(PLAYING)`.

## Not yet implemented (Phase 8.7+)

- Stats periodic emission (1 Hz)
- Snapshot tee wiring (see design decision #2)
- Live threshold filter propagation (see #4)
- Jetson data-plane end-to-end test (blocked — see above)
- `mediamtx` deployment artifacts (systemd unit)
- Frontend UI integration (NeoMind main repo)
