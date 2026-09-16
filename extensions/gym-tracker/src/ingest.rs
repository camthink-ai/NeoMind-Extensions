// ingest.rs — WebSocket subscriber for NE503 `gym/track` events.
//
// Spawns a dedicated OS thread ("gym-ingest") that owns a single-threaded
// Tokio runtime, connects to `wss://<host>/api/v1/events/stream?token=<token>`
// (the NE503 serves a self-signed cert, so we reuse `crate::tls`'s TLS-skip
// `rustls::ClientConfig` via a `tokio_tungstenite::Connector::Rustls`), and
// feeds every `gym/`-prefixed event into the shared `LiveState` mirror.
//
// Why a dedicated thread + own runtime? This crate is a `cdylib` loaded into a
// host process that may or may not have an ambient Tokio runtime. Rather than
// rely on `tokio::runtime::Handle::try_current()` (which the homeassistant-bridge
// uses but which silently no-ops when the host has no runtime), we create our
// own current-thread runtime inside a spawned `std::thread`. That keeps the WS
// I/O off the SDK's metric/command threads entirely — `stop()` flips an
// `AtomicBool` and the `tokio::select!` honors it within ~200ms.
//
// The CLAUDE.md note "use sync ureq, never async" is about the *HTTP* client
// specifically; the homeassistant-bridge precedent shows async WS via
// tokio-tungstenite is fine on a dedicated runtime thread, and we follow that.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::state::LiveState;
use crate::tls;
use crate::types::TrackFrame;

/// Build a WS URL: `<ws_base>?token=<urlencoded token>`.
fn build_ws_url(ws_base: &str, token: &str) -> String {
    format!("{}?token={}", ws_base, urlencoding::encode(token))
}

/// Pure: given one raw WS text message (the NE503 event envelope), return the
/// `TrackFrame` if its topic starts with `gym/`, else `None`. Unit-test THIS —
/// it holds all the parse/filter logic. Everything else here is I/O glue.
///
/// The NE503 event bus delivers envelopes shaped like:
/// ```jsonc
/// {"event_id":"evt-…","payload":"{\"device_id\":\"…\",…}",  // NOTE: payload is a JSON-encoded STRING
///  "payload_type":"json","source":"","timestamp_ns":…,"topic":"gym/test"}
/// ```
/// i.e. `payload` is **double-encoded** — a JSON string whose contents are the
/// serialized `TrackFrame`. Some emitters may inline it as a nested object
/// instead, so we handle both. Topics not under `gym/` (e.g. `device/temp`)
/// are dropped client-side — the bus delivers all topics to every subscriber.
pub fn parse_event(raw: &str) -> Option<TrackFrame> {
    let ev: serde_json::Value = serde_json::from_str(raw).ok()?;
    let topic = ev["topic"].as_str()?;
    if !topic.starts_with("gym/") {
        return None;
    }
    let payload = &ev["payload"];
    // Unwrap one layer of string-encoding when the bus delivered the payload
    // as a JSON string (the real device format). Inline objects pass through.
    let frame_val = match payload {
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s).ok()?,
        other => other.clone(),
    };
    serde_json::from_value::<TrackFrame>(frame_val).ok()
}

/// Parse a `gym/preview` envelope → (ts_ns, JPEG bytes). None for other
/// topics or payloads that don't decode into a JPEG. The device publishes
/// `img_b64`; decoding HERE — once, at the edge — is what lets the whole
/// downstream push path stay binary (no re-encode anywhere).
pub fn parse_preview(raw: &str) -> Option<(u64, Vec<u8>)> {
    let ev: serde_json::Value = serde_json::from_str(raw).ok()?;
    if ev["topic"].as_str()? != "gym/preview" {
        return None;
    }
    let payload = &ev["payload"];
    let p = match payload {
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s).ok()?,
        other => other.clone(),
    };
    let ts_ns = p["ts_ns"].as_u64()?;
    let img = crate::frame::decode_b64_jpeg(p["img_b64"].as_str()?)?;
    Some((ts_ns, img))
}

/// Parse a `gym/preview_h264` envelope → the fields of an [`H264Sample`]
/// (nalu as raw bytes). None for other topics or undecodable payloads.
pub fn parse_preview_h264(
    raw: &str,
) -> Option<(u64, u64, bool, u32, u32, u64, Vec<u8>)> {
    let ev: serde_json::Value = serde_json::from_str(raw).ok()?;
    if ev["topic"].as_str()? != "gym/preview_h264" {
        return None;
    }
    let payload = &ev["payload"];
    let p = match payload {
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s).ok()?,
        other => other.clone(),
    };
    let ts_ns = p["ts_ns"].as_u64()?;
    let pts_ns = p["pts_ns"].as_u64().unwrap_or(0);
    let key = p["key"].as_bool().unwrap_or(false);
    let w = p["w"].as_u64().unwrap_or(0) as u32;
    let h = p["h"].as_u64().unwrap_or(0) as u32;
    use base64::Engine as _;
    let b64 = p["nalu_b64"].as_str()?;
    let nalu = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(b64.trim_end_matches('='))
                .ok()
        })?;
    // Annex-B sanity: must start with a start code (00 00 01 / 00 00 00 01)
    if nalu.len() < 4 || !(nalu.starts_with(&[0, 0, 0, 1]) || nalu.starts_with(&[0, 0, 1])) {
        return None;
    }
    let seq = p["seq"].as_u64().unwrap_or(0);
    Some((ts_ns, pts_ns, key, w, h, seq, nalu))
}

