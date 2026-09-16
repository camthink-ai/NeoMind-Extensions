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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
/// One foot_log sample per track per this many seconds (query grain).
const FOOT_SAMPLE_SEC: u64 = 2;
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
    workouts: HashMap<i64, WorkoutTracker>,
    alerts: VecDeque<Alert>,
    /// alerts not yet flushed to alerts_log.
    alerts_unsaved: u32,
    /// per-track fall-rule state: (lying_since, alerted)
    lying: HashMap<i64, (f64, bool)>,
    /// per-(track,zone) long-occupancy alerted flag
    long_occ: HashMap<(i64, String), bool>,
    /// Pending foot_log samples (ts, track_id, x, y) awaiting the poll
    /// flush; throttled to one per track per FOOT_SAMPLE_SEC.
    pending_feet: Vec<(i64, i64, f32, f32)>,
    feet_last: HashMap<i64, u64>,
}

/// A safety/ops alert pushed to the Gym·Alerts card.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Alert {
    pub ts: f64,
    /// "fall" | "long_occupancy"
    pub kind: &'static str,
    /// "warn" | "info"
    pub level: &'static str,
    pub track_id: i64,
    pub message: String,
}

const ALERT_CAP: usize = 50;
/// torso lean beyond this reads as lying/fallen (degrees from vertical)
const LYING_TORSO_DEG: f32 = 55.0;
/// lying must persist this long before a fall alert (filters bench crunch
/// transitions and pose flicker)
const LYING_SUSTAIN_SEC: f64 = 8.0;
/// single zone occupied continuously this long → courtesy alert
const LONG_OCCUPANCY_SEC: f64 = 1800.0;
/// zones whose exercises legitimately involve lying — no fall alerts there
const LYING_OK_EXERCISES: &[&str] = &["crunch", "situp", "plank", "bench_press", "pushup"];

pub struct Analytics {
    inner: Mutex<Inner>,
    dirty_frames: AtomicU64,
    /// A crossing counter changed since the last maybe_save flush.
    dirty_crossings: AtomicBool,
    /// foot_log retention in seconds (from roi.foot_retain_days).
    foot_retain_sec: i64,
}

impl Analytics {
    pub fn new(db: &Db) -> Self {
        Self::with_foot_retention(db, 30 * 86400)
    }

    pub fn with_foot_retention(db: &Db, retain_sec: i64) -> Self {
        let mut lines: Vec<LineState> = db
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
        // restore today's crossing counters — a reload/restart must not
        // zero the day's in/out tally mid-shift
        for (lid, ic, oc) in db.load_crossings(today()) {
            if let Some(ls) = lines.iter_mut().find(|l| l.line.id == lid) {
                ls.in_count = ic;
                ls.out_count = oc;
            }
        }
        let (heat, heat_day) = db
            .load_heatmap(today())
            .unwrap_or_else(|| (vec![0u32; HEATMAP_COLS * HEATMAP_ROWS], today()));
        Self {
            inner: Mutex::new(Inner {
                lines,
                trails: Default::default(),
                heat,
                heat_day,
                workouts: Default::default(),
                alerts: Default::default(),
                alerts_unsaved: 0,
                lying: Default::default(),
                long_occ: Default::default(),
                pending_feet: Default::default(),
                feet_last: Default::default(),
            }),
            dirty_frames: AtomicU64::new(0),
            dirty_crossings: AtomicBool::new(false),
            foot_retain_sec: retain_sec.max(3600),
        }
    }

    /// Recent alerts for the Gym·Alerts card — persisted history
    /// (unresolved first) merged with anything not yet flushed.
    pub fn alerts_snapshot(&self, db: &Db) -> serde_json::Value {
        let mut rows = db.list_alerts(60);
        {
            let g = self.inner.lock();
            if g.alerts_unsaved > 0 {
                for a in g.alerts.iter().skip(g.alerts.len() - g.alerts_unsaved as usize) {
                    rows.push(serde_json::json!({
                        "id": (-1 - a.track_id),  // ephemeral: no row id yet
                        "ts": a.ts, "kind": a.kind, "level": a.level,
                        "track_id": a.track_id, "message": a.message,
                        "resolved": false,
                    }));
                }
            }
        }
        serde_json::json!({ "alerts": rows })
    }

