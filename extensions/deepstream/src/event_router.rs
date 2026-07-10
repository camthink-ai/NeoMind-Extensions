//! Event router (Tasks 5.1, 5.2, 5.2b, 5.4).
//!
//! The router is a **push-based transformation stage** that sits between the
//! sidecar stdout reader (Phase 6+ owns that loop) and four output channels.
//! It does NOT own the `SidecarHandle::recv()` loop — that would starve the
//! command handlers in `lib.rs` which call `wait_event(...)` and need direct
//! access to `handle.recv()`.
//!
//! Responsibilities:
//! - **5.1** Classify each `SidecarEvent` into one of four channels (priority,
//!   business, detection, stats) or report it as not-routable (command-handler
//!   territory: Ready/HelloAck/Pong/ErrorResponse/Bye).
//! - **5.2** Apply a per-stream token bucket to `Detection` events (default 1 Hz,
//!   configurable via `set_detection_hz`).
//! - **5.2b** Deduplicate `LineCross` and `ROIIntrusion` events within a 3s TTL
//!   window keyed by `(stream_id, rule_id, track_id)`.
//! - **5.4** Best-effort publish each routed event to the NeoMind EventBus via
//!   the SDK's CapabilityContext.
//!
//! ## Channel sizing
//!
//! - `priority` (Pong/Bye) and `business` (StreamAdded/StreamError/LineCross/ROIIntrusion)
//!   are `mpsc::unbounded_channel` — these are low-volume and must not block.
//! - `detection` (Detection + AnalyticsSnapshot) is `mpsc::channel(512)` — bounded
//!   because Detection can fire at the configured rate per stream. On full,
//!   the new event is dropped (drop-newest policy) and a warning is logged.
//! - `stats` is `mpsc::channel(64)` — Stats is small but bounded to prevent
//!   a runaway stats emitter from eating memory. Same drop-newest policy.
//!
//! The caller (extension's `init()` in a future task) creates all four channels
//! and hands the senders to `EventRouter::new`, keeping the receivers for its
//! own consumer tasks.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc;

use neomind_extension_sdk::CapabilityContext;

use crate::protocol::SidecarEvent;

/// Default Detection rate limit: 1 event per stream per second.
pub const DEFAULT_DETECTION_HZ: f32 = 1.0;

/// Dedup window for LineCross / ROIIntrusion events.
const DEDUP_TTL: Duration = Duration::from_secs(3);

/// Default capacity for the bounded Detection channel.
pub const DETECTION_CHANNEL_CAPACITY: usize = 512;

/// Default capacity for the bounded Stats channel.
pub const STATS_CHANNEL_CAPACITY: usize = 64;

// ============================================================================
// Public types
// ============================================================================

/// Which output channel an event was routed to (Task 5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Priority,
    Business,
    Detection,
    Stats,
}

/// Result of routing a single event through the router.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingOutcome {
    /// Event was successfully sent to the named channel.
    Routed(Channel),
    /// Detection dropped by per-stream rate limit (Task 5.2).
    DroppedByRateLimit,
    /// LineCross / ROIIntrusion dropped by dedup cache (Task 5.2b).
    DroppedByDedup,
    /// Bounded channel (Detection 512 / Stats 64) was full — drop-newest.
    DroppedByFullChannel,
    /// Ready / HelloAck / Pong / ErrorResponse / Bye — these belong to the
    /// command handlers and the heartbeat task, not the output channels.
    NotRoutable,
}

/// Wrapper carrying the original event alongside its computed `event_type`
/// string (used for EventBus publish and metric labels).
#[derive(Debug, Clone)]
pub struct RoutedEvent {
    /// Dotted event type string, e.g. `"deepstream.detection"`.
    pub event_type: &'static str,
    /// JSON-serialized event payload for EventBus publish and downstream
    /// consumers that don't want to re-serialize.
    pub payload: serde_json::Value,
    /// The original event, in case a consumer wants structured access.
    pub original: SidecarEvent,
}

// ============================================================================
// Rate limiter (Task 5.2)
// ============================================================================

/// Per-stream token bucket for Detection events.
///
/// Simple elapsed-time gate: emits at most once per `1/hz` seconds. Not a
/// full token bucket (no burst capacity) — for Detection at 1 Hz, bursts
/// are undesirable (we want a steady sample rate per stream).
#[derive(Debug, Clone)]
pub struct RateLimiter {
    last_emit: Instant,
    hz: f32,
}

