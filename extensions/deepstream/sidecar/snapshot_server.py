"""HTTP snapshot server (stdlib ``http.server``).

Exposes one endpoint:

    GET /snapshot/<stream_id>.jpg?token=<token>

Grabs one frame **on demand** from the stream's RTSP output via ffmpeg
and returns it as JPEG. This approach avoids any tee/appsink branch in
the main pipeline (which was found to stall the pipeline — see
STATUS.md design decision #2).

Design:

- **Stdlib only.** Avoids aiohttp so the only third-party runtime dep
  is pyds. ``http.server.ThreadingHTTPServer`` gives us a thread per
  request which is fine for low request rates (operators fetching a
  snapshot every few seconds per stream).
- **Bind address** comes from the ``hello.snapshot_bind_addr`` field
  (e.g. ``127.0.0.1`` or ``0.0.0.0``). Default port 8555.
- **Token validation:** per-stream token stored in
  :class:`SnapshotStore.register_stream`. Missing/unknown token -> 404.
- **ffmpeg subprocess** captures one frame from the RTSP URL (1-2s
  latency per snapshot, acceptable for on-demand operator use).
- **Content-Type:** ``image/jpeg`` on success.
- Runs in its own daemon thread; shutdown is graceful (the server's
  ``shutdown()`` is called from the runner on exit).
"""

from __future__ import annotations

import logging
import secrets
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Dict, Optional
from urllib.parse import parse_qs, urlparse

log = logging.getLogger("deepstream.snapshot_server")


class SnapshotStore:
    """Per-stream snapshot registry with token gating + on-demand capture.

    Stores stream_id → (token, rtsp_url). When a snapshot is requested,
    runs ffmpeg to grab one frame from the RTSP output URL.
    """

    def __init__(self) -> None:
        self._streams: Dict[str, _StreamSlot] = {}
        self._global_lock = threading.Lock()

    def register_stream(
        self, stream_id: str, rtsp_url: str = "", token: Optional[str] = None
    ) -> str:
        """Add ``stream_id`` to the store; returns the snapshot token.

        ``rtsp_url`` is the output RTSP URL used for on-demand frame
        capture. If ``token`` is None, a new token is generated
        (cryptographic random, 32 hex chars).
        """
        tok = token or secrets.token_hex(16)
        with self._global_lock:
            self._streams[stream_id] = _StreamSlot(token=tok, rtsp_url=rtsp_url)
        log.info("snapshot store registered stream %s (rtsp=%s)", stream_id, rtsp_url)
        return tok

    def unregister_stream(self, stream_id: str) -> None:
        with self._global_lock:
            self._streams.pop(stream_id, None)

    def get(self, stream_id: str, token: str) -> Optional[bytes]:
        """Capture one JPEG frame from the stream's RTSP output.

        Runs ffmpeg as a subprocess to grab a single frame. Returns
        JPEG bytes, or None if stream not registered / token mismatch /
        capture failed.
        """
        with self._global_lock:
            slot = self._streams.get(stream_id)
        if slot is None:
            return None
        if not _consttime_eq(slot.token, token):
            return None
        if not slot.rtsp_url:
            return None
        return _capture_frame(slot.rtsp_url)


class _StreamSlot:
    __slots__ = ("token", "rtsp_url")

    def __init__(self, token: str, rtsp_url: str = "") -> None:
        self.token = token
        self.rtsp_url = rtsp_url