    /// Process one frame: trails, crossing tests, heatmap accumulation.
    pub fn on_frame(&self, f: &TrackFrame) {
        let day = today();
        let mut g = self.inner.lock();
        let mut crossing_dirty = false;

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
                let trail = g.trails.entry(t.track_id).or_insert_with(|| TrackTrail {
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
                        crossing_dirty = true;
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

            // time-series foot sample (throttled) for window queries.
            // Clock regression (device resync) would starve saturating_sub
            // forever — a backward stamp RESETS the throttle instead.
            let last = g.feet_last.get(&t.track_id).copied().unwrap_or(0);
            let sample = match f.ts_ns.checked_sub(last) {
                Some(d) if d >= FOOT_SAMPLE_SEC * 1_000_000_000 => true,
                _ => last == 0 || f.ts_ns < last,
            };
            if sample {
                g.feet_last.insert(t.track_id, f.ts_ns);
                g.pending_feet.push((
                    (f.ts_ns / 1_000_000_000) as i64,
                    t.track_id,
                    fx,
                    fy,
                ));
            }
        }

        // reap stale trails (frame timestamps are unix nanoseconds)
        let cutoff = f.ts_ns.saturating_sub(TRAIL_MAX_AGE_SEC * 1_000_000_000);
        g.trails.retain(|_, tr| tr.last_ts_ns >= cutoff);

        drop(g);
        self.dirty_frames.fetch_add(1, Ordering::Relaxed);
        if crossing_dirty {
            self.dirty_crossings.store(true, Ordering::Relaxed);
        }
    }

    /// Persist the heatmap if enough dirty frames have accumulated, and any
    /// dirty crossing counters (they flip rarely — always worth a write).
    /// Called opportunistically from the command path (polls are frequent).
    pub fn maybe_save(&self, db: &Db) {
        // new alerts persist immediately — the in-memory ring resets on
        // every reload and hid the fall history
        {
            let mut g = self.inner.lock();
            if g.alerts_unsaved > 0 {
                for a in g.alerts.iter().skip(g.alerts.len() - g.alerts_unsaved as usize) {
                    let _ = db.insert_alert(a.ts, a.kind, a.level, a.track_id, &a.message);
                }
                g.alerts_unsaved = 0;
            }
        }
        db.prune_alerts(30);
        // foot samples ride every flush (poll cadence); prune old rows
        // opportunistically — the DELETE is indexed and cheap.
        {
            let mut g = self.inner.lock();
            if !g.pending_feet.is_empty() {
                db.insert_foot_log(&g.pending_feet);
                g.pending_feet.clear();
            }
        }
        db.prune_foot_log(chrono::Utc::now().timestamp() - self.foot_retain_sec);
        if self.dirty_crossings.swap(false, Ordering::Relaxed) {
            let g = self.inner.lock();
            for ls in g.lines.iter() {
                let _ = db.save_crossing(&ls.line.id, ls.day, ls.in_count, ls.out_count);
            }
        }
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
        let day = today();
        // full-replace keeps today's counters for lines that survive (the
        // editor round-trips the same ids on every save)
        let persisted = db.load_crossings(day);
        let mut g = self.inner.lock();
        g.lines = lines
            .into_iter()
            .map(|l| {
                let (ic, oc) = persisted
                    .iter()
                    .find(|(lid, _, _)| *lid == l.id)
                    .map(|(_, i, o)| (*i, *o))
                    .unwrap_or((0, 0));
                LineState {
                    line: l,
                    in_count: ic,
                    out_count: oc,
                    day,
                    armed: Default::default(),
                }
            })
            .collect();
        Ok(())
    }

    pub fn get_lines(&self) -> Vec<CrossLine> {
        self.inner
            .lock()
            .lines
            .iter()
            .map(|ls| ls.line.clone())
            .collect()
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

/// Today as CE days (matches the crossing_day/heatmap_day `day` key).
pub fn today() -> i64 {
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

/// Zone-prior vs temporal arbitration.
/// - cardio zones: zone IS the exercise (pose can't tell treadmill from
///   elliptical); duration only.
/// - strength zones: zone acts as a PRIOR; a temporal result that keeps
///   disagreeing for >5 s wins ("curling by the squat rack" = curl).
/// - no/unmapped zone: temporal classifier decides (pending → seed pose).
/// Returns (exercise, is_cardio, new_disagree_since).
fn arbitrate(
    zone_ex: Option<(&'static str, bool)>,
    temporal: &'static str,
    disagree_since: Option<f64>,
    now: f64,
) -> (&'static str, bool, Option<f64>) {
    match zone_ex {
        Some((e, true)) => (e, true, None),
        Some((e, false)) => {
            if temporal != "pending" && temporal != e {
                let since = disagree_since.unwrap_or(now);
                if now - since > 5.0 {
                    (temporal, false, Some(since))
                } else {
                    (e, false, Some(since))
                }
            } else {
                (e, false, None)
            }
        }
        None => (temporal, false, None),
    }
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
                    ts: None,
                    bbox: Bbox {
                        x,
                        y: 0.2,
                        w: 0.1,
                        h: 0.5,
                    },
                    foot: Point { x, y },
                    pose: None,
                    face: None,
                    vel: None,
                    ex: None,
                })
                .collect(),
            faces: vec![],
            img_b64: None,
        }
    }

    #[test]
    fn arbitrate_zone_prior_rules() {
        use super::arbitrate;
        // cardio zone: zone wins outright, no arbitration
        assert_eq!(
            arbitrate(Some(("treadmill_run", true)), "squat", None, 100.0),
            ("treadmill_run", true, None)
        );
        // strength zone agreeing with temporal → zone, timer reset
        assert_eq!(
            arbitrate(Some(("squat", false)), "squat", None, 100.0),
            ("squat", false, None)
        );
        // disagreement < 5 s → zone still wins, timer running
        let (ex, _, ds) = arbitrate(Some(("squat", false)), "bicep_curl", None, 100.0);
        assert_eq!(ex, "squat");
        assert_eq!(ds, Some(100.0));
        let (ex, _, ds) = arbitrate(Some(("squat", false)), "bicep_curl", Some(100.0), 104.0);
        assert_eq!(ex, "squat"); // only 4 s in
        assert_eq!(ds, Some(100.0));
        // sustained > 5 s → temporal wins
        let (ex, _, ds) = arbitrate(Some(("squat", false)), "bicep_curl", Some(100.0), 106.0);
        assert_eq!(ex, "bicep_curl");
        assert_eq!(ds, Some(100.0));
        // agreement again resets the streak
        assert_eq!(
            arbitrate(Some(("squat", false)), "squat", Some(100.0), 107.0),
            ("squat", false, None)
        );
        // pending temporal in a strength zone → zone holds
        assert_eq!(
            arbitrate(Some(("squat", false)), "pending", Some(100.0), 107.0),
            ("squat", false, None)
        );
        // unmapped zone → temporal
        assert_eq!(
            arbitrate(None, "lunge", None, 100.0),
            ("lunge", false, None)
        );
    }

    #[test]
    fn crossing_direction_and_hysteresis() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        // vertical line x=0.5, a=(0.5,0) b=(0.5,1): moving right-to-left across it
        a.set_lines(
            &db,
            vec![CrossLine {
                id: "l1".into(),
                name: "入口".into(),
                a: (0.5, 0.0),
                b: (0.5, 1.0),
            }],
        )
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
        assert_eq!(
            (s.in_count, s.out_count),
            (1, 0),
            "hysteresis blocks band loiter"
        );

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
    fn crossing_counters_survive_reload_and_resave() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        a.set_lines(
            &db,
            vec![CrossLine {
                id: "door".into(),
                name: "门口".into(),
                a: (0.5, 0.0),
                b: (0.5, 1.0),
            }],
        )
        .unwrap();
        // one left→right traversal
        a.on_frame(&frame(1_000_000_000, vec![(1, 0.30, 0.5)]));
        a.on_frame(&frame(2_000_000_000, vec![(1, 0.70, 0.5)]));
        let s = &a.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (1, 0));