impl RateLimiter {
    /// Create with the given frequency (events per second). A higher `hz`
    /// means more frequent emissions. `hz <= 0` disables rate-limiting
    /// (always emits).
    pub fn new(hz: f32) -> Self {
        // Initialize last_emit to the epoch so the first event always passes
        // (no artificial startup delay).
        Self {
            last_emit: Instant::now() - Duration::from_secs(86400),
            hz,
        }
    }

    /// Returns `true` if enough time has elapsed since the last emission;
    /// updates `last_emit` when returning `true`.
    ///
    /// `now` is passed in (rather than read from `Instant::now()` inside)
    /// so tests can drive the clock deterministically without
    /// `tokio::time::pause` (which only affects `tokio::time`, not `Instant`).
    pub fn check_and_update(&mut self, now: Instant) -> bool {
        if self.hz <= 0.0 {
            return true;
        }
        let min_interval = Duration::from_secs_f32(1.0 / self.hz);
        if now.duration_since(self.last_emit) >= min_interval {
            self.last_emit = now;
            true
        } else {
            false
        }
    }

    /// Current configured frequency.
    pub fn hz(&self) -> f32 {
        self.hz
    }
}

// ============================================================================
// Dedup cache (Task 5.2b)
// ============================================================================

/// Deduplicates `LineCross` and `ROIIntrusion` events within a sliding TTL
/// window, keyed by `(stream_id, rule_id, track_id)`.
///
/// Lazy eviction: each `check_and_record` call first drops any entries older
/// than the TTL, so the cache doesn't grow unboundedly even if a stream has
/// many distinct tracks pass through it. For long-running streams with stable
/// track IDs, the steady-state size is bounded by
/// `(# streams × # rules × # concurrent tracks)` — small in practice.
#[derive(Debug)]
pub struct DedupCache {
    entries: HashMap<(String, String, u64), Instant>,
    ttl: Duration,
}

impl DedupCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            ttl: DEDUP_TTL,
        }
    }

    /// Returns `true` if the event should be emitted (no recent emission for
    /// this key within TTL). On `true`, records `now` as the last-emit time
    /// for the key. On `false`, the event is a duplicate within the TTL window.
    ///
    /// Eviction is performed first: any entry whose timestamp is older than
    /// `now - ttl` is removed. This bounds memory without needing a separate
    /// janitor task.
    pub fn check_and_record(&mut self, key: (String, String, u64), now: Instant) -> bool {
        // Lazy eviction: drop anything older than `now - ttl` before checking.
        let cutoff = now.checked_sub(self.ttl).unwrap_or(now);
        self.entries.retain(|_, ts| *ts >= cutoff);

        // Entry-API form avoids a double hash lookup (clippy::map_entry).
        use std::collections::hash_map::Entry;
        match self.entries.entry(key) {
            Entry::Vacant(e) => {
                e.insert(now);
                true
            }
            Entry::Occupied(_) => {
                // Within TTL window — duplicate.
                false
            }
        }
    }

    /// Number of currently-tracked keys (debug / tests).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for DedupCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EventRouter
// ============================================================================

/// Push-based transformation stage: classify + rate-limit + dedup + EventBus publish.
///
/// See the module docs for the architecture rationale (in particular, why
/// this doesn't own the `SidecarHandle::recv()` loop).
pub struct EventRouter {
    /// Per-stream Detection rate limiters. Keyed by stream_id.
    rate_limiters: Mutex<HashMap<String, RateLimiter>>,
    /// Dedup cache for LineCross / ROIIntrusion. Shared across streams —
    /// keys already include stream_id so there's no cross-talk.
    dedup: Mutex<DedupCache>,
    /// EventBus context for Task 5.4 publish. None until `set_context` is
    /// called from the extension's `init()`.
    context: RwLock<Option<CapabilityContext>>,
    /// Output senders. Unbounded for priority + business, bounded for
    /// detection (512) + stats (64).
    priority_tx: mpsc::UnboundedSender<RoutedEvent>,
    business_tx: mpsc::UnboundedSender<RoutedEvent>,
    detection_tx: mpsc::Sender<RoutedEvent>,
    stats_tx: mpsc::Sender<RoutedEvent>,
}

