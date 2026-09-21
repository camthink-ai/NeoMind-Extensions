//! Vision task abstraction — the plugin interface every task family
//! (detect / ocr / face / ground / vlm) implements. One image in, one
//! [`vision_common::VisionResult`] out; the pipeline engine and the
//! `analyze` command never care which model family ran.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use vision_common::{DeviceHint, VisionResult};

/// Which task family a spec refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Detect,
    Ocr,
    Face,
    Ground,
    Vlm,
}

impl TaskKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskKind::Detect => "detect",
            TaskKind::Ocr => "ocr",
            TaskKind::Face => "face",
            TaskKind::Ground => "ground",
            TaskKind::Vlm => "vlm",
        }
    }

    /// Friendly aliases ("yolo" → detect, "locate" → ground). Kept for the
    /// analyze command's early-validation path and future batches.
    #[allow(dead_code)]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "detect" | "yolo" => Some(TaskKind::Detect),
            "ocr" | "paddle" => Some(TaskKind::Ocr),
            "face" => Some(TaskKind::Face),
            "ground" | "locate" => Some(TaskKind::Ground),
            "vlm" | "describe" | "caption" => Some(TaskKind::Vlm),
            _ => None,
        }
    }
}

/// "auto" | "cpu" | "cuda" | "coreml" | "tensorrt"/"trt" → DeviceHint.
pub fn parse_device_hint(device: Option<&str>) -> DeviceHint {
    match device.map(|d| d.to_lowercase()).as_deref() {
        Some("cpu") => DeviceHint::Cpu,
        Some("cuda") => DeviceHint::Cuda(0),
        Some("coreml") => DeviceHint::CoreMl,
        Some("tensorrt") | Some("trt") => DeviceHint::TensorRt(0),
        _ => DeviceHint::Auto,
    }
}

/// A configured task instance inside a pipeline or an analyze call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    #[serde(rename = "type")]
    pub kind: TaskKind,
    /// Per-task parameters (confidence, rois, language, prompt, model…).
    #[serde(default)]
    pub params: serde_json::Value,
    /// Device override: "auto" | "cpu" | "cuda" | "coreml".
    #[serde(default)]
    pub device: Option<String>,
}

/// A loaded, stateful task (owns its model session).
pub trait VisionTask: Send {
    fn kind(&self) -> TaskKind;
    fn accelerator(&self) -> String;
    /// Analyze one image. `params` comes from the TaskSpec.
    fn analyze(
        &mut self,
        image: &image::DynamicImage,
        params: &serde_json::Value,
    ) -> Result<VisionResult, String>;
}

/// Registry of lazily-loaded tasks keyed by kind (+ model identity, so
/// different model files of the same kind coexist). Enforces the license
/// gate at the single point where task instances come into existence.
pub struct TaskRegistry {
    profile: Arc<vision_common::accel::HardwareProfile>,
    license: parking_lot::RwLock<vision_common::license::LicenseState>,
    tasks: Mutex<Vec<TaskEntry>>,
}

struct TaskEntry {
    key: String,
    task: Box<dyn VisionTask>,
}

impl TaskRegistry {
    pub fn new(profile: Arc<vision_common::accel::HardwareProfile>) -> Self {
        let license = vision_common::license::LicenseState::load();
        tracing::info!(
            "[vision-hub] license: source={:?} features={:?} reason={:?}",
            license.source,
            license.features,
            license.reason()
        );
        Self {
            profile,
            license: parking_lot::RwLock::new(license),
            tasks: Mutex::new(Vec::new()),
        }
    }

    pub fn license(&self) -> vision_common::license::LicenseState {
        self.license.read().clone()
    }

    /// Re-read the license file (e.g. after the user drops a key in place).
    pub fn reload_license(&self) -> vision_common::license::LicenseState {
        let fresh = vision_common::license::LicenseState::load();
        *self.license.write() = fresh.clone();
        fresh
    }

    /// Inject a license state (tests; future platform-side entitlements).
    #[cfg_attr(not(test), doc(hidden), allow(dead_code))]
    pub fn set_license(&self, state: vision_common::license::LicenseState) {
        *self.license.write() = state;
    }

    pub fn profile(&self) -> &vision_common::accel::HardwareProfile {
        &self.profile
    }

    #[allow(dead_code)]
    pub fn hint_for(&self, device: &Option<String>) -> DeviceHint {
        parse_device_hint(device.as_deref())
    }

    /// Get-or-load the task for a spec. The model identity (from params)
    /// keys the cache so `"model": "custom.onnx"` loads a second instance.
    pub fn get_or_load(&self, spec: &TaskSpec) -> Result<(), String> {
        // License gate — the one place task instances are created. Absent
        // license = default open, so this is transparent until monetization
        // switches on; a rejected license fails closed.
        {
            let license = self.license.read();
            license.require(spec.kind.as_str())?;
        }
        let key = task_key(spec);
        let mut tasks = self.tasks.lock();
        if tasks.iter().any(|e| e.key == key) {
            return Ok(());
        }
        let task = build_task(spec, &self.profile)?;
        tasks.push(TaskEntry { key, task });
        Ok(())
    }

