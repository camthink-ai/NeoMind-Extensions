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
pub mod geo;
pub mod ingest;
pub mod metrics;
pub mod ne503;
pub mod state;
pub mod tls;
pub mod types;

use std::sync::{Arc, OnceLock};

use neomind_extension_sdk::{
    async_trait, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, MetricDescriptor, Result,
};
use parking_lot::RwLock;

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

#[async_trait]
impl Extension for GymTrackerExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("gym-tracker", "Gym Tracker", env!("CARGO_PKG_VERSION"))
                .with_description("Smart-gym analytics on the NeoEyes NE503")
                .with_author("NeoMind Team")
        })
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            cmd("get_live_state", "Current in-gym tracks (incl. trails)"),
            cmd("get_roi_zones", "List ROI equipment zones"),
            cmd("set_roi_zones", "Replace ROI zones (full set)"),
            cmd("get_device_status", "NE503 device status"),
            cmd("get_snapshot", "Capture one frame (base64 JPEG)"),
            cmd("get_lines", "List crossing lines"),
            cmd("set_lines", "Replace crossing lines (full set)"),
            cmd("get_crossings", "Per-line in/out counters (today)"),
            cmd("get_heatmap", "Foot-position heatmap grid (today)"),
            cmd("register_member", "Register a live track's embedding as a named member"),
            cmd("list_members", "List registered members"),
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
            let analytics = Arc::new(Analytics::new(&db));
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

        // Best-effort login so the ingest WS `?token=` query param has a value.
        // If it fails here (device unreachable at configure time), ingest still
        // spawns and reconnects on its own schedule, but without a token — log
        // and continue rather than failing the whole configure.
        let token = ne
            .login()
            .ok()
            .and_then(|()| ne.token_string())
            .unwrap_or_default();

        let state = Arc::new(LiveState::new(cfg.ingest.track_ttl_sec));
        let metrics = Arc::new(Metrics::new());
        let analytics = Arc::new(Analytics::new(&db));
        // Seed the per-zone metric registry from the persisted zone set so the
        // first produce_metrics() advertises the right descriptors.
        metrics.sync_zones(&db.list_zones().unwrap_or_default());

        let ingest_handle =
            ingest::spawn(cfg.clone(), state.clone(), analytics.clone(), token);

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

    fn stop(&mut self) -> Result<()> {
        // Take the Inner out of the RwLock (replaces with None) and drop it
        // immediately — dropping Inner drops IngestHandle, whose Drop stops the
        // ingest thread. Stops within ~200ms (ingest read-loop select ticker).
        let _ = self.inner.write().take();
        Ok(())
    }
}

neomind_extension_sdk::neomind_export!(GymTrackerExtension);
