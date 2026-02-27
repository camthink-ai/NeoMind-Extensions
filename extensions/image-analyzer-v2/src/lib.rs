//! NeoMind Image Analyzer Extension (V2)
//!
//! Image analysis using YOLOv8 object detection.
//! Demonstrates the unified SDK with ABI Version 3.

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionMetadata, ExtensionError, ExtensionMetricValue,
    MetricDescriptor, ExtensionCommand, MetricDataType, ParameterDefinition,
    ParamMetricValue, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashMap;
use semver::Version;

// ============================================================================
// Types
// ============================================================================

/// Image detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub label: String,
    pub confidence: f32,
    pub bbox: Option<BoundingBox>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub objects: Vec<Detection>,
    pub description: String,
    pub processing_time_ms: u64,
}

// ============================================================================
// COCO Classes
// ============================================================================

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

// ============================================================================
// Extension Implementation
// ============================================================================

pub struct ImageAnalyzer {
    images_processed: AtomicU64,
    total_processing_time_ms: AtomicU64,
    detections_found: AtomicU64,
    #[cfg(not(target_arch = "wasm32"))]
    detector: YOLOv8Detector,
}

impl ImageAnalyzer {
    pub fn new() -> Self {
        Self {
            images_processed: AtomicU64::new(0),
            total_processing_time_ms: AtomicU64::new(0),
            detections_found: AtomicU64::new(0),
            #[cfg(not(target_arch = "wasm32"))]
            detector: YOLOv8Detector::new(),
        }
    }

    /// Analyze image data and return detection results
    pub fn analyze_image(&self, data: &[u8]) -> Result<AnalysisResult> {
        let start = std::time::Instant::now();

        #[cfg(not(target_arch = "wasm32"))]
        let (objects, description) = {
            if self.detector.is_loaded() {
                match self.detector.detect(data, 0.25) {
                    Ok(detections) => {
                        let desc = format!("YOLOv8 detected {} objects", detections.len());
                        (detections, desc)
                    }
                    Err(e) => {
                        eprintln!("[ImageAnalyzer] YOLOv8 error: {}", e);
                        self.fallback_analysis(data)
                    }
                }
            } else {
                self.fallback_analysis(data)
            }
        };

        #[cfg(target_arch = "wasm32")]
        let (objects, description) = self.fallback_analysis(data);

        let processing_time = start.elapsed().as_millis() as u64;

        // Update stats
        self.images_processed.fetch_add(1, Ordering::SeqCst);
        self.total_processing_time_ms.fetch_add(processing_time, Ordering::SeqCst);
        self.detections_found.fetch_add(objects.len() as u64, Ordering::SeqCst);

        Ok(AnalysisResult {
            objects,
            description,
            processing_time_ms: processing_time,
        })
    }

    /// Fallback analysis when YOLOv8 is not available
    pub fn fallback_analysis(&self, data: &[u8]) -> (Vec<Detection>, String) {
        let size = data.len();
        let mut objects = Vec::new();

        // Check image format
        if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            objects.push(Detection {
                label: "jpeg_image".to_string(),
                confidence: 0.95,
                bbox: None,
            });
        } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            objects.push(Detection {
                label: "png_image".to_string(),
                confidence: 0.95,
                bbox: None,
            });
        }

        let description = format!(
            "Fallback analysis (YOLOv8 unavailable). Size: {} bytes.",
            size
        );

        (objects, description)
    }

    /// Reset statistics
    pub fn reset_stats(&self) -> serde_json::Value {
        self.images_processed.store(0, Ordering::SeqCst);
        self.total_processing_time_ms.store(0, Ordering::SeqCst);
        self.detections_found.store(0, Ordering::SeqCst);
        serde_json::json!({"status": "reset"})
    }
}

