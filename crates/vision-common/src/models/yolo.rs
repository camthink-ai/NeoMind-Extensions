//! YOLO object detection — direct-ort port of the usls `models::YOLO`
//! detection path (MIT, © 2024 Jamjamjon). Covers the detection layouts:
//!
//! | Layout | Versions | Output shape (logical) | NMS |
//! |--------|----------|------------------------|-----|
//! | [`YoloLayout::V8`]  | v8/v9/v11/v12/v13 | `[1, 4+nc, N]` cx,cy,w,h + class scores | yes |
//! | [`YoloLayout::V5`]  | v5/v6/v7 | `[N, 5+nc]` cx,cy,w,h,obj + class scores | yes |
//! | [`YoloLayout::V10`] | v10 | `[N, 6]` x1,y1,x2,y2,conf,cls (NMS-free) | no |
//!
//! NMS is class-agnostic by default — matching usls exactly, which the
//! golden regression tests (vs. the old extensions) rely on.

use std::path::{Path, PathBuf};

use image::DynamicImage;

use crate::accel::{DeviceHint, DevicePlan};
use crate::engine::{OrtSession, OutputTensor};
use crate::image::{fit_adaptive, rgb_to_nchw_f32};
use crate::types::{BBox, Detection};

/// COCO-80 class names (shared by all COCO-pretrained YOLO models).
pub const COCO_CLASSES: [&str; 80] = [
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat",
    "traffic light", "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat",
    "dog", "horse", "sheep", "cow", "elephant", "bear", "zebra", "giraffe", "backpack",
    "umbrella", "handbag", "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball",
    "kite", "baseball bat", "baseball glove", "skateboard", "surfboard", "tennis racket",
    "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
    "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake",
    "chair", "couch", "potted plant", "bed", "dining table", "toilet", "tv", "laptop",
    "mouse", "remote", "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
    "refrigerator", "book", "clock", "vase", "scissors", "teddy bear", "hair drier", "toothbrush",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YoloLayout {
    /// v8/v9/v11/v12/v13: `[1, 4+nc, N]`, anchors-last, argmax over classes.
    V8,
    /// v5/v6/v7: `[N, 5+nc]`, objectness × class score.
    V5,
    /// v10: `[N, 6]` xyxy + conf + class-id, NMS-free.
    V10,
}

impl YoloLayout {
    /// Infer from a model version number (5, 6, 7, 8, 9, 10, 11, 12, 13).
    pub fn from_version(v: u8) -> Option<Self> {
        match v {
            5 | 6 | 7 => Some(YoloLayout::V5),
            8 | 9 | 11 | 12 | 13 => Some(YoloLayout::V8),
            10 => Some(YoloLayout::V10),
            _ => None,
        }
    }

    /// Guess from a model filename (`yolo11n.onnx` → V8, `yolov5s.onnx` → V5).
    pub fn from_filename(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        let digits: String = lower
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let v: u8 = digits.parse().ok()?;
        // "yolo11n" → 11 (V8); "yolov8n" → skip 'v' handled by digit scan of "8"
        Self::from_version(v)
    }
}

/// Class-agnostic NMS, byte-compatible with usls `NmsOps` (sort desc by
/// score, greedily keep boxes whose IoU with every kept box ≤ threshold).
pub fn nms(detections: &mut Vec<Detection>, iou_threshold: f32) {
    detections.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut keep = 0;
    for i in 0..detections.len() {
        let mut drop = false;
        for k in 0..keep {
            if detections[k].bbox.iou(&detections[i].bbox) > iou_threshold {
                drop = true;
                break;
            }
        }
        if !drop {
            detections.swap(keep, i);
            keep += 1;
        }
    }
    detections.truncate(keep);
}

/// Pure decode: raw output tensor → detections in ORIGINAL image coords.
/// `scale` is the letterbox scale (original = processed / scale).
#[allow(clippy::too_many_arguments)]
pub fn decode_detections(
    output: &OutputTensor,
    layout: YoloLayout,
    num_classes: usize,
    names: &[&str],
    conf_threshold: f32,
    scale: f32,
    orig_size: (u32, u32),
    iou_threshold: f32,
) -> Vec<Detection> {
    // Flatten to logical [rows, cols] (anchors × features), anchors-first.
    let data = &output.data;
    let (rows, cols): (usize, usize) = match layout {
        YoloLayout::V8 => {
            // raw [1, C, N] — a shape/product mismatch means the model's
            // output layout isn't what we assume (e.g. a transposed export);
            // say so instead of silently decoding zeros into "no detections".
            let c_dim = output.shape.get(1).copied().unwrap_or(0) as usize;
            let n_dim = output.shape.get(2).copied().unwrap_or(0) as usize;
            let declared: usize = output.shape.iter().map(|d| (*d).max(0) as usize).product();
            if c_dim == 0
                || n_dim == 0
                || declared != output.data.len()
                || output.shape.first().copied().unwrap_or(0) != 1
            {
                tracing::warn!(
                    shape = ?output.shape,
                    data_len = output.data.len(),
                    "V8 output shape does not look like [1, 4+nc, N] —                      check the model export or set an explicit layout"
                );
            }
            (n_dim.max(1), c_dim.max(1))
        }
        YoloLayout::V5 | YoloLayout::V10 => {
            // raw [N, C] (or [1, N, C]) — rows already anchors-first
            if output.shape.len() == 2 {
                (output.shape[0] as usize, output.shape[1] as usize)
            } else {
                (
                    output.shape.get(1).copied().unwrap_or(0) as usize,
                    output.shape.get(2).copied().unwrap_or(0) as usize,
                )
            }
        }
    };
    // Element accessor: V8 is transposed (col-major), V5/V10 row-major.
    let at: Box<dyn Fn(usize, usize) -> f32 + '_> = match layout {
        YoloLayout::V8 => {
            let n = rows;
            Box::new(move |a, c| data.get(c * n + a).copied().unwrap_or(0.0))
        }
        YoloLayout::V5 | YoloLayout::V10 => {
            let stride = cols;
            Box::new(move |a, f| data.get(a * stride + f).copied().unwrap_or(0.0))
        }
    };

    let mut dets = Vec::new();
    let (orig_w, orig_h) = orig_size;

    for a in 0..rows {
        let (class_id, confidence, bbox_pre) = match layout {
            YoloLayout::V8 => {
                // cols: cx, cy, w, h, scores[nc]
                if 4 + num_classes > cols {
                    break;
                }
                let (cx, cy, w, h) = (at(a, 0), at(a, 1), at(a, 2), at(a, 3));
                let (mut best, mut best_conf) = (0usize, f32::MIN);
                for c in 0..num_classes {
                    let s = at(a, 4 + c);
                    if s > best_conf {
                        best_conf = s;
                        best = c;
                    }
                }
                (best, best_conf, (cx - w / 2.0, cy - h / 2.0, w, h))
            }
            YoloLayout::V5 => {
                // cols: cx, cy, w, h, obj, scores[nc]
                if 5 + num_classes > cols {
                    break;
                }
                let (cx, cy, w, h) = (at(a, 0), at(a, 1), at(a, 2), at(a, 3));
                let obj = at(a, 4);
                let (mut best, mut best_conf) = (0usize, f32::MIN);
                for c in 0..num_classes {
                    let s = at(a, 5 + c);
                    if s > best_conf {
                        best_conf = s;
                        best = c;
                    }
                }
                (best, best_conf * obj, (cx - w / 2.0, cy - h / 2.0, w, h))
            }
            YoloLayout::V10 => {
                // cols: x1, y1, x2, y2, conf, cls
                if cols < 6 {
                    break;
                }
                let (x1, y1, x2, y2) = (at(a, 0), at(a, 1), at(a, 2), at(a, 3));
                let conf = at(a, 4);
                let cls_raw = at(a, 5);
                if !(cls_raw >= 0.0) {
                    continue; // negative/NaN class id — corrupt row
                }
                (cls_raw as usize, conf, (x1, y1, x2 - x1, y2 - y1))
            }
        };

        if !(confidence.is_finite()) || confidence < conf_threshold {
            continue; // NaN compares false against < — must filter explicitly
        }

        let bbox = BBox::new(bbox_pre.0, bbox_pre.1, bbox_pre.2, bbox_pre.3);
        let bbox = if scale > 0.0 {
            BBox::from_xyxy(
                bbox.xmin() / scale,
                bbox.ymin() / scale,
                bbox.xmax() / scale,
                bbox.ymax() / scale,
            )
            .clamp_to(orig_w, orig_h)
        } else {
            bbox
        };

        let label = names
            .get(class_id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("class_{class_id}"));

        dets.push(Detection {
            label,
            class_id: Some(class_id),
            confidence,
            bbox,
            attrs: None,
        });
    }

    if layout != YoloLayout::V10 {
        nms(&mut dets, iou_threshold);
    }
    dets
}

