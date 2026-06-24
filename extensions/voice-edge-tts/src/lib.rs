//! voice-edge-tts — Rust cdylib proxy for the Python TTS service.
#![deny(unsafe_code)]

use async_trait::async_trait;
use neomind_extension_sdk::{Extension, ExtensionMetadata, Result};
use serde_json::Value;
use std::sync::OnceLock;

pub struct VoiceEdgeTtsExtension;

impl VoiceEdgeTtsExtension {
    pub fn new() -> Self { Self }
}

impl Default for VoiceEdgeTtsExtension {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Extension for VoiceEdgeTtsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("voice-edge-tts", "Voice Edge TTS", "2.7.6")
                .with_description("Edge TTS via sherpa-onnx ZipVoice — cross-platform CPU streaming.")
                .with_author("NeoMind Team")
        })
    }

    fn commands(&self) -> Vec<neomind_extension_sdk::ExtensionCommand> { vec![] }

    async fn execute_command(&self, _cmd: &str, _args: &Value) -> Result<Value> {
        Err(neomind_extension_sdk::ExtensionError::NotSupported("not implemented yet".into()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

neomind_extension_sdk::neomind_export!(VoiceEdgeTtsExtension);