impl Default for ImageAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for ImageAnalyzer {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata {
                id: "image-analyzer-v2".to_string(),
                name: "Image Analyzer V2".to_string(),
                version: Version::parse("2.0.0").unwrap(),
                description: Some("Image analysis with YOLOv8".to_string()),
                author: Some("NeoMind Team".to_string()),
                homepage: None,
                license: Some("Apache-2.0".to_string()),
                file_path: None,
                config_parameters: None,
            }
        })
    }

    fn metrics(&self) -> &[MetricDescriptor] {
        static METRICS: std::sync::OnceLock<Vec<MetricDescriptor>> = std::sync::OnceLock::new();
        METRICS.get_or_init(|| {
            vec![
                MetricDescriptor {
                    name: "images_processed".to_string(),
                    display_name: "Images Processed".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "count".to_string(),
                    min: Some(0.0),
                    max: None,
                    required: false,
                },
                MetricDescriptor {
                    name: "avg_processing_time_ms".to_string(),
                    display_name: "Avg Processing Time".to_string(),
                    data_type: MetricDataType::Float,
                    unit: "ms".to_string(),
                    min: Some(0.0),
                    max: None,
                    required: false,
                },
                MetricDescriptor {
                    name: "total_detections".to_string(),
                    display_name: "Total Detections".to_string(),
                    data_type: MetricDataType::Integer,
                    unit: "count".to_string(),
                    min: Some(0.0),
                    max: None,
                    required: false,
                },
            ]
        })
    }

    fn commands(&self) -> &[ExtensionCommand] {
        static COMMANDS: std::sync::OnceLock<Vec<ExtensionCommand>> = std::sync::OnceLock::new();
        COMMANDS.get_or_init(|| {
            vec![
                ExtensionCommand {
                    name: "analyze_image".to_string(),
                    display_name: "Analyze Image".to_string(),
                    payload_template: String::new(),
                    parameters: vec![
                        ParameterDefinition {
                            name: "image".to_string(),
                            display_name: "Image".to_string(),
                            description: "Base64 encoded image data".to_string(),
                            param_type: MetricDataType::String,
                            required: true,
                            default_value: None,
                            min: None,
                            max: None,
                            options: Vec::new(),
                        },
                    ],
                    fixed_values: HashMap::new(),
                    samples: vec![
                        json!({ "image": "base64_encoded_data" }),
                    ],
                    llm_hints: "Analyze an image and return detected objects".to_string(),
                    parameter_groups: Vec::new(),
                },
                ExtensionCommand {
                    name: "reset_stats".to_string(),
                    display_name: "Reset Statistics".to_string(),
                    payload_template: String::new(),
                    parameters: vec![],
                    fixed_values: HashMap::new(),
                    samples: vec![],
                    llm_hints: "Reset extension statistics".to_string(),
                    parameter_groups: Vec::new(),
                },
            ]
        })
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "analyze_image" => {
                let image_data = args.get("image")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("Missing 'image' parameter".to_string()))?;

                // Decode base64
                let image_bytes = base64_decode(image_data)
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("Base64 decode error: {}", e)))?;

                let result = self.analyze_image(&image_bytes)?;
                Ok(serde_json::to_value(result)
                    .map_err(|e| ExtensionError::ExecutionFailed(e.to_string()))?)
            }
            "reset_stats" => {
                Ok(self.reset_stats())
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now = chrono::Utc::now().timestamp_millis();
        let images = self.images_processed.load(Ordering::SeqCst);
        let total_time = self.total_processing_time_ms.load(Ordering::SeqCst);
        let avg_time = if images > 0 { total_time as f64 / images as f64 } else { 0.0 };

        Ok(vec![
            ExtensionMetricValue {
                name: "images_processed".to_string(),
                value: ParamMetricValue::Integer(images as i64),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "avg_processing_time_ms".to_string(),
                value: ParamMetricValue::Float(avg_time),
                timestamp: now,
            },
            ExtensionMetricValue {
                name: "total_detections".to_string(),
                value: ParamMetricValue::Integer(self.detections_found.load(Ordering::SeqCst) as i64),
                timestamp: now,
            },
        ])
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD
        .decode(input)
        .map_err(|e| format!("Base64 decode error: {}", e))
}

