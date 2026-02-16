//! Image Analyzer Extension
//!
//! A stateless extension that analyzes images and returns detection results.
//! Demonstrates the Stateless streaming mode for single-chunk processing.
//!
//! # Streaming Mode
//!
//! - **Mode**: Stateless (stateless)
//! - **Direction**: Upload (client → extension)
//! - **Supported Types**: JPEG, PNG, WebP images
//! - **Max Chunk Size**: 10MB
//!
//! # Usage
//!
//! Build the extension:
//! ```bash
//! cd /Users/shenmingming/NeoMind-Extension
//! cargo build --release -p neomind-image-analyzer
//! ```
//!
//! Output:
//! - macOS: `target/release/libneomind_extension_image_analyzer.dylib`
//! - Linux: `target/release/libneomind_extension_image_analyzer.so`
//! - Windows: `target/release/neomind_extension_image_analyzer.dll`

use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;

// Import from neomind-core
use neomind_core::extension::system::{
    Extension, ExtensionMetadata, ExtensionError, MetricDefinition, ExtensionCommand,
    ExtensionMetricValue, ParamMetricValue, MetricDataType, CommandDefinition,
    CExtensionMetadata, ABI_VERSION, Result,
};
use neomind_core::extension::{
    StreamCapability, StreamMode, StreamDirection, StreamDataType, DataChunk, StreamResult,
};

use async_trait::async_trait;
use serde_json::Value;
use semver::Version;
use std::collections::HashMap;

// ============================================================================
// Types
// ============================================================================

