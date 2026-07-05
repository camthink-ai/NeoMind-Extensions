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
    metric_bool, metric_int, send_push_output, CapabilityContext, CommandBuilder, Extension,
    ExtensionCommand, ExtensionError, ExtensionMetadata, ExtensionMetricValue, FlowControl,
    MetricBuilder, MetricDescriptor, PushOutputMessage, Result, StreamCapability, StreamDataType,
    StreamDirection, StreamMode, StreamResult, StreamSession,
};
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc;

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_ORCHESTRATOR_URL: &str = "ws://127.0.0.1:9384/ws";
const PCM_MIME: &str = "audio/pcm";
/// Heartbeat interval for Push-mode stall prevention. The platform kills
/// sessions that produce no `push_output` for 30s (PUSH_STALL_TIMEOUT in
/// extension_stream.rs). For an idle voice assistant (user hasn't spoken
/// yet), no events flow, so we emit a no-op JSON heartbeat every 15s to
/// keep the session alive.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

// Persistent tokio runtime for long-lived pump tasks.
//
// Why: the SDK runs each FFI bridge call (init_session, start_push, ...) on a
// dedicated OS thread via `safe_ffi_call_with_timeout`, and `block_on_result`
// in the SDK creates an EPHEMERAL tokio runtime when no current handle exists
// (which is the case on that FFI thread). That ephemeral runtime is dropped
// the moment the FFI call returns, cancelling every `tokio::spawn` task it
// hosted. `run_session_pump` must outlive `init_session`, so we spawn it onto
// this global persistent runtime instead. Mirrors the yolo-video-v2 pattern
// (which side-steps the issue entirely with `std::thread::spawn`).
fn persistent_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build voice-assistant persistent runtime")
    })
}

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
    /// Per-ChatStream-session outbound text senders. When an AgentStreamChunk
    /// event arrives on the EventBus with `session_id == N`, we look up
    /// `chat_streams[N]` and push the chunk JSON string. The owning
    /// `run_session_pump` drains it in its select loop and forwards as a WS
    /// text frame to the Python orchestrator.
    ///
    /// Uses `parking_lot::RwLock` (not tokio) because the Extension trait's
    /// `handle_event` is sync — we need a sync read path. The pump's
    /// insertions/removals are also sync via `.write()`.
    chat_streams:
        parking_lot::RwLock<std::collections::HashMap<String, mpsc::Sender<String>>>,
    /// Per-voice-session control text senders. `execute_command("stop")`
    /// pushes JSON text frames here (e.g. `{"type":"stop"}`); the owning
    /// pump drains its control_rx in the select! loop and forwards each as
    /// a WS text frame to the Python orchestrator. This gives extension
    /// commands an out-of-band path to the Python WS without going through
    /// the browser PCM channel.
    control_senders:
        parking_lot::RwLock<std::collections::HashMap<String, mpsc::Sender<String>>>,
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
                chat_streams: parking_lot::RwLock::new(std::collections::HashMap::new()),
                control_senders: parking_lot::RwLock::new(std::collections::HashMap::new()),
            }),
        }
    }

    /// Forward one AgentStreamChunk to the registered pump's mpsc as a
    /// `chat_chunk` WS text frame. Non-terminal: chunk-internal `type=end`
    /// is NOT a reliable stream terminator (reasoning models/tool loops
    /// can emit intermediate end-like markers); wait for AgentStreamEnd.
    fn handle_stream_chunk(&self, neomind_sid: &str, inner: &Value) -> Result<()> {
        let chunk = inner.get("chunk").cloned().unwrap_or(Value::Null);
        eprintln!(
            "[VA] handle_event: AgentStreamChunk sid={} chunk_type={:?}",
            neomind_sid,
            chunk.get("type").and_then(|v| v.as_str()).unwrap_or("?")
        );
        let chunk_txt = json!({
            "type": "chat_chunk",
            "session_id": neomind_sid,
            "chunk": chunk,
        })
        .to_string();
        let tx_opt = self
            .inner
            .chat_streams
            .try_read()
            .and_then(|guard| guard.get(neomind_sid).cloned());
        if let Some(tx) = tx_opt {
            if let Err(e) = tx.try_send(chunk_txt) {
                tracing::debug!(error = %e, "chat_chunk dropped (pump slow/closed)");
            }
        } else {
            eprintln!(
                "[VA] handle_event: no chat_streams entry for sid={} (not registered yet?)",
                neomind_sid
            );
        }
        Ok(())
    }

    /// Authoritative per-turn terminator (Phase 1: AgentStreamEnd event
    /// decouples "stream ended" from chunk-internal `type=end`). Emits a
    /// `chat_stream_end` WS frame so the Python LLM backend's stream()
    /// loop returns cleanly for this turn.
    ///
    /// **Does NOT remove the chat_streams entry** in Phase 2 — the
    /// persistent session means the same sid (and the same pump→Python
    /// mpsc) is reused for the next turn. Removing here would force a
    /// `chat_session_open` capability round-trip on every turn, defeating
    /// the persistent-session optimization. The entry is removed only on
    /// WS teardown (run_session_pump cleanup, which also fires
    /// chat_session_close on the host).
    fn handle_stream_end(&self, neomind_sid: &str, inner: &Value) -> Result<()> {
        let reason = inner
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("completed");
        let error = inner.get("error").and_then(|v| v.as_str());
        eprintln!(
            "[VA] handle_event: AgentStreamEnd sid={} reason={:?} error={:?}",
            neomind_sid, reason, error
        );
        let tx_opt = self
            .inner
            .chat_streams
            .try_read()
            .and_then(|guard| guard.get(neomind_sid).cloned());
        if let Some(tx) = tx_opt {
            let end_txt = json!({
                "type": "chat_stream_end",
                "session_id": neomind_sid,
                "reason": reason,
            })
            .to_string();
            let _ = tx.try_send(end_txt);
        } else {
            eprintln!(
                "[VA] handle_stream_end: no chat_streams entry for sid={} (WS tearing down?)",
                neomind_sid
            );
        }
        Ok(())
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

    // Outbound chat-chunk channel: the global event handler pushes JSON
    // strings here (keyed by neomind_session_id in `inner.chat_streams`),
    // and we drain it in the select! loop below, forwarding each as a WS
    // text frame to Python. One channel per pump; multiple chat turns may
    // reuse it across the lifetime of the voice session.
    let (chat_tx, mut chat_rx) = mpsc::channel::<String>(64);
    // Control channel: extension commands (like "stop") push JSON text
    // frames here; we forward each as a WS text frame to Python.
    let (control_tx, mut control_rx) = mpsc::channel::<String>(16);
    inner
        .control_senders
        .write()
        .insert(session_id.clone(), control_tx);
    // Track ChatStream session_ids owned by this pump so we can unregister
    // them on shutdown and avoid leaking entries that point at a dead sender.
    let mut owned_chat_sids: Vec<String> = Vec::new();

    let mut out_seq: u64 = 0;
    // Heartbeat timer: emits a no-op push_output every HEARTBEAT_INTERVAL
    // to prevent the platform's 30s Push-mode stall detection from killing
    // idle sessions (user opened the voice assistant but hasn't spoken).
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    // Skip the immediate first tick (fires at t=0).
    heartbeat.tick().await;

    loop {
        tokio::select! {
            // Browser → Python
            Some(pcm) = browser_rx.recv() => {
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
                        match serde_json::from_str::<Value>(&txt) {
                            Ok(v) => {
                                let evt_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("event").to_string();
                                if evt_type == "chat_stream_request" {
                                    // Phase 2 (Direct Stream Pattern): lazy open +
                                    // send. First time we see no session_id (or a
                                    // session_id we don't have a chat_streams entry
                                    // for) we invoke `chat_session_open` to allocate
                                    // a persistent session, register the chat_tx
                                    // under the returned session_id, and emit
                                    // `chat_stream_started` so Python captures the
                                    // sid. Then we invoke `chat_session_send` to
                                    // drive this turn and emit
                                    // `chat_session_turn_started { turn_id }` so
                                    // Python can correlate incoming chunks by turn.
                                    //
                                    // Compared to Phase 1's `chat_stream` path this
                                    // avoids re-opening the session each turn and
                                    // gives us turn_id (transport-layer metadata
                                    // injected by the provider into each chunk).
                                    // Phase 1's authoritative AgentStreamEnd is
                                    // still the terminator; chunk routing is
                                    // unchanged (handle_stream_chunk / handle_stream_end
                                    // in lib.rs).
                                    eprintln!("[VA] chat_stream_request received, lazy open + send...");
                                    let message = v.get("message")
                                        .and_then(|m| m.as_str()).unwrap_or("").to_string();
                                    let existing_sid = v.get("session_id")
                                        .and_then(|s| s.as_str()).map(|s| s.to_string());
                                    // Voice hint flows in ONLY on the first turn
                                    // (Python sends it iff it doesn't yet have a
                                    // neomind session id). Used as system_prompt
                                    // for chat_session_open — keeps the hint in
                                    // the LLM's system slot instead of being
                                    // prepended to every user message via the
                                    // old pageContext path. Forwarded verbatim;
                                    // empty string == no override (the platform
                                    // treats absent field and empty string the
                                    // same).
                                    let voice_hint = v.get("voice_hint")
                                        .and_then(|s| s.as_str())
                                        .filter(|s| !s.is_empty())
                                        .map(|s| s.to_string());

                                    // Do we need to open a new session? Yes iff we
                                    // don't already have a chat_streams entry for
                                    // this sid (covers None-sid first turn AND
                                    // server-restart sid-stale edge cases).
                                    let need_open = match &existing_sid {
                                        None => true,
                                        Some(sid) => !inner
                                            .chat_streams
                                            .try_read()
                                            .map(|g| g.contains_key(sid))
                                            .unwrap_or(false),
                                    };

                                    let neomind_sid = if need_open {
                                        // Build open params. system_prompt is
                                        // honored ONLY at session creation —
                                        // existing sessions ignore it (safety
                                        // property tested in session.rs).
                                        let open_params = match &existing_sid {
                                            Some(sid) => json!({ "session_id": sid }),
                                            None => {
                                                let mut p = json!({});
                                                if let Some(hint) = &voice_hint {
                                                    p["system_prompt"] = json!(hint);
                                                }
                                                p
                                            }
                                        };
                                        let cap_ctx = CapabilityContext::default();
                                        let result = match tokio::task::spawn_blocking(
                                            move || cap_ctx.invoke_capability("chat_session_open", &open_params),
                                        )
                                        .await
                                        {
                                            Ok(r) => r,
                                            Err(e) => {
                                                let err = json!({
                                                    "type": "chat_stream_error",
                                                    "error": format!("open join: {e}"),
                                                });
                                                if ws.send(tokio_tungstenite::tungstenite::Message::Text(err.to_string())).await.is_err() {
                                                    break;
                                                }
                                                continue;
                                            }
                                        };
                                        let success = result.get("success")
                                            .and_then(|s| s.as_bool()).unwrap_or(true);
                                        if !success {
                                            let err_msg = result.get("error")
                                                .and_then(|e| e.as_str()).unwrap_or("unknown");
                                            let err = json!({
                                                "type": "chat_stream_error",
                                                "error": format!("open: {err_msg}"),
                                            });
                                            if ws.send(tokio_tungstenite::tungstenite::Message::Text(err.to_string())).await.is_err() {
                                                break;
                                            }
                                            continue;
                                        }
                                        let Some(sid) = result.get("session_id")
                                            .and_then(|s| s.as_str()).map(|s| s.to_string()) else {
                                            let err = json!({
                                                "type": "chat_stream_error",
                                                "error": "missing session_id in chat_session_open response",
                                            });
                                            if ws.send(tokio_tungstenite::tungstenite::Message::Text(err.to_string())).await.is_err() {
                                                break;
                                            }
                                            continue;
                                        };
                                        // Register chat_tx for this sid (chunk
                                        // routing) and track for teardown cleanup.
                                        inner.chat_streams.write().insert(sid.clone(), chat_tx.clone());
                                        owned_chat_sids.push(sid.clone());
                                        let started = json!({
                                            "type": "chat_stream_started",
                                            "session_id": sid,
                                            "voice_session_id": session_id,
                                        });
                                        if ws.send(tokio_tungstenite::tungstenite::Message::Text(started.to_string())).await.is_err() {
                                            break;
                                        }
                                        sid
                                    } else {
                                        existing_sid.unwrap()
                                    };

                                    // Send phase — always invoked per turn.
                                    let send_params = json!({
                                        "session_id": neomind_sid,
                                        "message": message,
                                    });
                                    let cap_ctx2 = CapabilityContext::default();
                                    let sid_for_send = neomind_sid.clone();
                                    let send_result = match tokio::task::spawn_blocking(
                                        move || cap_ctx2.invoke_capability("chat_session_send", &send_params),
                                    )
                                    .await
                                    {
                                        Ok(r) => r,
                                        Err(e) => {
                                            let err = json!({
                                                "type": "chat_stream_error",
                                                "error": format!("send join: {e}"),
                                            });
                                            if ws.send(tokio_tungstenite::tungstenite::Message::Text(err.to_string())).await.is_err() {
                                                break;
                                            }
                                            continue;
                                        }
                                    };
                                    let send_ok = send_result.get("success")
                                        .and_then(|s| s.as_bool()).unwrap_or(true);
                                    if !send_ok {
                                        let err_msg = send_result.get("error")
                                            .and_then(|e| e.as_str()).unwrap_or("unknown");
                                        let err = json!({
                                            "type": "chat_stream_error",
                                            "error": format!("send: {err_msg}"),
                                        });
                                        if ws.send(tokio_tungstenite::tungstenite::Message::Text(err.to_string())).await.is_err() {
                                            break;
                                        }
                                        continue;
                                    }
                                    let turn_id = send_result.get("turn_id")
                                        .and_then(|t| t.as_str()).map(|s| s.to_string())
                                        .unwrap_or_default();
                                    eprintln!(
                                        "[VA] chat_session_send returned: sid={} turn_id={}",
                                        neomind_sid, turn_id
                                    );
                                    let turn_started = json!({
                                        "type": "chat_session_turn_started",
                                        "session_id": neomind_sid,
                                        "turn_id": turn_id,
                                    });
                                    if ws.send(tokio_tungstenite::tungstenite::Message::Text(turn_started.to_string())).await.is_err() {
                                        break;
                                    }
                                    let _ = sid_for_send; // already consumed via neomind_sid
                                } else if evt_type == "chat_stream_cancel" {
                                    // Python requests cancellation of an in-flight turn.
                                    // Phase 2: invoke ChatStreamCancelTurn (turn-level
                                    // cancel; keeps the session open for the next turn).
                                    // The host's ChatSessionCapabilityProvider delegates
                                    // to SessionManager.cancel_session — today the only
                                    // cancel granularity is per-session, but the API is
                                    // forward-compatible with per-turn mutex tracking.
                                    if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
                                        let sid_owned = sid.to_string();
                                        let inner_clone = inner.clone();
                                        tokio::task::spawn_blocking(move || {
                                            let cap_ctx = CapabilityContext::default();
                                            let params = json!({ "session_id": sid_owned });
                                            let r = cap_ctx.invoke_capability("chat_stream_cancel_turn", &params);
                                            let success = r.get("success")
                                                .and_then(|b| b.as_bool()).unwrap_or(true);
                                            let cancelled = r.get("cancelled")
                                                .and_then(|b| b.as_bool()).unwrap_or(false);
                                            if !success {
                                                tracing::warn!(
                                                    session_id = %sid_owned,
                                                    error = %r.get("error").and_then(|e| e.as_str()).unwrap_or("?"),
                                                    "chat_stream_cancel_turn failed"
                                                );
                                            } else {
                                                tracing::info!(
                                                    session_id = %sid_owned,
                                                    cancelled,
                                                    "chat_stream_cancel_turn invoked"
                                                );
                                            }
                                        });
                                        // Note: we intentionally do NOT remove the
                                        // chat_streams entry here. Turn-level cancel
                                        // should leave the session open for the next
                                        // turn. Phase 1's authoritative AgentStreamEnd
                                        // (delivered when the cancelled turn's spawn
                                        // task winds down) handles cleanup.
                                        let _ = inner_clone;
                                    }
                                } else {
                                    // Existing forwarding path for transcript / stop / etc.
                                    out_seq += 1;
                                    if evt_type != "pong" {
                                        let m = match PushOutputMessage::json(
                                            &session_id, out_seq, v.clone(),
                                        ) {
                                            Ok(m) => m,
                                            Err(_) => continue,
                                        };
                                        let _ = send_push_output(&m);
                                    }
                                    // NOTE: do NOT set closed=true on "stop"/"end".
                                    // These are per-turn signals (Python sends
                                    // {"type":"stop"} at end of every turn;
                                    // chat capability emits terminal "end").
                                    // Treating them as session-terminal drops
                                    // all subsequent browser PCM and breaks
                                    // multi-turn conversations. Real session
                                    // teardown happens via the Close frame at
                                    // the loop break below, or close_session()
                                    // removing the session_senders entry.
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => break,
                    Some(Ok(_)) => {} // Ping/Pong/Frame — ignore
                    Some(Err(_)) => break,
                    None => break,
                }
            }
            // Chat chunks queued by the global event handler → forward to Python.
            Some(chat_txt) = chat_rx.recv() => {
                if ws.send(tokio_tungstenite::tungstenite::Message::Text(chat_txt)).await.is_err() {
                    break;
                }
            }
            // Control messages from extension commands (stop, etc.) → forward to Python.
            Some(ctrl_txt) = control_rx.recv() => {
                if ws.send(tokio_tungstenite::tungstenite::Message::Text(ctrl_txt)).await.is_err() {
                    break;
                }
            }
            // Heartbeat: emit a no-op push_output to reset the platform's
            // 30s Push stall timer. Without this, idle voice sessions
            // (user hasn't spoken) get killed after 30s.
            _ = heartbeat.tick() => {
                out_seq += 1;
                if let Ok(m) = PushOutputMessage::json(
                    &session_id,
                    out_seq,
                    json!({"type": "heartbeat"}),
                ) {
                    let _ = send_push_output(&m);
                }
            }
        }
    }

    // Cleanup: best-effort cancel any in-flight ChatStream LLM generation
    // for sessions owned by this pump, then unregister our senders. Without
    // the cancel call the host's spawned task would keep driving the LLM
    // to completion, wasting VRAM/compute while we silently drop every
    // subsequent AgentStreamChunk event.
    if !owned_chat_sids.is_empty() {
        let sids_for_close = owned_chat_sids.clone();
        // Phase 2: ChatSessionClose tears down the persistent session
        // (cancel_session + remove_subscriber). One call per sid covers
        // both the in-flight turn and the session lifetime.
        tokio::task::spawn_blocking(move || {
            let cap_ctx = CapabilityContext::default();
            for sid in &sids_for_close {
                let params = json!({ "session_id": sid });
                let r = cap_ctx.invoke_capability("chat_session_close", &params);
                let success = r.get("success")
                    .and_then(|b| b.as_bool()).unwrap_or(true);
                if !success {
                    tracing::debug!(
                        session_id = %sid,
                        error = %r.get("error").and_then(|e| e.as_str()).unwrap_or("?"),
                        "chat_session_close during pump cleanup failed (stream may already be done)"
                    );
                }
            }
        });
        let mut cs = inner.chat_streams.write();
        for sid in &owned_chat_sids {
            cs.remove(sid);
        }
    }
    inner.active_sessions.fetch_sub(1, Ordering::SeqCst);
    inner.session_senders.write().await.remove(&session_id);
    inner.control_senders.write().remove(&session_id);
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
            CommandBuilder::new("stop")
                .display_name("Stop / Barge-In")
                .description(
                    "Immediately stop the current voice turn: cancel any in-flight \
                     ChatStream LLM response and signal the Python orchestrator to \
                     barge-in (stop TTS, return to listening).",
                )
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
            // Stop / barge-in: immediately cancel any in-flight ChatStream
            // (clear chat_streams so further AgentStreamChunk events are
            // dropped) and send `{"type":"stop"}` to every active Python
            // orchestrator session via the control channel. Python's
            // ws_handler triggers BargeInHandler + cancels the pipeline task.
            "stop" => {
                // Snapshot owned chat_stream sids before clearing so we can
                // fire ChatStreamCancel on each — otherwise the host keeps
                // generating LLM tokens after barge-in.
                let (cleared, cancelled_sids) = {
                    let mut cs = self.inner.chat_streams.write();
                    let sids: Vec<String> = cs.keys().cloned().collect();
                    let n = sids.len();
                    cs.clear();
                    (n, sids)
                };
                // Best-effort cancel on host side. Fire-and-forget — barge-in
                // latency matters more than waiting for the FFI round-trip.
                // Phase 2: turn-level cancel (ChatStreamCancelTurn) — keeps
                // the session alive for the next turn.
                tokio::task::spawn_blocking(move || {
                    let cap_ctx = CapabilityContext::default();
                    for sid in &cancelled_sids {
                        let params = json!({ "session_id": sid });
                        let r = cap_ctx.invoke_capability("chat_stream_cancel_turn", &params);
                        let success = r.get("success")
                            .and_then(|b| b.as_bool()).unwrap_or(true);
                        if !success {
                            tracing::debug!(
                                session_id = %sid,
                                error = %r.get("error").and_then(|e| e.as_str()).unwrap_or("?"),
                                "stop: chat_stream_cancel_turn failed (may already be done)"
                            );
                        }
                    }
                });
                let mut notified = 0u32;
                let senders = self.inner.control_senders.read().clone();
                let stop_msg = json!({"type": "stop"}).to_string();
                for (_sid, tx) in &senders {
                    if tx.try_send(stop_msg.clone()).is_ok() {
                        notified += 1;
                    }
                }
                tracing::info!(
                    cleared_chat_streams = cleared,
                    notified_sessions = notified,
                    "voice-assistant stop command executed"
                );
                Ok(json!({
                    "ok": true,
                    "cleared_chat_streams": cleared,
                    "notified_sessions": notified,
                }))
            }
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

    // --- Event subscription (ChatStream capability bridge) ---
    //
    // The host's EventDispatcher only forwards event types that appear in
    // this list. Without "AgentStreamChunk" here, the host filters them out
    // before they ever reach `handle_event`, and the chat_stream bridge is
    // silently dead. (Verified against event_dispatcher.rs:148-176.)
    fn event_subscriptions(&self) -> &'static [&'static str] {
        &["AgentStreamChunk", "AgentStreamEnd"]
    }

    /// Synchronous entry point invoked by the runner (or by the in-process
    /// EventDispatcher) for each subscribed event. Routes AgentStreamChunk
    /// payloads by `session_id` to the per-pump chat_chunks mpsc, which the
    /// `run_session_pump` select loop drains and forwards as WS text frames
    /// to the Python orchestrator.
    ///
    /// Sync because the Extension trait requires it; mpsc::Sender::try_send
    /// is sync-safe. Bounded channel means a stuck pump can't wedge the host.
    fn handle_event(&self, event_type: &str, payload: &Value) -> Result<()> {
        // EventDispatcher wraps every NeoMindEvent as
        // `{event_type, payload: {<event fields>}, timestamp}` (see
        // extension_event_subscription.rs::convert_to_extension_format).
        // Fields like `session_id` / `chunk` live one level down. Same
        // unwrap pattern as face-recognition/src/lib.rs:499.
        let inner = payload.get("payload").unwrap_or(payload);
        let Some(neomind_sid) = inner.get("session_id").and_then(|v| v.as_str()) else {
            eprintln!(
                "[VA] handle_event: {} without session_id, dropping",
                event_type
            );
            return Ok(());
        };

        match event_type {
            "AgentStreamChunk" => self.handle_stream_chunk(neomind_sid, inner),
            "AgentStreamEnd" => self.handle_stream_end(neomind_sid, inner),
            _ => Ok(()),
        }
    }

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
        // Spawn the pump on the global persistent runtime, NOT the FFI call's
        // ephemeral runtime. The SDK drops its ephemeral runtime when
        // init_session returns, which would cancel any task spawned there.
        let session_id_for_log = session_id.clone();
        persistent_runtime().spawn(async move {
            if let Err(e) = run_session_pump(session_id.clone(), ws_url, rx, inner).await {
                tracing::error!("voice-assistant session {} pump ended: {}", session_id_for_log, e);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `stop` clears all registered chat_streams entries so subsequent
    /// AgentStreamChunk events find no listener and are dropped.
    #[tokio::test]
    async fn test_stop_clears_chat_streams() {
        let ext = VoiceAssistantExtension::new();
        // Insert two fake chat stream senders.
        let (tx1, _rx1) = mpsc::channel::<String>(8);
        let (tx2, _rx2) = mpsc::channel::<String>(8);
        ext.inner
            .chat_streams
            .write()
            .insert("sid-aaa".to_string(), tx1);
        ext.inner
            .chat_streams
            .write()
            .insert("sid-bbb".to_string(), tx2);
        assert_eq!(ext.inner.chat_streams.read().len(), 2);

        let result = ext.execute_command("stop", &json!({})).await.unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["cleared_chat_streams"], json!(2));
        assert!(ext.inner.chat_streams.read().is_empty());
    }

    /// `stop` pushes `{"type":"stop"}` into every registered control sender
    /// so the pump forwards it to the Python orchestrator as a WS text frame.
    #[tokio::test]
    async fn test_stop_notifies_control_senders() {
        let ext = VoiceAssistantExtension::new();
        let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<String>(16);
        ext.inner
            .control_senders
            .write()
            .insert("voice-session-1".to_string(), ctrl_tx);

        let result = ext.execute_command("stop", &json!({})).await.unwrap();
        assert_eq!(result["notified_sessions"], json!(1));

        // The control receiver should get exactly {"type":"stop"}.
        let msg = ctrl_rx.recv().await.expect("control channel empty");
        let parsed: Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(parsed["type"], json!("stop"));
    }

    /// `stop` with no active sessions/chat streams returns zeros, not an error.
    #[tokio::test]
    async fn test_stop_noop_when_idle() {
        let ext = VoiceAssistantExtension::new();
        let result = ext.execute_command("stop", &json!({})).await.unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["cleared_chat_streams"], json!(0));
        assert_eq!(result["notified_sessions"], json!(0));
    }

    /// Full integration: chat_streams has entries AND control senders are
    //  registered. `stop` must atomically clear both.
    #[tokio::test]
    async fn test_stop_full_clears_chat_and_notifies_control() {
        let ext = VoiceAssistantExtension::new();

        // Set up a fake chat stream.
        let (chat_tx, _chat_rx) = mpsc::channel::<String>(8);
        ext.inner
            .chat_streams
            .write()
            .insert("neomind-sid-xyz".to_string(), chat_tx);

        // Set up two control senders (two active voice sessions).
        let (ctrl_tx_a, mut ctrl_rx_a) = mpsc::channel::<String>(16);
        let (ctrl_tx_b, mut ctrl_rx_b) = mpsc::channel::<String>(16);
        ext.inner
            .control_senders
            .write()
            .insert("session-a".to_string(), ctrl_tx_a);
        ext.inner
            .control_senders
            .write()
            .insert("session-b".to_string(), ctrl_tx_b);

        let result = ext.execute_command("stop", &json!({})).await.unwrap();
        assert_eq!(result["cleared_chat_streams"], json!(1));
        assert_eq!(result["notified_sessions"], json!(2));

        // Both control channels receive the stop frame.
        let msg_a = ctrl_rx_a.recv().await.unwrap();
        let msg_b = ctrl_rx_b.recv().await.unwrap();
        assert!(msg_a.contains(r#""type":"stop""#));
        assert!(msg_b.contains(r#""type":"stop""#));

        // chat_streams is empty.
        assert!(ext.inner.chat_streams.read().is_empty());
    }

    /// After `stop`, a subsequent AgentStreamChunk event targeting the cleared
    /// session_id is silently dropped (handle_event logs "no chat_streams entry").
    #[tokio::test]
    async fn test_handle_event_drops_after_stop() {
        let ext = VoiceAssistantExtension::new();
        // Register then stop.
        let (chat_tx, _chat_rx) = mpsc::channel::<String>(8);
        ext.inner
            .chat_streams
            .write()
            .insert("post-stop-sid".to_string(), chat_tx);
        let _ = ext.execute_command("stop", &json!({})).await.unwrap();

        // Now handle_event should succeed (return Ok) but NOT forward — the
        // entry was cleared.
        let payload = json!({
            "event_type": "AgentStreamChunk",
            "payload": {
                "session_id": "post-stop-sid",
                "chunk": {"type": "Content", "content": "hello"},
            },
            "timestamp": 0,
        });
        let result = ext.handle_event("AgentStreamChunk", &payload);
        assert!(result.is_ok()); // Ok, but chunk was silently dropped.
    }

    /// `stop` command appears in the registered commands list.
    #[test]
    fn test_stop_command_registered() {
        let ext = VoiceAssistantExtension::new();
        let cmds: Vec<String> = ext.commands().iter().map(|c| c.name.clone()).collect();
        assert!(cmds.contains(&"stop".to_string()));
        assert!(cmds.contains(&"health".to_string()));
        assert!(cmds.contains(&"status".to_string()));
    }

    // ========================================================================
    // Integration tests: spawn a real mock WS server + real run_session_pump,
    // verify end-to-end that control-channel messages and browser PCM actually
    // reach the WebSocket as text/binary frames.
    // ========================================================================

    use tokio_tungstenite::tungstenite::Message;

    /// Spins up a mock WS server, returns `(ws_url, ready_rx, msg_rx, server_task)`.
    /// The server signals `ready_rx` once it has accepted the connection and
    /// drained the pump's initial "start" message. All subsequent WS messages
    /// are forwarded to `msg_rx`.
    async fn spawn_mock_orchestrator() -> (
        String,
        tokio::sync::oneshot::Receiver<()>,
        mpsc::Receiver<Message>,
        tokio::task::JoinHandle<()>,
    ) {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        let ws_url = format!("ws://127.0.0.1:{port}/ws");

        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let (msg_tx, msg_rx) = mpsc::channel::<Message>(32);

        let handle = tokio::spawn(async move {
            let (stream, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut ws = match tokio_tungstenite::accept_async(stream).await {
                Ok(w) => w,
                Err(_) => return,
            };
            // Drain the pump's initial "start" message, then signal ready.
            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(t) = &msg {
                    if let Ok(v) = serde_json::from_str::<Value>(t) {
                        if v.get("type").and_then(|t| t.as_str()) == Some("start") {
                            let _ = ready_tx.send(());
                            break;
                        }
                    }
                }
            }
            // Forward subsequent messages.
            while let Some(Ok(msg)) = ws.next().await {
                if msg_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        (ws_url, ready_rx, msg_rx, handle)
    }

    /// Wait for `control_senders` to contain `session_id` (the pump registers
    /// it right after the WS connect succeeds). Bounded poll to keep tests fast
    /// on failure.
    async fn wait_for_control_registered(ext: &VoiceAssistantExtension, sid: &str) {
        for _ in 0..200 {
            if ext.inner.control_senders.read().contains_key(sid) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("control_tx not registered for session {sid} within 2s");
    }

    /// Real WS server + real pump. `execute_command("stop")` → control channel
    /// → WS text frame `{"type":"stop"}`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn integration_stop_reaches_ws() {
        let (ws_url, ready_rx, mut msg_rx, server_handle) =
            spawn_mock_orchestrator().await;

        let ext = VoiceAssistantExtension::new();
        let (_browser_tx, browser_rx) = mpsc::channel::<Vec<u8>>(64);
        let session_id = "voice-sid-stop-itest".to_string();

        let inner = ext.inner.clone();
        let pump_handle = tokio::spawn(async move {
            let _ = run_session_pump(session_id.clone(), ws_url, browser_rx, inner).await;
        });

        // Wait for WS connect + control_tx registration.
        ready_rx.await.expect("server never saw start");
        wait_for_control_registered(&ext, "voice-sid-stop-itest").await;

        // Trigger stop via the extension command path (what the platform calls).
        let result = ext.execute_command("stop", &json!({})).await.unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["notified_sessions"], json!(1));

        // Verify the WS server actually received the stop frame.
        let frame = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            msg_rx.recv(),
        )
        .await
        .expect("timeout waiting for stop frame on WS")
        .expect("server channel closed");

        match frame {
            Message::Text(t) => {
                let v: Value = serde_json::from_str(&t).expect("non-json text frame");
                assert_eq!(v["type"], json!("stop"), "unexpected payload: {v}");
            }
            other => panic!("expected Text({{\"type\":\"stop\"}}), got {other:?}"),
        }

        pump_handle.abort();
        server_handle.abort();
    }

    /// Real WS server + real pump. Browser PCM (sent via session_senders'
    /// browser_tx mpsc) arrives at the WS as a Binary frame.
    ///
    /// Note: the pump owns `browser_rx`; we don't go through
    /// `process_session_chunk` here (that requires an SDK session registration).
    /// Instead we hand the pump a `browser_rx` directly and push PCM into the
    /// matching `browser_tx` — same bytes-on-the-wire outcome.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn integration_browser_pcm_forwarded_to_ws() {
        let (ws_url, ready_rx, mut msg_rx, server_handle) =
            spawn_mock_orchestrator().await;

        // Construct an extension just so we have an `Inner` to pass the pump —
        // we don't go through the SDK Extension trait here.
        let ext = VoiceAssistantExtension::new();
        let (browser_tx, browser_rx) = mpsc::channel::<Vec<u8>>(64);
        let session_id = "voice-sid-pcm-itest".to_string();

        let inner = ext.inner.clone();
        let pump_handle = tokio::spawn(async move {
            let _ = run_session_pump(session_id.clone(), ws_url, browser_rx, inner).await;
        });

        ready_rx.await.expect("server never saw start");
        wait_for_control_registered(&ext, "voice-sid-pcm-itest").await;

        // Push 100 bytes of fake PCM.
        let pcm: Vec<u8> = (0u8..100).collect();
        browser_tx.send(pcm.clone()).await.unwrap();

        // Verify WS received a Binary frame with exactly those bytes.
        let frame = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            msg_rx.recv(),
        )
        .await
        .expect("timeout waiting for PCM binary frame on WS")
        .expect("server channel closed");

        match frame {
            Message::Binary(data) => {
                let got: &[u8] = data.as_ref();
                assert_eq!(got, pcm.as_slice(), "PCM bytes mismatch");
            }
            other => panic!("expected Binary({pcm:?}), got {other:?}"),
        }

        pump_handle.abort();
        server_handle.abort();
    }

    /// Regression: Python sends `{"type":"stop"}` at end of every voice turn
    /// (server.py "stop marks turn complete"). The pump MUST keep forwarding
    /// browser PCM after this frame so the user can speak again in the same
    /// session. Previously the pump set `closed = true` on stop/end events,
    /// silently dropping all subsequent browser PCM and breaking multi-turn
    /// conversations.
    ///
    /// We spin up a mock server that:
    ///   1. Accepts the connection, drains the pump's "start" message.
    ///   2. Sends a {"type":"stop"} text frame (simulating end-of-turn).
    ///   3. Then receives any subsequent frames into msg_rx.
    /// The test pushes browser PCM AFTER the stop frame and asserts it still
    /// arrives at the WS as a Binary frame.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn integration_stop_does_not_close_session() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        let ws_url = format!("ws://127.0.0.1:{port}/ws");

        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let (msg_tx, mut msg_rx) = mpsc::channel::<Message>(32);

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut ws = tokio_tungstenite::accept_async(stream).await.expect("ws handshake");
            // Drain the pump's initial "start" message, then signal ready.
            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(t) = &msg {
                    if let Ok(v) = serde_json::from_str::<Value>(t) {
                        if v.get("type").and_then(|t| t.as_str()) == Some("start") {
                            let _ = ready_tx.send(());
                            break;
                        }
                    }
                }
            }
            // Simulate Python's end-of-turn: send {"type":"stop"}.
            ws.send(Message::Text(r#"{"type":"stop"}"#.to_string()))
                .await
                .expect("send stop");
            // Forward subsequent frames.
            while let Some(Ok(msg)) = ws.next().await {
                if msg_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let ext = VoiceAssistantExtension::new();
        let (browser_tx, browser_rx) = mpsc::channel::<Vec<u8>>(64);
        let session_id = "voice-sid-multiturn-itest".to_string();

        let inner = ext.inner.clone();
        let pump_handle = tokio::spawn(async move {
            let _ = run_session_pump(session_id.clone(), ws_url, browser_rx, inner).await;
        });

        ready_rx.await.expect("server never saw start");
        wait_for_control_registered(&ext, "voice-sid-multiturn-itest").await;

        // Give the pump a moment to ingest the stop frame. The server has
        // already sent it; the pump's ws.next() branch will process it.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Push PCM AFTER the stop frame arrived. Pre-fix: dropped silently
        // (closed=true). Post-fix: forwarded as Binary to the WS server.
        let pcm: Vec<u8> = (200u8..250).collect();
        browser_tx.send(pcm.clone()).await.unwrap();

        let frame = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            msg_rx.recv(),
        )
        .await
        .expect("timeout waiting for PCM after stop — pump likely set closed=true")
        .expect("server channel closed");

        match frame {
            Message::Binary(data) => {
                let got: &[u8] = data.as_ref();
                assert_eq!(got, pcm.as_slice(), "post-stop PCM bytes mismatch");
            }
            other => panic!(
                "expected Binary({:?}) (PCM after stop), got {:?}",
                pcm, other
            ),
        }

        pump_handle.abort();
        server_handle.abort();
    }

    /// Real WS server + real pump. Verifies the heartbeat path keeps producing
    /// `push_output` frames — observable via the SDK's send_push_output hook.
    /// We don't intercept send_push_output here (it requires a platform host),
    /// but we DO verify the pump survives >1 heartbeat interval without
    /// terminating. To keep CI fast, HEARTBEAT_INTERVAL is 15s — too long for
    /// a unit test. We instead verify the pump task is still pending after a
    /// short idle period, which is the precondition for heartbeats to fire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn integration_pump_survives_idle() {
        let (ws_url, ready_rx, _msg_rx, server_handle) =
            spawn_mock_orchestrator().await;

        let ext = VoiceAssistantExtension::new();
        let (_browser_tx, browser_rx) = mpsc::channel::<Vec<u8>>(64);
        let session_id = "voice-sid-idle-itest".to_string();

        let inner = ext.inner.clone();
        let pump_handle = tokio::spawn(async move {
            let _ = run_session_pump(session_id.clone(), ws_url, browser_rx, inner).await;
        });

        ready_rx.await.expect("server never saw start");
        wait_for_control_registered(&ext, "voice-sid-idle-itest").await;

        // Idle for 500ms — if the pump had any startup-time bug (e.g. immediate
        // return on empty browser_rx, deadlock on first select arm), this would
        // surface as the task having finished.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // The pump task should still be running (not finished).
        // `is_finished()` returns false for a still-pending task.
        assert!(
            !pump_handle.is_finished(),
            "pump task terminated during idle period — heartbeat path can never fire"
        );
        assert_eq!(ext.inner.active_sessions.load(Ordering::SeqCst), 1);

        pump_handle.abort();
        server_handle.abort();
    }
}
