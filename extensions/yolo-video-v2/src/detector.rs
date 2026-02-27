//! YOLOv11 ONNX Detector Implementation (V2)
//!
//! Real-time object detection using YOLOv11n ONNX model.
//! Part of the unified NeoMind Extension SDK.

use image::{Rgb, RgbImage};
use ndarray::Array;

use crate::{BoundingBox, COCO_CLASSES, MODEL_CONFIG};

#[cfg(not(target_arch = "wasm32"))]
use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::DynValue,
};

/// Detection result
#[derive(Debug, Clone)]
pub struct Detection {
    pub class_id: u32,
    pub class_name: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
}

/// YOLOv11 detector with ONNX runtime
pub struct YoloDetector {
    #[cfg(not(target_arch = "wasm32"))]
    session: Option<std::sync::Arc<parking_lot::Mutex<Session>>>,
    #[cfg(target_arch = "wasm32")]
    model_loaded: bool,
    model_size: usize,
    input_size: u32,
    num_classes: usize,
}

impl YoloDetector {
    /// Create a new detector by loading the model
    pub fn new() -> Result<Self, String> {
        let model_data = Self::load_model_data()?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            if model_data.is_none() {
                tracing::warn!("YOLOv11n model not found, running in demo mode");
                return Ok(Self {
                    session: None,
                    model_size: 0,
                    input_size: MODEL_CONFIG.input_size,
                    num_classes: MODEL_CONFIG.num_classes,
                });
            }

            let model_bytes = model_data.unwrap();
            let model_size = model_bytes.len();

            tracing::info!("Loading YOLOv11n model ({} bytes)...", model_size);

            // Build session
            let session = Session::builder()
                .map_err(|e| format!("Failed to create session builder: {}", e))?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| format!("Failed to set optimization level: {}", e))?
                .with_intra_threads(2)
                .map_err(|e| format!("Failed to set intra threads: {}", e))?
                .commit_from_memory(&model_bytes)
                .map_err(|e| format!("Failed to load model: {}", e))?;

            tracing::info!("YOLOv11n model loaded successfully");

            Ok(Self {
                session: Some(std::sync::Arc::new(parking_lot::Mutex::new(session))),
                model_size,
                input_size: MODEL_CONFIG.input_size,
                num_classes: MODEL_CONFIG.num_classes,
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            if model_data.is_some() {
                tracing::warn!("ONNX not available in WASM, running in demo mode");
            } else {
                tracing::warn!("YOLOv11n model not found, running in demo mode");
            }
            Ok(Self {
                model_loaded: false,
                model_size: 0,
                input_size: MODEL_CONFIG.input_size,
                num_classes: MODEL_CONFIG.num_classes,
            })
        }
    }

    /// Load model data from disk
    fn load_model_data() -> Result<Option<Vec<u8>>, String> {
        // Try loading from models directory relative to extension binary
        if let Ok(mut model_path) = std::env::current_exe() {
            model_path.pop();
            model_path.push("models");
            model_path.push("yolo11n.onnx");

            if model_path.exists() {
                tracing::info!("Loading YOLOv11n model from: {}", model_path.display());
                return std::fs::read(&model_path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            }
        }

        // Try extension source models directory (for development)
        #[cfg(debug_assertions)]
        {
            let dev_paths = [
                "../models/yolo11n.onnx",
                "../../models/yolo11n.onnx",
                "models/yolo11n.onnx",
            ];

            for path in &dev_paths {
                if std::path::Path::new(path).exists() {
                    tracing::info!("Loading YOLOv11n model from development path: {}", path);
                    return std::fs::read(path)
                        .map(Some)
                        .map_err(|e| format!("Failed to read model: {}", e));
                }
            }
        }

        Ok(None)
    }

    /// Check if model is loaded
    pub fn is_loaded(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.session.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.model_loaded
        }
    }

    /// Get model size in bytes
    pub fn model_size(&self) -> usize {
        self.model_size
    }

