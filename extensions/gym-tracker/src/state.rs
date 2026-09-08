// state.rs
//! Live track-state mirror + TTL eviction.
//!
//! The NE503 device-app is the authoritative source of `track_id` assignment.
//! This mirror keeps the latest `Track` per `track_id`, updated from each
//! incoming `TrackFrame`. Tracks that have not been seen within `ttl` are
//! evicted; P2 will turn evictions into session-close events.
//!
//! Thread-safe via `parking_lot::RwLock` (sync, NOT tokio) — matches repo
//! convention. Intended to be shared via `Arc<LiveState>` between the ingest
//! thread and the metric-poll thread.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, RwLock};

use crate::types::{FaceBox, Track, TrackFrame};

#[derive(Debug, Clone)]
struct Entry {
    track: Track,
    last_seen: Instant,
}

/// One hardware-encoded H.264 access unit relayed from the device's
/// EncodedPublisher (`gym/preview_h264`). `nalu` is Annex-B bytes.
#[derive(Debug, Clone)]
pub struct H264Sample {
    /// Device wall clock at relay time — the jitter-buffer key (same clock
    /// as the JPEG preview and TrackFrame ts_ns).
    pub ts_ns: u64,
    /// Encoder presentation timestamp (for the browser VideoDecoder).
    pub pts_ns: u64,
    pub key: bool,
    pub w: u32,
    pub h: u32,
    /// Relay-side monotonic sequence (per relay socket session; 0 when the
    /// producer doesn't send one). Lets every hop DETECT loss — dropping a
    /// differential frame corrupts the decode until the next keyframe.
    pub seq: u64,
    pub nalu: Arc<Vec<u8>>,
}

/// Push-pipeline diagnostics (readable via the `get_push_diag` command).
/// The tracing pipeline is not wired through the isolated runner, so
/// warns are LOST — these counters are the only reliable loss instrument.
#[derive(Default)]
pub struct PushDiag {
    /// gym/preview_h264 events accepted from the device event bus.
    pub ingested: std::sync::atomic::AtomicU64,
    /// Device-seq gaps detected at ingest (frames lost bus→extension).
    pub ingest_gaps: std::sync::atomic::AtomicU64,
    /// Frames dropped by the internal 120-deep queue (push starved).
    pub queue_overflow: std::sync::atomic::AtomicU64,
    /// Frames handed to send_push_output.
    pub pushed: std::sync::atomic::AtomicU64,
}

pub static PUSH_DIAG: std::sync::OnceLock<PushDiag> = std::sync::OnceLock::new();

pub fn push_diag() -> &'static PushDiag {
    PUSH_DIAG.get_or_init(PushDiag::default)
}

pub use std::sync::atomic::Ordering as AtomicOrd;

pub struct LiveState {
    ttl: Duration,
    /// A track absent from the latest frame survives this long before
    /// being pruned (device tracker drops tracks after ~3 s without a
    /// detection and stops publishing them immediately).
    refresh_grace: Duration,
    inner: RwLock<HashMap<i64, Entry>>,
    /// Latest frame-level face boxes (P2). Refreshed per frame; faces are
    /// detected every Nth frame on the device and reused in between, so a
    /// plain last-value cache matches the producer's own semantics.
    faces: RwLock<Vec<FaceBox>>,
    faces_seen: RwLock<Instant>,
    /// Latest producer frame preview (base64 JPEG) + the tracks OF THAT
    /// SAME FRAME — the Monitor renders both from one source, so image and
    /// overlay can never desync (two-clock problem eliminated by design).
    frame_img: RwLock<Option<(String, Vec<Track>, Vec<FaceBox>)>>,
    /// Display-rate preview stream (`gym/preview`): latest (ts_ns, JPEG
    /// bytes) behind an `Arc` — the push thread wakes per frame and must
    /// never copy ~100 KB under the lock.
    preview: RwLock<Option<(u64, Arc<Vec<u8>>)>>,
    /// Hardware H.264 relay (`gym/preview_h264`): bounded FIFO of access
    /// units. H.264 is differential — handing the push thread only the
    /// LATEST sample (the old design, fine for JPEG) silently dropped
    /// every frame that landed between wakes and corrupted the decode.
    /// Shares the preview condvar.
    h264: Mutex<std::collections::VecDeque<H264Sample>>,
    /// Per-track consecutive-unknown counters gating auto-enrollment
    /// (see commands.rs persistence gate).
    unknown_streaks: RwLock<HashMap<i64, u32>>,
    /// ts_ns of the TrackFrame the latest tracks came from — the TRUE
    /// capture time of those positions (they lag the preview stream by the
    /// inference latency; keying overlay history by this keeps A/V sync).
    tracks_ts: RwLock<u64>,
    /// Wakes the push thread when a new preview frame lands.
    preview_wait: Mutex<()>,
    preview_cv: Condvar,
    /// ts_ns-keyed track history (~3 s) for timestamp-exact interpolation.
    track_hist: RwLock<std::collections::VecDeque<(u64, Vec<Track>)>>,
}