/// Debug counters + liveness trace for ingest diagnostics.
pub static INGEST_COUNTS: std::sync::OnceLock<parking_lot::Mutex<IngestDbg>> =
    std::sync::OnceLock::new();
#[derive(Default, Clone)]
pub struct IngestDbg {
    pub total: u64,
    pub topic_counts: std::collections::HashMap<String, u64>,
    pub parsed_ok: u64,
    pub parse_fail: u64,
    /// Liveness trace — each reconnect-cycle step stamps unix secs. A zeroed
    /// field = the step never ran; an old early stamp with zeroed later
    /// fields = exactly where the loop is stuck.
    pub started_at: u64,
    pub login_at: u64,
    pub login_ok: bool,
    pub ws_connect_at: u64,
    pub ws_url: String,
    pub ws_ok: bool,
    pub last_frame_at: u64,
    pub cycles: u64,
    pub last_error: String,
    /// h264 envelopes skipped because no push session was subscribed
    /// (on-demand ingest).
    pub h264_no_viewer: u64,
    /// Track-latency decomposition (ms, EMA over recent frames):
    /// `gap_cam_ms`  = bus envelope timestamp − payload ts_ns  (camera-
    /// internal: grab→publish through the app + bus hop)
    /// `gap_bus_ms`  = local arrival − envelope timestamp        (bus→
    /// platform→extension; cross-clock, treat as ±50ms approximate)
    pub gap_cam_ms: f64,
    pub gap_bus_ms: f64,
    pub gap_n: u64,
    /// First gym/track raw payload (truncated) — deserialization forensics.
    pub first_track_payload: String,
}
pub fn ingest_dbg() -> &'static parking_lot::Mutex<IngestDbg> {
    INGEST_COUNTS.get_or_init(|| parking_lot::Mutex::new(IngestDbg::default()))
}

/// Payload ts_ns out of a gym/track envelope (double-encoded or inline).
fn frame_ts_hint(raw: &str) -> u64 {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|ev| match &ev["payload"] {
            serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s).ok(),
            other => Some(other.clone()),
        })
        .and_then(|p| p["ts_ns"].as_u64())
        .unwrap_or(0)
}

fn stamp_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Handle returned by `spawn` — call `stop()` to request a graceful shutdown
/// of the ingest thread (honored within ~200ms via the runtime's `select!`).
pub struct IngestHandle {
    stop: Arc<AtomicBool>,
}

impl IngestHandle {
    /// Request the ingest loop to stop. The background thread checks the flag
    /// between WS reads and during reconnect backoff, then exits.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Stop the ingest thread when the handle is dropped, so re-`spawn` (e.g. a
/// re-`configure`) can never orphan the previous thread + WS connection + runtime.
impl Drop for IngestHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Outcome of one connect-and-listen cycle. Drives the outer reconnect loop.
enum Disconnect {
    /// `stop` was requested mid-cycle — exit the whole loop.
    Stop,
    /// Transient error (connect failure, read error, stream closed). Backoff
    /// and retry.
    Error(String),
}

/// Spawn the WS subscriber on a dedicated `std::thread` with its own
/// current-thread Tokio runtime.
///
/// The login token is resolved INSIDE the reconnect loop from `ne` (cache
/// first, re-login on every retry): a login that failed at configure time
/// (device busy, empty token cache) or a token revoked mid-session must not
/// starve the subscriber forever. On this firmware the token includes the
/// `Bearer ` prefix; the primary WS URL uses it verbatim and a Bearer-stripped
/// fallback is tried too, in case the device's WS auth wants the bare secret.
pub fn spawn(
    cfg: Config,
    state: Arc<LiveState>,
    analytics: Arc<crate::analytics::Analytics>,
    db: Arc<crate::db::Db>,
    ne: Arc<crate::ne503::Ne503Client>,
) -> IngestHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_c = stop.clone();
    std::thread::Builder::new()
        .name("gym-ingest".into())
        .spawn(move || {
            run_loop(&ne, &cfg, &state, &analytics, &db, &stop_c);
        })
        .ok();
    IngestHandle { stop }
}

