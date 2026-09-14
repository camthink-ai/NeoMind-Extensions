// shadow.rs — Rust port of the camera's five-signal tracker (tracker.py),
// fed by the `gym/detect` raw-detection tap. SHADOW MODE: results are kept
// beside the Python pipeline's published ids for parity validation; the
// camera flips to lean mode only after the grouping agreement holds.
//
// Ported semantics (2026-09-14 state of tracker.py):
//   - gates: stationary (>15 still frames) tighten to 0.12; movers widen
//     with speed×dt (cap 0.6 full / 0.20 tile)
//   - OCM direction penalty (1+0.3·(1−cos)/2, only for tracks moving
//     >0.03/frame), BIoU height penalty (1+0.2·|Δh|/max)
//   - tile-source detections: suppress as stale duplicates only near a
//     size-comparable (0.4-2.5×) full detection; far-beside-near mints
//   - stationary takeover on mint (radius 0.25 / 0.35 when still)
//   - ORU gap velocity reset, home-EMA rep-aware stickiness
//   - ghosts: center FROZEN at expiry, speed-aware resurrect radius
//     (base 0.35, cap 0.60), height gate 0.3-3.3
// Appearance signals (osnet embeddings) are absent from the detect
// stream — same reality as the camera (osnet returns none on this
// firmware), so pass-2 rescue and appearance vetoes are omitted.

use std::collections::HashMap;

const MAX_DIST: f32 = 0.30;
const MAX_MISSED: u32 = 45;
const RESURRECT_FRAMES: u32 = 120;

#[derive(Clone, Copy, PartialEq)]
struct Pt {
    x: f32,
    y: f32,
}

struct Track {
    id: i64,
    last_center: Pt,
    missed: u32,
    vel: Option<Pt>,
    h: f32,
    still_frames: u32,
    home: Option<Pt>,
    anchor: Pt,
}

struct Ghost {
    center: Pt,
    vel: Option<Pt>,
    h: f32,
    coasted: u32,
}

/// One raw detection from `gym/detect` (keypoints already thresholded;
/// missing joints are [0,0,0]).
#[derive(Clone, Debug)]
pub struct Det {
    pub src: String,
    pub ts: u64,
    pub kp: Vec<[f32; 3]>,
}

fn kp_get(kp: &[[f32; 3]], i: usize) -> Option<Pt> {
    let k = kp.get(i)?;
    if k[2] > 0.0 {
        Some(Pt { x: k[0], y: k[1] })
    } else {
        None
    }
}

fn mid(a: Option<Pt>, b: Option<Pt>) -> Option<Pt> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Pt {
            x: (a.x + b.x) / 2.0,
            y: (a.y + b.y) / 2.0,
        }),
        _ => None,
    }
}

/// Torso center with the shoulder fallback (hips are the first keypoints
/// to drop at distance / low contrast — see pose.py body_center).
fn body_center(kp: &[[f32; 3]]) -> Option<Pt> {
    let sh = mid(kp_get(kp, 5), kp_get(kp, 6));
    let hp = mid(kp_get(kp, 11), kp_get(kp, 12));
    match mid(sh, hp) {
        Some(c) => Some(c),
        None => sh,
    }
}

