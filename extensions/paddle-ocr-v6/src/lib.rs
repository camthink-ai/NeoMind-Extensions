//! paddle-ocr-v6 — PP-OCRv6 native ONNX inference extension.
//!
//! Architecture: this extension ONLY exposes inference commands
//! (`recognize`, `switch_tier`, `health`). It does NOT bind to NeoMind
//! devices — that's the upper layer's job. The upper layer calls
//! `recognize` with image bytes + ROI and gets back text + bboxes.
//!
//! Internally: OcrEngine holds a single detector (DB) + single
//! multilingual recognizer (SVTR). Tier switching reloads both
//! models. Tiny tier ships in the .nep; small/medium lazy-download
//! from HuggingFace on first switch.

mod downloader;
mod preset;
mod tier;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use neomind_extension_sdk::{
    Extension, ExtensionCommand, ExtensionError, ExtensionMetadata, MetricDataType,
    MetricDescriptor, MetricValue, ParameterDefinition, Result,
};
use parking_lot::RwLock;
use serde_json::{json, Value};

use crate::downloader::Downloader;
use crate::tier::Tier;

// ---------------------------------------------------------------------------
// OcrEngine — detector + single multilingual recognizer
// ---------------------------------------------------------------------------

pub struct OcrEngine {
    pub detector: Option<usls::models::DB>,
    pub recognizer: Option<usls::models::SVTR>,
    pub tier: Tier,
    pub downloader: Downloader,
    pub models_dir: PathBuf,
    pub loaded: bool,
    pub load_error: Option<String>,
}

impl OcrEngine {
    pub fn new() -> Self {
        Self {
            detector: None,
            recognizer: None,
            tier: Tier::default(),
            downloader: Downloader::default(),
            models_dir: default_models_dir(),
            loaded: false,
            load_error: None,
        }
    }

    /// Lazy-load detector + recognizer for `tier`. No-op if already
    /// loaded at the same tier. Records any failure in `load_error`
    /// rather than returning Err — callers check `loaded` / `load_error`.
    pub fn ensure_loaded(&mut self, tier: Tier, device_hint: Option<&str>) {
        if self.loaded && self.tier == tier {
            return;
        }

        // Resolve Auto → concrete tier using host capability.
        let resolved = tier.resolve(
            has_cuda(device_hint),
            has_coreml(device_hint),
            total_ram_gb(),
        );
        self.tier = resolved;

        // Lazy-download missing files. Tiny tier ships in .nep so this
        // is usually a no-op; only small/medium trigger downloads.
        if let Err(e) = self.downloader.ensure_models(resolved, &self.models_dir) {
            self.loaded = false;
            self.load_error = Some(format!("download failed: {}", e));
            return;
        }

        let device = auto_device(device_hint);

        match try_load_detector(resolved, &self.models_dir, device) {
            Ok(d) => self.detector = Some(d),
            Err(e) => {
                self.loaded = false;
                self.load_error = Some(format!("detector load failed: {}", e));
                return;
            }
        }
        match try_load_recognizer(resolved, &self.models_dir, device) {
            Ok(r) => self.recognizer = Some(r),
            Err(e) => {
                self.loaded = false;
                self.load_error = Some(format!("recognizer load failed: {}", e));
                return;
            }
        }
        self.loaded = true;
        self.load_error = None;
    }
}

impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn try_load_detector(
    tier: Tier,
    models_dir: &std::path::Path,
    device: usls::Device,
) -> std::result::Result<usls::models::DB, String> {
    let cfg = preset::ppocr_det_v6(tier, models_dir)
        .with_device_all(device)
        .commit()
        .map_err(|e| format!("det config commit failed: {}", e))?;
    usls::models::DB::new(cfg).map_err(|e| format!("DB init failed: {}", e))
}

