//! NeoMind Voice-Edge-TTS Extension
//!
//! Edge TTS via sherpa-onnx ZipVoice — cross-platform CPU streaming.
//!
//! # Commands
//!
//! - `synthesize`   — call service, return full WAV as base64 (no playback).
//!                    Use this when the caller wants the bytes (UI cards,
//!                    recording, forwarding).
//! - `speak`        — stream PCM chunks from the service and play them
//!                    directly on the host audio device via `rodio`.
//!                    No UI required. Ideal for edge devices / kiosks /
//!                    Agent voice replies.
//! - `stop_speaking`— stop current playback immediately.
//! - `list_voices`  — list built-in voice presets from the service.
//! - `health`       — ping the Python service.
//!
//! # Architecture
//!
//! The extension is a Rust cdylib. The actual TTS model runs in a separate
//! Python process (see `service/server.py`). They talk over HTTP using
//! `ureq` (sync). All blocking HTTP calls are wrapped in
//! `tokio::task::spawn_blocking` so the async executor is never stalled.
//!
//! Audio playback happens on a single dedicated audio thread (see
//! `audio_thread`) that owns the `rodio::OutputStream`. This avoids
//! CoreAudio's "must be on main thread / a single thread" requirement
//! while keeping the rest of the extension async-friendly.

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
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_SERVICE_URL: &str = "http://127.0.0.1:9386";
const DEFAULT_VOICE: &str = "中文女";
const DEFAULT_SAMPLE_MODE: &str = "greedy";
const DEFAULT_SAMPLE_RATE: u32 = 24000;
const DEFAULT_CHANNELS: u16 = 1;
const HTTP_TIMEOUT_SECS: u64 = 120;

// ============================================================================
// Inner shared state (Arc-wrapped so spawn_blocking tasks can capture it)
// ============================================================================

struct Inner {
    service_url: RwLock<String>,
    voice: RwLock<String>,
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
}

pub struct VoiceEdgeTtsExtension {
    inner: Arc<Inner>,
}

impl VoiceEdgeTtsExtension {
    pub fn new() -> Self {
        let service_url = std::env::var("VOICE_EDGE_TTS_SERVICE_URL")
            .unwrap_or_else(|_| DEFAULT_SERVICE_URL.to_string());
        let voice = std::env::var("VOICE_EDGE_TTS_VOICE")
            .unwrap_or_else(|_| DEFAULT_VOICE.to_string());

        let http_agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS)))
            .build()
            .into();

        Self {
            inner: Arc::new(Inner {
                service_url: RwLock::new(service_url),
                voice: RwLock::new(voice),
                service_ok: AtomicBool::new(false),
                total_requests: AtomicI64::new(0),
                last_latency_ms: AtomicI64::new(0),
                last_audio_duration_ms: AtomicI64::new(0),
                http_agent,
            }),
        }
    }
}

impl Default for VoiceEdgeTtsExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HTTP helpers
// ============================================================================

/// Decode base64 → bytes.
fn b64_decode(s: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| ExtensionError::InvalidArguments(format!("base64 decode: {e}")))
}

