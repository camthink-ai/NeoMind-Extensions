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

use parking_lot::RwLock;

use crate::types::{Track, TrackFrame};

#[derive(Debug, Clone)]
struct Entry {
    track: Track,
    last_seen: Instant,
}

pub struct LiveState {
    ttl: Duration,
    inner: RwLock<HashMap<i64, Entry>>,
}

impl LiveState {
    pub fn new(ttl_sec: u32) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_sec as u64),
            inner: Default::default(),
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
}