fn try_load_recognizer(
    tier: Tier,
    models_dir: &std::path::Path,
    device: usls::Device,
) -> std::result::Result<usls::models::SVTR, String> {
    let cfg = preset::ppocr_rec_v6(tier, models_dir)
        .with_device_all(device)
        .commit()
        .map_err(|e| format!("rec config commit failed: {}", e))?;
    usls::models::SVTR::new(cfg).map_err(|e| format!("SVTR init failed: {}", e))
}

// ---------------------------------------------------------------------------
// Host capability detection (dependency-free)
// ---------------------------------------------------------------------------

/// Locate bundled models directory. Searches candidate paths relative
/// to the current executable and cwd.
fn default_models_dir() -> PathBuf {
    // Try: <exe_dir>/models, <exe_dir>/../models, <cwd>/models
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidates: [Option<PathBuf>; 2] = [
                Some(exe_dir.join("models")),
                exe_dir.parent().map(|p| p.join("models")),
            ];
            for cand in candidates.into_iter().flatten() {
                if cand.is_dir() {
                    return cand;
                }
            }
        }
    }
    std::env::current_dir().unwrap_or_default().join("models")
}

fn has_cuda(hint: Option<&str>) -> bool {
    if let Some(h) = hint {
        return h.eq_ignore_ascii_case("cuda");
    }
    // Best-effort: env var presence is a reasonable proxy. v1 additionally
    // probes GPU free memory; we keep this dependency-free.
    cfg!(target_os = "linux") && std::env::var("CUDA_VISIBLE_DEVICES").is_ok()
}

fn has_coreml(hint: Option<&str>) -> bool {
    if let Some(h) = hint {
        return h.eq_ignore_ascii_case("coreml");
    }
    cfg!(target_os = "macos")
}

/// Best-effort total RAM in GiB. Reads /proc/meminfo on Linux;
/// returns 16 (assume Mid-tier capable) elsewhere to avoid pulling
/// a sysinfo crate dependency for one probe.
fn total_ram_gb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let kib: u64 = rest
                        .trim()
                        .trim_end_matches(" kB")
                        .parse()
                        .unwrap_or(0);
                    return kib / (1024 * 1024);
                }
            }
        }
        return 8;
    }
    #[cfg(not(target_os = "linux"))]
    {
        16
    }
}

fn auto_device(hint: Option<&str>) -> usls::Device {
    // macOS → CoreML (Apple Silicon optimizes SVTR significantly)
    #[cfg(target_os = "macos")]
    {
        let _ = hint;
        return usls::Device::CoreMl;
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(h) = hint {
            if h.eq_ignore_ascii_case("cpu") {
                return usls::Device::Cpu(0);
            }
        }
        if has_cuda(Some("cuda")) || has_cuda(None) {
            return usls::Device::Cuda(0);
        }
        usls::Device::Cpu(0)
    }
}

// ---------------------------------------------------------------------------
// Extension shell — owns the engine behind a RwLock
// ---------------------------------------------------------------------------

pub struct PaddleOcrV6Extension {
    engine: Arc<RwLock<OcrEngine>>,
    configured_tier: RwLock<Tier>,
}