/// Convert raw little-endian i16 bytes into a Vec<i16>.
fn bytes_to_i16_le(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Read an X-* header as f64 from a HeaderMap, falling back to `default`.
fn header_f64(
    headers: &ureq::http::HeaderMap,
    name: &str,
    default: f64,
) -> f64 {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(default)
}

fn header_i64(
    headers: &ureq::http::HeaderMap,
    name: &str,
    default: i64,
) -> i64 {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(default)
}

impl Inner {
    /// POST /tts — returns the full WAV. The service writes the X-* latency /
    /// duration headers we propagate to metrics + the returned payload.
    fn call_synthesize(&self, body: &Value) -> Result<Value> {
        let url = self.url("/tts");
        let resp = self
            .http_agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| {
                self.service_ok.store(false, Ordering::SeqCst);
                ExtensionError::ExecutionFailed(format!("HTTP /tts failed: {e}"))
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
        let sample_rate =
            header_i64(resp.headers(), "X-Sample-Rate", DEFAULT_SAMPLE_RATE as i64);

        let wav = resp
            .into_body()
            .read_to_vec()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("read body: {e}")))?;

        self.service_ok.store(true, Ordering::SeqCst);
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.last_latency_ms
            .store(latency_ms as i64, Ordering::SeqCst);
        self.last_audio_duration_ms
            .store(duration_ms as i64, Ordering::SeqCst);

        let b64 = base64::engine::general_purpose::STANDARD.encode(&wav);

        Ok(json!({
            "audio_base64": b64,
            "format": "wav",
            "sample_rate": sample_rate,
            "latency_ms": latency_ms as i64,
            "duration_ms": duration_ms as i64,
            "size_bytes": wav.len(),
        }))
    }

    /// POST /tts/stream — read NDJSON lines and emit each chunk's PCM samples
    /// through `on_chunk`. Returns (frames_emitted, total_samples_played).
    fn stream_to_callback<F>(&self, body: &Value, mut on_chunk: F) -> Result<(usize, usize)>
    where
        F: FnMut(&[i16], u32, u16),
    {
        let url = self.url("/tts/stream");
        let resp = self
            .http_agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| {
                self.service_ok.store(false, Ordering::SeqCst);
                ExtensionError::ExecutionFailed(format!("HTTP /tts/stream failed: {e}"))
            })?;

        if resp.status() != 200 {
            let status = resp.status();
            let body_text = resp.into_body().read_to_string().unwrap_or_default();
            return Err(ExtensionError::ExecutionFailed(format!(
                "service returned status {status}: {body_text}"
            )));
        }

        let reader = BufReader::new(resp.into_body().into_reader());
        let mut frames = 0usize;
        let mut total_samples = 0usize;

        for line in reader.lines() {
            let line: String = line.map_err(|e| {
                ExtensionError::ExecutionFailed(format!("read stream line: {e}"))
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let chunk: Value = serde_json::from_str(&line).map_err(|e| {
                ExtensionError::ExecutionFailed(format!("parse NDJSON chunk: {e}"))
            })?;

            let data_b64 = chunk
                .get("data")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ExtensionError::ExecutionFailed("missing `data` in chunk".into()))?;
            let pcm = b64_decode(data_b64)?;
            let samples = bytes_to_i16_le(&pcm);
            let sr = chunk
                .get("sample_rate")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_SAMPLE_RATE as u64) as u32;
            let ch = chunk
                .get("channels")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_CHANNELS as u64) as u16;

            on_chunk(&samples, sr, ch);
            frames += 1;
            total_samples += samples.len();
        }

        self.service_ok.store(true, Ordering::SeqCst);
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        Ok((frames, total_samples))
    }
}

// ============================================================================
// Audio playback thread
// ----------------------------------------------------------------------------
// A single dedicated audio thread owns the `rodio::OutputStream`. We push
// PCM frames to it through a channel. CoreAudio (macOS) and most other
// backends require the output stream to live on one stable thread.
// ============================================================================

enum AudioCmd {
    /// Start a new playback session. `done` is signalled when the session
    /// finishes (either naturally or by stop/end_session).
    StartSession { done: Sender<SessionResult> },
    /// Append PCM samples to the current session's sink.
    Chunk {
        samples: Vec<i16>,
        sample_rate: u32,
        channels: u16,
    },
    /// End-of-stream: wait for playback to drain, then signal `done`.
    EndSession,
    /// Immediate stop: clear the sink, drop buffered audio.
    Stop,
}

#[derive(Debug, Clone)]
struct SessionResult {
    played: bool,
    error: Option<String>,
}

impl SessionResult {
    fn ok() -> Self {
        Self {
            played: true,
            error: None,
        }
    }
    fn err(msg: impl Into<String>) -> Self {
        Self {
            played: false,
            error: Some(msg.into()),
        }
    }
}

