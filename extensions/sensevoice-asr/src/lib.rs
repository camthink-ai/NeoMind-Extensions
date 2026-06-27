//! NeoMind SenseVoice ASR Extension
//!
//! Speech-to-text via a Python FastAPI service wrapping `sherpa_onnx`
//! running SenseVoice-Small (INT8 ONNX). Supports Mandarin, English,
//! Japanese, Korean, Cantonese, and auto-detect.
//!
//! # Commands
//!
//! - `transcribe`       — accept base64 WAV or a local file path, return
//!                        recognized text + timing metrics.
//! - `transcribe_file`  — convenience: pass a local path only.
//! - `health`           — ping the Python service.
//! - `languages`        — list supported language hints.
//!
//! # Architecture
//!
//! The extension is a Rust cdylib. The actual ASR model runs in a separate
//! Python process (see `service/server.py`). They talk over HTTP using
//! `ureq` (sync). All blocking HTTP calls are wrapped in
//! `tokio::task::spawn_blocking` so the async executor is never stalled.

#![deny(unsafe_code)]

use async_trait::async_trait;
use base64::Engine as _;
use neomind_extension_sdk::{
    metric_float, metric_int, CommandBuilder, Extension, ExtensionCommand, ExtensionError,
    ExtensionMetadata, ExtensionMetricValue, MetricBuilder, MetricDataType, MetricDescriptor,
    ParamBuilder, ParameterDefinition, Result,
};
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_SERVICE_URL: &str = "http://127.0.0.1:9383";
const DEFAULT_LANGUAGE: &str = "auto";
const HTTP_TIMEOUT_SECS: u64 = 60;

const SUPPORTED_LANGS: &[&str] = &["auto", "zh", "en", "ja", "ko", "yue"];

// ============================================================================
// Inner shared state
// ============================================================================

struct Inner {
    service_url: RwLock<String>,
    language: RwLock<String>,
    service_ok: AtomicBool,
    total_requests: AtomicI64,
    last_latency_ms: AtomicI64,
    last_audio_duration_ms: AtomicI64,
    http_agent: ureq::Agent,
}

impl Inner {
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.service_url.read(), path)
    }

    fn check_health(&self) -> bool {
        let url = self.url("/health");
        let ok = self
            .http_agent
            .get(&url)
            .call()
            .map(|r| r.status() == 200)
            .unwrap_or(false);
        self.service_ok.store(ok, Ordering::SeqCst);
        ok
    }

    /// POST /asr with a JSON body. Returns the parsed JSON response.
    fn call_asr(&self, body: &Value) -> Result<Value> {
        let url = self.url("/asr");
        let resp = self
            .http_agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| {
                self.service_ok.store(false, Ordering::SeqCst);
                ExtensionError::ExecutionFailed(format!("HTTP /asr failed: {e}"))
            })?;

        if resp.status() != 200 {
            let status = resp.status();
            let body_text = resp.into_body().read_to_string().unwrap_or_default();
            return Err(ExtensionError::ExecutionFailed(format!(
                "service returned status {status}: {body_text}"
            )));
        }

        let latency_ms = header_f64(resp.headers(), "X-Elapsed-Seconds", 0.0) * 1000.0;
        let duration_ms = header_f64(resp.headers(), "X-Duration-Seconds", 0.0) * 1000.0;

        let parsed: Value = resp
            .into_body()
            .read_json()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("parse: {e}")))?;

        self.service_ok.store(true, Ordering::SeqCst);
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.last_latency_ms
            .store(latency_ms as i64, Ordering::SeqCst);
        self.last_audio_duration_ms
            .store(duration_ms as i64, Ordering::SeqCst);
        Ok(parsed)
    }
}

pub struct SenseVoiceAsrExtension {
    inner: Arc<Inner>,
}

