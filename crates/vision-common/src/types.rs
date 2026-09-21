//! Unified vision result types — the one schema every task emits and every
//! consumer (LLM tools, dashboards, rules engine, events) reads.

use serde::{Deserialize, Serialize};

/// Axis-aligned bounding box. `x`/`y` is the TOP-LEFT corner in original
/// image pixel coordinates; `w`/`h` are extents.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl BBox {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Build from (xmin, ymin, xmax, ymax).
    pub fn from_xyxy(xmin: f32, ymin: f32, xmax: f32, ymax: f32) -> Self {
        Self {
            x: xmin,
            y: ymin,
            w: (xmax - xmin).max(0.0),
            h: (ymax - ymin).max(0.0),
        }
    }

    pub fn xmin(&self) -> f32 {
        self.x
    }
    pub fn ymin(&self) -> f32 {
        self.y
    }
    pub fn xmax(&self) -> f32 {
        self.x + self.w
    }
    pub fn ymax(&self) -> f32 {
        self.y + self.h
    }

    pub fn area(&self) -> f32 {
        self.w.max(0.0) * self.h.max(0.0)
    }

    /// Intersection-over-Union with another box.
    pub fn iou(&self, other: &BBox) -> f32 {
        let ix = self.x.max(other.x);
        let iy = self.y.max(other.y);
        let ix2 = self.xmax().min(other.xmax());
        let iy2 = self.ymax().min(other.ymax());
        let iw = (ix2 - ix).max(0.0);
        let ih = (iy2 - iy).max(0.0);
        let inter = iw * ih;
        let union = self.area() + other.area() - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }

    /// Clip to image bounds. Non-finite coordinates (NaN/inf from a
    /// corrupt tensor) collapse to 0 instead of poisoning min/max chains.
    pub fn clamp_to(&self, width: u32, height: u32) -> BBox {
        let sane = |v: f32, max: u32| {
            if v.is_finite() {
                v.clamp(0.0, max as f32)
            } else {
                0.0
            }
        };
        let x = sane(self.x, width);
        let y = sane(self.y, height);
        let x2 = sane(self.xmax(), width);
        let y2 = sane(self.ymax(), height);
        BBox::from_xyxy(x, y, x2.max(x), y2.max(y))
    }
}

/// A single detection (object, face, text region, grounded region…).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub label: String,
    pub class_id: Option<usize>,
    pub confidence: f32,
    pub bbox: BBox,
    /// Task-specific payload (e.g. OCR text, face name, polygon points).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<serde_json::Value>,
}

/// A keypoint (pose landmark, face landmark…).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Keypoint {
    pub x: f32,
    pub y: f32,
    pub confidence: f32,
    pub id: usize,
}

/// The task-agnostic result of analyzing one image. Every vision task
/// converts its raw output into this shape so downstream consumers never
/// need to know which model family produced it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionResult {
    /// Task kind: "detect" | "ocr" | "face" | "ground" | "vlm" | …
    pub task: String,
    pub detections: Vec<Detection>,
    /// Free-form text answer (VLM) or transcript (OCR full text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub width: u32,
    pub height: u32,
    pub inference_ms: f64,
    /// Which accelerator actually ran the model ("cpu" | "cuda:0" | "coreml" | "remote").
    pub accelerator: String,
}

impl VisionResult {
    pub fn new(task: &str, width: u32, height: u32) -> Self {
        Self {
            task: task.to_string(),
            detections: Vec::new(),
            text: None,
            width,
            height,
            inference_ms: 0.0,
            accelerator: "cpu".to_string(),
        }
    }

    /// Compact JSON summary for LLM tool output (no images, bounded size).
    pub fn to_tool_json(&self) -> serde_json::Value {
        serde_json::json!({
            "task": self.task,
            "count": self.detections.len(),
            "detections": self.detections.iter().map(|d| serde_json::json!({
                "label": d.label,
                "confidence": (d.confidence * 100.0).round() as f64 / 100.0,
                "bbox": [d.bbox.x.round(), d.bbox.y.round(), d.bbox.w.round(), d.bbox.h.round()],
            })).collect::<Vec<_>>(),
            "text": self.text,
            "inference_ms": self.inference_ms as u64,
            "accelerator": self.accelerator,
        })
    }
}

/// Where an image comes from. Uniform config-level enum covering the four
/// sources the old extensions each implemented separately.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// A camera device metric frame (NE101/NE301 push base64 via MQTT →
    /// the hub receives `DeviceMetric` events and matches this binding).
    Device { device_id: String, metric: String },
    /// A media stream: rtsp:// rtmp:// hls:// file:// http(s):// camera://
    Stream { url: String },
    /// One-shot user/agent upload (base64 or data-URL, passed as command arg).
    Upload,
    /// A URL reference (http(s):// or the platform-internal /api/images/…).
    Url { url: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_iou_identical() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        assert!((a.iou(&a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bbox_iou_disjoint() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(20.0, 20.0, 5.0, 5.0);
        assert!(a.iou(&b).abs() < 1e-6);
    }

    #[test]
    fn bbox_iou_half_overlap() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(5.0, 0.0, 10.0, 10.0);
        // inter = 5*10 = 50, union = 100 + 100 - 50 = 150
        assert!((a.iou(&b) - 50.0 / 150.0).abs() < 1e-5);
    }

    #[test]
    fn bbox_from_xyxy_and_clamp() {
        let b = BBox::from_xyxy(-5.0, 2.0, 30.0, 12.0).clamp_to(20, 10);
        assert_eq!(b, BBox::new(0.0, 2.0, 20.0, 8.0));
    }

    #[test]
    fn image_source_serde_roundtrip() {
        let s = ImageSource::Device {
            device_id: "NE301-1".into(),
            metric: "image".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: ImageSource = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
        assert!(j.contains(r#""type":"device""#));
    }

    #[test]
    fn vision_result_tool_json_shape() {
        let mut r = VisionResult::new("detect", 640, 480);
        r.detections.push(Detection {
            label: "person".into(),
            class_id: Some(0),
            confidence: 0.91234,
            bbox: BBox::new(1.0, 2.0, 3.0, 4.0),
            attrs: None,
        });
        let j = r.to_tool_json();
        assert_eq!(j["count"], 1);
        assert_eq!(j["detections"][0]["label"], "person");
        assert_eq!(j["detections"][0]["confidence"], 0.91);
    }
}