/// Get (or lazily spawn) the global audio-thread sender.
///
/// The sender is stored in a `OnceLock` for process lifetime. The first
/// caller spawns the audio thread; subsequent callers reuse it.
fn audio_sender() -> Result<&'static Sender<AudioCmd>> {
    static AUDIO_TX: OnceLock<Sender<AudioCmd>> = OnceLock::new();
    if let Some(tx) = AUDIO_TX.get() {
        return Ok(tx);
    }

    let (tx, rx) = mpsc::channel::<AudioCmd>();
    let spawned = std::thread::Builder::new()
        .name("voice-edge-tts-audio".into())
        .spawn(move || {
            audio_thread(rx);
        })
        .map_err(|e| {
            ExtensionError::ExecutionFailed(format!("spawn audio thread: {e}"))
        });

    match spawned {
        Ok(_) => {
            // get_or_init cannot fail since we just constructed the sender.
            let _ = AUDIO_TX.set(tx);
            AUDIO_TX
                .get()
                .ok_or_else(|| ExtensionError::ExecutionFailed("audio thread init failed".into()))
        }
        Err(e) => Err(e),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn audio_thread(rx: Receiver<AudioCmd>) {
    use rodio::{OutputStream, OutputStreamHandle, Sink};
    // `Source` trait is implicitly in scope via SamplesBuffer's blanket impl.
    // Open the output stream on this thread. If it fails we drain the
    // channel so senders don't block forever and then exit.
    let (_stream, handle): (OutputStream, OutputStreamHandle) = match rodio_open_output_stream() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[voice-edge-tts] open audio device failed: {e}");
            drain_channel_failing(rx);
            return;
        }
    };

    let mut current: Option<(Sink, Option<Sender<SessionResult>>)> = None;

    for cmd in rx {
        match cmd {
            AudioCmd::StartSession { done } => {
                // Replace any existing session (drop its sink, signal previous done).
                if let Some((sink, prev_done)) = current.take() {
                    sink.stop();
                    let _ = prev_done.map(|s| s.send(SessionResult::ok()));
                }
                match Sink::try_new(&handle) {
                    Ok(sink) => {
                        current = Some((sink, Some(done)));
                    }
                    Err(e) => {
                        let _ = done.send(SessionResult::err(format!("new sink: {e}")));
                    }
                }
            }
            AudioCmd::Chunk {
                samples,
                sample_rate,
                channels,
            } => {
                if let Some((sink, _)) = &current {
                    let buf = rodio::buffer::SamplesBuffer::new(channels, sample_rate, samples);
                    sink.append(buf);
                }
            }
            AudioCmd::EndSession => {
                if let Some((sink, done)) = current.take() {
                    sink.sleep_until_end();
                    sink.detach();
                    let _ = done.map(|s| s.send(SessionResult::ok()));
                }
            }
            AudioCmd::Stop => {
                if let Some((sink, done)) = current.take() {
                    sink.stop();
                    let _ = done.map(|s| s.send(SessionResult::ok()));
                }
            }
        }
    }
    // `_stream` dropped here when the channel closes (process teardown).
    let _ = _stream;
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn rodio_open_output_stream()
    -> std::result::Result<(rodio::OutputStream, rodio::OutputStreamHandle), String>
{
    rodio::OutputStream::try_default().map_err(|e| format!("{e:?}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn audio_thread(_rx: Receiver<AudioCmd>) {
    // No audio backend on this platform (e.g. wasm). Drain and fail all sessions.
    drain_channel_failing(_rx);
}

/// Drain the channel, signalling failure to every StartSession sender.
/// Used when the audio device cannot be opened or the platform has no
/// audio backend.
fn drain_channel_failing(rx: Receiver<AudioCmd>) {
    for cmd in rx {
        if let AudioCmd::StartSession { done } = cmd {
            let _ = done.send(SessionResult::err("audio device unavailable"));
        }
    }
}

// ============================================================================
// Speak implementation
// ============================================================================

impl Inner {
    /// Blocking speak: stream from Python service, push PCM to audio thread.
    /// If `wait_for_completion` is true, block until playback finishes.
    fn speak_blocking(
        &self,
        text: &str,
        voice: &str,
        sample_mode: &str,
        wait_for_completion: bool,
    ) -> Result<Value> {
        let tx = audio_sender()?;
        let (done_tx, done_rx) = mpsc::channel::<SessionResult>();

        // Start session.
        tx.send(AudioCmd::StartSession {
            done: done_tx.clone(),
        })
        .map_err(|e| ExtensionError::ExecutionFailed(format!("audio thread dead: {e}")))?;

        // Stream chunks.
        let body = json!({
            "text": text,
            "voice": voice,
            "sample_mode": sample_mode,
        });

        let (frames, total_samples) =
            self.stream_to_callback(&body, |samples, sr, ch| {
                let _ = tx.send(AudioCmd::Chunk {
                    samples: samples.to_vec(),
                    sample_rate: sr,
                    channels: ch,
                });
            })?;

        // End-of-stream marker.
        let _ = tx.send(AudioCmd::EndSession);

        if wait_for_completion {
            let res = done_rx
                .recv()
                .map_err(|e| ExtensionError::ExecutionFailed(format!("audio result: {e}")))?;
            if let Some(err) = res.error {
                return Err(ExtensionError::ExecutionFailed(err));
            }
            Ok(json!({
                "played": res.played,
                "finished": true,
                "frames": frames,
                "samples": total_samples,
                "duration_ms": (total_samples as f64 / DEFAULT_CHANNELS as f64
                    / DEFAULT_SAMPLE_RATE as f64
                    * 1000.0) as i64,
            }))
        } else {
            // Detach: background thread consumes the result signal so the
            // audio thread's `done` send doesn't block forever.
            std::thread::spawn(move || {
                let _ = done_rx.recv();
            });
            Ok(json!({
                "played": true,
                "finished": false,
                "background": true,
                "frames": frames,
                "samples": total_samples,
            }))
        }
    }

    fn stop_speaking(&self) -> Result<Value> {
        match audio_sender() {
            Ok(tx) => {
                let _ = tx.send(AudioCmd::Stop);
                Ok(json!({"stopped": true}))
            }
            Err(_) => Ok(json!({"stopped": false, "reason": "audio thread not initialized"})),
        }
    }
}

// ============================================================================
// Extension trait
// ============================================================================

#[async_trait]
impl Extension for VoiceEdgeTtsExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("voice-edge-tts", "Voice Edge TTS", "2.7.6")
                .with_description(
                    "Edge TTS via sherpa-onnx ZipVoice — cross-platform CPU streaming. \
                     `speak` plays on the host audio device; `synthesize` returns WAV.",
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
            MetricBuilder::new("rtf", "Real-Time Factor")
                .float()
                .build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("synthesize")
                .display_name("Synthesize Speech (WAV)")
                .description("Synthesize speech via sherpa-onnx ZipVoice and return WAV as base64. Does NOT play on host audio.")
                .param(
                    ParamBuilder::new("text", MetricDataType::String)
                        .display_name("Text")
                        .description("Text to synthesize.")
                        .build(),
                )
                .param(voice_param())
                .param(
                    optional_string("prompt_audio_path", "Reference Audio Path",
                        "Local wav path used for voice cloning. Overrides `voice`."),
                )
                .param(
                    ParamBuilder::new("sample_mode", MetricDataType::String)
                        .display_name("Sampling Mode")
                        .description("greedy / fixed / full")
                        .options(vec!["greedy".into(), "fixed".into(), "full".into()])
                        .default("greedy".into())
                        .build(),
                )
                .sample(json!({"text":"你好，世界","voice":"中文女"}))
                .build(),
            CommandBuilder::new("speak")
                .display_name("Speak (Host Audio Playback)")
                .description("Synthesize and play directly on the host audio device. No UI required. Ideal for edge devices / Agent voice replies.")
                .param(
                    ParamBuilder::new("text", MetricDataType::String)
                        .display_name("Text")
                        .description("Text to speak.")
                        .build(),
                )
                .param(voice_param())
                .param(
                    optional_string("prompt_audio_path", "Reference Audio Path",
                        "Local wav path used for voice cloning. Overrides `voice`."),
                )
                .param(
                    ParamBuilder::new("sample_mode", MetricDataType::String)
                        .display_name("Sampling Mode")
                        .description("greedy = deterministic output (recommended for agents); fixed = light sampling; full = high randomness")
                        .options(vec!["greedy".into(), "fixed".into(), "full".into()])
                        .default("greedy".into())
                        .build(),
                )
                .param(
                    ParamBuilder::new("blocking", MetricDataType::Boolean)
                        .display_name("Block Until Finished")
                        .description("If true, the command returns only after playback finishes. If false, playback runs in the background.")
                        .default(true.into())
                        .build(),
                )
                .sample(json!({"text":"你好，世界","voice":"中文女","blocking":true}))
                .build(),
            CommandBuilder::new("stop_speaking")
                .display_name("Stop Playback")
                .description("Stop current TTS playback immediately and clear buffered audio.")
                .build(),
            CommandBuilder::new("list_voices")
                .display_name("List Voices")
                .description("List built-in voice presets from the Python service.")
                .build(),
            CommandBuilder::new("health")
                .display_name("Health Check")
                .description("Ping the Python TTS service and report reachability.")
                .build(),
        ]
    }

    async fn execute_command(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "synthesize" => {
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("missing `text`".into()))?;
                let default_voice = self.inner.voice.read().clone();
                let voice = args
                    .get("voice")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&default_voice);
                let prompt_audio_path = args.get("prompt_audio_path").and_then(|v| v.as_str());
                let sample_mode = args
                    .get("sample_mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or(DEFAULT_SAMPLE_MODE);
                let body = json!({
                    "text": text,
                    "voice": voice,
                    "prompt_audio_path": prompt_audio_path,
                    "sample_mode": sample_mode,
                    "response_format": "wav",
                });
                let inner = self.inner.clone();
                tokio::task::spawn_blocking(move || inner.call_synthesize(&body))
                    .await
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?
            }
            "speak" => {
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ExtensionError::InvalidArguments("missing `text`".into()))?;
                let default_voice = self.inner.voice.read().clone();
                let voice = args
                    .get("voice")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&default_voice)
                    .to_string();
                let sample_mode = args
                    .get("sample_mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or(DEFAULT_SAMPLE_MODE)
                    .to_string();
                let blocking = args
                    .get("blocking")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let text = text.to_string();
                let inner = self.inner.clone();
                tokio::task::spawn_blocking(move || {
                    inner.speak_blocking(&text, &voice, &sample_mode, blocking)
                })
                .await
                .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?
            }
            "stop_speaking" => {
                let inner = self.inner.clone();
                tokio::task::spawn_blocking(move || inner.stop_speaking())
                    .await
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?
            }
            "list_voices" => {
                let inner = self.inner.clone();
                tokio::task::spawn_blocking(move || -> Result<Value> {
                    let url = inner.url("/voices");
                    let resp = inner
                        .http_agent
                        .get(&url)
                        .call()
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("HTTP /voices: {e}")))?;
                    let body: Value = resp
                        .into_body()
                        .read_json()
                        .map_err(|e| ExtensionError::ExecutionFailed(format!("parse: {e}")))?;
                    Ok(body)
                })
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
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let latency = self.inner.last_latency_ms.load(Ordering::SeqCst) as f64;
        let duration = self.inner.last_audio_duration_ms.load(Ordering::SeqCst) as f64;
        let rtf = if duration > 0.0 { latency / duration } else { 0.0 };

        Ok(vec![
            metric_int!(
                "service_ok",
                self.inner.service_ok.load(Ordering::SeqCst) as i64
            ),
            metric_int!(
                "total_requests",
                self.inner.total_requests.load(Ordering::SeqCst)
            ),
            metric_float!("last_latency_ms", latency),
            metric_float!("last_audio_duration_ms", duration),
            metric_float!("rtf", rtf),
        ])
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Built-in voice presets. Mirror of sherpa-onnx ZipVoice as of 2025-06.
const BUILTIN_VOICES: &[&str] = &["中文女"];

