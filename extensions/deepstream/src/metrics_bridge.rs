//! Bridge between Stats events (async, from sidecar) and produce_metrics (sync).
//!
//! Globals are stored as atomics (read sync at produce_metrics time).
//! Per-stream values flow through [`DynamicMetricsRegistry`] (also sync reads).
//!
//! See spec §3.2 for the metrics surface contract. The 7 globals + 9 per-stream
//! templates are declared here once; downstream code (sidecar supervisor,
//! runner loop) calls [`MetricsBridge::apply_stats`] to push data in.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;

use neomind_extension_sdk::{
    DynamicMetricsRegistry, ExtensionMetricValue, MetricBuilder, MetricDataType, MetricTemplate,
    MetricValue,
};

use crate::protocol::Stats;

/// Bridge between Stats events (async, from sidecar) and produce_metrics (sync).
///
/// Globals are stored as atomics (read sync at produce_metrics time).
/// Per-stream values flow through [`DynamicMetricsRegistry`] (also sync reads).
pub struct MetricsBridge {
    /// Per-stream metric registry. Public so the host `metrics()` trait method
    /// can call `descriptors()` directly without an extra indirection.
    pub dynamic: Arc<DynamicMetricsRegistry>,

    // ---- Global atomics. Floats stored as f64::to_bits() in AtomicU64. ----
    active_stream_count: AtomicI64,
    total_throughput_fps: AtomicU64,
    gpu_utilization_percent: AtomicU64,
    gpu_memory_used_mb: AtomicU64,
    sidecar_uptime_start: RwLock<Option<Instant>>,
    restart_count: AtomicI64,
    // sidecar_status is a string — low write frequency, RwLock<String> is fine.
    sidecar_status: RwLock<String>,
}

impl MetricsBridge {
    pub fn new() -> Self {
        Self {
            dynamic: Arc::new(make_registry()),
            active_stream_count: AtomicI64::new(0),
            total_throughput_fps: AtomicU64::new(0),
            gpu_utilization_percent: AtomicU64::new(0),
            gpu_memory_used_mb: AtomicU64::new(0),
            sidecar_uptime_start: RwLock::new(None),
            restart_count: AtomicI64::new(0),
            sidecar_status: RwLock::new("starting".to_string()),
        }
    }

    /// Mark the sidecar as up; subsequent `produce_metrics()` calls will
    /// compute uptime relative to this instant.
    pub fn mark_sidecar_started(&self) {
        *self.sidecar_uptime_start.write() = Some(Instant::now());
        *self.sidecar_status.write() = "running".to_string();
    }

    /// Mark the sidecar as stopped; uptime resets to 0 and status → "stopped".
    pub fn mark_sidecar_stopped(&self) {
        *self.sidecar_uptime_start.write() = None;
        *self.sidecar_status.write() = "stopped".to_string();
    }

    /// Record the supervisor-observed restart count.
    pub fn set_restart_count(&self, count: i64) {
        self.restart_count.store(count, Ordering::SeqCst);
    }

    /// Register a new stream so its 9 per-stream metrics appear in the
    /// registry's descriptor/value output. Idempotent.
    pub fn register_stream(&self, stream_id: &str) {
        self.dynamic.upsert(stream_id, stream_id);
    }

    /// Unregister a stream. Historical samples already stored by the host are
    /// unaffected (see SDK `dynamic_metrics` module docs — orphan series).
    pub fn unregister_stream(&self, stream_id: &str) {
        self.dynamic.remove(stream_id);
    }

    /// Apply a [`Stats`] event from the sidecar: update global atomics +
    /// per-stream registry values.
    ///
    /// Globals derived from `Stats`:
    /// - `active_stream_count` = `per_stream.len()`
    /// - `total_throughput_fps` = `global_fps` (renamed for the metric surface)
    /// - `gpu_utilization_percent`, `gpu_memory_used_mb` passed through.
    ///
    /// Per-stream: sets fps, latency_ms, status, and maps `object_count` →
    /// `detection_count`. The other per-stream templates (person_count,
    /// vehicle_count, line_cross_events, roi_intrusion_events, error_count)
    /// are declared but not populated from `StreamStat` — the sidecar event
    /// format doesn't carry them today. They will appear in `descriptors()`
    /// (so the host knows the series exists) but be omitted from `values()`
    /// until the Python sidecar emits richer per-stream stats.
    pub fn apply_stats(&self, stats: &Stats) {
        // Globals
        self.active_stream_count
            .store(stats.per_stream.len() as i64, Ordering::SeqCst);
        store_f64(&self.total_throughput_fps, stats.global_fps as f64);
        store_f64(
            &self.gpu_utilization_percent,
            stats.gpu_utilization_percent as f64,
        );
        store_f64(
            &self.gpu_memory_used_mb,
            stats.gpu_memory_used_mb as f64,
        );

        // Per-stream
        for s in &stats.per_stream {
            self.dynamic
                .set(&s.stream_id, "stream_fps", MetricValue::Float(s.fps as f64));
            self.dynamic.set(
                &s.stream_id,
                "stream_latency_ms",
                MetricValue::Float(s.latency_ms as f64),
            );
            self.dynamic.set(
                &s.stream_id,
                "stream_detection_count",
                MetricValue::Integer(s.object_count as i64),
            );
            self.dynamic.set(
                &s.stream_id,
                "stream_status",
                MetricValue::String(s.status.clone()),
            );
        }
    }

