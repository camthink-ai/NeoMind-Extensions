//! DeepStream extension — see docs/superpowers/specs/2026-07-06-deepstream-extension-design.md

pub mod protocol;
pub mod sidecar;
pub mod stream_manager;
pub mod system_status;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;

use neomind_extension_sdk::{
    CommandBuilder, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    MetricDataType, ParamBuilder, Result,
};

use crate::protocol::{ControlMessage, SidecarEvent};
use crate::sidecar::SidecarHandle;
use crate::stream_manager::{StreamConfig, StreamManager, StreamManagerError};
use crate::system_status::SystemStatus;

/// Max concurrent streams the extension will accept (spec §3.1.1).
const DEFAULT_MAX_STREAMS: u32 = 32;

/// User-registered model entry. Preset models live in the sidecar config, not here.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelRegistration {
    pub id: String,
    pub name: String,
    pub engine_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_shape: Option<(u32, u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<String>,
}

pub struct DeepStreamExtension {
    /// Authoritative stream state. Arc so the supervisor's on_restart callback
    /// can share it when wiring replay (future task).
    streams: Arc<StreamManager>,
    /// User-registered models (preset models live in the sidecar config).
    registered_models: RwLock<HashMap<String, ModelRegistration>>,
    /// Cached system status from the last `diagnose` run.
    system_status: RwLock<Option<SystemStatus>>,
    /// Current sidecar handle. None until start_sidecar() has run.
    /// Arc+RwLock so execute_command can grab a snapshot and the supervisor
    /// can swap it on restart. Cloning the Arc out of the lock and dropping
    /// the guard before `.await` is the canonical access pattern.
    sidecar: Arc<RwLock<Option<Arc<SidecarHandle>>>>,
    /// Supervisor restart count at last observation (for `restart_sidecar`).
    /// Wired in a future task — kept here so the struct shape is stable.
    #[allow(dead_code)]
    restart_count: Arc<AtomicU64>,
    /// Python interpreter path resolved by pre-flight (e.g., "python3.10").
    /// Used when spawning the sidecar (wired in a future task).
    #[allow(dead_code)]
    python_bin: RwLock<Option<String>>,
}

