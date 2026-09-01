// analytics.rs — trails, line-crossing counting, and foot-position heatmap.
//
// Fed once per TrackFrame by the ingest thread (same cadence as
// LiveState::apply_frame). Owns three independent accumulators:
//
//   trails    — per-track rolling foot positions (for the Monitor's fading
//               track tails). Capped; stale entries reaped by frame timestamp.
//   crossings — per-line in/out counters. Direction = sign of the cross
//               product of (movement × line): positive counts as `in`
//               (crossing towards the LEFT of a→b), negative as `out`.
//               Hysteresis: after a counted crossing the (track, line) pair
//               disarms until the foot leaves a 2%-of-frame band around the
//               line, so loitering on the line doesn't machine-gun counts.
//               Counters reset on natural-day rollover (local device time).
//   heatmap   — 64×36 bucket histogram of foot positions for today,
//               persisted (JSON) to SQLite on a lazy cadence and reloaded
//               at configure time.
//
// Lines are configured via set_lines (full-replace) and persisted like zones.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Datelike, Local};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::types::TrackFrame;

const TRAIL_MAX_POINTS: usize = 120;
const TRAIL_MAX_AGE_SEC: u64 = 180;
const HEATMAP_COLS: usize = 64;
const HEATMAP_ROWS: usize = 36;
/// Persist the heatmap after at least this many dirty frames.
const HEATMAP_SAVE_EVERY: u64 = 250;
/// Re-arm distance for crossing hysteresis, in normalized units.
const CROSS_REARM_DIST: f32 = 0.02;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossLine {
    pub id: String,
    pub name: String,
    pub a: (f32, f32),
    pub b: (f32, f32),
}

#[derive(Debug, Clone, Serialize)]
pub struct LineStats {
    pub line_id: String,
    pub name: String,
    pub in_count: u64,
    pub out_count: u64,
    /// Natural day the counters belong to (days-from-CE, local time).
    pub day: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrailPoint {
    pub x: f32,
    pub y: f32,
}

struct LineState {
    line: CrossLine,
    in_count: u64,
    out_count: u64,
    day: i64,
    /// (track_id → armed). Absent = armed.
    armed: HashMap<i64, bool>,
}

struct TrackTrail {
    pts: VecDeque<TrailPoint>,
    last_ts_ns: u64,
    /// Previous foot position — the movement segment origin for crossings.
    prev: Option<(f32, f32)>,
}

struct Inner {
    lines: Vec<LineState>,
    trails: HashMap<i64, TrackTrail>,
    heat: Vec<u32>,
    heat_day: i64,
}

pub struct Analytics {
    inner: Mutex<Inner>,
    dirty_frames: AtomicU64,
}

impl Analytics {
    pub fn new(db: &Db) -> Self {
        let lines: Vec<LineState> = db
            .list_lines()
            .unwrap_or_default()
            .into_iter()
            .map(|l| LineState {
                line: l,
                in_count: 0,
                out_count: 0,
                day: today(),
                armed: Default::default(),
            })
            .collect();
        let (heat, heat_day) = db
            .load_heatmap(today())
            .unwrap_or_else(|| (vec![0u32; HEATMAP_COLS * HEATMAP_ROWS], today()));
        Self {
            inner: Mutex::new(Inner {
                lines,
                trails: Default::default(),
                heat,
                heat_day,
            }),
            dirty_frames: AtomicU64::new(0),
        }
    }

