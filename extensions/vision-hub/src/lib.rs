//! vision-hub — the unified NeoMind vision extension.
//!
//! One extension, one task schema, one pipeline config:
//! - `analyze` — one-shot analysis of any image (upload / data-URL) with a
//!   task list; the same entry the agent uses as an LLM tool.
//! - pipelines — bind camera devices to task lists; results flow to
//!   virtual metrics (`virtual.vision.<pipeline>.*`), `vision.result`
//!   events, capture state, and UI snapshots.
//! - hardware acceleration via vision-common: probe once, plan per model,
//!   `get_status` shows which accelerator each model actually runs on.
//!
//! Detection ships in this build; OCR / face / grounding / VLM land in
//! follow-up batches (see README roadmap) without schema changes.

mod detect;
mod pipeline;
mod task;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionCommand, ExtensionError, ExtensionMetadata, ExtensionMetricValue,
    MetricDataType, MetricDescriptor, ParamMetricValue, ParameterDefinition, Result,
};
use parking_lot::Mutex;
use serde_json::json;

use pipeline::PipelineEngine;
use task::{TaskRegistry, TaskSpec};

pub struct VisionHub {
    registry: Arc<TaskRegistry>,
    engine: PipelineEngine,
    analyze_count: AtomicU64,
    analyze_errors: AtomicU64,
    model_manager: vision_common::model::ModelManager,
}

impl VisionHub {
    pub fn new() -> Self {
        // Probe hardware once for the whole extension lifetime.
        let profile = Arc::new(vision_common::accel::HardwareProfile::probe());
        tracing::info!(
            "[vision-hub] hardware: os={} arch={} cuda_device={} gpu_free={:?} ram={}GB board={:?} coreml={}",
            profile.os,
            profile.arch,
            profile.nvidia_device,
            profile.gpu_free_mb,
            profile.total_ram_gb,
            profile.board,
            profile.coreml
        );
        let registry = Arc::new(TaskRegistry::new(profile));
        let engine = PipelineEngine::new(registry.clone());
        engine.load_config();
        Self {
            registry,
            engine,
            analyze_count: AtomicU64::new(0),
            analyze_errors: AtomicU64::new(0),
            model_manager: vision_common::model::ModelManager::new(),
        }
    }

    /// The `analyze` command: image + task list → unified results.
    fn analyze(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        self.analyze_count.fetch_add(1, Ordering::SeqCst);
        let img = args
            .get("image")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("missing 'image'".into()))?;
        // Input cap: a multi-MB base64 plus a decoded copy plus an RGB copy
        // tripled peak memory; refuse absurd payloads before decoding.
        const MAX_IMAGE_INPUT_BYTES: usize = 32 * 1024 * 1024;
        if img.len() > MAX_IMAGE_INPUT_BYTES {
            return Err(ExtensionError::InvalidArguments(format!(
                "image input {} bytes exceeds {}MB cap — downscale before sending",
                img.len(),
                MAX_IMAGE_INPUT_BYTES / (1024 * 1024)
            )));
        }
        let img = vision_common::image::decode_image_input(img)
            .map_err(|e| ExtensionError::InvalidArguments(e.to_string()))?;
        // Decoded-pixel cap: a valid 32MB base64 can still be a 100MP+
        // image; RGB conversion triples it before inference even starts.
        const MAX_PIXELS: u64 = 50_000_000;
        if (img.width() as u64) * (img.height() as u64) > MAX_PIXELS {
            return Err(ExtensionError::InvalidArguments(format!(
                "image is {}x{} ({}MP) — exceeds {}MP cap, downscale first",
                img.width(),
                img.height(),
                img.width() as u64 * img.height() as u64 / 1_000_000,
                MAX_PIXELS / 1_000_000
            )));
        }

        // tasks: [{type, params, device}] — default [detect]
        let specs: Vec<TaskSpec> = match args.get("tasks") {
            Some(list) => serde_json::from_value(list.clone())
                .map_err(|e| ExtensionError::InvalidArguments(format!("bad tasks: {e}")))?,
            None => vec![serde_json::from_value(json!({"type": "detect"})).unwrap()],
        };
        if specs.is_empty() {
            return Err(ExtensionError::InvalidArguments("tasks list is empty".into()));
        }
        let global_device = args.get("device").and_then(|d| d.as_str()).map(String::from);
        let specs: Vec<TaskSpec> = specs
            .into_iter()
            .map(|mut s| {
                if s.device.is_none() {
                    s.device = global_device.clone().or_else(|| Some("auto".to_string()));
                }
                s
            })
            .collect();