impl EventRouter {
    /// Construct with four pre-created senders. The caller keeps the receivers.
    ///
    /// The bounded senders should be created with `mpsc::channel(512)` for
    /// detection and `mpsc::channel(64)` for stats (see module-level consts).
    pub fn new(
        priority_tx: mpsc::UnboundedSender<RoutedEvent>,
        business_tx: mpsc::UnboundedSender<RoutedEvent>,
        detection_tx: mpsc::Sender<RoutedEvent>,
        stats_tx: mpsc::Sender<RoutedEvent>,
    ) -> Self {
        Self {
            rate_limiters: Mutex::new(HashMap::new()),
            dedup: Mutex::new(DedupCache::new()),
            context: RwLock::new(None),
            priority_tx,
            business_tx,
            detection_tx,
            stats_tx,
        }
    }

    /// Update the per-stream Detection rate. Called when `add_stream` configures
    /// `events.detection_hz`. A stream not seen before will get a fresh
    /// RateLimiter on its next Detection event using the default rate; this
    /// method overrides that default ahead of time.
    pub fn set_detection_hz(&self, stream_id: &str, hz: f32) {
        let mut limiters = self.rate_limiters.lock();
        limiters
            .entry(stream_id.to_string())
            .or_insert_with(|| RateLimiter::new(DEFAULT_DETECTION_HZ))
            .hz = hz;
    }

    /// Drop the per-stream rate limiter and any dedup entries for this stream.
    /// Called when `remove_stream` succeeds. Dedup keys include stream_id,
    /// so we remove any entry whose key.0 matches — slightly more allocation
    /// than tracking separate per-stream maps but simpler and correct.
    pub fn forget_stream(&self, stream_id: &str) {
        self.rate_limiters.lock().remove(stream_id);
        // Drain dedup entries for this stream in-place.
        let mut dedup = self.dedup.lock();
        dedup.entries.retain(|(sid, _, _), _| sid != stream_id);
    }

    /// Inject the EventBus context. Called from extension `init()`.
    pub fn set_context(&self, ctx: CapabilityContext) {
        *self.context.write() = Some(ctx);
    }

    /// Test helper: check whether a rate limiter exists for a stream.
    pub fn has_rate_limiter(&self, stream_id: &str) -> bool {
        self.rate_limiters.lock().contains_key(stream_id)
    }

    /// Test helper: number of entries currently tracked in the dedup cache.
    pub fn dedup_len(&self) -> usize {
        self.dedup.lock().len()
    }

    /// Push one `SidecarEvent` through classification + rate-limit + dedup +
    /// EventBus publish. Returns the routing outcome.
    ///
    /// This is the entry point the sidecar stdout reader (Phase 6+) will call
    /// for each parsed event. It is `async` for forward compatibility (Phase 6
    /// may add async consumer hooks), even though the current implementation
    /// is effectively synchronous (bounded-channel sends use `try_send`).
    pub async fn route(&self, event: SidecarEvent) -> RoutingOutcome {
        self.route_at(event, Instant::now()).await
    }

