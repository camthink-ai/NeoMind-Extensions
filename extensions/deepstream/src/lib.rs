//! DeepStream extension — see docs/superpowers/specs/2026-07-06-deepstream-extension-design.md

pub mod event_router;
pub mod metrics_bridge;
pub mod protocol;
pub mod sidecar;
pub mod stream_manager;
pub mod system_status;
pub mod url_redact;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::{Mutex as PlMutex, RwLock};
use tokio::task::JoinHandle;

use neomind_extension_sdk::{
    CapabilityContext, CommandBuilder, Extension, ExtensionCommand, ExtensionError,
    ExtensionMetadata, MetricDataType, ParamBuilder, ParameterDefinition, ParamMetricValue, Result,
};

use crate::event_router::EventRouter;
use crate::protocol::{ControlMessage, SidecarEvent};
use crate::sidecar::{SidecarHandle, SidecarSupervisor};
use crate::stream_manager::{StreamConfig, StreamManager, StreamManagerError};
use crate::system_status::SystemStatus;

/// Max concurrent streams the extension will accept (spec §3.1.1).
const DEFAULT_MAX_STREAMS: u32 = 32;

/// Default TCP port the remote `sidecar_bridge.py` daemon listens on.
/// Used when `sidecar_mode="remote"` and `sidecar_port` is not explicitly set.
const DEFAULT_SIDECAR_BRIDGE_PORT: u16 = 9556;

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

/// Config sent to the sidecar via the `Hello` handshake message. Populated
/// from `with_config_parameters()` metadata defaults and overridable via
/// the `configure()` lifecycle method. Without this reaching the sidecar,
/// the sidecar blocks in `_read_hello()` (300s timeout) and silently drops
/// every `AddStream` / `RemoveStream`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HandshakeConfig {
    /// RTSP server port (annotated output) — default 8554.
    pub rtsp_port: u16,
    /// HTTP port for snapshot JPEGs — default 8555.
    pub snapshot_port: u16,
    /// Sidecar log level — default "info".
    pub log_level: String,
    /// Where preset + user-registered model files live.
    pub models_dir: String,
    /// Hard cap on concurrent streams; Python enforces with GPU memory check.
    pub max_streams: u32,
    /// Bind address for the snapshot HTTP server — default "0.0.0.0".
    pub snapshot_bind_addr: String,
}

impl Default for HandshakeConfig {
    fn default() -> Self {
        Self {
            rtsp_port: 8554,
            snapshot_port: 8555,
            log_level: "info".to_string(),
            models_dir: "/opt/nvidia/deepstream/deepstream/samples/models".to_string(),
            max_streams: DEFAULT_MAX_STREAMS,
            snapshot_bind_addr: "0.0.0.0".to_string(),
        }
    }
}

pub struct DeepStreamExtension {
    /// Authoritative stream state. Arc so the supervisor's on_restart callback
    /// can share it when wiring replay.
    streams: Arc<StreamManager>,
    /// User-registered models (preset models live in the sidecar config).
    registered_models: RwLock<HashMap<String, ModelRegistration>>,
    /// Cached system status from the last `diagnose` run.
    system_status: RwLock<Option<SystemStatus>>,
    /// Current sidecar handle. None until ensure_sidecar() has run.
    /// Arc+RwLock so execute_command can grab a snapshot and the supervisor
    /// can swap it on restart.
    sidecar: Arc<RwLock<Option<Arc<SidecarHandle>>>>,
    /// Owns the live sidecar + watch loop. None until ensure_sidecar().
    supervisor: Arc<RwLock<Option<Arc<SidecarSupervisor>>>>,
    /// Watch-loop JoinHandle (from supervisor.start()). Aborted on restart.
    watch_task: Arc<PlMutex<Option<JoinHandle<()>>>>,
    /// Serializes concurrent sidecar startups (add_stream racing restart_sidecar, etc.).
    startup_lock: tokio::sync::Mutex<()>,
    /// Metrics bridge: 7 global atomics + 9 per-stream dynamic templates.
    metrics: Arc<metrics_bridge::MetricsBridge>,
    /// Handshake config (ports, models_dir, log_level, max_streams) sent to the
    /// sidecar via `Hello`. Overridable via `configure()`; defaults from
    /// `with_config_parameters()`. Arc-cloned into the on_restart callback so
    /// crash-recovery respawns handshake with the same config.
    sidecar_config: Arc<RwLock<HandshakeConfig>>,
    /// Frontend-facing DeepStream server address (e.g. the Jetson's IP). NOT
    /// sent to the sidecar — the sidecar binds `snapshot_bind_addr` itself;
    /// this is purely the address the dashboard should use to reach the
    /// RTSP/snapshot endpoints. Empty string = derive from page hostname
    /// (same-host deployment). Surfaced via `list_streams` / `get_stream_info`
    /// so cards can fall back to it when their per-card `serverHost` is unset.
    server_host: Arc<RwLock<String>>,
    /// Sidecar transport mode: `"local"` (default) spawns a child process on
    /// the same host; `"remote"` connects to a `sidecar_bridge.py` daemon on
    /// a Jetson over TCP (路 C — for when NeoMind runs off-device).
    sidecar_mode: Arc<RwLock<String>>,
    /// Remote daemon hostname (only used when `sidecar_mode == "remote"`).
    sidecar_host: Arc<RwLock<String>>,
    /// Remote daemon TCP port (only used when `sidecar_mode == "remote"`).
    /// Default [`DEFAULT_SIDECAR_BRIDGE_PORT`] (9556).
    sidecar_port: Arc<RwLock<u16>>,
    /// Models loaded by the sidecar, reported in HelloAck. Updated after
    /// every handshake. Used by `cmd_list_models` so the dashboard dropdown
    /// shows the real presets (Primary_Detector, etc.) instead of a stale
    /// hardcoded default.
    sidecar_models: Arc<RwLock<Vec<String>>>,
    /// EventRouter — classifies + publishes sidecar events to the NeoMind
    /// EventBus. Passed into every reader_loop via the supervisor so stats /
    /// detection / analytics reach the frontend WS in real time. The internal
    /// channel receivers are intentionally dropped (nobody consumes them); with
    /// the unconditional maybe_publish fix, EventBus delivery does not depend
    /// on channel capacity.
    event_router: Arc<EventRouter>,
}