/// Loaded YOLO detector (detection task only).
pub struct YoloDetector {
    session: OrtSession,
    layout: YoloLayout,
    names: Vec<String>,
    num_classes: usize,
    confidence: f32,
    iou: f32,
    input_size: u32,
}

impl YoloDetector {
    /// Load a model. `names` empty ⇒ COCO-80.
    pub fn load(
        model_path: &Path,
        plan: DevicePlan,
        layout: YoloLayout,
        input_size: u32,
        confidence: f32,
        iou: f32,
        names: Vec<String>,
    ) -> Result<Self, String> {
        let names = if names.is_empty() {
            COCO_CLASSES.iter().map(|s| s.to_string()).collect()
        } else {
            names
        };
        let num_classes = names.len();
        let session = OrtSession::from_model(model_path, plan, 3, input_size, input_size)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            session,
            layout,
            names,
            num_classes,
            confidence,
            iou,
            input_size,
        })
    }

    /// Convenience: load with auto plan + layout from filename.
    pub fn load_auto(
        model_path: &Path,
        profile: &crate::accel::HardwareProfile,
        hint: &DeviceHint,
        confidence: f32,
        iou: f32,
    ) -> Result<Self, String> {
        let file_name = model_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let layout = YoloLayout::from_filename(&file_name).unwrap_or(YoloLayout::V8);
        let plan = crate::accel::plan(profile, hint);
        Self::load(model_path, plan, layout, 640, confidence, iou, Vec::new())
    }

    pub fn accelerator(&self) -> String {
        self.session.accelerator_str()
    }

    pub fn plan(&self) -> &DevicePlan {
        &self.session.plan
    }

    /// Detect on one image; returns detections in original coordinates.
    pub fn detect(&mut self, img: &DynamicImage) -> Result<Vec<Detection>, String> {
        let orig = (img.width(), img.height());
        let start = std::time::Instant::now();
        let (canvas, fit) = fit_adaptive(img, self.input_size, self.input_size);
        let input = rgb_to_nchw_f32(&canvas);
        let outputs = self
            .session
            .run_nchw_f32(input)
            .map_err(|e| format!("inference: {e}"))?;
        let out0 = outputs
            .first()
            .ok_or_else(|| "model produced no outputs".to_string())?;
        let names: Vec<&str> = self.names.iter().map(|s| s.as_str()).collect();
        let dets = decode_detections(
            out0,
            self.layout,
            self.num_classes,
            &names,
            self.confidence,
            fit.scale,
            orig,
            self.iou,
        );
        tracing::debug!(
            "[yolo] {} dets in {}ms ({})",
            dets.len(),
            start.elapsed().as_millis(),
            self.accelerator()
        );
        Ok(dets)
    }
}