    /// Time-injected variant of `route` for deterministic tests.
    ///
    /// `now` is passed to the rate limiter and dedup cache so tests can
    /// advance the clock without `tokio::time::pause` (which only affects
    /// `tokio::time::*`, not `std::time::Instant`).
    pub async fn route_at(&self, event: SidecarEvent, now: Instant) -> RoutingOutcome {
        let (channel, event_type, payload) = match &event {
            // Command-handler territory — not routed through the four channels.
            SidecarEvent::Ready { .. }
            | SidecarEvent::HelloAck { .. }
            | SidecarEvent::Pong { .. }
            | SidecarEvent::ErrorResponse { .. }
            | SidecarEvent::Bye { .. } => {
                return RoutingOutcome::NotRoutable;
            }

            // Business channel — no rate-limit, no dedup.
            SidecarEvent::StreamAdded { .. } => (
                Channel::Business,
                "deepstream.stream_added",
                serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
            ),
            SidecarEvent::StreamRemoved { .. } => (
                Channel::Business,
                "deepstream.stream_removed",
                serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
            ),
            SidecarEvent::StreamError { .. } => (
                Channel::Business,
                "deepstream.stream_error",
                serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
            ),

            // Detection channel — subject to per-stream rate-limit (Task 5.2).
            SidecarEvent::Detection { stream_id, .. } => {
                let allow = {
                    let mut limiters = self.rate_limiters.lock();
                    let limiter = limiters
                        .entry(stream_id.clone())
                        .or_insert_with(|| RateLimiter::new(DEFAULT_DETECTION_HZ));
                    limiter.check_and_update(now)
                };
                if !allow {
                    return RoutingOutcome::DroppedByRateLimit;
                }
                (
                    Channel::Detection,
                    "deepstream.detection",
                    serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
                )
            }

            // Detection channel — NO rate-limit. AnalyticsSnapshot is a periodic
            // aggregate (e.g. one-per-second per stream); the bounded detection
            // channel is enough backpressure.
            SidecarEvent::AnalyticsSnapshot { .. } => (
                Channel::Detection,
                "deepstream.analytics_snapshot",
                serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
            ),

            // Business channel — subject to dedup (Task 5.2b).
            SidecarEvent::LineCross {
                stream_id,
                line_id,
                track_id,
                ..
            } => {
                let key = (stream_id.clone(), line_id.clone(), u64::from(*track_id));
                if !self.dedup.lock().check_and_record(key, now) {
                    return RoutingOutcome::DroppedByDedup;
                }
                (
                    Channel::Business,
                    "deepstream.line_cross",
                    serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
                )
            }
            SidecarEvent::ROIIntrusion {
                stream_id,
                roi_id,
                track_id,
                ..
            } => {
                let key = (stream_id.clone(), roi_id.clone(), u64::from(*track_id));
                if !self.dedup.lock().check_and_record(key, now) {
                    return RoutingOutcome::DroppedByDedup;
                }
                (
                    Channel::Business,
                    "deepstream.roi_intrusion",
                    serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
                )
            }

            // Stats channel.
            SidecarEvent::Stats(_) => (
                Channel::Stats,
                "deepstream.stats",
                serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
            ),
        };

        let routed = RoutedEvent {
            event_type,
            payload: payload.clone(),
            original: event,
        };

        // Task 5.4: best-effort EventBus publish. Done BEFORE the channel send
        // so that even if the channel is full or closed (nobody consuming the
        // internal channels), the event still reaches the NeoMind EventBus and
        // the frontend. The channel is a secondary internal delivery path.
        self.maybe_publish(event_type, &payload);

        // Channel send. Bounded channels (Detection, Stats) use try_send so
        // we can apply drop-newest on Full without awaiting.
        let send_outcome = match channel {
            Channel::Priority => {
                // Priority channel is unbounded in the current design (only
                // Pong/Bye route here, both low-volume). Use send, not try_send.
                // UnboundedSender::send is infallible (only Err on receiver dropped).
                let _ = self.priority_tx.send(routed);
                Ok(())
            }
            Channel::Business => {
                let _ = self.business_tx.send(routed);
                Ok(())
            }
            Channel::Detection => self.detection_tx.try_send(routed).map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => ChannelSendError::Full,
                mpsc::error::TrySendError::Closed(_) => ChannelSendError::Closed,
            }),
            Channel::Stats => self.stats_tx.try_send(routed).map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => ChannelSendError::Full,
                mpsc::error::TrySendError::Closed(_) => ChannelSendError::Closed,
            }),
        };

        if let Err(ChannelSendError::Full) = send_outcome {
            tracing::warn!(
                channel = ?channel,
                "bounded channel full — dropping event (drop-newest)"
            );
            return RoutingOutcome::DroppedByFullChannel;
        }
        // Closed channel: log but treat as Routed (the consumer's gone, but
        // that's not the producer's concern — the extension is shutting down
        // and the receiver is dropped before the senders). Don't fail the
        // routing path over it.
        if let Err(ChannelSendError::Closed) = send_outcome {
            tracing::debug!(channel = ?channel, "channel closed — event not delivered");
        }

        RoutingOutcome::Routed(channel)
    }

    /// Best-effort EventBus publish. Failure is logged but does not affect
    /// the routing outcome — the channel send above is the primary delivery.
    fn maybe_publish(&self, event_type: &str, payload: &serde_json::Value) {
        let ctx_guard = self.context.read();
        let Some(ctx) = ctx_guard.as_ref() else {
            return;
        };

        // CapabilityContext::invoke_capability is sync and returns a Value
        // containing `{success, ...}` or `{success:false, error}`. We treat
        // the error shape as best-effort: log + move on.
        let result = ctx.invoke_capability(
            "event_publish",
            &serde_json::json!({ "event_type": event_type, "payload": payload }),
        );
        if result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            // Published successfully. Trace-level by default to avoid log
            // spam — flip to `RUST_LOG=neomind_extension_deepstream=trace`
            // to confirm the publish path is alive.
            tracing::trace!(
                event_type = %event_type,
                "EventBus publish ok"
            );
        } else {
            tracing::warn!(
                event_type = %event_type,
                result = ?result,
                "EventBus publish did not succeed (best-effort; channel delivery unaffected)"
            );
        }
    }
}