def _capture_frame(rtsp_url: str, timeout: int = 10) -> Optional[bytes]:
    """Grab one JPEG frame from ``rtsp_url`` via a one-shot GStreamer pipeline.

    Uses nvv4l2decoder + nvjpegenc for hardware-accelerated decode/encode
    on Jetson. The pipeline is created, played until one buffer arrives on
    the appsink, then set to NULL. Safe to call from the HTTP thread —
    GStreamer element creation is thread-safe after Gst.init().
    """
    import time as _time

    try:
        from gi.repository import Gst  # type: ignore[import-not-found]
    except Exception as e:
        log.warning("GStreamer not available for snapshot: %s", e)
        return None

    desc = (
        f"rtspsrc location={rtsp_url} protocols=tcp latency=200 "
        f"! rtph264depay ! queue max-size-buffers=0 max-size-time=0 max-size-bytes=0 "
        f"! nvv4l2decoder ! queue max-size-buffers=0 max-size-time=0 max-size-bytes=0 "
        f"! nvvideoconvert ! queue max-size-buffers=0 max-size-time=0 max-size-bytes=0 "
        f"! nvjpegenc ! queue max-size-buffers=0 max-size-time=0 max-size-bytes=0 "
        f"! appsink name=sink emit-signals=true sync=false max-buffers=1 drop=true"
    )

    try:
        pl = Gst.parse_launch(desc)
    except Exception as e:
        log.warning("snapshot pipeline parse failed: %s", e)
        return None

    sink = pl.get_by_name("sink")
    if sink is None:
        log.warning("snapshot pipeline has no appsink")
        return None

    jpeg_data: list = []  # mutable container for closure

    def _on_sample(sink: Any) -> Any:
        try:
            sample = sink.emit("pull-sample")
            if sample is None:
                return Gst.FlowReturn.OK
            buf = sample.get_buffer()
            if buf is None:
                return Gst.FlowReturn.OK
            ok, info = buf.map(Gst.MapFlags.READ)
            if ok:
                jpeg_data.append(bytes(info.data))
                buf.unmap(info)
        except Exception as e:
            log.warning("snapshot appsink error: %s", e)
        return Gst.FlowReturn.OK

    sink.connect("new-sample", _on_sample)

    ret = pl.set_state(Gst.State.PLAYING)
    if ret == Gst.StateChangeReturn.FAILURE:
        log.warning("snapshot pipeline failed to start")
        pl.set_state(Gst.State.NULL)
        return None

    deadline = _time.monotonic() + timeout
    while not jpeg_data and _time.monotonic() < deadline:
        _time.sleep(0.1)

    pl.set_state(Gst.State.NULL)

    if jpeg_data:
        return jpeg_data[0]
    log.warning("snapshot timed out (%ds) for %s", timeout, rtsp_url)
    return None


def _consttime_eq(a: str, b: str) -> bool:
    """Constant-time string compare (defends against timing attacks)."""
    return secrets.compare_digest(a, b)


# --- HTTP handler ----------------------------------------------------------


def _make_handler(store: SnapshotStore) -> type:
    """Build a handler class that closes over the snapshot store."""

    class _SnapshotHandler(BaseHTTPRequestHandler):
        # Quieter logging — BaseHTTPRequestHandler logs every line by default.
        def log_message(self, format: str, *args) -> None:  # noqa: A002
            log.debug("snapshot http: " + format, *args)

        def do_GET(self) -> None:  # noqa: N802 - stdlib API name
            try:
                self._handle_get()
            except Exception as e:
                log.exception("snapshot handler error: %s", e)
                self._send_text(500, f"internal error: {e}")

        def _handle_get(self) -> None:
            parsed = urlparse(self.path)
            qs = parse_qs(parsed.query)
            path = parsed.path

            # Health endpoint for liveness probes (no token required).
            if path in ("/health", "/healthz"):
                self._send_text(200, "ok")
                return

            # /snapshot/<stream_id>.jpg
            if not path.startswith("/snapshot/"):
                self._send_text(404, "not found")
                return
            rest = path[len("/snapshot/"):]
            if "/" in rest or not rest.endswith(".jpg"):
                self._send_text(404, "not found")
                return
            stream_id = rest[:-len(".jpg")]
            if not stream_id:
                self._send_text(404, "not found")
                return

            token_vals = qs.get("token", [])
            token = token_vals[0] if token_vals else ""

            jpeg = store.get(stream_id, token)
            if jpeg is None:
                # Ambiguous: could be missing stream, bad token, or no
                # capture yet. We return 404 for all three to avoid
                # leaking stream existence to unauthenticated callers.
                self._send_text(404, "not found")
                return
            self.send_response(200)
            self.send_header("Content-Type", "image/jpeg")
            self.send_header("Content-Length", str(len(jpeg)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(jpeg)

        def _send_text(self, code: int, msg: str) -> None:
            body = msg.encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return _SnapshotHandler


class SnapshotServer:
    """Wraps a :class:`ThreadingHTTPServer` running in a daemon thread."""

    def __init__(
        self,
        store: SnapshotStore,
        *,
        bind_addr: str = "127.0.0.1",
        port: int = 8555,
    ) -> None:
        handler_cls = _make_handler(store)
        self._server = ThreadingHTTPServer((bind_addr, port), handler_cls)
        self._server.daemon_threads = True
        self._thread = threading.Thread(
            target=self._server.serve_forever,
            name="ds-snapshot-http",
            daemon=True,
        )
        self.bind_addr = bind_addr
        self.port = port

    def start(self) -> None:
        log.info("snapshot server listening on %s:%d", self.bind_addr, self.port)
        self._thread.start()

    def stop(self) -> None:
        log.info("snapshot server stopping")
        try:
            self._server.shutdown()
        except Exception as e:
            log.warning("snapshot server shutdown error: %s", e)
        try:
            self._server.server_close()
        except Exception:
            pass
        # Don't join — daemon thread, will exit when process does.

    @property
    def base_url(self) -> str:
        return f"http://{self.bind_addr}:{self.port}"