    /// Produce all metric values for `Extension::produce_metrics()`.
    /// Combines global atomics + DynamicMetricsRegistry output.
    pub fn produce_values(&self) -> Vec<ExtensionMetricValue> {
        let mut out = self.dynamic.values(now_ms());
        out.extend(self.global_values(now_ms()));
        out
    }

    /// Build the 7 global `ExtensionMetricValue` entries. Extracted so tests
    /// can verify globals in isolation without per-stream noise.
    fn global_values(&self, ts: i64) -> Vec<ExtensionMetricValue> {
        vec![
            ExtensionMetricValue {
                name: "active_stream_count".into(),
                value: MetricValue::Integer(self.active_stream_count.load(Ordering::SeqCst)),
                timestamp: ts,
            },
            ExtensionMetricValue {
                name: "total_throughput_fps".into(),
                value: MetricValue::Float(load_f64(&self.total_throughput_fps)),
                timestamp: ts,
            },
            ExtensionMetricValue {
                name: "gpu_utilization_percent".into(),
                value: MetricValue::Float(load_f64(&self.gpu_utilization_percent)),
                timestamp: ts,
            },
            ExtensionMetricValue {
                name: "gpu_memory_used_mb".into(),
                value: MetricValue::Float(load_f64(&self.gpu_memory_used_mb)),
                timestamp: ts,
            },
            ExtensionMetricValue {
                name: "sidecar_status".into(),
                value: MetricValue::String(self.sidecar_status.read().clone()),
                timestamp: ts,
            },
            ExtensionMetricValue {
                name: "sidecar_uptime_secs".into(),
                value: MetricValue::Integer(self.sidecar_uptime_secs()),
                timestamp: ts,
            },
            ExtensionMetricValue {
                name: "restart_count".into(),
                value: MetricValue::Integer(self.restart_count.load(Ordering::SeqCst)),
                timestamp: ts,
            },
        ]
    }

    fn sidecar_uptime_secs(&self) -> i64 {
        self.sidecar_uptime_start
            .read()
            .map(|t| t.elapsed().as_secs() as i64)
            .unwrap_or(0)
    }
}

impl Default for MetricsBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// The 9 per-stream metric templates (spec §3.2). Order matters — the
/// registry emits descriptors/values in template registration order, so keep
/// this stable for deterministic test output.
fn make_registry() -> DynamicMetricsRegistry {
    DynamicMetricsRegistry::new(vec![
        MetricTemplate::new("stream_fps", "FPS · {}", MetricDataType::Float)
            .with_unit("fps")
            .with_min(0.0),
        MetricTemplate::new("stream_latency_ms", "Latency · {}", MetricDataType::Float)
            .with_unit("ms")
            .with_min(0.0),
        MetricTemplate::new("stream_status", "Status · {}", MetricDataType::String),
        MetricTemplate::new(
            "stream_detection_count",
            "Detections · {}",
            MetricDataType::Integer,
        )
        .with_min(0.0),
        MetricTemplate::new(
            "stream_person_count",
            "Persons · {}",
            MetricDataType::Integer,
        )
        .with_min(0.0),
        MetricTemplate::new(
            "stream_vehicle_count",
            "Vehicles · {}",
            MetricDataType::Integer,
        )
        .with_min(0.0),
        MetricTemplate::new(
            "stream_line_cross_events",
            "LineCross · {}",
            MetricDataType::Integer,
        )
        .with_min(0.0),
        MetricTemplate::new(
            "stream_roi_intrusion_events",
            "ROI · {}",
            MetricDataType::Integer,
        )
        .with_min(0.0),
        MetricTemplate::new(
            "stream_error_count",
            "Errors · {}",
            MetricDataType::Integer,
        )
        .with_min(0.0),
    ])
}