impl SenseVoiceAsrExtension {
    pub fn new() -> Self {
        let service_url = std::env::var("SENSEVOICE_ASR_SERVICE_URL")
            .unwrap_or_else(|_| DEFAULT_SERVICE_URL.to_string());
        let language = std::env::var("SENSEVOICE_ASR_LANGUAGE")
            .unwrap_or_else(|_| DEFAULT_LANGUAGE.to_string());

        let http_agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS)))
            .build()
            .into();

        Self {
            inner: Arc::new(Inner {
                service_url: RwLock::new(service_url),
                language: RwLock::new(language),
                service_ok: AtomicBool::new(false),
                total_requests: AtomicI64::new(0),
                last_latency_ms: AtomicI64::new(0),
                last_audio_duration_ms: AtomicI64::new(0),
                http_agent,
            }),
        }
    }
}

impl Default for SenseVoiceAsrExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HTTP helpers
// ============================================================================

fn header_f64(headers: &ureq::http::HeaderMap, name: &str, default: f64) -> f64 {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(default)
}

/// Read a local audio file and return its bytes + extension-derived mime.
fn read_local_audio(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| {
        ExtensionError::InvalidArguments(format!("cannot read audio_path `{path}`: {e}"))
    })
}

/// Encode bytes as a base64 string.
fn b64_encode(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

// ============================================================================
// Extension trait
// ============================================================================

#[async_trait]
impl Extension for SenseVoiceAsrExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("sensevoice-asr", "SenseVoice ASR", "2.7.6")
                .with_description(
                    "Multilingual ASR (zh/en/ja/ko/yue) via SenseVoice-Small ONNX. \
                     Returns recognized text from audio files or base64 WAV.",
                )
                .with_author("NeoMind Team")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("service_ok", "Service OK").boolean().build(),
            MetricBuilder::new("total_requests", "Total Requests").integer().build(),
            MetricBuilder::new("last_latency_ms", "Last Latency")
                .float()
                .unit("ms")
                .build(),
            MetricBuilder::new("last_audio_duration_ms", "Last Audio Duration")
                .float()
                .unit("ms")
                .build(),
            MetricBuilder::new("rtf", "Real-Time Factor").float().build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("transcribe")
                .display_name("Transcribe Audio")
                .description(
                    "Transcribe audio via SenseVoice. Accepts either a local file path \
                     (`audio_path`) or a base64-encoded WAV (`audio_base64`). Returns text.",
                )
                .param(
                    ParamBuilder::new("audio_path", MetricDataType::String)
                        .display_name("Audio File Path")
                        .description("Local audio file path (wav/mp3/m4a/flac). Mutually exclusive with audio_base64.")
                        .build(),
                )
                .param(
                    ParamBuilder::new("audio_base64", MetricDataType::String)
                        .display_name("Audio Base64 (WAV)")
                        .description("Base64-encoded WAV bytes. Mutually exclusive with audio_path.")
                        .build(),
                )
                .param(language_param())
                .param(
                    ParamBuilder::new("use_itn", MetricDataType::Boolean)
                        .display_name("Inverse Text Normalization")
                        .description("Apply ITN (e.g. convert '123' from spoken form to digits).")
                        .default(true.into())
                        .build(),
                )
                .sample(json!({"audio_path":"/tmp/test.wav","language":"auto"}))
                .build(),
            CommandBuilder::new("transcribe_file")
                .display_name("Transcribe File")
                .description("Convenience wrapper: transcribe a local audio file by path.")
                .param(
                    ParamBuilder::new("path", MetricDataType::String)
                        .display_name("File Path")
                        .description("Local audio file path.")
                        .required()
                        .build(),
                )
                .param(language_param())
                .sample(json!({"path":"/tmp/test.wav"}))
                .build(),
            CommandBuilder::new("health")
                .display_name("Health Check")
                .description("Ping the Python ASR service and report reachability.")
                .build(),
            CommandBuilder::new("languages")
                .display_name("List Languages")
                .description("List supported language hints.")
                .build(),
        ]
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "transcribe" => {
                let body = self.build_asr_body(args, "audio_path", "audio_base64")?;
                let inner = self.inner.clone();
                tokio::task::spawn_blocking(move || inner.call_asr(&body))
                    .await
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?
            }
            "transcribe_file" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("missing `path`".into()))?;
                let default_lang = self.inner.language.read().clone();
                let language = args
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&default_lang)
                    .to_string();
                let bytes = read_local_audio(path)?;
                let body = json!({
                    "audio_base64": b64_encode(&bytes),
                    "language": language,
                    "use_itn": args.get("use_itn").and_then(|v| v.as_bool()).unwrap_or(true),
                });
                let inner = self.inner.clone();
                tokio::task::spawn_blocking(move || inner.call_asr(&body))
                    .await
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?
            }
            "health" => {
                let inner = self.inner.clone();
                let ok = tokio::task::spawn_blocking(move || inner.check_health())
                    .await
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?;
                Ok(json!({"ok": ok, "service_url": *self.inner.service_url.read()}))
            }
            "languages" => {
                let inner = self.inner.clone();
                tokio::task::spawn_blocking(move || -> Result<Value> {
                    let url = inner.url("/languages");
                    let resp = inner
                        .http_agent
                        .get(&url)
                        .call()
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("HTTP /languages: {e}")))?;
                    let body: Value = resp
                        .into_body()
                        .read_json()
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("parse: {e}")))?;
                    Ok(body)
                })
                .await
                .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?
            }
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let latency = self.inner.last_latency_ms.load(Ordering::SeqCst) as f64;
        let duration = self.inner.last_audio_duration_ms.load(Ordering::SeqCst) as f64;
        let rtf = if duration > 0.0 { latency / duration } else { 0.0 };

        Ok(vec![
            metric_int!("service_ok", self.inner.service_ok.load(Ordering::SeqCst) as i64),
            metric_int!("total_requests", self.inner.total_requests.load(Ordering::SeqCst)),
            metric_float!("last_latency_ms", latency),
            metric_float!("last_audio_duration_ms", duration),
            metric_float!("rtf", rtf),
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl SenseVoiceAsrExtension {
    /// Build the JSON body for /asr from either `audio_path` or
    /// `audio_base64` in `args`. Reads + base64-encodes the file if path
    /// is provided. Returns ExtensionError if both/neither are set.
    fn build_asr_body(&self, args: &Value, path_key: &str, b64_key: &str) -> Result<Value> {
        let audio_path = args.get(path_key).and_then(|v| v.as_str());
        let audio_base64 = args.get(b64_key).and_then(|v| v.as_str());

        let b64 = match (audio_path, audio_base64) {
            (Some(path), None) => {
                let bytes = read_local_audio(path)?;
                b64_encode(&bytes)
            }
            (None, Some(b64)) => b64.to_string(),
            (Some(_), Some(_)) => {
                return Err(ExtensionError::InvalidArguments(format!(
                    "provide only one of `{path_key}` or `{b64_key}`"
                )))
            }
            (None, None) => {
                return Err(ExtensionError::InvalidArguments(format!(
                    "must provide `{path_key}` or `{b64_key}`"
                )))
            }
        };

        let default_lang = self.inner.language.read().clone();
        let language = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or(&default_lang)
            .to_string();

        if !SUPPORTED_LANGS.contains(&language.as_str()) {
            return Err(ExtensionError::InvalidArguments(format!(
                "unsupported language `{language}`; supported: {:?}",
                SUPPORTED_LANGS
            )));
        }

        Ok(json!({
            "audio_base64": b64,
            "language": language,
            "use_itn": args.get("use_itn").and_then(|v| v.as_bool()).unwrap_or(true),
        }))
    }
}

fn language_param() -> ParameterDefinition {
    ParamBuilder::new("language", MetricDataType::String)
        .display_name("Language")
        .description("Language hint. `auto` works for mixed-language content.")
        .options(SUPPORTED_LANGS.iter().map(|s| (*s).to_string()).collect())
        .default("auto".into())
        .build()
}

// FFI export — generates all required `_neomind_extension_*` symbols.
neomind_extension_sdk::neomind_export!(SenseVoiceAsrExtension);