fn bbox_kp(kp: &[[f32; 3]]) -> Option<(f32, f32, f32, f32)> {
    let vis: Vec<&[f32; 3]> = kp.iter().filter(|k| k[2] > 0.0).collect();
    if vis.is_empty() {
        return None;
    }
    let mut x0 = f32::MAX;
    let mut y0 = f32::MAX;
    let mut x1 = f32::MIN;
    let mut y1 = f32::MIN;
    for k in vis {
        x0 = x0.min(k[0]);
        y0 = y0.min(k[1]);
        x1 = x1.max(k[0]);
        y1 = y1.max(k[1]);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let (dx, dy) = (w * 0.10, h * 0.10);
    Some((
        (x0 - dx).max(0.0),
        (y0 - dy).max(0.0),
        (x1 + dx).min(1.0) - (x0 - dx).max(0.0),
        (y1 + dy).min(1.0) - (y0 - dy).max(0.0),
    ))
}

pub struct ShadowTracker {
    tracks: HashMap<i64, Track>,
    ghosts: HashMap<i64, Ghost>,
    next_id: i64,
    dt_ema: f32,
    last_now: Option<f64>,
}

impl Default for ShadowTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ShadowTracker {
    pub fn new() -> Self {
        Self {
            tracks: HashMap::new(),
            ghosts: HashMap::new(),
            next_id: 0,
            dt_ema: 0.1,
            last_now: None,
        }
    }

    /// Feed one frame of detections; returns (detection index → track id)
    /// for matched/minted detections (mirrors tracker.update's `ids`).
    pub fn update(&mut self, dets: &[Det], now: f64) -> Vec<Option<i64>> {
        if let Some(last) = self.last_now {
            if now > last {
                let dt = (now - last).min(1.0) as f32;
                self.dt_ema = 0.8 * self.dt_ema + 0.2 * dt;
            }
        }
        self.last_now = Some(now);

        // centers + sliver filter
        let mut centers: Vec<(usize, Pt, f32)> = Vec::new();
        for (i, d) in dets.iter().enumerate() {
            if let Some(c) = body_center(&d.kp) {
                if let Some((_, _, w, h)) = bbox_kp(&d.kp) {
                    if w < 0.015 || (h < 0.05 && w / h.max(1e-6) > 1.2) {
                        continue;
                    }
                    centers.push((i, c, h));
                }
            }
        }
        let is_tile: Vec<bool> = centers
            .iter()
            .map(|(i, _, _)| dets[*i].src == "tile")
            .collect();
        let tids: Vec<i64> = self.tracks.keys().copied().collect();

        // predicted + last centers per track
        let pred: Vec<Pt> = tids
            .iter()
            .map(|t| {
                let tr = &self.tracks[t];
                match tr.vel {
                    Some(v) => Pt {
                        x: tr.last_center.x + v.x,
                        y: tr.last_center.y + v.y,
                    },
                    None => tr.last_center,
                }
            })
            .collect();
        let tlc: Vec<Pt> = tids.iter().map(|t| self.tracks[t].last_center).collect();
        let t_h: Vec<f32> = tids.iter().map(|t| self.tracks[t].h).collect();
        let speed: Vec<f32> = tids
            .iter()
            .map(|t| {
                self.tracks[t]
                    .vel
                    .map(|v| (v.x * v.x + v.y * v.y).sqrt())
                    .unwrap_or(0.0)
            })
            .collect();
        let still: Vec<bool> = tids.iter().map(|t| self.tracks[t].still_frames > 15).collect();

        // cost matrix with gates, OCM and BIoU — collect candidate cells
        let mut candidates: Vec<(u8, f32, usize, usize)> = Vec::new(); // (pass, cost, track_idx, det_idx)
        for (di, &(_, dc, dh)) in centers.iter().enumerate() {
            for (ti, _) in tids.iter().enumerate() {
                let gate_full = if still[ti] {
                    0.12f32
                } else {
                    MAX_DIST + speed[ti] * self.dt_ema * 2.0
                }
                .clamp(0.05, 0.6);
                let gate_tile = (MAX_DIST + speed[ti] * self.dt_ema * 1.5).min(0.20);
                let gate = if is_tile[di] { gate_tile } else { gate_full };
                let d_pred = ((dc.x - pred[ti].x).powi(2) + (dc.y - pred[ti].y).powi(2)).sqrt();
                let d_last = ((dc.x - tlc[ti].x).powi(2) + (dc.y - tlc[ti].y).powi(2)).sqrt();
                let d = d_pred.min(d_last);
                let ratio_ok = t_h[ti] <= 0.0
                    || dh <= 0.0
                    || (dh / t_h[ti] > 0.18 && dh / t_h[ti] < 5.5);
                if !(d <= gate && ratio_ok) {
                    continue;
                }
                // OCM: displacement vs track velocity direction
                let mut cost = d;
                if speed[ti] > 0.03 {
                    let tv = self.tracks[&tids[ti]].vel.unwrap();
                    let disp = (dc.x - tlc[ti].x, dc.y - tlc[ti].y);
                    let dn = (disp.0 * disp.0 + disp.1 * disp.1).sqrt().max(1e-6);
                    let cos = (disp.0 * tv.x + disp.1 * tv.y) / (dn * speed[ti].max(1e-6));
                    cost *= 1.0 + 0.3 * (1.0 - cos) / 2.0;
                }
                // BIoU height similarity
                let hdiff = (dh - t_h[ti]).abs() / dh.max(t_h[ti]).max(1e-6);
                cost *= 1.0 + 0.2 * hdiff;
                candidates.push((if is_tile[di] { 1 } else { 0 }, cost, ti, di));
            }
        }
        candidates.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());

        // greedy one-to-one assignment (pass 1)
        let mut matched_tracks: Vec<usize> = Vec::new();
        let mut matched_dets: Vec<usize> = Vec::new();
        let mut assign: HashMap<usize, usize> = HashMap::new(); // det_idx → track_idx
        for (_, _, ti, di) in candidates {
            if matched_tracks.contains(&ti) || matched_dets.contains(&di) {
                continue;
            }
            matched_tracks.push(ti);
            matched_dets.push(di);
            assign.insert(di, ti);
        }

        // unmatched detections: tile suppression / takeover / mint
        let mut ids: Vec<Option<i64>> = vec![None; dets.len()];
        let mut new_centers: HashMap<i64, (Pt, f32)> = HashMap::new();
        for (di, &(_, dc, dh)) in centers.iter().enumerate() {
            let tid = if let Some(ti) = assign.get(&di) {
                tids[*ti]
            } else if is_tile[di] {
                // stale-duplicate suppression: only near a SIZE-COMPARABLE
                // full detection (far-beside-near mints, 2026-09-14 fix)
                let mut near_full = false;
                for (dj, &(_, dc2, dh2)) in centers.iter().enumerate() {
                    if is_tile[dj] {
                        continue;
                    }
                    if dh > 1e-6 && dh2 > 1e-6 && !(0.4..2.5).contains(&(dh / dh2)) {
                        continue;
                    }
                    let dist = ((dc.x - dc2.x).powi(2) + (dc.y - dc2.y).powi(2)).sqrt();
                    if dist < 0.22f32.max(0.9 * dh.max(dh2)) {
                        near_full = true;
                        break;
                    }
                }
                if near_full {
                    continue; // drop: ids[di] stays None
                }
                match self.resurrect(dc, dh) {
                    Some(t) => t,
                    None => {
                        let t = self.next_id;
                        self.next_id += 1;
                        t
                    }
                }
            } else {
                match self.resurrect(dc, dh) {
                    Some(t) => t,
                    None => {
                        // stationary takeover before minting
                        let mut taken = None;
                        for (lt, ltr) in self.tracks.iter() {
                            if ltr.h > 0.0
                                && dh > 0.0
                                && !(0.35..2.9).contains(&(dh / ltr.h))
                            {
                                continue;
                            }
                            let r = if ltr.still_frames > 15 { 0.35 } else { 0.25 };
                            let d2 = ((dc.x - ltr.last_center.x).powi(2)
                                + (dc.y - ltr.last_center.y).powi(2))
                                .sqrt();
                            if d2 < r {
                                taken = Some(*lt);
                                break;
                            }
                        }
                        match taken {
                            Some(t) => t,
                            None => {
                                let t = self.next_id;
                                self.next_id += 1;
                                t
                            }
                        }
                    }
                }
            };
            ids[centers[di].0] = Some(tid);
            new_centers.insert(tid, (dc, dh));
        }

        // refresh matched tracks / expire into ghosts
        let matched_tids: Vec<i64> = matched_tracks.iter().map(|ti| tids[*ti]).collect();
        for (tid, tr) in self.tracks.iter_mut() {
            if let Some(&(nc, nh)) = new_centers.get(tid) {
                if !matched_tids.contains(tid) {
                    tr.missed += 1; // tile-only keep-alive
                    continue;
                }
                tr.h = nh;
                let _was_missed = tr.missed;
                if _was_missed > 2 && tr.vel.is_some() {
                    let gap = _was_missed.max(1) as f32;
                    let v = tr.vel.unwrap();
                    tr.vel = Some(Pt {
                        x: (nc.x - tr.last_center.x) / gap,
                        y: (nc.y - tr.last_center.y) / gap,
                    });
                    let _ = v;
                } else {
                    tr.vel = Some(match tr.vel {
                        None => Pt {
                            x: nc.x - tr.last_center.x,
                            y: nc.y - tr.last_center.y,
                        },
                        Some(v) => Pt {
                            x: 0.6 * v.x + 0.4 * (nc.x - tr.last_center.x),
                            y: 0.6 * v.y + 0.4 * (nc.y - tr.last_center.y),
                        },
                    });
                }
                tr.last_center = nc;
                tr.anchor = nc;
                tr.missed = 0;
                // rep-aware stickiness: motionless OR oscillating near home
                let spd = tr.vel.map(|v| (v.x * v.x + v.y * v.y).sqrt()).unwrap_or(0.0);
                let home = match tr.home {
                    None => {
                        tr.home = Some(nc);
                        nc
                    }
                    Some(h) => {
                        let nh = Pt {
                            x: 0.9 * h.x + 0.1 * nc.x,
                            y: 0.9 * h.y + 0.1 * nc.y,
                        };
                        tr.home = Some(nh);
                        nh
                    }
                };
                let home_r = 0.05f32.max(0.45 * tr.h.max(0.0));
                if spd < 0.02 || ((nc.x - home.x).powi(2) + (nc.y - home.y).powi(2)).sqrt() < home_r
                {
                    tr.still_frames = (tr.still_frames + 1).min(300);
                } else {
                    tr.still_frames = 0;
                }
            } else {
                tr.missed += 1;
            }
        }

        // minted tracks register their state
        for (tid, &(nc, dh)) in new_centers.iter() {
            if !self.tracks.contains_key(tid) {
                self.tracks.insert(
                    *tid,
                    Track {
                        id: *tid,
                        last_center: nc,
                        missed: 0,
                        vel: None,
                        h: dh,
                        still_frames: 0,
                        home: Some(nc),
                        anchor: nc,
                    },
                );
            }
        }

        // expire → ghosts
        let expired: Vec<i64> = self
            .tracks
            .iter()
            .filter(|(_, tr)| tr.missed > MAX_MISSED)
            .map(|(t, _)| *t)
            .collect();
        for tid in expired {
            let tr = self.tracks.remove(&tid).unwrap();
            self.ghosts.insert(
                tid,
                Ghost {
                    center: tr.anchor,
                    vel: tr.vel,
                    h: tr.h,
                    coasted: 0,
                },
            );
        }

        // ghost aging
        for gtid in self.ghosts.keys().copied().collect::<Vec<_>>() {
            if self.tracks.contains_key(&gtid) {
                self.ghosts.remove(&gtid);
                continue;
            }
            if let Some(g) = self.ghosts.get_mut(&gtid) {
                g.coasted += 1;
                if g.coasted > RESURRECT_FRAMES {
                    self.ghosts.remove(&gtid);
                }
            }
        }

        ids
    }

    fn resurrect(&mut self, dc: Pt, dh: f32) -> Option<i64> {
        // no appearance on the detect stream — position/size only
        let mut best_tid: Option<i64> = None;
        let mut best_d = 0.35f32;
        for (gtid, g) in self.ghosts.iter() {
            if g.h > 0.0 && dh > 0.0 && !(0.3..3.3).contains(&(dh / g.h)) {
                continue;
            }
            let speed = g.vel.map(|v| (v.x * v.x + v.y * v.y).sqrt()).unwrap_or(0.0);
            let radius = (0.35 + speed * g.coasted as f32 * self.dt_ema * 0.8).min(0.60);
            let d = ((dc.x - g.center.x).powi(2) + (dc.y - g.center.y).powi(2)).sqrt();
            if d < best_d.min(radius) {
                best_tid = Some(*gtid);
                best_d = d;
            }
        }
        if let Some(t) = best_tid {
            self.ghosts.remove(&t);
        }
        best_tid
    }

    pub fn live_ids(&self) -> Vec<i64> {
        self.tracks.keys().copied().collect()
    }
}