/// Image detection result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Detection {
    label: String,
    confidence: f32,
    bbox: Option<BoundingBox>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BoundingBox {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Analysis result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AnalysisResult {
    objects: Vec<Detection>,
    dominant_color: Option<String>,
    estimated_size: Option<String>,
    processing_time_ms: u64,
}

/// Extension statistics
#[derive(Debug, Default)]
struct ImageAnalyzerStats {
    images_processed: u64,
    total_processing_time_ms: u64,
    detections_found: u64,
}

// ============================================================================
// Static Metrics and Commands
// ============================================================================

static METRICS: Lazy<[MetricDefinition; 3]> = Lazy::new(|| [
    MetricDefinition {
        name: "images_processed".to_string(),
        display_name: "Images Processed".to_string(),
        data_type: MetricDataType::Integer,
        unit: "count".to_string(),
        min: Some(0.0),
        max: None,
        required: false,
    },
    MetricDefinition {
        name: "avg_processing_time_ms".to_string(),
        display_name: "Average Processing Time".to_string(),
        data_type: MetricDataType::Float,
        unit: "ms".to_string(),
        min: Some(0.0),
        max: None,
        required: false,
    },
    MetricDefinition {
        name: "total_detections".to_string(),
        display_name: "Total Detections".to_string(),
        data_type: MetricDataType::Integer,
        unit: "count".to_string(),
        min: Some(0.0),
        max: None,
        required: false,
    },
]);

static COMMANDS: Lazy<[CommandDefinition; 1]> = Lazy::new(|| [
    CommandDefinition {
        name: "reset_stats".to_string(),
        display_name: "Reset Statistics".to_string(),
        payload_template: "{}".to_string(),
        parameters: vec![],
        fixed_values: HashMap::new(),
        samples: vec![],
        llm_hints: "Resets all processing statistics to zero".to_string(),
        parameter_groups: vec![],
    },
]);

// ============================================================================
// Image Analyzer Extension
// ============================================================================

pub struct ImageAnalyzer {
    metadata: ExtensionMetadata,
    stats: Arc<Mutex<ImageAnalyzerStats>>,
}

impl ImageAnalyzer {
    pub fn new() -> Self {
        let metadata = ExtensionMetadata::new(
            "image-analyzer",
            "Image Analyzer",
            Version::new(1, 0, 0),
        )
        .with_description("Stateless image analysis extension that detects objects and analyzes image properties")
        .with_author("NeoMind Team");

        Self {
            metadata,
            stats: Arc::new(Mutex::new(ImageAnalyzerStats::default())),
        }
    }

    /// Analyze image data and return detection results
    fn analyze_image(&self, data: &[u8]) -> Result<AnalysisResult> {
        let start = std::time::Instant::now();

        // In a real implementation, this would use a CNN or similar ML model
        // For demonstration, we'll do basic image analysis
        let objects = self.detect_objects(data)?;
        let dominant_color = self.extract_dominant_color(data)?;
        let estimated_size = self.estimate_image_size(data);

        let processing_time = start.elapsed().as_millis() as u64;

        // Update stats
        let mut stats = self.stats.lock().unwrap();
        stats.images_processed += 1;
        stats.total_processing_time_ms += processing_time;
        stats.detections_found += objects.len() as u64;

        Ok(AnalysisResult {
            objects,
            dominant_color,
            estimated_size,
            processing_time_ms: processing_time,
        })
    }

    /// Simple object detection (placeholder)
    fn detect_objects(&self, _data: &[u8]) -> Result<Vec<Detection>> {
        // In a real implementation, this would use a model like YOLO, SSD, etc.
        // For demonstration, return some mock detections
        Ok(vec![
            Detection {
                label: "example_object".to_string(),
                confidence: 0.85,
                bbox: Some(BoundingBox {
                    x: 100,
                    y: 100,
                    width: 200,
                    height: 150,
                }),
            },
        ])
    }

    /// Extract dominant color from image (placeholder)
    fn extract_dominant_color(&self, data: &[u8]) -> Result<Option<String>> {
        // For JPEG/PNG, we could analyze the pixel data
        // For demonstration, just return a placeholder
        if data.len() < 100 {
            return Ok(None);
        }

        // Simple heuristic: check for common image signatures
        let color = if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some("#808080".to_string()) // JPEG - gray placeholder
        } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            Some("#808080".to_string()) // PNG - gray placeholder
        } else {
            None
        };

        Ok(color)
    }

    /// Estimate image size category
    fn estimate_image_size(&self, data: &[u8]) -> Option<String> {
        let size = data.len();
        let category = if size < 10_000 {
            "small"
        } else if size < 100_000 {
            "medium"
        } else if size < 1_000_000 {
            "large"
        } else {
            "very_large"
        };
        Some(category.to_string())
    }

    fn reset_stats(&self) -> Result<Value> {
        let mut stats = self.stats.lock().unwrap();
        stats.images_processed = 0;
        stats.total_processing_time_ms = 0;
        stats.detections_found = 0;
        Ok(serde_json::json!({"status": "reset"}))
    }
}

#[async_trait::async_trait]
impl Extension for ImageAnalyzer {
    fn metadata(&self) -> &ExtensionMetadata {
        &self.metadata
    }

    fn metrics(&self) -> &[MetricDefinition] {
        &*METRICS
    }

    fn commands(&self) -> &[CommandDefinition] {
        &*COMMANDS
    }

