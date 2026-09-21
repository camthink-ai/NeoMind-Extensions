//! Lightweight detector stub — the VLM replaces YOLO object detection.
//! Kept for ABI/type compatibility with the video streaming pipeline;
//! `is_loaded()` always returns false so the pipeline uses VLM analysis.

use serde::{Deserialize, Serialize};
use crate::BoundingBox;

/// Detection result (kept for type compatibility; VLM path uses text instead).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub class_id: u32,
    pub class_name: String,
    pub confidence: f32,
    pub bbox: BoundingBox,
}

/// YOLO detector stub — always "not loaded", so the pipeline never runs
/// object detection and relies on the on-board VLM instead.
pub struct YoloDetector {
    load_error: Option<String>,
}

impl YoloDetector {
    pub fn new() -> Result<Self, String> {
        // Stub: no model, no error.
        Ok(Self { load_error: None })
    }

    pub fn ensure_loaded(&mut self) {}

    pub fn is_loaded(&self) -> bool {
        false
    }

    pub fn model_size(&self) -> usize {
        0
    }

    pub fn get_load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn detect(
        &self,
        _image: &image::RgbImage,
        _confidence_threshold: f32,
        _max_detections: u32,
    ) -> Vec<Detection> {
        Vec::new()
    }

    pub fn cleanup_memory(&self) {}
}