// ============================================================================
// Native YOLOv8 Detector
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    use ort::{
        session::{Session, builder::GraphOptimizationLevel},
        value::DynValue,
    };
    use ndarray::Array;
    use std::sync::Arc;
    use parking_lot::Mutex;

    pub struct YOLOv8Detector {
        session: Option<Arc<Mutex<Session>>>,
        _model_path: String,
        input_size: u32,
    }

    impl YOLOv8Detector {
        pub fn new() -> Self {
            let model_path = std::env::var("YOLOV8_MODEL_PATH")
                .unwrap_or_else(|_| "./models/yolov8n.onnx".to_string());

            let session = Self::load_model(&model_path);

            Self {
                session,
                _model_path: model_path,
                input_size: 640,
            }
        }

        fn load_model(path: &str) -> Option<Arc<Mutex<Session>>> {
            let session = Session::builder()
                .ok()?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .ok()?
                .with_intra_threads(2)
                .ok()?
                .commit_from_file(path)
                .ok()?;

            eprintln!("[YOLOv8] Model loaded from: {}", path);
            Some(Arc::new(Mutex::new(session)))
        }

        pub fn detect(&self, image_data: &[u8], confidence_threshold: f32) -> std::result::Result<Vec<Detection>, String> {
            let session = match &self.session {
                Some(s) => Arc::clone(s),
                None => return Err("YOLOv8 model not loaded".to_string()),
            };

            // Decode image
            let img = image::load_from_memory(image_data)
                .map_err(|e| format!("Failed to decode image: {}", e))?;

            let (orig_width, orig_height) = (img.width(), img.height());

            // Preprocess and run inference
            let (tensor_data, scale) = self.preprocess_image(&img);

            // Create ndarray for input
            let input_array = Array::from_shape_vec(
                (1, 3, self.input_size as usize, self.input_size as usize),
                tensor_data,
            ).map_err(|e| format!("Failed to create input array: {}", e))?
             .into_dyn();

            // Create input tensor
            let input_tensor = ort::value::Tensor::from_array(input_array)
                .map_err(|e| format!("Failed to create input tensor: {}", e))?;

            // Run inference
            let mut session_guard = session.lock();
            let input_value = ort::session::input::SessionInputValue::Owned(input_tensor.into());
            let outputs = session_guard.run([input_value])
                .map_err(|e| format!("YOLOv8 inference failed: {}", e))?;

            // Parse output
            if outputs.len() == 0 {
                return Err("No output from model".to_string());
            }

            let output = &outputs[0];
            self.postprocess(output, orig_width, orig_height, scale, confidence_threshold)
        }

        fn preprocess_image(&self, img: &image::DynamicImage) -> (Vec<f32>, f32) {
            let resized = img.resize_exact(
                self.input_size,
                self.input_size,
                image::imageops::FilterType::Triangle,
            );

            let scale = img.width() as f32 / self.input_size as f32;

            let rgb = resized.to_rgb8();
            let mut input = Vec::with_capacity((self.input_size * self.input_size * 3) as usize);

            for pixel in rgb.pixels() {
                input.push(pixel[0] as f32 / 255.0);
                input.push(pixel[1] as f32 / 255.0);
                input.push(pixel[2] as f32 / 255.0);
            }

            (input, scale)
        }

        fn postprocess(
            &self,
            output: &DynValue,
            orig_width: u32,
            orig_height: u32,
            scale: f32,
            confidence_threshold: f32,
        ) -> std::result::Result<Vec<Detection>, String> {
            let output_data = output.try_extract_tensor::<f32>()
                .map_err(|e| format!("Failed to extract output: {}", e))?;

            let data = output_data.1;
            let nms_threshold = 0.45;

            let mut detections = Vec::new();
            let num_predictions = 8400;
            let num_classes = 80;

            let mut all_boxes: Vec<(usize, f32, [f32; 4])> = Vec::new();

            for i in 0..num_predictions {
                let offset = i * (4 + num_classes);
                if offset + 4 + num_classes > data.len() {
                    break;
                }

                // Find max class
                let mut max_class = 0;
                let mut max_score = 0.0f32;

                for c in 0..num_classes {
                    let score = data[offset + 4 + c];
                    if score > max_score {
                        max_score = score;
                        max_class = c;
                    }
                }

                if max_score >= confidence_threshold {
                    let cx = data[offset] * self.input_size as f32 / scale;
                    let cy = data[offset + 1] * self.input_size as f32 / scale;
                    let w = data[offset + 2] * self.input_size as f32 / scale;
                    let h = data[offset + 3] * self.input_size as f32 / scale;

                    let x1 = (cx - w / 2.0).max(0.0);
                    let y1 = (cy - h / 2.0).max(0.0);
                    let x2 = (cx + w / 2.0).min(orig_width as f32);
                    let y2 = (cy + h / 2.0).min(orig_height as f32);

                    all_boxes.push((max_class, max_score, [x1, y1, x2, y2]));
                }
            }

            // Apply NMS
            for class_id in 0..num_classes {
                let mut class_boxes: Vec<_> = all_boxes.iter()
                    .filter(|(c, _, _)| *c == class_id)
                    .cloned()
                    .collect();

                class_boxes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

                while !class_boxes.is_empty() {
                    let best = class_boxes.remove(0);

                    let x1 = best.2[0] as u32;
                    let y1 = best.2[1] as u32;
                    let x2 = best.2[2] as u32;
                    let y2 = best.2[3] as u32;

                    let label = COCO_CLASSES.get(class_id).unwrap_or(&"unknown");

                    detections.push(Detection {
                        label: label.to_string(),
                        confidence: best.1,
                        bbox: Some(BoundingBox {
                            x: x1,
                            y: y1,
                            width: x2.saturating_sub(x1),
                            height: y2.saturating_sub(y1),
                        }),
                    });

                    class_boxes.retain(|(_, _, box2)| {
                        Self::compute_iou(&best.2, box2) < nms_threshold
                    });
                }
            }

            detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
            detections.truncate(100);

            Ok(detections)
        }

        fn compute_iou(box1: &[f32; 4], box2: &[f32; 4]) -> f32 {
            let x1 = box1[0].max(box2[0]);
            let y1 = box1[1].max(box2[1]);
            let x2 = box1[2].min(box2[2]);
            let y2 = box1[3].min(box2[3]);

            let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
            let area1 = (box1[2] - box1[0]) * (box1[3] - box1[1]);
            let area2 = (box2[2] - box2[0]) * (box2[3] - box2[1]);
            let union = area1 + area2 - intersection;

            if union > 0.0 { intersection / union } else { 0.0 }
        }

        pub fn is_loaded(&self) -> bool {
            self.session.is_some()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
use native::YOLOv8Detector;

// ============================================================================
// Export FFI using SDK macro
// ============================================================================

neomind_extension_sdk::neomind_export!(ImageAnalyzer);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = ImageAnalyzer::new();
        let meta = ext.metadata();
        assert_eq!(meta.id, "image-analyzer-v2");
        assert_eq!(meta.name, "Image Analyzer V2");
    }

    #[test]
    fn test_extension_metrics() {
        let ext = ImageAnalyzer::new();
        let metrics = ext.metrics();
        assert_eq!(metrics.len(), 3);
    }

    #[test]
    fn test_extension_commands() {
        let ext = ImageAnalyzer::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "analyze_image");
    }

    #[test]
    fn test_fallback_analysis() {
        let ext = ImageAnalyzer::new();
        let (objects, description) = ext.fallback_analysis(&[0xFF, 0xD8, 0xFF, 0x00]);

        assert!(!objects.is_empty());
        assert!(description.contains("Fallback"));
    }
}
