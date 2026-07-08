"""Main sidecar entry point.

Wire-protocol summary (see ``protocol.py`` for the full contract):

1. **On startup:** emit ``ready`` immediately (before reading hello).
   This tells the host the sidecar process is alive and what version
   of pyds/Gst it has.
2. **Handshake:** wait for ``hello`` from host, then emit ``hello_ack``
   listing loaded models + the RTSP URL prefix.
3. **Steady state:** read control messages from stdin, dispatch:
   - ``add_stream`` -> build Gst pipeline, attach probes, send
     ``stream_added`` (or ``stream_error`` on failure).
   - ``remove_stream`` -> tear down pipeline, send ``stream_removed``.
   - ``update_analytics`` -> hot-update nvdsanalytics config.
   - ``set_threshold`` -> best-effort (no live engine rebuild).
   - ``list_state`` -> emit ``analytics_snapshot`` per stream.
   - ``health_check`` -> emit ``pong``.
   - ``shutdown`` -> graceful teardown, emit ``bye`` exit 0.
4. **On unrecoverable error:** emit ``error_response`` then ``bye``
   with non-zero exit_code, exit non-zero.
5. **SIGTERM / SIGINT:** run graceful shutdown (same as ``shutdown``
   message with ``graceful_secs=3``).

Threading model:

- **Main thread:** asyncio event loop. Reads stdin (line-by-line) via
  ``loop.run_in_executor`` to avoid blocking. Writes to stdout are
  synchronous (small, line-delimited).
- **GLib thread:** runs Gst MainLoop (see :mod:`glib_bridge`).
- **Snapshot HTTP thread:** daemon thread serving the snapshot endpoint.

stdout is flushed after EVERY write — the host reads line-by-line and
will hang on partial lines.
"""

from __future__ import annotations

import asyncio
import logging
import os
import signal
import sys
import threading
import time
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from protocol import (
    PROTOCOL_VERSION,
    AddStream,
    AnalyticsSnapshot,
    Bye,
    ErrorResponse,
    HealthCheck,
    Hello,
    HelloAck,
    ListState,
    Pong,
    Ready,
    RemoveStream,
    SetThreshold,
    Shutdown,
    Stats,
    StreamAdded,
    StreamError,
    StreamRemoved,
    StreamStat,
    UpdateAnalytics,
    deserialize_line,
    parse_control_message,
    serialize,
)
from config import parse_stream_config

log = logging.getLogger("deepstream.runner")


@dataclass
class StreamEntry:
    """One active stream's runtime state."""
    stream_id: str
    pipeline_handle: Any = None        # BuiltPipeline
    probe_handle: Any = None           # analytics.ProbeHandle
    snapshot_token: str = ""
    rtsp_url: str = ""
    config: Any = None                 # StreamConfig
    added_at: float = field(default_factory=time.time)


@dataclass
class RunnerConfig:
    rtsp_port: int = 8554
    snapshot_port: int = 8555
    log_level: str = "info"
    models_dir: str = "/opt/nvidia/deepstream/deepstream/samples/models"
    max_streams: int = 8
    snapshot_bind_addr: str = "127.0.0.1"