/// The (blocking) reconnect loop, run inside the ingest thread. Owns a
/// current-thread Tokio runtime (the cdylib has no ambient runtime to borrow).
fn run_loop(
    ne: &Arc<crate::ne503::Ne503Client>,
    cfg: &Config,
    state: &Arc<LiveState>,
    analytics: &Arc<crate::analytics::Analytics>,
    db: &Arc<crate::db::Db>,
    stop: &Arc<AtomicBool>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("ingest: failed to build runtime: {e}");
            return;
        }
    };

    rt.block_on(async move {
        let backoff = &cfg.ingest.reconnect_backoff_sec;
        let mut attempt: usize = 0;
        ingest_dbg().lock().started_at = stamp_now();
        while !stop.load(Ordering::Relaxed) {
            ingest_dbg().lock().cycles += 1;
            // Cycle 0 reuses the token cached by configure()'s login; every
            // retry re-logins first (cheap REST call, at most once per backoff
            // cycle) so an empty or dead token can't wedge the subscriber.
            let token = match ne.token_string() {
                Some(t) if attempt == 0 && !t.is_empty() => t,
                _ => {
                    let ok = ne.login().is_ok();
                    let tok = ne.token_string().unwrap_or_default();
                    let mut g = ingest_dbg().lock();
                    g.login_at = stamp_now();
                    g.login_ok = ok && !tok.is_empty();
                    tok
                }
            };
            // Primary URL = token verbatim (Bearer prefix on this firmware).
            // Fallback = bare secret, only if there was a prefix to strip.
            let mut urls: Vec<String> = vec![build_ws_url(&cfg.ws_url(), &token)];
            if let Some(bare) = token.strip_prefix("Bearer ") {
                urls.push(build_ws_url(&cfg.ws_url(), bare));
            }
            match connect_and_drain(&analytics, &db, &urls, cfg, state, stop).await {
                Disconnect::Stop => break,
                Disconnect::Error(e) => {
                    ingest_dbg().lock().last_error = e.chars().take(160).collect();
                    tracing::warn!("ingest: wss cycle ended ({e})");
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // Backoff from config (default [1,2,5,10,30]); clamp to last entry
            // once we've exhausted the schedule.
            let secs = backoff
                .get(attempt)
                .copied()
                .or_else(|| backoff.last().copied())
                .unwrap_or(30);
            attempt = attempt.saturating_add(1);
            tracing::info!("ingest: reconnecting in {secs}s (attempt {})", attempt);
            sleep_with_stop(secs, stop).await;
        }
        tracing::info!("ingest: loop exiting (stop requested)");
    });
}

/// Connect to the first reachable candidate URL, then read WS frames until the
/// connection drops or `stop` is requested.
async fn connect_and_drain(
    analytics: &Arc<crate::analytics::Analytics>,
    db: &Arc<crate::db::Db>,
    urls: &[String],
    cfg: &Config,
    state: &Arc<LiveState>,
    stop: &Arc<AtomicBool>,
) -> Disconnect {
    // Build the TLS connector. Insecure (NoVerify) for the self-signed NE503
    // cert when configured; otherwise None → tokio-tungstenite uses webpki
    // roots (the secure path, which would reject the self-signed cert).
    let connector = if cfg.device.tls_insecure {
        Some(tokio_tungstenite::Connector::Rustls(Arc::new(
            tls::insecure_client_config(),
        )))
    } else {
        None
    };

    // Try each candidate URL (primary, then Bearer-stripped fallback) in order.
    let mut ws_stream = None;
    for url in urls {
        if stop.load(Ordering::Relaxed) {
            return Disconnect::Stop;
        }
        // tokio-tungstenite 0.28 signature: (request, config, disable_nagle, connector).
        match tokio_tungstenite::connect_async_tls_with_config(
            url.as_str(),
            None,
            false,
            connector.clone(),
        )
        .await
        {
            Ok((s, resp)) => {
                tracing::info!("ingest: wss connected to {url} (status {})", resp.status());
                let mut g = ingest_dbg().lock();
                g.ws_connect_at = stamp_now();
                g.ws_url = url.chars().take(120).collect();
                g.ws_ok = resp.status().is_success();
                ws_stream = Some(s);
                break;
            }
            Err(e) => {
                tracing::warn!("ingest: wss connect to {url} failed: {e}");
            }
        }
    }

    let ws_stream = match ws_stream {
        Some(s) => s,
        None => return Disconnect::Error("all connect attempts failed".into()),
    };
    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Read loop: select between the next WS frame and a stop ticker so a
    // shutdown request is honored even while no frames are arriving.
    // KEEPALIVE: a device restart leaves the TCP half-open — without pings
    // the client never notices and the ingest freezes forever (observed:
    // 49 min of zero traffic with the socket "open"). Ping every 5 s and
    // force-reconnect after 20 s of silence.
    let mut last_alive = std::time::Instant::now();
    // DATA-staleness (Text events only) — separate from the keepalive: a
    // half-dead proxied TCP can still answer Pings while delivering zero
    // events (the 2026-09-09 zombie: socket "open", pongs flowing, no data
    // for hours). The camera's bus always carries ≥1 Hz of events while
    // the app runs, so 90 s of event silence means the pipe is dead —
    // reconnect. (A legitimately quiet bus — app stopped — also trips
    // this; the reconnect cycle is cheap and self-limits via backoff.)
    let mut last_event = std::time::Instant::now();
    let mut last_h264_seq: Option<u64> = None;
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(5));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = await_stop(stop) => return Disconnect::Stop,
            _ = ping.tick() => {
                // PERIODIC PERSISTENCE (RS-3, 2026-09-16 audit): the
                // heatmap/crossing flush + workout session close used to
                // ride ONLY the UI's get_live_state poll — dashboard
                // closed meant nothing persisted for hours and departed
                // people's sessions never closed. Piggyback the 5 s
                // keepalive tick (this loop already owns analytics + db +
                // state) so persistence no longer depends on a viewer.
                // get_live_state keeps its own calls as belt-and-braces.
                // Blocking SQLite is fine here: the per-frame arm below
                // already does DB reads on this same dedicated runtime
                // thread. Camera clock per RS-4.
                analytics.maybe_save(db);
                analytics.close_expired_workouts(
                    db,
                    cfg.ingest.track_ttl_sec,
                    state.camera_now_secs(),
                );
                if last_alive.elapsed() > std::time::Duration::from_secs(20) {
                    return Disconnect::Error("keepalive timeout (20s silence)".into());
                }
                if last_event.elapsed() > std::time::Duration::from_secs(90) {
                    return Disconnect::Error(
                        "data staleness (90s without events on a live socket) — \
                         forcing reconnect".into(),
                    );
                }
                if ws_sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return Disconnect::Error("ping send failed".into());
                }
            }
            msg = ws_stream.next() => match msg {
                Some(Ok(Message::Text(txt))) => {
                    last_alive = std::time::Instant::now();
                    last_event = std::time::Instant::now();
                    ingest_dbg().lock().last_frame_at = stamp_now();
                    // SHADOW TRACKER (0.9.x migration): the camera dual-
                    // publishes raw post-dedupe detections on gym/detect;
                    // the Rust five-signal port consumes them here and
                    // snapshots its id assignments for parity validation
                    // against the device's Python tracker (whose ids ride
                    // gym/track — snapshotted in the parse_event arm).
                    if txt.contains("\"gym/detect\"") {
                        if let Ok(ev) = serde_json::from_str::<serde_json::Value>(&txt) {
                            if ev["topic"].as_str() == Some("gym/detect") {
                                let payload = ev["payload"].as_str().unwrap_or("");
                                if let Ok(d) = serde_json::from_str::<serde_json::Value>(payload) {
                                    let ts_ns = d["ts_ns"].as_u64().unwrap_or(0);
                                    let mut dets: Vec<crate::shadow::Det> = Vec::new();
                                    if let Some(arr) = d["dets"].as_array() {
                                        for dj in arr {
                                            let kp: Vec<[f32; 3]> = dj["kp"]
                                                .as_array()
                                                .map(|a| {
                                                    a.iter().map(|k| {
                                                        [k[0].as_f64().unwrap_or(0.0) as f32,
                                                         k[1].as_f64().unwrap_or(0.0) as f32,
                                                         k[2].as_f64().unwrap_or(0.0) as f32]
                                                    }).collect()
                                                })
                                                .unwrap_or_default();
                                            dets.push(crate::shadow::Det {
                                                src: dj["src"].as_str().unwrap_or("full").into(),
                                                ts: dj["ts"].as_u64().unwrap_or(ts_ns),
                                                kp,
                                            });
                                        }
                                    }
                                    let now = ts_ns as f64 / 1e9;
                                    let mut g = crate::shadow::shadow().lock();
                                    let ids = g.0.update(&dets, now);
                                    // center = visible-kpts bbox center — the
                                    // SAME metric the py side reports (bbox
                                    // padding expands symmetrically, center
                                    // unchanged) so parity matching is honest
                                    let assigns = ids.iter().enumerate()
                                        .filter_map(|(i, id)| id.map(|id| {
                                            let mut x0 = f32::MAX; let mut y0 = f32::MAX;
                                            let mut x1 = f32::MIN; let mut y1 = f32::MIN; let mut n = 0;
                                            for k in &dets[i].kp {
                                                if k[2] > 0.0 {
                                                    x0 = x0.min(k[0]); y0 = y0.min(k[1]);
                                                    x1 = x1.max(k[0]); y1 = y1.max(k[1]);
                                                    n += 1;
                                                }
                                            }
                                            if n > 0 { ((x0 + x1) / 2.0, (y0 + y1) / 2.0, id) } else { (0.0, 0.0, id) }
                                        }))
                                        .collect();
                                    g.1.push_rust(crate::shadow::IdSnap { ts_ns, assigns });
                                }
                                // counted + consumed — do not fall through
                                // to parse_event (it would record a parse
                                // failure for this valid payload)
                                {
                                    let mut g = ingest_dbg().lock();
                                    g.parsed_ok += 1;
                                    g.topic_counts.entry("gym/detect".into())
                                        .and_modify(|c| *c += 1).or_insert(1);
                                }
                                continue;
                            }
                        }
                    }
                    tracing::debug!(target: "gym_tracker::ingest::frame", len = txt.len(), "ws text frame");
                    if true { // temp diagnostics: always count topics
                        let topic = serde_json::from_str::<serde_json::Value>(&txt)
                            .ok()
                            .and_then(|v| v["topic"].as_str().map(|t| t.chars().take(24).collect::<String>()))
                            .unwrap_or_else(|| "?".into());
                        {
                            let mut g = ingest_dbg().lock();
                            if topic.starts_with("gym/track") && g.first_track_payload.is_empty() {
                                g.first_track_payload = txt.chars().take(2400).collect();
                            }
                            g.topic_counts.entry(topic).and_modify(|c| *c += 1).or_insert(1);
                        }
                        let n = { let mut g = ingest_dbg().lock(); g.total += 1; g.total };
                        if n % 200 == 0 {
                            let c = ingest_dbg().lock().topic_counts.clone();
                            tracing::info!(target: "gym_tracker::ingest", counts = ?c, "ingest dbg");
                        }
                    }
                    if let Some((pts, pimg)) = parse_preview(&txt) {
                        state.set_preview(pts, Arc::new(pimg));
                        // counted + consumed — mirrors the gym/detect arm:
                        // a valid preview envelope is NOT a TrackFrame, so
                        // the parse_event fallthrough recorded a parse_fail
                        // for every healthy preview frame (RS-6,
                        // 2026-09-16 audit) — the counter was pure noise.
                        continue;
                    } else if let Some((ts, pts, key, w, h, seq, nalu)) = parse_preview_h264(&txt) {
                        // ON-DEMAND h264: with no push session subscribed,
                        // ring churn was pure waste — 30 fps parsed and
                        // immediately dropped by the 120-frame ring's
                        // overflow (a WARN per dropped frame when nobody
                        // watched). Live-edge join (sessions start at the
                        // ring HEAD) means no consumer needs history, so
                        // skipping ingest while the session map is empty is
                        // safe; the next session picks up frames that
                        // arrive AFTER its start_push registered it.
                        if crate::push_sessions().lock().is_empty() {
                            ingest_dbg().lock().h264_no_viewer += 1;
                        } else {
                        // relay seq continuity check: a gap means the
                        // device→platform hop dropped frames (event bus /
                        // relay). Warn loudly — the decoder downstream will
                        // corrupt until the next keyframe; only a ≤1 s GOP
                        // bounds the damage.
                        crate::state::push_diag().ingested.fetch_add(1, crate::state::AtomicOrd::Relaxed);
                        if seq > 1 {
                            if let Some(prev) = last_h264_seq.replace(seq) {
                                if seq > prev + 1 {
                                    crate::state::push_diag().ingest_gaps
                                        .fetch_add(seq - prev - 1, crate::state::AtomicOrd::Relaxed);
                                    tracing::warn!(
                                        expected = prev + 1,
                                        got = seq,
                                        gap = seq - prev - 1,
                                        "h264 frame loss on device→platform hop"
                                    );
                                }
                            }
                        } else {
                            last_h264_seq.take(); // seq-less producer or session reset
                        }
                        state.set_h264(crate::state::H264Sample {
                            ts_ns: ts,
                            pts_ns: pts,
                            key,
                            w,
                            h,
                            seq,
                            nalu: Arc::new(nalu),
                        });
                        }
                        // consumed likewise — h264 envelopes are not
                        // TrackFrames either; same RS-6 fallthrough
                        // pollution (whether ingested or skipped as
                        // no-viewer, the message is fully handled here).
                        continue;
                    } else if serde_json::from_str::<serde_json::Value>(&txt)
                        .ok()
                        .and_then(|v| v["topic"].as_str().map(|t| t == "gym/preview"))
                        .unwrap_or(false)
                    {
                        // A gym/preview that didn't decode (bad base64 / not a
                        // JPEG) is corruption, not a filter miss — count it.
                        ingest_dbg().lock().parse_fail += 1;
                    }
                    // latency decomposition on track envelopes (EMA, cheap)
                    if let Ok(ev) = serde_json::from_str::<serde_json::Value>(&txt) {
                        if ev["topic"].as_str() == Some("gym/track") {
                            let env_ts = ev["timestamp_ns"].as_u64().unwrap_or(0);
                            if env_ts > 0 {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default().as_nanos() as u64;
                                let mut g = ingest_dbg().lock();
                                let a = 0.1;
                                let cam = (env_ts.saturating_sub(frame_ts_hint(&txt))) as f64 / 1e6;
                                let bus = now.saturating_sub(env_ts) as f64 / 1e6;
                                if cam >= 0.0 && cam < 30_000.0 {
                                    g.gap_cam_ms = if g.gap_n == 0 { cam } else { g.gap_cam_ms * (1.0 - a) + cam * a };
                                }
                                if bus >= 0.0 && bus < 30_000.0 {
                                    g.gap_bus_ms = if g.gap_n == 0 { bus } else { g.gap_bus_ms * (1.0 - a) + bus * a };
                                }
                                g.gap_n += 1;
                            }
                        }
                    }
                    match parse_event(&txt) {
                        Some(frame) => {
                            ingest_dbg().lock().parsed_ok += 1;
                            // py-side id snapshot for the shadow parity
                            // report (bbox center — same metric as the
                            // detect-side snapshot)
                            {
                                let assigns = frame
                                    .tracks
                                    .iter()
                                    .map(|t| {
                                        (
                                            t.bbox.x + t.bbox.w / 2.0,
                                            t.bbox.y + t.bbox.h / 2.0,
                                            t.track_id,
                                        )
                                    })
                                    .collect();
                                crate::shadow::shadow()
                                    .lock()
                                    .1
                                    .push_py(crate::shadow::IdSnap {
                                        ts_ns: frame.ts_ns,
                                        assigns,
                                    });
                            }
                            // workout pipeline: zones + members fresh per frame
                            // (small tables; keeps set_roi_zones / member edits
                            // live without a cache-invalidation dance)
                            let zones = db.list_zones().unwrap_or_default();
                            let members = db.list_members().unwrap_or_default();
                            // EXCLUSION zones (mirrors / no-go areas): tracks
                            // inside them are reflections or noise — dropped
                            // HERE, before any consumer, so live state, trails,
                            // crossings, heatmap, occupancy and enrollment all
                            // stay clean. Foot first, bbox center as fallback
                            // (mirrors the occupancy rule).
                            let excl: Vec<&crate::db::Zone> = zones
                                .iter()
                                .filter(|z| z.enabled && z.equipment_type == "exclusion")
                                .collect();
                            let frame = if excl.is_empty() {
                                frame
                            } else {
                                let in_excl = |x: f32, y: f32| {
                                    excl.iter().any(|z| {
                                        crate::geo::point_in_polygon(x, y, &z.polygon)
                                    })
                                };
                                let mut f = frame;
                                f.tracks.retain(|t| {
                                    let (cx, cy) = (
                                        t.bbox.x + t.bbox.w / 2.0,
                                        t.bbox.y + t.bbox.h / 2.0,
                                    );
                                    !(in_excl(t.foot.x, t.foot.y) || in_excl(cx, cy))
                                });
                                f
                            };
                            // exclusion areas are filters, never equipment
                            let excl_owned: Vec<crate::db::Zone> =
                                excl.iter().map(|z| (*z).clone()).collect();
                            let active: Vec<crate::db::Zone> = zones
                                .into_iter()
                                .filter(|z| z.equipment_type != "exclusion")
                                .collect();
                            state.apply_frame_filtered(&frame, &excl_owned);
                            analytics.on_frame(&frame);
                            analytics.on_workout_frame(
                                &frame, &active, &members, &cfg.identity,
                                cfg.roi.dwell_debounce_sec);
                        }
                        None => {
                            let topic_is_gym = serde_json::from_str::<serde_json::Value>(&txt)
                                .ok().and_then(|v| v["topic"].as_str().map(|t| t.starts_with("gym/")))
                                .unwrap_or(false);
                            if topic_is_gym { ingest_dbg().lock().parse_fail += 1; }
                        }
                    }
                }
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                    last_alive = std::time::Instant::now();
                }
                Some(Ok(Message::Close(_))) => {
                    return Disconnect::Error("server closed connection".into());
                }
                Some(Ok(_)) => {
                    // Binary / other frames: the NE503 publishes JSON as text,
                    // so ignore anything else.
                }
                Some(Err(e)) => {
                    return Disconnect::Error(format!("ws read error: {e}"));
                }
                None => {
                    return Disconnect::Error("ws stream ended".into());
                }
            }
        }
    }
}

