//! LAYA Decision Layer monitor extension.
//!
//! The decision layer itself is COMPILED INTO the platform (decision-native
//! feature + ONNX artifacts under `<data>/models/decision/`); this extension
//! does not implement it — it makes it observable:
//!
//! - **Metrics**: model-artifact presence/size and the decision audit log's
//!   record count, derived from the data dir (file-based, works regardless
//!   of auth or server version).
//! - **`status` command**: one JSON summary — artifacts, sizes, and the last
//!   audit record (ts/latency/backend/heads).
//! - **Dashboard card** (frontend/): the live view — mode badge, backend and
//!   the platform's authoritative counters via `GET /api/decisions/status`.
//!
//! `dataDir` is a config parameter (default `data`, matching the platform's
//! CWD-relative default). All reads are read-only and missing-file safe: a
//! deployment without the decision layer reports `model_present=false` and
//! zero counts, never errors.

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, MetricDataType, MetricDescriptor, ParamMetricValue,
    ParameterDefinition, Result,
};

pub struct LayaDecisionExtension {
    data_dir: Mutex<String>,
    request_count: AtomicI64,
}

impl LayaDecisionExtension {
    pub fn new() -> Self {
        Self {
            data_dir: Mutex::new(String::new()), // empty = auto-detect (dir())
            request_count: AtomicI64::new(0),
        }
    }

    /// Resolve the platform data dir: explicit config > CWD's grandparent
    /// (the host runs extensions with CWD = `<data>/extensions/<id>/`, so
    /// `../..` IS the data dir) > `data` (the platform's CWD-relative
    /// default, for dev runs). Verified against real markers — a stale or
    /// renamed install dir must not silently read some other tree.
    fn dir(&self) -> PathBuf {
        let configured = self.data_dir.lock().unwrap().clone();
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
        if let Ok(cwd) = std::env::current_dir() {
            let candidate = cwd.join("../..");
            if candidate.join("models").join("decision").is_dir()
                || candidate.join("logs").join("decisions.jsonl").is_file()
            {
                return candidate;
            }
        }
        PathBuf::from("data")
    }

    /// Artifact presence + total size of `<data>/models/decision/`.
    fn artifacts(&self) -> (bool, u64) {
        let dir = self.dir().join("models").join("decision");
        let mut total = 0u64;
        let mut present = false;
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                if let Ok(md) = entry.metadata() {
                    if md.is_file() {
                        present = true;
                        total += md.len();
                    }
                }
            }
        }
        (present, total)
    }

    /// Decision audit-log stats: record count + the last record, parsed
    /// leniently (the file is append-only JSONL; a torn tail line from a
    /// crash mid-write is skipped, not fatal).
    fn audit(&self) -> (i64, Option<serde_json::Value>) {
        let path = self.dir().join("logs").join("decisions.jsonl");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return (0, None);
        };
        let mut count = 0i64;
        let mut last = None;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(v) => {
                    count += 1;
                    last = Some(v);
                }
                Err(_) => continue,
            }
        }
        (count, last)
    }
}

impl Default for LayaDecisionExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for LayaDecisionExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("laya-decision", "LAYA Decision Layer", "2.12.0")
                .with_description(
                    "Status monitor for the LAYA decision layer: artifact presence, \
                     audit-log stats, and the live dashboard card",
                )
                .with_author("NeoMind Team")
                .with_config_parameters(vec![ParameterDefinition {
                    name: "dataDir".to_string(),
                    display_name: "Data Directory".to_string(),
                    description: "Platform data directory (holds models/decision/ and logs/decisions.jsonl)".to_string(),
                    param_type: MetricDataType::String,
                    required: false,
                    default_value: Some(ParamMetricValue::String("data".to_string())),
                    min: None,
                    max: None,
                    options: Vec::new(),
                }])
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "model_present".to_string(),
                display_name: "Model Artifacts Present".to_string(),
                data_type: MetricDataType::Boolean,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "model_size_mb".to_string(),
                display_name: "Model Artifacts Size".to_string(),
                data_type: MetricDataType::Float,
                unit: "MB".to_string(),
                min: None,
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "decisions_logged".to_string(),
                display_name: "Decisions Logged".to_string(),
                data_type: MetricDataType::Integer,
                unit: String::new(),
                min: None,
                max: None,
                required: false,
            },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![ExtensionCommand {
            name: "status".to_string(),
            display_name: "Status".to_string(),
            description: "One JSON summary: artifact presence/size, audit-log count and the last decision record".to_string(),
            payload_template: String::new(),
            parameters: Vec::new(),
            fixed_values: std::collections::HashMap::new(),
            samples: vec![json!({})],
            parameter_groups: Vec::new(),
        }]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        match command {
            "status" => {
                let (present, size) = self.artifacts();
                let (logged, last) = self.audit();
                Ok(json!({
                    "model_present": present,
                    "model_size_bytes": size,
                    "decisions_logged": logged,
                    "last_decision": last,
                }))
            }
            "configure" => {
                if let Some(d) = args.get("dataDir").and_then(|v| v.as_str()) {
                    *self.data_dir.lock().unwrap() = d.to_string();
                }
                Ok(json!({"status": "ok"}))
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let (present, size) = self.artifacts();
        let (logged, _) = self.audit();
        Ok(vec![
            ExtensionMetricValue {
                name: "model_present".to_string(),
                value: ParamMetricValue::Boolean(present),
                timestamp: 0,
            },
            ExtensionMetricValue {
                name: "model_size_mb".to_string(),
                value: ParamMetricValue::Float((size as f64) / 1_048_576.0),
                timestamp: 0,
            },
            ExtensionMetricValue {
                name: "decisions_logged".to_string(),
                value: ParamMetricValue::Integer(logged),
                timestamp: 0,
            },
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

neomind_extension_sdk::neomind_export!(LayaDecisionExtension);
