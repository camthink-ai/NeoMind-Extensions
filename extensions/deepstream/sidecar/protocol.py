"""JSON Lines wire protocol for the DeepStream sidecar.

Mirror of the Rust enum ``protocol.rs``. Tag-keyed discriminated unions
(``{"type": "<snake_case_tag>", ...fields}``) — matches the Rust
``#[serde(tag = "type", rename_all = "snake_case")]`` exactly.

Wire format (single JSON object per line, terminated by ``\n``):

- Control messages (host -> sidecar, read from stdin):

    * ``hello``            { rtsp_port, snapshot_port, log_level,
                             models_dir, max_streams, snapshot_bind_addr }
    * ``add_stream``       { id, config: <StreamConfig dict> }
    * ``remove_stream``    { id, stream_id, graceful_secs }
    * ``update_analytics`` { id, stream_id, line_crossing, roi }
    * ``set_threshold``    { id, stream_id, conf, iou }
    * ``list_state``       { id }
    * ``health_check``     { ts }
    * ``shutdown``         { graceful_secs }

- Sidecar events (sidecar -> host, written to stdout):

    * ``ready``             { ds_ver, pyds_ver, protocol_ver, gpu_info }
    * ``hello_ack``         { max_streams, rtsp_url_prefix, models_loaded }
    * ``stream_added``      { id, stream_id, rtsp_url }
    * ``stream_removed``    { id, stream_id }
    * ``stream_error``      { stream_id, code, message, id? }
    * ``detection``         { stream_id, ts, frame_id, objects: [...] }
    * ``line_cross``        { stream_id, ts, line_id, track_id, class, direction }
    * ``roi_intrusion``     { stream_id, ts, roi_id, track_id, class, mode }
    * ``analytics_snapshot``{ stream_id, ts, snapshot }
    * ``stats``             { ts, global_fps, gpu_utilization_percent,
                              gpu_memory_used_mb, per_stream: [...] }
    * ``pong``              { ts }
    * ``error_response``    { id, code, message }
    * ``bye``               { reason, exit_code }

This module uses ONLY the Python stdlib so it can be imported on macOS
for unit tests of the wire format. Anything that touches ``pyds`` /
``Gst`` / ``GLib`` lives in other modules.

``__main__`` smoke test (Task 8.2 Step 2): reads stdin line-by-line,
parses each line as a ControlMessage, and echoes any ``health_check``
back as a ``pong`` event on stdout. Empty stdin -> no output, exit 0.
"""

from __future__ import annotations

import dataclasses
import json
import sys
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Type, Union

# --- Constants (mirror protocol.rs) ----------------------------------------

PROTOCOL_VERSION: int = 1
MAX_LINE_BYTES: int = 4 * 1024 * 1024  # 4 MiB, matches Rust.


class ProtocolError(Exception):
    """Raised when a line cannot be serialized / deserialized."""


class LineTooLongError(ProtocolError):
    """A line exceeded MAX_LINE_BYTES."""


# --- Helpers ---------------------------------------------------------------


def _snake(s: str) -> str:
    """CamelCase -> snake_case (only for error messages / debug)."""
    out = []
    for i, ch in enumerate(s):
        if ch.isupper() and i > 0:
            out.append("_")
        out.append(ch.lower())
    return "".join(out)


def serialize(obj: Any) -> bytes:
    """Serialize ``obj`` to a single JSON line terminated by ``\\n``.

    ``obj`` must be a dataclass instance whose ``to_dict()`` (or whose
    ``dataclasses.asdict`` output) yields the canonical wire shape,
    including a ``"type"`` discriminator. Raises :class:`LineTooLongError`
    if the resulting line exceeds :data:`MAX_LINE_BYTES`.
    """
    payload = to_wire_dict(obj)
    line = json.dumps(payload, separators=(",", ":"))
    if len(line) > MAX_LINE_BYTES:
        raise LineTooLongError(
            f"line exceeds {MAX_LINE_BYTES} bytes ({len(line)})"
        )
    return (line + "\n").encode("utf-8")