/// Resolves once `stop` is set. Polls every 200ms — cheap, and bounds the
/// shutdown latency of the read loop's `tokio::select!`.
async fn await_stop(stop: &Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Sleep for `secs`, but bail early (return) as soon as `stop` is set, so
/// reconnect backoff never blocks shutdown.
async fn sleep_with_stop(secs: u64, stop: &Arc<AtomicBool>) {
    let mut waited = 0u64;
    while waited < secs {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let step = 1u64.min(secs - waited);
        tokio::time::sleep(Duration::from_secs(step)).await;
        waited += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filters_and_decodes() {
        let good = serde_json::json!({"topic":"gym/track","payload":{
            "device_id":"d","frame_seq":1,"ts_ns":0,
            "tracks":[{"track_id":3,"bbox":{"x":0,"y":0,"w":0,"h":0},"foot":{"x":0,"y":0},"pose":null,"face":null}]}})
            .to_string();
        assert!(parse_event(&good).is_some());
        let other = r#"{"topic":"device/temp","payload":{"x":1}}"#;
        assert!(parse_event(other).is_none());
    }

    #[test]
    fn parse_accepts_gym_subtopic() {
        // Any gym/ prefix passes the client-side filter (e.g. gym/test probes).
        let probe = serde_json::json!({"topic":"gym/test","payload":{
            "device_id":"d","frame_seq":9,"ts_ns":1,"tracks":[]}})
        .to_string();
        let f = parse_event(&probe).expect("gym/test should pass the topic filter");
        assert_eq!(f.frame_seq, 9);
        assert!(f.tracks.is_empty());
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(parse_event("not json").is_none());
        // no topic
        assert!(parse_event(r#"{"payload":{}}"#).is_none());
        // gym/ topic but malformed payload (not a valid TrackFrame)
        assert!(parse_event(r#"{"topic":"gym/track","payload":{"nope":1}}"#).is_none());
        // non-gym topic with valid payload shape still dropped
        assert!(parse_event(
            r#"{"topic":"other/track","payload":{"device_id":"d","frame_seq":1,"ts_ns":0,"tracks":[]}}"#
        )
        .is_none());
    }

    #[test]
    fn parse_real_device_envelope_double_encoded_payload() {
        // Exact shape the NE503 delivers over WSS: `payload` is a JSON-encoded
        // STRING (double-encoded), plus the event_id/payload_type/timestamp_ns
        // wrapper fields. This locks in the on-the-wire contract.
        let inner = serde_json::json!({
            "device_id": "ne503-001", "frame_seq": 4242, "ts_ns": 0u64,
            "tracks": [{"track_id": 4242,
                "bbox": {"x":0.1,"y":0.2,"w":0.3,"h":0.4},
                "foot": {"x":0.25,"y":0.6}, "pose": null, "face": null}]
        })
        .to_string();
        let envelope = serde_json::json!({
            "event_id": "evt-1784085237439-901369",
            "payload": inner,                 // <-- STRING, not object
            "payload_type": "json",
            "source": "",
            "timestamp_ns": 1784085237437820084u64,
            "topic": "gym/test"
        })
        .to_string();
        let f = parse_event(&envelope).expect("real device envelope must decode");
        assert_eq!(f.device_id, "ne503-001");
        assert_eq!(f.frame_seq, 4242);
        assert_eq!(f.tracks.len(), 1);
        assert_eq!(f.tracks[0].track_id, 4242);
    }

    #[test]
    fn parse_preview_decodes_b64_and_returns_jpeg_bytes() {
        let jpeg = [0xFFu8, 0xD8, 0xE0, 0x11];
        let b64 = crate::frame::encode_b64(&jpeg);
        let env = serde_json::json!({
            "topic": "gym/preview",
            "payload": {"ts_ns": 42u64, "img_b64": b64}
        })
        .to_string();
        let (ts, img) = parse_preview(&env).expect("valid preview must decode");
        assert_eq!(ts, 42);
        assert_eq!(img, jpeg.to_vec());
    }

    #[test]
    fn parse_preview_rejects_bad_payloads() {
        // Not a gym/preview topic
        assert!(parse_preview(r#"{"topic":"gym/track","payload":{}}"#).is_none());
        // Preview topic but img_b64 is not a JPEG (SOI gate)
        let not_jpeg = crate::frame::encode_b64(b"plain text");
        let env = serde_json::json!({
            "topic": "gym/preview",
            "payload": {"ts_ns": 1u64, "img_b64": not_jpeg}
        })
        .to_string();
        assert!(parse_preview(&env).is_none());
        // Missing ts_ns
        let env = serde_json::json!({
            "topic": "gym/preview",
            "payload": {"img_b64": crate::frame::encode_b64(&[0xFF, 0xD8, 0x00])}
        })
        .to_string();
        assert!(parse_preview(&env).is_none());
    }

    #[test]
    fn parse_preview_h264_roundtrip_and_rejects() {
        let nalu: Vec<u8> = vec![0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f, 0xab];
        let env = serde_json::json!({
            "topic": "gym/preview_h264",
            "payload": {"ts_ns": 7u64, "pts_ns": 70u64, "key": true,
                        "w": 1280u64, "h": 720u64, "seq": 42u64,
                        "nalu_b64": crate::frame::encode_b64(&nalu)}
        })
        .to_string();
        let (ts, pts, key, w, h, sq, n) =
            parse_preview_h264(&env).expect("valid h264 sample must decode");
        assert_eq!((ts, pts, key, w, h, sq), (7, 70, true, 1280, 720, 42));
        assert_eq!(n, nalu);

        // non-Annex-B bytes → rejected
        let bad = serde_json::json!({
            "topic": "gym/preview_h264",
            "payload": {"ts_ns": 1u64, "nalu_b64": crate::frame::encode_b64(b"junk")}
        })
        .to_string();
        assert!(parse_preview_h264(&bad).is_none());
        // other topics → rejected
        assert!(parse_preview_h264(r#"{"topic":"gym/preview","payload":{}}"#).is_none());
    }

    #[test]
    fn build_url_encodes_token() {
        let u = build_ws_url("wss://h/api/v1/events/stream", "Bearer abc def");
        // spaces and the rest of "Bearer abc def" are percent-encoded so the
        // value survives intact in a query string.
        assert_eq!(u, "wss://h/api/v1/events/stream?token=Bearer%20abc%20def");
    }

    /// LIVE integration test against the real NE503 at 192.168.93.200.
    ///
    /// Proves the end-to-end WSS path: TLS-skip connect, token auth via the
    /// `?token=` query param, and receipt of a `gym/` event we publish via the
    /// REST event bus. We publish a `gym/test` frame, then assert the
    /// `LiveState` mirror received it within a short window.
    ///
    /// Run manually:
    ///   cargo test -p gym-tracker ingest::tests::live_wss_receives_gym_event -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_wss_receives_gym_event() {
        let raw = r#"{"device":{"host":"192.168.93.200","username":"admin","password":"password","tls_insecure":true},"device_id":"ne503-001","ingest":{"topic":"gym/track","publish_hz":8,"track_ttl_sec":30,"reconnect_backoff_sec":[1,2,5,10,30]},"identity":{"match_threshold":0.55,"auto_capture_unknown":true,"unknown_prefix":"未知会员"},"roi":{"dwell_debounce_sec":3,"hysteresis":true},"data_dir":"/tmp/gym"}"#;
        let cfg = Config::parse(raw).expect("config parse");

        // Login to get the token used for the REST publish Authorization
        // header; the ingest loop resolves its own WS token from this client.
        let client = Arc::new(crate::ne503::Ne503Client::new(&cfg));
        client.login().expect("login should succeed");
        tracing::info!("[live] login ok"); // token redacted from logs

        // Fresh live-state mirror + spawn the ingest subscriber.
        let state = Arc::new(LiveState::new(30));
        let db = Arc::new(crate::db::Db::open(":memory:").expect("in-memory db"));
        let analytics = Arc::new(crate::analytics::Analytics::new(&db));
        let handle = spawn(
            cfg.clone(),
            state.clone(),
            analytics,
            db.clone(),
            client.clone(),
        );

        // Give the WS subscriber a moment to connect + run its backoff cycle.
        std::thread::sleep(Duration::from_secs(2));

        // Publish a gym/test event carrying a valid TrackFrame via the REST
        // event bus. The device fans this out to all WS subscribers, so our
        // ingest thread should receive it and apply it to the mirror.
        let probe = serde_json::json!({
            "topic": "gym/test",
            "payload": {
                "device_id": "ne503-001",
                "frame_seq": 4242,
                "ts_ns": 0u64,
                "tracks": [{
                    "track_id": 4242,
                    "bbox": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4},
                    "foot": {"x": 0.25, "y": 0.6},
                    "pose": null,
                    "face": null
                }]
            }
        });
        let pub_url = format!("{}/api/v1/events/publish", cfg.rest_base());
        eprintln!("[live] publishing probe event to {pub_url}");
        // The device's self-signed cert means we must publish via a TLS-skipping
        // ureq agent too — a bare ureq::post() uses webpki roots and is rejected.
        let token = client
            .token_string()
            .expect("token should be set after login");
        let agent = ureq::AgentBuilder::new()
            .tls_config(Arc::new(crate::tls::insecure_client_config()))
            .timeout(Duration::from_secs(5))
            .build();
        let resp = agent
            .post(&pub_url)
            .set("Authorization", &token)
            .send_json(probe);
        match &resp {
            Ok(r) => eprintln!("[live] publish status = {}", r.status()),
            Err(e) => eprintln!("[live] publish error = {e}"),
        }
        let _ = resp;

        // Wait for the WS subscriber to receive + apply the frame.
        let mut received = false;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(200));
            for t in state.snapshot() {
                if t.track_id == 4242 {
                    received = true;
                    break;
                }
            }
            if received {
                break;
            }
        }

        handle.stop();
        std::thread::sleep(Duration::from_millis(400));

        assert!(
            received,
            "did not receive the gym/test probe in LiveState within 4s — \
             check that wss connected (see '[live]' logs above) and the REST \
             publish succeeded"
        );
        eprintln!("[live] SUCCESS — gym/test probe landed in LiveState");
    }

    #[test]
    fn captured_g200_track_parses() {
        let raw = include_str!("../tests/g200-track-sample.json");
        let f = parse_event(raw).expect("captured .200 gym/track must parse");
        assert!(!f.tracks.is_empty());
    }
}
