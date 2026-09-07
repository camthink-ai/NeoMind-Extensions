// lib.rs — gym-tracker P1 integration milestone.
//
// Wires the SDK `Extension` trait to the module stack built in Tasks 1-9:
//   - `configure(&mut self, &Value)` builds the `Inner` (db + ne503 client +
//     best-effort login for the WS `?token=` + live-state mirror + metrics
//     synced to the zone set + ingest thread spawn). Replacing a previous
//     `Inner` drops the old `IngestHandle`, whose `Drop` stops the prior ingest
//     thread — so reconfigure never orphans a WS connection / runtime.
//   - `execute_command` dispatches to `commands::handle` with a `Ctx` built
//     from the current `Inner`; maps the `Err(String)` to `ExtensionError::Other`.
//   - `produce_metrics` (SYNC, matching the SDK trait) emits the static
//     aggregates (`present_count`, `visits_today=0` in P1) + per-zone occupancy
//     via `Metrics::produce`, all stamped at millisecond resolution.
//   - `stop` takes the `Inner` out → `IngestHandle::Drop` tears the ingest down.
//
// Trait shape (verified against modbus-bridge + SDK host.rs): SYNC
// `produce_metrics`, `async fn configure(&mut self, &Value)`, required
// `as_any`, `stop`, `OnceLock` metadata, `#[async_trait]`, `Default`,
// `neomind_export!`. No `initialize`/`shutdown`/`async produce_metrics`.

pub mod analytics;
pub mod commands;
pub mod config;
pub mod db;
pub mod exercise;
pub mod frame;
pub mod geo;
mod identity;
mod ingest;
pub mod metrics;
pub mod ne503;
pub mod state;
pub mod tls;
pub mod types;

use std::sync::{Arc, OnceLock};

use neomind_extension_sdk::prelude::{
    FlowControl, StreamCapability, StreamDataType, StreamDirection, StreamMode, StreamSession,
};
use neomind_extension_sdk::{
    async_trait, send_push_output, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, MetricDataType, MetricDescriptor, MetricValue, ParameterDefinition,
    PushOutputMessage, Result,
};
use parking_lot::{Mutex, RwLock};

use analytics::Analytics;
use commands::Ctx;
use config::Config;
use db::Db;
use metrics::Metrics;
use ne503::Ne503Client;
use state::LiveState;

pub struct GymTrackerExtension {
    inner: RwLock<Option<Inner>>,
}

struct Inner {
    state: Arc<LiveState>,
    db: Arc<Db>,
    metrics: Arc<Metrics>,
    analytics: Arc<Analytics>,
    ne: Arc<Ne503Client>,
    #[allow(dead_code)]
    cfg: Config,
    /// Held so its `Drop` (which calls `IngestHandle::stop`) runs on
    /// reconfigure / shutdown, stopping the ingest thread cleanly.
    #[allow(dead_code)]
    ingest: Option<ingest::IngestHandle>,
}

