//! DeepStream extension — see docs/superpowers/specs/2026-07-06-deepstream-extension-design.md

mod protocol;

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionError, ExtensionMetadata, Result,
};

pub struct DeepStreamExtension;

impl Default for DeepStreamExtension {
    fn default() -> Self { Self::new() }
}

impl DeepStreamExtension {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Extension for DeepStreamExtension {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("deepstream-v2", "NVIDIA DeepStream", env!("CARGO_PKG_VERSION"))
                .with_description("Multi-stream RTSP video inference via NVIDIA DeepStream")
                .with_author("NeoMind Team")
        })
    }

    fn commands(&self) -> Vec<neomind_extension_sdk::ExtensionCommand> { vec![] }
    fn metrics(&self) -> Vec<neomind_extension_sdk::MetricDescriptor> { vec![] }

    async fn execute_command(&self, cmd: &str, _args: &serde_json::Value) -> Result<serde_json::Value> {
        Err(ExtensionError::CommandNotFound(cmd.to_string()))
    }

    fn produce_metrics(&self) -> Result<Vec<neomind_extension_sdk::ExtensionMetricValue>> {
        Ok(vec![])
    }
}

neomind_extension_sdk::neomind_export!(DeepStreamExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_id() {
        assert_eq!(DeepStreamExtension::new().metadata().id, "deepstream-v2");
    }

    #[test]
    fn panic_unwind_invariant() {
        // Workspace profile sets panic=unwind; member override would silently break this.
        // See CLAUDE.md "Safety Requirements".
        // NOTE: this only fires meaningfully under `cargo test --release` because the dev
        // profile defaults to panic=unwind anyway. CI must run release tests for this guard
        // to catch regressions in [profile.release] overrides.
        assert!(cfg!(panic = "unwind"),
            "panic must be unwind — check workspace Cargo.toml [profile.release]");
    }
}
