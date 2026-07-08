"""GStreamer pipeline construction for one DeepStream stream.

This module imports ``gi`` / ``Gst`` / ``pyds`` at the TOP LEVEL — it
cannot be unit-tested on macOS. Smoke testing happens on Jetson.

Design (matches plan §4.5):

- **One Gst.Pipeline per stream.** A single batched nvstreammux pipeline
  would be more efficient for many streams, but it is much harder to
  add/remove streams live and harder to debug. For a first cut we keep
  streams isolated. Phase 2 can re-batch once the host has a way to
  issue batched AddStream and we have analytics-confidence.

- **Per-pipeline element topology:**

      uridecodebin uri=<rtsp_url>
        ! queue
        ! nvvideoconvert
        ! nvstreammux name=mux batch-size=1 width=1920 height=1080
        ! nvinfer config-file-path=<model.txt>
        ! nvtracker
        ! nvdsanalytics name=analytics
        ! nvvideoconvert
        ! nvdsosd                    # only when output.osd
        ! nvv4l2h264enc bitrate=<kbps>
        ! h264parse
        ! rtspclientsink location=<rtsp_out>

  When ``output.osd`` is False, the ``nvdsosd`` element is omitted but
  the analytics still emit metadata via the bus probe registered in
  :mod:`analytics`.

- **RTSP output strategy — DESIGN DECISION (see STATUS.md):**

  Two options were considered:

  (A) **GstRtspServer** in-process: cleaner ownership, no external dep.
      Downside: GstRtspServer is fiddly to wire up (factory + mount-points
      + appsrc interop) and debugging encoder-tee issues inside the same
      process is painful on first integration.
  (B) **rtspclientsink** to an external ``mediamtx`` instance: separates
      the RTSP server concern from the inference pipeline concern.
      Downside: requires an extra system service (mediamtx binary).

  We chose **(B)** for the first cut: the sidecar emits
  ``rtsp://<snapshot_bind_addr>:<rtsp_port>/ds/<stream_id>`` via
  ``rtspclientsink`` and the operator runs ``mediamtx`` separately.
  This is documented in ``sidecar/README.md``. The migration to (A)
  is a tracked follow-up.

- **Model engine file:** nvinfer compiles the .etlt to .engine on first
  run (10-60s blocking). The model config file written by
  :func:`write_model_config` points nvinfer at the .etlt; the engine
  cache path is the same directory + ``.engine`` suffix. We do NOT
  pre-compile engines here — that's an operator concern (one-time).

- **DEVIATION FROM PLAN:** Plan §4.5 mentions a "snapshot appsink
  branch" that tees off the encoder. We implement the branch but the
  actual JPEG ring buffer is owned by :class:`SnapshotStore` in
  :mod:`snapshot_server` (which the appsink callback feeds).

- All pyds / Gst calls are guarded with try/except — pyds has poor
  error messages and we want the traceback to point at the failing
  property set, not at deep C internals.
"""

from __future__ import annotations

import logging
import os
import tempfile
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

# These imports will fail on macOS — that's expected. The module is
# only importable inside the ds:7.1-pyds container.
try:
    import gi
    gi.require_version("Gst", "1.0")
    from gi.repository import Gst, GLib  # noqa: F401
    _GST_OK = True
except Exception as _exc:  # pragma: cover - macOS dev path OR missing GstRtspServer gir
    Gst = None  # type: ignore[assignment]
    GLib = None  # type: ignore[assignment]
    _GST_OK = False
    _IMPORT_ERROR = _exc

try:
    import pyds  # type: ignore[import-not-found]
    _PYDS_OK = True
except Exception as _exc:  # pragma: no cover - macOS dev path
    pyds = None  # type: ignore[assignment]
    _PYDS_OK = False

log = logging.getLogger("deepstream.pipeline_builder")

DEFAULT_RTSP_URL_PREFIX = "rtsp://127.0.0.1:8554/ds/"
DEFAULT_FRAME_WIDTH = 1920
DEFAULT_FRAME_HEIGHT = 1080
DEFAULT_BITRATE_KBPS = 2000


def require_gst() -> None:
    """Raise a clear error if Gst/pyds are unavailable (e.g. on dev laptop)."""
    if not _GST_OK:
        raise RuntimeError(
            "GStreamer / GI not available — pipeline_builder can only run "
            f"inside the ds:7.1-pyds container. Original error: {_IMPORT_ERROR!r}"
        )
    if not _PYDS_OK:
        raise RuntimeError("pyds not available")