        let include_annotated = args
            .get("include_annotated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut results = Vec::new();
        let mut task_errors = Vec::new();
        for spec in &specs {
            match self.registry.get_or_load(spec).and_then(|_| self.registry.run(spec, &img)) {
                Ok(mut r) => {
                    // optional label filter for one-shot calls too
                    if let Some(keep) = spec.params.get("labels").and_then(|l| l.as_array()) {
                        let keep: Vec<String> =
                            keep.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                        r.detections.retain(|d| keep.contains(&d.label));
                    }
                    results.push(r);
                }
                Err(e) => {
                    tracing::warn!("[vision-hub] task {} failed: {e}", spec.kind.as_str());
                    task_errors.push(json!({
                        "task": spec.kind.as_str(),
                        "error": e,
                    }));
                }
            }
        }

        let annotated = if include_annotated {
            let dets: Vec<vision_common::Detection> =
                results.iter().flat_map(|r| r.detections.clone()).collect();
            let mut rgb = img.to_rgb8();
            vision_common::draw::draw_detections(&mut rgb, &dets);
            vision_common::image::encode_jpeg(&image::DynamicImage::ImageRgb8(rgb), 85)
                .ok()
                .map(|jpeg| vision_common::image::to_data_url("image/jpeg", &jpeg))
        } else {
            None
        };

        if results.is_empty() {
            self.analyze_errors.fetch_add(1, Ordering::SeqCst);
            let detail = task_errors
                .iter()
                .map(|e| e["error"].as_str().unwrap_or("?").to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ExtensionError::ExecutionFailed(format!(
                "no task succeeded ({detail})"
            )));
        }

        let inference_ms: f64 = results.iter().map(|r| r.inference_ms).sum::<f64>()
            / results.len() as f64;

        Ok(json!({
            "image_size": [img.width(), img.height()],
            "results": results.iter().map(|r| r.to_tool_json()).collect::<Vec<_>>(),
            "task_errors": task_errors,
            "annotated_image": annotated,
            "inference_ms": inference_ms as u64,
        }))
    }

    fn get_status(&self) -> serde_json::Value {
        let profile = self.registry.profile();
        let inventory: Vec<serde_json::Value> = self
            .model_manager
            .inventory()
            .into_iter()
            .map(|(name, size)| json!({ "file": name, "bytes": size }))
            .collect();
        json!({
            "hardware": {
                "os": profile.os,
                "arch": profile.arch,
                "nvidia_device": profile.nvidia_device,
                "gpu_free_mb": profile.gpu_free_mb,
                "total_ram_gb": profile.total_ram_gb,
                "rlimit_as_mb": profile.rlimit_as_mb,
                "board": profile.board,
                "coreml": profile.coreml,
            },
            "license": self.registry.license(),
            "loaded_tasks": self.registry.status(),
            "pipelines": {
                "count": self.engine.configs.lock().len(),
                "frames_processed": self.engine.frames_processed.load(Ordering::SeqCst),
            },
            "model_cache": inventory,
            "analyze": {
                "count": self.analyze_count.load(Ordering::SeqCst),
                "errors": self.analyze_errors.load(Ordering::SeqCst),
            },
        })
    }

    fn parse_pipeline(args: &serde_json::Value) -> Result<pipeline::PipelineConfig> {
        let raw = args.get("pipeline").unwrap_or(args);
        serde_json::from_value(raw.clone())
            .map_err(|e| ExtensionError::InvalidArguments(format!("bad pipeline: {e}")))
    }
}

impl Default for VisionHub {
    fn default() -> Self {
        Self::new()
    }
}

/// Re-wrap an unwrapped MetricValue for the pipeline's JSON-based extractor.
fn metric_value_to_json(v: &neomind_extension_sdk::events::MetricValueData) -> serde_json::Value {
    use neomind_extension_sdk::events::MetricValueData;
    match v {
        MetricValueData::Float(f) => json!(f),
        MetricValueData::Integer(i) => json!(i),
        MetricValueData::Boolean(b) => json!(b),
        MetricValueData::String(s) => json!(s),
        MetricValueData::Binary(b) => json!(b),
        MetricValueData::Json(j) => j.clone(),
        MetricValueData::Null => serde_json::Value::Null,
    }
}

