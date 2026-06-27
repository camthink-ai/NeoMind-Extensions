//! NeoMind Voice Assistant Extension (PoC)
//!
//! Real-time voice conversation orchestrator. Acts as a thin Rust proxy
//! between the browser (via StreamCapability) and a Python orchestrator
//! service (via WebSocket) that owns the full pipeline:
//!
//!   mic PCM → Silero VAD → sensevoice-asr HTTP → echo reply →
//!   moss-tts-nano /tts/stream HTTP → PCM chunks back
//!
//! All heavy lifting (VAD, ASR HTTP client, TTS HTTP client) lives in
//! the Python service. The extension just routes bytes both ways.
//!
//! # Architecture
//!
//! ```text
//! Browser ──── PCM chunks ────► Extension ──── ws text/binary ────► Python
//!   ▲                              (Rust)                            │
//!   │                                                                │
//!   └──── PCM chunks ◄──── send_push_output ◄──── ws recv ◄─────┘
//! ```
//!
//! # Stream protocol (extension ↔ Python)
//!
//! - Browser → extension: raw bytes via `process_session_chunk` (PCM int16 LE)
//! - Extension → Python: binary WS frames = PCM bytes; text WS frames = JSON control
//! - Python → extension: binary WS frames = PCM bytes; text WS frames = JSON events
//! - Extension → browser: `send_push_output` with mime `audio/pcm`
//!
//! Barge-in (PoC): Python sets `{"type":"stop"}` event; extension invalidates
//! session_id; old pipeline loop in Python checks session_id before each stage.

#![deny(unsafe_code)]

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use neomind_extension_sdk::{
    metric_bool, metric_int, send_push_output, CommandBuilder, Extension, ExtensionCommand,
    ExtensionError, ExtensionMetadata, ExtensionMetricValue, FlowControl, MetricBuilder,
    MetricDescriptor, PushOutputMessage, Result, StreamCapability, StreamDataType,
    StreamDirection, StreamMode, StreamResult, StreamSession,
};
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::mpsc;

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_ORCHESTRATOR_URL: &str = "ws://127.0.0.1:9384/ws";
const PCM_MIME: &str = "audio/pcm";

// ============================================================================
// Inner shared state
// ============================================================================

struct Inner {
    orchestrator_url: RwLock<String>,
    service_ok: AtomicBool,
    active_sessions: AtomicI64,
    total_bytes_in: AtomicU64,
    total_bytes_out: AtomicU64,
    /// Sender halves for each active session, used by `process_session_chunk`
    /// to forward browser PCM to the Python orchestrator WS.
    session_senders: tokio::sync::RwLock<std::collections::HashMap<String, mpsc::Sender<Vec<u8>>>>,
}

impl Inner {
    fn check_health(&self) -> bool {
        // Quick WS ping — just try to connect. We do not keep the connection.
        let url = self.orchestrator_url.read().clone();
        let ok = tokio::task::block_in_place(|| {
            let rt = match tokio::runtime::Handle::try_current() {
                Ok(h) => h,
                Err(_) => return false,
            };
            rt.block_on(async move {
                use tokio_tungstenite::tungstenite::Message;
                let mut ws = match tokio_tungstenite::connect_async(&url).await {
                    Ok((w, _)) => w,
                    Err(_) => return false,
                };
                let _ = ws.send(Message::Text(
                    json!({"type":"ping"}).to_string(),
                ))
                .await;
                let _ = ws.close(None).await;
                true
            })
        });
        self.service_ok.store(ok, Ordering::SeqCst);
        ok
    }
}

pub struct VoiceAssistantExtension {
    inner: Arc<Inner>,
}

impl VoiceAssistantExtension {
    pub fn new() -> Self {
        let orchestrator_url = std::env::var("VOICE_ASSISTANT_ORCHESTRATOR_URL")
            .unwrap_or_else(|_| DEFAULT_ORCHESTRATOR_URL.to_string());
        Self {
            inner: Arc::new(Inner {
                orchestrator_url: RwLock::new(orchestrator_url),
                service_ok: AtomicBool::new(false),
                active_sessions: AtomicI64::new(0),
                total_bytes_in: AtomicU64::new(0),
                total_bytes_out: AtomicU64::new(0),
                session_senders: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            }),
        }
    }
}

