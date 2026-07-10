"""Probe + parse nvdsanalytics output metadata.

Reads DeepStream frame metadata via :mod:`pyds` and emits typed events
(:class:`Detection`, :class:`LineCross`, :class:`ROIIntrusion`) which
the runner converts to wire frames.

Per-frame processing flow (matches NVIDIA deepstream_python_apps/
deepstream-analytics reference):

1. ``buffer = gst_buffer_get_nvds_batch_meta(hash(gst_buffer))``
2. ``batch_meta.iterate()``
3. For each frame_meta in the batch:
   - Iterate ``obj_meta_list`` for detector/tracker results
   - Pull ``frame_meta.frame_user_meta_list`` for nvdsanalytics results
     (LC status + ROI status)
4. Apply per-stream filters:
   - ``events.detection_hz`` throttles Detection events
   - ``events.always_emit`` lets the host opt in to specific event
     kinds even when the throttle would otherwise suppress them
   - ``model_config.filter_classes`` drops objects not in the allowlist
   - ``tracker.min_confidence`` drops low-conf tracks
5. Convert filtered metadata to typed events, hand to caller

All pyds calls are guarded with try/except — pyds error messages are
notoriously terse, and a probe exception would tear down the whole
pipeline. On exception we log + skip the frame (better to lose one
event than the entire stream).

**Hot-update (Task 8.4):** :func:`set_line_crossing` and
:func:`set_roi` re-write the nvdsanalytics ``config`` property while
the pipeline remains PLAYING. The new geometry takes effect within ~1s
(next buffer cycle).
"""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass, field
from typing import Any, Callable, List, Optional

# pyds only importable in container.
try:
    import pyds  # type: ignore[import-not-found]
    _PYDS_OK = True
except Exception as _exc:  # pragma: no cover - macOS dev path
    pyds = None  # type: ignore[assignment]
    _PYDS_OK = False
    _IMPORT_ERROR = _exc

from protocol import (
    Detection,
    DetectionObject,
    LineCross,
    ROIIntrusion,
)

log = logging.getLogger("deepstream.analytics")

# Callback signature: ``(event) -> None``. The event is one of Detection /
# LineCross / ROIIntrusion / AnalyticsSnapshot.
EventCallback = Callable[[Any], None]


def require_pyds() -> None:
    if not _PYDS_OK:
        raise RuntimeError(
            f"pyds not available — analytics.py can only run inside the "
            f"ds:7.1-pyds container. Original error: {_IMPORT_ERROR!r}"
        )


@dataclass
class StreamFilter:
    """Per-stream event filtering state."""
    stream_id: str
    detection_hz: Optional[float] = None
    always_emit: List[str] = field(default_factory=list)
    filter_classes: Optional[List[int]] = None
    min_confidence: Optional[float] = None
    last_detection_ts: float = 0.0

    def should_emit_detection(self, now: float) -> bool:
        if "detection" in self.always_emit:
            return True
        if self.detection_hz is None or self.detection_hz <= 0:
            return True
        period = 1.0 / self.detection_hz
        if (now - self.last_detection_ts) >= period:
            self.last_detection_ts = now
            return True
        return False

    def class_allowed(self, cls_id: int) -> bool:
        if self.filter_classes is None:
            return True
        return cls_id in self.filter_classes

    def confidence_ok(self, conf: float) -> bool:
        if self.min_confidence is None:
            return True
        return conf >= self.min_confidence


@dataclass
class ProbeHandle:
    """Returned from :func:`attach_probe` so callers can detach later."""
    pad: Any
    probe_id: int
    stream_id: str
    # Mutable counters read by the runner's periodic Stats task. Incremented
    # in the GLib probe thread; read from the asyncio stats task via GLib
    # bridge (single-threaded access, no lock needed).
    frame_count: int = 0
    object_count: int = 0


# --- Probe attachment -----------------------------------------------------


