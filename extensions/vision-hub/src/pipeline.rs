//! Pipeline engine — the unified replacement for the three per-extension
//! DeviceBinding frameworks. A pipeline binds an image source to a task
//! list and a sink set; the engine reacts to `DeviceMetric` frames, runs
//! the tasks, and fans results out to virtual metrics, EventBus events,
//! capture state, and an in-memory snapshot for UI polling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use vision_common::types::{ImageSource, VisionResult};

use crate::task::{TaskRegistry, TaskSpec};

/// When a pipeline runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Schedule {
    /// Run on every matching frame (with an optional fps cap and
    /// per-pipeline cooldown).
    OnFrame {
        #[serde(default)]
        fps_limit: Option<f32>,
        #[serde(default = "default_cooldown")]
        cooldown_secs: u64,
    },
    /// Never run automatically; `analyze`-driven only.
    Manual,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule::OnFrame {
            fps_limit: None,
            cooldown_secs: default_cooldown(),
        }
    }
}

fn default_cooldown() -> u64 {
    2
}

/// Capture rule: snapshot when a condition holds, with cooldown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRule {
    /// "threshold" (≥ N detections) | "presence" (any) | "absence" (none)
    #[serde(default = "default_capt_kind")]
    pub kind: String,
    #[serde(default)]
    pub count: usize,
    /// Restrict the count to these labels.
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
}

fn default_capt_kind() -> String {
    "presence".into()
}

impl CaptureRule {
    /// Does this result trigger a capture?
    pub fn triggers(&self, result: &VisionResult) -> bool {
        let count = if self.labels.is_empty() {
            result.detections.len()
        } else {
            result
                .detections
                .iter()
                .filter(|d| self.labels.contains(&d.label))
                .count()
        };
        match self.kind.as_str() {
            "threshold" => count >= self.count.max(1),
            "absence" => count == 0,
            _ => count > 0,
        }
    }
}

/// Where results go.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sinks {
    /// Write `virtual.vision.<pipeline>.<field>` metrics to the bound device.
    #[serde(default = "default_true")]
    pub virtual_metrics: bool,
    /// Publish `vision.result` events on the EventBus.
    #[serde(default = "default_true")]
    pub event: bool,
    /// Smart-capture frames per the rule.
    #[serde(default)]
    pub capture: Option<CaptureRule>,
    /// Keep annotated snapshot for UI polling.
    #[serde(default = "default_true")]
    pub snapshot: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Sinks {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({})).unwrap()
    }
}

/// A configured pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub source: ImageSource,
    pub tasks: Vec<TaskSpec>,
    #[serde(default)]
    pub sinks: Sinks,
    #[serde(default)]
    pub schedule: Schedule,
    /// Draw boxes on the snapshot / annotated output.
    #[serde(default = "default_true")]
    pub draw: bool,
}

impl PipelineConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("pipeline id must not be empty".into());
        }
        if self.tasks.is_empty() {
            return Err(format!("pipeline '{}' has no tasks", self.id));
        }
        if !matches!(self.source, ImageSource::Device { .. }) {
            return Err(format!(
                "pipeline '{}' source must be {{type:'device'}} in this build — \
                 stream/url sources ship with the source module batch",
                self.id
            ));
        }
        Ok(())
    }
}

/// Runtime state of one pipeline (last run, cooldown, capture).
#[derive(Debug, Clone, Serialize)]
pub struct PipelineState {
    /// Attempt stamp (set when a run STARTS, success or not — failures
    /// must be cooled down too, or a missing model turns into a per-frame
    /// hot loop).
    pub last_run_ms: Option<u64>,
    /// In-flight guard: concurrent DeviceMetric events for the same
    /// pipeline must not double-run while an inference is in progress.
    #[serde(skip)]
    pub running: bool,
    pub last_error: Option<String>,
    pub total_frames: u64,
    pub total_detections: u64,
    pub last_capture_ms: Option<u64>,
    /// Latest annotated frame (data URL) for UI polling.
    pub last_snapshot: Option<String>,
    pub last_result: Option<serde_json::Value>,
}