impl Default for VoiceAssistantExtension {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Per-session state
// ============================================================================

/// Spawns the background pump that:
///   1. Reads from the browser→Python mpsc channel, forwards as binary WS frames
///   2. Reads incoming WS messages from Python, pushes them to browser via
///      `send_push_output`. Handles text frames as control events.
async fn run_session_pump(
    session_id: String,
    ws_url: String,
    browser_rx: mpsc::Receiver<Vec<u8>>,
    inner: Arc<Inner>,
) -> Result<()> {
    inner.active_sessions.fetch_add(1, Ordering::SeqCst);

    // Open WS to orchestrator with session_id in subprotocol / query.
    let url = format!("{}?session_id={}", ws_url, session_id);
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| ExtensionError::ExecutionFailed(format!("ws connect: {e}")))?;

    // Send initial config so Python knows sample rate etc.
    let start_msg = tokio_tungstenite::tungstenite::Message::Text(
        json!({
            "type": "start",
            "session_id": session_id,
            "sample_rate": 16000,
            "channels": 1,
            "format": "pcm_int16_le",
        })
        .to_string(),
    );
    let _ = ws.send(start_msg).await;

    let mut browser_rx = browser_rx;

    let mut out_seq: u64 = 0;
    let mut closed = false;

    loop {
        tokio::select! {
            // Browser → Python
            Some(pcm) = browser_rx.recv(), if !closed => {
                if ws.send(tokio_tungstenite::tungstenite::Message::Binary(pcm)).await.is_err() {
                    break;
                }
            }
            // Python → Browser
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(pcm))) => {
                        out_seq += 1;
                        inner.total_bytes_out.fetch_add(pcm.len() as u64, Ordering::SeqCst);
                        let m = PushOutputMessage::new(
                            &session_id,
                            out_seq,
                            pcm.to_vec(),
                            PCM_MIME,
                        );
                        let _ = send_push_output(&m);
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(txt))) => {
                        // Control events from Python (e.g., {"type":"transcript","text":"..."})
                        // Push as JSON metadata so the browser can display transcripts.
                        out_seq += 1;
                        if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                            let evt_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("event");
                            // Only forward non-ping events to host.
                            if evt_type != "pong" {
                                let m = match PushOutputMessage::json(
                                    &session_id, out_seq, v.clone(),
                                ) {
                                    Ok(m) => m,
                                    Err(_) => continue,
                                };
                                let _ = send_push_output(&m);
                            }
                            // stop event: orchestrator finished / cancelled
                            if evt_type == "stop" || evt_type == "end" {
                                closed = true;
                            }
                        }
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                    Some(Ok(_)) => {} // Ping/Pong/Frame — ignore
                    Some(Err(_)) => break,
                    None => break,
                }
            }
        }
    }

    inner.active_sessions.fetch_sub(1, Ordering::SeqCst);
    inner.session_senders.write().await.remove(&session_id);
    Ok(())
}

// ============================================================================
// Extension trait
// ============================================================================