/// Default model file candidates, searched under `NEOMIND_EXTENSION_DIR/models`,
/// cwd, and a few relative fallbacks (port of the per-extension finders).
pub fn find_model(filename: &str) -> Option<PathBuf> {
    let filename = normalize_model_name(filename);
    if filename.starts_with("__rejected__") {
        return None; // unsafe name — never search the filesystem
    }
    let mut candidates = Vec::new();
    if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
        candidates.push(PathBuf::from(&ext_dir).join("models").join(&filename));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("models").join(&filename));
    }
    for rel in ["models", "../models", "../../models"] {
        candidates.push(PathBuf::from(rel).join(&filename));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Accept "yolov8n", "yolo11n", "yolov8n.onnx", "v8-n" style names and map
/// to the canonical bundled filename.
///
/// SECURITY: the model name reaches here from user-controllable command
/// params — reject anything outside the safe filename charset so it can
/// never escape the models/ directory (`../x`, absolute paths, NTFS
/// streams, …). `PathBuf::join` would happily honor an absolute path.
fn normalize_model_name(input: &str) -> String {
    let lower = input.to_lowercase();
    if lower.chars().any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
        || lower.split('/').count() > 1
        || lower.contains("..")
    {
        tracing::warn!("[yolo] rejecting unsafe model name: {input:?}");
        return format!("__rejected__{}.onnx", lower.len());
    }
    if lower.ends_with(".onnx") {
        return lower;
    }
    // "v8-n" / "v11-n" → ultralytics filenames: yolov8n.onnx, yolo11n.onnx
    // (the "v" was dropped from official names starting at v11)
    if let Some(rest) = lower.strip_prefix('v') {
        if let Some((ver, scale)) = rest.split_once('-') {
            if ver.chars().all(|c| c.is_ascii_digit()) && scale.len() == 1 {
                let Ok(v): Result<u8, _> = ver.parse() else {
                    return format!("{lower}.onnx"); // unknown scheme — as-is,上层报 not found
                };
                if v >= 11 {
                    return format!("yolo{ver}{scale}.onnx");
                }
                return format!("yolov{ver}{scale}.onnx");
            }
        }
    }
    format!("{lower}.onnx")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<&'static str> {
        vec!["cat", "dog", "person"]
    }

    fn make_v8_output(scores: &[(usize, usize, f32)]) -> OutputTensor {
        // shape [1, 4+3, N=8]
        let n = 8usize;
        let c = 7usize;
        let mut data = vec![0f32; n * c];
        for &(anchor, class, score) in scores {
            data[(4 + class) * n + anchor] = score;
        }
        OutputTensor {
            name: "out0".into(),
            shape: vec![1, c as i64, n as i64],
            data,
        }
    }

    #[test]
    fn layout_from_version_and_filename() {
        assert_eq!(YoloLayout::from_version(8), Some(YoloLayout::V8));
        assert_eq!(YoloLayout::from_version(11), Some(YoloLayout::V8));
        assert_eq!(YoloLayout::from_version(5), Some(YoloLayout::V5));
        assert_eq!(YoloLayout::from_version(10), Some(YoloLayout::V10));
        assert_eq!(YoloLayout::from_version(14), None);
        assert_eq!(YoloLayout::from_filename("yolo11n.onnx"), Some(YoloLayout::V8));
        assert_eq!(YoloLayout::from_filename("yolov5s.onnx"), Some(YoloLayout::V5));
        assert_eq!(YoloLayout::from_filename("yolov10x.onnx"), Some(YoloLayout::V10));
    }

    #[test]
    fn decode_v8_argmax_and_unscale() {
        // anchor 2: box at cx=320,cy=320,w=640,h=640 (in 640-space), person(2)=0.9
        let n = 8usize;
        let mut data = vec![0f32; n * 7];
        data[0 * n + 2] = 320.0;
        data[1 * n + 2] = 320.0;
        data[2 * n + 2] = 640.0;
        data[3 * n + 2] = 640.0;
        data[6 * n + 2] = 0.9; // class 2 (person)
        let out = OutputTensor {
            name: "out0".into(),
            shape: vec![1, 7, 8],
            data,
        };
        // scale=2.0 → original box (0,0,320,320) on a 320×320 image
        let dets = decode_detections(&out, YoloLayout::V8, 3, &names(), 0.5, 2.0, (320, 320), 0.45);
        assert_eq!(dets.len(), 1);
        assert_eq!(dets[0].label, "person");
        assert_eq!(dets[0].class_id, Some(2));
        assert!((dets[0].confidence - 0.9).abs() < 1e-6);
        assert_eq!(dets[0].bbox, BBox::new(0.0, 0.0, 320.0, 320.0));
    }

    #[test]
    fn decode_v8_conf_filter() {
        let out = make_v8_output(&[(0, 0, 0.3)]); // below 0.5 threshold
        let dets = decode_detections(&out, YoloLayout::V8, 3, &names(), 0.5, 1.0, (640, 640), 0.45);
        assert!(dets.is_empty());
    }

    #[test]
    fn decode_v5_objectness_multiplies() {
        // [N=2, 5+3] rows: cx,cy,w,h,obj,scores
        let mut data = vec![0f32; 2 * 8];
        // row 0: strong
        data[0..4].copy_from_slice(&[100.0, 100.0, 50.0, 50.0]);
        data[4] = 0.8; // obj
        data[5] = 0.9; // cat
        // row 1: high class score but obj kills it
        data[8 + 0..8 + 4].copy_from_slice(&[10.0, 10.0, 5.0, 5.0]);
        data[8 + 4] = 0.2;
        data[8 + 6] = 0.99; // person
        let out = OutputTensor {
            name: "out0".into(),
            shape: vec![2, 8],
            data,
        };
        let dets = decode_detections(&out, YoloLayout::V5, 3, &names(), 0.5, 1.0, (640, 640), 0.45);
        assert_eq!(dets.len(), 1);
        assert_eq!(dets[0].label, "cat");
        assert!((dets[0].confidence - 0.72).abs() < 1e-6);
    }

    #[test]
    fn decode_v10_no_nms_direct_coords() {
        // [N=2, 6]: x1,y1,x2,y2,conf,cls
        let mut data = vec![0f32; 2 * 6];
        data[0..6].copy_from_slice(&[10.0, 10.0, 110.0, 110.0, 0.8, 1.0]); // dog
        data[6..12].copy_from_slice(&[20.0, 20.0, 120.0, 120.0, 0.7, 0.0]); // cat — overlaps, kept (v10 is NMS-free)
        let out = OutputTensor {
            name: "out0".into(),
            shape: vec![2, 6],
            data,
        };
        let dets = decode_detections(&out, YoloLayout::V10, 3, &names(), 0.5, 1.0, (640, 640), 0.45);
        assert_eq!(dets.len(), 2); // no suppression
        assert_eq!(dets[0].label, "dog");
        assert_eq!(dets[1].label, "cat");
    }

    #[test]
    fn nms_class_agnostic_suppression() {
        let mk = |label: &str, conf: f32, x: f32| Detection {
            label: label.into(),
            class_id: None,
            confidence: conf,
            bbox: BBox::new(x, 0.0, 100.0, 100.0),
            attrs: None,
        };
        // two heavily-overlapping boxes of DIFFERENT classes: usls parity
        // means the lower-scored one is suppressed too.
        let mut dets = vec![mk("person", 0.9, 0.0), mk("dog", 0.8, 5.0), mk("cat", 0.7, 300.0)];
        nms(&mut dets, 0.45);
        assert_eq!(dets.len(), 2);
        assert_eq!(dets[0].label, "person");
        assert_eq!(dets[1].label, "cat");
    }

    #[test]
    fn nms_keeps_disjoint() {
        let mk = |conf: f32, x: f32| Detection {
            label: "a".into(),
            class_id: None,
            confidence: conf,
            bbox: BBox::new(x, 0.0, 10.0, 10.0),
            attrs: None,
        };
        let mut dets = vec![mk(0.9, 0.0), mk(0.8, 100.0), mk(0.7, 200.0)];
        nms(&mut dets, 0.45);
        assert_eq!(dets.len(), 3);
    }

    #[test]
    fn model_name_rejects_traversal() {
        assert_eq!(find_model("../../etc/passwd"), None);
        assert_eq!(find_model("/etc/foo.onnx"), None);
        assert_eq!(find_model("a/b.onnx"), None);
        assert_eq!(find_model("ok-model_1.onnx"), None.or(None)); // 合法名不存在时 None,但不该被拒
        // 合法字符集仍能正常规范化
        assert_eq!(normalize_model_name("v8-n"), "yolov8n.onnx");
    }

    #[test]
    fn v10_nan_and_negative_class_rows_skipped() {
        let mut data = vec![0f32; 2 * 6];
        data[0..6].copy_from_slice(&[10.0, 10.0, 110.0, 110.0, 0.8, f32::NAN]); // NaN cls
        data[6..12].copy_from_slice(&[20.0, 20.0, 120.0, 120.0, f32::NAN, 1.0]); // NaN conf
        let out = OutputTensor { name: "o".into(), shape: vec![2, 6], data };
        let dets = decode_detections(&out, YoloLayout::V10, 3, &names(), 0.5, 1.0, (640, 640), 0.45);
        assert!(dets.is_empty());
    }

    #[test]
    fn model_name_normalization() {
        assert_eq!(normalize_model_name("v8-n"), "yolov8n.onnx");
        assert_eq!(normalize_model_name("v11-n"), "yolo11n.onnx");
        assert_eq!(normalize_model_name("yolov8n"), "yolov8n.onnx");
        assert_eq!(normalize_model_name("custom.onnx"), "custom.onnx");
    }

    #[test]
    fn coco_classes_sane() {
        assert_eq!(COCO_CLASSES.len(), 80);
        assert_eq!(COCO_CLASSES[0], "person");
    }
}
