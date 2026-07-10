#!/usr/bin/env python3
"""Mock sidecar for deepstream integration tests.

Speaks the JSONL protocol defined in extensions/deepstream/src/protocol.rs
but with no DeepStream/GPU dependency. Lets Rust supervisor tests run on any host.

Env vars:
  MOCK_SCRIPT_PATH     Path to a .jsonl file whose lines are emitted to stdout
                       on a 100ms timer (flood/backpressure testing).
  MOCK_DIE_AT_SECONDS  If set, hard-exit (code 139, simulating segfault) after
                       N seconds via a background thread.
  MOCK_IGNORE_HEALTHCHECK  If "1"/"true"/"yes", health_check pings are silently
                           dropped (no pong). Used by heartbeat timeout tests.
  MOCK_IGNORE_SHUTDOWN If "1"/"true"/"yes", shutdown control messages are silently
                       dropped (no bye, no exit). Used by shutdown SIGTERM tests.
  MOCK_IGNORE_SIGTERM   If "1"/"true"/"yes", the SIGTERM handler is NOT installed
                       so SIGTERM kills the process outright. Used by shutdown
                       SIGKILL escalation tests.
  MOCK_IGNORE_ADD_STREAM If "1"/"true"/"yes", add_stream control messages are
                        silently dropped (no stream_added emitted). Used by
                        config replay timeout tests.
"""
import json
import os
import signal
import sys
import threading
import time

PROTOCOL_VER = 1
DS_VER = "7.1.0-mock"
PYDS_VER = "1.1.1-mock"
RTSP_PREFIX = "rtsp://mock:8554/ds/"

# If true, health_check pings are silently dropped (no pong emitted).
# Lets the Rust heartbeat task test the timeout path without a real hang.
MOCK_IGNORE_HEALTHCHECK = os.environ.get("MOCK_IGNORE_HEALTHCHECK", "").lower() in ("1", "true", "yes")

# If true, shutdown control messages are silently dropped (no bye, no exit).
# Lets the Rust shutdown() test the SIGTERM escalation path.
MOCK_IGNORE_SHUTDOWN = os.environ.get("MOCK_IGNORE_SHUTDOWN", "").lower() in ("1", "true", "yes")

# If true, the SIGTERM handler is NOT installed so SIGTERM kills the process
# via the default action. Lets the Rust shutdown() test the SIGKILL escalation path.
MOCK_IGNORE_SIGTERM = os.environ.get("MOCK_IGNORE_SIGTERM", "").lower() in ("1", "true", "yes")

# If true, add_stream control messages are silently dropped (no stream_added
# emitted). Lets the Rust replay-to test exercise the per-stream timeout path.
MOCK_IGNORE_ADD_STREAM = os.environ.get("MOCK_IGNORE_ADD_STREAM", "").lower() in ("1", "true", "yes")

# Global shutdown flag
_stopping = False


def emit(obj):
    """Write one JSON object + newline to stdout, flush."""
    line = json.dumps(obj, separators=(",", ":"))
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def log(msg):
    """Log to stderr."""
    sys.stderr.write(f"[mock_sidecar] {msg}\n")
    sys.stderr.flush()


def emit_ready():
    emit({
        "type": "ready",
        "ds_ver": DS_VER,
        "pyds_ver": PYDS_VER,
        "protocol_ver": PROTOCOL_VER,
        "gpu_info": {"name": "mock", "mem_mb": 0},
    })


def emit_bye(reason, exit_code):
    emit({"type": "bye", "reason": reason, "exit_code": exit_code})