fn voice_param() -> ParameterDefinition {
    ParamBuilder::new("voice", MetricDataType::String)
        .display_name("Voice")
        .description("Built-in voice preset. Overrides `VOICE_EDGE_TTS_VOICE` default.")
        .options(BUILTIN_VOICES.iter().map(|s| (*s).to_string()).collect())
        .default("中文女".into())
        .build()
}

// Helper: build an optional (not required) string ParameterDefinition.
fn optional_string(name: &str, display_name: &str, description: &str) -> ParameterDefinition {
    let mut p = ParamBuilder::new(name, MetricDataType::String)
        .display_name(display_name)
        .description(description)
        .build();
    p.required = false;
    p
}

// FFI export — generates all required `_neomind_extension_*` symbols.
neomind_extension_sdk::neomind_export!(VoiceEdgeTtsExtension);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_sane() {
        let ext = VoiceEdgeTtsExtension::new();
        let m = ext.metadata();
        assert_eq!(m.id, "voice-edge-tts");
        assert_eq!(m.version, "2.7.6");
    }

    #[test]
    fn commands_present() {
        let ext = VoiceEdgeTtsExtension::new();
        let cmds = ext.commands();
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"synthesize"));
        assert!(names.contains(&"speak"));
        assert!(names.contains(&"stop_speaking"));
        assert!(names.contains(&"list_voices"));
        assert!(names.contains(&"health"));
    }

    #[test]
    fn bytes_to_i16_le_roundtrip() {
        let samples = vec![0i16, 1, -1, 32767, -32768];
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in &samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let decoded = bytes_to_i16_le(&bytes);
        assert_eq!(decoded, samples);
    }
}