impl Default for PipelineState {
    fn default() -> Self {
        Self {
            last_run_ms: None,
            running: false,
            last_error: None,
            total_frames: 0,
            total_detections: 0,
            last_capture_ms: None,
            last_snapshot: None,
            last_result: None,
        }
    }
}

/// The engine: configs + runtime states + registry, persisted to config.json.
pub struct PipelineEngine {
    pub configs: Mutex<Vec<PipelineConfig>>,
    pub states: Mutex<HashMap<String, PipelineState>>,
    pub registry: Arc<TaskRegistry>,
    pub frames_processed: AtomicU64,
    pub total_inferences: AtomicU64,
    pub total_detections: AtomicU64,
    /// Cumulative inference milliseconds (for a REAL avg_inference_ms —
    /// the old metric grabbed an arbitrary pipeline's latest value).
    pub total_inference_ms: AtomicU64,
    config_path: Mutex<Option<std::path::PathBuf>>,
}

impl PipelineEngine {
    pub fn new(registry: Arc<TaskRegistry>) -> Self {
        Self {
            configs: Mutex::new(Vec::new()),
            states: Mutex::new(HashMap::new()),
            registry,
            frames_processed: AtomicU64::new(0),
            total_inferences: AtomicU64::new(0),
            total_detections: AtomicU64::new(0),
            total_inference_ms: AtomicU64::new(0),
            config_path: Mutex::new(None),
        }
    }

    // -- persistence ------------------------------------------------------

    fn default_config_path() -> std::path::PathBuf {
        // Platform-guaranteed data dir (survives upgrades AND uninstall).
        if let Ok(dir) = std::env::var("NEOMIND_EXTENSION_DATA_DIR") {
            let new_path = std::path::PathBuf::from(dir).join("config.json");
            if new_path.exists() {
                return new_path;
            }
            // One-shot migration from the legacy extension-root location.
            if let Ok(old_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
                let old_path = std::path::PathBuf::from(old_dir).join("config.json");
                if old_path.exists() {
                    if let Ok(content) = std::fs::read(&old_path) {
                        if std::fs::write(&new_path, content).is_ok() {
                            tracing::info!(
                                "[vision-hub] migrated config.json into the preserved data dir"
                            );
                            let _ = std::fs::remove_file(&old_path);
                            return new_path;
                        }
                    }
                }
            }
            return new_path;
        }
        // Older platforms don't set the data dir — keep the legacy path.
        std::env::var("NEOMIND_EXTENSION_DIR")
            .map(|d| std::path::PathBuf::from(d).join("config.json"))
            .unwrap_or_else(|_| std::path::PathBuf::from("config.json"))
    }

    pub fn load_config(&self) {
        let path = Self::default_config_path();
        *self.config_path.lock() = Some(path.clone());
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        match serde_json::from_str::<Vec<PipelineConfig>>(&raw) {
            Ok(configs) => {
                let valid: Vec<PipelineConfig> =
                    configs.into_iter().filter(|c| c.validate().is_ok()).collect();
                tracing::info!(
                    "[vision-hub] loaded {} pipelines from {}",
                    valid.len(),
                    path.display()
                );
                // Drop runtime state (snapshots included) for pipelines that
                // no longer exist in the config — otherwise megabyte-scale
                // last_snapshot blobs leak for every renamed/removed id.
                let ids: std::collections::HashSet<String> =
                    valid.iter().map(|c| c.id.clone()).collect();
                self.states.lock().retain(|id, _| ids.contains(id));
                *self.configs.lock() = valid;
            }
            Err(e) => tracing::warn!("[vision-hub] config.json unreadable: {e}"),
        }
    }

