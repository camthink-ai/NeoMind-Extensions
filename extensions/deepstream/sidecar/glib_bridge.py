"""GLib MainLoop <-> asyncio integration.

GLib's MainLoop is the only correct way to drive a GStreamer pipeline
(bus watch, probe callbacks, etc.) in Python. asyncio is the natural
fit for the stdin/stdout line-protocol. They can't share a single
event loop in pure Python (gbulb exists but adds a third-party dep).

**Choice: Option A from plan §8.6a** — run GLib MainLoop in a dedicated
thread; cross to the asyncio main thread via
``asyncio.run_coroutine_threadsafe``. Cross back from asyncio -> GLib
via :func:`GLib.idle_add`.

Thread topology (after :class:`Bridge.start`):

- **Main thread:** asyncio event loop, reads stdin line-by-line,
  dispatches control messages. Calls into GLib via ``bridge.call_from_asyncio``.
- **GLib thread:** runs ``MainLoop.run()``. All Gst state changes and
  probe callbacks happen here. Calls into asyncio via
  ``bridge.call_from_asyncio`` (which itself uses
  ``asyncio.run_coroutine_threadsafe``).

Both directions are non-blocking: GLib idle callbacks are short, and
asyncio coroutines are awaited by the main loop. The only shared
mutable state is the stream registry (owned by the runner, guarded by
a ``threading.Lock``).

**Smoke test pattern** (Task 8.6a Step 2):

    bridge = Bridge()
    bridge.start()
    # create a fake pipeline (videotestsrc -> fakesink), watch bus
    # for "state-changed" message, emit it via bridge.call_from_asyncio
"""

from __future__ import annotations

import asyncio
import logging
import threading
from typing import Any, Callable, Optional

# GLib is only importable inside the container.
try:
    import gi
    gi.require_version("Gst", "1.0")
    gi.require_version("GLib", "2.0")
    from gi.repository import GLib, Gst  # noqa: F401
    _GLIB_OK = True
except Exception as _exc:  # pragma: no cover - macOS dev path
    GLib = None  # type: ignore[assignment]
    Gst = None  # type: ignore[assignment]
    _GLIB_OK = False
    _IMPORT_ERROR = _exc

log = logging.getLogger("deepstream.glib_bridge")


def require_glib() -> None:
    if not _GLIB_OK:
        raise RuntimeError(
            f"GLib/Gst not available — glib_bridge requires the ds:7.1-pyds "
            f"container. Original error: {_IMPORT_ERROR!r}"
        )


class Bridge:
    """Owns the GLib MainLoop thread + cross-thread call helpers."""

    def __init__(self, asyncio_loop: Optional[asyncio.AbstractEventLoop] = None) -> None:
        require_glib()
        self._loop = GLib.MainLoop()
        self._thread: Optional[threading.Thread] = None
        self._asyncio_loop: Optional[asyncio.AbstractEventLoop] = asyncio_loop
        self._started = threading.Event()

    @property
    def loop(self) -> Any:
        return self._loop

    @property
    def asyncio_loop(self) -> Optional[asyncio.AbstractEventLoop]:
        return self._asyncio_loop

    def set_asyncio_loop(self, loop: asyncio.AbstractEventLoop) -> None:
        """Bind the asyncio loop. Call from the asyncio main thread."""
        self._asyncio_loop = loop

    def start(self) -> None:
        """Spawn the GLib thread and start MainLoop.run()."""
        if self._thread is not None:
            raise RuntimeError("bridge already started")
        self._thread = threading.Thread(
            target=self._run,
            name="ds-glib-mainloop",
            daemon=True,
        )
        self._thread.start()
        # Wait until GLib is actually spinning so callers can
        # immediately schedule idle callbacks.
        if not self._started.wait(timeout=5.0):
            raise RuntimeError("GLib MainLoop failed to start within 5s")

    def _run(self) -> None:
        log.info("GLib MainLoop thread starting")
        # Signal start *before* run() so callers don't race with the
        # initialization. We use idle_add to defer the signal until GLib
        # is actually processing events.
        GLib.idle_add(self._signal_started)
        try:
            self._loop.run()
        except Exception:
            log.exception("GLib MainLoop crashed")
            raise
        finally:
            log.info("GLib MainLoop thread exiting")

    def _signal_started(self) -> bool:
        self._started.set()
        # Returning False removes the idle source (one-shot).
        return False

    def stop(self) -> None:
        """Stop the GLib MainLoop. Safe to call from any thread."""
        if self._loop is None:
            return
        try:
            self._loop.quit()
        except Exception as e:
            log.warning("MainLoop.quit() error: %s", e)
        if self._thread is not None and self._thread.is_alive():
            self._thread.join(timeout=5.0)
            if self._thread.is_alive():
                log.warning("GLib thread did not exit within 5s (will be killed as daemon)")
        self._thread = None

    # --- Cross-thread helpers --------------------------------------------

    def call_from_glib(self, fn: Callable[[], Any]) -> None:
        """Schedule ``fn`` to run on the GLib thread.

        Use this when the caller is in the asyncio main thread and wants
        to mutate Gst state (set pipeline to PLAYING, add/remove elements,
        attach probes). ``fn`` runs in the next GLib idle cycle.
        """
        require_glib()
        def _wrap() -> bool:
            try:
                fn()
            except Exception:
                log.exception("call_from_glib callback raised")
            return False  # one-shot
        GLib.idle_add(_wrap)

    def call_from_asyncio(self, coro_or_fn: Any) -> Any:
        """Cross from GLib -> asyncio.

        ``coro_or_fn`` is either a coroutine (scheduled via
        ``run_coroutine_threadsafe``) or a sync callable (scheduled via
        ``call_soon_threadsafe``). Returns the concurrent.futures.Future
        for coroutines so the GLib side can await if needed.
        """
        if self._asyncio_loop is None:
            raise RuntimeError("asyncio loop not bound — call set_asyncio_loop first")
        if asyncio.iscoroutine(coro_or_fn):
            return asyncio.run_coroutine_threadsafe(coro_or_fn, self._asyncio_loop)
        return self._asyncio_loop.call_soon_threadsafe(coro_or_fn)
