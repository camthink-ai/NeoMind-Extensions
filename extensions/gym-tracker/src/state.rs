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
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex, RwLock};

use crate::types::{FaceBox, Track, TrackFrame};

#[derive(Debug, Clone)]
struct Entry {
    track: Track,
    last_seen: Instant,
}

pub struct LiveState {
    ttl: Duration,
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
    /// Display-rate preview stream (`gym/preview`): latest (ts_ns, img).
    preview: RwLock<Option<(u64, String)>>,
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
            inner: Default::default(),
            faces: Default::default(),
            frame_img: Default::default(),
            preview: Default::default(),
            preview_wait: Mutex::new(()),
            preview_cv: Condvar::new(),
            track_hist: Default::default(),
            faces_seen: RwLock::new(
                Instant::now() - Duration::from_secs(3600),
            ),
        }
    }

    /// Upsert all tracks in `f` with `last_seen = now`.
    pub fn apply_frame(&self, f: &TrackFrame) {
        let mut g = self.inner.write();
        let now = Instant::now();
        for t in &f.tracks {
            g.insert(
                t.track_id,
                Entry {
                    track: t.clone(),
                    last_seen: now,
                },
            );
        }
        let tracks_now: Vec<Track> = f.tracks.clone();
        drop(g);
        *self.faces.write() = f.faces.clone();
        *self.faces_seen.write() = now;
        if let Some(img) = f.img_b64.as_ref() {
            *self.frame_img.write() =
                Some((img.clone(), tracks_now, f.faces.clone()));
        }
        // ts-keyed history for timestamp-exact overlay interpolation
        {
            let mut h = self.track_hist.write();
            h.push_back((f.ts_ns, f.tracks.clone()));
            while h.len() > 1 && f.ts_ns - h.front().map(|(t, _)| *t).unwrap_or(0) > 3_000_000_000 {
                h.pop_front();
            }
        }
    }

    /// Latest display preview (ts_ns, img_b64) from `gym/preview`.
    pub fn set_preview(&self, ts_ns: u64, img: String) {
        *self.preview.write() = Some((ts_ns, img));
        let _guard = self.preview_wait.lock();
        self.preview_cv.notify_all();
    }

    /// Block until a new preview arrives or `timeout` elapses; returns the
    /// latest (ts, img) at wake time.
    pub fn wait_preview(&self, timeout: Duration) -> Option<(u64, String)> {
        let mut guard = self.preview_wait.lock();
        let _ = self.preview_cv.wait_for(&mut guard, timeout);
        self.preview.read().clone()
    }

    /// Latest tracks (of the most recent TrackFrame).
    pub fn snapshot_tracks(&self) -> Vec<Track> {
        self.snapshot()
    }

    /// Latest display preview from `gym/preview`.
    pub fn snapshot_preview(&self) -> Option<(u64, String)> {
        self.preview.read().clone()
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
        self.inner.read().values().map(|e| e.track.clone()).collect()
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
                bbox: Bbox {
                    x: 0.,
                    y: 0.,
                    w: 0.,
                    h: 0.,
                },
                foot: Point { x: 0., y: 0. },
                pose: None,
                face: None,
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
    fn faces_mirror_and_ttl() {
        use crate::types::FaceBox;
        let s = LiveState::new(3600);
        // Empty producer frames (no faces field / detector off) mirror as
        // an empty-but-fresh list.
        s.apply_frame(&frame(1));
        assert_eq!(s.snapshot_faces(), Some(vec![]));

        let mut f = frame(2);
        f.faces = vec![FaceBox {
            bbox: Bbox { x: 0.1, y: 0.1, w: 0.2, h: 0.2 },
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
