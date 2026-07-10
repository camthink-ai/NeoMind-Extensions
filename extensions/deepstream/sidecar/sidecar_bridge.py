#!/usr/bin/env python3
"""TCP bridge daemon for the DeepStream sidecar.

Bridges the JSONL protocol between a remote NeoMind host (running the
``deepstream`` Rust extension in ``sidecar_mode=remote``) and a locally-spawned
``deepstream_runner.py`` child process. This enables the "NeoMind on macOS /
sidecar on Jetson" deployment topology (路 C).

Protocol:
  * The Rust extension connects on the configured ``sidecar_host:sidecar_port``
    (default 9556).
  * On connect the bridge spawns ``deepstream_runner.py`` and pumps bytes
    bidirectionally: socket bytes → sidecar stdin; sidecar stdout bytes → socket.
  * The JSONL framing is preserved 1:1; the bridge is byte-opaque.
  * On socket close: bridge SIGTERMs the sidecar, waits up to 5s, SIGKILLs if
    still alive, then waits for another connection.
  * Only ONE client at a time — a second connection arriving while a sidecar
    is running is rejected with an explanatory JSON ``error_response`` line
    so the caller logs and backs off (the Rust supervisor reconnects on its
    own backoff schedule).

The daemon deliberately depends only on Python 3 stdlib so it can run on a
minimal Jetson image without pip-installing anything beyond what
``deepstream_runner.py`` itself needs.

Usage::

    python3 sidecar_bridge.py                       # 0.0.0.0:9556
    python3 sidecar_bridge.py --host 0.0.0.0 --port 9556
    SIDECAR_BRIDGE_PORT=10000 python3 sidecar_bridge.py

Environment overrides:
  * ``SIDECAR_BRIDGE_HOST`` / ``SIDECAR_BRIDGE_PORT``  — bind address.
  * ``DEEPSTREAM_SIDECAR_PATH``  — absolute path to ``deepstream_runner.py``
    (default: sibling file in the same directory as this script).
  * ``SIDECAR_PYTHON_BIN``  — interpreter to use (default: ``python3``).
    Set to e.g. ``/usr/bin/python3.10`` if the pyds-installed interpreter
    isn't on $PATH.
  * ``SIDECAR_SPAWN_CMD``  — **full argv override**. When set, the bridge
    ignores ``SIDECAR_PYTHON_BIN`` / ``DEEPSTREAM_SIDECAR_PATH`` and uses
    ``shlex.split($SIDECAR_SPAWN_CMD)`` as the child argv. The argv is
    then re-joined with ``shlex.quote`` and spawned via the shell
    (``/bin/sh -c``). This is necessary because Docker CLI 29.x on
    Jetson doesn't forward container stdout to subprocess pipes when
    spawned via ``create_subprocess_exec`` — the shell path avoids
    this. Use this when the sidecar must run inside a container, e.g.::

        SIDECAR_SPAWN_CMD='docker run --rm -i --runtime=nvidia --network=host \\
            -v /home/box/ds-deps/sidecar:/srv/sidecar \\
            -v /home/box/ds-engines:/engines \\
            ds:7.1-pyds-gi python3 /srv/sidecar/deepstream_runner.py'

    The container **must** be started with ``-i`` (keep stdin open) and
    **must not** attach a TTY (``-t``), because the bridge pumps the
    JSONL protocol over stdin/stdout. ``--rm`` is strongly recommended
    so the container cleans up on disconnect.
"""

from __future__ import annotations

import argparse
import asyncio
import logging
import os
import shlex
import signal
import socket
import sys
import tempfile
from pathlib import Path
from typing import Optional

LOG = logging.getLogger("sidecar_bridge")

# JSONL line length cap — matches the Rust side protocol::MAX_MESSAGE_BYTES (4 MiB).
# Pumping in chunks this size keeps memory bounded.
CHUNK_SIZE = 64 * 1024

# How long to wait for the sidecar to exit after SIGTERM before SIGKILL.
GRACEFUL_SECS = 5.0

# Default port — must match the Rust side's DEFAULT_SIDECAR_BRIDGE_PORT (9556).
DEFAULT_PORT = 9556


# Locked globally — only one sidecar may be active at a time. A second client
# arriving while this lock is held is rejected with an explanatory JSON line.
_active_client_lock = asyncio.Lock()


def _resolve_runner_path() -> Path:
    """Locate ``deepstream_runner.py`` next to this script (or via env override)."""
    override = os.environ.get("DEEPSTREAM_SIDECAR_PATH")
    if override:
        p = Path(override)
        if not p.is_file():
            raise FileNotFoundError(
                f"DEEPSTREAM_SIDECAR_PATH={override} is not a file"
            )
        return p
    # Default: sibling file in same directory as this script.
    return Path(__file__).resolve().parent / "deepstream_runner.py"


