pub mod commands;
pub mod config;
pub mod db;
pub mod ingest;
pub mod metrics;
pub mod ne503;
pub mod state;
pub mod tls;
pub mod types;

use std::sync::OnceLock;
use neomind_extension_sdk::{
    async_trait, json, Extension, ExtensionCommand, ExtensionMetadata, ExtensionMetricValue,
    MetricDescriptor, Result,
};

pub struct GymTrackerExtension;

impl GymTrackerExtension {
    fn new() -> Self { Self }
}

impl Default for GymTrackerExtension { fn default() -> Self { Self::new() } }

#[async_trait]
impl Extension for GymTrackerExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("gym-tracker", "Gym Tracker", env!("CARGO_PKG_VERSION"))
                .with_description("Smart-gym analytics on the NeoEyes NE503")
                .with_author("NeoMind Team")
        })
    }
    fn commands(&self) -> Vec<ExtensionCommand> { Vec::new() }
    fn metrics(&self) -> Vec<MetricDescriptor> { Vec::new() }
    async fn execute_command(&self, _cmd: &str, _args: &serde_json::Value) -> Result<serde_json::Value> { Ok(json!({})) }
    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> { Ok(Vec::new()) }   // SYNC — not async
    async fn configure(&mut self, _config: &serde_json::Value) -> Result<()> { Ok(()) }   // &mut self, not initialize(&self,&str)
    fn as_any(&self) -> &dyn std::any::Any { self }                                        // required, no default
    fn stop(&mut self) -> Result<()> { Ok(()) }                                            // stop ingest thread here later
}

neomind_extension_sdk::neomind_export!(GymTrackerExtension);
