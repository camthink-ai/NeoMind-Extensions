//! Detect task — YOLO detection via vision-common's direct-ort wrapper
//! (the ported usls decode path). Lazy model load with auto device plan.

use std::path::PathBuf;

use serde_json::Value;

use vision_common::accel::{plan, HardwareProfile};
use vision_common::models::{find_model, YoloDetector, YoloLayout};
use vision_common::VisionResult;

use crate::task::{TaskKind, VisionTask};

pub struct DetectTask {
    detector: YoloDetector,
}

impl DetectTask {
    /// Load from `params`: `model` (filename or alias, default yolo11n),
    /// `confidence` (default 0.25), `iou` (default 0.45), `input_size`
    /// (default 640).
    pub fn load(params: &Value, profile: &HardwareProfile, device: Option<&str>) -> Result<Self, String> {
        let model_name = params
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("yolo11n");
        let confidence = params.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.25) as f32;
        let iou = params.get("iou").and_then(|c| c.as_f64()).unwrap_or(0.45) as f32;
        let input_size = params.get("input_size").and_then(|c| c.as_u64()).unwrap_or(640) as u32;

        let path: PathBuf = find_model(model_name)
            .ok_or_else(|| format!(
                "model '{model_name}' not found — expected under NEOMIND_EXTENSION_DIR/models (bundled: yolo11n.onnx)"
            ))?;

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let layout = params
            .get("layout")
            .and_then(|l| l.as_str())
            .and_then(layout_from_str)
            .or_else(|| YoloLayout::from_filename(&file_name))
            .unwrap_or(YoloLayout::V8);

        let hint = crate::task::parse_device_hint(device);
        let plan = plan(profile, &hint);
        tracing::info!(
            "[vision-hub:detect] loading {} ({:?}) on {} [{}]",
            path.display(),
            layout,
            plan.accelerator_str(),
            plan.notes.join("; ")
        );

        let detector = YoloDetector::load(&path, plan, layout, input_size, confidence, iou, Vec::new())?;
        Ok(Self { detector })
    }
}

impl VisionTask for DetectTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Detect
    }

    fn accelerator(&self) -> String {
        self.detector.accelerator()
    }

    fn analyze(&mut self, image: &image::DynamicImage, params: &Value) -> Result<VisionResult, String> {
        let start = std::time::Instant::now();
        let detections = self.detector.detect(image)?;
        let mut result = VisionResult::new("detect", image.width(), image.height());
        result.detections = detections;
        result.inference_ms = start.elapsed().as_secs_f64() * 1000.0;
        result.accelerator = self.detector.accelerator();

        // Optional label filter (class names to keep).
        if let Some(keep) = params.get("labels").and_then(|l| l.as_array()) {
            let keep: Vec<String> = keep
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            result.detections.retain(|d| keep.contains(&d.label));
        }
        Ok(result)
    }
}

/// Parse an explicit `layout` param ("v5" | "v8" | "v10").
fn layout_from_str(s: &str) -> Option<YoloLayout> {
    let v: u8 = s.trim_start_matches('v').parse().ok()?;
    YoloLayout::from_version(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_param_parse() {
        assert_eq!(layout_from_str("v8"), Some(YoloLayout::V8));
        assert_eq!(layout_from_str("5"), Some(YoloLayout::V5));
        assert_eq!(layout_from_str("v10"), Some(YoloLayout::V10));
        assert_eq!(layout_from_str("v99"), None);
    }
}