impl Default for DeepStreamExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepStreamExtension {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(StreamManager::new(DEFAULT_MAX_STREAMS)),
            registered_models: RwLock::new(HashMap::new()),
            system_status: RwLock::new(None),
            sidecar: Arc::new(RwLock::new(None)),
            restart_count: Arc::new(AtomicU64::new(0)),
            python_bin: RwLock::new(None),
        }
    }

    /// Test seam: build an extension with a pre-wired sidecar handle and
    /// StreamManager. Skips the real `init()` path entirely.
    ///
    /// The returned extension shares the provided `streams` Arc (so a test can
    /// inspect state directly) and stores the provided `sidecar` Arc for use
    /// by `execute_command`. The supervisor's restart/replay wiring is NOT
    /// exercised through this constructor.
    ///
    /// Marked `#[doc(hidden)]` + `#[allow(dead_code)]` so integration tests in
    /// `tests/` can reach it through the crate's public surface without it
    /// cluttering the rustdoc or triggering dead-code warnings in production
    /// builds (where no caller uses it).
    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn for_test(
        streams: Arc<StreamManager>,
        sidecar: Arc<SidecarHandle>,
    ) -> Self {
        Self {
            streams,
            registered_models: RwLock::new(HashMap::new()),
            system_status: RwLock::new(None),
            sidecar: Arc::new(RwLock::new(Some(sidecar))),
            restart_count: Arc::new(AtomicU64::new(0)),
            python_bin: RwLock::new(None),
        }
    }

    /// Clone the sidecar Arc out of the RwLock, dropping the guard before any
    /// await. Holding a parking_lot guard across `.await` is a foot-gun
    /// (parking_lot is sync; the guard would pin the lock for the duration of
    /// the task's suspension).
    ///
    /// Returns `Err(NotSupported)` when no sidecar is wired yet.
    fn sidecar_handle(&self) -> Result<Arc<SidecarHandle>> {
        let guard = self.sidecar.read();
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| ExtensionError::NotSupported(
                "sidecar not started — call diagnose to check pre-flight".into(),
            ))
    }

    /// Wait up to `timeout` for an event matching `predicate`. Non-matching
    /// events are dropped (drained). Used by the add/remove/update commands
    /// to correlate a response to a sent control message.
    ///
    /// TODO(Phase 5): replace with a proper response multiplexer keyed by
    /// request `id`. Today this is a single-consumer drain; concurrent
    /// `wait_event` calls would race on `handle.recv()` and steal each
    /// other's events. Each command path uses it serially, which is safe.
    async fn wait_event<F>(
        handle: &SidecarHandle,
        timeout: Duration,
        predicate: F,
    ) -> Result<SidecarEvent>
    where
        F: Fn(&SidecarEvent) -> bool,
    {
        match tokio::time::timeout(timeout, async {
            loop {
                match handle.recv().await {
                    Some(ev) if predicate(&ev) => return ev,
                    Some(_) => continue,
                    None => {
                        return SidecarEvent::Bye {
                            reason: "stdout closed".into(),
                            exit_code: -1,
                        }
                    }
                }
            }
        })
        .await
        {
            Ok(ev) => Ok(ev),
            Err(_) => Err(ExtensionError::Timeout(format!(
                "no matching event in {:?}",
                timeout
            ))),
        }
    }

    // --- Command handlers --------------------------------------------------

    async fn cmd_add_stream(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        // Accept either {stream_id, config:{...}} (wrapper form) or a bare
        // StreamConfig object with stream_id at top level. The wrapper form
        // matches the ExtensionCommand param shape (stream_id + config string);
        // the bare form matches what the replay protocol already sends.
        let (stream_id, config_value) = if let Some(sid) = args.get("stream_id").and_then(|v| v.as_str()) {
            let cfg = args.get("config").cloned().unwrap_or_else(|| args.clone());
            (sid.to_string(), cfg)
        } else {
            // Bare StreamConfig form — parse it to extract stream_id.
            let cfg: StreamConfig = serde_json::from_value(args.clone())
                .map_err(|e| ExtensionError::InvalidArguments(format!("config: {e}")))?;
            let sid = cfg.stream_id.clone();
            (sid, serde_json::to_value(&cfg).unwrap_or_else(|_| args.clone()))
        };

        // Parse (or re-parse) into a full StreamConfig so we can store it.
        let mut config: StreamConfig = serde_json::from_value(config_value.clone())
            .map_err(|e| ExtensionError::InvalidArguments(format!("config: {e}")))?;
        // The wrapper form may not have stream_id inside config — patch it in
        // so the StreamManager add() doesn't see an empty/duplicate id.
        if config.stream_id.is_empty() || config.stream_id != stream_id {
            config.stream_id = stream_id.clone();
        }

        // Register in the StreamManager first (fast-fail on dup / max_streams).
        self.streams
            .add(config.clone())
            .map_err(|e| match e {
                StreamManagerError::AlreadyExists(id) => {
                    ExtensionError::AlreadyRegistered(id)
                }
                StreamManagerError::MaxStreamsReached(n) => ExtensionError::NotSupported(format!(
                    "max_streams ({n}) reached"
                )),
                other => ExtensionError::ExecutionFailed(other.to_string()),
            })?;

        // Send AddStream and wait for StreamAdded correlated by request id.
        let req_id = uuid::Uuid::new_v4().to_string();
        let handle = self.sidecar_handle()?;
        handle
            .send(&ControlMessage::AddStream {
                id: req_id.clone(),
                config: serde_json::to_value(&config)
                    .unwrap_or_else(|_| serde_json::Value::Null),
            })
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(format!("send add_stream: {e}")))?;

        let matched = Self::wait_event(
            &handle,
            Duration::from_secs(10),
            |ev| match ev {
                SidecarEvent::StreamAdded { id, .. } => id == &req_id,
                SidecarEvent::ErrorResponse { id, .. } => id == &req_id,
                _ => false,
            },
        )
        .await?;

        match matched {
            SidecarEvent::StreamAdded {
                rtsp_url, ..
            } => {
                let _ = self.streams.set_rtsp_url(&stream_id, rtsp_url.clone());
                let _ = self.streams.transition(
                    &stream_id,
                    crate::stream_manager::StreamStatus::Running,
                );
                Ok(serde_json::json!({
                    "stream_id": stream_id,
                    "rtsp_url": rtsp_url,
                }))
            }
            SidecarEvent::ErrorResponse { code, message, .. } => {
                let _ = self.streams.remove(&stream_id);
                Err(ExtensionError::ExecutionFailed(format!(
                    "sidecar rejected add_stream: {message} ({code})"
                )))
            }
            // wait_event synthesizes a Bye on stdout close
            other => {
                let _ = self.streams.remove(&stream_id);
                Err(ExtensionError::ExecutionFailed(format!(
                    "unexpected event: {other:?}"
                )))
            }
        }
    }

    async fn cmd_remove_stream(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let stream_id = args
            .get("stream_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("missing 'stream_id' parameter".into())
            })?
            .to_string();

        // Remove from manager first; error if not present.
        self.streams
            .remove(&stream_id)
            .map_err(|e| match e {
                StreamManagerError::NotFound(id) => ExtensionError::NotFound(id),
                other => ExtensionError::ExecutionFailed(other.to_string()),
            })?;

        let req_id = uuid::Uuid::new_v4().to_string();
        let handle = self.sidecar_handle()?;
        handle
            .send(&ControlMessage::RemoveStream {
                id: req_id.clone(),
                stream_id: stream_id.clone(),
                graceful_secs: 2,
            })
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(format!("send remove_stream: {e}")))?;

        // Wait for StreamRemoved correlated by request id. Best-effort: if the
        // sidecar doesn't ack within 5s we still report success because the
        // local StreamManager state is already updated — the stream is gone
        // from the user's perspective.
        let _ = Self::wait_event(
            &handle,
            Duration::from_secs(5),
            |ev| match ev {
                SidecarEvent::StreamRemoved { id, .. } => id == &req_id,
                SidecarEvent::ErrorResponse { id, .. } => id == &req_id,
                _ => false,
            },
        )
        .await;

        Ok(serde_json::json!({ "removed": stream_id }))
    }

    async fn cmd_list_streams(&self, _args: &serde_json::Value) -> Result<serde_json::Value> {
        let snapshot = self.streams.list();
        let arr: Vec<serde_json::Value> = snapshot
            .iter()
            .map(|s| {
                serde_json::json!({
                    "stream_id": s.config.stream_id,
                    "status": s.status.as_str(),
                    "rtsp_url": s.rtsp_url,
                    "model": s.config.model,
                    "added_at": s.added_at,
                })
            })
            .collect();
        Ok(serde_json::json!({ "streams": arr }))
    }

    async fn cmd_get_stream_info(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let stream_id = args
            .get("stream_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("missing 'stream_id' parameter".into())
            })?;

        let state = self.streams.get(stream_id).ok_or_else(|| {
            ExtensionError::NotFound(stream_id.to_string())
        })?;

        Ok(serde_json::json!({
            "stream_id": state.config.stream_id,
            "status": state.status.as_str(),
            "rtsp_url": state.rtsp_url,
            "model": state.config.model,
            "source": state.config.source,
            "added_at": state.added_at,
            "last_transition_at": state.last_transition_at,
        }))
    }

    async fn cmd_update_analytics(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let stream_id = args
            .get("stream_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("missing 'stream_id' parameter".into())
            })?
            .to_string();
        if self.streams.get(&stream_id).is_none() {
            return Err(ExtensionError::NotFound(stream_id));
        }
        let config = args.get("config").cloned().unwrap_or(serde_json::Value::Null);
        let (line_crossing, roi) = parse_analytics_config(&config);

        let req_id = uuid::Uuid::new_v4().to_string();
        let handle = self.sidecar_handle()?;
        handle
            .send(&ControlMessage::UpdateAnalytics {
                id: req_id.clone(),
                stream_id: stream_id.clone(),
                line_crossing,
                roi,
            })
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(format!("send update_analytics: {e}")))?;

        // Mock acks via re-emitting stream_added with matching id.
        let _ = Self::wait_event(
            &handle,
            Duration::from_secs(5),
            |ev| match ev {
                SidecarEvent::StreamAdded { id, .. } => id == &req_id,
                SidecarEvent::ErrorResponse { id, .. } => id == &req_id,
                _ => false,
            },
        )
        .await?;

        Ok(serde_json::json!({ "applied": stream_id }))
    }

    async fn cmd_set_threshold(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let stream_id = args
            .get("stream_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("missing 'stream_id' parameter".into())
            })?
            .to_string();
        if self.streams.get(&stream_id).is_none() {
            return Err(ExtensionError::NotFound(stream_id));
        }
        let conf = args.get("conf").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
        let iou = args.get("iou").and_then(|v| v.as_f64()).unwrap_or(0.45) as f32;

        let req_id = uuid::Uuid::new_v4().to_string();
        let handle = self.sidecar_handle()?;
        handle
            .send(&ControlMessage::SetThreshold {
                id: req_id.clone(),
                stream_id: stream_id.clone(),
                conf,
                iou,
            })
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(format!("send set_threshold: {e}")))?;

        let _ = Self::wait_event(
            &handle,
            Duration::from_secs(5),
            |ev| match ev {
                SidecarEvent::StreamAdded { id, .. } => id == &req_id,
                SidecarEvent::ErrorResponse { id, .. } => id == &req_id,
                _ => false,
            },
        )
        .await?;

        Ok(serde_json::json!({ "applied": stream_id }))
    }

    async fn cmd_list_models(&self, _args: &serde_json::Value) -> Result<serde_json::Value> {
        let mut out: Vec<serde_json::Value> = Vec::new();
        // Preset (hardcoded) — these live in the sidecar config but the host
        // also surfaces them so the UI can show options before startup.
        out.push(serde_json::json!({
            "id": "yolov8n-coco",
            "name": "YOLOv8n COCO",
            "preset": true,
        }));
        // User-registered.
        let registered = self.registered_models.read();
        for m in registered.values() {
            out.push(serde_json::json!({
                "id": m.id,
                "name": m.name,
                "engine_path": m.engine_path,
                "labels_path": m.labels_path,
                "input_shape": m.input_shape,
                "precision": m.precision,
                "preset": false,
            }));
        }
        Ok(serde_json::json!({ "models": out }))
    }

    async fn cmd_register_model(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let model: ModelRegistration = serde_json::from_value(args.clone())
            .map_err(|e| ExtensionError::InvalidArguments(format!("model: {e}")))?;

        {
            let mut registered = self.registered_models.write();
            if registered.contains_key(&model.id) {
                return Err(ExtensionError::AlreadyRegistered(model.id));
            }
            registered.insert(model.id.clone(), model);
        }
        Ok(serde_json::json!({ "registered": args.get("id").cloned().unwrap_or_default() }))
    }

    async fn cmd_restart_sidecar(
        &self,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Stub — full supervisor wiring + replay is its own task.
        Err(ExtensionError::NotSupported(
            "restart_sidecar not wired to supervisor yet — manual restart required".into(),
        ))
    }

    async fn cmd_diagnose(&self, _args: &serde_json::Value) -> Result<serde_json::Value> {
        let status = SystemStatus::run_checks().await;
        let json = serde_json::to_value(&status)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("serialize status: {e}")))?;
        *self.system_status.write() = Some(status);
        Ok(json)
    }
}