// ---- parity validation ----

/// One frame's id-assignment snapshot from one tracker.
#[derive(Clone, Debug)]
pub struct IdSnap {
    pub ts_ns: u64,
    /// (center_x, center_y, id)
    pub assigns: Vec<(f32, f32, i64)>,
}

#[derive(Default)]
pub struct ParityState {
    pub rust_snaps: Vec<IdSnap>,
    pub py_snaps: Vec<IdSnap>,
    pub detect_frames: u64,
    pub track_frames: u64,
}

impl ParityState {
    const MAX_SNAPS: usize = 400;

    pub fn push_rust(&mut self, s: IdSnap) {
        self.detect_frames += 1;
        self.rust_snaps.push(s);
        if self.rust_snaps.len() > Self::MAX_SNAPS {
            self.rust_snaps.remove(0);
        }
    }

    pub fn push_py(&mut self, s: IdSnap) {
        self.track_frames += 1;
        self.py_snaps.push(s);
        if self.py_snaps.len() > Self::MAX_SNAPS {
            self.py_snaps.remove(0);
        }
    }

    /// Compare partitions frame-by-frame: for every detect frame, find the
    /// py snapshot within ±150 ms, match detections by center (<0.035),
    /// then count id-grouping agreement over all detection PAIRS.
    pub fn report(&self) -> serde_json::Value {
        let mut frames = 0u64;
        let mut pair_agree = 0u64;
        let mut pair_disagree = 0u64;
        let mut unmatched_dets = 0u64;
        for rs in &self.rust_snaps {
            let py = self
                .py_snaps
                .iter()
                .filter(|p| p.ts_ns.abs_diff(rs.ts_ns) < 150_000_000)
                .min_by_key(|p| p.ts_ns.abs_diff(rs.ts_ns));
            let Some(py) = py else { continue };
            // match rust dets to py dets by center proximity
            let mut map: Vec<(usize, usize)> = Vec::new(); // (rust_idx, py_idx)
            for (ri, r) in rs.assigns.iter().enumerate() {
                let mut best: Option<(f32, usize)> = None;
                for (pi, p) in py.assigns.iter().enumerate() {
                    if map.iter().any(|(_, pj)| *pj == pi) {
                        continue;
                    }
                    let d = ((r.0 - p.0).powi(2) + (r.1 - p.1).powi(2)).sqrt();
                    if d < 0.035 && best.map(|(bd, _)| d < bd).unwrap_or(true) {
                        best = Some((d, pi));
                    }
                }
                if let Some((_, pi)) = best {
                    map.push((ri, pi));
                } else {
                    unmatched_dets += 1;
                }
            }
            if map.len() < 2 {
                continue;
            }
            frames += 1;
            for a in 0..map.len() {
                for b in (a + 1)..map.len() {
                    let (ra, rb) = (map[a].0, map[b].0);
                    let (pa, pb) = (map[a].1, map[b].1);
                    let same_rust = rs.assigns[ra].2 == rs.assigns[rb].2;
                    let same_py = py.assigns[pa].2 == py.assigns[pb].2;
                    if same_rust == same_py {
                        pair_agree += 1;
                    } else {
                        pair_disagree += 1;
                    }
                }
            }
        }
        let total = pair_agree + pair_disagree;
        serde_json::json!({
            "frames_compared": frames,
            "pair_agree": pair_agree,
            "pair_disagree": pair_disagree,
            "pair_agreement": if total > 0 {
                format!("{:.3}", pair_agree as f64 / total as f64)
            } else { "n/a".into() },
            "unmatched_dets": unmatched_dets,
            "detect_frames": self.detect_frames,
            "track_frames": self.track_frames,
            "rust_live_ids": self.rust_snaps.last().map(|s| s.assigns.len()).unwrap_or(0),
        })
    }
}