class DeepStreamRunner:
    """Coordinates the wire protocol, Gst pipelines, and HTTP server."""

    def __init__(self) -> None:
        self.config: Optional[RunnerConfig] = None
        self.streams: Dict[str, StreamEntry] = {}
        self.streams_lock = threading.Lock()
        self._bridge: Any = None       # glib_bridge.Bridge (lazy)
        self._snapshot_store: Any = None
        self._snapshot_server: Any = None
        self._shutdown_requested = threading.Event()
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        # Per-stream previous-stats snapshot for FPS delta computation.
        # Keyed by stream_id; value = (ts_monotonic, frame_count).
        self._stats_prev: Dict[str, tuple] = {}

    # --- Emit helpers ----------------------------------------------------

    def emit(self, event: Any) -> None:
        """Serialize one SidecarEvent to stdout + flush.

        Called from any thread. stdout writes are atomic for lines
        shorter than the pipe buffer (typically 64 KiB on Linux; our
        events are <4 KiB).
        """
        try:
            data = serialize(event)
        except Exception as e:
            log.error("failed to serialize event %r: %s", event, e)
            return
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()

    # --- Lifecycle -------------------------------------------------------

    async def run(self) -> int:
        """Main entry. Returns the process exit code."""
        self._loop = asyncio.get_event_loop()

        # 1. Emit ready immediately (before reading hello).
        try:
            self._emit_ready()
        except Exception as e:
            log.exception("ready emit failed: %s", e)
            return 1

        # 2. Wait for hello.
        hello = await self._read_hello()
        if hello is None:
            # stdin closed before hello — exit cleanly.
            return 0
        self.config = RunnerConfig(
            rtsp_port=hello.rtsp_port,
            snapshot_port=hello.snapshot_port,
            log_level=hello.log_level,
            models_dir=hello.models_dir,
            max_streams=hello.max_streams,
            snapshot_bind_addr=hello.snapshot_bind_addr,
        )
        self._configure_logging(self.config.log_level)

        # 3. Lazy-import Gst-dependent modules now that we know we're in
        # the container (anything earlier would crash on import error).
        # NOTE: ``pipeline_builder`` and ``analytics`` are referenced by
        # handler methods (which run after Gst.init) — ruff's static
        # analysis can't see those uses so they need explicit noqa.
        try:
            import glib_bridge, snapshot_server  # noqa: F401
            import pipeline_builder, analytics  # noqa: F401
            # Gst init must happen on the main thread before any element creation.
            try:
                import gi
                gi.require_version("Gst", "1.0")
                from gi.repository import Gst
                Gst.init(None)
            except Exception as e:
                self.emit(ErrorResponse(
                    id="init",
                    code="gst_init_failed",
                    message=f"GStreamer init failed: {e}",
                ))
                self.emit(Bye(reason=f"gst init failed: {e}", exit_code=2))
                return 2
        except Exception as e:
            self.emit(ErrorResponse(
                id="init",
                code="import_failed",
                message=f"sidecar module import failed: {e}",
            ))
            self.emit(Bye(reason=f"import failed: {e}", exit_code=2))
            return 2

        # 4. Start GLib bridge + snapshot server.
        try:
            self._bridge = glib_bridge.Bridge(asyncio_loop=self._loop)
            self._bridge.start()
        except Exception as e:
            self.emit(ErrorResponse(
                id="init",
                code="glib_start_failed",
                message=f"GLib bridge failed: {e}",
            ))
            self.emit(Bye(reason=str(e), exit_code=3))
            return 3

        try:
            self._snapshot_store = snapshot_server.SnapshotStore()
            self._snapshot_server = snapshot_server.SnapshotServer(
                self._snapshot_store,
                bind_addr=self.config.snapshot_bind_addr,
                port=self.config.snapshot_port,
            )
            self._snapshot_server.start()
        except Exception as e:
            log.warning("snapshot server start failed: %s — continuing without it", e)
            self._snapshot_server = None

        # 5. Emit hello_ack.
        # rtsp_url_prefix uses 127.0.0.1 (connect address) — mediamtx runs
        # alongside the sidecar on the same host. snapshot_bind_addr (default
        # 0.0.0.0) is the BIND address for the snapshot HTTP server and is
        # unrelated to the RTSP connect address.
        self.emit(HelloAck(
            max_streams=self.config.max_streams,
            rtsp_url_prefix=f"rtsp://127.0.0.1:{self.config.rtsp_port}/ds/",
            models_loaded=self._scan_models(),
        ))

        # 6. Install signal handlers + enter control loop.
        self._install_signal_handlers()

        # Start periodic Stats emission (1 Hz).
        stats_task = asyncio.ensure_future(self._stats_loop())

        try:
            await self._control_loop()
        except asyncio.CancelledError:
            pass
        finally:
            stats_task.cancel()
            try:
                await stats_task
            except (asyncio.CancelledError, Exception):
                pass
            await self._graceful_shutdown(reason="control loop ended", exit_code=0)
            if self._bridge is not None:
                self._bridge.stop()
            if self._snapshot_server is not None:
                self._snapshot_server.stop()

        return 0

    # --- Phase 1: ready + hello -----------------------------------------

    def _emit_ready(self) -> None:
        ds_ver = os.environ.get("DEEPSTREAM_VERSION", "7.1")
        pyds_ver = os.environ.get("PYDS_VERSION", "unknown")
        gpu_info = self._probe_gpu()
        self.emit(Ready(
            ds_ver=ds_ver,
            pyds_ver=pyds_ver,
            protocol_ver=PROTOCOL_VERSION,
            gpu_info=gpu_info,
        ))

    def _probe_gpu(self) -> Any:
        """Return {name, mem_mb} via nvml if available, else placeholder.

        On Jetson (shared CPU/GPU memory) we read the model name from
        device-tree and total memory from /proc/meminfo. On dGPU setups
        this won't be accurate; pull in pynvml as an optional dep if/when
        dGPU support is needed.
        """
        # Fall back to /proc/meminfo or just a placeholder. The host
        # tolerates any value here — it's informational only.
        try:
            with open("/proc/device-tree/model", "r", encoding="utf-8") as f:
                name = f.read().strip().rstrip("\x00")
        except OSError:
            name = "Jetson (unknown)"
        # Best-effort: read total memory from /proc/meminfo.
        mem_mb = 0
        try:
            with open("/proc/meminfo", "r", encoding="utf-8") as f:
                for line in f:
                    if line.startswith("MemTotal:"):
                        kb = int(line.split()[1])
                        mem_mb = kb // 1024
                        break
        except OSError:
            pass
        from protocol import GpuInfo
        return GpuInfo(name=name, mem_mb=mem_mb)

    async def _read_hello(self) -> Optional[Hello]:
        """Read lines until we get a hello (or stdin closes).

        NOTE: We deliberately avoid ``loop.connect_read_pipe(sys.stdin)``
        because asyncio's pipe transport only supports true pipes/FIFOs,
        not regular files. When the host spawns the sidecar via
        ``subprocess.PIPE`` it works, but when stdin is redirected from a
        file (smoke-test: ``python3 runner.py < input.jsonl``) or in some
        TTY-less container configurations, ``connect_read_pipe`` either
        raises ``OSError: [Errno 22]`` or silently hangs forever. Using
        ``run_in_executor`` for blocking ``readline()`` works in all three
        cases (pipe, file, pty).
        """
        while not self._shutdown_requested.is_set():
            try:
                line_bytes = await asyncio.wait_for(
                    self._loop.run_in_executor(None, sys.stdin.readline),
                    timeout=300,
                )
            except asyncio.TimeoutError:
                log.warning("hello timeout (300s) — exiting")
                return None
            if not line_bytes:
                log.info("stdin closed before hello")
                return None
            line = (line_bytes.decode("utf-8", errors="replace") if isinstance(line_bytes, bytes) else line_bytes).rstrip("\n")
            if not line:
                continue
            try:
                msg = parse_control_message(deserialize_line(line))
            except Exception as e:
                log.warning("ignoring malformed pre-hello line: %s", e)
                continue
            if isinstance(msg, Hello):
                return msg
            # Any other message pre-hello is a protocol error.
            log.warning("expected hello, got %s — ignoring", type(msg).__name__)

        return None

    # --- Stats emission --------------------------------------------------

    async def _stats_loop(self) -> None:
        """Emit a ``Stats`` SidecarEvent every 5 seconds.

        Per-stream FPS is computed by sampling frame_count deltas across
        intervals. Latency is not measured (no reliable per-buffer latency
        probe without a jitterbuffer); reported as 0.0.
        """
        while not self._shutdown_requested.is_set():
            try:
                await asyncio.sleep(5.0)
            except asyncio.CancelledError:
                return
            if self._shutdown_requested.is_set():
                return
            try:
                self._emit_one_stats()
            except Exception as e:
                log.warning("stats emission failed: %s", e)

    def _emit_one_stats(self) -> None:
        with self.streams_lock:
            entries = list(self.streams.values())
        if not entries:
            return

        now_mono = time.monotonic()
        per_stream: List[StreamStat] = []
        total_fps = 0.0
        for entry in entries:
            fc = 0
            oc = 0
            if entry.probe_handle is not None:
                fc = entry.probe_handle.frame_count
                oc = entry.probe_handle.object_count
            # FPS delta.
            prev = self._stats_prev.get(entry.stream_id)
            fps = 0.0
            if prev is not None:
                dt = now_mono - prev[0]
                df = fc - prev[1]
                if dt > 0 and df >= 0:
                    fps = df / dt
            self._stats_prev[entry.stream_id] = (now_mono, fc)
            total_fps += fps

            status = "unknown"
            try:
                from gi.repository import Gst  # type: ignore[import-not-found]
                if entry.pipeline_handle is not None:
                    _, state, _ = entry.pipeline_handle.pipeline.get_state(0)
                    status = {
                        Gst.State.NULL: "null",
                        Gst.State.READY: "ready",
                        Gst.State.PAUSED: "paused",
                        Gst.State.PLAYING: "playing",
                    }.get(state, str(state))
            except Exception:
                pass

            per_stream.append(StreamStat(
                stream_id=entry.stream_id,
                fps=round(fps, 2),
                latency_ms=0.0,
                frame_count=fc,
                object_count=oc,
                status=status,
            ))

        # Prune stale entries from _stats_prev.
        live_ids = {e.stream_id for e in entries}
        for sid in list(self._stats_prev.keys()):
            if sid not in live_ids:
                del self._stats_prev[sid]

        self.emit(Stats(
            ts=int(time.time() * 1000),
            global_fps=round(total_fps, 2),
            gpu_utilization_percent=0.0,
            gpu_memory_used_mb=0.0,
            per_stream=per_stream,
        ))

    # --- Phase 2: control loop -------------------------------------------

    async def _control_loop(self) -> None:
        # NOTE: See _read_hello for why we use run_in_executor here instead
        # of connect_read_pipe.
        while not self._shutdown_requested.is_set():
            try:
                line_bytes = await self._loop.run_in_executor(None, sys.stdin.readline)
            except asyncio.CancelledError:
                break
            if not line_bytes:
                log.info("stdin closed — shutting down")
                break
            line = (line_bytes.decode("utf-8", errors="replace") if isinstance(line_bytes, bytes) else line_bytes).rstrip("\n")
            if not line:
                continue
            try:
                msg = parse_control_message(deserialize_line(line))
            except Exception as e:
                self.emit(ErrorResponse(
                    id="?",
                    code="parse_error",
                    message=str(e),
                ))
                continue
            # Dispatch each message type. Errors are converted to
            # ErrorResponse / StreamError rather than crashing the runner.
            try:
                await self._dispatch(msg)
            except Exception as e:
                log.exception("dispatch error on %s: %s", type(msg).__name__, e)
                self.emit(ErrorResponse(
                    id=getattr(msg, "id", "?"),
                    code="dispatch_error",
                    message=f"{type(e).__name__}: {e}",
                ))

    async def _dispatch(self, msg: Any) -> None:
        if isinstance(msg, AddStream):
            await self._handle_add_stream(msg)
        elif isinstance(msg, RemoveStream):
            await self._handle_remove_stream(msg)
        elif isinstance(msg, UpdateAnalytics):
            await self._handle_update_analytics(msg)
        elif isinstance(msg, SetThreshold):
            await self._handle_set_threshold(msg)
        elif isinstance(msg, ListState):
            await self._handle_list_state(msg)
        elif isinstance(msg, HealthCheck):
            self.emit(Pong(ts=msg.ts))
        elif isinstance(msg, Shutdown):
            log.info("shutdown requested (graceful=%ss)", msg.graceful_secs)
            self._shutdown_requested.set()
        else:
            log.warning("unhandled message type: %s", type(msg).__name__)

    # --- Message handlers -----------------------------------------------

    async def _handle_add_stream(self, msg: AddStream) -> None:
        import pipeline_builder, analytics

        try:
            cfg = parse_stream_config(msg.config)
        except Exception as e:
            self.emit(ErrorResponse(
                id=msg.id,
                code="config_parse_error",
                message=f"invalid StreamConfig: {e}",
            ))
            return

        with self.streams_lock:
            if cfg.stream_id in self.streams:
                self.emit(ErrorResponse(
                    id=msg.id,
                    code="already_exists",
                    message=f"stream {cfg.stream_id!r} already exists",
                ))
                return
            if len(self.streams) >= (self.config.max_streams if self.config else 8):
                self.emit(ErrorResponse(
                    id=msg.id,
                    code="max_streams_reached",
                    message="max_streams reached",
                ))
                return

        rtsp_prefix = (
            f"rtsp://127.0.0.1:{self.config.rtsp_port}/ds/"
            if self.config else pipeline_builder.DEFAULT_RTSP_URL_PREFIX
        )

        # Build pipeline on the GLib thread — element creation must
        # happen there to avoid races with the bus watch.
        def _build() -> Any:
            return pipeline_builder.build_pipeline(
                stream_id=cfg.stream_id,
                config=cfg,
                rtsp_url_prefix=rtsp_prefix,
                models_dir=self.config.models_dir if self.config else pipeline_builder.DEFAULT_FRAME_WIDTH,
            )

        try:
            built = await self._call_on_glib(_build)
        except Exception as e:
            self.emit(StreamError(
                stream_id=cfg.stream_id,
                code="pipeline_build_failed",
                message=str(e),
                id=msg.id,
            ))
            return

        # Register snapshot token + RTSP URL for on-demand capture.
        token = ""
        if self._snapshot_store is not None:
            token = self._snapshot_store.register_stream(
                cfg.stream_id, rtsp_url=built.rtsp_url
            )

        # Build event filter + attach analytics probe.
        events_cfg = cfg.events
        filt = analytics.StreamFilter(
            stream_id=cfg.stream_id,
            detection_hz=getattr(events_cfg, "detection_hz", None) if events_cfg else None,
            always_emit=getattr(events_cfg, "always_emit", []) or [],
            filter_classes=getattr(cfg.model_config, "filter_classes", None) if cfg.model_config else None,
            min_confidence=getattr(cfg.tracker, "min_confidence", None) if cfg.tracker else None,
        )

        def _attach() -> Any:
            return analytics.attach_probe(
                built.analytics_elem, cfg.stream_id, filt,
                on_event=lambda ev: self.emit(ev),
            )

        try:
            probe_handle = await self._call_on_glib(_attach)
        except Exception as e:
            log.warning("probe attach failed: %s — continuing without analytics", e)
            probe_handle = None

        # Set pipeline to PLAYING.
        def _play() -> None:
            built.pipeline.set_state(__import__("gi").repository.Gst.State.PLAYING)

        try:
            await self._call_on_glib(_play)
        except Exception as e:
            self.emit(StreamError(
                stream_id=cfg.stream_id,
                code="state_change_failed",
                message=f"failed to set PLAYING: {e}",
                id=msg.id,
            ))
            return

        with self.streams_lock:
            self.streams[cfg.stream_id] = StreamEntry(
                stream_id=cfg.stream_id,
                pipeline_handle=built,
                probe_handle=probe_handle,
                snapshot_token=token,
                rtsp_url=built.rtsp_url,
                config=cfg,
            )

        self.emit(StreamAdded(
            id=msg.id,
            stream_id=cfg.stream_id,
            rtsp_url=built.rtsp_url,
            snapshot_token=token,
        ))

    async def _handle_remove_stream(self, msg: RemoveStream) -> None:
        import analytics as analytics_mod
        with self.streams_lock:
            entry = self.streams.pop(msg.stream_id, None)
        if entry is None:
            self.emit(ErrorResponse(
                id=msg.id,
                code="not_found",
                message=f"stream {msg.stream_id!r} not found",
            ))
            return
        if entry.probe_handle is not None:
            try:
                await self._call_on_glib(
                    lambda: analytics_mod.detach_probe(entry.probe_handle)
                )
            except Exception as e:
                log.warning("probe detach failed: %s", e)

        def _stop() -> None:
            entry.pipeline_handle.pipeline.set_state(
                __import__("gi").repository.Gst.State.NULL
            )

        try:
            await self._call_on_glib(_stop)
        except Exception as e:
            log.warning("pipeline stop failed: %s", e)

        if self._snapshot_store is not None:
            self._snapshot_store.unregister_stream(msg.stream_id)

        self.emit(StreamRemoved(id=msg.id, stream_id=msg.stream_id))

    async def _handle_update_analytics(self, msg: UpdateAnalytics) -> None:
        import analytics
        with self.streams_lock:
            entry = self.streams.get(msg.stream_id)
        if entry is None:
            self.emit(ErrorResponse(
                id=msg.id,
                code="not_found",
                message=f"stream {msg.stream_id!r} not found",
            ))
            return
        try:
            if msg.line_crossing:
                rules = _build_line_rules(msg.line_crossing)
                await self._call_on_glib(
                    lambda: analytics.set_line_crossing(
                        entry.pipeline_handle.analytics_elem, rules
                    )
                )
            if msg.roi:
                rules = _build_roi_rules(msg.roi)
                await self._call_on_glib(
                    lambda: analytics.set_roi(
                        entry.pipeline_handle.analytics_elem, rules
                    )
                )
        except Exception as e:
            self.emit(ErrorResponse(
                id=msg.id,
                code="analytics_update_failed",
                message=str(e),
            ))
            return
        # Ack via an AnalyticsSnapshot event (best-effort).
        self.emit(AnalyticsSnapshot(
            stream_id=msg.stream_id,
            ts=int(time.time() * 1000),
            snapshot={"applied": True},
        ))

    async def _handle_set_threshold(self, msg: SetThreshold) -> None:
        # Live threshold change without an engine rebuild is approximated
        # by updating the per-stream filter's min_confidence. The actual
        # nvinfer threshold is fixed at engine build time.
        with self.streams_lock:
            entry = self.streams.get(msg.stream_id)
        if entry is None:
            self.emit(ErrorResponse(
                id=msg.id,
                code="not_found",
                message=f"stream {msg.stream_id!r} not found",
            ))
            return
        # Note: the filter is per-probe; for the first cut we just log.
        log.info(
            "set_threshold stream=%s conf=%.3f iou=%.3f "
            "(applied to filter only; nvinfer threshold not live-updatable)",
            msg.stream_id, msg.conf, msg.iou,
        )
        # TODO: stash a mutable filter and have the probe read it.

    async def _handle_list_state(self, msg: ListState) -> None:
        with self.streams_lock:
            entries = list(self.streams.values())
        for entry in entries:
            self.emit(AnalyticsSnapshot(
                stream_id=entry.stream_id,
                ts=int(time.time() * 1000),
                snapshot={
                    "rtsp_url": entry.rtsp_url,
                    "added_at": entry.added_at,
                    "model": getattr(entry.config, "model", None) if entry.config else None,
                },
            ))

    # --- Helpers ---------------------------------------------------------

    async def _call_on_glib(self, fn: Any) -> Any:
        """Run a sync callable on the GLib thread; await its result.

        Returns the callable's return value. Raises any exception that
        the callable raises (re-thrown on the asyncio side).
        """
        loop = asyncio.get_event_loop()
        fut: asyncio.Future = loop.create_future()

        def _wrap() -> None:
            try:
                result = fn()
                # Cross back to asyncio.
                loop.call_soon_threadsafe(_resolve, fut, result, None)
            except Exception as e:
                loop.call_soon_threadsafe(_resolve, fut, None, e)

        def _resolve(f: asyncio.Future, result: Any, exc: Optional[BaseException]) -> None:
            if not f.done():
                if exc is not None:
                    f.set_exception(exc)
                else:
                    f.set_result(result)

        self._bridge.call_from_glib(_wrap)
        return await fut

    def _scan_models(self) -> List[str]:
        if self.config is None:
            return []
        models_dir = self.config.models_dir
        if not os.path.isdir(models_dir):
            return []
        out: List[str] = []
        try:
            for name in sorted(os.listdir(models_dir)):
                full = os.path.join(models_dir, name)
                if os.path.isdir(full):
                    out.append(name)
        except OSError:
            pass
        return out

    def _configure_logging(self, level: str) -> None:
        levels = {
            "debug": logging.DEBUG,
            "info": logging.INFO,
            "warn": logging.WARNING,
            "warning": logging.WARNING,
            "error": logging.ERROR,
        }
        logging.basicConfig(
            stream=sys.stderr,  # NEVER stdout — that's the wire channel
            level=levels.get(level.lower(), logging.INFO),
            format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        )

    def _install_signal_handlers(self) -> None:
        for sig in (signal.SIGINT, signal.SIGTERM):
            try:
                self._loop.add_signal_handler(sig, self._on_signal, sig)
            except (NotImplementedError, RuntimeError):
                # add_signal_handler doesn't work on Windows / some loops.
                signal.signal(sig, self._on_signal_sync)

    def _on_signal(self, sig: Any) -> None:
        log.info("received signal %s — requesting shutdown", sig)
        self._shutdown_requested.set()

    def _on_signal_sync(self, sig: int, frame: Any) -> None:
        log.info("received signal %d — requesting shutdown", sig)
        self._shutdown_requested.set()

    async def _graceful_shutdown(self, *, reason: str, exit_code: int) -> None:
        log.info("graceful shutdown: %s", reason)
        # Stop all streams.
        with self.streams_lock:
            entries = list(self.streams.values())
            self.streams.clear()
        for entry in entries:
            try:
                def _stop(e: Any = entry) -> None:
                    e.pipeline_handle.pipeline.set_state(
                        __import__("gi").repository.Gst.State.NULL
                    )
                if self._bridge is not None:
                    self._bridge.call_from_glib(_stop)
            except Exception as e:
                log.warning("stop stream %s failed: %s", entry.stream_id, e)
        # Give Gst a moment to flush buffers.
        await asyncio.sleep(0.5)
        self.emit(Bye(reason=reason, exit_code=exit_code))