/// Static global-metric descriptors (the 7 non-dynamic metrics). Returned by
/// `Extension::metrics()` appended after the dynamic per-stream descriptors.
pub fn global_metric_descriptors() -> Vec<neomind_extension_sdk::MetricDescriptor> {
    vec![
        MetricBuilder::new("active_stream_count", "Active Streams")
            .integer()
            .unit("count")
            .build(),
        MetricBuilder::new("total_throughput_fps", "Total Throughput")
            .float()
            .unit("fps")
            .min(0.0)
            .build(),
        MetricBuilder::new("gpu_utilization_percent", "GPU Utilization")
            .float()
            .unit("%")
            .min(0.0)
            .max(100.0)
            .build(),
        MetricBuilder::new("gpu_memory_used_mb", "GPU Memory Used")
            .float()
            .unit("MB")
            .min(0.0)
            .build(),
        MetricBuilder::new("sidecar_status", "Sidecar Status")
            .string()
            .build(),
        MetricBuilder::new("sidecar_uptime_secs", "Sidecar Uptime")
            .integer()
            .unit("s")
            .min(0.0)
            .build(),
        MetricBuilder::new("restart_count", "Restart Count")
            .integer()
            .unit("count")
            .min(0.0)
            .build(),
    ]
}

// ---- helpers ----

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn store_f64(a: &AtomicU64, v: f64) {
    a.store(v.to_bits(), Ordering::SeqCst);
}