impl Default for DeepStreamExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepStreamExtension {
    pub fn new() -> Self {
        let event_router = Self::build_event_router();
        Self {
            streams: Arc::new(StreamManager::new(DEFAULT_MAX_STREAMS)),
            registered_models: RwLock::new(HashMap::new()),
            system_status: RwLock::new(None),
            sidecar: Arc::new(RwLock::new(None)),
            supervisor: Arc::new(RwLock::new(None)),
            watch_task: Arc::new(PlMutex::new(None)),
            startup_lock: tokio::sync::Mutex::new(()),
            metrics: Arc::new(metrics_bridge::MetricsBridge::new()),
            sidecar_config: Arc::new(RwLock::new(HandshakeConfig::default())),
            server_host: Arc::new(RwLock::new(String::new())),
            sidecar_mode: Arc::new(RwLock::new("local".to_string())),
            sidecar_host: Arc::new(RwLock::new(String::new())),
            sidecar_port: Arc::new(RwLock::new(DEFAULT_SIDECAR_BRIDGE_PORT)),
            sidecar_models: Arc::new(RwLock::new(Vec::new())),
            event_router,
        }
    }

    /// Create the EventRouter with fresh internal channels (receivers dropped —
    /// nobody consumes them) and set the CapabilityContext so `maybe_publish`
    /// can invoke the `event_publish` host capability.
    fn build_event_router() -> Arc<EventRouter> {
        use tokio::sync::mpsc;
        let (priority_tx, _priority_rx) = mpsc::unbounded_channel();
        let (business_tx, _business_rx) = mpsc::unbounded_channel();
        let (detection_tx, _detection_rx) = mpsc::channel(512);
        let (stats_tx, _stats_rx) = mpsc::channel(64);
        let router = EventRouter::new(priority_tx, business_tx, detection_tx, stats_tx);
        router.set_context(CapabilityContext::default());
        Arc::new(router)
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
            supervisor: Arc::new(RwLock::new(None)),
            watch_task: Arc::new(PlMutex::new(None)),
            startup_lock: tokio::sync::Mutex::new(()),
            metrics: Arc::new(metrics_bridge::MetricsBridge::new()),
            sidecar_config: Arc::new(RwLock::new(HandshakeConfig::default())),
            server_host: Arc::new(RwLock::new(String::new())),
            sidecar_mode: Arc::new(RwLock::new("local".to_string())),
            sidecar_host: Arc::new(RwLock::new(String::new())),
            sidecar_port: Arc::new(RwLock::new(DEFAULT_SIDECAR_BRIDGE_PORT)),
            sidecar_models: Arc::new(RwLock::new(Vec::new())),
            event_router: Self::build_event_router(),
        }
    }

    /// Resolve the Python interpreter path + sidecar script path for spawning.
    ///
    /// python_bin: from cached SystemStatus if available, otherwise runs
    /// `diagnose` checks. Falls back to "python3" if detection fails.
    ///
    /// script_path: searched in order:
    ///   1. `DEEPSTREAM_SIDECAR_PATH` env var (absolute path to .py)
    ///   2. `NEOMIND_EXTENSION_DIR` env var + `/sidecar/deepstream_runner.py`
    ///   3. `CARGO_MANIFEST_DIR` + `/sidecar/deepstream_runner.py` (dev fallback)
    ///   4. `./sidecar/deepstream_runner.py` (last resort)
    async fn resolve_spawn_config(&self) -> Result<(String, PathBuf)> {
        // python_bin — use cached status if fresh, otherwise run checks.
        let python_bin = {
            let cached = self.system_status.read().clone();
            let py = cached
                .as_ref()
                .and_then(|s| s.python_bin.clone())
                .filter(|p| !p.is_empty());
            match py {
                Some(p) => p,
                None => {
                    let status = SystemStatus::run_checks().await;
                    let p = status.python_bin.clone().unwrap_or_else(|| "python3".to_string());
                    *self.system_status.write() = Some(status);
                    p
                }
            }
        };

        // script_path — env vars first, then filesystem fallbacks.
        let script_path = std::env::var("DEEPSTREAM_SIDECAR_PATH")
            .ok()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .or_else(|| {
                std::env::var("NEOMIND_EXTENSION_DIR")
                    .ok()
                    .map(PathBuf::from)
                    .map(|d| d.join("sidecar").join("deepstream_runner.py"))
                    .filter(|p| p.exists())
            })
            .or_else(|| {
                // Dev fallback: relative to the compiled crate root.
                let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("sidecar")
                    .join("deepstream_runner.py");
                if p.exists() { Some(p) } else { None }
            })
            .or_else(|| {
                let p = PathBuf::from("sidecar").join("deepstream_runner.py");
                if p.exists() { Some(p) } else { None }
            })
            .ok_or_else(|| {
                ExtensionError::ExecutionFailed(format!(
                    "deepstream_runner.py not found. Set DEEPSTREAM_SIDECAR_PATH or NEOMIND_EXTENSION_DIR."
                ))
            })?;

        Ok((python_bin, script_path))
    }

    /// Build a fresh supervisor according to the current sidecar mode.
    ///
    /// - `local` (default): resolves python_bin + script_path (via
    ///   `resolve_spawn_config`) and returns `SidecarSupervisor::new`.
    /// - `remote`: returns `SidecarSupervisor::new_remote(host, port)` without
    ///   touching the local filesystem — this is what makes the extension
    ///   usable on a non-Jetson host (macOS dev box, x86 server) that has no
    ///   pyds / GStreamer / deepstream_runner.py installed.
    ///
    /// The returned supervisor carries no stream state; replay is the caller's
    /// job (see `cmd_restart_sidecar` and the on_restart callback).
    async fn build_supervisor(&self) -> Result<Arc<SidecarSupervisor>> {
        let mode = self.sidecar_mode.read().clone();
        let mut sup = match mode.as_str() {
            "remote" => {
                let host = self.sidecar_host.read().clone();
                if host.is_empty() {
                    return Err(ExtensionError::InvalidArguments(format!(
                        "sidecar_mode='remote' but sidecar_host is empty — set the Jetson's IP in the extension config"
                    )));
                }
                let port = *self.sidecar_port.read();
                eprintln!("[deepstream] starting REMOTE sidecar bridge: {}:{}", host, port);
                SidecarSupervisor::new_remote(&host, port)
            }
            // Treat "local" and any unrecognized value as local (default path).
            // Unknown values are logged so a typo doesn't silently switch modes.
            other => {
                if other != "local" {
                    eprintln!(
                        "[deepstream] unrecognized sidecar_mode='{other}' — falling back to local"
                    );
                }
                let (python_bin, script_path) = self.resolve_spawn_config().await?;
                eprintln!(
                    "[deepstream] starting LOCAL sidecar: {} {}",
                    python_bin,
                    script_path.display()
                );
                SidecarSupervisor::new(&python_bin, script_path)
            }
        };
        // Wire the EventRouter so every reader_loop publishes sidecar events
        // to the NeoMind EventBus (stats / detection / analytics).
        sup.set_router(self.event_router.clone());
        Ok(Arc::new(sup))
    }

    /// Lazily start the sidecar supervisor if not already running, then return
    /// the current handle. Uses `startup_lock` to serialize concurrent
    /// startups (e.g., two add_stream calls racing on a cold extension).
    async fn ensure_sidecar(&self) -> Result<Arc<SidecarHandle>> {
        crate::sidecar::dbg_log("ensure_sidecar: enter");
        // Fast path: handle already available.
        {
            let guard = self.sidecar.read();
            if let Some(h) = guard.as_ref() {
                crate::sidecar::dbg_log("ensure_sidecar: fast path (handle exists)");
                return Ok(h.clone());
            }
        }

        // Slow path: serialize startups.
        crate::sidecar::dbg_log("ensure_sidecar: acquiring startup_lock");
        let _guard = self.startup_lock.lock().await;
        crate::sidecar::dbg_log("ensure_sidecar: startup_lock acquired");

        // Double-check after acquiring the lock (another task may have started it).
        {
            let sc = self.sidecar.read();
            if let Some(h) = sc.as_ref() {
                crate::sidecar::dbg_log("ensure_sidecar: double-check hit");
                return Ok(h.clone());
            }
        }

        let sup = self.build_supervisor().await?;

        // on_restart callback: fires on crash-recovery respawns (not initial start).
        // Updates self.sidecar + handshakes the fresh process + replays active
        // stream configs. Handshake BEFORE replay is mandatory: a fresh sidecar
        // blocks in _read_hello() and will drop every replayed AddStream.
        let sidecar_field = self.sidecar.clone();
        let streams = self.streams.clone();
        let metrics = self.metrics.clone();
        let config_clone = self.sidecar_config.clone();
        let models_field = self.sidecar_models.clone();
        let on_restart = move |handle: Arc<SidecarHandle>| {
            eprintln!("[deepstream] supervisor delivered fresh handle after crash recovery");
            *sidecar_field.write() = Some(handle.clone());
            metrics.mark_sidecar_started();
            let config_for_handshake = config_clone.read().clone();
            let streams = streams.clone();
            let models_clone = models_field.clone();
            tokio::spawn(async move {
                match DeepStreamExtension::perform_handshake(&handle, &config_for_handshake).await {
                    Ok(ack) => {
                        eprintln!(
                            "[deepstream] post-restart handshake ok: max_streams={}, models_loaded={:?}",
                            ack.max_streams, ack.models_loaded
                        );
                        *models_clone.write() = ack.models_loaded.clone();
                    }
                    Err(e) => {
                        eprintln!(
                            "[deepstream] post-restart handshake failed: {e} — skipping replay"
                        );
                        return;
                    }
                }
                let summary = streams.replay_to(&handle).await;
                eprintln!(
                    "[deepstream] post-restart replay: {} succeeded, {} failed",
                    summary.succeeded.len(),
                    summary.failed.len()
                );
                for f in &summary.failed {
                    eprintln!("[deepstream]   FAILED {}: {}", f.stream_id, f.error);
                }
            });
        };

        let (handle, watch_task) = sup
            .clone()
            .start(on_restart)
            .await
            .map_err(|e| {
                crate::sidecar::dbg_log(&format!("ensure_sidecar: sup.start() FAILED: {e}"));
                ExtensionError::ExecutionFailed(format!("sidecar spawn failed: {e}"))
            })?;

        crate::sidecar::dbg_log("ensure_sidecar: sup.start() ok, starting handshake");

        // Initial handshake before publishing the handle. A sidecar that hasn't
        // been Hello'd silently drops every command; surfacing the failure here
        // is better than the caller's first add_stream timing out in wait_event.
        let config = self.sidecar_config.read().clone();
        let ack = Self::perform_handshake(&handle, &config).await.map_err(|e| {
            eprintln!("[deepstream] initial handshake failed: {e}");
            e
        })?;
        eprintln!(
            "[deepstream] handshake ok: max_streams={}, models_loaded={:?}, rtsp_prefix={}",
            ack.max_streams, ack.models_loaded, ack.rtsp_url_prefix
        );
        // Sidecar may negotiate a different max_streams (e.g. GPU memory check).
        if ack.max_streams != config.max_streams {
            eprintln!(
                "[deepstream] sidecar negotiated max_streams {} → {}",
                config.max_streams, ack.max_streams
            );
            self.sidecar_config.write().max_streams = ack.max_streams;
        }
        *self.sidecar_models.write() = ack.models_loaded.clone();

        *self.supervisor.write() = Some(sup);
        *self.watch_task.lock() = Some(watch_task);
        *self.sidecar.write() = Some(handle.clone());
        self.metrics.mark_sidecar_started();

        Ok(handle)
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

    /// Ready → Hello → HelloAck handshake. Called after every sidecar spawn
    /// (initial `ensure_sidecar`, crash-recovery respawn, manual
    /// `restart_sidecar`). Without this, the sidecar blocks in its
    /// `_read_hello()` loop (300s timeout) and silently drops every command.
    ///
    /// 10s timeouts on both Ready and HelloAck. Non-matching events arriving
    /// before Ready / HelloAck are drained (the sidecar emits nothing else
    /// pre-handshake, so this is purely defensive).
    async fn perform_handshake(
        handle: &SidecarHandle,
        config: &HandshakeConfig,
    ) -> Result<crate::protocol::HelloAck> {
        // Reconstruct the HelloAck fields without a protocol re-export.
        use crate::protocol::{ControlMessage, SidecarEvent};

        // 1. Wait for Ready (10s timeout). Drain any non-matching events.
        match tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match handle.recv().await {
                    Some(SidecarEvent::Ready { .. }) => return Ok(()),
                    Some(_) => continue,
                    None => {
                        return Err(ExtensionError::ExecutionFailed(
                            "sidecar stdout closed before Ready".into(),
                        ))
                    }
                }
            }
        })
        .await
        {
            Ok(inner) => inner?,
            Err(_) => {
                return Err(ExtensionError::ExecutionFailed(
                    "sidecar Ready timeout (10s)".into(),
                ))
            }
        }

        // 2. Send Hello with the 6 handshake fields.
        handle
            .send(&ControlMessage::Hello {
                rtsp_port: config.rtsp_port,
                snapshot_port: config.snapshot_port,
                log_level: config.log_level.clone(),
                models_dir: config.models_dir.clone(),
                max_streams: config.max_streams,
                snapshot_bind_addr: config.snapshot_bind_addr.clone(),
            })
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(format!("send Hello: {e}")))?;

        // 3. Wait for HelloAck (10s timeout). An ErrorResponse here means the
        //    sidecar rejected the config (bad port, missing models_dir, etc.).
        match tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match handle.recv().await {
                    Some(SidecarEvent::HelloAck {
                        max_streams,
                        rtsp_url_prefix,
                        models_loaded,
                    }) => {
                        return Ok(crate::protocol::HelloAck {
                            max_streams,
                            rtsp_url_prefix,
                            models_loaded,
                        })
                    }
                    Some(SidecarEvent::ErrorResponse { code, message, .. }) => {
                        return Err(ExtensionError::ExecutionFailed(format!(
                            "sidecar rejected Hello: {code} {message}"
                        )))
                    }
                    Some(_) => continue,
                    None => {
                        return Err(ExtensionError::ExecutionFailed(
                            "sidecar stdout closed before HelloAck".into(),
                        ))
                    }
                }
            }
        })
        .await
        {
            Ok(inner) => inner,
            Err(_) => Err(ExtensionError::ExecutionFailed(
                "sidecar HelloAck timeout (10s)".into(),
            )),
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

        // Send AddStream to the sidecar.
        let req_id = uuid::Uuid::new_v4().to_string();
        let handle = self.ensure_sidecar().await?;
        handle
            .send(&ControlMessage::AddStream {
                id: req_id.clone(),
                config: serde_json::to_value(&config)
                    .unwrap_or_else(|_| serde_json::Value::Null),
            })
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(format!("send add_stream: {e}")))?;

        // NON-BLOCKING: The extension runner processes IPC commands sequentially
        // (handle_message().await in the main loop). If we wait_event here for
        // up to 60s (DeepStream pipeline build on Jetson), every list_streams
        // / get_stream_info poll from the frontend is blocked in the IPC queue,
        // leaving the dashboard empty even though the add succeeded. Instead,
        // return immediately with status "connecting" and resolve the
        // StreamAdded / ErrorResponse asynchronously in a background task.
        let streams = self.streams.clone();
        let sidecar_slot = self.sidecar.clone();
        let sid = stream_id.clone();
        let wait_req_id = req_id.clone();
        eprintln!("[deepstream] add_stream: spawning bg task for {sid} req_id={wait_req_id}");
        // MUST use persistent_runtime().handle().spawn — NOT bare tokio::spawn.
        // execute_command runs inside pollster::block_on (FFI boundary, no tokio
        // context), so bare tokio::spawn panics ("no reactor running"). The
        // persistent runtime is the same 2-worker runtime that owns the sidecar's
        // reader_loop / heartbeat / I/O resources.
        crate::sidecar::persistent_runtime().handle().spawn(async move {
            // Brief read-lock to clone the Arc<SidecarHandle> out — we must
            // NOT hold the parking_lot guard across the 60s wait_event await.
            let handle = {
                let guard = sidecar_slot.read();
                guard.as_ref().cloned()
            };
            let Some(handle) = handle else {
                eprintln!("[deepstream] add_stream bg: sidecar gone, removing {sid}");
                let _ = streams.remove(&sid);
                return;
            };
            eprintln!("[deepstream] add_stream bg: got handle, starting wait_event for {sid}");

            // DeepStream pipeline build on Jetson can take 30+ seconds
            // (engine cache miss, tracker init, encoder open). 60s ceiling.
            let matched = Self::wait_event(
                &handle,
                Duration::from_secs(60),
                |ev| match ev {
                    SidecarEvent::StreamAdded { id, .. } => id == &wait_req_id,
                    SidecarEvent::ErrorResponse { id, .. } => id == &wait_req_id,
                    _ => false,
                },
            )
            .await;

            match matched {
                Ok(SidecarEvent::StreamAdded {
                    rtsp_url, snapshot_token, ..
                }) => {
                    let _ = streams.set_rtsp_url(&sid, rtsp_url, snapshot_token);
                    let _ = streams
                        .transition(&sid, crate::stream_manager::StreamStatus::Running);
                    eprintln!("[deepstream] add_stream bg: {sid} → running");
                }
                Ok(SidecarEvent::ErrorResponse { code, message, .. }) => {
                    eprintln!(
                        "[deepstream] add_stream bg: sidecar rejected {sid}: {message} ({code})"
                    );
                    let _ = streams.remove(&sid);
                }
                // wait_event synthesizes a Bye on stdout close, or times out
                other => {
                    eprintln!("[deepstream] add_stream bg: {sid} failed: {other:?}");
                    let _ = streams.remove(&sid);
                }
            }
        });

        // Return immediately — the card shows up as "connecting" right away.
        // The frontend's 3s list_streams poll will pick up the rtsp_url once
        // the background task transitions the stream to Running.
        Ok(serde_json::json!({
            "stream_id": stream_id,
            "status": "connecting",
        }))
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
        let handle = self.ensure_sidecar().await?;
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

    /// Returns `server_host` if set; otherwise falls back to `sidecar_host`
    /// (in remote mode, mediamtx and the sidecar run on the same host, so
    /// `sidecar_host` is a valid default for building browser-facing HLS /
    /// snapshot / RTSP URLs). Empty if neither is set.
    fn effective_server_host(&self) -> String {
        let sh = self.server_host.read().clone();
        if !sh.is_empty() {
            return sh;
        }
        self.sidecar_host.read().clone()
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
                    "snapshot_token": s.snapshot_token,
                    "model": s.config.model,
                    "added_at": s.added_at,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "streams": arr,
            "server_host": self.effective_server_host(),
        }))
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
            "snapshot_token": state.snapshot_token,
            "model": state.config.model,
            "source": state.config.source,
            "added_at": state.added_at,
            "last_transition_at": state.last_transition_at,
            "server_host": self.effective_server_host(),
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
        let handle = self.ensure_sidecar().await?;
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
        let handle = self.ensure_sidecar().await?;
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
        // Preset models reported by the sidecar in HelloAck. Empty before
        // the first successful handshake (dashboard shows an empty dropdown).
        let loaded = self.sidecar_models.read();
        for id in loaded.iter() {
            out.push(serde_json::json!({
                "id": id,
                "name": id,
                "preset": true,
            }));
        }
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
        // Serialize against concurrent startups / restarts.
        let _guard = self.startup_lock.lock().await;

        // 1. Tear down the existing supervisor + watch loop.
        let prev_count = self.streams.list().len();
        let old_sup = self.supervisor.write().take();
        if let Some(sup) = old_sup {
            eprintln!("[deepstream] restart_sidecar: shutting down current supervisor");
            if let Err(e) = sup.shutdown().await {
                eprintln!("[deepstream] restart_sidecar: shutdown error: {e:?}");
            }
        }
        if let Some(task) = self.watch_task.lock().take() {
            task.abort();
        }
        *self.sidecar.write() = None;
        self.metrics.mark_sidecar_stopped();

        // 2. Spawn a fresh supervisor. Mode dispatch happens under the lock
        //    so no other command can observe a half-restarted state.
        let sup = self.build_supervisor().await?;
        eprintln!(
            "[deepstream] restart_sidecar: respawning sidecar (mode={})",
            self.sidecar_mode.read()
        );

        let sidecar_field = self.sidecar.clone();
        let streams = self.streams.clone();
        let metrics = self.metrics.clone();
        let config_clone = self.sidecar_config.clone();
        let models_field = self.sidecar_models.clone();
        let on_restart = move |handle: Arc<SidecarHandle>| {
            eprintln!("[deepstream] supervisor delivered fresh handle after crash recovery");
            *sidecar_field.write() = Some(handle.clone());
            metrics.mark_sidecar_started();
            let config_for_handshake = config_clone.read().clone();
            let streams = streams.clone();
            let models_clone = models_field.clone();
            tokio::spawn(async move {
                match DeepStreamExtension::perform_handshake(&handle, &config_for_handshake).await {
                    Ok(ack) => {
                        eprintln!(
                            "[deepstream] post-restart handshake ok: max_streams={}, models_loaded={:?}",
                            ack.max_streams, ack.models_loaded
                        );
                        *models_clone.write() = ack.models_loaded.clone();
                    }
                    Err(e) => {
                        eprintln!(
                            "[deepstream] post-restart handshake failed: {e} — skipping replay"
                        );
                        return;
                    }
                }
                let summary = streams.replay_to(&handle).await;
                eprintln!(
                    "[deepstream] post-restart replay: {} succeeded, {} failed",
                    summary.succeeded.len(),
                    summary.failed.len()
                );
                for f in &summary.failed {
                    eprintln!("[deepstream]   FAILED {}: {}", f.stream_id, f.error);
                }
            });
        };

        let (handle, watch_task) = sup
            .clone()
            .start(on_restart)
            .await
            .map_err(|e| ExtensionError::ExecutionFailed(format!(
                "sidecar respawn failed: {e}"
            )))?;

        *self.supervisor.write() = Some(sup);
        *self.watch_task.lock() = Some(watch_task);
        *self.sidecar.write() = Some(handle.clone());
        self.metrics.mark_sidecar_started();

        // Handshake the freshly-spawned sidecar BEFORE replay. Without this,
        // every replayed AddStream would be silently dropped.
        let config = self.sidecar_config.read().clone();
        let ack = Self::perform_handshake(&handle, &config).await?;
        eprintln!(
            "[deepstream] restart handshake ok: max_streams={}, models_loaded={:?}, rtsp_prefix={}",
            ack.max_streams, ack.models_loaded, ack.rtsp_url_prefix
        );
        if ack.max_streams != config.max_streams {
            eprintln!(
                "[deepstream] sidecar negotiated max_streams {} → {}",
                config.max_streams, ack.max_streams
            );
            self.sidecar_config.write().max_streams = ack.max_streams;
        }
        *self.sidecar_models.write() = ack.models_loaded.clone();

        // 3. Replay active stream configs to the fresh sidecar.
        let summary = if prev_count > 0 {
            eprintln!(
                "[deepstream] restart_sidecar: replaying {} stream(s)",
                prev_count
            );
            self.streams.replay_to(&handle).await
        } else {
            crate::stream_manager::ReplaySummary {
                succeeded: Vec::new(),
                failed: Vec::new(),
            }
        };

        Ok(serde_json::json!({
            "restarted": true,
            "replay": {
                "succeeded": summary.succeeded,
                "failed": summary.failed.iter().map(|f| serde_json::json!({
                    "stream_id": f.stream_id,
                    "error": f.error,
                })).collect::<Vec<_>>(),
            }
        }))
    }

    async fn cmd_diagnose(&self, _args: &serde_json::Value) -> Result<serde_json::Value> {
        // In remote mode, DeepStream runs on the Jetson, not on this host.
        // Running local probes here would always report `deepstream_installed:
        // false`, which would force the frontend StatsBar into the
        // 'not_installed' state even though the sidecar is healthy. Instead,
        // synthesize a status derived from the supervisor's liveness: if the
        // supervisor is alive, treat DeepStream as installed on the remote.
        let mode = self.sidecar_mode.read().clone();
        if mode == "remote" {
            let host = self.server_host.read().clone();
            let port = *self.sidecar_port.read();
            let reachable = if host.is_empty() {
                false
            } else {
                tokio::net::TcpStream::connect((host.as_str(), port))
                    .await
                    .is_ok()
            };
            let status = SystemStatus {
                deepstream_installed: reachable,
                deepstream_version: None,
                pyds_available: reachable,
                pyds_version: None,
                gst_plugins_ok: reachable,
                gst_missing: Vec::new(),
                python_bin: Some("remote".to_string()),
                last_check_at: chrono::Utc::now().timestamp_millis(),
                install_hint: if reachable {
                    format!("sidecar bridge reachable at {host}:{port}")
                } else {
                    format!("sidecar bridge at {host}:{port} unreachable — is sidecar_bridge.py running on the Jetson?")
                },
            };
            let json = serde_json::to_value(&status)
                .map_err(|e| ExtensionError::ExecutionFailed(format!("serialize status: {e}")))?;
            *self.system_status.write() = Some(status);
            return Ok(json);
        }

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
            let defaults = HandshakeConfig::default();
            ExtensionMetadata::new("deepstream", "NVIDIA DeepStream", env!("CARGO_PKG_VERSION"))
                .with_description("Multi-stream RTSP video inference via NVIDIA DeepStream")
                .with_author("NeoMind Team")
                .with_config_parameters(vec![
                    ParameterDefinition {
                        name: "rtsp_port".into(),
                        display_name: "RTSP Port".into(),
                        description: "RTSP server port for annotated output streams".into(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(defaults.rtsp_port as i64)),
                        min: Some(1.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "snapshot_port".into(),
                        display_name: "Snapshot Port".into(),
                        description: "HTTP port for snapshot JPEGs".into(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(
                            defaults.snapshot_port as i64,
                        )),
                        min: Some(1.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "log_level".into(),
                        display_name: "Log Level".into(),
                        description: "Sidecar Python log level".into(),
                        // MetricDataType::Enum (not String+options) — the host's
                        // build_config_schema_dto() only emits the `enum` JSON field
                        // when param_type is the Enum variant, and that's what the
                        // frontend renderConfigInput() keys on to draw a <Select>.
                        param_type: MetricDataType::Enum {
                            options: vec![
                                "debug".into(),
                                "info".into(),
                                "warning".into(),
                                "error".into(),
                            ],
                        },
                        required: false,
                        default_value: Some(ParamMetricValue::String(defaults.log_level.clone())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "models_dir".into(),
                        display_name: "Models Directory".into(),
                        description: "Filesystem path where preset + user-registered model files live".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String(defaults.models_dir.clone())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "max_streams".into(),
                        display_name: "Max Streams".into(),
                        description: "Hard cap on concurrent streams (GPU memory check)".into(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(defaults.max_streams as i64)),
                        min: Some(1.0),
                        max: Some(64.0),
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "snapshot_bind_addr".into(),
                        display_name: "Snapshot Bind Address".into(),
                        description: "Bind address for the snapshot HTTP server".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String(
                            defaults.snapshot_bind_addr.clone(),
                        )),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "server_host".into(),
                        display_name: "DeepStream Server Address".into(),
                        description: "IP/hostname where the DeepStream sidecar runs (e.g. the Jetson's IP, 192.168.93.20). Leave empty to derive from the dashboard's own hostname. Used by frontend cards to build RTSP / snapshot URLs; per-card serverHost overrides this.".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String(String::new())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "sidecar_mode".into(),
                        display_name: "Sidecar Mode".into(),
                        description: "How the extension talks to the DeepStream sidecar. 'local' (default) spawns the sidecar as a child process on this host (requires Jetson + DeepStream SDK installed locally). 'remote' connects to a sidecar_bridge.py daemon on a Jetson over TCP — use this when NeoMind runs on a non-Jetson host (macOS, x86 Linux) and the sidecar runs on a LAN Jetson.".into(),
                        // Enum variant — see log_level above for why this isn't
                        // String + options.
                        param_type: MetricDataType::Enum {
                            options: vec!["local".into(), "remote".into()],
                        },
                        required: false,
                        default_value: Some(ParamMetricValue::String("local".into())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "sidecar_host".into(),
                        display_name: "Sidecar Bridge Host".into(),
                        description: "When sidecar_mode='remote': IP/hostname of the Jetson running sidecar_bridge.py. Ignored in local mode.".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: Some(ParamMetricValue::String(String::new())),
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "sidecar_port".into(),
                        display_name: "Sidecar Bridge Port".into(),
                        description: "When sidecar_mode='remote': TCP port of the sidecar_bridge.py daemon (default 9556). Ignored in local mode.".into(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(
                            DEFAULT_SIDECAR_BRIDGE_PORT as i64,
                        )),
                        min: Some(1.0),
                        max: Some(65535.0),
                        options: Vec::new(),
                    },
                ])
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
        // Dynamic per-stream descriptors (empty until a stream is registered)
        // followed by the 7 static global descriptors.
        let mut out = self.metrics.dynamic.descriptors();
        out.extend(metrics_bridge::global_metric_descriptors());
        out
    }

    /// Apply host-supplied config overrides BEFORE the sidecar is spawned.
    /// The stored `HandshakeConfig` is what `ensure_sidecar()` / the on_restart
    /// callback will send via the `Hello` message; mutations here take effect
    /// on the NEXT spawn (initial or crash-recovery respawn).
    ///
    /// Changing ports / models_dir / max_streams on an already-running sidecar
    /// has no immediate effect — call `restart_sidecar` to force a respawn with
    /// the new config.
    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        {
            let mut cfg = self.sidecar_config.write();
            if let Some(port) = config.get("rtsp_port").and_then(|v| v.as_u64()) {
                if !(1..=65535).contains(&port) {
                    return Err(ExtensionError::InvalidArguments(format!(
                        "rtsp_port out of range: {port}"
                    )));
                }
                cfg.rtsp_port = port as u16;
            }
            if let Some(port) = config.get("snapshot_port").and_then(|v| v.as_u64()) {
                if !(1..=65535).contains(&port) {
                    return Err(ExtensionError::InvalidArguments(format!(
                        "snapshot_port out of range: {port}"
                    )));
                }
                cfg.snapshot_port = port as u16;
            }
            if let Some(level) = config.get("log_level").and_then(|v| v.as_str()) {
                match level {
                    "debug" | "info" | "warning" | "error" => cfg.log_level = level.to_string(),
                    _ => {
                        return Err(ExtensionError::InvalidArguments(format!(
                            "invalid log_level '{level}' (expected debug|info|warning|error)"
                        )))
                    }
                }
            }
            if let Some(dir) = config.get("models_dir").and_then(|v| v.as_str()) {
                if dir.is_empty() {
                    return Err(ExtensionError::InvalidArguments(
                        "models_dir must not be empty".into(),
                    ));
                }
                cfg.models_dir = dir.to_string();
            }
            if let Some(m) = config.get("max_streams").and_then(|v| v.as_u64()) {
                if !(1..=64).contains(&m) {
                    return Err(ExtensionError::InvalidArguments(format!(
                        "max_streams out of range (1..=64): {m}"
                    )));
                }
                cfg.max_streams = m as u32;
            }
            if let Some(addr) = config.get("snapshot_bind_addr").and_then(|v| v.as_str()) {
                if addr.is_empty() {
                    return Err(ExtensionError::InvalidArguments(
                        "snapshot_bind_addr must not be empty".into(),
                    ));
                }
                cfg.snapshot_bind_addr = addr.to_string();
            }
        }
        // server_host is NOT in HandshakeConfig — the sidecar doesn't need it.
        // It lives in its own RwLock and is surfaced to the frontend via
        // list_streams / get_stream_info so cards can build RTSP/snapshot URLs.
        if let Some(host) = config.get("server_host").and_then(|v| v.as_str()) {
            *self.server_host.write() = host.trim().to_string();
        }
        // Sidecar transport mode + remote bridge location. These do NOT
        // belong in HandshakeConfig — they shape which supervisor constructor
        // runs, not what the sidecar process receives. A mode flip takes
        // effect on the next `ensure_sidecar()` (or `restart_sidecar()`),
        // never on a live sidecar. When the mode changes, tear down any
        // existing supervisor that may be crash-looping in the old mode —
        // otherwise it exhausts the 5-restart/60s rate-limit budget before
        // the next ensure_sidecar() can spawn in the correct mode.
        if let Some(mode) = config.get("sidecar_mode").and_then(|v| v.as_str()) {
            let mode = mode.trim().to_string();
            match mode.as_str() {
                "local" | "remote" => {}
                other => {
                    return Err(ExtensionError::InvalidArguments(format!(
                        "invalid sidecar_mode '{other}' (expected local|remote)"
                    )))
                }
            }
            let old_mode = self.sidecar_mode.read().clone();
            if old_mode != mode {
                eprintln!(
                    "[deepstream] sidecar_mode changing '{old_mode}' → '{mode}': tearing down supervisor to prevent wrong-mode crash-loop"
                );
                let old_sup = self.supervisor.write().take();
                if let Some(sup) = old_sup {
                    if let Err(e) = sup.shutdown().await {
                        eprintln!("[deepstream] configure: shutdown error: {e:?}");
                    }
                }
                if let Some(task) = self.watch_task.lock().take() {
                    task.abort();
                }
                *self.sidecar.write() = None;
                self.metrics.mark_sidecar_stopped();
            }
            *self.sidecar_mode.write() = mode;
        }
        if let Some(host) = config.get("sidecar_host").and_then(|v| v.as_str()) {
            *self.sidecar_host.write() = host.trim().to_string();
        }
        if let Some(port) = config.get("sidecar_port").and_then(|v| v.as_u64()) {
            if !(1..=65535).contains(&port) {
                return Err(ExtensionError::InvalidArguments(format!(
                    "sidecar_port out of range: {port}"
                )));
            }
            *self.sidecar_port.write() = port as u16;
        }
        let cfg = self.sidecar_config.read();
        eprintln!("[deepstream] configured: rtsp_port={}, snapshot_port={}, log_level={}, max_streams={}, models_dir={}, server_host='{}', sidecar_mode={}, sidecar_host='{}', sidecar_port={}",
            cfg.rtsp_port, cfg.snapshot_port, cfg.log_level, cfg.max_streams, cfg.models_dir,
            self.server_host.read(),
            self.sidecar_mode.read(),
            self.sidecar_host.read(),
            self.sidecar_port.read());
        Ok(())
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
        Ok(self.metrics.produce_values())
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
    fn metadata_declares_config_parameters_with_defaults() {
        // The README documents these; without with_config_parameters() the
        // host has no way to surface them in the UI and no defaults to feed
        // configure() with. Regressions that drop the call silently make the
        // config section fiction again.
        let ext = DeepStreamExtension::new();
        let meta = ext.metadata();
        let params = meta
            .config_parameters
            .as_ref()
            .expect("config_parameters declared");
        let by_name: std::collections::HashMap<&str, &ParameterDefinition> = params
            .iter()
            .map(|p| (p.name.as_str(), p))
            .collect();
        assert_eq!(params.len(), 10, "params: {:?}", params.iter().map(|p| &p.name).collect::<Vec<_>>());
        for required in [
            "rtsp_port",
            "snapshot_port",
            "log_level",
            "models_dir",
            "max_streams",
            "snapshot_bind_addr",
            "server_host",
            "sidecar_mode",
            "sidecar_host",
            "sidecar_port",
        ] {
            assert!(by_name.contains_key(required), "missing config param {required}");
        }
        // Spot-check defaults match HandshakeConfig::default().
        let defaults = HandshakeConfig::default();
        let rtsp = &by_name["rtsp_port"];
        match &rtsp.default_value {
            Some(ParamMetricValue::Integer(v)) => assert_eq!(*v, defaults.rtsp_port as i64),
            other => panic!("rtsp_port default wrong shape: {other:?}"),
        }
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
    async fn configure_overrides_sidecar_config_fields() {
        // configure() is the only way the host can push UI-entered values
        // into the HandshakeConfig that will be sent on the next spawn.
        // A regression that no-ops this method makes the README config table
        // fiction again.
        use neomind_extension_sdk::Extension;
        let mut ext = DeepStreamExtension::new();
        ext.configure(&serde_json::json!({
            "rtsp_port": 9554,
            "snapshot_port": 9555,
            "log_level": "debug",
            "models_dir": "/data/models",
            "max_streams": 16,
            "snapshot_bind_addr": "127.0.0.1",
            "server_host": "192.168.93.20",
            "sidecar_mode": "remote",
            "sidecar_host": "192.168.93.20",
            "sidecar_port": 9666,
        }))
        .await
        .expect("configure ok");
        let cfg = ext.sidecar_config.read();
        assert_eq!(cfg.rtsp_port, 9554);
        assert_eq!(cfg.snapshot_port, 9555);
        assert_eq!(cfg.log_level, "debug");
        assert_eq!(cfg.models_dir, "/data/models");
        assert_eq!(cfg.max_streams, 16);
        assert_eq!(cfg.snapshot_bind_addr, "127.0.0.1");
        assert_eq!(ext.server_host.read().as_str(), "192.168.93.20");
        // Sidecar transport fields — these shape which supervisor constructor
        // runs on the next ensure_sidecar(), not anything in HandshakeConfig.
        assert_eq!(ext.sidecar_mode.read().as_str(), "remote");
        assert_eq!(ext.sidecar_host.read().as_str(), "192.168.93.20");
        assert_eq!(*ext.sidecar_port.read(), 9666);
    }

    #[tokio::test]
    async fn configure_rejects_invalid_sidecar_mode() {
        use neomind_extension_sdk::Extension;
        let mut ext = DeepStreamExtension::new();
        ext.configure(&serde_json::json!({ "sidecar_mode": "telepathy" }))
            .await
            .expect_err("sidecar_mode='telepathy' should be rejected");
        // Default unchanged on rejection.
        assert_eq!(ext.sidecar_mode.read().as_str(), "local");
    }

    #[tokio::test]
    async fn configure_rejects_out_of_range_sidecar_port() {
        use neomind_extension_sdk::Extension;
        let mut ext = DeepStreamExtension::new();
        ext.configure(&serde_json::json!({ "sidecar_port": 99999 }))
            .await
            .expect_err("sidecar_port=99999 should be rejected");
        // Default unchanged on rejection.
        assert_eq!(*ext.sidecar_port.read(), DEFAULT_SIDECAR_BRIDGE_PORT);
    }

    #[tokio::test]
    async fn configure_rejects_out_of_range_max_streams() {
        use neomind_extension_sdk::Extension;
        let mut ext = DeepStreamExtension::new();
        let err = ext
            .configure(&serde_json::json!({ "max_streams": 999 }))
            .await
            .expect_err("max_streams=999 should be rejected");
        // Sanity: default unchanged on rejection.
        assert_eq!(ext.sidecar_config.read().max_streams, DEFAULT_MAX_STREAMS);
        let _ = err; // error type shape varies by SDK version; just assert it errored.
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
        // No sidecar running → no preset models; only the registered one.
        assert_eq!(models.len(), 1, "models: {models:?}");
        // registered present
        assert!(
            models
                .iter()
                .any(|m| m.get("id").and_then(|v| v.as_str()) == Some("test-yolov8s")
                    && m.get("preset").and_then(|v| v.as_bool()) == Some(false)),
            "registered missing: {models:?}"
        );
    }

    #[test]
    fn produce_metrics_on_fresh_extension_returns_7_globals() {
        let ext = DeepStreamExtension::new();
        let vals = ext.produce_metrics().expect("produce_metrics ok");
        assert_eq!(vals.len(), 7, "expected 7 globals on fresh extension, got {vals:?}");
        let names: Vec<&str> = vals.iter().map(|v| v.name.as_str()).collect();
        let expected = [
            "active_stream_count",
            "total_throughput_fps",
            "gpu_utilization_percent",
            "gpu_memory_used_mb",
            "sidecar_status",
            "sidecar_uptime_secs",
            "restart_count",
        ];
        for e in expected {
            assert!(names.contains(&e), "missing global {e} in {names:?}");
        }
    }

    #[test]
    fn metrics_descriptors_on_fresh_extension_returns_7_globals() {
        // Fresh extension has no streams registered → dynamic.descriptors() is
        // empty; we should still advertise the 7 static globals so the host
        // knows the metric series exists.
        let ext = DeepStreamExtension::new();
        let d = ext.metrics();
        assert_eq!(d.len(), 7, "descriptors: {:?}", d.iter().map(|m| &m.name).collect::<Vec<_>>());
    }
}