def make_element(factory_name: str, name: Optional[str] = None, **properties: Any) -> Any:
    """Create a Gst element and set its properties.

    Raises ``RuntimeError`` if the factory doesn't exist or property-set
    fails. All pyds errors are caught and re-raised with the element name
    + offending property so the operator can see exactly what failed.
    """
    require_gst()
    elem = Gst.ElementFactory.make(factory_name, name)
    if elem is None:
        raise RuntimeError(f"cannot create element factory={factory_name!r} name={name!r}")
    for k, v in properties.items():
        try:
            elem.set_property(k, v)
        except Exception as e:
            raise RuntimeError(
                f"set_property failed on {factory_name}({name!r}): "
                f"{k}={v!r}: {e}"
            ) from e
    return elem


def link_chain(*elements: Any) -> None:
    """Link a chain of Gst elements; raise on the first failed link."""
    require_gst()
    for a, b in zip(elements, elements[1:]):
        if not a.link(b):
            raise RuntimeError(
                f"failed to link {a.get_name()!r} -> {b.get_name()!r}"
            )


# --- Model config file emitter --------------------------------------------


@dataclass
class ModelPaths:
    """Resolved filesystem paths for one model preset."""
    model_name: str
    config_txt_path: str        # nvinfer config .txt
    engine_path: Optional[str] = None  # pre-compiled .engine (None = let nvinfer build)
    labels_path: Optional[str] = None
    num_classes: int = 80


def resolve_model_preset(model: str, models_dir: str) -> ModelPaths:
    """Resolve a model preset name like ``Primary_Detector`` to filesystem paths.

    Supports two on-disk layouts:

    1. **User-laid-out (recommended):** everything under one dir
       ``<models_dir>/<model>/`` containing
       ``config_infer_primary_<model>.txt``, ``labels.txt``, optional
       ``<model>.engine``. This is what user-registered models look like.
    2. **NVIDIA samples (split layout):** engine + labels live under
       ``<models_dir>/<model>/`` (e.g. ``models/Primary_Detector/``) but
       the nvinfer ``.txt`` config lives separately under
       ``<ds_root>/samples/configs/deepstream-app/``. We probe a small
       set of well-known config filenames for this case
       (``config_infer_primary.txt``, ``config_infer_primary_<model>.txt``,
       ``config_infer_secondary_<model>.txt``).

    The .txt file is the source of truth for engine + labels paths (nvinfer
    reads ``model-engine-file`` / ``labelfile-path`` from it at init).
    """
    model_dir = os.path.join(models_dir, model)
    if not os.path.isdir(model_dir):
        raise FileNotFoundError(
            f"model preset {model!r} not found under {models_dir!r}"
        )

    # Candidate config .txt locations (probed in order).
    # models_dir = <ds_root>/samples/models  =>  ds_root = dirname(dirname(models_dir))
    ds_root = os.path.dirname(os.path.dirname(models_dir))
    sample_cfg_dir = os.path.join(ds_root, "samples", "configs", "deepstream-app")
    cfg_candidates = [
        os.path.join(model_dir, f"config_infer_primary_{model}.txt"),
        os.path.join(sample_cfg_dir, "config_infer_primary.txt"),
        os.path.join(sample_cfg_dir, f"config_infer_primary_{model}.txt"),
        os.path.join(sample_cfg_dir, f"config_infer_secondary_{model}.txt"),
        os.path.join(sample_cfg_dir, "config_infer_secondary.txt"),
    ]
    cfg = next((p for p in cfg_candidates if os.path.isfile(p)), None)
    if cfg is None:
        raise FileNotFoundError(
            f"missing nvinfer config for preset {model!r}; tried: "
            + ", ".join(cfg_candidates)
        )

    labels = os.path.join(model_dir, "labels.txt")
    if not os.path.isfile(labels):
        # NVIDIA samples put labels next to the .txt or in model dir as
        # labels_<name>.txt. We don't try to be exhaustive — nvinfer also
        # reads labelfile-path from the .txt config, so leave labels=None
        # and let nvinfer resolve it.
        labels = ""  # type: ignore[assignment]
    engine = None
    for candidate in (f"{model}.engine", "model.engine", "*.engine"):
        if "*" in candidate:
            import glob
            hits = glob.glob(os.path.join(model_dir, candidate))
            if hits:
                engine = hits[0]
                break
        else:
            p = os.path.join(model_dir, candidate)
            if os.path.isfile(p):
                engine = p
                break
    return ModelPaths(
        model_name=model,
        config_txt_path=cfg,
        engine_path=engine,
        labels_path=labels if os.path.isfile(labels) else None,
        num_classes=_read_num_classes(cfg),
    )