impl LiveState {
    pub fn new(ttl_sec: u32) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_sec as u64),
            refresh_grace: Duration::from_millis(2500),
            inner: Default::default(),
            faces: Default::default(),
            frame_img: Default::default(),
            preview: Default::default(),
            h264: Mutex::new(std::collections::VecDeque::new()),
            tracks_ts: RwLock::new(0),
            unknown_streaks: Default::default(),
            preview_wait: Mutex::new(()),
            preview_cv: Condvar::new(),
            track_hist: Default::default(),
            faces_seen: RwLock::new(Instant::now() - Duration::from_secs(3600)),
        }
    }

    /// Upsert all tracks in `f` with `last_seen = now`. Frames carry the
    /// producer's FULL live set, so tracks missing from the latest frame are
    /// authoritative-departed: they are pruned once unseen for longer than
    /// the refresh grace (short occlusions — a beat where the device's own
    /// tracker coasts without publishing — survive inside the grace).
    /// Drop tracks AND faces inside exclusion zones (equipment_type
    /// 'exclusion'): mirror reflections are not people, so nothing about
    /// them — boxes, keypoints, mosaics, embeddings — should survive.
    /// called by apply_frame when the zone set is supplied.
    pub fn apply_frame_filtered(&self, f: &TrackFrame, excl: &[crate::db::Zone]) {
        if excl.is_empty() {
            self.apply_frame(f);
            return;
        }
        let in_excl = |x: f32, y: f32| {
            excl.iter()
                .any(|z| crate::geo::point_in_polygon(x, y, &z.polygon))
        };
        let mut f2 = f.clone();
        f2.tracks.retain(|t| {
            let (cx, cy) = (t.bbox.x + t.bbox.w / 2.0, t.bbox.y + t.bbox.h / 2.0);
            !(in_excl(t.foot.x, t.foot.y) || in_excl(cx, cy))
        });
        f2.faces.retain(|fc| {
            let b = &fc.bbox;
            let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
            // face box center + lower-quarter point (covers tall boxes)
            !(in_excl(cx, cy) || in_excl(cx, b.y + b.h * 0.85))
        });
        self.apply_frame(&f2);
    }

    pub fn apply_frame(&self, f: &TrackFrame) {
        let now = Instant::now();
        {
            let mut g = self.inner.write();
            for t in &f.tracks {
                g.insert(
                    t.track_id,
                    Entry {
                        track: t.clone(),
                        last_seen: now,
                    },
                );
            }
            // Upsert-only with the 30 s TTL kept ghosts on screen for the
            // whole TTL after a person left (boxes + keypoints frozen on
            // the picture, tracking "lost" but never cleaned).
            let grace = self.refresh_grace;
            g.retain(|_, e| now.duration_since(e.last_seen) < grace);
        }
        let tracks_now: Vec<Track> = f.tracks.clone();
        *self.faces.write() = f.faces.clone();
        *self.faces_seen.write() = now;
        *self.tracks_ts.write() = f.ts_ns;
        if let Some(img) = f.img_b64.as_ref() {
            *self.frame_img.write() = Some((img.clone(), tracks_now, f.faces.clone()));
        }
        // ts-keyed history for timestamp-exact overlay interpolation
        {
            let mut h = self.track_hist.write();
            h.push_back((f.ts_ns, f.tracks.clone()));
            // saturating_sub: a stale/out-of-order ts (camera clock jump) must
            // not underflow — it panics debug builds and corrupts the window.
            while h.len() > 1
                && f.ts_ns
                    .saturating_sub(h.front().map(|(t, _)| *t).unwrap_or(0))
                    > 3_000_000_000
            {
                h.pop_front();
            }
        }
    }

    /// Latest display preview (ts_ns, JPEG bytes) from `gym/preview`.
    pub fn set_preview(&self, ts_ns: u64, img: Arc<Vec<u8>>) {
        *self.preview.write() = Some((ts_ns, img));
        let _guard = self.preview_wait.lock();
        self.preview_cv.notify_all();
    }

    /// Block until a new preview arrives or `timeout` elapses; returns the
    /// latest (ts, JPEG bytes) at wake time (Arc clone — no image copy).
    pub fn wait_preview(&self, timeout: Duration) -> Option<(u64, Arc<Vec<u8>>)> {
        let mut guard = self.preview_wait.lock();
        let _ = self.preview_cv.wait_for(&mut guard, timeout);
        self.preview.read().clone()
    }

    /// Latest tracks (of the most recent TrackFrame).
    pub fn snapshot_tracks(&self) -> Vec<Track> {
        self.snapshot()
    }

    /// Auto-enrollment persistence counters (track_id → consecutive unknown).
    pub fn unknown_streaks_write(&self) -> parking_lot::RwLockWriteGuard<'_, HashMap<i64, u32>> {
        self.unknown_streaks.write()
    }

    /// ts_ns of the TrackFrame the latest tracks came from.
    pub fn snapshot_tracks_ts(&self) -> u64 {
        *self.tracks_ts.read()
    }

    /// Latest display preview from `gym/preview` (JPEG bytes).
    pub fn snapshot_preview(&self) -> Option<(u64, Arc<Vec<u8>>)> {
        self.preview.read().clone()
    }

    /// Enqueue an H.264 access unit and wake the push thread (shares the
    /// preview condvar). The queue is bounded; overflow only sheds under a
    /// pathological producer burst and is counted loudly — silence here
    /// used to corrupt downstream decodes invisibly.
    pub fn set_h264(&self, sample: H264Sample) {
        {
            let mut q = self.h264.lock();
            q.push_back(sample);
            while q.len() > 120 {
                q.pop_front();
                tracing::warn!("h264 queue overflow — access unit dropped");
                push_diag().queue_overflow.fetch_add(1, AtomicOrd::Relaxed);
            }
        }
        let _guard = self.preview_wait.lock();
        self.preview_cv.notify_all();
    }

    /// Drain all queued H.264 access units (oldest first, lossless).
    pub fn drain_h264(&self) -> Vec<H264Sample> {
        self.h264.lock().drain(..).collect()
    }

    /// All queued access units with `seq > after_seq`, oldest first —
    /// PER-SESSION cursor replay. The queue is a global singleton while
    /// push threads are per-session: draining (the old semantics) let two
    /// concurrent sessions SPLIT the stream (~15 fps each with 50%
    /// "loss"); cursor replay hands every session the full stream (the
    /// sample payload is an Arc — cloning per session is free). A cursor
    /// older than the queue head jumps to the head (the browser's
    /// device-seq gap check resyncs its decoder on the next keyframe).
    pub fn h264_since(&self, after_seq: u64) -> Vec<H264Sample> {
        let q = self.h264.lock();
        let mut out = Vec::new();
        for s in q.iter() {
            if s.seq > after_seq {
                out.push(s.clone());
            }
        }
        out
    }

    /// Tracks interpolated keyframes around `ts_ns` (for frontend or push).
    pub fn tracks_near(&self, ts_ns: u64) -> Vec<(u64, Vec<Track>)> {
        self.track_hist.read().iter().cloned().collect()
    }

    /// Latest preview bundle: (img_b64, tracks at that frame, faces).
    pub fn snapshot_frame(&self) -> Option<(String, Vec<Track>, Vec<FaceBox>)> {
        self.frame_img.read().clone()
    }

    /// Evict tracks not seen within `ttl`. Returns the expired `track_id`s
    /// (P2 session-close hook; unused in P1).
    pub fn evict_expired(&self) -> Vec<i64> {
        let mut g = self.inner.write();
        let now = Instant::now();
        let expired: Vec<i64> = g
            .iter()
            .filter(|(_, e)| now.duration_since(e.last_seen) > self.ttl)
            .map(|(id, _)| *id)
            .collect();
        for id in &expired {
            g.remove(id);
        }
        expired
    }

    pub fn present_count(&self) -> usize {
        self.inner.read().len()
    }

    pub fn snapshot(&self) -> Vec<Track> {
        self.inner
            .read()
            .values()
            .map(|e| e.track.clone())
            .collect()
    }

    /// Latest frame-level face boxes, `None` when the last face-bearing
    /// payload is older than the track TTL (producer stopped sending —
    /// don't mosaic against stale geometry).
    pub fn snapshot_faces(&self) -> Option<Vec<FaceBox>> {
        let seen = *self.faces_seen.read();
        if seen.elapsed() > self.ttl {
            return None;
        }
        Some(self.faces.read().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Bbox, Point};

    fn frame(tid: i64) -> TrackFrame {
        TrackFrame {
            device_id: "d".into(),
            frame_seq: 1,
            ts_ns: 0,
            tracks: vec![Track {
                track_id: tid,
                ts: None,
                bbox: Bbox {
                    x: 0.,
                    y: 0.,
                    w: 0.,
                    h: 0.,
                },
                foot: Point { x: 0., y: 0. },
                pose: None,
                face: None,
                vel: None,
                ex: None,
            }],
            faces: vec![],
            img_b64: None,
        }
    }

    #[test]
    fn apply_then_evict() {
        let s = LiveState::new(0); // ttl 0 -> immediate expiry
        s.apply_frame(&frame(1));
        assert_eq!(s.present_count(), 1);
        s.evict_expired();
        assert_eq!(s.present_count(), 0);
        assert!(s.evict_expired().is_empty());
    }

    #[test]
    fn departed_tracks_pruned_after_refresh_grace() {
        let mut s = LiveState::new(3600); // ttl long — grace is what cleans
        s.refresh_grace = Duration::from_millis(30);
        s.apply_frame(&frame(1));
        assert_eq!(s.present_count(), 1);
        // still inside the grace: a frame that omits tid1 keeps it (plus tid2)
        s.apply_frame(&frame(2));
        assert_eq!(s.present_count(), 2, "short omission survives the grace");
        std::thread::sleep(Duration::from_millis(60));
        let mut empty = frame(9);
        empty.tracks = vec![];
        s.apply_frame(&empty);
        assert_eq!(s.present_count(), 0, "departed tracks pruned, no ghost");
    }

    #[test]
    fn faces_mirror_and_ttl() {
        use crate::types::FaceBox;
        let s = LiveState::new(3600);
        // Empty producer frames (no faces field / detector off) mirror as
        // an empty-but-fresh list.
        s.apply_frame(&frame(1));
        assert_eq!(s.snapshot_faces(), Some(vec![]));

        let mut f = frame(2);
        f.faces = vec![FaceBox {
            bbox: Bbox {
                x: 0.1,
                y: 0.1,
                w: 0.2,
                h: 0.2,
            },
            det: 0.9,
            emb: None,
        }];
        s.apply_frame(&f);
        assert_eq!(s.snapshot_faces().unwrap().len(), 1);

        // ttl 0 -> any age exceeds it -> stale faces drop to None.
        let old = LiveState::new(0);
        old.apply_frame(&f);
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(old.snapshot_faces(), None);
    }
}