    /// Process one frame: trails, crossing tests, heatmap accumulation.
    pub fn on_frame(&self, f: &TrackFrame) {
        let day = today();
        let mut g = self.inner.lock();

        if g.heat_day != day {
            g.heat_day = day;
            g.heat = vec![0u32; HEATMAP_COLS * HEATMAP_ROWS];
        }
        for ls in g.lines.iter_mut() {
            if ls.day != day {
                ls.day = day;
                ls.in_count = 0;
                ls.out_count = 0;
                ls.armed.clear();
            }
        }

        for t in &f.tracks {
            let (fx, fy) = (t.foot.x, t.foot.y);

            // trail — scoped borrow: read out `prev` so `g.lines` can be
            // mutably borrowed below without overlapping the entry borrow.
            let prev = {
                let trail = g
                    .trails
                    .entry(t.track_id)
                    .or_insert_with(|| TrackTrail {
                        pts: VecDeque::with_capacity(TRAIL_MAX_POINTS),
                        last_ts_ns: f.ts_ns,
                        prev: None,
                    });
                trail.last_ts_ns = f.ts_ns;
                match trail.pts.back() {
                    Some(last) if (last.x - fx).abs() <= 1e-3 && (last.y - fy).abs() <= 1e-3 => {}
                    _ => trail.pts.push_back(TrailPoint { x: fx, y: fy }),
                }
                while trail.pts.len() > TRAIL_MAX_POINTS {
                    trail.pts.pop_front();
                }
                trail.prev
            };

            // crossings: movement segment prev→cur against each line
            if let Some((px, py)) = prev {
                for ls in g.lines.iter_mut() {
                    let (a, b) = (ls.line.a, ls.line.b);
                    // Re-arm FIRST using the current position: a traversal
                    // that ends outside the band counts even when it began
                    // disarmed (fast back-and-forth), while loitering inside
                    // the band stays suppressed.
                    if !*ls.armed.get(&t.track_id).unwrap_or(&true)
                        && point_line_dist(fx, fy, a, b) > CROSS_REARM_DIST
                    {
                        ls.armed.insert(t.track_id, true);
                    }
                    if *ls.armed.get(&t.track_id).unwrap_or(&true)
                        && segments_intersect((px, py), (fx, fy), a, b)
                    {
                        // sign of (movement × line direction)
                        let cross = (fx - px) * (b.1 - a.1) - (fy - py) * (b.0 - a.0);
                        if cross > 0.0 {
                            ls.in_count += 1;
                        } else {
                            ls.out_count += 1;
                        }
                        ls.armed.insert(t.track_id, false);
                    }
                }
            }
            if let Some(trail) = g.trails.get_mut(&t.track_id) {
                trail.prev = Some((fx, fy));
            }

            // heatmap
            let col = (fx.clamp(0.0, 0.999) * HEATMAP_COLS as f32) as usize;
            let row = (fy.clamp(0.0, 0.999) * HEATMAP_ROWS as f32) as usize;
            g.heat[row * HEATMAP_COLS + col] += 1;
        }

        // reap stale trails (frame timestamps are unix nanoseconds)
        let cutoff = f.ts_ns.saturating_sub(TRAIL_MAX_AGE_SEC * 1_000_000_000);
        g.trails.retain(|_, tr| tr.last_ts_ns >= cutoff);

        drop(g);
        self.dirty_frames.fetch_add(1, Ordering::Relaxed);
    }

    /// Persist the heatmap if enough dirty frames have accumulated.
    /// Called opportunistically from the command path (polls are frequent).
    pub fn maybe_save(&self, db: &Db) {
        let d = self.dirty_frames.swap(0, Ordering::Relaxed);
        if d >= HEATMAP_SAVE_EVERY {
            self.save_heatmap(db);
        }
    }

    pub fn save_heatmap(&self, db: &Db) {
        let g = self.inner.lock();
        let _ = db.save_heatmap(g.heat_day, &g.heat);
    }

    pub fn set_lines(&self, db: &Db, lines: Vec<CrossLine>) -> Result<(), String> {
        db.replace_lines(&lines).map_err(|e| e.to_string())?;
        let mut g = self.inner.lock();
        g.lines = lines
            .into_iter()
            .map(|l| LineState {
                line: l,
                in_count: 0,
                out_count: 0,
                day: today(),
                armed: Default::default(),
            })
            .collect();
        Ok(())
    }

    pub fn get_lines(&self) -> Vec<CrossLine> {
        self.inner.lock().lines.iter().map(|ls| ls.line.clone()).collect()
    }

    pub fn get_crossings(&self) -> Vec<LineStats> {
        let g = self.inner.lock();
        g.lines
            .iter()
            .map(|ls| LineStats {
                line_id: ls.line.id.clone(),
                name: ls.line.name.clone(),
                in_count: ls.in_count,
                out_count: ls.out_count,
                day: ls.day,
            })
            .collect()
    }

    /// Trails for all tracks, oldest first, capped per track.
    pub fn get_trails(&self, max_points: usize) -> HashMap<i64, Vec<TrailPoint>> {
        let g = self.inner.lock();
        g.trails
            .iter()
            .map(|(id, t)| {
                let skip = t.pts.len().saturating_sub(max_points);
                (*id, t.pts.iter().skip(skip).cloned().collect())
            })
            .collect()
    }