    async fn execute_command(
        &self,
        command: &str,
        _args: &Value,
    ) -> Result<Value> {
        match command {
            "reset_stats" => self.reset_stats(),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let stats = self.stats.lock().unwrap();
        let avg_time = if stats.images_processed > 0 {
            stats.total_processing_time_ms as f64 / stats.images_processed as f64
        } else {
            0.0
        };

        Ok(vec![
            ExtensionMetricValue::new(
                "images_processed",
                ParamMetricValue::Integer(stats.images_processed as i64),
            ),
            ExtensionMetricValue::new(
                "avg_processing_time_ms",
                ParamMetricValue::Float(avg_time),
            ),
            ExtensionMetricValue::new(
                "total_detections",
                ParamMetricValue::Integer(stats.detections_found as i64),
            ),
        ])
    }

    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability {
            direction: StreamDirection::Upload,
            mode: StreamMode::Stateless,
            supported_data_types: vec![
                StreamDataType::Image { format: "jpeg".to_string() },
                StreamDataType::Image { format: "png".to_string() },
                StreamDataType::Image { format: "webp".to_string() },
            ],
            max_chunk_size: 10 * 1024 * 1024, // 10MB
            preferred_chunk_size: 1024 * 1024, // 1MB
            max_concurrent_sessions: 10,
            flow_control: Default::default(),
            config_schema: None,
        })
    }

    async fn process_chunk(&self, chunk: DataChunk) -> Result<StreamResult> {
        // Validate data type
        match &chunk.data_type {
            StreamDataType::Image { .. } => (),
            StreamDataType::Binary => {
                // Allow binary, assume it's an image
            }
            _ => {
                return Err(ExtensionError::InvalidStreamData(
                    "Expected image data".to_string(),
                ))
            }
        }

        // Analyze the image
        let result = self.analyze_image(&chunk.data)?;

        // Serialize result as JSON
        let output_data = serde_json::to_vec(&result)
            .map_err(|e| ExtensionError::InvalidStreamData(e.to_string()))?;

        Ok(StreamResult {
            input_sequence: Some(chunk.sequence),
            output_sequence: chunk.sequence,
            data: output_data,
            data_type: StreamDataType::Json,
            processing_ms: result.processing_time_ms as f32,
            metadata: Some(serde_json::json!({
                "processing_time_ms": result.processing_time_ms,
                "objects_detected": result.objects.len(),
            })),
            error: None,
        })
    }
}

// ============================================================================
// Global Extension Instance
// ============================================================================

static EXTENSION_INSTANCE: Lazy<ImageAnalyzer> = Lazy::new(|| ImageAnalyzer::new());

// ============================================================================
// FFI Exports for Dynamic Loading
// ============================================================================

use tokio::sync::RwLock;

/// Get ABI version
#[no_mangle]
pub extern "C" fn neomind_extension_abi_version() -> u32 {
    ABI_VERSION
}

/// Get extension metadata
#[no_mangle]
pub extern "C" fn neomind_extension_metadata() -> CExtensionMetadata {
    use std::ffi::CStr;

    // Use static CStr references to avoid dangling pointers
    let id = CStr::from_bytes_with_nul(b"image-analyzer\0").unwrap();
    let name = CStr::from_bytes_with_nul(b"Image Analyzer\0").unwrap();
    let version = CStr::from_bytes_with_nul(b"1.0.0\0").unwrap();
    let description = CStr::from_bytes_with_nul(b"Stateless image analysis extension\0").unwrap();
    let author = CStr::from_bytes_with_nul(b"NeoMind Team\0").unwrap();

    CExtensionMetadata {
        abi_version: ABI_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        version: version.as_ptr(),
        description: description.as_ptr(),
        author: author.as_ptr(),
        metric_count: 3,
        command_count: 1,
    }
}

/// Create extension instance
#[no_mangle]
pub extern "C" fn neomind_extension_create(
    config_json: *const u8,
    config_len: usize,
) -> *mut RwLock<Box<dyn Extension>> {
    use std::sync::Arc;

    // Parse config (ignored for this extension)
    let _config = if config_json.is_null() || config_len == 0 {
        serde_json::json!({})
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(config_json, config_len);
            let s = std::str::from_utf8_unchecked(slice);
            serde_json::from_str(s).unwrap_or(serde_json::json!({}))
        }
    };

    let extension = ImageAnalyzer::new();
    Box::into_raw(Box::new(RwLock::new(Box::new(extension))))
}

/// Destroy extension instance
#[no_mangle]
pub extern "C" fn neomind_extension_destroy(ptr: *mut RwLock<Box<dyn Extension>>) {
    if !ptr.is_null() {
        unsafe {
            let _ = Box::from_raw(ptr);
        }
    }
}