def attach_probe(
    analytics_elem: Any,
    stream_id: str,
    filt: StreamFilter,
    on_event: EventCallback,
) -> ProbeHandle:
    """Attach a buffer probe to ``analytics_elem``'s src pad.

    The probe walks the per-frame metadata, applies ``filt``, and emits
    events via ``on_event``. The probe runs in the GLib thread — the
    callback should NOT block.
    """
    require_pyds()
    src_pad = analytics_elem.get_static_pad("src")
    if src_pad is None:
        raise RuntimeError(f"nvdsanalytics for {stream_id} has no src pad")

    handle = ProbeHandle(pad=src_pad, probe_id=0, stream_id=stream_id)

    def _probe_cb(pad: Any, info: Any) -> Any:
        try:
            _process_buffer(info, stream_id, filt, on_event, handle)
        except Exception as e:
            log.exception("probe error on %s: %s", stream_id, e)
        return __import__("gi").repository.Gst.PadProbeReturn.OK

    probe_id = src_pad.add_probe(
        __import__("gi").repository.Gst.PadProbeType.BUFFER,
        _probe_cb,
    )
    handle.probe_id = probe_id
    return handle


def detach_probe(handle: ProbeHandle) -> None:
    try:
        handle.pad.remove_probe(handle.probe_id)
    except Exception as e:
        log.warning("could not detach probe on %s: %s", handle.stream_id, e)


# --- Per-buffer processing ------------------------------------------------


def _process_buffer(
    info: Any,
    stream_id: str,
    filt: StreamFilter,
    on_event: EventCallback,
    handle: "ProbeHandle",
) -> None:
    """Walk one GstBuffer's batch metadata and emit events."""
    require_pyds()
    gst_buffer = info.get_buffer()
    if gst_buffer is None:
        return
    batch_meta = pyds.gst_buffer_get_nvds_batch_meta(hash(gst_buffer))
    if batch_meta is None:
        return

    l_frame = batch_meta.frame_meta_list
    while l_frame is not None:
        try:
            frame_meta = pyds.NvDsFrameMeta.cast(l_frame.data)
        except Exception as e:
            log.debug("frame_meta cast failed: %s", e)
            break

        handle.frame_count += 1
        now = time.time()
        ts_ms = int(time.time() * 1000)
        frame_id = int(getattr(frame_meta, "frame_num", 0))
        objects = _collect_objects(frame_meta, filt)
        if objects:
            handle.object_count += len(objects)
            if filt.should_emit_detection(now):
                on_event(Detection(
                    stream_id=stream_id,
                    ts=ts_ms,
                    frame_id=frame_id,
                    objects=objects,
                ))

        _collect_analytics(frame_meta, stream_id, ts_ms, on_event)

        try:
            l_frame = l_frame.next
        except Exception:
            break


def _collect_objects(frame_meta: Any, filt: StreamFilter) -> List[DetectionObject]:
    """Walk obj_meta_list -> DetectionObject list (with filter applied)."""
    require_pyds()
    out: List[DetectionObject] = []
    l_obj = frame_meta.obj_meta_list
    while l_obj is not None:
        try:
            obj = pyds.NvDsObjectMeta.cast(l_obj.data)
        except Exception as e:
            log.debug("obj cast failed: %s", e)
            break
        try:
            cls_id = int(obj.class_id)
            conf = float(obj.confidence)
            track_id_raw = getattr(obj, "object_id", None)
            track_id = int(track_id_raw) if track_id_raw is not None else None
            rect = obj.rect_params
            # DeepStream rect_params: left/top/width/height (in frame pixels).
            left = float(rect.left)
            top = float(rect.top)
            right = left + float(rect.width)
            bottom = top + float(rect.height)
            if filt.class_allowed(cls_id) and filt.confidence_ok(conf):
                out.append(DetectionObject(
                    class_=cls_id,
                    conf=conf,
                    bbox=[left, top, right, bottom],
                    track_id=track_id,
                ))
        except Exception as e:
            log.debug("obj parse failed: %s", e)
        try:
            l_obj = l_obj.next
        except Exception:
            break
    return out


