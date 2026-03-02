//! YOLOv11 Detector using usls library
//!
//! This implementation uses the usls crate which provides:
//! - Automatic GPU acceleration detection
//! - Better memory management
//! - Simpler API with built-in preprocessing/postprocessing

use image::RgbImage;
use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use usls::{models::YOLO, Config, Model, ORTConfig, Runtime, Version};

// Re-export BoundingBox from parent module to avoid duplication
pub use crate::BoundingBox;

/// Detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub class_id: u32,
    pub class_name: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
}

/// YOLOv11 detector using usls
pub struct YoloDetector {
    #[cfg(not(target_arch = "wasm32"))]
    model: Option<Runtime<YOLO>>,
    #[cfg(target_arch = "wasm32")]
    model_loaded: bool,
    model_size: usize,
}

impl YoloDetector {
    /// Create a new detector by loading the model
    pub fn new() -> Result<Self, String> {
        tracing::info!("Initializing YOLO detector with usls library");

        #[cfg(not(target_arch = "wasm32"))]
        {
            let model_data = Self::load_model_data()?;

            if model_data.is_none() {
                tracing::warn!("YOLOv11n model not found, running in demo mode");
                return Ok(Self {
                    model: None,
                    model_size: 0,
                });
            }

            let model_bytes = model_data.unwrap();
            let model_size = model_bytes.len();

            tracing::info!("Loading YOLO model ({} bytes) with usls", model_size);

            // Save model to temp file (usls requires file path)
            let temp_dir = std::env::temp_dir();
            let model_path = temp_dir.join("yolo11n.onnx");
            std::fs::write(&model_path, &model_bytes)
                .map_err(|e| format!("Failed to write temp model file: {}", e))?;

            // Create Config for YOLO detection
            tracing::info!("Configuring YOLO model with usls Config...");

            // Create ORTConfig with model file path
            let ort_config = ORTConfig::default()
                .with_file(model_path.to_str().unwrap());

            // Use yolo_detect() preset configuration for YOLOv11 detection
            let config = Config::yolo_detect()
                .with_model(ort_config)
                .with_version(Version(11, 0, None))  // YOLOv11
                .with_class_confs(&[0.25]);  // Confidence threshold 0.25

            // Create YOLO model from config using Model::new()
            let model = YOLO::new(config)
                .map_err(|e| format!("Failed to create YOLO model: {:?}", e))?;

            tracing::info!("✓ YOLO model loaded successfully with usls");
            tracing::info!("Model: YOLOv11n, Confidence: 0.25");

            // Clean up temp file
            let _ = std::fs::remove_file(&model_path);

            Ok(Self {
                model: Some(model),
                model_size,
            })
        }

        #[cfg(target_arch = "wasm32")]
        {
            tracing::warn!("YOLO not available in WASM, running in demo mode");
            Ok(Self {
                model_loaded: false,
                model_size: 0,
            })
        }
    }

    /// Load model data from disk
    fn load_model_data() -> Result<Option<Vec<u8>>, String> {
        // Try to get extension directory from environment variable (set by runner)
        let base_paths: Vec<std::path::PathBuf> = if let Ok(ext_dir) = std::env::var("NEOMIND_EXTENSION_DIR") {
            vec![
                std::path::PathBuf::from(&ext_dir),
                std::path::PathBuf::from(ext_dir).join(".."),
            ]
        } else {
            // Fallback to current directory
            vec![
                std::path::PathBuf::from("."),
                std::path::PathBuf::from(".."),
                std::path::PathBuf::from("../.."),
            ]
        };

        for base in &base_paths {
            let model_path = base.join("models").join("yolo11n.onnx");
            if model_path.exists() {
                tracing::info!("Loading YOLOv11n model from: {}", model_path.display());
                return std::fs::read(&model_path)
                    .map(Some)
                    .map_err(|e| format!("Failed to read model: {}", e));
            } else {
                tracing::debug!("Model not found at: {}", model_path.display());
            }
        }

        tracing::warn!("YOLOv11n model not found in any expected location");
        Ok(None)
    }

    /// Check if model is loaded
    pub fn is_loaded(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.model.is_some()
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
    pub fn detect(&mut self, image: &RgbImage, _confidence_threshold: f32, max_detections: u32) -> Vec<Detection> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(ref mut model) = self.model {
                return Self::run_inference(model, image, max_detections);
            }
        }

        tracing::debug!("Model not loaded, returning empty detections");
        Vec::new()
    }

    /// Run YOLO inference using usls with proper API
    #[cfg(not(target_arch = "wasm32"))]
    fn run_inference(
        model: &mut Runtime<YOLO>,
        image: &RgbImage,
        max_detections: u32,
    ) -> Vec<Detection> {
        let start = std::time::Instant::now();

        // Convert RgbImage to usls::Image
        let dynamic_image = image::DynamicImage::ImageRgb8(image.clone());

        // Create usls Image from DynamicImage
        let usls_image = match usls::Image::try_from(dynamic_image) {
            Ok(img) => img,
            Err(e) => {
                tracing::error!("Failed to convert image: {:?}", e);
                return Vec::new();
            }
        };

        // Run inference using Model::run()
        let ys = match model.run(&[usls_image]) {
            Ok(results) => results,
            Err(e) => {
                tracing::error!("YOLO inference failed: {:?}", e);
                return Vec::new();
            }
        };

        // Get first result (single image inference)
        let y = match ys.into_iter().next() {
            Some(result) => result,
            None => {
                tracing::debug!("No detection results returned");
                return Vec::new();
            }
        };

        // Extract horizontal bounding boxes from results
        let hbbs = y.hbbs();

        // Convert usls Hbb results to our Detection format
        let mut detections: Vec<Detection> = hbbs
            .iter()
            .take(max_detections as usize)
            .filter_map(|hbb| {
                // Get bounding box coordinates
                let x = hbb.x();
                let y_coord = hbb.y();
                let width = hbb.w();
                let height = hbb.h();

                // Skip invalid boxes
                if width <= 0.0 || height <= 0.0 {
                    return None;
                }

                // Get metadata
                let class_id = hbb.id().unwrap_or(0) as u32;
                let confidence = hbb.confidence().unwrap_or(0.0);
                let class_name = hbb.name()
                    .map(|s: &str| s.to_string())
                    .unwrap_or_else(|| Self::get_class_name(class_id as usize));

                Some(Detection {
                    class_id,
                    class_name,
                    confidence,
                    bbox: BoundingBox {
                        x,
                        y: y_coord,
                        width,
                        height,
                    },
                })
            })
            .collect();

        // Sort by confidence descending
        detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        let elapsed = start.elapsed();
        tracing::debug!(
            "YOLO inference took {}ms, found {} detections",
            elapsed.as_millis(),
            detections.len()
        );

        detections
    }

    /// Get COCO class name by index
    fn get_class_name(class_id: usize) -> String {
        const COCO_CLASSES: [&str; 80] = [
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

        COCO_CLASSES
            .get(class_id)
            .unwrap_or(&"unknown")
            .to_string()
    }
}