#[async_trait]
impl Extension for VoiceAssistantExtension {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: OnceLock<ExtensionMetadata> = OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("voice-assistant", "Voice Assistant", "2.7.6")
                .with_description(
                    "Real-time voice conversation orchestrator. Browser mic PCM → VAD → ASR → reply → TTS → browser speaker.",
                )
                .with_author("NeoMind Team")
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricBuilder::new("service_ok", "Service OK").boolean().build(),
            MetricBuilder::new("active_sessions", "Active Sessions").integer().build(),
            MetricBuilder::new("total_bytes_in", "Total Bytes In")
                .integer()
                .unit("bytes")
                .build(),
            MetricBuilder::new("total_bytes_out", "Total Bytes Out")
                .integer()
                .unit("bytes")
                .build(),
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            CommandBuilder::new("health")
                .display_name("Health Check")
                .description("Ping the Python orchestrator service.")
                .build(),
            CommandBuilder::new("status")
                .display_name("Status")
                .description("Report active sessions and byte counters.")
                .build(),
        ]
    }

    async fn execute_command(&self, command: &str, _args: &Value) -> Result<Value> {
        match command {
            "health" => {
                let inner = self.inner.clone();
                let ok = tokio::task::spawn_blocking(move || inner.check_health())
                    .await
                    .map_err(|e| ExtensionError::ExecutionFailed(format!("join: {e}")))?;
                Ok(json!({
                    "ok": ok,
                    "orchestrator_url": *self.inner.orchestrator_url.read(),
                }))
            }
            "status" => Ok(json!({
                "service_ok": self.inner.service_ok.load(Ordering::SeqCst),
                "active_sessions": self.inner.active_sessions.load(Ordering::SeqCst),
                "total_bytes_in": self.inner.total_bytes_in.load(Ordering::SeqCst),
                "total_bytes_out": self.inner.total_bytes_out.load(Ordering::SeqCst),
            })),
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        Ok(vec![
            metric_bool!("service_ok", self.inner.service_ok.load(Ordering::SeqCst)),
            metric_int!("active_sessions", self.inner.active_sessions.load(Ordering::SeqCst)),
            metric_int!("total_bytes_in", self.inner.total_bytes_in.load(Ordering::SeqCst) as i64),
            metric_int!("total_bytes_out", self.inner.total_bytes_out.load(Ordering::SeqCst) as i64),
        ])
    }

    // --- Stream capability ---

    fn stream_capability(&self) -> Option<StreamCapability> {
        Some(StreamCapability {
            direction: StreamDirection::Bidirectional,
            mode: StreamMode::Push,
            supported_data_types: vec![
                StreamDataType::Audio {
                    format: "pcm".to_string(),
                    sample_rate: 16000,
                    channels: 1,
                },
                StreamDataType::Binary,
                StreamDataType::Json,
            ],
            max_chunk_size: 32 * 1024,
            preferred_chunk_size: 3200, // 100ms @ 16kHz mono int16
            max_concurrent_sessions: 4,
            flow_control: FlowControl::default_stream(),
            config_schema: None,
        })
    }

    async fn init_session(&self, session: &StreamSession) -> Result<()> {
        let session_id = session.id.clone();
        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
        self.inner
            .session_senders
            .write()
            .await
            .insert(session_id.clone(), tx);

        let ws_url = self.inner.orchestrator_url.read().clone();
        let inner = self.inner.clone();
        // Pump task lives for the whole session.
        tokio::spawn(async move {
            if let Err(e) = run_session_pump(session_id.clone(), ws_url, rx, inner).await {
                tracing::error!("voice-assistant session {} pump ended: {}", session_id, e);
            }
        });
        tracing::info!("voice-assistant session init: {}", session.id);
        Ok(())
    }

    async fn process_session_chunk(
        &self,
        session_id: &str,
        chunk: neomind_extension_sdk::DataChunk,
    ) -> Result<StreamResult> {
        self.inner
            .total_bytes_in
            .fetch_add(chunk.data.len() as u64, Ordering::SeqCst);
        let senders = self.inner.session_senders.read().await;
        if let Some(tx) = senders.get(session_id) {
            // Backpressure: if channel full, drop oldest-style by try_send.
            let _ = tx.try_send(chunk.data.clone());
        }
        Ok(StreamResult {
            input_sequence: Some(chunk.sequence),
            output_sequence: 0,
            data: vec![],
            data_type: StreamDataType::Binary,
            processing_ms: 0.0,
            metadata: None,
            error: None,
        })
    }

    async fn close_session(&self, session_id: &str) -> Result<neomind_extension_sdk::SessionStats> {
        self.inner.session_senders.write().await.remove(session_id);
        tracing::info!("voice-assistant session closed: {}", session_id);
        Ok(neomind_extension_sdk::SessionStats::default())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// FFI export.
neomind_extension_sdk::neomind_export!(VoiceAssistantExtension);