def handle_control(msg):
    """Translate one control message into zero or more events."""
    t = msg.get("type")
    msg_id = msg.get("id", "")

    if t == "hello":
        emit({
            "type": "hello_ack",
            "max_streams": 32,
            "rtsp_url_prefix": RTSP_PREFIX,
            "models_loaded": ["yolov8n-coco"],
        })
    elif t == "add_stream":
        if MOCK_IGNORE_ADD_STREAM:
            log("add_stream ignored (MOCK_IGNORE_ADD_STREAM=true)")
            return
        cfg = msg.get("config", {})
        stream_id = cfg.get("stream_id") or msg_id
        emit({
            "type": "stream_added",
            "id": msg_id,
            "stream_id": stream_id,
            "rtsp_url": f"{RTSP_PREFIX}{stream_id}",
            "snapshot_token": f"mock-token-{stream_id}",
        })
    elif t == "remove_stream":
        emit({
            "type": "stream_removed",
            "id": msg_id,
            "stream_id": msg.get("stream_id", ""),
        })
    elif t in ("update_analytics", "set_threshold"):
        stream_id = msg.get("stream_id", msg_id)
        # Ack by re-emitting stream_added (mock simplification)
        emit({
            "type": "stream_added",
            "id": msg_id,
            "stream_id": stream_id,
            "rtsp_url": f"{RTSP_PREFIX}{stream_id}",
            "snapshot_token": f"mock-token-{stream_id}",
        })
    elif t == "list_state":
        emit({
            "type": "error_response",
            "id": msg_id,
            "code": "NOT_IMPLEMENTED",
            "message": "list_state not supported in mock",
        })
    elif t == "health_check":
        if MOCK_IGNORE_HEALTHCHECK:
            log("health_check ignored (MOCK_IGNORE_HEALTHCHECK=true)")
            # Silently drop — supervisor heartbeat should time out
        else:
            emit({"type": "pong", "ts": msg.get("ts", 0)})
    elif t == "shutdown":
        if MOCK_IGNORE_SHUTDOWN:
            log("shutdown ignored (MOCK_IGNORE_SHUTDOWN=true)")
            return
        emit_bye("graceful", 0)
        sys.exit(0)
    else:
        emit({
            "type": "error_response",
            "id": msg_id,
            "code": "UNKNOWN_COMMAND",
            "message": f"mock doesn't understand {t!r}",
        })


def sigterm_handler(signum, frame):
    global _stopping
    _stopping = True
    emit_bye("sigterm", 0)
    sys.exit(0)


def die_after_thread(seconds):
    """Background thread that hard-exits with code 139 after N seconds."""
    def worker():
        time.sleep(seconds)
        # Simulate segfault: bypass Python cleanup
        os._exit(139)
    t = threading.Thread(target=worker, daemon=True)
    t.start()


def emit_scripted_file(path):
    """Emit each line of a .jsonl file to stdout on a 100ms cadence."""
    try:
        with open(path, "r") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                # Validate it's JSON, then pass through verbatim
                try:
                    json.loads(line)
                except json.JSONDecodeError:
                    log(f"bad json in script file: {line!r}")
                    continue
                sys.stdout.write(line + "\n")
                sys.stdout.flush()
                time.sleep(0.1)
    except FileNotFoundError:
        log(f"MOCK_SCRIPT_PATH={path} not found, ignoring")


def main():
    if not MOCK_IGNORE_SIGTERM:
        signal.signal(signal.SIGTERM, sigterm_handler)
    signal.signal(signal.SIGINT, sigterm_handler)  # SIGINT always handled

    die_at = os.environ.get("MOCK_DIE_AT_SECONDS")
    if die_at:
        try:
            die_after_thread(float(die_at))
        except ValueError:
            log(f"bad MOCK_DIE_AT_SECONDS={die_at!r}, ignoring")

    emit_ready()

    script_path = os.environ.get("MOCK_SCRIPT_PATH")
    if script_path:
        # Emit scripted events in the background
        threading.Thread(
            target=emit_scripted_file, args=(script_path,), daemon=True
        ).start()

    # Main loop: read JSONL from stdin
    for raw in sys.stdin:
        raw = raw.strip()
        if not raw:
            continue
        try:
            msg = json.loads(raw)
        except json.JSONDecodeError as e:
            log(f"bad json from stdin: {raw!r} ({e})")
            continue
        log(f"recv: {msg.get('type')}")
        try:
            handle_control(msg)
        except SystemExit:
            raise
        except Exception as e:
            log(f"handler error: {e}")

    # stdin closed without shutdown — exit cleanly
    emit_bye("stdin_closed", 0)


if __name__ == "__main__":
    main()