impl GymTrackerExtension {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl Default for GymTrackerExtension {
    fn default() -> Self {
        Self::new()
    }
}

fn cmd(name: &str, desc: &str) -> ExtensionCommand {
    // CommandDescriptor::new / with_description take `impl Into<String>`, and
    // `&str: Into<String>` holds, so we pass the slices directly (avoiding the
    // ambiguous `.into()` that E0283 flags on `impl Into<String>` bounds).
    ExtensionCommand::new(name).with_description(desc)
}

/// Unix epoch milliseconds for `PushOutputMessage::timestamp`.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Live push sessions (session_id → stop flag) for the frame stream.
static PUSH_SESSIONS: OnceLock<
    Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
> = OnceLock::new();
fn push_sessions(
) -> &'static Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>> {
    PUSH_SESSIONS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

#[async_trait]
impl Extension for GymTrackerExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("gym-tracker", "Gym Tracker", env!("CARGO_PKG_VERSION"))
                .with_description("Smart-gym analytics on the NeoEyes NE503")
                .with_author("NeoMind Team")
                // Extension-level settings (rendered by the host's extension
                // config UI). `ui.*` keys are consumed by the frontend: the
                // widgets read ui.language as the GLOBAL default and apply
                // it to every card unless a card overrides lang itself.
                .with_config_parameters(vec![
                    ParameterDefinition {
                        name: "ui.language".into(),
                        display_name: "界面语言 / UI Language".into(),
                        description: "所有 Gym Tracker 卡片的默认语言（卡片级 lang 配置可覆盖）"
                            .into(),
                        param_type: MetricDataType::Enum {
                            options: vec!["en".into(), "zh".into()],
                        },
                        required: false,
                        default_value: Some(MetricValue::from("en")),
                        ..Default::default()
                    },
                    ParameterDefinition {
                        name: "ui.mosaicDefault".into(),
                        display_name: "默认开启人脸打码".into(),
                        description: "Monitor 卡片人脸隐私打码的默认开关".into(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: Some(MetricValue::from(true)),
                        ..Default::default()
                    },
                ])
        })
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            cmd("get_frame", "Latest producer frame (JPEG preview) + its tracks — single-source Monitor rendering"),
            cmd("get_live_state", "Current in-gym tracks (incl. trails)"),
            cmd("get_roi_zones", "List ROI equipment zones"),
            cmd("set_roi_zones", "Replace ROI zones (full set)"),
            cmd("resolve_alert", "Acknowledge/dismiss a safety alert"),
            cmd("get_device_status", "NE503 device status"),
            cmd("get_snapshot", "Capture one frame (base64 JPEG)"),
            cmd("get_lines", "List crossing lines"),
            cmd("set_lines", "Replace crossing lines (full set)"),
            cmd("get_crossings", "Per-line in/out counters (today)"),
            cmd("get_crossing_history", "Daily door in/out totals (persisted history)"),
            cmd("get_heatmap", "Foot-position heatmap grid (today)"),
            cmd("get_workout_summary", "Sessions + equipment usage + reps (today or per member)"),
            cmd("get_member_workout_detail", "Per-member exercises/sets/reps/zones/sessions over N days"),
            cmd("get_activity_window", "Trails + heatmap over an arbitrary time window (24h log)"),
            cmd("register_member", "Register a live track's embedding as a named member"),
            cmd("list_members", "List registered members"),
            cmd("rename_member", "Fill in / correct a member's name"),
            cmd("set_member_photo", "Set/clear a member's avatar photo (base64 JPEG)"),
            cmd("merge_members", "Merge one member's samples into another (outfit-change confirmation)"),
            cmd("delete_member", "Delete a member by id"),
        ]
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        match &*self.inner.read() {
            Some(i) => i.metrics.descriptors(),
            None => Metrics::static_descriptors(),
        }
    }

    // SYNC — matches the SDK trait (NOT async). The host polls this off the
    // metric thread; locking the inner RwLock for a read is cheap.
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        match &*self.inner.read() {
            Some(i) => {
                // Evict departed tracks before reading present_count, matching
                // get_live_state's TTL wiring — otherwise the gauge drifts up as
                // track_ids that stopped being republished never leave the mirror.
                let _ = i.state.evict_expired();
                let tracks = i.state.snapshot();
                // Drive the per-zone `gym.equipment_occupied` gauges from real
                // foot positions: a zone is occupied iff a live track's foot is
                // inside its polygon (ray-cast). Recomputed every metric tick
                // from the current mirror + the persisted zone set, so the
                // gauges reflect who's actually on each piece of equipment.
                if let Ok(zones) = i.db.list_zones() {
                    i.metrics.apply_occupation(&tracks, &zones);
                }
                Ok(i.metrics.produce(
                    tracks.len() as i64,
                    0, // visits_today = 0 in P1 (sessions land in P2)
                    chrono::Utc::now().timestamp_millis(),
                ))
            }
            None => Ok(vec![]),
        }
    }

    async fn execute_command(
        &self,
        cmd: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Scope the read guard to the Arc clone-out only: a command like
        // `get_device_status` does a BLOCKING ureq call (up to 5s). Holding the
        // guard across it would block `configure`/`stop` (which need a write
        // lock) for that whole window. The cloned Arcs keep the handles alive
        // after the guard is dropped.
        let ctx = {
            let g = self.inner.read();
            let inner = g
                .as_ref()
                .ok_or_else(|| ExtensionError::Other("not configured".into()))?;
            Ctx {
                state: inner.state.clone(),
                db: inner.db.clone(),
                ne: inner.ne.clone(),
                metrics: inner.metrics.clone(),
                analytics: inner.analytics.clone(),
                identity: inner.cfg.identity.clone(),
            }
        };
        commands::handle(&ctx, cmd, args).map_err(ExtensionError::Other)
    }

    // `configure(&mut self, &Value)` — NOT `initialize(&self, &str)`. Called by
    // the host on load and on any reconfigure. Idempotent + reconfigure-safe:
    // the previous `Inner` (and thus its `IngestHandle`) is dropped when we
    // overwrite `inner`, stopping the prior ingest thread before the new one
    // starts.
    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        let cfg: Config = serde_json::from_value(config.clone())
            .map_err(|e| ExtensionError::Other(format!("config: {e}")))?;

        // The data dir may not exist yet (fresh install / moved data_dir) —
        // create it so Db::open doesn't strand the extension in the inert
        // cold-load state on a hot-reload.
        std::fs::create_dir_all(&cfg.data_dir).ok();
        let db = Arc::new(
            Db::open(&format!(
                "{}/gym-tracker.db",
                cfg.data_dir.trim_end_matches('/')
            ))
            .map_err(|e| ExtensionError::Other(format!("db: {e}")))?,
        );

        // Cold-load path: the host's Init handshake sends `{}` before any real
        // config exists. Stay inert (no login, no ingest thread) until a
        // ConfigUpdate supplies a device host; configure() runs again then.
        if !cfg.provisioned() {
            tracing::info!("gym-tracker: no device.host configured, idling until ConfigUpdate");
            let analytics = Arc::new(Analytics::with_foot_retention(
                &db,
                (cfg.roi.foot_retain_days.max(1) as i64) * 86400,
            ));
            *self.inner.write() = Some(Inner {
                state: Arc::new(LiveState::new(cfg.ingest.track_ttl_sec)),
                db,
                metrics: Arc::new(Metrics::new()),
                analytics,
                ne: Arc::new(Ne503Client::new(&cfg)),
                cfg,
                ingest: None,
            });
            return Ok(());
        }

        let ne = Arc::new(Ne503Client::new(&cfg));

        // Best-effort warm-up login; the ingest loop re-logins on its own
        // whenever its cached token is empty or a reconnect is needed, so a
        // failure here (device busy at configure time) no longer starves the
        // subscriber.
        let _ = ne.login();

        let state = Arc::new(LiveState::new(cfg.ingest.track_ttl_sec));
        let metrics = Arc::new(Metrics::new());
        let analytics = Arc::new(Analytics::with_foot_retention(
            &db,
            (cfg.roi.foot_retain_days.max(1) as i64) * 86400,
        ));
        // Seed the per-zone metric registry from the persisted zone set so the
        // first produce_metrics() advertises the right descriptors.
        metrics.sync_zones(&db.list_zones().unwrap_or_default());

        let ingest_handle = ingest::spawn(
            cfg.clone(),
            state.clone(),
            analytics.clone(),
            db.clone(),
            ne.clone(),
        );

        // Replacing a previous Inner drops the old IngestHandle → its Drop stops
        // the old ingest thread. Single write-lock acquisition.
        *self.inner.write() = Some(Inner {
            state,
            db,
            metrics,
            analytics,
            ne,
            cfg,
            ingest: Some(ingest_handle),
        });

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self // required by the trait, no default
    }

    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability {
            direction: StreamDirection::Download,
            mode: StreamMode::Push,
            supported_data_types: vec![StreamDataType::Json],
            max_chunk_size: 1 << 20,
            preferred_chunk_size: 48 * 1024,
            max_concurrent_sessions: 4,
            flow_control: FlowControl::default_stream(),
            config_schema: None,
        })
    }

    async fn init_session(&self, _session: &StreamSession) -> Result<()> {
        Ok(()) // stateless push of the shared latest-frame cache
    }

    /// Push mode: stream the cached device frames (preview + tracks of that
    /// frame) to a dashboard WS client. WS messages are the one channel that
    /// browser timer-throttling cannot slow down — the Monitor gets full
    /// frame rate even in occluded/energy-saving tabs where polling would
    /// clamp to ~1 Hz.
    async fn start_push(&self, session_id: &str) -> Result<()> {
        let state = self.inner.read().as_ref().map(|i| i.state.clone());
        let Some(state) = state else {
            return Err(ExtensionError::ExecutionFailed(
                "start_push: extension not configured".into(),
            ));
        };
        let flag = {
            let mut g = push_sessions().lock();
            let f = g
                .entry(session_id.to_string())
                .or_insert_with(|| Arc::new(std::sync::atomic::AtomicBool::new(true)));
            f.store(true, std::sync::atomic::Ordering::SeqCst);
            f.clone()
        };
        let sid = session_id.to_string();
        std::thread::spawn(move || {
            let mut seq: u64 = 0;
            // per-session cursor into the SHARED h264 queue: every session
            // replays the full stream (see LiveState::h264_since)
            let mut h264_cursor: u64 = 0;
            let mut last_jpeg_ts: Option<u64> = None;
            while flag.load(std::sync::atomic::Ordering::SeqCst) {
                // Wake on EITHER stream (shared condvar); the timeout is the
                // idle heartbeat. Do NOT gate on the return value: with the
                // JPEG fallback suppressed (H.264 healthy) the JPEG cache is
                // legitimately None forever, and treating that as "nothing
                // to do" deadlocked the push loop (0 frames, session alive).
                let _ = state.wait_preview(std::time::Duration::from_millis(120));
                // evict departed tracks BEFORE snapshotting — otherwise a
                // person who left lingers in every pushed frame until the
                // (much slower) REST poll happens to evict, and the Monitor
                // draws a frozen box for seconds after they're gone
                let _ = state.evict_expired();

                // ---- H.264 relay (primary video, zero-CPU on device) ----
                // PER-SESSION replay from the shared queue — H.264 is
                // differential; every session must see the FULL stream
                // (drain semantics split it across concurrent sessions).
                for s in state.h264_since(h264_cursor) {
                    h264_cursor = s.seq;
                    state::push_diag().pushed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let tracks = state.snapshot_tracks();
                        let meta = serde_json::json!({
                            "ts_ns": s.ts_ns,
                            "pts_ns": s.pts_ns,
                            "key": s.key,
                            // relay-side sequence (0 when producer omits it) —
                            // lets the Monitor correlate end-to-end loss
                            "h264_seq": s.seq,
                            "w": s.w,
                            "h": s.h,
                            // track bundle rides at the relay rate so the
                            // Monitor's history keeps its cadence even with
                            // the JPEG fallback slowed to ~2 Hz
                            "tracks": tracks,
                            "tracks_ts": state.snapshot_tracks_ts(),
                            "faces": state.snapshot_faces().unwrap_or_default(),
                            "present_count": tracks.len(),
                        });
                        seq += 1;
                        let m = PushOutputMessage {
                            session_id: sid.clone(),
                            sequence: seq,
                            data: frame::build_frame_payload(&meta, &s.nalu),
                            data_type: frame::AVC_DATA_TYPE.to_string(),
                            timestamp: now_ms(),
                            metadata: None,
                        };
                    if send_push_output(&m).is_err() {
                        break; // channel gone — session ended
                    }
                }

                // ---- JPEG fallback (compat; low rate) ----
                if let Some((jts, jimg)) = state.snapshot_preview() {
                    if last_jpeg_ts != Some(jts) {
                        last_jpeg_ts = Some(jts);
                        let faces = state.snapshot_faces().unwrap_or_default();
                        let tracks = state.snapshot_tracks();
                        let tracks_ts = state.snapshot_tracks_ts();
                        // Binary container `[u32 meta_len BE][meta JSON][JPEG bytes]`
                        // (data_type `application/x-neomind-frame`). On binary-
                        // negotiated sessions the JPEG reaches the browser as raw
                        // bytes — no base64 on either leg. Legacy Text sessions get
                        // this same payload base64-wrapped by the platform; the
                        // Monitor's parser handles both shapes.
                        let meta = serde_json::json!({
                            // latest tracks at the device clock — the client
                            // builds its own ts-keyed history from the stream
                            // (sending the full hist per frame doubled the
                            // parse cost and stalled the browser at 23 fps)
                            "tracks": tracks,
                            "ts_ns": jts,
                            // TRUE capture time of those track positions — the
                            // client keys its overlay history by this, not by
                            // the preview ts (which is one inference-latency ahead)
                            "tracks_ts": tracks_ts,
                            "faces": faces,
                            "present_count": tracks.len(),
                        });
                        seq += 1;
                        let m = PushOutputMessage {
                            session_id: sid.clone(),
                            sequence: seq,
                            data: frame::build_frame_payload(&meta, &jimg),
                            data_type: frame::FRAME_DATA_TYPE.to_string(),
                            timestamp: now_ms(),
                            metadata: None,
                        };
                        if send_push_output(&m).is_err() {
                            break; // channel gone — session ended
                        }
                    }
                }
            }
        });
        Ok(())
    }

    async fn stop_push(&self, session_id: &str) -> Result<()> {
        if let Some(f) = push_sessions().lock().get(session_id) {
            f.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        // Take the Inner out of the RwLock (replaces with None) and drop it
        // immediately — dropping Inner drops IngestHandle, whose Drop stops the
        // ingest thread. Stops within ~200ms (ingest read-loop select ticker).
        let _ = self.inner.write().take();
        Ok(())
    }
}

neomind_extension_sdk::neomind_export!(GymTrackerExtension);