    /// Run a spec's task on an image.
    pub fn run(
        &self,
        spec: &TaskSpec,
        image: &image::DynamicImage,
    ) -> Result<VisionResult, String> {
        let key = task_key(spec);
        let mut tasks = self.tasks.lock();
        let entry = tasks
            .iter_mut()
            .find(|e| e.key == key)
            .ok_or_else(|| format!("task {key} not loaded (call get_or_load first)"))?;
        entry.task.analyze(image, &spec.params)
    }

    /// Force-reload: drop all loaded tasks. The lock MUST be released
    /// before get_or_load — it re-locks internally, and parking_lot Mutexes
    /// are not reentrant (holding it here deadlocked the whole runtime).
    pub fn reload_all(&self, specs: &[TaskSpec]) -> Result<(), String> {
        {
            let mut tasks = self.tasks.lock();
            tasks.clear();
        }
        for spec in specs {
            self.get_or_load(spec)?;
        }
        Ok(())
    }

    /// Status snapshot: one entry per loaded task.
    pub fn status(&self) -> Vec<serde_json::Value> {
        self.tasks
            .lock()
            .iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "kind": e.task.kind().as_str(),
                    "accelerator": e.task.accelerator(),
                })
            })
            .collect()
    }
}

/// Cache key: kind + model + device — enough to distinguish sessions.
fn task_key(spec: &TaskSpec) -> String {
    let model = spec
        .params
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("default");
    let device = spec.device.as_deref().unwrap_or("auto");
    format!("{}|{}|{}", spec.kind.as_str(), model, device)
}

/// Instantiate a task. detect is live; the other families land in
/// follow-up batches (each with its golden regression tests) — see README.
fn build_task(
    spec: &TaskSpec,
    profile: &vision_common::accel::HardwareProfile,
) -> Result<Box<dyn VisionTask>, String> {
    match spec.kind {
        TaskKind::Detect => Ok(Box::new(crate::detect::DetectTask::load(
            &spec.params,
            profile,
            spec.device.as_deref(),
        )?)),
        TaskKind::Ocr | TaskKind::Face | TaskKind::Ground | TaskKind::Vlm => Err(format!(
            "task '{}' ships in the next vision-hub batch; this build carries detect",
            spec.kind.as_str()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_parse_aliases() {
        assert_eq!(TaskKind::parse("detect"), Some(TaskKind::Detect));
        assert_eq!(TaskKind::parse("YOLO"), Some(TaskKind::Detect));
        assert_eq!(TaskKind::parse("locate"), Some(TaskKind::Ground));
        assert_eq!(TaskKind::parse("nope"), None);
    }

    #[test]
    fn task_key_distinguishes_models() {
        let a = serde_json::from_str::<TaskSpec>(
            r#"{"type":"detect","params":{"model":"yolo11n.onnx"}}"#,
        )
        .unwrap();
        let b = serde_json::from_str::<TaskSpec>(
            r#"{"type":"detect","params":{"model":"custom.onnx"}}"#,
        )
        .unwrap();
        let c = serde_json::from_str::<TaskSpec>(r#"{"type":"detect"}"#).unwrap();
        assert_ne!(task_key(&a), task_key(&b));
        assert_ne!(task_key(&a), task_key(&c));
        assert_eq!(task_key(&a), task_key(&a));
    }

    #[test]
    fn reload_all_does_not_deadlock() {
        // Regression: reload_all used to hold the tasks lock while calling
        // get_or_load (which re-locks) — parking_lot is not reentrant, so
        // this hung the whole extension runtime. Run it in a thread with a
        // timeout so a regression FAILS instead of hanging the suite.
        let reg = TaskRegistry::new(Arc::new(
            vision_common::accel::HardwareProfile::default(),
        ));
        let specs = vec![TaskSpec {
            kind: TaskKind::Detect,
            params: serde_json::json!({"model": "definitely-missing.onnx"}),
            device: None,
        }];
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Missing model -> Err is fine; a HANG is the bug.
            let _ = reg.reload_all(&specs);
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "reload_all deadlocked"
        );
    }

    #[test]
    fn hint_mapping() {
        let reg = TaskRegistry::new(Arc::new(vision_common::accel::HardwareProfile::default()));
        assert_eq!(reg.hint_for(&None), DeviceHint::Auto);
        assert_eq!(reg.hint_for(&Some("cpu".into())), DeviceHint::Cpu);
        assert_eq!(reg.hint_for(&Some("CUDA".into())), DeviceHint::Cuda(0));
        assert_eq!(reg.hint_for(&Some("trt".into())), DeviceHint::TensorRt(0));
    }

    #[test]
    fn task_spec_serde_roundtrip() {
        let spec: TaskSpec = serde_json::from_str(
            r#"{"type":"detect","params":{"confidence":0.4},"device":"coreml"}"#,
        )
        .unwrap();
        assert_eq!(spec.kind, TaskKind::Detect);
        assert_eq!(spec.device.as_deref(), Some("coreml"));
        let back = serde_json::to_value(&spec).unwrap();
        assert_eq!(back["type"], "detect");
    }

    #[test]
    fn unbuilt_families_error_clearly() {
        let profile = vision_common::accel::HardwareProfile::default();
        let spec = TaskSpec {
            kind: TaskKind::Ocr,
            params: serde_json::json!({}),
            device: None,
        };
        let err = match build_task(&spec, &profile) {
            Err(e) => e,
            Ok(_) => panic!("ocr should not build in this batch"),
        };
        assert!(err.contains("next vision-hub batch"));
    }
}
