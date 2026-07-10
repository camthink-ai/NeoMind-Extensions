# DeepStream Sidecar

Python process spawned by the NeoMind `deepstream` Rust extension.
Wraps NVIDIA DeepStream 7.1 + pyds 1.2.0 to do real-time object
detection, tracking, and analytics (line crossing / ROI intrusion) on
RTSP camera streams.

Communicates with the Rust host over **stdin (control) / stdout
(events)** using **newline-delimited JSON** — see
[`../src/protocol.rs`](../src/protocol.rs) for the authoritative wire
format.

## Prerequisites

Tested on:

- **Jetson Orin NX 8G** with JetPack 6.x (R36.4+)
- Container: `nvcr.io/nvidia/deepstream:7.1-pyds` (DeepStream 7.1 + pyds 1.2.0)
- Python 3.10 (from the container — do NOT swap)
- `mediamtx` binary on the host for RTSP output (see "RTSP output" below)

Verify the container is healthy before launching the sidecar:

```bash
docker run --rm --runtime nvidia --gpus all -it \
    --network host \
    -v /tmp/nvmpi:/tmp/nvmpi \
    nvcr.io/nvidia/deepstream:7.1-pyds \
    bash -c 'python3 -c "import pyds; print(pyds.__version__)" && \
             gst-inspect-1.0 nvinfer | head -1'
```

Expected: pyds version printed and `nvinfer: nvinfer-` line.

## Running the sidecar manually

The Rust host normally spawns this sidecar automatically; for debugging
you can drive it by hand:

```bash
# 1. Start mediamtx on the host (different terminal).
./mediamtx &

# 2. Start the sidecar inside the container.
docker run --rm -it --runtime nvidia --gpus all \
    --network host \
    -v "$PWD/extensions/deepstream:/opt/ds_ext" \
    -v /tmp/xdg:/tmp/xdg \
    -e XDG_RUNTIME_DIR=/tmp/xdg \
    nvcr.io/nvidia/deepstream:7.1-pyds \
    python3 /opt/ds_ext/sidecar/deepstream_runner.py
```

You should see a `ready` event on stdout, e.g.

```json
{"type":"ready","ds_ver":"7.1","pyds_ver":"1.2.0","protocol_ver":1,"gpu_info":{"name":"Jetson Orin NX","mem_mb":8129}}
```

Send `hello`:

```bash
echo '{"type":"hello","rtsp_port":8554,"snapshot_port":8555,"log_level":"info","models_dir":"/opt/ds_ext/models","max_streams":8,"snapshot_bind_addr":"127.0.0.1"}' \
    | python3 /opt/ds_ext/sidecar/deepstream_runner.py
```

Add a stream (in a real session, pipe via the Rust host):

```json
{"type":"add_stream","id":"r1","config":{"stream_id":"cam1","source":{"type":"rtsp","url":"rtsp://10.0.0.42/axis-media/media.amp"},"model":"yolov8n-coco","tracker":{"enabled":true,"type":"NvDCF"},"analytics":{"line_crossing":[{"id":"L1","points":[[100,300],[500,300]],"mode":"bidirectional","classes":[0]}]}}}
```

Watch for `stream_added` and pull the RTSP output with VLC:

```bash
vlc rtsp://127.0.0.1:8554/ds/cam1
```

Fetch a snapshot:

```bash
curl -o snap.jpg "http://127.0.0.1:8555/snapshot/cam1.jpg?token=<token>"
# token is issued by the host; for manual testing, read it from the
# sidecar's stderr log on stream_added.
```

## Ports

| Port | Protocol | Purpose                              | Default bind |
|------|----------|--------------------------------------|--------------|
| 8554 | RTSP     | Output streams (mediamtx listens)    | 0.0.0.0      |
| 8555 | HTTP     | Snapshot JPEG server                 | 127.0.0.1    |

Both ports are configurable via the `hello` message fields
(`rtsp_port`, `snapshot_port`, `snapshot_bind_addr`).

## Model directory layout

The sidecar expects each model preset to live in its own directory
under `hello.models_dir`:

```
models/
└── yolov8n-coco/
    ├── config_infer_primary_yolov8n-coco.txt    # nvinfer config
    ├── labels.txt                                # COCO class names (optional)
    └── yolov8n-coco.etlt  OR  yolov8n-coco.engine
```

If the `.engine` file is absent, nvinfer compiles it from the `.etlt`
on first stream-add (10-60s blocking — only do this on the first
launch per model).

Required nvinfer config keys (`config_infer_primary_*.txt`):

```
[property]
model-engine-file=yolov8n-coco.engine      # or model-file=.etlt for first-run compile
labelfile-path=labels.txt
num-detected-classes=80
gie-unique-id=1
```

## Logging

The sidecar writes ALL logs to **stderr**. stdout is reserved
exclusively for the wire protocol — mixing logs into stdout will break
the Rust host's JSON parser.

Log level comes from `hello.log_level` (one of `debug`, `info`,
`warning`, `error`).

## Troubleshooting

### "model preset 'X' not found under /path"

Either `hello.models_dir` is wrong, or the model directory is missing
the `config_infer_primary_*.txt` file. Check directory layout above.

### Stream goes to `stream_error` immediately

Most common causes:

1. RTSP URL unreachable from inside the container. Test with
   `gst-launch-1.0 rtspsrc location=<url> ! fakesink` in the container.
2. nvinfer config refers to a missing engine file path. Check
   `model-engine-file=` in the .txt.
3. First-run engine compile timed out. Pre-compile with
   `deepstream-app -c <config>` once outside the sidecar.

### Snapshot returns 404 for a known stream

Token mismatch (404 hides all failures — see snapshot_server.py), OR
the snapshot tee branch is not wired (see STATUS.md design decision #2).
Phase 8.3e will complete the wiring.

### `rtsp://.../ds/cam1` won't play in VLC

Make sure `mediamtx` is running on the same host and listening on the
RTSP port (8554 by default). The sidecar uses `rtspclientsink` to push
to mediamtx; mediamtx is the actual publisher.

### Sidecar crashes with "GLib MainLoop failed to start within 5s"

Usually means Gst.init failed earlier in the run. Check stderr for the
GStreamer init error. On Jetson you may need `--runtime nvidia` and
`--gpus all` on `docker run`.

## Wire protocol reference

See [`../src/protocol.rs`](../src/protocol.rs). The Python mirror lives
in [`protocol.py`](protocol.py). Both sides must agree on
`PROTOCOL_VERSION` — currently `1`.

## See also

- [`STATUS.md`](STATUS.md) — module status + open design decisions.
- [`../src/protocol.rs`](../src/protocol.rs) — authoritative wire format.
- [`../src/stream_manager.rs`](../src/stream_manager.rs) — `StreamConfig` source of truth.