        // poll flush (as get_live_state does) then "reload" via a fresh
        // Analytics over the same DB — counts must round-trip
        a.maybe_save(&db);
        let a2 = Analytics::new(&db);
        let s = &a2.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (1, 0), "counts survive reload");

        // an editor save (full-replace, same ids) must NOT zero them either
        a2.set_lines(
            &db,
            vec![CrossLine {
                id: "door".into(),
                name: "门口改线".into(),
                a: (0.5, 0.0),
                b: (0.5, 1.0),
            }],
        )
        .unwrap();
        let s = &a2.get_crossings()[0];
        assert_eq!((s.in_count, s.out_count), (1, 0), "set_lines keeps the day's tally");
    }

    #[test]
    fn no_crossing_when_parallel_or_away() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        a.set_lines(
            &db,
            vec![CrossLine {
                id: "l1".into(),
                name: "l".into(),
                a: (0.5, 0.0),
                b: (0.5, 1.0),
            }],
        )
        .unwrap();
        // movement entirely on one side
        a.on_frame(&frame(1_000_000_000, vec![(1, 0.10, 0.3)]));
        a.on_frame(&frame(2_000_000_000, vec![(1, 0.20, 0.4)]));
        assert_eq!(
            a.get_crossings()[0].in_count + a.get_crossings()[0].out_count,
            0
        );
    }

    #[test]
    fn trail_capped_and_reaped() {
        let db = Db::open(":memory:").unwrap();
        let a = Analytics::new(&db);
        let t0: u64 = 1_700_000_000_000_000_000;
        for i in 0..200 {
            a.on_frame(&frame(
                t0 + i * 100_000_000,
                vec![(1, 0.01 * (i % 100) as f32, 0.5)],
            ));
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
        let total2: u64 = hm2["grid"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap_or(0))
            .sum();
        assert_eq!(total2, 10, "heatmap round-trips through SQLite");
    }
}