#[async_trait]
impl Extension for VisionHub {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("vision-hub", "Vision Hub", "0.1.0")
                .with_description(
                    "Unified vision: hardware-accelerated detection pipelines on device streams \
                     with virtual metrics, events and capture (detect batch; ocr/face/ground/vlm next)",
                )
                .with_author("NeoMind Team")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor {
                name: "pipelines_active".into(),
                display_name: "Active Pipelines".into(),
                data_type: MetricDataType::Integer,
                unit: "count".into(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "frames_processed".into(),
                display_name: "Frames Processed".into(),
                data_type: MetricDataType::Integer,
                unit: "count".into(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_inferences".into(),
                display_name: "Total Inferences".into(),
                data_type: MetricDataType::Integer,
                unit: "count".into(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "total_detections".into(),
                display_name: "Total Detections".into(),
                data_type: MetricDataType::Integer,
                unit: "count".into(),
                min: Some(0.0),
                max: None,
                required: false,
            },
            MetricDescriptor {
                name: "avg_inference_ms".into(),
                display_name: "Avg Inference Time".into(),
                data_type: MetricDataType::Float,
                unit: "ms".into(),
                min: Some(0.0),
                max: None,
                required: false,
            },
        ]
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let inferences = self.engine.total_inferences.load(Ordering::SeqCst);
        let detections = self.engine.total_detections.load(Ordering::SeqCst);
        let active = self
            .engine
            .configs
            .lock()
            .iter()
            .filter(|c| c.enabled)
            .count();
        // REAL average across all runs (pipelines + reloads): cumulative
        // ms / cumulative count. The old version surfaced an arbitrary
        // pipeline's latest value — unstable under polling.
        let count = self.engine.total_inferences.load(Ordering::SeqCst);
        let total_ms = self.engine.total_inference_ms.load(Ordering::SeqCst);
        let avg = if count > 0 {
            total_ms as f64 / count as f64
        } else {
            0.0
        };
        Ok(vec![
            ExtensionMetricValue {
                name: "pipelines_active".into(),
                value: ParamMetricValue::Integer(active as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "frames_processed".into(),
                value: ParamMetricValue::Integer(
                    self.engine.frames_processed.load(Ordering::SeqCst) as i64,
                ),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_inferences".into(),
                value: ParamMetricValue::Integer(inferences as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_detections".into(),
                value: ParamMetricValue::Integer(detections as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "avg_inference_ms".into(),
                value: ParamMetricValue::Float(avg),
                timestamp: now,
            },
        ])
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        let image_param = || ParameterDefinition {
            name: "image".into(),
            display_name: "Image".into(),
            description: "Base64 or data-URL image".into(),
            param_type: MetricDataType::String,
            required: true,
            default_value: None,
            min: None,
            max: None,
            options: Vec::new(),
        };
        vec![
            ExtensionCommand {
                name: "analyze".into(),
                display_name: "Analyze Image".into(),
                description: "Run vision tasks (detect/…) on an image; returns the unified result schema".into(),
                payload_template: String::new(),
                parameters: vec![
                    image_param(),
                    ParameterDefinition {
                        name: "tasks".into(),
                        display_name: "Tasks".into(),
                        description: r#"[{"type":"detect","params":{"confidence":0.4,"labels":["person"]}}]"#.into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                    ParameterDefinition {
                        name: "include_annotated".into(),
                        display_name: "Include Annotated Image".into(),
                        description: "Return an annotated JPEG data-URL".into(),
                        param_type: MetricDataType::Boolean,
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: Vec::new(),
                    },
                ],
                fixed_values: HashMap::new(),
                samples: vec![json!({
                    "image": "data:image/png;base64,iVBORw0KGgo=",
                    "tasks": [{"type": "detect"}]
                })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "get_status".into(),
                display_name: "Get Status".into(),
                description: "Hardware profile, accelerator per loaded model, pipelines, model cache".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "list_pipelines".into(),
                display_name: "List Pipelines".into(),
                description: "All pipelines with runtime state (last result, snapshot)".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "create_pipeline".into(),
                display_name: "Create Pipeline".into(),
                description: "Create or replace a pipeline: {id, source:{type:device,device_id,metric}, tasks:[{type:detect,...}], sinks, schedule}".into(),
                payload_template: String::new(),
                parameters: vec![ParameterDefinition {
                    name: "pipeline".into(),
                    display_name: "Pipeline Config".into(),
                    description: "Pipeline JSON object".into(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                }],
                fixed_values: HashMap::new(),
                samples: vec![json!({
                    "pipeline": {
                        "id": "gate-camera",
                        "source": {"type": "device", "device_id": "NE301-0001", "metric": "image"},
                        "tasks": [{"type": "detect", "params": {"confidence": 0.5, "labels": ["person"]}}],
                        "sinks": {"virtual_metrics": true, "event": true, "capture": {"kind": "presence", "cooldown_secs": 60}}
                    }
                })],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "delete_pipeline".into(),
                display_name: "Delete Pipeline".into(),
                description: "Remove a pipeline by id".into(),
                payload_template: String::new(),
                parameters: vec![ParameterDefinition {
                    name: "id".into(),
                    display_name: "Pipeline ID".into(),
                    description: "Pipeline id".into(),
                    param_type: MetricDataType::String,
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: Vec::new(),
                }],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "reload_models".into(),
                display_name: "Reload Models".into(),
                description: "Drop all loaded model sessions; they reload lazily with current config".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "reload_license".into(),
                display_name: "Reload License".into(),
                description: "Re-read license.key (or NEOMIND_LICENSE_KEY) without restarting".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
            ExtensionCommand {
                name: "model_cache".into(),
                display_name: "Model Cache".into(),
                description: "List cached model files".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: HashMap::new(),
                samples: vec![],
                parameter_groups: Vec::new(),
            },
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "analyze" => self.analyze(args),
            "get_status" => Ok(self.get_status()),
            "list_pipelines" => {
                let include_snapshots = args
                    .get("include_snapshots")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(json!({ "pipelines": self.engine.list(include_snapshots) }))
            }
            "create_pipeline" => {
                let config = Self::parse_pipeline(args)?;
                self.engine
                    .upsert(config.clone())
                    .map_err(|e| ExtensionError::InvalidArguments(e))?;
                Ok(json!({ "status": "ok", "pipeline": config }))
            }
            "delete_pipeline" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("missing id".into()))?;
                if self.engine.delete(id) {
                    Ok(json!({ "status": "deleted", "id": id }))
                } else {
                    Err(ExtensionError::InvalidArguments(format!("no pipeline '{id}'")))
                }
            }
            "reload_models" => {
                let configs = self.engine.configs.lock().clone();
                let specs: Vec<TaskSpec> = configs.iter().flat_map(|c| c.tasks.clone()).collect();
                self.registry
                    .reload_all(&specs)
                    .map_err(ExtensionError::ExecutionFailed)?;
                Ok(json!({ "status": "reloaded", "tasks": specs.len() }))
            }
            "reload_license" => {
                let license = self.registry.reload_license();
                // Summary only: the full state carries the fingerprint and
                // raw license material — no reason to hand those to whoever
                // can invoke a command.
                Ok(json!({ "status": "ok", "license": {
                    "source": license.source,
                    "licensed_to": license.licensed_to,
                    "expires_at": license.expires_at,
                    "features": license.features,
                }}))
            }
            "model_cache" => Ok(json!({
                "dir": self.model_manager.cache_dir().display().to_string(),
                "files": self.model_manager.inventory(),
            })),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn event_subscriptions(&self) -> &[&'static str] {
        &["DeviceMetric"]
    }

    fn handle_event(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        // SDK 0.6.5 typed mirror: handles the {event_type,payload} envelope
        // AND bare payloads, unwraps MetricValue, never panics on misses.
        if let Some(m) = neomind_extension_sdk::events::SdkEvent::parse(event_type, payload)
            .as_device_metric()
        {
            // Virtual metrics (including OUR OWN virtual.vision.* writes)
            // must never re-enter the pipeline engine — matching only by
            // metric name would work until someone binds a nested name
            // that collides with a virtual prefix.
            if m.is_virtual {
                return Ok(());
            }
            if !m.device_id.is_empty() {
                let value = metric_value_to_json(&m.value);
                self.engine.handle_device_metric(&m.device_id, &m.metric, &value);
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

neomind_extension_sdk::neomind_export!(VisionHub);

#[cfg(test)]
mod tests {
    use super::*;
    use task::TaskKind;

    #[test]
    fn metadata_shape() {
        let hub = VisionHub::new();
        let meta = hub.metadata();
        assert_eq!(meta.id, "vision-hub");
    }

    #[test]
    fn command_surface() {
        let hub = VisionHub::new();
        let cmds = hub.commands();
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        for expected in [
            "analyze",
            "get_status",
            "list_pipelines",
            "create_pipeline",
            "delete_pipeline",
            "reload_models",
            "reload_license",
            "model_cache",
        ] {
            assert!(names.contains(&expected), "missing command {expected}");
        }
    }

    #[tokio::test]
    async fn analyze_rejects_bad_args() {
        let hub = VisionHub::new();
        let err = hub
            .execute_command("analyze", &json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, ExtensionError::InvalidArguments(_)));
    }

    #[tokio::test]
    async fn unknown_command_errors() {
        let hub = VisionHub::new();
        let err = hub.execute_command("nope", &json!({})).await.unwrap_err();
        assert!(matches!(err, ExtensionError::CommandNotFound(_)));
    }

    #[tokio::test]
    async fn pipeline_crud_roundtrip() {
        let hub = VisionHub::new();
        let args = json!({
            "pipeline": {
                "id": "test-pl",
                "source": {"type": "device", "device_id": "D1", "metric": "image"},
                "tasks": [{"type": "detect"}],
            }
        });
        hub.execute_command("create_pipeline", &args).await.unwrap();
        let listed = hub.execute_command("list_pipelines", &json!({})).await.unwrap();
        assert_eq!(listed["pipelines"].as_array().unwrap().len(), 1);
        assert_eq!(listed["pipelines"][0]["config"]["id"], "test-pl");

        hub.execute_command("delete_pipeline", &json!({"id": "test-pl"}))
            .await
            .unwrap();
        let listed = hub.execute_command("list_pipelines", &json!({})).await.unwrap();
        assert_eq!(listed["pipelines"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn create_pipeline_rejects_stream_source() {
        let hub = VisionHub::new();
        let args = json!({
            "pipeline": {
                "id": "bad",
                "source": {"type": "stream", "url": "rtsp://x"},
                "tasks": [{"type": "detect"}],
            }
        });
        let err = hub
            .execute_command("create_pipeline", &args)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("source"));
    }

    #[tokio::test]
    async fn get_status_reports_hardware() {
        let hub = VisionHub::new();
        let status = hub.execute_command("get_status", &json!({})).await.unwrap();
        assert!(status["hardware"].is_object());
        assert!(status["hardware"]["os"].is_string());
        assert!(status["license"].is_object());
        assert!(status["model_cache"].is_array());
    }

    #[test]
    fn license_gate_default_open_and_fail_closed() {
        let reg = TaskRegistry::new(Arc::new(
            vision_common::accel::HardwareProfile::default(),
        ));

        // Default open: building is attempted (fails on missing model file,
        // NOT on license).
        let spec = TaskSpec {
            kind: TaskKind::Detect,
            params: serde_json::json!({"model": "definitely-missing.onnx"}),
            device: None,
        };
        let err = reg.get_or_load(&spec).unwrap_err();
        assert!(err.contains("not found"), "got: {err}");

        // Rejected license: fails closed at the gate, before model lookup.
        reg.set_license(vision_common::license::LicenseState {
            licensed_to: None,
            expires_at: None,
            features: vec![],
            fingerprint: None,
            source: vision_common::license::LicenseSource::Rejected("test".into()),
        });
        let err = reg.get_or_load(&spec).unwrap_err();
        assert!(err.contains("blocked by license"), "got: {err}");

        // Partial license: unlisted task denied, listed task proceeds.
        reg.set_license(vision_common::license::LicenseState {
            licensed_to: Some("acme".into()),
            expires_at: None,
            features: vec!["detect".into()],
            fingerprint: None,
            source: vision_common::license::LicenseSource::Verified,
        });
        let ocr_spec = TaskSpec {
            kind: TaskKind::Ocr,
            params: serde_json::json!({}),
            device: None,
        };
        assert!(reg.get_or_load(&ocr_spec).unwrap_err().contains("not included"));
        let err = reg.get_or_load(&spec).unwrap_err();
        assert!(err.contains("not found"), "detect allowed, model missing: {err}");
    }

    #[tokio::test]
    async fn handle_event_ignores_virtual_metrics() {
        // Our own virtual.vision.* writes must never re-enter the engine.
        let hub = VisionHub::new();
        hub.handle_event(
            "DeviceMetric",
            &json!({
                "payload": {
                    "device_id": "D1",
                    "metric": "virtual.vision.gate.detections",
                    "value": {"Integer": 3},
                    "is_virtual": true,
                }
            }),
        )
        .unwrap();
        // frames_processed must NOT have moved
        assert_eq!(hub.engine.frames_processed.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn analyze_rejects_oversized_input() {
        let hub = VisionHub::new();
        let huge = "A".repeat(33 * 1024 * 1024);
        let err = hub
            .execute_command("analyze", &json!({"image": huge}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cap"), "got: {err}");
    }

    #[tokio::test]
    async fn handle_event_ignores_non_matching_devices() {
        let hub = VisionHub::new();
        hub.handle_event(
            "DeviceMetric",
            &json!({
                "payload": {
                    "device_id": "unknown-device",
                    "metric": "image",
                    "value": {"String": "not-base64-at-all!!"}
                }
            }),
        )
        .unwrap(); // must not error or crash
    }
}