    /// Run inference on an image
    pub fn detect(&self, image: &RgbImage, confidence_threshold: f32, max_detections: u32) -> Vec<Detection> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(session) = &self.session {
                return self.run_inference(session, image, confidence_threshold, max_detections);
            }
        }

        tracing::debug!("Model not loaded, returning empty detections");
        Vec::new()
    }

    /// Run ONNX inference
    #[cfg(not(target_arch = "wasm32"))]
    fn run_inference(
        &self,
        session: &std::sync::Arc<parking_lot::Mutex<Session>>,
        image: &RgbImage,
        confidence_threshold: f32,
        max_detections: u32,
    ) -> Vec<Detection> {
        let start = std::time::Instant::now();

        // Preprocess image
        let (tensor_data, scale, pad) = self.preprocess_image(image);

        // Create ndarray for input
        let input_array = Array::from_shape_vec(
            (1, 3, self.input_size as usize, self.input_size as usize),
            tensor_data,
        )
        .expect("Failed to create input array")
        .into_dyn();

        // Create input tensor
        let input_tensor = match ort::value::Tensor::from_array(input_array) {
            Ok(tensor) => tensor,
            Err(e) => {
                tracing::error!("Failed to create input tensor: {}", e);
                return Vec::new();
            }
        };

        // Run inference
        let detections = {
            let mut session_guard = session.lock();
            let input_value = ort::session::input::SessionInputValue::Owned(input_tensor.into());
            let inputs = [input_value];

            let outputs = match session_guard.run(inputs) {
                Ok(outputs) => outputs,
                Err(e) => {
                    tracing::error!("ONNX inference failed: {}", e);
                    return Vec::new();
                }
            };

            if outputs.len() > 0 {
                let output = &outputs[0];
                self.postprocess(output, image.width(), image.height(), scale, pad, confidence_threshold, max_detections)
            } else {
                tracing::error!("No output from ONNX session");
                Vec::new()
            }
        };

        let elapsed = start.elapsed();
        tracing::debug!("Inference took {:?}, found {} detections", elapsed, detections.len());

        detections
    }

    /// Preprocess image for YOLO model
    fn preprocess_image(&self, image: &RgbImage) -> (Vec<f32>, f32, (f32, f32)) {
        let model_size = self.input_size as f32;
        let img_w = image.width() as f32;
        let img_h = image.height() as f32;

        // Calculate scale and padding
        let scale = (model_size / img_w).min(model_size / img_h);
        let new_w = (img_w * scale).round() as u32;
        let new_h = (img_h * scale).round() as u32;
        let pad_w = ((model_size as u32 - new_w) / 2) as f32;
        let pad_h = ((model_size as u32 - new_h) / 2) as f32;

        // Resize image
        let resized = image::imageops::resize(
            image,
            new_w,
            new_h,
            image::imageops::FilterType::Triangle,
        );

        // Create padded image
        let mut padded = RgbImage::from_pixel(self.input_size, self.input_size, Rgb([114, 114, 114]));
        image::imageops::overlay(&mut padded, &resized, pad_w as i64, pad_h as i64);

        // Convert to normalized CHW format
        let chw: Vec<f32> = padded
            .pixels()
            .flat_map(|p| {
                [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]
            })
            .collect();

        (chw, scale, (pad_w, pad_h))
    }

    /// Postprocess model output to detections
    #[allow(clippy::too_many_arguments)]
    #[cfg(not(target_arch = "wasm32"))]
    fn postprocess(
        &self,
        output: &DynValue,
        orig_w: u32,
        orig_h: u32,
        scale: f32,
        pad: (f32, f32),
        confidence_threshold: f32,
        max_detections: u32,
    ) -> Vec<Detection> {
        // Extract output tensor data
        let output_data = match output.try_extract_tensor::<f32>() {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Failed to extract output tensor: {}", e);
                return Vec::new();
            }
        };

        let data = output_data.1;
        let num_predictions = 8400;
        let num_classes = self.num_classes;

        let mut detections = Vec::new();

        for i in 0..num_predictions {
            let offset = i * (4 + num_classes);

            if offset + 4 + num_classes > data.len() {
                break;
            }

            // Extract bbox
            let x_center = data[offset] * self.input_size as f32;
            let y_center = data[offset + 1] * self.input_size as f32;
            let width = data[offset + 2] * self.input_size as f32;
            let height = data[offset + 3] * self.input_size as f32;

            // Find max class score
            let mut max_class_score = 0.0f32;
            let mut max_class_id = 0u32;

            for class_id in 0..num_classes {
                let score = data[offset + 4 + class_id];
                if score > max_class_score {
                    max_class_score = score;
                    max_class_id = class_id as u32;
                }
            }

            if max_class_score < confidence_threshold {
                continue;
            }

            // Convert coordinates
            let x1 = ((x_center - width / 2.0 - pad.0) / scale).max(0.0);
            let y1 = ((y_center - height / 2.0 - pad.1) / scale).max(0.0);
            let x2 = ((x_center + width / 2.0 - pad.0) / scale).min(orig_w as f32);
            let y2 = ((y_center + height / 2.0 - pad.1) / scale).min(orig_h as f32);

            let bbox_width = (x2 - x1).max(1.0);
            let bbox_height = (y2 - y1).max(1.0);

            if bbox_width < 5.0 || bbox_height < 5.0 {
                continue;
            }

            detections.push(Detection {
                class_id: max_class_id,
                class_name: COCO_CLASSES.get(max_class_id as usize)
                    .unwrap_or(&"unknown")
                    .to_string(),
                confidence: max_class_score,
                bbox: BoundingBox {
                    x: x1,
                    y: y1,
                    width: bbox_width,
                    height: bbox_height,
                },
            });
        }

        // Apply NMS
        let detections = self.nms(detections, 0.45);

        // Limit to max detections
        detections.into_iter().take(max_detections as usize).collect()
    }

    /// Non-Maximum Suppression
    fn nms(&self, mut detections: Vec<Detection>, iou_threshold: f32) -> Vec<Detection> {
        if detections.is_empty() {
            return detections;
        }

        detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        let mut result = Vec::new();
        let mut suppressed = vec![false; detections.len()];

        for i in 0..detections.len() {
            if suppressed[i] {
                continue;
            }

            result.push(detections[i].clone());

            for j in (i + 1)..detections.len() {
                if suppressed[j] {
                    continue;
                }

                if detections[i].class_id != detections[j].class_id {
                    continue;
                }

                let iou = Self::calculate_iou(&detections[i].bbox, &detections[j].bbox);

                if iou > iou_threshold {
                    suppressed[j] = true;
                }
            }
        }

        result
    }

    /// Calculate IoU between two bounding boxes
    fn calculate_iou(box1: &BoundingBox, box2: &BoundingBox) -> f32 {
        let x1 = box1.x.max(box2.x);
        let y1 = box1.y.max(box2.y);
        let x2 = (box1.x + box1.width).min(box2.x + box2.width);
        let y2 = (box1.y + box1.height).min(box2.y + box2.height);

        let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        let area1 = box1.width * box1.height;
        let area2 = box2.width * box2.height;
        let union = area1 + area2 - intersection;

        if union > 0.0 {
            intersection / union
        } else {
            0.0
        }
    }
}