// ---- workout tracking (P4): sessions, zone dwell, exercise, reps ----

/// One tracked person's workout state, from first appearance to TTL
/// expiry. The session + equipment-usage rows are persisted when the
/// track closes (or opportunistically on save).
#[derive(Debug, Clone)]
pub struct WorkoutTracker {
    pub session_id: String,
    pub started_at: f64,
    pub last_seen: f64,
    /// Sticky member association: the LAST confident identity seen while
    /// the person was in frame (face or body — face overwrites body).
    pub member_id: Option<String>,
    pub member_name: Option<String>,
    /// zone → accumulated seconds (debounced by dwell_debounce_sec)
    pub zone_secs: HashMap<String, f64>,
    pub zone_enter: Option<(String, f64)>,
    /// Device the track was seen on (config device_id; empty on
    /// hand-built test tracks → persisted as "unknown").
    pub device_id: Option<String>,
    /// Continuous dwell in the CURRENT zone (device clock), recomputed
    /// every frame — the equipment board's BUSY state keys off this
    /// (same person, same zone, ≥ busy threshold), not raw presence.
    pub zone_hold: f64,
    pub last_flush: Option<f64>,
    pub exercise: String,
    pub reps: u32,
    pub sets: u32,
    counter: crate::exercise::RepCounter,
    counter_inited: bool,
    timeline: crate::exercise::PoseTimeline,
    /// Zone-prior arbitration: when the temporal classifier persistently
    /// disagrees with the zone's mapped exercise, the temporal result wins
    /// ("curling next to the squat rack" is a curl). Timestamp of the
    /// FIRST disagreement in the current streak; None = agreeing/pending.
    disagree_since: Option<f64>,
    /// Per-EXERCISE accumulation (label → secs/reps/sets). The global
    /// reps/sets totals mix labels together when the classifier flips;
    /// this keeps the breakdown the member detail panel shows. Rep/set
    /// deltas are attributed to the label active when they landed.
    ex_log: std::collections::BTreeMap<String, (f64, u32, u32)>,
    ex_last: Option<f64>,
    last_rep_total: u32,
    last_set_total: u32,
    dirty: bool,
}

