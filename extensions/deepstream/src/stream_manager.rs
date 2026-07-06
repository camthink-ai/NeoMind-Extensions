//! StreamManager + StreamStatus state machine (Tasks 4.1 + 4.2).
//!
//! Owns the in-memory registry of active streams. All mutation goes through
//! a `parking_lot::RwLock` so that sync callers (`produce_metrics()`,
//! `handle_event()`) can take it without a Tokio runtime.
//!
//! The state machine is encoded in [`is_legal_transition`] and enforced
//! atomically by [`StreamManager::transition`], which holds the write lock
//! for the whole check+update so a concurrent reader cannot observe a
//! half-applied transition.

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::protocol::{ControlMessage, SidecarEvent};
use crate::sidecar::SidecarHandle;

/// Per-stream timeout for config replay. Generous because DeepStream engine
/// compilation can take 10–20s on first run (spec §4.7).
pub const REPLAY_TIMEOUT_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Replay summary types (Task 4.3)
// ---------------------------------------------------------------------------

/// Outcome of a replay-to-sidecar operation. Each stream ends up in exactly
/// one of `succeeded` or `failed`.
#[derive(Debug, Clone)]
pub struct ReplaySummary {
    /// stream_ids that received a matching `StreamAdded` from the sidecar.
    pub succeeded: Vec<String>,
    /// stream_ids whose AddStream timed out, was rejected, or whose channel closed.
    pub failed: Vec<ReplayFailure>,
}

/// One stream's replay failure with a human-readable cause.
#[derive(Debug, Clone)]
pub struct ReplayFailure {
    pub stream_id: String,
    /// Human-readable error: "send failed: ...", "sidecar rejected: ...",
    /// "timeout after Ns", or "sidecar stdout closed".
    pub error: String,
}