pub static SHADOW: std::sync::OnceLock<parking_lot::Mutex<(ShadowTracker, ParityState)>> =
    std::sync::OnceLock::new();

pub fn shadow() -> &'static parking_lot::Mutex<(ShadowTracker, ParityState)> {
    SHADOW.get_or_init(|| parking_lot::Mutex::new((ShadowTracker::new(), ParityState::default())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(cx: f32, cy: f32, h: f32) -> Det {
        // minimal person: shoulders + hips around the center
        let mut kp = vec![[0.0, 0.0, 0.0]; 17];
        kp[5] = [cx - h * 0.15, cy - h * 0.25, 0.9];
        kp[6] = [cx + h * 0.15, cy - h * 0.25, 0.9];
        kp[11] = [cx - h * 0.12, cy + h * 0.25, 0.8];
        kp[12] = [cx + h * 0.12, cy + h * 0.25, 0.8];
        Det { src: "full".into(), ts: 0, kp }
    }

    #[test]
    fn stable_id_for_stationary_person() {
        let mut t = ShadowTracker::new();
        let ids1 = t.update(&[det(0.5, 0.5, 0.3)], 1.0);
        assert_eq!(ids1[0], Some(0));
        for i in 2..40 {
            let ids = t.update(&[det(0.5, 0.5, 0.3)], i as f64);
            assert_eq!(ids[0], Some(0), "frame {}", i);
        }
    }

    #[test]
    fn two_people_keep_separate_ids() {
        let mut t = ShadowTracker::new();
        for i in 0..30 {
            let a = det(0.3, 0.5, 0.3);
            let b = det(0.7, 0.5, 0.3);
            let ids = t.update(&[a, b], i as f64);
            if i == 0 {
                assert_eq!(ids.iter().filter_map(|x| *x).count(), 2);
            }
            let set: std::collections::HashSet<i64> =
                ids.iter().filter_map(|x| *x).collect();
            assert_eq!(set.len(), 2, "frame {}", i);
        }
    }

    #[test]
    fn mover_keeps_id_across_gap() {
        let mut t = ShadowTracker::new();
        let ids = t.update(&[det(0.2, 0.5, 0.3)], 1.0);
        assert_eq!(ids[0], Some(0));
        // short occlusion gap then reappear nearby
        let ids = t.update(&[], 2.0);
        assert!(ids.is_empty());
        let ids = t.update(&[det(0.24, 0.5, 0.3)], 3.0);
        assert_eq!(ids[0], Some(0), "resurrect after gap");
    }

    #[test]
    fn far_tile_det_next_to_near_full_mints() {
        let mut t = ShadowTracker::new();
        // near person (h=0.5) + far person (h=0.06) stacked in 2D
        let mut near = det(0.5, 0.6, 0.5);
        near.src = "full".into();
        let mut far = det(0.5, 0.30, 0.06);
        far.src = "tile".into();
        let ids = t.update(&[near, far], 1.0);
        let set: std::collections::HashSet<i64> =
            ids.iter().filter_map(|x| *x).collect();
        assert_eq!(set.len(), 2, "far-beside-near must mint, not suppress");
    }
}