def _resolve_python_bin() -> str:
    return os.environ.get("SIDECAR_PYTHON_BIN", "python3")


def _spawn_argv(runner_path: Path) -> list[str]:
    """Build the child argv for spawning the sidecar.

    When ``SIDECAR_SPAWN_CMD`` is set, it completely replaces the default
    ``[python_bin, runner_path]`` argv — use this to wrap the sidecar in a
    container invocation (Docker, podman, etc.). The command is split with
    ``shlex.split`` (POSIX rules) so quotes are respected.

    Otherwise returns ``[python_bin, str(runner_path)]`` — the legacy direct
    invocation.
    """
    override = os.environ.get("SIDECAR_SPAWN_CMD")
    if override:
        argv = shlex.split(override)
        if not argv:
            raise ValueError("SIDECAR_SPAWN_CMD is set but splits to empty argv")
        return argv
    return [_resolve_python_bin(), str(runner_path)]


def _detect_lan_ips() -> list[str]:
    """Best-effort enumerate non-loopback IPv4 addresses on this machine.

    Walks UDP socket ``connect`` trick (cross-platform, no third-party dep):
    open a dummy socket to a public address, read ``getsockname()``. Then
    falls back to ``socket.gethostbyname_ex(gethostname())`` for multi-IP
    hosts. Deduplicates, drops loopback / link-local.

    Used at startup so the operator can copy the right address into
    NeoMind's ``sidecar_host`` config field without running ``ip addr``.
    """
    out: list[str] = []
    # Primary: the "fake connect" trick returns the source IP routing would pick.
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            # 192.0.2.0/24 is TEST-NET-1 (RFC 5737) — never a real destination.
            # We only need the kernel to resolve a route; no packets go out.
            s.connect(("192.0.2.1", 1))
            ip = s.getsockname()[0]
            if ip and not ip.startswith("127.") and ip != "0.0.0.0":
                out.append(ip)
        finally:
            s.close()
    except OSError:
        pass

    # Secondary: hostname → IP(s). Picks up addresses on interfaces the
    # fake-connect trick didn't reveal (e.g. extra NICs, bonded links).
    try:
        _, _, ips = socket.gethostbyname_ex(socket.gethostname())
        for ip in ips:
            if ip not in out and not ip.startswith("127.") and not ip.startswith("169.254."):
                out.append(ip)
    except OSError:
        pass

    return out


async def _pump_socket_to_stdin(reader: asyncio.StreamReader, proc: asyncio.subprocess.Process) -> None:
    """Socket bytes → sidecar stdin. Returns when socket closes (EOF) or stdin breaks."""
    try:
        stdin = proc.stdin
        assert stdin is not None
        while True:
            data = await reader.read(CHUNK_SIZE)
            if not data:
                LOG.info("client closed connection (EOF on socket)")
                break
            stdin.write(data)
            await stdin.drain()
    except (ConnectionResetError, BrokenPipeError) as e:
        LOG.info("socket→stdin pump ended: %r", e)
    except asyncio.CancelledError:
        raise
    except Exception as e:
        LOG.exception("unexpected error in socket→stdin pump: %r", e)
    finally:
        # Close stdin so the sidecar sees EOF even if the socket closed
        # abnormally — this is the primary cue for deepstream_runner.py to exit.
        try:
            stdin.close()
        except Exception:
            pass