// ---------------------------------------------------------------------------
// Config types — mirror spec §3.1.1 `add_stream` payload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    pub stream_id: String,
    pub source: StreamSource,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config: Option<ModelConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker: Option<TrackerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics: Option<AnalyticsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<EventsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtsp_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conf: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iou: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infer_device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_classes: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerConfig {
    pub enabled: bool,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub tracker_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_crossing: Option<Vec<LineCrossingRule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roi: Option<Vec<RoiRule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counting: Option<CountingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineCrossingRule {
    pub id: String,
    pub points: Vec<(i32, i32)>,
    pub mode: String,
    pub classes: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoiRule {
    pub id: String,
    pub polygon: Vec<(i32, i32)>,
    pub mode: String,
    pub classes: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountingConfig {
    pub enabled: bool,
    pub line_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtsp_mount: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub osd: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_hz: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_emit: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Status + state
// ---------------------------------------------------------------------------

/// Lifecycle state of one stream. Copy so callers can pass it by value
/// without bumping refcounts; Serialize/Deserialize so it round-trips
/// through the JSON protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamStatus {
    Connecting,
    Running,
    Degraded,
    Reconnecting,
    Error,
    Stopped,
}

impl StreamStatus {
    /// Stable lowercase wire string — handy for metric labels & logging.
    pub fn as_str(self) -> &'static str {
        match self {
            StreamStatus::Connecting => "connecting",
            StreamStatus::Running => "running",
            StreamStatus::Degraded => "degraded",
            StreamStatus::Reconnecting => "reconnecting",
            StreamStatus::Error => "error",
            StreamStatus::Stopped => "stopped",
        }
    }
}

impl std::fmt::Display for StreamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct StreamState {
    pub config: StreamConfig,
    pub status: StreamStatus,
    pub rtsp_url: Option<String>,
    pub snapshot_token: Option<String>,
    /// Epoch millis when the stream was added (`chrono::Utc::now().timestamp_millis()`).
    pub added_at: i64,
    /// Epoch millis of the last successful status transition.
    pub last_transition_at: i64,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StreamManagerError {
    #[error("stream already exists: {0}")]
    AlreadyExists(String),
    #[error("stream not found: {0}")]
    NotFound(String),
    #[error("max_streams ({0}) reached")]
    MaxStreamsReached(u32),
    #[error("illegal state transition: {from:?} -> {to:?}")]
    IllegalTransition {
        from: StreamStatus,
        to: StreamStatus,
    },
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

pub struct StreamManager {
    streams: RwLock<HashMap<String, StreamState>>,
    max_streams: u32,
}

impl StreamManager {
    pub fn new(max_streams: u32) -> Self {
        Self {
            streams: RwLock::new(HashMap::new()),
            // 0 is a degenerate "block everything" config; we still accept it
            // so unit tests can exercise the MaxStreamsReached path trivially.
            max_streams,
        }
    }

    /// Add a new stream. Returns Err on duplicate id or when `max_streams`
    /// would be exceeded. Initial status is [`StreamStatus::Connecting`].
    pub fn add(&self, config: StreamConfig) -> Result<(), StreamManagerError> {
        let mut streams = self.streams.write();
        if streams.contains_key(&config.stream_id) {
            return Err(StreamManagerError::AlreadyExists(config.stream_id.clone()));
        }
        if (streams.len() as u32) >= self.max_streams {
            return Err(StreamManagerError::MaxStreamsReached(self.max_streams));
        }
        let now = chrono::Utc::now().timestamp_millis();
        streams.insert(
            config.stream_id.clone(),
            StreamState {
                config,
                status: StreamStatus::Connecting,
                rtsp_url: None,
                snapshot_token: None,
                added_at: now,
                last_transition_at: now,
            },
        );
        Ok(())
    }

    /// Remove a stream. Returns the removed state (useful for the
    /// `remove_stream` command's response payload). Err if not present.
    pub fn remove(&self, stream_id: &str) -> Result<StreamState, StreamManagerError> {
        self.streams
            .write()
            .remove(stream_id)
            .ok_or_else(|| StreamManagerError::NotFound(stream_id.to_string()))
    }

    /// Snapshot of one stream.
    pub fn get(&self, stream_id: &str) -> Option<StreamState> {
        self.streams.read().get(stream_id).cloned()
    }

    /// Snapshot of every stream. Order is unspecified (HashMap interior).
    pub fn list(&self) -> Vec<StreamState> {
        self.streams.read().values().cloned().collect()
    }

    /// Atomically transition a stream's status. The write lock is held
    /// across the legality check + the update, so concurrent transitions
    /// are serialized: the second caller observes the first one's result.
    pub fn transition(
        &self,
        stream_id: &str,
        new: StreamStatus,
    ) -> Result<(), StreamManagerError> {
        let mut streams = self.streams.write();
        let state = streams
            .get_mut(stream_id)
            .ok_or_else(|| StreamManagerError::NotFound(stream_id.to_string()))?;
        let from = state.status;
        if !is_legal_transition(from, new) {
            warn!(stream_id, from = ?from, to = ?new, "stream state transition rejected");
            return Err(StreamManagerError::IllegalTransition { from, to: new });
        }
        info!(stream_id, from = ?from, to = ?new, "stream state transition");
        state.status = new;
        state.last_transition_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    /// Record the sidecar-assigned RTSP URL (and optional snapshot token)
    /// once the StreamAdded event arrives.
    pub fn set_rtsp_url(
        &self,
        stream_id: &str,
        url: String,
    ) -> Result<(), StreamManagerError> {
        let mut streams = self.streams.write();
        let state = streams
            .get_mut(stream_id)
            .ok_or_else(|| StreamManagerError::NotFound(stream_id.to_string()))?;
        state.rtsp_url = Some(url);
        Ok(())
    }

    /// Number of currently-tracked streams (for produce_metrics / dashboard).
    pub fn len(&self) -> usize {
        self.streams.read().len()
    }

    /// Current max_streams limit.
    pub fn max_streams(&self) -> u32 {
        self.max_streams
    }

    /// True when there are zero streams.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.streams.read().is_empty()
    }

    // -------------------------------------------------------------------
    // Config replay (Task 4.3 — spec §4.7)
    // -------------------------------------------------------------------

    /// Replay all stored stream configs to a fresh sidecar using the default
    /// per-stream timeout ([`REPLAY_TIMEOUT_SECS`]).
    ///
    /// Delegates to [`Self::replay_to_with_timeout`].
    pub async fn replay_to(&self, handle: &SidecarHandle) -> ReplaySummary {
        self.replay_to_with_timeout(handle, Duration::from_secs(REPLAY_TIMEOUT_SECS))
            .await
    }

    /// Replay all stored stream configs to a fresh sidecar with a custom
    /// per-stream timeout (test seam).
    ///
    /// For each stream currently in the manager whose status is not `Stopped`:
    ///   1. Send `ControlMessage::AddStream { id, config }`
    ///   2. Wait up to `per_stream_timeout` for a `SidecarEvent::StreamAdded`
    ///      whose `id` matches, draining any non-matching events meanwhile.
    ///   3. On success: update rtsp_url via `set_rtsp_url()`, transition to Running.
    ///   4. On timeout / ErrorResponse / channel-closed: transition to Error,
    ///      record failure.
    ///
    /// Streams are replayed serially. Bounded concurrency (max 4 in flight,
    /// per spec §4.7) is deferred until the response multiplexer (Task 5.1)
    /// lands — until then `SidecarHandle::recv()` is single-consumer, so
    /// concurrent recvs would race on the lock.
    ///
    // TODO(Phase 5): Replace serial replay with bounded-concurrency (max 4 in flight)
    // replay once the response multiplexer (Task 5.1) lands. The multiplexer will
    // correlate request/response by `id` so multiple AddStream sends can overlap
    // without recv() contention.
    //
    // KNOWN LIMITATION: when an event arrives for a different stream_id than
    // the one we're waiting for, we keep reading (drain). We don't requeue it.
    // If a previous burst left orphan events in the channel, this can starve
    // other consumers of `handle.recv()`. For the integration test (5 fresh
    // streams on a fresh mock sidecar), this won't trigger.
    pub async fn replay_to_with_timeout(
        &self,
        handle: &SidecarHandle,
        per_stream_timeout: Duration,
    ) -> ReplaySummary {
        // Snapshot streams under read lock; release before any await so we
        // don't hold the lock across sidecar I/O.
        let snapshot: Vec<(String, StreamConfig)> = {
            let streams = self.streams.read();
            streams
                .iter()
                .filter(|(_, s)| s.status != StreamStatus::Stopped)
                .map(|(id, s)| (id.clone(), s.config.clone()))
                .collect()
        };

        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for (id, config) in snapshot {
            let send_result = handle
                .send(&ControlMessage::AddStream {
                    id: id.clone(),
                    config: serde_json::to_value(&config).unwrap_or_default(),
                })
                .await;

            if let Err(e) = send_result {
                let _ = self.transition(&id, StreamStatus::Error);
                failed.push(ReplayFailure {
                    stream_id: id,
                    error: format!("send failed: {}", e),
                });
                continue;
            }

            // Wait up to per_stream_timeout for matching StreamAdded or ErrorResponse.
            // Drain non-matching events (see KNOWN LIMITATION above).
            let outcome = tokio::time::timeout(per_stream_timeout, async {
                loop {
                    match handle.recv().await {
                        Some(SidecarEvent::StreamAdded {
                            id: resp_id,
                            stream_id,
                            rtsp_url,
                        }) if resp_id == id =>
                        {
                            return Ok((stream_id, rtsp_url));
                        }
                        Some(SidecarEvent::ErrorResponse {
                            id: resp_id,
                            code,
                            message,
                        }) if resp_id == id =>
                        {
                            return Err(format!("sidecar rejected: {} ({})", message, code));
                        }
                        Some(_) => continue, // not ours, keep draining
                        None => return Err("sidecar stdout closed".to_string()),
                    }
                }
            })
            .await;

            match outcome {
                Ok(Ok((stream_id, rtsp_url))) => {
                    // Best-effort: update rtsp_url + transition to Running.
                    let _ = self.set_rtsp_url(&id, rtsp_url);
                    let _ = self.transition(&id, StreamStatus::Running);
                    succeeded.push(id.clone());
                    info!(
                        stream_id = %id,
                        side_stream_id = %stream_id,
                        "replay succeeded"
                    );
                }
                Ok(Err(msg)) => {
                    let _ = self.transition(&id, StreamStatus::Error);
                    failed.push(ReplayFailure {
                        stream_id: id,
                        error: msg,
                    });
                }
                Err(_elapsed) => {
                    let _ = self.transition(&id, StreamStatus::Error);
                    failed.push(ReplayFailure {
                        stream_id: id,
                        error: format!(
                            "timeout after {}s",
                            per_stream_timeout.as_secs()
                        ),
                    });
                }
            }
        }

        ReplaySummary { succeeded, failed }
    }
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// Returns true iff `from -> to` is a legal transition per the spec table.
///
/// Self-transitions (e.g. Running -> Running) are explicitly allowed as
/// no-op successes — they let callers idempotently nudge status without
/// first checking the current value.
pub fn is_legal_transition(from: StreamStatus, to: StreamStatus) -> bool {
    if from == to {
        return true;
    }
    // `matches!` here reads as a list of allowed edges; anything not listed
    // returns false. Order within each arm mirrors the spec table rows.
    match (from, to) {
        // Connecting
        (StreamStatus::Connecting, StreamStatus::Running) => true,
        (StreamStatus::Connecting, StreamStatus::Error) => true,
        (StreamStatus::Connecting, StreamStatus::Stopped) => true,
        // Running
        (StreamStatus::Running, StreamStatus::Degraded) => true,
        (StreamStatus::Running, StreamStatus::Reconnecting) => true,
        (StreamStatus::Running, StreamStatus::Error) => true,
        (StreamStatus::Running, StreamStatus::Stopped) => true,
        // Degraded
        (StreamStatus::Degraded, StreamStatus::Running) => true,
        (StreamStatus::Degraded, StreamStatus::Reconnecting) => true,
        (StreamStatus::Degraded, StreamStatus::Error) => true,
        (StreamStatus::Degraded, StreamStatus::Stopped) => true,
        // Reconnecting
        (StreamStatus::Reconnecting, StreamStatus::Running) => true,
        (StreamStatus::Reconnecting, StreamStatus::Error) => true,
        (StreamStatus::Reconnecting, StreamStatus::Stopped) => true,
        // Error
        (StreamStatus::Error, StreamStatus::Connecting) => true,
        (StreamStatus::Error, StreamStatus::Stopped) => true,
        // Stopped
        (StreamStatus::Stopped, StreamStatus::Connecting) => true,
        // Everything else (Running -> Connecting, Stopped -> Running, etc.)
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Convenience: minimal StreamConfig with the given id.
    fn cfg(id: &str) -> StreamConfig {
        StreamConfig {
            stream_id: id.to_string(),
            source: StreamSource {
                source_type: "rtsp".to_string(),
                url: format!("rtsp://example/{id}"),
                rtsp_transport: None,
                latency_ms: None,
                retry_count: None,
            },
            model: "yolov8n".to_string(),
            model_config: None,
            tracker: None,
            analytics: None,
            output: None,
            events: None,
        }
    }

    // ---- Task 4.1 -------------------------------------------------------

    #[test]
    fn add_3_list_returns_3_remove_1_list_returns_2() {
        let mgr = StreamManager::new(16);
        for id in ["s1", "s2", "s3"] {
            mgr.add(cfg(id)).expect("add");
        }
        assert_eq!(mgr.list().len(), 3);
        mgr.remove("s2").expect("remove");
        assert_eq!(mgr.list().len(), 2);
    }

    #[test]
    fn get_non_existent_returns_none() {
        let mgr = StreamManager::new(16);
        assert!(mgr.get("nope").is_none());
    }

    #[test]
    fn add_duplicate_returns_already_exists() {
        let mgr = StreamManager::new(16);
        mgr.add(cfg("dup")).expect("first add");
        let err = mgr.add(cfg("dup")).unwrap_err();
        assert!(matches!(err, StreamManagerError::AlreadyExists(_)));
    }

    #[test]
    fn add_over_max_streams_returns_max_streams_reached() {
        let mgr = StreamManager::new(2);
        mgr.add(cfg("a")).expect("add 1");
        mgr.add(cfg("b")).expect("add 2");
        let err = mgr.add(cfg("c")).unwrap_err();
        assert!(matches!(err, StreamManagerError::MaxStreamsReached(2)));
        assert_eq!(mgr.len(), 2);
    }

    #[test]
    fn concurrent_adds_do_not_deadlock() {
        // parking_lot::RwLock is sync — using std::thread::scope is the
        // simplest faithful test of cross-thread contention. If the lock
        // deadlocks, scope::join hangs and the test times out.
        let mgr = std::sync::Arc::new(StreamManager::new(16));
        std::thread::scope(|s| {
            let m1 = mgr.clone();
            let m2 = mgr.clone();
            s.spawn(move || m1.add(cfg("t1")).expect("t1 add"));
            s.spawn(move || m2.add(cfg("t2")).expect("t2 add"));
        });
        assert_eq!(mgr.len(), 2);
    }

    // ---- Task 4.2 -------------------------------------------------------

    #[test]
    fn legal_transition_path_happy() {
        let mgr = StreamManager::new(16);
        mgr.add(cfg("happy")).expect("add");
        // Connecting -> Running -> Degraded -> Running -> Stopped
        mgr.transition("happy", StreamStatus::Running).expect("-> Running");
        mgr.transition("happy", StreamStatus::Degraded).expect("-> Degraded");
        mgr.transition("happy", StreamStatus::Running).expect("-> Running (recovered)");
        mgr.transition("happy", StreamStatus::Stopped).expect("-> Stopped");
    }

    #[test]
    fn illegal_transition_stopped_to_running_rejected() {
        let mgr = StreamManager::new(16);
        mgr.add(cfg("x")).expect("add");
        mgr.transition("x", StreamStatus::Running).expect("-> Running");
        mgr.transition("x", StreamStatus::Stopped).expect("-> Stopped");
        // Stopped -> Running is NOT in the legal list
        let err = mgr.transition("x", StreamStatus::Running).unwrap_err();
        assert!(matches!(
            err,
            StreamManagerError::IllegalTransition {
                from: StreamStatus::Stopped,
                to: StreamStatus::Running,
            }
        ));
        // Stopped -> Connecting (re-add) IS legal.
        mgr.transition("x", StreamStatus::Connecting).expect("-> Connecting (re-add)");
    }

    #[test]
    fn illegal_transition_running_to_connecting_rejected() {
        let mgr = StreamManager::new(16);
        mgr.add(cfg("y")).expect("add");
        mgr.transition("y", StreamStatus::Running).expect("-> Running");
        let err = mgr.transition("y", StreamStatus::Connecting).unwrap_err();
        assert!(matches!(
            err,
            StreamManagerError::IllegalTransition {
                from: StreamStatus::Running,
                to: StreamStatus::Connecting,
            }
        ));
    }

    #[test]
    fn same_state_transition_is_no_op_success() {
        let mgr = StreamManager::new(16);
        mgr.add(cfg("z")).expect("add");
        mgr.transition("z", StreamStatus::Running).expect("-> Running");
        // Running -> Running must succeed (idempotent nudge).
        mgr.transition("z", StreamStatus::Running).expect("Running -> Running");
    }

    #[test]
    fn transition_on_missing_stream_returns_not_found() {
        let mgr = StreamManager::new(16);
        let err = mgr.transition("ghost", StreamStatus::Running).unwrap_err();
        assert!(matches!(err, StreamManagerError::NotFound(_)));
    }
}
