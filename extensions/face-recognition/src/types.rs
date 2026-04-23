//! Shared type definitions for the face-recognition extension.

use serde::{Deserialize, Serialize};

/// A single facial landmark point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Landmark {
    pub x: f64,
    pub y: f64,
}

/// Bounding box for a detected face
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landmarks: Option<Vec<Landmark>>,
}

/// Result for a single detected face
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceResult {
    pub face_box: FaceBox,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face_id: Option<String>,
}

/// Device binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceBinding {
    pub device_id: String,
    pub metric_name: String,
    pub active: bool,
    pub created_at: i64,
}

/// Statistics and latest results for a device binding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingStats {
    pub total_inferences: u64,
    pub total_recognized: u64,
    pub total_unknown: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_faces: Option<Vec<FaceResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Extension configuration for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceRecConfig {
    pub confidence_threshold: f64,
    pub recognition_threshold: f64,
    pub max_faces: usize,
    pub auto_detect: bool,
    #[serde(default)]
    pub bindings: Vec<DeviceBinding>,
}

impl Default for FaceRecConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.5,
            recognition_threshold: 0.45,
            max_faces: 10,
            auto_detect: true,
            bindings: Vec::new(),
        }
    }
}
