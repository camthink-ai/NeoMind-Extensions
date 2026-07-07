"""HTTP snapshot server (stdlib ``http.server``).

Exposes one endpoint:

    GET /snapshot/<stream_id>.jpg?token=<token>

Returns the latest JPEG from the stream's snapshot ring buffer
(populated by the appsink callback wired by :mod:`pipeline_builder`).

Design:

- **Stdlib only.** Avoids aiohttp so the only third-party runtime dep
  is pyds. ``http.server.ThreadingHTTPServer`` gives us a thread per
  request which is fine for low request rates (operators fetching a
  snapshot every few seconds per stream).
- **Bind address** comes from the ``hello.snapshot_bind_addr`` field
  (e.g. ``127.0.0.1`` or ``0.0.0.0``). Default port 8555.
- **Token validation:** per-stream token stored in
  :class:`SnapshotStore.register_stream`. Missing/unknown token -> 401.
- **Missing stream / no JPEG yet:** 404.
- **Content-Type:** ``image/jpeg`` on success.
- Runs in its own daemon thread; shutdown is graceful (the server's
  ``shutdown()`` is called from the runner on exit).

JPEG buffer ownership:

- :class:`SnapshotStore` is a process-wide singleton owned by the runner.
- The appsink ``new-sample`` callback (registered by pipeline_builder)
  pushes the latest JPEG bytes into the store.
- HTTP handlers pull the latest bytes (one-shot read, no copy).
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
    """Thread-safe per-stream latest-JPEG registry with token gating.

    The store has one slot per stream_id. Latest write wins; readers
    always get the most recent bytes. Locking is per-stream so a slow
    HTTP read on one stream doesn't block writes on another.
    """

    def __init__(self) -> None:
        self._streams: Dict[str, _StreamSlot] = {}
        self._global_lock = threading.Lock()

    def register_stream(self, stream_id: str, token: Optional[str] = None) -> str:
        """Add ``stream_id`` to the store; returns the snapshot token.

        If ``token`` is None, a new token is generated (cryptographic
        random, 32 hex chars). Re-registering an existing stream_id
        overwrites the token (used by re-add after remove).
        """
        tok = token or secrets.token_hex(16)
        with self._global_lock:
            self._streams[stream_id] = _StreamSlot(token=tok)
        log.info("snapshot store registered stream %s", stream_id)
        return tok

    def unregister_stream(self, stream_id: str) -> None:
        with self._global_lock:
            self._streams.pop(stream_id, None)

    def push_jpeg(self, stream_id: str, jpeg_bytes: bytes) -> None:
        """Called from the appsink callback (GLib thread)."""
        with self._global_lock:
            slot = self._streams.get(stream_id)
        if slot is None:
            return
        with slot.lock:
            slot.bytes = jpeg_bytes
            slot.size = len(jpeg_bytes)

    def get(self, stream_id: str, token: str) -> Optional[bytes]:
        """Return JPEG bytes for ``stream_id`` if token matches.

        Returns None if stream not registered, token mismatch, or no
        JPEG captured yet.
        """
        with self._global_lock:
            slot = self._streams.get(stream_id)
        if slot is None:
            return None
        if not _consttime_eq(slot.token, token):
            return None
        with slot.lock:
            return slot.bytes


class _StreamSlot:
    __slots__ = ("token", "bytes", "size", "lock")

    def __init__(self, token: str) -> None:
        self.token = token
        self.bytes: Optional[bytes] = None
        self.size: int = 0
        self.lock = threading.Lock()


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