    pub fn save_config(&self) {
        let path = self
            .config_path
            .lock()
            .clone()
            .unwrap_or_else(Self::default_config_path);
        let configs = self.configs.lock().clone();
        match serde_json::to_string_pretty(&configs) {
            Ok(json) => {
                // Atomic replace: a crash mid-write must not leave a
                // truncated JSON that wipes ALL pipelines on next boot.
                let tmp = path.with_extension("json.tmp");
                if let Err(e) = std::fs::write(&tmp, json) {
                    tracing::warn!("[vision-hub] config save failed: {e}");
                    return;
                }
                if let Err(e) = std::fs::rename(&tmp, &path) {
                    tracing::warn!("[vision-hub] config rename failed: {e}");
                }
            }
            Err(e) => tracing::warn!("[vision-hub] config serialize failed: {e}"),
        }
    }

    // -- CRUD (commands) ---------------------------------------------------

    pub fn list(&self, include_snapshots: bool) -> Vec<serde_json::Value> {
        let configs = self.configs.lock();
        let states = self.states.lock();
        configs
            .iter()
            .map(|c| {
                let state = states.get(&c.id).cloned().unwrap_or_default();
                // Snapshots are megabyte-scale data URLs — strip unless the
                // caller explicitly wants them (frontend polls per panel).
                let state = if include_snapshots {
                    state
                } else {
                    let mut s = state;
                    s.last_snapshot = None;
                    s
                };
                serde_json::json!({
                    "config": c,
                    "state": state,
                })
            })
            .collect()
    }

    pub fn upsert(&self, config: PipelineConfig) -> Result<(), String> {
        config.validate()?;
        // Models load lazily on first run — pipeline creation must work
        // even when the model still needs downloading or the runner wants
        // to defer memory cost.
        let mut configs = self.configs.lock();
        match configs.iter_mut().find(|c| c.id == config.id) {
            Some(existing) => *existing = config,
            None => configs.push(config),
        }
        drop(configs);
        self.save_config();
        Ok(())
    }

    pub fn delete(&self, id: &str) -> bool {
        let mut configs = self.configs.lock();
        let before = configs.len();
        configs.retain(|c| c.id != id);
        let removed = configs.len() < before;
        drop(configs);
        self.states.lock().remove(id);
        if removed {
            self.save_config();
        }
        removed
    }

    // -- event handling -----------------------------------------------------

    /// Feed a DeviceMetric event through matching pipelines.
    pub fn handle_device_metric(
        &self,
        device_id: &str,
        metric: &str,
        value: &serde_json::Value,
    ) {
        let configs = self.configs.lock().clone();
        for config in configs.iter().filter(|c| c.enabled) {
            let ImageSource::Device {
                device_id: bound_device,
                metric: bound_metric,
            } = &config.source
            else {
                continue;
            };
            if bound_device != device_id {
                continue;
            }
            // "image" | "image.frame" → top-level match + nested extraction
            let (top, nested) = match bound_metric.split_once('.') {
                Some((t, n)) => (t.to_string(), Some(n.to_string())),
                None => (bound_metric.clone(), None),
            };
            if metric != bound_metric && metric != top {
                continue;
            }
            let nested = if metric == bound_metric { None } else { nested };

            if !self.try_start(config) {
                continue;
            }

            match vision_common::image::extract_image(Some(value), nested.as_deref()) {
                Ok(img) => {
                    // Count only frames that actually reached inference —
                    // the old version counted every event on the bus.
                    self.frames_processed.fetch_add(1, Ordering::SeqCst);
                    let result = self.run_pipeline(config, &img);
                    // Single收口 critical section for terminal state.
                    let mut states = self.states.lock();
                    let state = states.entry(config.id.clone()).or_default();
                    state.running = false;
                    match result {
                        Err(e) => {
                            tracing::warn!(
                                "[vision-hub] pipeline '{}' failed: {e}",
                                config.id
                            );
                            state.last_error = Some(e);
                        }
                        Ok(()) => {
                            state.last_error = None;
                        }
                    }
                }
                Err(e) => {
                    // No image: not an attempt — release the slot without
                    // leaving last_error set.
                    self.finish_run(&config.id);
                    tracing::debug!(
                        "[vision-hub] pipeline '{}': no image in metric ({}): {e}",
                        config.id,
                        metric
                    );
                }
            }
        }
    }

