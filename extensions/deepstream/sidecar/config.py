"""Typed accessors for ``StreamConfig`` and friends.

Pure stdlib mirror of ``extensions/deepstream/src/stream_manager.rs``.
The host sends ``AddStream { id, config: <StreamConfig JSON> }`` where
``config`` is a raw JSON object — :func:`parse_stream_config` turns it
into a typed tree with sensible defaults for missing optionals.

Key design notes:

- Optional nested objects (``tracker``, ``analytics``, ``output``,
  ``events``, ``model_config``) default to ``None`` — same as Rust's
  ``Option<T>`` with ``#[serde(default)]``.
- ``StreamSource.source_type`` mirrors Rust's ``#[serde(rename="type")]``
  field. In Python we expose ``source_type`` because ``type`` is a
  reserved word; the parser handles the rename.
- ``TrackerConfig.tracker_type`` mirrors ``#[serde(rename="type")]``.
- Tuple fields (``points``, ``polygon``) come in as ``list[tuple[int,int]]``
  — JSON arrays of 2-element arrays.
- Unknown keys are ignored (forward-compat for new config fields).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

Point = Tuple[int, int]


def _get(d: Dict[str, Any], key: str, default: Any = None) -> Any:
    """dict.get with explicit default for readability."""
    return d.get(key, default)


def _opt_int(d: Dict[str, Any], k: str) -> Optional[int]:
    v = d.get(k)
    return int(v) if v is not None else None


def _opt_float(d: Dict[str, Any], k: str) -> Optional[float]:
    v = d.get(k)
    return float(v) if v is not None else None


def _opt_str(d: Dict[str, Any], k: str) -> Optional[str]:
    v = d.get(k)
    return str(v) if v is not None else None


def _opt_bool(d: Dict[str, Any], k: str) -> Optional[bool]:
    v = d.get(k)
    return bool(v) if v is not None else None


def _opt_list(d: Dict[str, Any], k: str) -> Optional[List[Any]]:
    v = d.get(k)
    if v is None:
        return None
    if not isinstance(v, list):
        raise ValueError(f"expected list for {k!r}, got {type(v).__name__}")
    return list(v)


def _opt_int_list(d: Dict[str, Any], k: str) -> Optional[List[int]]:
    v = _opt_list(d, k)
    if v is None:
        return None
    return [int(x) for x in v]


def _opt_points(d: Dict[str, Any], k: str) -> Optional[List[Point]]:
    """Parse a list of ``[x, y]`` pairs into ``list[tuple[int, int]]``."""
    v = _opt_list(d, k)
    if v is None:
        return None
    out: List[Point] = []
    for pair in v:
        if not isinstance(pair, (list, tuple)) or len(pair) != 2:
            raise ValueError(f"expected [x, y] pair, got {pair!r}")
        out.append((int(pair[0]), int(pair[1])))
    return out


# --- Sub-config dataclasses ------------------------------------------------


@dataclass
class StreamSource:
    """Mirrors ``stream_manager.rs::StreamSource``."""
    source_type: str           # Rust: #[serde(rename="type")]
    url: str
    rtsp_transport: Optional[str] = None  # "tcp" | "udp"
    latency_ms: Optional[int] = None
    retry_count: Optional[int] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "StreamSource":
        return cls(
            source_type=str(d["type"]) if "type" in d else str(d.get("source_type", "rtsp")),
            url=str(d["url"]),
            rtsp_transport=_opt_str(d, "rtsp_transport"),
            latency_ms=_opt_int(d, "latency_ms"),
            retry_count=_opt_int(d, "retry_count"),
        )


@dataclass
class ModelConfig:
    """Mirrors ``stream_manager.rs::ModelConfig``."""
    conf: Optional[float] = None
    iou: Optional[float] = None
    infer_device: Optional[str] = None
    filter_classes: Optional[List[int]] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "ModelConfig":
        return cls(
            conf=_opt_float(d, "conf"),
            iou=_opt_float(d, "iou"),
            infer_device=_opt_str(d, "infer_device"),
            filter_classes=_opt_int_list(d, "filter_classes"),
        )


@dataclass
class TrackerConfig:
    """Mirrors ``stream_manager.rs::TrackerConfig``."""
    enabled: bool = True
    tracker_type: Optional[str] = None   # Rust: #[serde(rename="type")]
    min_confidence: Optional[float] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "TrackerConfig":
        enabled = bool(d.get("enabled", True))
        # Accept either "type" or "tracker_type".
        ttype = d.get("type", d.get("tracker_type"))
        return cls(
            enabled=enabled,
            tracker_type=str(ttype) if ttype is not None else None,
            min_confidence=_opt_float(d, "min_confidence"),
        )


@dataclass
class LineCrossingRule:
    """Mirrors ``stream_manager.rs::LineCrossingRule``."""
    id: str
    points: List[Point] = field(default_factory=list)
    mode: str = "balanced"          # "balanced" | "bidirectional"
    classes: List[int] = field(default_factory=list)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "LineCrossingRule":
        return cls(
            id=str(d["id"]),
            points=_opt_points(d, "points") or [],
            mode=str(d.get("mode", "balanced")),
            classes=_opt_int_list(d, "classes") or [],
        )


@dataclass
class RoiRule:
    """Mirrors ``stream_manager.rs::RoiRule``."""
    id: str
    polygon: List[Point] = field(default_factory=list)
    mode: str = "entry"             # "entry" | "exit" | "inside"
    classes: List[int] = field(default_factory=list)

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "RoiRule":
        return cls(
            id=str(d["id"]),
            polygon=_opt_points(d, "polygon") or [],
            mode=str(d.get("mode", "entry")),
            classes=_opt_int_list(d, "classes") or [],
        )


@dataclass
class CountingConfig:
    enabled: bool = False
    line_id: str = ""

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "CountingConfig":
        return cls(
            enabled=bool(d.get("enabled", False)),
            line_id=str(d.get("line_id", "")),
        )


@dataclass
class AnalyticsConfig:
    """Mirrors ``stream_manager.rs::AnalyticsConfig``."""
    line_crossing: Optional[List[LineCrossingRule]] = None
    roi: Optional[List[RoiRule]] = None
    counting: Optional[CountingConfig] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "AnalyticsConfig":
        lc_raw = _opt_list(d, "line_crossing")
        roi_raw = _opt_list(d, "roi")
        count_raw = d.get("counting")
        return cls(
            line_crossing=[LineCrossingRule.from_dict(x) for x in lc_raw] if lc_raw else None,
            roi=[RoiRule.from_dict(x) for x in roi_raw] if roi_raw else None,
            counting=CountingConfig.from_dict(count_raw) if isinstance(count_raw, dict) else None,
        )


@dataclass
class OutputConfig:
    """Mirrors ``stream_manager.rs::OutputConfig``."""
    rtsp_mount: Optional[str] = None
    osd: Optional[bool] = None
    encoder: Optional[str] = None        # "h264" | "h265"
    bitrate_kbps: Optional[int] = None
    fps: Optional[int] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "OutputConfig":
        return cls(
            rtsp_mount=_opt_str(d, "rtsp_mount"),
            osd=_opt_bool(d, "osd"),
            encoder=_opt_str(d, "encoder"),
            bitrate_kbps=_opt_int(d, "bitrate_kbps"),
            fps=_opt_int(d, "fps"),
        )


@dataclass
class EventsConfig:
    """Mirrors ``stream_manager.rs::EventsConfig``."""
    detection_hz: Optional[float] = None
    always_emit: Optional[List[str]] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "EventsConfig":
        ae = _opt_list(d, "always_emit")
        return cls(
            detection_hz=_opt_float(d, "detection_hz"),
            always_emit=[str(x) for x in ae] if ae else None,
        )


@dataclass
class StreamConfig:
    """Top-level per-stream configuration. Mirrors ``StreamConfig`` in Rust.

    Tolerant parser: missing optional fields default to ``None``.
    Raises ``KeyError`` if ``stream_id``, ``source``, or ``model`` are
    missing (these are required on the Rust side too).
    """
    stream_id: str
    source: StreamSource
    model: str
    model_config: Optional[ModelConfig] = None
    tracker: Optional[TrackerConfig] = None
    analytics: Optional[AnalyticsConfig] = None
    output: Optional[OutputConfig] = None
    events: Optional[EventsConfig] = None

    @classmethod
    def from_dict(cls, d: Dict[str, Any]) -> "StreamConfig":
        if "stream_id" not in d:
            raise KeyError("stream_id is required")
        if "source" not in d:
            raise KeyError("source is required")
        if "model" not in d:
            raise KeyError("model is required")
        source = StreamSource.from_dict(d["source"])
        model_cfg = d.get("model_config")
        tracker = d.get("tracker")
        analytics = d.get("analytics")
        output = d.get("output")
        events = d.get("events")
        return cls(
            stream_id=str(d["stream_id"]),
            source=source,
            model=str(d["model"]),
            model_config=ModelConfig.from_dict(model_cfg) if isinstance(model_cfg, dict) else None,
            tracker=TrackerConfig.from_dict(tracker) if isinstance(tracker, dict) else None,
            analytics=AnalyticsConfig.from_dict(analytics) if isinstance(analytics, dict) else None,
            output=OutputConfig.from_dict(output) if isinstance(output, dict) else None,
            events=EventsConfig.from_dict(events) if isinstance(events, dict) else None,
        )


def parse_stream_config(d: Dict[str, Any]) -> StreamConfig:
    """Public entry point used by ``deepstream_runner``."""
    return StreamConfig.from_dict(d)