impl Inner {
    /// Workout pipeline for one frame: zone dwell, exercise
    /// classification, rep counting, sticky member identity.
    pub fn on_workout_frame(
        &mut self,
        f: &TrackFrame,
        zones: &[crate::db::Zone],
        members: &[crate::db::Member],
        idcfg: &crate::config::IdentityCfg,
        dwell_debounce_sec: u32,
    ) {
        let now = f.ts_ns as f64 / 1e9;
        let matches = crate::identity::match_tracks(idcfg, members, &f.tracks, &f.faces);
        for t in &f.tracks {
            let w = self
                .workouts
                .entry(t.track_id)
                .or_insert_with(|| WorkoutTracker {
                    session_id: format!("sess_{}", uuid::Uuid::new_v4().simple()),
                    started_at: now,
                    last_seen: now,
                    // frame's own device id — was hardcoded 'ne503-001',
                    // so every session row lied about its source
                    device_id: Some(f.device_id.clone()),
                    member_id: None,
                    member_name: None,
                    zone_secs: HashMap::new(),
                    zone_enter: None,
                    zone_hold: 0.0,
                    last_flush: None,
                    exercise: "unknown".into(),
                    reps: 0,
                    sets: 0,
                    counter: crate::exercise::RepCounter::new("unknown", false, now),
                    counter_inited: false,
                    timeline: crate::exercise::PoseTimeline::default(),
                    disagree_since: None,
                    ex_log: Default::default(),
                    ex_last: None,
                    last_rep_total: 0,
                    last_set_total: 0,
                    dirty: true,
                });
            w.last_seen = now;

            // sticky member identity
            if let Some(m) = matches.get(&t.track_id) {
                let mem = &members[m.member_idx];
                if w.member_id.as_deref() != Some(mem.id.as_str()) || m.via == "face" {
                    w.member_id = Some(mem.id.clone());
                    w.member_name = Some(mem.name.clone());
                    w.dirty = true;
                }
            }

            // zone dwell with debounce: only zones held ≥ dwell_debounce
            // seconds accumulate usage time. Attribution prefers the foot
            // (ground contact); when the camera angle hides the floor (gear
            // blocks the ankles) the body's bbox center stands in for the
            // person — a torso inside the zone is occupancy too.
            let body_in = |x: f32, y: f32| -> bool {
                zones
                    .iter()
                    .any(|z| z.enabled && crate::geo::point_in_polygon(x, y, &z.polygon))
            };
            let in_zone = if body_in(t.foot.x, t.foot.y) {
                zones.iter().find(|z| {
                    z.enabled && crate::geo::point_in_polygon(t.foot.x, t.foot.y, &z.polygon)
                })
            } else {
                let (cx, cy) = (t.bbox.x + t.bbox.w / 2.0, t.bbox.y + t.bbox.h / 2.0);
                zones
                    .iter()
                    .find(|z| z.enabled && crate::geo::point_in_polygon(cx, cy, &z.polygon))
            };
            match (in_zone, &w.zone_enter) {
                (Some(z), Some((zid, since))) if z.id == *zid => {
                    // continuous dwell in this zone — the equipment board
                    // flips BUSY only when this crosses its threshold
                    w.zone_hold = now - since;
                    // still inside: accumulate once we pass the debounce
                    if now - since >= dwell_debounce_sec as f64 {
                        *w.zone_secs.entry(z.id.clone()).or_insert(0.0) +=
                            (now - w.last_flush.unwrap_or(*since)).min(now - since);
                        w.last_flush = Some(now);
                        w.dirty = true;
                    }
                }
                (Some(z), _) => {
                    w.zone_enter = Some((z.id.clone(), now));
                    w.zone_hold = 0.0;
                    w.last_flush = None;
                }
                (None, Some(_)) => {
                    w.zone_enter = None;
                    w.zone_hold = 0.0;
                    w.last_flush = None;
                }
                (None, None) => {}
            }

            // exercise classification + reps. Tier 1: zone equipment map.
            // Tier 2: temporal oscillation analysis (which joint moves,
            // how far, how fast — separates crunch vs bench, run vs walk,
            // squat vs lunge…). The single-frame classifier remains as the
            // seed while the ~2 s history window fills ("pending").
            if let Some(pose) = t.pose.as_ref() {
                w.timeline.push(pose, now);
                let zone_ex = w
                    .zone_enter
                    .as_ref()
                    .and_then(|(zid, _)| zones.iter().find(|z| &z.id == zid))
                    .and_then(|z| crate::exercise::zone_exercise(&z.equipment_type));
                let temporal = crate::exercise::classify_with_history(&w.timeline);
                let (ex, cardio, dsince) = arbitrate(zone_ex, temporal, w.disagree_since, now);
                w.disagree_since = dsince;
                if !w.counter_inited || w.exercise != ex {
                    let total = w.reps + w.counter.reps;
                    w.reps = if w.counter_inited { total } else { 0 };
                    // bank completed sets too — the label legitimately
                    // flutters during rest (squat → standing → squat) and
                    // losing the set count each flip understated training
                    w.sets += w.counter.sets;
                    w.counter = crate::exercise::RepCounter::new(ex, cardio, now);
                    w.counter_inited = true;
                    w.exercise = ex.into();
                    w.dirty = true;
                }
                let _ = w.counter.update(pose, now);
                if w.counter.reps > 0 || w.counter.sets > 0 {
                    w.dirty = true;
                }

                // per-exercise accrual: time by dwell under the CURRENT
                // label, rep/set totals by monotonic delta (the global
                // counters preserve totals across label flips, so deltas
                // attribute cleanly to whichever label was active)
                let dt = w.ex_last.map_or(0.0, |t| (now - t).max(0.0));
                w.ex_last = Some(now);
                let rep_total = w.reps + w.counter.reps;
                let set_total = w.sets + w.counter.sets;
                let entry = w.ex_log.entry(ex.to_string()).or_insert((0.0, 0, 0));
                entry.0 += dt;
                if rep_total > w.last_rep_total {
                    entry.1 += rep_total - w.last_rep_total;
                }
                if set_total > w.last_set_total {
                    entry.2 += set_total - w.last_set_total;
                }
                w.last_rep_total = rep_total;
                w.last_set_total = set_total;

                // ---- alert rules ----
                // copy the workout data out first so the &mut w borrow ends
                // before the alert-state maps borrow self again
                let zone_hold: Option<(String, f64)> = w.zone_enter.as_ref().map(|(zid, _)| {
                    (zid.clone(), w.zone_secs.get(zid).copied().unwrap_or(0.0))
                });
                // fall-suspect: torso lying (away from vertical) sustained
                // outside the legitimately-lying exercises/zones
                let lying_ok = LYING_OK_EXERCISES.contains(&ex);
                let torso_deg = crate::exercise::torso_angle_public(pose);
                if !lying_ok && torso_deg.map_or(false, |a| a > LYING_TORSO_DEG) {
                    let st = self.lying.entry(t.track_id).or_insert((now, false));
                    if !st.1 && now - st.0 >= LYING_SUSTAIN_SEC {
                        st.1 = true;
                        self.push_alert(Alert {
                            ts: now,
                            kind: "fall",
                            level: "warn",
                            track_id: t.track_id,
                            message: format!(
                                "Fall suspect: track #{} torso tilt {:.0}\u{00b0} sustained over {}s",
                                t.track_id, torso_deg.unwrap_or(0.0), LYING_SUSTAIN_SEC as u32
                            ),
                        });
                    }
                } else {
                    self.lying.remove(&t.track_id);
                }
                // long occupancy: one person holding one zone for 30 min
                if let Some((zid, secs)) = zone_hold {
                    let key = (t.track_id, zid.clone());
                    let flagged = self.long_occ.entry(key).or_insert(false);
                    if !*flagged && secs >= LONG_OCCUPANCY_SEC {
                        *flagged = true;
                        let zname = zones
                            .iter()
                            .find(|z| z.id == zid)
                            .map(|z| z.name.as_str())
                            .unwrap_or("区域");
                        self.push_alert(Alert {
                            ts: now,
                            kind: "long_occupancy",
                            level: "info",
                            track_id: t.track_id,
                            message: format!(
                                "{} occupied over {} min (track #{})",
                                zname,
                                (LONG_OCCUPANCY_SEC / 60.0) as u32,
                                t.track_id
                            ),
                        });
                    }
                }
            }
        }
    }