/// Extract (line_crossing, roi) JSON values from an analytics config blob.
/// The mock sidecar ignores them; the real sidecar wants them as raw JSON.
fn parse_analytics_config(config: &serde_json::Value) -> (serde_json::Value, serde_json::Value) {
    let line_crossing = config
        .get("line_crossing")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let roi = config
        .get("roi")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    (line_crossing, roi)
}

// Serialize helper for SystemStatus — derive isn't viable because SystemStatus
// doesn't derive Serialize (only Debug + PartialEq). Add a manual impl.
mod status_serialize {
    use super::SystemStatus;
    use serde::{Serialize, Serializer};

    impl Serialize for SystemStatus {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeStruct;
            let mut st = s.serialize_struct("SystemStatus", 9)?;
            st.serialize_field("deepstream_installed", &self.deepstream_installed)?;
            st.serialize_field("deepstream_version", &self.deepstream_version)?;
            st.serialize_field("pyds_available", &self.pyds_available)?;
            st.serialize_field("pyds_version", &self.pyds_version)?;
            st.serialize_field("gst_plugins_ok", &self.gst_plugins_ok)?;
            st.serialize_field("gst_missing", &self.gst_missing)?;
            st.serialize_field("python_bin", &self.python_bin)?;
            st.serialize_field("last_check_at", &self.last_check_at)?;
            st.serialize_field("install_hint", &self.install_hint)?;
            st.end()
        }
    }
}