def to_wire_dict(obj: Any) -> Dict[str, Any]:
    """Convert a dataclass instance to its wire-format dict.

    - If the class has an explicit ``to_wire_dict`` method, defer to it
      (covers nested structs that don't carry a ``type`` tag, and event
      variants that need special handling like Stats).
    - Otherwise the dataclass is a "plain" tagged event: emit
      ``"type": TYPE_TAG`` plus all dataclass fields, omitting fields in
      ``OPTIONAL_OMIT_NONE`` whose value is None.
    """
    if hasattr(obj, "to_wire_dict") and callable(obj.to_wire_dict):
        return obj.to_wire_dict()
    if dataclasses.is_dataclass(obj):
        d: Dict[str, Any] = {}
        tag = getattr(obj, "TYPE_TAG", None)
        if tag is not None:
            d["type"] = tag
        omit_none = getattr(obj, "OPTIONAL_OMIT_NONE", frozenset())
        for f in dataclasses.fields(obj):
            if f.name in {"TYPE_TAG", "OPTIONAL_OMIT_NONE"}:
                continue
            val = getattr(obj, f.name)
            if val is None and f.name in omit_none:
                continue
            d[f.name] = _to_primitive(val)
        return d
    raise TypeError(f"cannot serialize {type(obj).__name__}")


def _to_primitive(val: Any) -> Any:
    """Recursively convert dataclass / tuple / list / dict to JSON-safe."""
    if hasattr(val, "to_wire_dict") and callable(val.to_wire_dict):
        return val.to_wire_dict()
    if dataclasses.is_dataclass(val) and not isinstance(val, type):
        return to_wire_dict(val)
    if isinstance(val, (list, tuple)):
        return [_to_primitive(v) for v in val]
    if isinstance(val, dict):
        return {k: _to_primitive(v) for k, v in val.items()}
    return val


def deserialize_line(line: str) -> Dict[str, Any]:
    """Parse one JSON line into a raw dict (caller dispatches on ``type``).

    The dict is NOT validated against the protocol — it is the raw
    payload as returned by :func:`json.loads`. The typed accessors
    (:class:`Hello`, :func:`parse_control_message`, etc.) provide the
    type-checked view.
    """
    if len(line) > MAX_LINE_BYTES:
        raise LineTooLongError(
            f"line exceeds {MAX_LINE_BYTES} bytes ({len(line)})"
        )
    try:
        return json.loads(line)
    except json.JSONDecodeError as e:
        raise ProtocolError(f"invalid JSON: {e}") from e


# --- GPU info / detection objects / stream stats ---------------------------


@dataclass
class GpuInfo:
    """Nested struct — NOT a tagged event variant, no ``type`` field."""
    name: str
    mem_mb: int

    def to_wire_dict(self) -> Dict[str, Any]:
        return {"name": self.name, "mem_mb": self.mem_mb}


@dataclass
class DetectionObject:
    """Nested struct — NOT a tagged event variant.

    ``bbox`` is ``[left, top, right, bottom]``.
    """
    class_: int
    conf: float
    bbox: List[float]
    track_id: Optional[int] = None

    def to_wire_dict(self) -> Dict[str, Any]:
        # ``class`` is a reserved word in Python — we expose ``class_`` but
        # the wire key is ``class`` (matches Rust field ``class: u32``).
        d: Dict[str, Any] = {
            "class": self.class_,
            "conf": self.conf,
            "bbox": list(self.bbox),
        }
        if self.track_id is not None:
            d["track_id"] = self.track_id
        return d


@dataclass
class StreamStat:
    """Nested struct — NOT a tagged event variant."""
    stream_id: str
    fps: float
    latency_ms: float
    frame_count: int
    object_count: int
    status: str

    def to_wire_dict(self) -> Dict[str, Any]:
        return {
            "stream_id": self.stream_id,
            "fps": self.fps,
            "latency_ms": self.latency_ms,
            "frame_count": self.frame_count,
            "object_count": self.object_count,
            "status": self.status,
        }


# --- Stats payload ---------------------------------------------------------
#
# The Rust enum has ``SidecarEvent::Stats(Stats)`` — a tuple variant.
# Tuple variants do NOT add a wrapping field; their inner fields are
# inlined directly into the event object alongside the tag. We mirror
# that here by making ``Stats`` carry the tag itself.

@dataclass
class Stats:
    ts: int
    global_fps: float
    gpu_utilization_percent: float
    gpu_memory_used_mb: float
    per_stream: List[StreamStat] = field(default_factory=list)

    TYPE_TAG: str = field(init=False, default="stats", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)

    def to_wire_dict(self) -> Dict[str, Any]:
        return {
            "type": "stats",
            "ts": self.ts,
            "global_fps": self.global_fps,
            "gpu_utilization_percent": self.gpu_utilization_percent,
            "gpu_memory_used_mb": self.gpu_memory_used_mb,
            "per_stream": [to_wire_dict(s) for s in self.per_stream],
        }


# --- Control messages (host -> sidecar) ------------------------------------