    fn push_alert(&mut self, a: Alert) {
        if self.alerts.len() >= ALERT_CAP {
            self.alerts.pop_front();
        }
        self.alerts_unsaved += 1;
        self.alerts.push_back(a);
    }

    /// Close workouts whose track expired; persist them. Called from the
    /// ingest 5 s tick and get_live_state (belt-and-braces).
    ///
    /// `cam_now_ts` — the current time on the CAMERA wall clock (unix
    /// seconds, from state's latest TrackFrame ts_ns), i.e. the SAME clock
    /// `w.last_seen` is stamped in. Expiry used to compare LOCAL
    /// Utc::now() against it (RS-4, 2026-09-16 audit): any camera-clock
    /// offset (device on another NTP regime / drifted) either closed live
    /// sessions early or never closed them at all. None → no camera frame
    /// seen yet → local Utc::now (nothing can be in `workouts` before the
    /// first frame, so the fallback is trivially safe).
    pub fn close_expired_workouts(&mut self, db: &Db, ttl_sec: u32, cam_now_ts: Option<f64>) {
        let now_ts = cam_now_ts.unwrap_or_else(|| chrono::Utc::now().timestamp() as f64);
        let expired: Vec<i64> = self
            .workouts
            .iter()
            .filter(|(_, w)| now_ts - w.last_seen > ttl_sec as f64)
            .map(|(k, _)| *k)
            .collect();
        for tid in expired {
            if let Some(w) = self.workouts.remove(&tid) {
                Self::persist_workout(db, &w);
            }
        }
        // opportunistic flush of live ones too
        for w in self.workouts.values_mut() {
            if w.dirty {
                Self::persist_workout(db, w);
                w.dirty = false;
            }
        }
    }