async def _pump_stdout_to_socket(proc_stdout: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    """Sidecar stdout bytes → socket. Returns when stdout closes (sidecar exited)."""
    try:
        while True:
            data = await proc_stdout.read(CHUNK_SIZE)
            if not data:
                LOG.info("sidecar stdout closed (process exiting)")
                break
            writer.write(data)
            await writer.drain()
    except (ConnectionResetError, BrokenPipeError) as e:
        LOG.info("stdout→socket pump ended: %r", e)
    except asyncio.CancelledError:
        raise
    except Exception as e:
        LOG.exception("unexpected error in stdout→socket pump: %r", e)


async def _terminate_sidecar(proc: asyncio.subprocess.Process) -> int:
    """SIGTERM → wait GRACEFUL_SECS → SIGKILL. Returns the process exit code."""
    if proc.returncode is not None:
        return proc.returncode
    try:
        proc.send_signal(signal.SIGTERM)
    except ProcessLookupError:
        return proc.returncode if proc.returncode is not None else -1

    try:
        code = await asyncio.wait_for(proc.wait(), timeout=GRACEFUL_SECS)
        LOG.info("sidecar exited with code %d after SIGTERM", code)
        return code
    except asyncio.TimeoutError:
        LOG.warning("sidecar did not exit within %.0fs after SIGTERM; sending SIGKILL", GRACEFUL_SECS)
        try:
            proc.kill()
        except ProcessLookupError:
            pass
        code = await proc.wait()
        LOG.info("sidecar exited with code %d after SIGKILL", code)
        return code


async def _pump_file_to_socket(file_path: str, writer: asyncio.StreamWriter,
                                proc: asyncio.subprocess.Process) -> None:
    """Tail a file → socket. Used when spawning via Docker (file-redirect mode).

    Docker CLI 29.x on Jetson doesn't forward container stdout to subprocess
    pipes — only to regular file FDs. This pump opens the file that Docker
    writes to, reads new bytes as they appear, and forwards them to the TCP
    socket. Polls every 100ms (low enough for responsive JSONL, high enough
    to avoid CPU spin).
    """
    loop = asyncio.get_running_loop()
    try:
        # Wait for the file to appear (Docker creates it on spawn).
        deadline = loop.time() + 10.0
        while not os.path.exists(file_path):
            if proc.returncode is not None:
                LOG.info("sidecar exited before stdout file appeared")
                return
            if loop.time() > deadline:
                LOG.warning("stdout file %s never appeared (10s)", file_path)
                return
            await asyncio.sleep(0.1)

        f = await loop.run_in_executor(None, open, file_path, "rb")
        try:
            while True:
                data = await loop.run_in_executor(None, f.read, CHUNK_SIZE)
                if data:
                    writer.write(data)
                    await writer.drain()
                else:
                    # No data right now — either EOF (file closed) or caught up.
                    # Check if the sidecar process is still alive.
                    if proc.returncode is not None:
                        # Drain any remaining bytes, then done.
                        remaining = await loop.run_in_executor(None, f.read, CHUNK_SIZE)
                        if remaining:
                            writer.write(remaining)
                            await writer.drain()
                        break
                    await asyncio.sleep(0.1)  # poll interval
        finally:
            await loop.run_in_executor(None, f.close)
    except (ConnectionResetError, BrokenPipeError) as e:
        LOG.info("file→socket pump ended: %r", e)
    except asyncio.CancelledError:
        raise
    except Exception as e:
        LOG.exception("unexpected error in file→socket pump: %r", e)


async def _handle_client(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    """Handle one TCP client: spawn sidecar, pump bytes both ways, then reap."""
    peer = writer.get_extra_info("peername")
    LOG.info("client connected: %s", peer)

    if _active_client_lock.locked():
        # A sidecar is already running for another client. Reject so the
        # Rust supervisor's connect fails fast and it retries on backoff —
        # otherwise we'd queue up behind the existing connection indefinitely.
        LOG.warning("rejecting client %s: another sidecar is active", peer)
        try:
            writer.write(
                b'{"type":"error_response","code":"bridge_busy",'
                b'"message":"another sidecar session is active"}\n'
            )
            await writer.drain()
        except Exception:
            pass
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass
        return

    async with _active_client_lock:
        # Spawn deepstream_runner.py.
        runner = _resolve_runner_path()
        argv = _spawn_argv(runner)
        use_shell = bool(os.environ.get("SIDECAR_SPAWN_CMD"))
        stdout_file: Optional[str] = None
        LOG.info("spawning sidecar: argv=%s (use_shell=%s)", argv, use_shell)

        try:
            if use_shell:
                # Docker CLI 29.x on Jetson has a fundamental bug: it does
                # NOT forward container stdout to subprocess pipes (PIPE).
                # Only regular-file FDs work. So we redirect Docker's stdout
                # to a temp file and tail it from a separate pump task.
                # Stdin still uses a pipe (Docker reads stdin fine).
                stdout_fd, stdout_file = tempfile.mkstemp(
                    prefix="sidecar_stdout_", suffix=".log",
                )
                os.close(stdout_fd)  # just needed the path
                # Truncate so the file starts empty.
                open(stdout_file, "w").close()
                shell_cmd = " ".join(shlex.quote(a) for a in argv)
                full_cmd = f"exec {shell_cmd} > {shlex.quote(stdout_file)} 2>&1"
                LOG.info("shell spawn (file-redirect): %s", full_cmd)
                proc = await asyncio.create_subprocess_shell(
                    full_cmd,
                    stdin=asyncio.subprocess.PIPE,
                    stdout=asyncio.subprocess.DEVNULL,
                    stderr=None,
                )
            else:
                proc = await asyncio.create_subprocess_exec(
                    *argv,
                    stdin=asyncio.subprocess.PIPE,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=None,  # inherit — sidecar logs land in the bridge's stderr
                )
        except Exception as e:
            LOG.exception("failed to spawn sidecar: %r", e)
            if stdout_file:
                try:
                    os.unlink(stdout_file)
                except OSError:
                    pass
            try:
                writer.write(
                    f'{{"type":"error_response","code":"bridge_spawn_failed",'
                    f'"message":"sidecar spawn failed: {e}"}}\n'.encode()
                )
                await writer.drain()
            except Exception:
                pass
            writer.close()
            try:
                await writer.wait_closed()
            except Exception:
                pass
            return

        # Run both pumps concurrently. Whichever finishes first will be
        # awaited; the other is cancelled in the finally block below. The
        # sidecar is reaped unconditionally after the pumps end so we never
        # leak a python process across client disconnects.
        if use_shell and stdout_file:
            pumps = [
                asyncio.create_task(_pump_socket_to_stdin(reader, proc)),
                asyncio.create_task(_pump_file_to_socket(stdout_file, writer, proc)),
            ]
        else:
            pumps = [
                asyncio.create_task(_pump_socket_to_stdin(reader, proc)),
                asyncio.create_task(_pump_stdout_to_socket(proc.stdout, writer)),
            ]
        try:
            # Wait for EITHER pump to finish. The other may still be blocked
            # on read() — cancelling it unblocks the wait.
            done, pending = await asyncio.wait(pumps, return_when=asyncio.FIRST_COMPLETED)
            LOG.info("bridge: a pump finished (sidecar returncode=%s)", proc.returncode)
        except asyncio.CancelledError:
            for t in pumps:
                t.cancel()
            raise
        finally:
            for t in pumps:
                if not t.done():
                    t.cancel()
            # Close the socket — even if stdout pump is still draining, the
            # client is going away (or the sidecar is) and we must not leak
            # a half-open TCP connection.
            try:
                writer.close()
                await writer.wait_closed()
            except Exception:
                pass

        # Reap the sidecar regardless of which side closed first.
        await _terminate_sidecar(proc)
        # Clean up the temp stdout file (Docker mode only).
        if stdout_file:
            try:
                os.unlink(stdout_file)
            except OSError:
                pass
        LOG.info("client %s session complete", peer)


async def _run(host: str, port: int) -> None:
    server = await asyncio.start_server(_handle_client, host, port, reuse_address=True)
    addrs = ", ".join(str(s.getsockname()) for s in server.sockets)
    LOG.info("sidecar_bridge listening on %s", addrs)
    LOG.info("runner path: %s", _resolve_runner_path())
    if os.environ.get("SIDECAR_SPAWN_CMD"):
        LOG.info("spawn cmd:   %s", os.environ["SIDECAR_SPAWN_CMD"])
    else:
        LOG.info("python bin:  %s", _resolve_python_bin())

    # Print detected LAN IPs so the operator can copy the right address
    # into NeoMind's `sidecar_host` config field. We never hard-code an IP
    # — the machine's own network stack is the source of truth.
    lan_ips = _detect_lan_ips()
    if lan_ips:
        LOG.info("detected LAN IPs (use one as NeoMind's sidecar_host):")
        for ip in lan_ips:
            LOG.info("  → %s  (connect: %s:%d)", ip, ip, port)
    else:
        LOG.warning(
            "no LAN IPs detected; set sidecar_host manually "
            "(check with `ip addr` or `ifconfig`)"
        )

    # Graceful SIGINT/SIGTERM — needed so systemd / docker stop don't hang.
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(sig, stop.set)
        except NotImplementedError:
            # Windows — signal handlers on the event loop aren't supported.
            pass

    async with server:
        serve_task = asyncio.create_task(server.serve_forever())
        await stop.wait()
        LOG.info("shutdown signal received; stopping server")
        serve_task.cancel()
        try:
            await serve_task
        except asyncio.CancelledError:
            pass


def _parse_args(argv: Optional[list] = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="DeepStream sidecar TCP bridge daemon")
    p.add_argument("--host", default=os.environ.get("SIDECAR_BRIDGE_HOST", "0.0.0.0"),
                   help="bind address (default 0.0.0.0; env SIDECAR_BRIDGE_HOST)")
    p.add_argument("--port", type=int,
                   default=int(os.environ.get("SIDECAR_BRIDGE_PORT", DEFAULT_PORT)),
                   help="bind port (default 9556; env SIDECAR_BRIDGE_PORT)")
    p.add_argument("--log-level", default=os.environ.get("SIDECAR_BRIDGE_LOG_LEVEL", "info"),
                   choices=["debug", "info", "warning", "error"],
                   help="log level (default info)")
    return p.parse_args(argv)


def main(argv: Optional[list] = None) -> int:
    args = _parse_args(argv)
    logging.basicConfig(
        level=args.log_level.upper(),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        stream=sys.stderr,
    )
    try:
        asyncio.run(_run(args.host, args.port))
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