    /// Today's heatmap descriptor: {cols, rows, day, grid}.
    pub fn get_heatmap(&self) -> serde_json::Value {
        let g = self.inner.lock();
        serde_json::json!({
            "cols": HEATMAP_COLS,
            "rows": HEATMAP_ROWS,
            "day": g.heat_day,
            "grid": g.heat,
        })
    }
}

fn today() -> i64 {
    Local::now().date_naive().num_days_from_ce() as i64
}

fn orient(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Strict segment intersection (endpoints touching excluded).
fn segments_intersect(p1: (f32, f32), p2: (f32, f32), a: (f32, f32), b: (f32, f32)) -> bool {
    let d1 = orient(a.0, a.1, b.0, b.1, p1.0, p1.1);
    let d2 = orient(a.0, a.1, b.0, b.1, p2.0, p2.1);
    let d3 = orient(p1.0, p1.1, p2.0, p2.1, a.0, a.1);
    let d4 = orient(p1.0, p1.1, p2.0, p2.1, b.0, b.1);
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

fn point_line_dist(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return ((px - a.0).powi(2) + (py - a.1).powi(2)).sqrt();
    }
    let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Bbox, Point, Track};

    fn frame(ts_ns: u64, tracks: Vec<(i64, f32, f32)>) -> TrackFrame {
        TrackFrame {
            device_id: "d".into(),
            frame_seq: ts_ns,
            ts_ns,
            tracks: tracks
                .into_iter()
                .map(|(id, x, y)| Track {
                    track_id: id,
                    bbox: Bbox { x, y: 0.2, w: 0.1, h: 0.5 },
                    foot: Point { x, y },
                    pose: None,
                    face: None,
                })
                .collect(),
            faces: vec![],
        }
    }

    #[test]
    fn crossing_direction_and_hysteresis() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        // vertical line x=0.5, a=(0.5,0) b=(0.5,1): moving right-to-left across it
        a.set_lines(&db, vec![CrossLine {
            id: "l1".into(), name: "入口".into(),
            a: (0.5, 0.0), b: (0.5, 1.0),
        }])
        .unwrap();

        // left→right crossing: movement (+x), line dir (+y): cross = +dx*dy > 0 → in
        a.on_frame(&frame(1_000_000_000, vec![(1, 0.40, 0.5)]));
        a.on_frame(&frame(2_000_000_000, vec![(1, 0.60, 0.5)]));
        let s = &a.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (1, 0), "left→right = in");

        // loiter inside the band: shuttling 0.49↔0.51 stays suppressed
        a.on_frame(&frame(3_000_000_000, vec![(1, 0.49, 0.5)]));
        a.on_frame(&frame(4_000_000_000, vec![(1, 0.51, 0.5)]));
        a.on_frame(&frame(5_000_000_000, vec![(1, 0.49, 0.5)]));
        a.on_frame(&frame(6_000_000_000, vec![(1, 0.51, 0.5)])); // ends right of line
        let s = &a.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (1, 0), "hysteresis blocks band loiter");

        // stepping out through the line (right→left) is a real traversal → out
        a.on_frame(&frame(7_000_000_000, vec![(1, 0.30, 0.5)]));
        let s = &a.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (1, 1), "band exit counts as out");

        // walking away on the same side doesn't count
        a.on_frame(&frame(8_000_000_000, vec![(1, 0.20, 0.5)]));
        let s = &a.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (1, 1));

        // full re-traversal left→right→left
        a.on_frame(&frame(9_000_000_000, vec![(1, 0.60, 0.5)]));
        a.on_frame(&frame(10_000_000_000, vec![(1, 0.30, 0.5)]));
        let s = &a.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (2, 2));
    }

    #[test]
    fn no_crossing_when_parallel_or_away() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        a.set_lines(&db, vec![CrossLine {
            id: "l1".into(), name: "l".into(),
            a: (0.5, 0.0), b: (0.5, 1.0),
        }])
        .unwrap();
        // movement entirely on one side
        a.on_frame(&frame(1_000_000_000, vec![(1, 0.10, 0.3)]));
        a.on_frame(&frame(2_000_000_000, vec![(1, 0.20, 0.4)]));
        assert_eq!(a.get_crossings()[0].in_count + a.get_crossings()[0].out_count, 0);
    }

    #[test]
    fn trail_capped_and_reaped() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        let t0: u64 = 1_700_000_000_000_000_000;
        for i in 0..200 {
            a.on_frame(&frame(t0 + i * 100_000_000, vec![(1, 0.01 * (i % 100) as f32, 0.5)]));
        }
        assert_eq!(a.get_trails(999)[&1].len(), TRAIL_MAX_POINTS);
        // 200 frames * 0.1s = 20s << 180s so still alive; advance far beyond
        a.on_frame(&frame(t0 + 300 * 1_000_000_000, vec![(2, 0.5, 0.5)]));
        assert!(!a.get_trails(10).contains_key(&1), "stale trail reaped");
    }

    #[test]
    fn heatmap_accumulates_and_persists() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        for i in 0..10 {
            a.on_frame(&frame(1_000_000_000 + i, vec![(1, 0.5, 0.5)]));
        }
        let hm = a.get_heatmap();
        let grid = hm["grid"].as_array().unwrap();
        let total: u64 = grid.iter().map(|v| v.as_u64().unwrap_or(0)).sum();
        assert_eq!(total, 10);
        a.save_heatmap(&db);
        let a2 = Analytics::new(&db);
        let hm2 = a2.get_heatmap();
        let total2: u64 = hm2["grid"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap_or(0)).sum();
        assert_eq!(total2, 10, "heatmap round-trips through SQLite");
    }
}