    fn persist_workout(db: &Db, w: &WorkoutTracker) {
        let device_id = w.device_id.as_deref().unwrap_or("unknown");
        // ended_at = LAST SEEN, not close time: close_expired_workouts
        // fires one TTL after the person left — wall-clock close inflated
        // every session's duration by the TTL
        let ended = w.last_seen as i64;
        let dur = (w.last_seen - w.started_at).max(0.0) as i64;
        let _ = db.upsert_session(
            &w.session_id,
            w.member_id.as_deref(),
            device_id,
            w.started_at as i64,
            ended,
            dur,
            "closed",
            w.member_name.as_deref(),
        );
        for (zone_id, secs) in &w.zone_secs {
            if *secs < 1.0 {
                continue;
            }
            let _ = db.upsert_equipment_usage(
                &format!("{}_{}", w.session_id, zone_id),
                &w.session_id,
                w.member_id.as_deref(),
                zone_id,
                *secs as i64,
                &w.exercise,
                (w.reps + w.counter.reps) as i64,
            );
        }
        // per-exercise breakdown (member detail panel)
        for (label, (secs, reps, sets)) in w.ex_log.iter() {
            let _ = db.upsert_exercise_usage(
                &w.session_id,
                w.member_id.as_deref(),
                label,
                *sets as i64,
                *reps as i64,
                *secs as i64,
            );
        }
    }

    /// Live workout snapshot for get_live_state.
    pub fn workout_snapshot(
        &self,
    ) -> HashMap<i64, (String, u32, u32, Option<String>, f64)> {
        self.workouts
            .iter()
            .map(|(tid, w)| {
                (
                    *tid,
                    (
                        w.exercise.clone(),
                        w.reps + w.counter.reps,
                        w.sets + w.counter.sets,
                        w.zone_enter.as_ref().map(|(z, _)| z.clone()),
                        w.zone_hold,
                    ),
                )
            })
            .collect()
    }
}

impl Analytics {
    pub fn on_workout_frame(
        &self,
        f: &TrackFrame,
        zones: &[crate::db::Zone],
        members: &[crate::db::Member],
        idcfg: &crate::config::IdentityCfg,
        dwell_debounce_sec: u32,
    ) {
        self.inner
            .lock()
            .on_workout_frame(f, zones, members, idcfg, dwell_debounce_sec);
    }

    /// Camera-clock expiry — see [`Inner::close_expired_workouts`]
    /// (RS-4, 2026-09-16 audit: ttl semantics unchanged).
    pub fn close_expired_workouts(&self, db: &Db, ttl_sec: u32, cam_now_ts: Option<f64>) {
        self.inner
            .lock()
            .close_expired_workouts(db, ttl_sec, cam_now_ts);
    }

    pub fn workout_snapshot(
        &self,
    ) -> HashMap<i64, (String, u32, u32, Option<String>, f64)> {
        self.inner.lock().workout_snapshot()
    }
}