#[derive(Debug)]
enum ChannelSendError {
    Full,
    Closed,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{GpuInfo, StreamStat};

    /// Build a router wired up to four fresh channels. Returns the router
    /// plus the four receivers so tests can drain them.
    struct Harness {
        router: EventRouter,
        priority_rx: mpsc::UnboundedReceiver<RoutedEvent>,
        business_rx: mpsc::UnboundedReceiver<RoutedEvent>,
        detection_rx: mpsc::Receiver<RoutedEvent>,
        stats_rx: mpsc::Receiver<RoutedEvent>,
    }

    impl Harness {
        fn new() -> Self {
            let (priority_tx, priority_rx) = mpsc::unbounded_channel();
            let (business_tx, business_rx) = mpsc::unbounded_channel();
            let (detection_tx, detection_rx) = mpsc::channel(DETECTION_CHANNEL_CAPACITY);
            let (stats_tx, stats_rx) = mpsc::channel(STATS_CHANNEL_CAPACITY);
            Self {
                router: EventRouter::new(priority_tx, business_tx, detection_tx, stats_tx),
                priority_rx,
                business_rx,
                detection_rx,
                stats_rx,
            }
        }
    }

    fn sample_detection(stream_id: &str) -> SidecarEvent {
        SidecarEvent::Detection {
            stream_id: stream_id.to_string(),
            ts: 1,
            frame_id: 1,
            objects: vec![],
        }
    }

    fn sample_line_cross(stream_id: &str, line_id: &str, track_id: u32) -> SidecarEvent {
        SidecarEvent::LineCross {
            stream_id: stream_id.to_string(),
            ts: 1,
            line_id: line_id.to_string(),
            track_id,
            class: 0,
            direction: "down".into(),
        }
    }

    fn sample_roi_intrusion(stream_id: &str, roi_id: &str, track_id: u32) -> SidecarEvent {
        SidecarEvent::ROIIntrusion {
            stream_id: stream_id.to_string(),
            ts: 1,
            roi_id: roi_id.to_string(),
            track_id,
            class: 0,
            mode: "enter".into(),
        }
    }

    fn sample_stats() -> SidecarEvent {
        SidecarEvent::Stats(crate::protocol::Stats {
            ts: 1,
            global_fps: 30.0,
            gpu_utilization_percent: 50.0,
            gpu_memory_used_mb: 1024.0,
            per_stream: vec![StreamStat {
                stream_id: "cam".into(),
                fps: 30.0,
                latency_ms: 10.0,
                frame_count: 1,
                object_count: 0,
                status: "running".into(),
            }],
        })
    }

    fn sample_stream_error() -> SidecarEvent {
        SidecarEvent::StreamError {
            stream_id: "cam".into(),
            code: "EPIPE".into(),
            message: "rtsp source closed".into(),
            id: None,
        }
    }

    fn sample_ready() -> SidecarEvent {
        SidecarEvent::Ready {
            ds_ver: "7.1".into(),
            pyds_ver: "1.1".into(),
            protocol_ver: 1,
            gpu_info: GpuInfo {
                name: "Orin".into(),
                mem_mb: 8192,
            },
        }
    }

    // --- Task 5.1: classification ----------------------------------------