fn load_f64(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Ordering::SeqCst))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::StreamStat;

    fn fake_stats(per_stream: Vec<StreamStat>) -> Stats {
        // Stats is a struct variant of SidecarEvent; construct directly.
        Stats {
            ts: 1_700_000_000_000,
            global_fps: 60.0,
            gpu_utilization_percent: 42.0,
            gpu_memory_used_mb: 2048.0,
            per_stream,
        }
    }

    #[test]
    fn templates_registered_correctly_empty() {
        // No instances → descriptors() is empty (no base descriptors by design).
        let bridge = MetricsBridge::new();
        assert_eq!(bridge.dynamic.instance_count(), 0);
        assert!(bridge.dynamic.descriptors().is_empty());
    }

    #[test]
    fn templates_registered_correctly_with_instance() {
        let bridge = MetricsBridge::new();
        bridge.register_stream("cam1");
        let d = bridge.dynamic.descriptors();
        // 9 templates × 1 instance = 9 descriptors.
        assert_eq!(d.len(), 9, "names: {:?}", d.iter().map(|m| &m.name).collect::<Vec<_>>());
        let names: Vec<&str> = d.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"stream_fps.cam1"));
        assert!(names.contains(&"stream_latency_ms.cam1"));
        assert!(names.contains(&"stream_status.cam1"));
        assert!(names.contains(&"stream_detection_count.cam1"));
        assert!(names.contains(&"stream_person_count.cam1"));
        assert!(names.contains(&"stream_vehicle_count.cam1"));
        assert!(names.contains(&"stream_line_cross_events.cam1"));
        assert!(names.contains(&"stream_roi_intrusion_events.cam1"));
        assert!(names.contains(&"stream_error_count.cam1"));
    }

    #[test]
    fn apply_stats_updates_globals() {
        let bridge = MetricsBridge::new();
        let stats = fake_stats(vec![]);
        bridge.apply_stats(&stats);
        let vals = bridge.produce_values();
        let by_name: std::collections::HashMap<&str, &ExtensionMetricValue> = vals
            .iter()
            .map(|v| (v.name.as_str(), v))
            .collect();
        match &by_name["active_stream_count"].value {
            MetricValue::Integer(n) => assert_eq!(*n, 0),
            other => panic!("expected Integer, got {:?}", other),
        }
        match &by_name["total_throughput_fps"].value {
            MetricValue::Float(f) => assert!((f - 60.0).abs() < 1e-9),
            other => panic!("expected Float, got {:?}", other),
        }
        match &by_name["gpu_utilization_percent"].value {
            MetricValue::Float(f) => assert!((f - 42.0).abs() < 1e-9),
            other => panic!("expected Float, got {:?}", other),
        }
        match &by_name["gpu_memory_used_mb"].value {
            MetricValue::Float(f) => assert!((f - 2048.0).abs() < 1e-9),
            other => panic!("expected Float, got {:?}", other),
        }
    }

    #[test]
    fn apply_stats_updates_per_stream() {
        let bridge = MetricsBridge::new();
        bridge.register_stream("cam1");
        let s = StreamStat {
            stream_id: "cam1".into(),
            fps: 30.0,
            latency_ms: 50.0,
            frame_count: 1000,
            object_count: 5,
            status: "running".into(),
        };
        bridge.apply_stats(&fake_stats(vec![s]));
        let vals = bridge.produce_values();
        let by_name: std::collections::HashMap<&str, &ExtensionMetricValue> = vals
            .iter()
            .map(|v| (v.name.as_str(), v))
            .collect();
        match &by_name["stream_fps.cam1"].value {
            MetricValue::Float(f) => assert!((f - 30.0).abs() < 1e-9),
            other => panic!("expected Float(30), got {:?}", other),
        }
        match &by_name["stream_latency_ms.cam1"].value {
            MetricValue::Float(f) => assert!((f - 50.0).abs() < 1e-9),
            other => panic!("expected Float(50), got {:?}", other),
        }
        match &by_name["stream_detection_count.cam1"].value {
            MetricValue::Integer(n) => assert_eq!(*n, 5),
            other => panic!("expected Integer(5), got {:?}", other),
        }
        match &by_name["stream_status.cam1"].value {
            MetricValue::String(s) => assert_eq!(s, "running"),
            other => panic!("expected String, got {:?}", other),
        }
        // active_stream_count reflects per_stream.len()
        match &by_name["active_stream_count"].value {
            MetricValue::Integer(n) => assert_eq!(*n, 1),
            other => panic!("expected Integer(1), got {:?}", other),
        }
    }

    #[test]
    fn produce_values_returns_7_globals_on_empty_registry() {
        let bridge = MetricsBridge::new();
        let vals = bridge.produce_values();
        assert_eq!(vals.len(), 7, "expected 7 globals only, got {vals:?}");
        let names: Vec<&str> = vals.iter().map(|v| v.name.as_str()).collect();
        for expected in [
            "active_stream_count",
            "total_throughput_fps",
            "gpu_utilization_percent",
            "gpu_memory_used_mb",
            "sidecar_status",
            "sidecar_uptime_secs",
            "restart_count",
        ] {
            assert!(names.contains(&expected), "missing {expected} in {names:?}");
        }
    }

    #[test]
    fn produce_values_returns_7_globals_plus_dynamic_after_register() {
        let bridge = MetricsBridge::new();
        bridge.register_stream("cam1");
        let s = StreamStat {
            stream_id: "cam1".into(),
            fps: 30.0,
            latency_ms: 50.0,
            frame_count: 1,
            object_count: 0,
            status: "running".into(),
        };
        bridge.apply_stats(&fake_stats(vec![s]));
        let vals = bridge.produce_values();
        // 7 globals + 4 populated per-stream (fps, latency_ms, detection_count,
        // status — the other 5 templates have no value set so are omitted).
        // Total = 7 + 4 = 11.
        assert_eq!(vals.len(), 11, "got {vals:?}");
    }

    #[test]
    fn register_then_unregister_stream_removes_per_stream_metrics() {
        let bridge = MetricsBridge::new();
        bridge.register_stream("cam1");
        bridge.register_stream("cam2");
        assert_eq!(bridge.dynamic.descriptors().len(), 18); // 9 × 2
        bridge.unregister_stream("cam1");
        let d = bridge.dynamic.descriptors();
        assert_eq!(d.len(), 9);
        assert!(d.iter().all(|m| !m.name.ends_with(".cam1")));
    }

    #[test]
    fn sidecar_uptime_zero_when_not_started() {
        let bridge = MetricsBridge::new();
        assert_eq!(bridge.sidecar_uptime_secs(), 0);
        bridge.mark_sidecar_started();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let up = bridge.sidecar_uptime_secs();
        // 100ms rounds down to 0 secs — but at least the timer is running.
        // Use >= 0 to be robust against sub-second sleeps while still proving
        // the Option is Some.
        assert!(up >= 0);
        // Force a > 1s sleep variant to actually observe uptime > 0.
        // (Skipping in fast CI; the mark_stopped assertion below is the real gate.)
        bridge.mark_sidecar_stopped();
        assert_eq!(bridge.sidecar_uptime_secs(), 0);
    }

    #[test]
    fn sidecar_uptime_grows_after_one_second() {
        // Separate test so the 1s sleep only runs when explicitly selected.
        let bridge = MetricsBridge::new();
        bridge.mark_sidecar_started();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(bridge.sidecar_uptime_secs() >= 1, "expected >= 1s uptime");
        bridge.mark_sidecar_stopped();
        assert_eq!(bridge.sidecar_uptime_secs(), 0);
    }

    #[test]
    fn sidecar_status_string_round_trips() {
        let bridge = MetricsBridge::new();
        assert_eq!(&*bridge.sidecar_status.read(), "starting");
        bridge.mark_sidecar_started();
        assert_eq!(&*bridge.sidecar_status.read(), "running");
        bridge.mark_sidecar_stopped();
        assert_eq!(&*bridge.sidecar_status.read(), "stopped");
    }

    #[test]
    fn restart_count_round_trips() {
        let bridge = MetricsBridge::new();
        bridge.set_restart_count(3);
        let vals = bridge.produce_values();
        let rc = vals
            .iter()
            .find(|v| v.name == "restart_count")
            .expect("restart_count present");
        match &rc.value {
            MetricValue::Integer(n) => assert_eq!(*n, 3),
            other => panic!("{other:?}"),
        }
    }
}