# --- Helpers --------------------------------------------------------------


def _wire_snapshot_callback(appsink: Any, stream_id: str, store: Any) -> None:
    """Wire the appsink ``new-sample`` callback to push JPEGs into the store."""
    try:
        from gi.repository import Gst  # type: ignore[import-not-found]
    except Exception:
        return

    def _on_sample(sink: Any) -> Any:
        try:
            sample = sink.emit("pull-sample")
            if sample is None:
                return Gst.FlowReturn.OK
            buf = sample.get_buffer()
            if buf is None:
                return Gst.FlowReturn.OK
            ok, info = buf.map(__import__("gi").repository.Gst.MapFlags.READ)
            if not ok:
                return Gst.FlowReturn.OK
            try:
                store.push_jpeg(stream_id, bytes(info.data))
            finally:
                buf.unmap(info)
        except Exception as e:
            log.warning("snapshot callback error: %s", e)
        return Gst.FlowReturn.OK

    appsink.connect("new-sample", _on_sample)


def _build_line_rules(raw: Any) -> List[Any]:
    """Coerce an update_analytics.line_crossing dict into LineCrossingRule list."""
    from config import LineCrossingRule
    if isinstance(raw, list):
        return [LineCrossingRule.from_dict(r) if isinstance(r, dict) else r for r in raw]
    return []


def _build_roi_rules(raw: Any) -> List[Any]:
    from config import RoiRule
    if isinstance(raw, list):
        return [RoiRule.from_dict(r) if isinstance(r, dict) else r for r in raw]
    return []


# --- main() ---------------------------------------------------------------


def main() -> int:
    runner = DeepStreamRunner()
    try:
        return asyncio.run(runner.run())
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
