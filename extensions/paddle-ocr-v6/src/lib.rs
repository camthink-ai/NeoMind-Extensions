// paddle-ocr-v6 — PP-OCRv6 native ONNX inference extension
//
// Placeholder. Real implementation lands in Phase 4 (tier/preset/downloader)
// and Phase 6 (OcrEngine + Extension trait).

mod downloader;
mod preset;
mod tier;

use async_trait::async_trait;
use neomind_extension_sdk::{Extension, ExtensionMetadata};

pub struct PaddleOcrV6Extension;

impl PaddleOcrV6Extension {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PaddleOcrV6Extension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for PaddleOcrV6Extension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new(
                "paddle-ocr-v6",
                "PaddleOCR-v6",
                env!("CARGO_PKG_VERSION"),
            )
            .with_description(
                "PP-OCRv6 native ONNX inference with multi-tier model support (tiny/small/medium)",
            )
            .with_author("NeoMind Team")
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

neomind_extension_sdk::neomind_export!(PaddleOcrV6Extension);