def _collect_analytics(
    frame_meta: Any,
    stream_id: str,
    ts_ms: int,
    on_event: EventCallback,
) -> None:
    """Walk frame_user_meta_list for nvdsanalytics LC/ROI results."""
    require_pyds()
    l_user = frame_meta.frame_user_meta_list
    while l_user is not None:
        try:
            user_meta = pyds.NvDsUserMeta.cast(l_user.data)
        except Exception as e:
            log.debug("user_meta cast failed: %s", e)
            break
        try:
            if int(user_meta.base_meta.meta_type) != int(
                pyds.NvDsMetaType.nvds_analytics_r_frame_meta_type
            ):
                try:
                    l_user = l_user.next
                except Exception:
                    break
                continue
            analytics_meta = pyds.NvDsAnalyticsFrameMeta.cast(
                user_meta.user_meta_data
            )
            _emit_line_cross(analytics_meta, stream_id, ts_ms, on_event)
            _emit_roi(analytics_meta, stream_id, ts_ms, on_event)
        except Exception as e:
            log.debug("analytics parse failed: %s", e)
        try:
            l_user = l_user.next
        except Exception:
            break


def _emit_line_cross(
    analytics_meta: Any,
    stream_id: str,
    ts_ms: int,
    on_event: EventCallback,
) -> None:
    """Walk lcStatus entries -> LineCross events."""
    require_pyds()
    try:
        lc_status = analytics_meta.lcStatus
    except AttributeError:
        return
    for entry in lc_status:
        try:
            # entry is a tuple (line_id_str, {track_id: direction_str})
            line_id, crossings = entry[0], entry[1]
            for track_id, direction in crossings.items():
                # class lookup requires the obj_meta — nvdsanalytics doesn't
                # carry it. Default to 0; host can correlate by track_id.
                on_event(LineCross(
                    stream_id=stream_id,
                    ts=ts_ms,
                    line_id=str(line_id),
                    track_id=int(track_id),
                    class_=0,
                    direction=str(direction),
                ))
        except Exception as e:
            log.debug("lc entry parse failed: %s", e)


def _emit_roi(
    analytics_meta: Any,
    stream_id: str,
    ts_ms: int,
    on_event: EventCallback,
) -> None:
    """Walk roiStatus entries -> ROIIntrusion events."""
    require_pyds()
    try:
        roi_status = analytics_meta.roiStatus
    except AttributeError:
        return
    for entry in roi_status:
        try:
            roi_id, track_classes = entry[0], entry[1]
            # track_classes: dict {track_id: [class_id, ...]} or {track_id: count}
            for track_id, val in track_classes.items():
                if isinstance(val, list):
                    for cls_id in val:
                        on_event(ROIIntrusion(
                            stream_id=stream_id,
                            ts=ts_ms,
                            roi_id=str(roi_id),
                            track_id=int(track_id),
                            class_=int(cls_id),
                            mode="inside",
                        ))
                else:
                    on_event(ROIIntrusion(
                        stream_id=stream_id,
                        ts=ts_ms,
                        roi_id=str(roi_id),
                        track_id=int(track_id),
                        class_=0,
                        mode=str(val),
                    ))
        except Exception as e:
            log.debug("roi entry parse failed: %s", e)


# --- Hot-update (Task 8.4) -----------------------------------------------


def set_line_crossing(
    analytics_elem: Any,
    rules: List[Any],
) -> None:
    """Hot-update nvdsanalytics with new line-crossing rules.

    Calls into :func:`pipeline_builder.analytics_line_config` to translate
    the rule list, then sets the ``config`` property. Safe to call while
    pipeline is PLAYING.
    """
    require_pyds()
    from pipeline_builder import analytics_line_config
    config = analytics_line_config(rules)
    try:
        # nvdsanalytics expects config as a dict-of-dicts on the Python side.
        analytics_elem.set_property("config", config)
    except Exception as e:
        log.error("hot-update line_crossing failed: %s", e)
        raise


def set_roi(analytics_elem: Any, rules: List[Any]) -> None:
    """Hot-update nvdsanalytics with new ROI polygons."""
    require_pyds()
    from pipeline_builder import analytics_roi_config
    config = analytics_roi_config(rules)
    try:
        analytics_elem.set_property("config", config)
    except Exception as e:
        log.error("hot-update roi failed: %s", e)
        raise