    /// Effective minimum interval between run ATTEMPTS: the configured
    /// cooldown, optionally tightened by fps_limit (declared but previously
    /// never enforced).
    fn effective_interval_ms(schedule: &Schedule) -> Option<u64> {
        let Schedule::OnFrame { cooldown_secs, fps_limit } = schedule else {
            return None; // Manual pipelines don't auto-run
        };
        let cooldown_ms = cooldown_secs * 1000;
        Some(match fps_limit {
            Some(fps) if *fps > 0.0 && fps.is_finite() => {
                let fps_ms = (1000.0 / fps).ceil() as u64;
                cooldown_ms.max(fps_ms)
            }
            _ => cooldown_ms,
        })
    }

    /// Atomically check cooldown AND claim the run slot (stamp + running
    /// flag in one critical section). Checking and stamping separately let
    /// concurrent frames for the same device both pass and double-run.
    fn try_start(&self, config: &PipelineConfig) -> bool {
        let Some(min_interval_ms) = Self::effective_interval_ms(&config.schedule) else {
            return false;
        };
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let mut states = self.states.lock();
        let state = states.entry(config.id.clone()).or_default();
        if state.running {
            return false;
        }
        if let Some(last) = state.last_run_ms {
            if now.saturating_sub(last) < min_interval_ms {
                return false;
            }
        }
        state.last_run_ms = Some(now); // attempt stamp — failures cool down too
        state.running = true;
        true
    }

    fn finish_run(&self, id: &str) {
        let mut states = self.states.lock();
        if let Some(state) = states.get_mut(id) {
            state.running = false;
        }
    }

    /// Run one pipeline's tasks on a decoded image and fan out to sinks.
    pub fn run_pipeline(&self, config: &PipelineConfig, img: &image::DynamicImage) -> Result<(), String> {
        let start = std::time::Instant::now();
        // Guard: callers must have claimed the slot via try_start. A
        // direct call (tests) still works — running is only advisory.
        debug_assert!(self.states.lock().get(&config.id).map(|s| s.running).unwrap_or(false)
            || cfg!(test));
        let mut results = Vec::new();
        for spec in &config.tasks {
            self.registry.get_or_load(spec)?;
            let r = self.registry.run(spec, img)?;
            self.total_detections
                .fetch_add(r.detections.len() as u64, Ordering::SeqCst);
            results.push(r);
        }
        self.total_inferences.fetch_add(1, Ordering::SeqCst);
        let elapsed = start.elapsed().as_millis() as u64;
        self.total_inference_ms.fetch_add(elapsed, Ordering::SeqCst);

        // snapshot (annotated)
        let snapshot = if config.draw && config.sinks.snapshot {
            let dets: Vec<vision_common::Detection> = results
                .iter()
                .flat_map(|r| r.detections.clone())
                .collect();
            let mut rgb = img.to_rgb8();
            vision_common::draw::draw_detections(&mut rgb, &dets);
            match vision_common::image::encode_jpeg(
                &image::DynamicImage::ImageRgb8(rgb),
                85,
            ) {
                Ok(jpeg) => Some(vision_common::image::to_data_url("image/jpeg", &jpeg)),
                Err(e) => {
                    tracing::warn!("[vision-hub] snapshot encode: {e}");
                    None
                }
            }
        } else {
            None
        };

        let result_json: Vec<serde_json::Value> =
            results.iter().map(|r| r.to_tool_json()).collect();
        let total_count: usize = results.iter().map(|r| r.detections.len()).sum();

        // virtual metrics
        if config.sinks.virtual_metrics {
            if let ImageSource::Device { device_id, .. } = &config.source {
                let ts = chrono::Utc::now().timestamp_millis();
                let writes = [
                    ("detections".to_string(), serde_json::json!(total_count)),
                    (
                        "inference_ms".to_string(),
                        serde_json::json!(elapsed),
                    ),
                    // Comma-joined string: platform metric values may not
                    // accept JSON arrays, and a silently-dropped write is
                    // worse than a parseable string.
                    (
                        "labels".to_string(),
                        serde_json::json!(results
                            .iter()
                            .flat_map(|r| r.detections.iter().map(|d| d.label.clone()))
                            .collect::<Vec<_>>()
                            .join(",")),
                    ),
                ];
                for (field, value) in writes {
                    let params = serde_json::json!({
                        "device_id": device_id,
                        "metric": format!("virtual.vision.{}.{}", config.id, field),
                        "value": value,
                        "timestamp": ts,
                    });
                    invoke_capability("device_metrics_write", &params);
                }
            }
        }

        // event
        if config.sinks.event {
            invoke_capability(
                "event_publish",
                &serde_json::json!({
                    "event_type": "vision.result",
                    "payload": {
                        "pipeline": config.id,
                        "source": config.source,
                        "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                        "results": result_json,
                    }
                }),
            );
        }

        // Terminal state update — ONE critical section so concurrent
        // `list_pipelines` polls can never observe a torn intermediate
        // (frames counted but capture stamp missing, etc.). The capture
        // decision reads AND writes last_capture_ms inside this same lock.
        {
            let mut states = self.states.lock();
            let state = states.entry(config.id.clone()).or_default();
            state.total_frames += 1;
            state.total_detections += total_count as u64;
            state.last_result = Some(serde_json::json!({
                "results": result_json,
                "inference_ms": elapsed,
            }));
            if let Some(snap) = snapshot {
                state.last_snapshot = Some(snap);
            }
            if let Some(rule) = config.sinks.capture.as_ref() {
                if results.iter().any(|r| rule.triggers(r))
                    && cooldown_elapsed(state.last_capture_ms, rule.cooldown_secs)
                {
                    state.last_capture_ms = Some(chrono::Utc::now().timestamp_millis() as u64);
                }
            }
        }
        Ok(())
    }
}