def _read_num_classes(config_txt: str) -> int:
    """Pull ``num-detected-classes=`` out of a nvinfer config file."""
    try:
        with open(config_txt, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line.lower().startswith("num-detected-classes"):
                    _, _, val = line.partition("=")
                    return int(val.strip())
    except OSError:
        pass
    return 80  # COCO default


def write_model_config(
    model_paths: ModelPaths,
    *,
    conf: Optional[float] = None,
    iou: Optional[float] = None,
    filter_classes: Optional[List[int]] = None,
    work_dir: Optional[str] = None,
) -> str:
    """Materialize a nvinfer config .txt for this stream.

    nvinfer reads a flat ``key=value`` .txt; per-stream overrides (conf,
    iou, filter_classes) are written on top of the preset's base file.
    Returns the path to the written config.
    """
    base_lines: List[str] = []
    try:
        with open(model_paths.config_txt_path, "r", encoding="utf-8") as f:
            base_lines = f.readlines()
    except OSError as e:
        raise RuntimeError(f"cannot read base config {model_paths.config_txt_path!r}: {e}")

    overrides: Dict[str, str] = {}
    if conf is not None:
        overrides["cluster-mode"] = "2"   # NMS
        overrides["nms-iou-threshold"] = f"{iou if iou is not None else 0.45:.4f}"
    if iou is not None and "nms-iou-threshold" not in overrides:
        overrides["nms-iou-threshold"] = f"{iou:.4f}"
    # nvinfer threshold comes from the model; per-stream conf is enforced
    # in the probe (filter at metadata level). We don't write conf here
    # because it would force an engine rebuild.

    # JETSON-DATAPLANE-CONSOLIDATED-PATCH (drop output-blob-names + path rewrite)
    # TRT 10.x no longer recognises the ONNX output tensor names that the
    # DS 7.1 sample configs hardcode (conv2d_bbox / conv2d_cov). Stripping
    # output-blob-names lets nvinfer discover outputs from the ONNX directly.
    # Path rewrite: nvinfer resolves relatives from CWD; the temp config lives
    # in /tmp so we rewrite model file keys to absolute container paths.
    _DS_ROOT = "/opt/nvidia/deepstream/deepstream"
    # DS sample configs live here and use paths like `../../models/...`
    # relative to this dir. nvinfer resolves relatives from CWD, not the
    # config file location, so we have to rewrite them to absolute paths.
    _DS_CFG_DIR = _DS_ROOT + "/samples/configs/deepstream-app"
    _path_keys = {
        "onnx-file", "onnx-file-path", "model-engine-file", "proto-file",
        "caffemodel", "uff-file", "labelfile", "labelfile-path",
        "int8-calib-file", "custom-lib-path", "model-cache-file", "tensorfile",
    }
    def _resolve(key: str, val: str) -> str:
        if os.path.isabs(val):
            return val  # already absolute; trust it
        # 1) Resolve relative to the original config file's directory.
        cand = os.path.normpath(os.path.join(_DS_CFG_DIR, val))
        if os.path.isfile(cand):
            return cand
        # 2) Resolve against the DS samples tree (covers bare `models/...` etc).
        for prefix in (_DS_ROOT + "/samples/models", _DS_ROOT + "/samples"):
            cand = os.path.normpath(os.path.join(prefix, val.lstrip("/")))
            if os.path.isfile(cand):
                return cand
        cand = os.path.normpath(os.path.join(_DS_ROOT, val.lstrip("/")))
        if os.path.isfile(cand):
            return cand
        return val  # give up; let nvinfer log the error

    out_dir = work_dir or tempfile.mkdtemp(prefix="ds_model_")
    out_path = os.path.join(out_dir, f"config_infer_{model_paths.model_name}.txt")
    with open(out_path, "w", encoding="utf-8") as f:
        for line in base_lines:
            stripped = line.strip().lower()
            if stripped.startswith("output-blob-names"):
                continue
            written = False
            for k, v in overrides.items():
                if stripped.startswith(k + "=") or stripped.startswith(k + " ="):
                    f.write(f"{k}={v}\n")
                    written = True
                    break
            if written:
                continue
            # Rewrite model file paths to absolute container paths.
            key, sep, val = line.partition("=")
            _lkey = key.strip().lower() if sep else ""

            # Pre-built FP16 engine injection (DS 7.1 default config is INT8).
            # Inline replacement keeps keys inside their original [property]
            # section so the nvinfer parser accepts them. Appending to EOF
            # landed them under [class-attrs-*] and parser rejected them.
            _PREBUILT_ENGINE = "/engines/trafficcam_fp16.engine"
            if _lkey == "model-engine-file":
                if os.path.isfile(_PREBUILT_ENGINE):
                    f.write(f"model-engine-file={_PREBUILT_ENGINE}\n")
                    log.info("pre-built engine: model-engine-file -> %s",
                             _PREBUILT_ENGINE)
                else:
                    log.warning("pre-built engine missing: %s; leaving "
                                "original %r", _PREBUILT_ENGINE, val)
                    f.write(line)
                continue
            if _lkey == "network-mode":
                if os.path.isfile(_PREBUILT_ENGINE):
                    f.write("network-mode=2\n")  # FP16 matches pre-built
                    continue
                # fall through: keep original
            if _lkey == "batch-size":
                if os.path.isfile(_PREBUILT_ENGINE):
                    # trtexec default builds max_batch=1 engine. Base config
                    # says batch-size=30 which makes nvinfer reject the engine
                    # as "failed to match config params" and rebuild. We run
                    # single-stream so batch-size=1 is correct.
                    f.write("batch-size=1\n")
                    continue
                # fall through: keep original
            if _lkey == "int8-calib-file":
                if os.path.isfile(_PREBUILT_ENGINE):
                    # FP16 engine doesn't need INT8 calib; drop the line.
                    continue
                # fall through: keep original

            if sep and _lkey in _path_keys:
                val = val.strip()
                resolved = _resolve(key, val)
                if resolved != val:
                    f.write(f"{key}={resolved}\n")
                    continue
            f.write(line)
        if filter_classes is not None:
            # nvinfer "detection-id" filtering is done at the probe level;
            # we just record it for documentation.
            f.write(f"# filter-classes={filter_classes}\n")
    return out_path



# --- Analytics config translation -----------------------------------------


def analytics_roi_config(rules: List[Any]) -> Dict[str, Any]:
    """Translate RoiRule list to nvdsanalytics ROI config dict.

    The NVDS_ANALYTICS ROI format is::

        [ROI-<stream-id>]
        enable=1
        ROI-<id>=<x1>;<y1>;<x2>;<y2>;<...>;<xN>;<yN>

    Returned dict is fed to ``nvdsanalytics.set_property("config", ...)``.
    """
    require_gst()
    config: Dict[str, Dict[str, str]] = {}
    for i, rule in enumerate(rules):
        section = f"ROI-{i}"
        coords = ";".join(f"{x};{y}" for x, y in rule.polygon)
        config[section] = {
            "enable": "1",
            f"ROI-{rule.id}": coords,
            # 'mode' is enforced in the probe; nvdsanalytics only does geometry.
        }
    return config


def analytics_line_config(rules: List[Any]) -> Dict[str, Any]:
    """Translate LineCrossingRule list to nvdsanalytics LineCrossing config.

    Format::

        [LINE-CROSSING-<i>]
        enable=1
        line-crossing-Exit-<line_id>=<x1>;<y1>;<x2>;<y2>
        class-id=0
        # extended=0 for balanced, =1 for bidirectional
        extended=<0|1>
    """
    require_gst()
    config: Dict[str, Dict[str, str]] = {}
    for i, rule in enumerate(rules):
        section = f"LINE-CROSSING-{i}"
        coords = ";".join(f"{x};{y}" for x, y in rule.points)
        extended = "1" if rule.mode == "bidirectional" else "0"
        config[section] = {
            "enable": "1",
            f"line-crossing-{rule.id}": coords,
            "class-id": ",".join(str(c) for c in rule.classes) if rule.classes else "0",
            "extended": extended,
            "mode": "balanced",
        }
    return config


# --- Per-stream pipeline --------------------------------------------------


@dataclass
class BuiltPipeline:
    """Wraps a constructed pipeline plus access handles for probes/snapshots."""
    pipeline: Any
    mux: Any
    analytics_elem: Any
    snapshot_sink: Optional[Any]    # appsink that feeds the snapshot store
    rtsp_url: str
    model_paths: ModelPaths
    nvinfer_config_path: str

    def get_bus(self) -> Any:
        return self.pipeline.get_bus()


def build_pipeline(
    stream_id: str,
    config: Any,                       # StreamConfig
    *,
    rtsp_url_prefix: str = DEFAULT_RTSP_URL_PREFIX,
    models_dir: str = "/opt/nvidia/deepstream/deepstream/samples/models",
    work_dir: Optional[str] = None,
    snapshot_enabled: bool = False,  # tee branch stalls; see STATUS.md §2
) -> BuiltPipeline:
    """Construct a single-stream Gst.Pipeline (not yet set to PLAYING).

    Caller is responsible for adding a bus watch and setting state.
    """
    require_gst()
    src_cfg = config.source
    out_cfg = config.output or _default_output()
    width = DEFAULT_FRAME_WIDTH
    height = DEFAULT_FRAME_HEIGHT

    # Resolve model + write per-stream config.
    model_paths = resolve_model_preset(config.model, models_dir)
    mc = config.model_config
    nvinfer_cfg = write_model_config(
        model_paths,
        conf=getattr(mc, "conf", None) if mc else None,
        iou=getattr(mc, "iou", None) if mc else None,
        filter_classes=getattr(mc, "filter_classes", None) if mc else None,
        work_dir=work_dir,
    )

    p = Gst.Pipeline.new(f"pipeline-{stream_id}")

    # --- Source -----------------------------------------------------------
    # JETSON-DATAPLANE-CONSOLIDATED-PATCH
    # For RTSP sources we bypass uridecodebin and build the chain manually.
    # Chain: rtspsrc -> rtph264depay -> nvv4l2decoder (NO h264parse).
    # Reason: h264parse fails to convert AVCC -> byte-stream (caps claim
    # byte-stream but buffers stay length-prefixed), breaking nvv4l2decoder.
    # mediamtx/RTSP SDP carries SPS/PPS via sprop-parameter-sets; rtph264depay
    # feeds them inline, nvv4l2decoder handles AVCC input directly.
    is_rtsp = src_cfg.source_type == "rtsp" or src_cfg.url.startswith("rtsp://")
    _rtsp_explicit = False
    _rtsp_decoder = None
    if is_rtsp:
        latency = src_cfg.latency_ms if src_cfg.latency_ms is not None else 200
        rtsp_src = make_element("rtspsrc", f"rtspsrc-{stream_id}",
                                location=src_cfg.url,
                                latency=latency,
                                protocols=4)  # GST_RTSP_LOWER_TRANS_TCP
        _rtsp_depay = make_element("rtph264depay", f"depay-{stream_id}")
        _rtsp_decoder = make_element("nvv4l2decoder", f"dec-{stream_id}")
        log.info("rtsp source: explicit chain rtspsrc+depay+nvv4l2decoder (no h264parse — AVCC->byte-stream conv was broken, depay feeds AVCC inline w/ sprop-parameter-sets)")

        # rtspsrc emits pad-added on SDP parse; link depay dynamically.
        def _link_rtsp_pad(rtspsrc_el, pad, _depay=_rtsp_depay):
            name = pad.get_name() or ""
            if "recv_rtp_src" not in name:
                return
            sinkpad = _depay.get_static_pad("sink")
            if sinkpad is None or sinkpad.is_linked():
                return
            if pad.link(sinkpad) != Gst.PadLinkReturn.OK:
                log.error("failed to link rtspsrc.%s -> rtph264depay", name)
            else:
                log.info("rtsp: linked rtspsrc.%s -> rtph264depay", name)

        rtsp_src.connect("pad-added", _link_rtsp_pad)
        src = rtsp_src
        _rtsp_explicit = True
    else:
        src = make_element("uridecodebin", f"src-{stream_id}", uri=src_cfg.url)
        if src_cfg.rtsp_transport:
            try:
                src.set_property("source-filter",
                                 f"rtspsrc protocols={src_cfg.rtsp_transport}")
            except Exception:
                log.warning("could not set rtsp_transport=%s", src_cfg.rtsp_transport)
        if src_cfg.latency_ms is not None:
            try:
                src.set_property("latency", src_cfg.latency_ms)
            except Exception:
                log.warning("could not set latency=%s", src_cfg.latency_ms)

    # nvstreammux is the standard DeepStream mux point.
    # live-source=1 for RTSP: decoder emits framerate=0/1 (mediamtx SPS lacks
    # VUI framerate); non-live mux mode rejects those caps. live-source=1 tells
    # mux to accept 0/1 framerate. (Previously thought live-source=1 dropped
    # frames with mediamtx — that was when its writeQueueSize was default 512;
    # now bumped to 8192.)
    _is_live_src = src_cfg.source_type == "rtsp" or src_cfg.url.startswith("rtsp://")
    mux = make_element(
        "nvstreammux", f"mux-{stream_id}",
        batch_size=1,
        width=width,
        height=height,
        live_source=1 if _is_live_src else 0,
    )

    # --- Infer + tracker + analytics -------------------------------------
    nvinfer = make_element(
        "nvinfer", f"infer-{stream_id}",
        config_file_path=nvinfer_cfg,
        unique_id=1,
    )
    # Engine cache override DISABLED — set_property(network-mode) fails
    # (not a GstNvInfer element property; only in the config file), and
    # model-engine-file to a non-existent path causes GST_STATE_CHANGE_FAILURE.
    # Use config file defaults (FP16 from cfg_infer_fp16.txt).

    tracker = make_element("nvtracker", f"tracker-{stream_id}")
    _apply_tracker_props(tracker, config.tracker)

    # nvdsanalytics requires a `config-file` property pointing to a real file
    # when enable=1. Without it, the pipeline fails state transition
    # (GST_STATE_CHANGE_FAILURE during ready->paused). When no analytics
    # rules are configured, write an empty config file so nvdsanalytics is
    # happy in pass-through mode.
    try:
        analytics_cfg_path = os.path.join(work_dir or tempfile.mkdtemp(prefix="ds_analytics_"),
                                          f"analytics_{stream_id}.txt")
        os.makedirs(os.path.dirname(analytics_cfg_path), exist_ok=True)
        # nvdsanalytics requires config-width + config-height even when
        # enable=0 — the parser fails state change without them.
        with open(analytics_cfg_path, "w") as f:
            f.write("[property]\n")
            f.write("enable=0\n")
            f.write("config-width=1920\n")
            f.write("config-height=1080\n")
            f.write("gpu-id=0\n")
        analytics_elem = make_element(
            "nvdsanalytics", f"analytics-{stream_id}",
            enable=0,
            config_file=analytics_cfg_path,
        )
        log.info("nvdsanalytics: config-file=%s (enable=0)", analytics_cfg_path)
    except Exception as e:
        log.warning("nvdsanalytics init failed, falling back to queue: %s", e)
        analytics_elem = make_element("queue", f"analytics-{stream_id}")
    _apply_analytics_props(analytics_elem, config.analytics, stream_id)

    # --- Convert + OSD + Encode ------------------------------------------
    converter = make_element("nvvideoconvert", f"conv-{stream_id}")
    osd = None
    if out_cfg.osd if out_cfg.osd is not None else False:
        osd = make_element("nvdsosd", f"osd-{stream_id}")

    encoder_kind = (out_cfg.encoder or "h264").lower()
    enc_factory = "nvv4l2h264enc" if encoder_kind == "h264" else "nvv4l2h265enc"
    bitrate = out_cfg.bitrate_kbps or DEFAULT_BITRATE_KBPS
    encoder = make_element(enc_factory, f"enc-{stream_id}", bitrate=bitrate)
    # insert-sps-pps=1: inject SPS/PPS before each IDR so mediamtx readers
    # can pick up the stream mid-publish without waiting for the next keyframe.
    # maxperf-enable=1: disable power-saving clock gating that caused the
    # encoder to stall after a few buffers under load.
    try:
        encoder.set_property("insert-sps-pps", 1)
        encoder.set_property("maxperf-enable", 1)
    except Exception as e:
        log.warning("could not set encoder insert-sps-pps/maxperf: %s", e)
    parser = make_element(
        "h264parse" if encoder_kind == "h264" else "h265parse",
        f"parse-{stream_id}",
        config_interval=1,  # re-inject SPS/PPS every second (belt+braces)
    )

    rtsp_url = f"{rtsp_url_prefix}{stream_id}"
    rtsp_sink = make_element(
        "rtspclientsink", f"rtsp-{stream_id}",
        location=rtsp_url,
        protocols="tcp",
        latency=200,
        do_rtsp_keep_alive=True,
    )

    # Snapshot branch (only when enabled; we need a tee to fan out buffers
    # to both the encoder and the nvjpegenc->appsink path).
    snapshot_sink = None
    snap_tee = None
    if snapshot_enabled:
        snap_tee, snapshot_sink = _make_snapshot_branch(
            p, stream_id, work_dir=work_dir
        )

    # --- Add + link ------------------------------------------------------
    if _rtsp_explicit:
        for e in (_rtsp_depay, _rtsp_decoder):
            p.add(e)
        try:
            _rtsp_depay.link(_rtsp_decoder)
        except Exception as e:
            log.error("rtsp explicit chain link failed: %s", e)
    for e in (src, mux, nvinfer, tracker, analytics_elem, converter):
        p.add(e)
    chain: List[Any] = [mux, nvinfer, tracker, analytics_elem, converter]
    if osd is not None:
        p.add(osd)
        chain.append(osd)
    # Insert snap_tee between converter (or osd) and encoder so the snapshot
    # branch can fan off it. Only added when snapshot_enabled.
    if snap_tee is not None:
        chain.append(snap_tee)
    chain.extend([encoder, parser, rtsp_sink])
    for e in (encoder, parser, rtsp_sink):
        p.add(e)
    # Link the FULL chain: mux -> nvinfer -> tracker -> analytics -> converter
    # -> [osd?] -> [snap_tee?] -> encoder -> parser -> rtsp_sink.
    # Insert a queue between every pair of elements to prevent back-pressure
    # stall (5 buffers then freeze was observed without queues).
    _queue_seq = [0]
    def _q():
        _queue_seq[0] += 1
        q = make_element("queue", f"q-{stream_id}-{_queue_seq[0]}")
        try:
            q.set_property("max-size-buffers", 0)
            q.set_property("max-size-time", 0)
            q.set_property("max-size-bytes", 0)
        except Exception:
            pass
        return q
    interleaved = []
    for i, el in enumerate(chain):
        interleaved.append(el)
        if i < len(chain) - 1:
            _q_e = _q()
            p.add(_q_e)
            interleaved.append(_q_e)
    log.info("pipeline: inserted %d queues between chain elements",
             len(interleaved) // 2)
    link_chain(*interleaved)

    # Link snapshot branch off the snap_tee.
    if snap_tee is not None:
        _wire_snapshot_branch(snap_tee, stream_id)

    # uridecodebin pads appear dynamically — connect on pad-added.
    if _rtsp_explicit:
        # RTSP path: nvv4l2decoder -> mux.sink_0 (DIRECT, no capsfilter)
        # Verified via gst-launch that capsfilter blocks buffer flow even
        # though caps negotiation succeeds. Direct link works because
        # nvstreammux sink_0 accepts the decoder's native NVMM NV12 caps.
        mux_sink = mux.get_request_pad("sink_0")
        if mux_sink is None:
            log.error("nvstreammux refused to allocate sink pad for rtsp decoder")
        elif _rtsp_decoder.get_static_pad("src").link(mux_sink) != Gst.PadLinkReturn.OK:
            log.error("failed to link nvv4l2decoder -> nvstreammux (rtsp explicit)")
        else:
            log.info("rtsp: linked nvv4l2decoder -> nvstreammux (direct, no capsfilter)")
    else:
        src.connect("pad-added", _on_src_pad_added, mux)

    log.info("pipeline built for stream %s -> %s", stream_id, rtsp_url)
    return BuiltPipeline(
        pipeline=p,
        mux=mux,
        analytics_elem=analytics_elem,
        snapshot_sink=snapshot_sink,
        rtsp_url=rtsp_url,
        model_paths=model_paths,
        nvinfer_config_path=nvinfer_cfg,
    )


def _on_src_pad_added(src: Any, pad: Any, mux: Any) -> None:
    """Dynamic pad-added handler: link uridecodebin src -> nvstreammux sink."""
    require_gst()
    sinkpad = mux.get_request_pad("sink_0")
    if sinkpad is None:
        log.error("nvstreammux refused to allocate sink pad")
        return
    if pad.link(sinkpad) != Gst.PadLinkReturn.OK:
        log.error("failed to link uridecodebin -> nvstreammux")


def _apply_tracker_props(tracker: Any, tracker_cfg: Optional[Any]) -> None:
    """Apply standard nvtracker properties. Best-effort — many are optional."""
    # Always set the ll-lib-file even when tracker is "disabled" — without it
    # gstnvtracker logs "Loading low-level lib at (null)" and refuses to init,
    # which blocks pad activation and stalls downstream buffer flow.
    default_ttype = "NvDCF"
    if tracker_cfg is None or not getattr(tracker_cfg, "enabled", True):
        try:
            tracker.set_property("ll-lib-file", _ll_lib_for(default_ttype))
            tracker.set_property("tracker-width", 640)
            tracker.set_property("tracker-height", 384)
            tracker.set_property("user-meta-pool-size", 0)
        except Exception as e:
            log.warning("failed to set default tracker props: %s", e)
        return
    ttype = (getattr(tracker_cfg, "tracker_type", None) or "NvDCF").lower()
    props = {
        "tracker-width": 640,
        "tracker-height": 384,
        "ll-lib-file": _ll_lib_for(ttype),
        "enable-past-frame": 0,
    }
    min_conf = getattr(tracker_cfg, "min_confidence", None)
    if min_conf is not None:
        props["tracking-surface-temp-warning"] = 0  # placeholder
    for k, v in props.items():
        try:
            tracker.set_property(k, v)
        except Exception as e:
            log.debug("nvtracker prop %s=%s skipped (%s)", k, v, e)


def _ll_lib_for(ttype: str) -> str:
    """Best-effort path to the tracker low-level lib on JetPack 6 / DS 7.1.

    DS 7.1 ships only the combined `libnvds_nvmultiobjecttracker.so`. The
    legacy per-tracker libs (libnvds_nvdcdcf_tracker.so etc.) are absent,
    so we prefer the combined lib regardless of ttype.
    """
    base = "/opt/nvidia/deepstream/deepstream/lib"
    combined = f"{base}/libnvds_nvmultiobjecttracker.so"
    if os.path.isfile(combined):
        return combined
    legacy = {
        "nvdcf": f"{base}/libnvds_nvdcdcf_tracker.so",
        "nvsort": f"{base}/libnvds_nvmultiobjecttracker.so",
        "deepsort": f"{base}/libnvds_deepsort_tracker.so",
    }
    return legacy.get(ttype, combined)


def _apply_analytics_props(
    analytics_elem: Any,
    analytics_cfg: Optional[Any],
    stream_id: str,
) -> None:
    if analytics_cfg is None:
        return
    config: Dict[str, Any] = {}
    try:
        if analytics_cfg.line_crossing:
            config.update(analytics_line_config(analytics_cfg.line_crossing))
        if analytics_cfg.roi:
            config.update(analytics_roi_config(analytics_cfg.roi))
    except Exception as e:
        log.warning("analytics config translation failed: %s", e)
        return
    if not config:
        return
    try:
        analytics_elem.set_property("config", config)
    except Exception as e:
        log.error("failed to apply nvdsanalytics config: %s", e)


def _make_snapshot_branch(
    pipeline: Any,
    stream_id: str,
    *,
    work_dir: Optional[str] = None,
) -> tuple:
    """Create the snapshot branch elements + tee.

    The returned tee must be inserted between converter (or osd) and the
    encoder in the main chain. After main link_chain, the caller invokes
    `_wire_snapshot_branch(tee, stream_id)` to attach the snapshot branch:

        tee.src_%u -> snap_queue -> snap_conv -> snap_jpegenc -> appsink

    Returns (tee, appsink).
    """
    require_gst()
    tee = make_element("tee", f"snap-tee-{stream_id}")
    # allow-not-linked: keep pushing to encoder branch even if snapshot
    # branch is slow / has no consumer yet. Without this, the tee blocks
    # the entire pipeline when appsink fills its tiny queue.
    try:
        tee.set_property("allow-not-linked", True)
    except Exception:
        pass
    queue = make_element("queue", f"snap-queue-{stream_id}")
    converter = make_element("nvvideoconvert", f"snap-conv-{stream_id}")
    jpegenc = make_element("nvjpegenc", f"snap-jpeg-{stream_id}")
    appsink = make_element(
        "appsink", f"snap-sink-{stream_id}",
        emit_signals=True,
        sync=False,
        max_buffers=2,
        drop=True,
    )
    try:
        queue.set_property("max-size-buffers", 0)
        queue.set_property("max-size-time", 0)
        queue.set_property("max-size-bytes", 0)
    except Exception:
        pass
    # Stash branch elements as Python attributes on the tee so
    # _wire_snapshot_branch can find them later.
    tee._snap_queue = queue
    tee._snap_conv = converter
    tee._snap_jpegenc = jpegenc
    tee._snap_appsink = appsink
    for e in (tee, queue, converter, jpegenc, appsink):
        pipeline.add(e)
    log.info("snapshot branch created for %s", stream_id)
    return tee, appsink


def _wire_snapshot_branch(tee: Any, stream_id: str) -> None:
    """Link the snapshot branch off the tee.

    gst_element_link(tee, queue) auto-requests a "src_%u" pad on the tee.
    Also sets `alloc-pad` on the tee to the encoder-side queue so the tee
    uses the encoder branch's buffer pool — required for NVMM interoperability.
    """
    require_gst()
    queue = getattr(tee, "_snap_queue", None)
    conv = getattr(tee, "_snap_conv", None)
    jpegenc = getattr(tee, "_snap_jpegenc", None)
    appsink = getattr(tee, "_snap_appsink", None)
    if not all([queue, conv, jpegenc, appsink]):
        log.warning("snapshot branch elements missing on tee for %s", stream_id)
        return
    try:
        if not tee.link(queue):
            log.error("tee -> snap-queue link failed for %s", stream_id)
            return
        if not queue.link(conv):
            log.error("snap-queue -> snap-conv link failed for %s", stream_id)
            return
        if not conv.link(jpegenc):
            log.error("snap-conv -> snap-jpegenc link failed for %s", stream_id)
            return
        if not jpegenc.link(appsink):
            log.error("snap-jpegenc -> snap-appsink link failed for %s", stream_id)
            return
    except Exception as e:
        log.error("snapshot branch link failed for %s: %s", stream_id, e)
        return

    # Find the encoder-side queue (the OTHER tee src pad) and set it as
    # alloc-pad. This tells the tee to use that branch's allocator.
    try:
        from gi.repository import Gst  # type: ignore[import-not-found]
        enc_pad = None
        for pad in tee.iterate_src_pads():
            # Skip the snapshot branch pad (linked to snap_queue).
            peer = pad.get_peer()
            if peer is not None and peer.get_parent() is not queue:
                enc_pad = pad
                break
        if enc_pad is not None:
            tee.set_property("alloc-pad", enc_pad)
            log.info("snapshot: tee alloc-pad set to encoder branch for %s", stream_id)
    except Exception as e:
        log.warning("could not set tee alloc-pad for %s: %s", stream_id, e)

    log.info("snapshot branch wired: tee -> queue -> conv -> jpegenc -> appsink (%s)",
             stream_id)


def _default_output() -> Any:
    """Lazy import to avoid circular at module load."""
    from config import OutputConfig
    return OutputConfig()