@dataclass
class Hello:
    rtsp_port: int
    snapshot_port: int
    log_level: str
    models_dir: str
    max_streams: int
    snapshot_bind_addr: str

    TYPE_TAG: str = field(init=False, default="hello", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class AddStream:
    """NOTE: there is NO top-level ``stream_id`` field — it lives inside ``config``."""
    id: str
    config: Dict[str, Any]  # raw StreamConfig JSON — parsed by config.py.

    TYPE_TAG: str = field(init=False, default="add_stream", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class RemoveStream:
    id: str
    stream_id: str
    graceful_secs: int

    TYPE_TAG: str = field(init=False, default="remove_stream", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class UpdateAnalytics:
    id: str
    stream_id: str
    line_crossing: Dict[str, Any]
    roi: Dict[str, Any]

    TYPE_TAG: str = field(init=False, default="update_analytics", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class SetThreshold:
    id: str
    stream_id: str
    conf: float
    iou: float

    TYPE_TAG: str = field(init=False, default="set_threshold", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class ListState:
    id: str

    TYPE_TAG: str = field(init=False, default="list_state", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class HealthCheck:
    ts: int

    TYPE_TAG: str = field(init=False, default="health_check", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class Shutdown:
    graceful_secs: int

    TYPE_TAG: str = field(init=False, default="shutdown", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


_CONTROL_REGISTRY: Dict[str, Type] = {
    "hello": Hello,
    "add_stream": AddStream,
    "remove_stream": RemoveStream,
    "update_analytics": UpdateAnalytics,
    "set_threshold": SetThreshold,
    "list_state": ListState,
    "health_check": HealthCheck,
    "shutdown": Shutdown,
}

ControlMessage = Union[
    Hello, AddStream, RemoveStream, UpdateAnalytics,
    SetThreshold, ListState, HealthCheck, Shutdown,
]


def parse_control_message(d: Dict[str, Any]) -> ControlMessage:
    """Type-check a raw dict into a :data:`ControlMessage` dataclass.

    Raises :class:`ProtocolError` if ``type`` is missing or unknown.
    Extra keys are ignored (forward-compat with new protocol fields).
    """
    tag = d.get("type")
    if not isinstance(tag, str):
        raise ProtocolError(f"missing/invalid 'type' tag: {tag!r}")
    cls = _CONTROL_REGISTRY.get(tag)
    if cls is None:
        raise ProtocolError(f"unknown control message type: {tag!r}")
    return _build_dataclass(cls, d)  # type: ignore[return-value]


def _build_dataclass(cls: Type, d: Dict[str, Any]) -> Any:
    """Construct ``cls`` from dict ``d`` using only known fields."""
    field_names = {f.name for f in dataclasses.fields(cls)
                   if f.name not in {"TYPE_TAG", "OPTIONAL_OMIT_NONE"}}
    kwargs: Dict[str, Any] = {}
    for fn in field_names:
        if fn in d:
            kwargs[fn] = d[fn]
    try:
        return cls(**kwargs)
    except TypeError as e:
        raise ProtocolError(f"cannot build {cls.__name__}: {e}") from e


# --- Sidecar events (sidecar -> host) --------------------------------------


@dataclass
class Ready:
    ds_ver: str
    pyds_ver: str
    protocol_ver: int
    gpu_info: GpuInfo

    TYPE_TAG: str = field(init=False, default="ready", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)

    def to_wire_dict(self) -> Dict[str, Any]:
        return {
            "type": "ready",
            "ds_ver": self.ds_ver,
            "pyds_ver": self.pyds_ver,
            "protocol_ver": self.protocol_ver,
            "gpu_info": to_wire_dict(self.gpu_info),
        }


@dataclass
class HelloAck:
    max_streams: int
    rtsp_url_prefix: str
    models_loaded: List[str] = field(default_factory=list)

    TYPE_TAG: str = field(init=False, default="hello_ack", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)

    def to_wire_dict(self) -> Dict[str, Any]:
        return {
            "type": "hello_ack",
            "max_streams": self.max_streams,
            "rtsp_url_prefix": self.rtsp_url_prefix,
            "models_loaded": list(self.models_loaded),
        }


@dataclass
class StreamAdded:
    id: str
    stream_id: str
    rtsp_url: str
    snapshot_token: str = ""

    TYPE_TAG: str = field(init=False, default="stream_added", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class StreamRemoved:
    id: str
    stream_id: str

    TYPE_TAG: str = field(init=False, default="stream_removed", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class StreamError:
    stream_id: str
    code: str
    message: str
    id: Optional[str] = None

    TYPE_TAG: str = field(init=False, default="stream_error", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(
        init=False, default=frozenset({"id"}), repr=False
    )

    def to_wire_dict(self) -> Dict[str, Any]:
        d: Dict[str, Any] = {
            "type": "stream_error",
            "stream_id": self.stream_id,
            "code": self.code,
            "message": self.message,
        }
        if self.id is not None:
            d["id"] = self.id
        return d


@dataclass
class Detection:
    stream_id: str
    ts: int
    frame_id: int
    objects: List[DetectionObject] = field(default_factory=list)

    TYPE_TAG: str = field(init=False, default="detection", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)

    def to_wire_dict(self) -> Dict[str, Any]:
        return {
            "type": "detection",
            "stream_id": self.stream_id,
            "ts": self.ts,
            "frame_id": self.frame_id,
            "objects": [o.to_wire_dict() for o in self.objects],
        }


@dataclass
class LineCross:
    stream_id: str
    ts: int
    line_id: str
    track_id: int
    class_: int
    direction: str

    TYPE_TAG: str = field(init=False, default="line_cross", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)

    def to_wire_dict(self) -> Dict[str, Any]:
        return {
            "type": "line_cross",
            "stream_id": self.stream_id,
            "ts": self.ts,
            "line_id": self.line_id,
            "track_id": self.track_id,
            "class": self.class_,
            "direction": self.direction,
        }


@dataclass
class ROIIntrusion:
    stream_id: str
    ts: int
    roi_id: str
    track_id: int
    class_: int
    mode: str

    TYPE_TAG: str = field(init=False, default="roi_intrusion", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)

    def to_wire_dict(self) -> Dict[str, Any]:
        return {
            "type": "roi_intrusion",
            "stream_id": self.stream_id,
            "ts": self.ts,
            "roi_id": self.roi_id,
            "track_id": self.track_id,
            "class": self.class_,
            "mode": self.mode,
        }


@dataclass
class AnalyticsSnapshot:
    stream_id: str
    ts: int
    snapshot: Dict[str, Any]

    TYPE_TAG: str = field(init=False, default="analytics_snapshot", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class Pong:
    ts: int

    TYPE_TAG: str = field(init=False, default="pong", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class ErrorResponse:
    id: str
    code: str
    message: str

    TYPE_TAG: str = field(init=False, default="error_response", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


@dataclass
class Bye:
    reason: str
    exit_code: int

    TYPE_TAG: str = field(init=False, default="bye", repr=False)
    OPTIONAL_OMIT_NONE: frozenset = field(init=False, default=frozenset(), repr=False)


SidecarEvent = Union[
    Ready, HelloAck, StreamAdded, StreamRemoved, StreamError,
    Detection, LineCross, ROIIntrusion, AnalyticsSnapshot,
    Stats, Pong, ErrorResponse, Bye,
]


# --- Asyncio line reader ---------------------------------------------------
#
# Optional helper for ``asyncio``-based runners. Kept here so that the
# protocol module is the one-stop shop for wire I/O. Pure-stdlib.

async def read_message_async(reader: "Any") -> Dict[str, Any]:
    """Read one JSON line from an asyncio ``StreamReader``.

    Raises :class:`LineTooLongError` if the next line (excluding newline)
    exceeds :data:`MAX_LINE_BYTES`, matching the Rust behavior.
    """

    buf = bytearray()
    while True:
        chunk = await reader.read(4096)
        if not chunk:
            if not buf:
                raise EOFError("stream closed before any data")
            break
        nl = chunk.find(b"\n")
        if nl >= 0:
            buf.extend(chunk[:nl])
            # NOTE: any bytes after the newline are silently dropped.
            # Callers should use a buffered reader (e.g. asyncio's
            # StreamReader) that handles framing itself.
            break
        buf.extend(chunk)
        if len(buf) > MAX_LINE_BYTES:
            raise LineTooLongError(f"line exceeds {MAX_LINE_BYTES} bytes")
    if len(buf) > MAX_LINE_BYTES:
        raise LineTooLongError(f"line exceeds {MAX_LINE_BYTES} bytes")
    line = buf.decode("utf-8")
    return deserialize_line(line)


# --- Smoke test (Task 8.2 Step 2) ------------------------------------------
#
# Reads ControlMessage lines from stdin, echoes any ``health_check``
# back as a ``pong`` event on stdout. Empty stdin -> no output, exit 0.

def _main() -> int:

    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line:
            continue
        try:
            msg = parse_control_message(deserialize_line(line))
        except ProtocolError as e:
            err = ErrorResponse(id="?", code="parse_error", message=str(e))
            sys.stdout.buffer.write(serialize(err))
            sys.stdout.buffer.flush()
            continue
        if isinstance(msg, HealthCheck):
            pong = Pong(ts=msg.ts)
            sys.stdout.buffer.write(serialize(pong))
            sys.stdout.buffer.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