impl Default for YoloDetector {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            tracing::error!("Failed to create YoloDetector: {}", e);
            #[cfg(not(target_arch = "wasm32"))]
            {
                Self {
                    session: None,
                    model_size: 0,
                    input_size: MODEL_CONFIG.input_size,
                    num_classes: MODEL_CONFIG.num_classes,
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                Self {
                    model_loaded: false,
                    model_size: 0,
                    input_size: MODEL_CONFIG.input_size,
                    num_classes: MODEL_CONFIG.num_classes,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_creation() {
        let detector = YoloDetector::new();
        assert!(detector.is_ok());
    }

    #[test]
    fn test_iou_calculation() {
        let box1 = BoundingBox { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
        let box2 = BoundingBox { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };

        let iou = YoloDetector::calculate_iou(&box1, &box2);
        assert!((iou - 1.0).abs() < 0.001);

        let box3 = BoundingBox { x: 100.0, y: 100.0, width: 100.0, height: 100.0 };
        let iou2 = YoloDetector::calculate_iou(&box1, &box3);
        assert_eq!(iou2, 0.0);
    }

    #[test]
    fn test_nms() {
        let detector = YoloDetector::default();

        let detections = vec![
            Detection {
                class_id: 0,
                class_name: "person".to_string(),
                confidence: 0.95,
                bbox: BoundingBox { x: 10.0, y: 10.0, width: 100.0, height: 100.0 },
            },
            Detection {
                class_id: 0,
                class_name: "person".to_string(),
                confidence: 0.85,
                bbox: BoundingBox { x: 15.0, y: 15.0, width: 100.0, height: 100.0 },
            },
            Detection {
                class_id: 1,
                class_name: "car".to_string(),
                confidence: 0.90,
                bbox: BoundingBox { x: 200.0, y: 200.0, width: 100.0, height: 100.0 },
            },
        ];

        let result = detector.nms(detections, 0.5);
        // Should suppress overlapping person detection
        assert_eq!(result.len(), 2);
    }
}
