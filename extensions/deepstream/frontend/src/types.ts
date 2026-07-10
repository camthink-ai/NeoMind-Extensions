// TypeScript mirrors of Rust structs in extensions/deepstream/src/{stream_manager,protocol,lib}.rs.
// Field names and JSON shapes were cross-checked against the Rust source —
// snake_case wire format is preserved. Where the task spec diverged from the
// actual Rust struct (and it diverged a lot), the Rust source wins.

// ---------------------------------------------------------------------------
// Stream status
// ---------------------------------------------------------------------------

// Rust: `StreamStatus` enum with `#[serde(rename_all = "snake_case")]`.
// The host emits lowercase wire strings ("connecting", "running", ...).
export type StreamStatus =
  | 'connecting'
  | 'running'
  | 'degraded'
  | 'reconnecting'
  | 'error'
  | 'stopped';

// ---------------------------------------------------------------------------
// Stream state (returned by list_streams / get_stream_info)
// ---------------------------------------------------------------------------

// Rust: `StreamState` struct. `list_streams` returns a JSON projection that
// omits `last_transition_at`, `snapshot_token`, and `config` (only `model` is
// surfaced). `get_stream_info` returns the fuller projection including
// `last_transition_at` and `source`. We model the union here so callers can
// rely on the shared fields; the optional ones are gated with `?`.
export interface Stream {
  stream_id: string;
  status: StreamStatus;
  rtsp_url?: string | null;
  model: string;                  // config.model (preset name)
  added_at: number;               // epoch millis
  // Present in get_stream_info response but not list_streams:
  source?: StreamSource;
  last_transition_at?: number;    // epoch millis
  snapshot_token?: string | null;
  config?: StreamConfig;
}

// ---------------------------------------------------------------------------
// Config structs (stream_manager.rs)
// ---------------------------------------------------------------------------

export interface StreamConfig {
  stream_id: string;
  source: StreamSource;
  model: string;                       // preset name OR path
  model_config?: ModelConfig;
  tracker?: TrackerConfig;
  analytics?: AnalyticsConfig;
  output?: OutputConfig;
  events?: EventsConfig;
}

// Rust: `StreamSource` is a STRUCT (not an enum). `source_type` is renamed to
// `type` on the wire. Only the `url` field is required; `type` defaults to
// "rtsp" in practice but is serialized as whatever the caller sent. The other
// knobs (rtsp_transport, latency_ms, retry_count) are all optional.
export interface StreamSource {
  type: string;                 // 'rtsp' in practice today
  url: string;
  rtsp_transport?: string;      // e.g. 'tcp'
  latency_ms?: number;
  retry_count?: number;
}

// Rust: `ModelConfig` is the per-model TUNING struct (thresholds, device,
// class filter). The preset name / engine path lives on
// `StreamConfig.model` (string) — NOT here. Mirroring the Rust fields exactly.
export interface ModelConfig {
  conf?: number;                // f32
  iou?: number;                 // f32
  infer_device?: string;
  filter_classes?: number[];    // u32[]
}

// Rust: `TrackerConfig`. `tracker_type` is renamed to `type` on the wire.
export interface TrackerConfig {
  enabled: boolean;
  type?: string;                // e.g. 'NvDCF' | 'NvSORT' (free-form string in Rust)
  min_confidence?: number;
}

export interface AnalyticsConfig {
  line_crossing?: LineCrossingRule[];
  roi?: RoiRule[];
  counting?: CountingConfig;
}

// Rust: `LineCrossingRule`. `points` is `Vec<(i32, i32)>` — pixel-coordinate
// tuples. `mode` and `classes` are required (no Option / no skip_serializing_if
// means they always emit, even when empty). Rendered as 2-tuples in JSON.
export interface LineCrossingRule {
  id: string;
  points: [number, number][];   // [[x, y], ...] pixel coords
  mode: string;                 // 'balanced' | 'bidirectional' | ... (free-form)
  classes: number[];            // u32[]
}

// Rust: `RoiRule`. `polygon: Vec<(i32, i32)>`.
export interface RoiRule {
  id: string;
  polygon: [number, number][];  // pixel coords
  mode: string;                 // 'entry' | 'exit' | 'inside' (free-form in Rust)
  classes: number[];
}

// Rust: `CountingConfig`. NOT the line-shape struct the task spec described —
// it just toggles counting and references a previously-declared line_id.
export interface CountingConfig {
  enabled: boolean;
  line_id: string;
}

// Rust: `OutputConfig`. All fields optional.
export interface OutputConfig {
  rtsp_mount?: string;
  osd?: boolean;
  encoder?: string;             // 'h264' | 'h265' (free-form)
  bitrate_kbps?: number;
  fps?: number;
}

// Rust: `EventsConfig`.
export interface EventsConfig {
  detection_hz?: number;        // f32
  always_emit?: string[];       // event names to always forward
}

// ---------------------------------------------------------------------------
// Stats (protocol.rs)
// ---------------------------------------------------------------------------

// Rust: `Stats`. All fields are non-optional in the struct, but the host's
// metric bridge only reads some of them, so we keep them required to match
// the wire shape exactly.
export interface Stats {
  ts: number;                   // i64, epoch millis
  global_fps: number;
  gpu_utilization_percent: number;
  gpu_memory_used_mb: number;
  per_stream: StreamStat[];
}

export interface StreamStat {
  stream_id: string;
  fps: number;
  latency_ms: number;
  frame_count: number;
  object_count: number;
  status: string;
}