fn cooldown_elapsed(last: Option<u64>, secs: u64) -> bool {
    match last {
        Some(l) => chrono::Utc::now().timestamp_millis() as u64 >= l + secs * 1000,
        None => true,
    }
}

/// Synchronous capability invocation (same pattern as yolo-device-inference).
fn invoke_capability(name: &str, params: &serde_json::Value) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use neomind_extension_sdk::capabilities::CapabilityContext;
        tokio::task::block_in_place(|| {
            let ctx = CapabilityContext::default();
            let resp = ctx.invoke_capability(name, params);
            if !resp.get("success").and_then(|s| s.as_bool()).unwrap_or(false) {
                tracing::debug!("[vision-hub] capability {name} failed: {resp}");
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (name, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device_pipeline(id: &str) -> PipelineConfig {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "source": { "type": "device", "device_id": "NE301-1", "metric": "image" },
            "tasks": [{ "type": "detect" }],
        }))
        .unwrap()
    }

    #[test]
    fn pipeline_serde_defaults() {
        let p = device_pipeline("gate");
        assert!(p.enabled);
        assert!(p.sinks.virtual_metrics);
        assert!(p.sinks.event);
        assert!(p.sinks.snapshot);
        assert!(p.draw);
        assert!(matches!(
            p.schedule,
            Schedule::OnFrame { cooldown_secs: 2, .. }
        ));
    }

    #[test]
    fn validate_rejects_stream_source_this_batch() {
        let p: PipelineConfig = serde_json::from_value(serde_json::json!({
            "id": "x",
            "source": { "type": "stream", "url": "rtsp://cam" },
            "tasks": [{ "type": "detect" }],
        }))
        .unwrap();
        assert!(p.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_tasks_and_id() {
        let mut p = device_pipeline("");
        assert!(p.validate().is_err());
        p.id = "ok".into();
        p.tasks.clear();
        assert!(p.validate().is_err());
    }

    #[test]
    fn capture_rules_trigger() {
        let mut r = VisionResult::new("detect", 10, 10);
        let mk = |kind: &str, count: usize, labels: Vec<&str>| CaptureRule {
            kind: kind.into(),
            count,
            labels: labels.into_iter().map(String::from).collect(),
            cooldown_secs: 0,
        };
        assert!(!mk("presence", 0, vec![]).triggers(&r));
        r.detections.push(vision_common::Detection {
            label: "person".into(),
            class_id: Some(0),
            confidence: 0.9,
            bbox: vision_common::BBox::new(0.0, 0.0, 1.0, 1.0),
            attrs: None,
        });
        assert!(mk("presence", 0, vec![]).triggers(&r));
        assert!(mk("threshold", 1, vec![]).triggers(&r));
        assert!(!mk("threshold", 2, vec![]).triggers(&r));
        assert!(mk("threshold", 1, vec!["person"]).triggers(&r));
        assert!(!mk("threshold", 1, vec!["car"]).triggers(&r));
        assert!(!mk("absence", 0, vec![]).triggers(&r));
    }

    #[test]
    fn effective_interval_respects_fps_limit() {
        let s = Schedule::OnFrame { fps_limit: Some(10.0), cooldown_secs: 1 };
        assert_eq!(PipelineEngine::effective_interval_ms(&s), Some(1000)); // 1s cooldown > 100ms fps
        let s = Schedule::OnFrame { fps_limit: Some(100.0), cooldown_secs: 5 };
        assert_eq!(PipelineEngine::effective_interval_ms(&s), Some(5000));
        let s = Schedule::OnFrame { fps_limit: Some(0.0), cooldown_secs: 2 };
        assert_eq!(PipelineEngine::effective_interval_ms(&s), Some(2000)); // fps 0 ignored
        assert_eq!(PipelineEngine::effective_interval_ms(&Schedule::Manual), None);
    }

    #[test]
    fn try_start_claims_slot_and_cooldowns() {
        let reg = crate::task::TaskRegistry::new(std::sync::Arc::new(
            vision_common::accel::HardwareProfile::default(),
        ));
        let engine = PipelineEngine::new(std::sync::Arc::new(reg));
        let config = device_pipeline("claim-test");

        assert!(engine.try_start(&config), "first attempt must claim");
        // Second attempt while running: rejected (concurrent double-run guard)
        assert!(!engine.try_start(&config), "in-flight attempt must be rejected");
        engine.finish_run("claim-test");
        // Immediately after finish: still inside the default 2s cooldown
        assert!(!engine.try_start(&config), "cooldown must apply post-finish");
    }

    #[test]
    fn failure_also_consumes_cooldown() {
        // The attempt stamp is set at START — a failing pipeline must not
        // retry every frame (the old per-frame hot loop).
        let reg = crate::task::TaskRegistry::new(std::sync::Arc::new(
            vision_common::accel::HardwareProfile::default(),
        ));
        let engine = PipelineEngine::new(std::sync::Arc::new(reg));
        let config = device_pipeline("fail-cd");
        assert!(engine.try_start(&config));
        engine.finish_run("fail-cd"); // simulate failure finishing
        assert!(!engine.try_start(&config), "failed run must cool down");
    }

    #[test]
    fn save_config_leaves_valid_json_and_no_tmp() {
        let reg = crate::task::TaskRegistry::new(std::sync::Arc::new(
            vision_common::accel::HardwareProfile::default(),
        ));
        let engine = PipelineEngine::new(std::sync::Arc::new(reg));
        let dir = std::env::temp_dir().join(format!("vh-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        *engine.config_path.lock() = Some(dir.join("config.json"));

        engine.upsert(device_pipeline("atomic-test")).unwrap();
        let raw = std::fs::read_to_string(dir.join("config.json")).unwrap();
        assert!(serde_json::from_str::<Vec<PipelineConfig>>(&raw).is_ok());
        assert!(!dir.join("config.json.tmp").exists(), "tmp file must be renamed away");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_roundtrip_through_json() {
        let p = device_pipeline("gate");
        let j = serde_json::to_string(&p).unwrap();
        let back: PipelineConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id, "gate");
        assert_eq!(back.source, p.source);
    }
}