    #[tokio::test]
    async fn detection_routed_to_detection_channel() {
        let mut h = Harness::new();
        let outcome = h.router.route(sample_detection("cam1")).await;
        assert_eq!(outcome, RoutingOutcome::Routed(Channel::Detection));
        assert!(h.detection_rx.try_recv().is_ok());
        // Other channels should be empty.
        assert!(h.priority_rx.try_recv().is_err());
        assert!(h.business_rx.try_recv().is_err());
        assert!(h.stats_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn linecross_routed_to_business_channel() {
        let mut h = Harness::new();
        let outcome = h.router.route(sample_line_cross("cam1", "l1", 42)).await;
        assert_eq!(outcome, RoutingOutcome::Routed(Channel::Business));
        let ev = h.business_rx.try_recv().expect("business recv");
        assert_eq!(ev.event_type, "deepstream.line_cross");
    }

    #[tokio::test]
    async fn stats_routed_to_stats_channel() {
        let mut h = Harness::new();
        let outcome = h.router.route(sample_stats()).await;
        assert_eq!(outcome, RoutingOutcome::Routed(Channel::Stats));
        let ev = h.stats_rx.try_recv().expect("stats recv");
        assert_eq!(ev.event_type, "deepstream.stats");
    }

    #[tokio::test]
    async fn stream_error_routed_to_business() {
        let mut h = Harness::new();
        let outcome = h.router.route(sample_stream_error()).await;
        assert_eq!(outcome, RoutingOutcome::Routed(Channel::Business));
        let ev = h.business_rx.try_recv().expect("business recv");
        assert_eq!(ev.event_type, "deepstream.stream_error");
    }

    #[tokio::test]
    async fn ready_is_not_routable() {
        let h = Harness::new();
        let outcome = h.router.route(sample_ready()).await;
        assert_eq!(outcome, RoutingOutcome::NotRoutable);
    }

    #[tokio::test]
    async fn all_command_handler_events_are_not_routable() {
        // Ready, HelloAck, Pong, ErrorResponse, Bye all belong to the
        // command-handler / heartbeat / shutdown paths — never routed.
        let cases = vec![
            SidecarEvent::Pong { ts: 1 },
            SidecarEvent::ErrorResponse {
                id: "r1".into(),
                code: "E".into(),
                message: "m".into(),
            },
            SidecarEvent::Bye {
                reason: "shutdown".into(),
                exit_code: 0,
            },
            SidecarEvent::HelloAck {
                max_streams: 32,
                rtsp_url_prefix: "rtsp://x".into(),
                models_loaded: vec![],
            },
            sample_ready(),
        ];
        for ev in cases {
            let h = Harness::new();
            assert_eq!(
                h.router.route(ev).await,
                RoutingOutcome::NotRoutable,
                "expected NotRoutable"
            );
        }
    }

    // --- Task 5.2: rate limit --------------------------------------------

    #[tokio::test]
    async fn detection_rate_limit_drops_excess_within_1s_window() {
        let h = Harness::new();
        // Default 1 Hz. Fire 5 Detection events "now" — only 1 should pass.
        let now = Instant::now();
        let mut routed = 0;
        let mut dropped = 0;
        for _ in 0..5 {
            match h.router.route_at(sample_detection("cam1"), now).await {
                RoutingOutcome::Routed(Channel::Detection) => routed += 1,
                RoutingOutcome::DroppedByRateLimit => dropped += 1,
                other => panic!("unexpected outcome: {other:?}"),
            }
        }
        assert_eq!(routed, 1, "exactly one Detection should pass in 1s window");
        assert_eq!(dropped, 4, "the other 4 should be rate-limited");
    }

    #[tokio::test]
    async fn detection_rate_limit_passes_after_window_elapses() {
        let h = Harness::new();
        let t0 = Instant::now();
        // First event passes.
        assert_eq!(
            h.router.route_at(sample_detection("cam1"), t0).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
        // 500ms later — blocked (1 Hz window = 1s).
        assert_eq!(
            h.router
                .route_at(sample_detection("cam1"), t0 + Duration::from_millis(500))
                .await,
            RoutingOutcome::DroppedByRateLimit
        );
        // 1.1s later — passes again.
        assert_eq!(
            h.router
                .route_at(sample_detection("cam1"), t0 + Duration::from_millis(1100))
                .await,
            RoutingOutcome::Routed(Channel::Detection)
        );
    }

    #[tokio::test]
    async fn detection_rate_limit_independent_per_stream() {
        let h = Harness::new();
        let now = Instant::now();
        // Two streams, both fire at t=now — both should pass.
        assert_eq!(
            h.router.route_at(sample_detection("cam1"), now).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
        assert_eq!(
            h.router.route_at(sample_detection("cam2"), now).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
    }

    #[tokio::test]
    async fn set_detection_hz_overrides_default() {
        let h = Harness::new();
        h.router.set_detection_hz("cam1", 10.0); // 10 Hz = 100ms interval
        let now = Instant::now();
        assert_eq!(
            h.router.route_at(sample_detection("cam1"), now).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
        // 50ms later — still blocked at 10 Hz (needs 100ms).
        assert_eq!(
            h.router
                .route_at(sample_detection("cam1"), now + Duration::from_millis(50))
                .await,
            RoutingOutcome::DroppedByRateLimit
        );
        // 150ms later — passes.
        assert_eq!(
            h.router
                .route_at(sample_detection("cam1"), now + Duration::from_millis(150))
                .await,
            RoutingOutcome::Routed(Channel::Detection)
        );
    }

    #[tokio::test]
    async fn analytics_snapshot_not_rate_limited() {
        let h = Harness::new();
        let now = Instant::now();
        // Fire two AnalyticsSnapshots back-to-back at the same instant — both
        // should pass (no rate-limit on this variant).
        let snap = SidecarEvent::AnalyticsSnapshot {
            stream_id: "cam1".into(),
            ts: 1,
            snapshot: serde_json::json!({}),
        };
        assert_eq!(
            h.router.route_at(snap.clone(), now).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
        assert_eq!(
            h.router.route_at(snap, now).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
    }

    // --- Task 5.2b: dedup ------------------------------------------------

    #[tokio::test]
    async fn linecross_dedup_suppresses_same_track_within_3s() {
        let h = Harness::new();
        let t0 = Instant::now();
        // Three emissions of the same (stream, line, track) within 3s — only
        // the first should pass.
        assert_eq!(
            h.router.route_at(sample_line_cross("cam1", "l1", 7), t0).await,
            RoutingOutcome::Routed(Channel::Business)
        );
        assert_eq!(
            h.router
                .route_at(sample_line_cross("cam1", "l1", 7), t0 + Duration::from_secs(1))
                .await,
            RoutingOutcome::DroppedByDedup
        );
        assert_eq!(
            h.router
                .route_at(sample_line_cross("cam1", "l1", 7), t0 + Duration::from_secs(2))
                .await,
            RoutingOutcome::DroppedByDedup
        );
    }

    #[tokio::test]
    async fn linecross_dedup_passes_after_3s_ttl() {
        let h = Harness::new();
        let t0 = Instant::now();
        assert_eq!(
            h.router.route_at(sample_line_cross("cam1", "l1", 7), t0).await,
            RoutingOutcome::Routed(Channel::Business)
        );
        // 3s + epsilon — TTL has elapsed (lazy eviction clears the entry
        // before the next check).
        assert_eq!(
            h.router
                .route_at(
                    sample_line_cross("cam1", "l1", 7),
                    t0 + Duration::from_secs(3) + Duration::from_millis(50)
                )
                .await,
            RoutingOutcome::Routed(Channel::Business)
        );
    }

    #[tokio::test]
    async fn roi_intrusion_dedup_independent_per_roi() {
        let h = Harness::new();
        let now = Instant::now();
        // Same track crossing two different ROIs — both should emit.
        assert_eq!(
            h.router
                .route_at(sample_roi_intrusion("cam1", "roi_a", 9), now)
                .await,
            RoutingOutcome::Routed(Channel::Business)
        );
        assert_eq!(
            h.router
                .route_at(sample_roi_intrusion("cam1", "roi_b", 9), now)
                .await,
            RoutingOutcome::Routed(Channel::Business)
        );
    }

    #[tokio::test]
    async fn dedup_independent_per_track_id() {
        let h = Harness::new();
        let now = Instant::now();
        // Two different tracks crossing the same line at the same instant —
        // both should emit.
        assert_eq!(
            h.router
                .route_at(sample_line_cross("cam1", "l1", 100), now)
                .await,
            RoutingOutcome::Routed(Channel::Business)
        );
        assert_eq!(
            h.router
                .route_at(sample_line_cross("cam1", "l1", 101), now)
                .await,
            RoutingOutcome::Routed(Channel::Business)
        );
    }

    // --- Task 5.1: bounded-channel backpressure -------------------------

    #[tokio::test]
    async fn bounded_stats_channel_drops_when_full() {
        // Create a router with a stats channel of capacity 2 for a quick test.
        let (priority_tx, _priority_rx) = mpsc::unbounded_channel();
        let (business_tx, _business_rx) = mpsc::unbounded_channel();
        let (detection_tx, _detection_rx) = mpsc::channel(DETECTION_CHANNEL_CAPACITY);
        let (stats_tx, _stats_rx) = mpsc::channel(2);
        let router = EventRouter::new(priority_tx, business_tx, detection_tx, stats_tx);

        // Fill the stats channel to capacity (2). We never drain it, so the
        // receiver stays alive but the buffer fills.
        assert_eq!(
            router.route(sample_stats()).await,
            RoutingOutcome::Routed(Channel::Stats)
        );
        assert_eq!(
            router.route(sample_stats()).await,
            RoutingOutcome::Routed(Channel::Stats)
        );
        // Third event — channel is full. Drop-newest.
        let outcome = router.route(sample_stats()).await;
        assert_eq!(outcome, RoutingOutcome::DroppedByFullChannel);
    }

    #[tokio::test]
    async fn bounded_detection_channel_drops_when_full() {
        let (priority_tx, _priority_rx) = mpsc::unbounded_channel();
        let (business_tx, _business_rx) = mpsc::unbounded_channel();
        let (detection_tx, _detection_rx) = mpsc::channel(1);
        let (stats_tx, _stats_rx) = mpsc::channel(STATS_CHANNEL_CAPACITY);
        let router = EventRouter::new(priority_tx, business_tx, detection_tx, stats_tx);

        // First Detection fills the channel.
        assert_eq!(
            router.route(sample_detection("cam1")).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
        // Second Detection (same stream, same instant) is rate-limited first,
        // so use a different stream to bypass the limiter.
        assert_eq!(
            router.route(sample_detection("cam2")).await,
            RoutingOutcome::DroppedByFullChannel,
            "detection channel capacity=1 should reject the second event"
        );
    }

    // --- forget_stream ----------------------------------------------------

    #[tokio::test]
    async fn forget_stream_clears_rate_limiter_and_dedup() {
        let h = Harness::new();
        let now = Instant::now();
        // Seed both rate limiter + dedup cache for cam1.
        h.router.route_at(sample_detection("cam1"), now).await;
        h.router
            .route_at(sample_line_cross("cam1", "l1", 5), now)
            .await;
        assert!(h.router.has_rate_limiter("cam1"));
        assert_eq!(h.router.dedup_len(), 1);

        h.router.forget_stream("cam1");

        assert!(!h.router.has_rate_limiter("cam1"));
        assert_eq!(h.router.dedup_len(), 0, "dedup entries for cam1 cleared");

        // After forget, the next Detection for cam1 should pass (fresh limiter).
        // Use a clearly different `now` so even if a stale limiter existed,
        // the window would have elapsed.
        let later = now + Duration::from_secs(10);
        assert_eq!(
            h.router.route_at(sample_detection("cam1"), later).await,
            RoutingOutcome::Routed(Channel::Detection)
        );
        // And a LineCross for cam1 should also pass (dedup cache cleared).
        assert_eq!(
            h.router
                .route_at(sample_line_cross("cam1", "l1", 5), later)
                .await,
            RoutingOutcome::Routed(Channel::Business)
        );
    }

    // --- RateLimiter unit tests (no router needed) -----------------------

    #[test]
    fn rate_limiter_first_call_passes() {
        let mut rl = RateLimiter::new(1.0);
        let now = Instant::now();
        assert!(rl.check_and_update(now), "first call should pass");
    }

    #[test]
    fn rate_limiter_second_call_within_window_blocked() {
        let mut rl = RateLimiter::new(1.0);
        let t0 = Instant::now();
        assert!(rl.check_and_update(t0));
        assert!(!rl.check_and_update(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn rate_limiter_zero_hz_always_emits() {
        let mut rl = RateLimiter::new(0.0);
        let now = Instant::now();
        assert!(rl.check_and_update(now));
        assert!(rl.check_and_update(now));
        assert!(rl.check_and_update(now));
    }

    // --- DedupCache unit tests (no router needed) ------------------------

    #[test]
    fn dedup_cache_first_call_emits_second_suppressed() {
        let mut cache = DedupCache::new();
        let now = Instant::now();
        let key = ("s1".into(), "l1".into(), 5u64);
        assert!(cache.check_and_record(key.clone(), now));
        assert!(!cache.check_and_record(key, now));
    }

    #[test]
    fn dedup_cache_evicts_after_ttl() {
        let mut cache = DedupCache::new();
        let t0 = Instant::now();
        let key = ("s1".into(), "l1".into(), 5u64);
        cache.check_and_record(key.clone(), t0);
        // After TTL: should emit again.
        assert!(cache.check_and_record(
            key,
            t0 + Duration::from_secs(3) + Duration::from_millis(50)
        ));
    }
}