// ---------------------------------------------------------------------------
// SidecarEvent (protocol.rs)
// ---------------------------------------------------------------------------

// Rust: `SidecarEvent` is `#[serde(tag = "type", rename_all = "snake_case")]`.
// So every variant serializes to `{ "type": "<snake_case_name>", ...fields }`.
// Variant name → wire tag:
//   Ready            -> "ready"
//   HelloAck         -> "hello_ack"
//   StreamAdded      -> "stream_added"
//   StreamRemoved    -> "stream_removed"
//   StreamError      -> "stream_error"
//   Detection        -> "detection"
//   LineCross        -> "line_cross"
//   ROIIntrusion     -> "roi_intrusion"
//   AnalyticsSnapshot-> "analytics_snapshot"
//   Stats(Stats)     -> "stats"     (newtype — inner fields are flattened)
//   Pong             -> "pong"
//   ErrorResponse    -> "error_response"
//   Bye              -> "bye"

export type SidecarEventType =
  | 'ready'
  | 'hello_ack'
  | 'stream_added'
  | 'stream_removed'
  | 'stream_error'
  | 'detection'
  | 'line_cross'
  | 'roi_intrusion'
  | 'analytics_snapshot'
  | 'stats'
  | 'pong'
  | 'error_response'
  | 'bye';

// Discriminated union. Use `ev.type` to narrow.
export type SidecarEvent =
  | ({ type: 'ready' } & ReadyPayload)
  | ({ type: 'hello_ack' } & HelloAckPayload)
  | ({ type: 'stream_added' } & StreamAddedPayload)
  | ({ type: 'stream_removed' } & StreamRemovedPayload)
  | ({ type: 'stream_error' } & StreamErrorPayload)
  | ({ type: 'detection' } & DetectionPayload)
  | ({ type: 'line_cross' } & LineCrossPayload)
  | ({ type: 'roi_intrusion' } & ROIIntrusionPayload)
  | ({ type: 'analytics_snapshot' } & AnalyticsSnapshotPayload)
  | ({ type: 'stats' } & Stats)
  | ({ type: 'pong' } & PongPayload)
  | ({ type: 'error_response' } & ErrorResponsePayload)
  | ({ type: 'bye' } & ByePayload);

interface GpuInfo {
  name: string;
  mem_mb: number;
}

interface ReadyPayload {
  ds_ver: string;
  pyds_ver: string;
  protocol_ver: number;
  gpu_info: GpuInfo;
}

interface HelloAckPayload {
  max_streams: number;
  rtsp_url_prefix: string;
  models_loaded: string[];
}

interface StreamAddedPayload {
  id: string;                   // request id (correlates with AddStream.id)
  stream_id: string;
  rtsp_url: string;
}

interface StreamRemovedPayload {
  id: string;                   // request id
  stream_id: string;
}

interface StreamErrorPayload {
  stream_id: string;
  code: string;
  message: string;
  id?: string;                  // optional request id
}

// Rust: `DetectionObject`. bbox is [f32; 4] = [left, top, right, bottom].
interface DetectionObject {
  class: number;                // u32
  conf: number;                 // f32
  track_id?: number;            // u32, omitted when skip_serializing_if Option::is_none
  bbox: [number, number, number, number];
}

interface DetectionPayload {
  stream_id: string;
  ts: number;                   // i64 epoch millis
  frame_id: number;             // u64
  objects: DetectionObject[];
}

interface LineCrossPayload {
  stream_id: string;
  ts: number;
  line_id: string;
  track_id: number;
  class: number;
  direction: string;            // free-form
}

interface ROIIntrusionPayload {
  stream_id: string;
  ts: number;
  roi_id: string;
  track_id: number;
  class: number;
  mode: string;                 // 'entry' | 'exit' (free-form)
}

// Rust: AnalyticsSnapshot.snapshot is serde_json::Value — opaque to the host.
// Callers that know the sidecar's contract can cast to their expected shape.
interface AnalyticsSnapshotPayload {
  stream_id: string;
  ts: number;
  snapshot: Record<string, unknown> | unknown[];
}

interface PongPayload {
  ts: number;
}

interface ErrorResponsePayload {
  id: string;                   // request id (required in Rust, not Optional)
  code: string;
  message: string;
}

interface ByePayload {
  reason: string;
  exit_code: number;
}

// ---------------------------------------------------------------------------
// Models (returned by list_models — see lib.rs cmd_list_models)
// ---------------------------------------------------------------------------

// Wire shape is { id, name, preset: bool, engine_path?, labels_path?,
// input_shape?, precision? }. `input_shape` is a 3-tuple (c, h, w).
export interface ModelInfo {
  id: string;
  name: string;
  preset: boolean;              // true = built-in, false = user-registered
  engine_path?: string;         // present when preset === false
  labels_path?: string;
  input_shape?: [number, number, number];
  precision?: string;           // 'fp16' | 'int8' | 'fp32'
}

// ---------------------------------------------------------------------------
// Diagnostics (returned by diagnose)
// ---------------------------------------------------------------------------

// Matches the manual Serialize impl for SystemStatus in lib.rs.
export interface SystemStatus {
  deepstream_installed: boolean;
  deepstream_version: string | null;
  pyds_available: boolean;
  pyds_version: string | null;
  gst_plugins_ok: boolean;
  gst_missing: string[];
  python_bin: string | null;
  last_check_at: number;        // epoch millis
  install_hint: string | null;
}