impl PaddleOcrV6Extension {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(RwLock::new(OcrEngine::new())),
            configured_tier: RwLock::new(Tier::default()),
        }
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

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand::new("recognize")
                .with_display_name("Recognize Text")
                .with_description("Run PP-OCRv6 detection + recognition on an image")
                .param(param_optional(
                    "image_base64",
                    "Image (base64)",
                    "Base64-encoded image bytes (PNG/JPEG)",
                    MetricDataType::String,
                ))
                .param(param_optional(
                    "image_url",
                    "Image URL",
                    "HTTP URL to fetch image from",
                    MetricDataType::String,
                )),
            ExtensionCommand::new("switch_tier")
                .with_display_name("Switch Tier")
                .with_description("Switch to a different model tier (tiny/small/medium/auto)")
                .param({
                    let mut p = ParameterDefinition::new("tier", MetricDataType::String);
                    p.display_name = "Tier".to_string();
                    p.description = "tiny|small|medium|auto".to_string();
                    p.default_value = Some(MetricValue::String("auto".to_string()));
                    p
                }),
            ExtensionCommand::new("health")
                .with_display_name("Health Check")
                .with_description("Return model load status and current tier"),
        ]
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor::new("loaded", "Models Loaded", MetricDataType::Boolean),
            MetricDescriptor::new("tier", "Active Tier", MetricDataType::String),
        ]
    }

    async fn execute_command(
        &self,
        command: &str,
        args: &Value,
    ) -> Result<Value> {
        match command {
            "recognize" => self.cmd_recognize(args).await,
            "switch_tier" => self.cmd_switch_tier(args).await,
            "health" => Ok(self.cmd_health()),
            _ => Err(ExtensionError::InvalidArguments(format!(
                "Unknown command: '{}'. Available: recognize | switch_tier | health",
                command
            ))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl PaddleOcrV6Extension {
    async fn cmd_recognize(&self, args: &Value) -> Result<Value> {
        // Phase 6.2 implements the actual detect→crop→recognize flow.
        // For now, return a placeholder so commands route end-to-end.
        let _ = args;
        let engine = self.engine.read();
        if !engine.loaded {
            if let Some(err) = &engine.load_error {
                return Err(ExtensionError::ExecutionFailed(format!(
                    "models not loaded: {}",
                    err
                )));
            }
            return Err(ExtensionError::ExecutionFailed(
                "models not loaded — call switch_tier first".to_string(),
            ));
        }
        Ok(json!({
            "text_blocks": [],
            "full_text": "",
            "processing_time_ms": 0_u64,
            "tier": engine.tier.as_str(),
        }))
    }

    async fn cmd_switch_tier(&self, args: &Value) -> Result<Value> {
        let tier_str = args
            .get("tier")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExtensionError::InvalidArguments("missing 'tier' parameter".to_string())
            })?;
        let new_tier = Tier::from_str(tier_str)?;

        // Update configured tier
        *self.configured_tier.write() = new_tier;

        // Reload engine. NOTE: this holds the write lock during the
        // (potentially long, if download is needed) reload. Spec §6.1
        // prescribes splitting download (no lock) from reload (short lock).
        // For MVP we accept the simpler synchronous path — small/medium
        // downloads are 18MB / 132MB and happen rarely (once per tier switch).
        // TODO: split download from reload per spec §6.1 if this becomes
        // a real bottleneck for parallel recognize calls during switch.
        {
            let mut engine = self.engine.write();
            engine.ensure_loaded(new_tier, None);
        }

        let engine = self.engine.read();
        if engine.loaded {
            Ok(json!({
                "success": true,
                "tier": engine.tier.as_str(),
                "loaded": true,
            }))
        } else {
            Ok(json!({
                "success": false,
                "tier": new_tier.as_str(),
                "loaded": false,
                "error": engine.load_error.clone().unwrap_or_default(),
            }))
        }
    }

    fn cmd_health(&self) -> Value {
        let engine = self.engine.read();
        json!({
            "loaded": engine.loaded,
            "tier": engine.tier.as_str(),
            "configured_tier": self.configured_tier.read().as_str(),
            "models_dir": engine.models_dir.to_string_lossy(),
            "load_error": engine.load_error.clone(),
        })
    }
}

// Drop unused import warnings; Mutex is reserved for future producer/consumer
// pattern when integrating with metrics dispatcher.
#[allow(dead_code)]
fn _retain_mutex_link() -> Mutex<()> {
    Mutex::new(())
}

/// Helper: build an optional ParameterDefinition with display name + description.
fn param_optional(
    name: &str,
    display: &str,
    desc: &str,
    ty: MetricDataType,
) -> ParameterDefinition {
    let mut p = ParameterDefinition::new(name, ty);
    p.display_name = display.to_string();
    p.description = desc.to_string();
    p.required = false;
    p
}

neomind_extension_sdk::neomind_export!(PaddleOcrV6Extension);