#[async_trait]
impl Extension for DeepStreamExtension {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("deepstream", "NVIDIA DeepStream", env!("CARGO_PKG_VERSION"))
                .with_description("Multi-stream RTSP video inference via NVIDIA DeepStream")
                .with_author("NeoMind Team")
        })
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("add_stream")
                .display_name("Add Stream")
                .description("Add an RTSP/file source with model + analytics config")
                .param(
                    ParamBuilder::new("stream_id", MetricDataType::String)
                        .display_name("Stream ID")
                        .description("Unique identifier for this stream")
                        .required()
                        .build(),
                )
                .param(
                    ParamBuilder::new("config", MetricDataType::String)
                        .display_name("Config JSON")
                        .description("JSON config per spec §3.1.1")
                        .required()
                        .build(),
                )
                .sample(serde_json::json!({
                    "stream_id": "cam_front",
                    "config": {"source":{"type":"rtsp","url":"rtsp://..."},"model":"yolov8n-coco"}
                }))
                .build(),
            CommandBuilder::new("remove_stream")
                .display_name("Remove Stream")
                .description("Remove a stream and stop its pipeline")
                .param(
                    ParamBuilder::new("stream_id", MetricDataType::String)
                        .display_name("Stream ID")
                        .required()
                        .build(),
                )
                .sample(serde_json::json!({"stream_id": "cam_front"}))
                .build(),
            CommandBuilder::new("list_streams")
                .display_name("List Streams")
                .description("List all streams with their current status")
                .sample(serde_json::json!({}))
                .build(),
            CommandBuilder::new("get_stream_info")
                .display_name("Get Stream Info")
                .description("Get detailed info for one stream")
                .param(
                    ParamBuilder::new("stream_id", MetricDataType::String)
                        .display_name("Stream ID")
                        .required()
                        .build(),
                )
                .sample(serde_json::json!({"stream_id": "cam_front"}))
                .build(),
            CommandBuilder::new("update_analytics")
                .display_name("Update Analytics")
                .description("Hot-swap line-crossing / ROI config on a running stream")
                .param(
                    ParamBuilder::new("stream_id", MetricDataType::String)
                        .display_name("Stream ID")
                        .required()
                        .build(),
                )
                .param(
                    ParamBuilder::new("config", MetricDataType::String)
                        .display_name("Config JSON")
                        .description("Analytics config: {line_crossing:[...], roi:[...]}")
                        .required()
                        .build(),
                )
                .sample(serde_json::json!({
                    "stream_id": "cam_front",
                    "config": {"line_crossing": [{"id":"l1","points":[[0,100],[1920,100]],"mode":"balanced","classes":[0]}]}
                }))
                .build(),
            CommandBuilder::new("set_threshold")
                .display_name("Set Threshold")
                .description("Hot-swap model confidence / IOU thresholds")
                .param(
                    ParamBuilder::new("stream_id", MetricDataType::String)
                        .display_name("Stream ID")
                        .required()
                        .build(),
                )
                .param_optional(
                    "conf",
                    "Confidence",
                    MetricDataType::Float,
                )
                .param_optional(
                    "iou",
                    "IOU",
                    MetricDataType::Float,
                )
                .sample(serde_json::json!({"stream_id": "cam_front", "conf": 0.5, "iou": 0.45}))
                .build(),
            CommandBuilder::new("list_models")
                .display_name("List Models")
                .description("List preset + user-registered model options")
                .sample(serde_json::json!({}))
                .build(),
            CommandBuilder::new("register_model")
                .display_name("Register Model")
                .description("Register a user-provided model (engine/etlt/onnx) with the extension")
                .param(
                    ParamBuilder::new("id", MetricDataType::String)
                        .display_name("Model ID")
                        .required()
                        .build(),
                )
                .param(
                    ParamBuilder::new("name", MetricDataType::String)
                        .display_name("Display Name")
                        .required()
                        .build(),
                )
                .param(
                    ParamBuilder::new("engine_path", MetricDataType::String)
                        .display_name("Engine Path")
                        .description("Path to .etlt / .onnx / .engine")
                        .required()
                        .build(),
                )
                .param_optional(
                    "labels_path",
                    "Labels Path",
                    MetricDataType::String,
                )
                .param(
                    ParamBuilder::new("precision", MetricDataType::String)
                        .display_name("Precision")
                        .options(vec!["fp16".into(), "int8".into(), "fp32".into()])
                        .optional()
                        .build(),
                )
                .sample(serde_json::json!({
                    "id": "yolov8s-custom",
                    "name": "YOLOv8s Custom",
                    "engine_path": "/models/yolov8s.engine",
                    "labels_path": "/models/labels.txt",
                    "precision": "fp16"
                }))
                .build(),
            CommandBuilder::new("restart_sidecar")
                .display_name("Restart Sidecar")
                .description("Restart the Python sidecar process (replays active streams)")
                .sample(serde_json::json!({}))
                .build(),
            CommandBuilder::new("diagnose")
                .display_name("Diagnose")
                .description("Run pre-flight checks (DeepStream, pyds, GStreamer plugins)")
                .sample(serde_json::json!({}))
                .build(),
        ]
    }

    fn metrics(&self) -> Vec<neomind_extension_sdk::MetricDescriptor> {
        vec![]
    }

    async fn execute_command(
        &self,
        cmd: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        match cmd {
            "add_stream" => self.cmd_add_stream(args).await,
            "remove_stream" => self.cmd_remove_stream(args).await,
            "list_streams" => self.cmd_list_streams(args).await,
            "get_stream_info" => self.cmd_get_stream_info(args).await,
            "update_analytics" => self.cmd_update_analytics(args).await,
            "set_threshold" => self.cmd_set_threshold(args).await,
            "list_models" => self.cmd_list_models(args).await,
            "register_model" => self.cmd_register_model(args).await,
            "restart_sidecar" => self.cmd_restart_sidecar(args).await,
            "diagnose" => self.cmd_diagnose(args).await,
            _ => Err(ExtensionError::CommandNotFound(cmd.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<neomind_extension_sdk::ExtensionMetricValue>> {
        Ok(vec![])
    }
}

neomind_extension_sdk::neomind_export!(DeepStreamExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_id() {
        assert_eq!(DeepStreamExtension::new().metadata().id, "deepstream");
    }

    #[test]
    fn panic_unwind_invariant() {
        // Workspace profile sets panic=unwind; member override would silently break this.
        // See CLAUDE.md "Safety Requirements".
        // NOTE: this only fires meaningfully under `cargo test --release` because the dev
        // profile defaults to panic=unwind anyway. CI must run release tests for this guard
        // to catch regressions in [profile.release] overrides.
        assert!(
            cfg!(panic = "unwind"),
            "panic must be unwind — check workspace Cargo.toml [profile.release]"
        );
    }

    #[test]
    fn commands_returns_10_entries() {
        let cmds = DeepStreamExtension::new().commands();
        assert_eq!(cmds.len(), 10, "expected 10 ExtensionCommand entries");
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"add_stream"), "names: {names:?}");
        assert!(names.contains(&"remove_stream"), "names: {names:?}");
        assert!(names.contains(&"diagnose"), "names: {names:?}");
        // Spot-check the others so a silent rename is caught.
        for expected in [
            "list_streams",
            "get_stream_info",
            "update_analytics",
            "set_threshold",
            "list_models",
            "register_model",
            "restart_sidecar",
        ] {
            assert!(names.contains(&expected), "missing {expected} in {names:?}");
        }
    }

    #[test]
    fn add_stream_command_has_required_params() {
        let cmds = DeepStreamExtension::new().commands();
        let add_cmd = cmds
            .iter()
            .find(|c| c.name == "add_stream")
            .expect("add_stream command declared");
        let stream_id = add_cmd
            .parameters
            .iter()
            .find(|p| p.name == "stream_id")
            .expect("stream_id param present");
        assert!(stream_id.required, "stream_id must be required");
        let config = add_cmd
            .parameters
            .iter()
            .find(|p| p.name == "config")
            .expect("config param present");
        assert!(config.required, "config must be required");
    }

    #[tokio::test]
    async fn list_streams_on_empty_manager_returns_empty_array() {
        // list_streams doesn't touch the sidecar — safe to call on a fresh
        // extension (no sidecar wired).
        let ext = DeepStreamExtension::new();
        let result = ext
            .execute_command("list_streams", &serde_json::json!({}))
            .await
            .expect("list_streams ok");
        let arr = result
            .get("streams")
            .and_then(|v| v.as_array())
            .expect("streams array");
        assert!(arr.is_empty(), "empty manager → empty array, got {arr:?}");
    }

    #[tokio::test]
    async fn register_model_then_list_models_includes_it() {
        let ext = DeepStreamExtension::new();
        let register_args = serde_json::json!({
            "id": "test-yolov8s",
            "name": "Test YOLOv8s",
            "engine_path": "/tmp/test.engine",
            "precision": "fp16",
        });
        ext.execute_command("register_model", &register_args)
            .await
            .expect("register_model ok");

        let list = ext
            .execute_command("list_models", &serde_json::json!({}))
            .await
            .expect("list_models ok");
        let models = list
            .get("models")
            .and_then(|v| v.as_array())
            .expect("models array");
        // preset + 1 registered = 2
        assert_eq!(models.len(), 2, "models: {models:?}");
        // preset present
        assert!(
            models
                .iter()
                .any(|m| m.get("id").and_then(|v| v.as_str()) == Some("yolov8n-coco")
                    && m.get("preset").and_then(|v| v.as_bool()) == Some(true)),
            "preset missing: {models:?}"
        );
        // registered present
        assert!(
            models
                .iter()
                .any(|m| m.get("id").and_then(|v| v.as_str()) == Some("test-yolov8s")
                    && m.get("preset").and_then(|v| v.as_bool()) == Some(false)),
            "registered missing: {models:?}"
        );
    }
}
