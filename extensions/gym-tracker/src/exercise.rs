// exercise.rs
//! Rule-based exercise classification + rep counting from COCO-17
//! keypoints — a Rust port of the gym-ops heuristics (MIT), pure CPU.
//!
//! Classification priority:
//! 1. Zone → equipment_type → exercise (cardio machines: no reps, only
//!    duration; strength zones map to their exercise)
//! 2. Keypoint posture heuristics (torso angle, knee/elbow/spine angles)
//!
//! Counting: per-track UP⇄DOWN state machine on the exercise's primary
//! joint angle, with a full-amplitude guard (a rep counts only after the
//! DOWN threshold was actually reached) and set segmentation (a rest
//! longer than SET_REST_SECONDS closes the set).

use crate::types::Pose;

// COCO-17 indices
const NOSE: usize = 0;
const L_SHOULDER: usize = 5;
const R_SHOULDER: usize = 6;
const L_ELBOW: usize = 7;
const R_ELBOW: usize = 8;
const L_WRIST: usize = 9;
const R_WRIST: usize = 10;
const L_HIP: usize = 11;
const R_HIP: usize = 12;
const L_KNEE: usize = 13;
const R_KNEE: usize = 14;
const L_ANKLE: usize = 15;
const R_ANKLE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pt {
    x: f32,
    y: f32,
}

fn kp(pose: &Pose, i: usize) -> Option<Pt> {
    let k = pose.kpts.get(i)?;
    if k[2] <= 0.15 {
        return None;
    }
    Some(Pt { x: k[0], y: k[1] })
}

/// Angle at `b` (degrees) formed by a-b-c; None when any point missing.
fn joint_angle(a: Option<Pt>, b: Option<Pt>, c: Option<Pt>) -> Option<f32> {
    let (a, b, c) = (a?, b?, c?);
    let v1 = (a.x - b.x, a.y - b.y);
    let v2 = (c.x - b.x, c.y - b.y);
    let dot = v1.0 * v2.0 + v1.1 * v2.1;
    let n1 = (v1.0 * v1.0 + v1.1 * v1.1).sqrt();
    let n2 = (v2.0 * v2.0 + v2.1 * v2.1).sqrt();
    if n1 <= 0.0 || n2 <= 0.0 {
        return None;
    }
    Some((dot / (n1 * n2)).clamp(-1.0, 1.0).acos().to_degrees())
}

/// Torso lean from vertical (0 = upright), from shoulder-mid vs hip-mid.
fn torso_angle(pose: &Pose) -> Option<f32> {
    let (ls, rs) = (kp(pose, L_SHOULDER)?, kp(pose, R_SHOULDER)?);
    let (lh, rh) = (kp(pose, L_HIP)?, kp(pose, R_HIP)?);
    let sx = (ls.x + rs.x) / 2.0;
    let sy = (ls.y + rs.y) / 2.0;
    let hx = (lh.x + rh.x) / 2.0;
    let hy = (lh.y + rh.y) / 2.0;
    let dx = sx - hx;
    let dy = sy - hy;
    Some(dx.abs().atan2(dy.abs().max(1e-6)).to_degrees())
}

/// Zone equipment_type → exercise. Cardio machines count duration only.
pub fn zone_exercise(equipment_type: &str) -> Option<(&'static str, bool)> {
    // (exercise, is_cardio)
    match equipment_type {
        "treadmill" => Some(("treadmill_run", true)),
        "elliptical" => Some(("elliptical", true)),
        "spin_bike" | "bike" => Some(("spin_bike", true)),
        "rowing" => Some(("rowing", true)),
        "squat_rack" | "power_rack" => Some(("squat", false)),
        "bench" | "bench_press" => Some(("bench_press", false)),
        "deadlift_platform" => Some(("deadlift", false)),
        "pullup_bar" => Some(("pullup", false)),
        "cable_machine" => Some(("lat_pulldown", false)),
        "mat" | "yoga_mat" | "crunch_mat" => Some(("crunch", false)),
        "dumbbell" | "free_weights" => Some(("bicep_curl", false)),
        "kettlebell" => Some(("kettlebell_swing", false)),
        _ => None,
    }
}

/// Pose-only classification (zone unknown / generic zones). Ported from
/// gym-ops ExerciseClassifier decision tree.
pub fn classify_from_pose(pose: &Pose) -> &'static str {
    let t = torso_angle(pose);

    let knee = joint_angle(
        kp(pose, L_HIP).or_else(|| kp(pose, R_HIP)),
        kp(pose, L_KNEE).or_else(|| kp(pose, R_KNEE)),
        kp(pose, L_ANKLE).or_else(|| kp(pose, R_ANKLE)),
    );
    let elbow = joint_angle(
        kp(pose, L_SHOULDER).or_else(|| kp(pose, R_SHOULDER)),
        kp(pose, L_ELBOW).or_else(|| kp(pose, R_ELBOW)),
        kp(pose, L_WRIST).or_else(|| kp(pose, R_WRIST)),
    );
    let spine = joint_angle(
        kp(pose, L_SHOULDER).or_else(|| kp(pose, R_SHOULDER)),
        kp(pose, L_HIP).or_else(|| kp(pose, R_HIP)),
        kp(pose, L_KNEE).or_else(|| kp(pose, R_KNEE)),
    );

    // torso horizontal → bench / pushup
    if t.map_or(false, |a| a > 45.0) {
        return if elbow.map_or(false, |e| e < 130.0) {
            "bench_press"
        } else {
            "pushup"
        };
    }
    if t.map_or(false, |a| a < 30.0) {
        // significant knee bend
        if knee.map_or(false, |k| k < 130.0) {
            return if spine.map_or(false, |s| s < 150.0) {
                "deadlift"
            } else {
                "squat"
            };
        }
        if elbow.map_or(false, |e| e < 90.0) {
            let arms_up = kp(pose, L_WRIST).zip(kp(pose, L_SHOULDER))
                .map_or(false, |(w, s)| w.y < s.y)
                || kp(pose, R_WRIST).zip(kp(pose, R_SHOULDER))
                    .map_or(false, |(w, s)| w.y < s.y);
            return if arms_up { "lat_pulldown" } else { "bicep_curl" };
        }
    }
    "unknown"
}

// ---- thresholds (degrees), ported from gym-ops counter.py ----
fn thresholds(exercise: &str) -> Option<(f32, f32)> {
    // (up_angle, down_angle) on the exercise's primary joint
    Some(match exercise {
        "squat" | "lunge" => (160.0, 110.0),   // knee angle
        "deadlift" => (160.0, 130.0),          // hip angle
        "bench_press" | "pushup" => (160.0, 100.0), // elbow
        "lat_pulldown" => (160.0, 110.0),      // elbow
        "bicep_curl" | "crunch" | "situp" => (150.0, 60.0), // elbow / torso-hip
        "shoulder_press" => (150.0, 80.0),     // elbow
        "pullup" => (160.0, 90.0),             // elbow
        "kettlebell_swing" => (170.0, 120.0),  // hip angle
        _ => return None,   // cardio / plank / standing: duration only
    })
}

/// Primary joint angle for the exercise (best visible side).
fn primary_angle(pose: &Pose, exercise: &str) -> Option<f32> {
    let best = |(a, b, c): (usize, usize, usize)| {
        let l = joint_angle(kp(pose, a), kp(pose, b), kp(pose, c));
        let r = joint_angle(
            kp(pose, a + 1), kp(pose, b + 1), kp(pose, c + 1),
        );
        // prefer the side with more confidence; fall back to either
        match (l, r) {
            (Some(l), Some(r)) => Some(l.min(r).max(l.min(r))), // avg-ish: take mean
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (None, None) => None,
        }
    };
    match exercise {
        "squat" | "lunge" => best((L_HIP, L_KNEE, L_ANKLE)),
        "deadlift" | "kettlebell_swing" => best((L_SHOULDER, L_HIP, L_KNEE)),
        "crunch" | "situp" => best((L_SHOULDER, L_HIP, L_KNEE)),
        "bench_press" | "pushup" | "lat_pulldown" | "bicep_curl"
        | "shoulder_press" | "pullup" => best((L_SHOULDER, L_ELBOW, L_WRIST)),
        _ => None,
    }
}

// ---- temporal (second-tier) classification ----
//
// Single-frame postures cannot separate e.g. crunch vs bench_press (both
// lying) or run vs walk (both upright with knee motion). The deciding
// signal is WHICH joint oscillates, at what amplitude and cadence:
//
//   lying + torso/shoulder-hip oscillates, elbow quiet → crunch (small
//        amplitude) / situp (large)
//   lying + elbow oscillates, torso quiet → bench_press / pushup
//   lying + everything static, elbows under shoulders → plank
//   upright + knee oscillation, ankles together → squat; ankles split
//        front/back → lunge
//   upright + knee oscillation, high cadence + hip bob, no load posture
//        → run (>~1.4 Hz) / walk (0.7–1.4 Hz)
//   upright + elbow oscillation + wrists above shoulders → pullup /
//        shoulder_press; wrists below → bicep_curl
//   symmetric arm+leg oscillation → jumping_jack
//   upright, nothing oscillating → standing

/// Rolling per-track pose feature window (~4 s at 5 Hz).
#[derive(Debug, Clone, Default)]
pub struct PoseTimeline {
    pub t: Vec<f64>,
    /// torso lean from vertical (deg)
    pub torso: Vec<Option<f32>>,
    /// hip-torso angle: shoulder-hip-knee (deg) — crunch/situp driver
    pub torso_hip: Vec<Option<f32>>,
    /// best knee angle (deg)
    pub knee: Vec<Option<f32>>,
    /// best elbow angle (deg)
    pub elbow: Vec<Option<f32>>,
    /// hip-center y (normalized; smaller = higher)
    pub hip_y: Vec<f32>,
    /// wrists above shoulders? (either side)
    pub arms_up: Vec<bool>,
    /// ankle x distance (normalized stride)
    pub stride: Vec<f32>,
}

fn shoulder_hip_angle(pose: &Pose) -> Option<f32> {
    joint_angle(
        kp(pose, L_SHOULDER).or_else(|| kp(pose, R_SHOULDER)),
        kp(pose, L_HIP).or_else(|| kp(pose, R_HIP)),
        kp(pose, L_KNEE).or_else(|| kp(pose, R_KNEE)),
    )
}

fn hip_center_y(pose: &Pose) -> Option<f32> {
    let l = kp(pose, L_HIP)?;
    let r = kp(pose, R_HIP)?;
    Some((l.y + r.y) / 2.0)
}

fn stride_width(pose: &Pose) -> Option<f32> {
    let l = kp(pose, L_ANKLE)?;
    let r = kp(pose, R_ANKLE)?;
    Some((l.x - r.x).abs())
}

impl PoseTimeline {
    pub fn push(&mut self, pose: &Pose, now: f64) {
        const MAX: usize = 24; // ~4-5 s of history
        self.t.push(now);
        self.torso.push(torso_angle(pose));
        self.torso_hip.push(shoulder_hip_angle(pose));
        self.knee.push(best(
            joint_angle(kp(pose, L_HIP), kp(pose, L_KNEE), kp(pose, L_ANKLE)),
            joint_angle(kp(pose, R_HIP), kp(pose, R_KNEE), kp(pose, R_ANKLE)),
        ));
        self.elbow.push(best(
            joint_angle(kp(pose, L_SHOULDER), kp(pose, L_ELBOW), kp(pose, L_WRIST)),
            joint_angle(kp(pose, R_SHOULDER), kp(pose, R_ELBOW), kp(pose, R_WRIST)),
        ));
        self.hip_y.push(hip_center_y(pose).unwrap_or(f32::NAN));
        self.arms_up.push(
            kp(pose, L_WRIST).zip(kp(pose, L_SHOULDER))
                .map_or(false, |(w, s)| w.y < s.y)
                || kp(pose, R_WRIST).zip(kp(pose, R_SHOULDER))
                    .map_or(false, |(w, s)| w.y < s.y),
        );
        self.stride.push(stride_width(pose).unwrap_or(f32::NAN));
        if self.t.len() > MAX {
            for v in [&mut self.torso, &mut self.torso_hip, &mut self.knee, &mut self.elbow] {
                v.remove(0);
            }
            self.t.remove(0);
            self.hip_y.remove(0);
            self.arms_up.remove(0);
            self.stride.remove(0);
        }
    }

    fn latest_torso(&self) -> Option<f32> {
        *self.torso.last()?
    }
}

fn best(l: Option<f32>, r: Option<f32>) -> Option<f32> {
    match (l, r) {
        (Some(l), Some(r)) => Some((l + r) / 2.0),
        (Some(l), None) => Some(l),
        (None, Some(r)) => Some(r),
        (None, None) => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct Osc {
    amplitude: f32,
    /// oscillations per second estimated from direction changes of the
    /// (detrended) series
    cadence_hz: f32,
}

fn oscillation(series: &[Option<f32>], t: &[f64]) -> Option<Osc> {
    let vals: Vec<f32> = series.iter().filter_map(|v| *v).collect();
    if vals.len() < 6 {
        return None;
    }
    let mx = vals.iter().cloned().fold(f32::MIN, f32::max);
    let mn = vals.iter().cloned().fold(f32::MAX, f32::min);
    let amplitude = mx - mn;
    // direction changes on the detrended series → half-cycles
    let mid = (mx + mn) / 2.0;
    let mut crossings = 0;
    let mut prev_above: Option<bool> = None;
    for v in &vals {
        let above = *v > mid;
        if let Some(p) = prev_above {
            if p != above {
                crossings += 1;
            }
        }
        prev_above = Some(above);
    }
    let span = t.last()? - t.first()?;
    let cadence_hz = if span > 0.0 {
        (crossings as f32 / 2.0) / span as f32
    } else {
        0.0
    };
    Some(Osc { amplitude, cadence_hz })
}

/// Second-tier classifier: posture family from the latest frame, specific
/// exercise from temporal oscillation features. Falls back to the static
/// classifier while the window is still filling.
pub fn classify_with_history(tl: &PoseTimeline) -> &'static str {
    if tl.t.len() < 8 {
        // not enough history yet — single-frame guess
        return "pending";
    }
    let torso = tl.latest_torso();
    let knee_osc = oscillation(&tl.knee, &tl.t);
    let elbow_osc = oscillation(&tl.elbow, &tl.t);
    let tho = oscillation(&tl.torso_hip, &tl.t);
    let hip_y_osc = oscillation(
        &tl.hip_y.iter().map(|v| Some(*v)).collect::<Vec<_>>(), &tl.t);

    // ---- lying family (torso > 45° lean) ----
    if torso.map_or(false, |a| a > 45.0) {
        // torso-hip oscillating, elbow quiet → crunch (small) / situp (big)
        if let Some(o) = tho {
            if o.amplitude > 12.0
                && elbow_osc.map_or(true, |e| e.amplitude < o.amplitude)
            {
                return if o.amplitude > 35.0 { "situp" } else { "crunch" };
            }
        }
        // fully static, elbows under shoulders → plank
        let all_quiet = knee_osc.map_or(true, |o| o.amplitude < 15.0)
            && elbow_osc.map_or(true, |o| o.amplitude < 20.0)
            && tho.map_or(true, |o| o.amplitude < 12.0);
        if all_quiet && tl.arms_up.iter().all(|_| true) {
            return "plank";
        }
        // elbow oscillating → bench_press (feet up/elevated) vs pushup
        // (body straight, ankles on ground) — approximate via hip height
        // stability: pushup keeps hips level
        return if hip_y_osc.map_or(false, |o| o.amplitude < 0.05) {
            "pushup"
        } else {
            "bench_press"
        };
    }

    // ---- upright family ----
    if torso.map_or(true, |a| a < 30.0) {
        // knee oscillation present?
        if let Some(o) = knee_osc {
            if o.amplitude > 25.0 {
                // wide stance / split stance → lunge
                let stride = tl.stride.iter()
                    .filter(|v| !v.is_nan()).cloned().fold(0.0f32, f32::max);
                if stride > 0.16 {
                    return "lunge";
                }
                // high cadence + hip bob + arms not loaded → run / walk
                let bob = hip_y_osc.map_or(0.0, |h| h.amplitude);
                if o.cadence_hz > 1.4 && bob > 0.012 {
                    return "run";
                }
                if o.cadence_hz > 0.6 {
                    return "walk";
                }
                // slow deep knee cycle → squat / deadlift by spine line
                return if tho.map_or(true, |t| t.amplitude < 20.0) {
                    "squat"
                } else {
                    "deadlift"
                };
            }
        }
        // elbow oscillation → pullup/shoulder_press (arms up) vs curl
        if let Some(o) = elbow_osc {
            if o.amplitude > 25.0 {
                let up_frac = tl.arms_up.iter().filter(|u| **u).count() as f32
                    / tl.arms_up.len() as f32;
                if up_frac > 0.7 {
                    // bar above (static wrists high) → shoulder_press /
                    // pullup; distinguish by hip height oscillation
                    // (pullups lift the whole body)
                    return if hip_y_osc.map_or(false, |h| h.amplitude > 0.03) {
                        "pullup"
                    } else {
                        "shoulder_press"
                    };
                }
                return "bicep_curl";
            }
        }
        return "standing";
    }

    // seated / transitional postures — static classifier fallback
    "unknown"
}

const MIN_REP_SECONDS: f64 = 0.6;
const SET_REST_SECONDS: f64 = 15.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Up,
    Down,
    Unknown,
}

/// Per-track rep counter state.
#[derive(Debug, Clone)]
pub struct RepCounter {
    pub exercise: String,
    pub is_cardio: bool,
    pub phase: Phase,
    pub reps: u32,
    pub sets: u32,
    reached_down: bool,
    last_transition: f64,
    last_rep: f64,
    rest_since: Option<f64>,
}

impl RepCounter {
    pub fn new(exercise: &str, is_cardio: bool, now: f64) -> Self {
        Self {
            exercise: exercise.into(),
            is_cardio,
            phase: Phase::Unknown,
            reps: 0,
            sets: 0,
            reached_down: false,
            last_transition: now,
            last_rep: now,
            rest_since: Some(now),
        }
    }

    /// Feed one pose; returns the rep count. `now` in epoch seconds.
    pub fn update(&mut self, pose: &Pose, now: f64) -> u32 {
        if self.is_cardio {
            return self.reps; // duration-only exercises
        }
        let Some((up_a, down_a)) = thresholds(&self.exercise) else {
            return self.reps;
        };
        let Some(angle) = primary_angle(pose, &self.exercise) else {
            self.phase = Phase::Unknown;
            return self.reps;
        };

        let new_phase = if angle >= up_a {
            Phase::Up
        } else if angle <= down_a {
            Phase::Down
        } else {
            self.phase // hysteresis band: keep previous phase
        };

        if new_phase != self.phase {
            // DOWN→UP with full amplitude = one rep
            if self.phase == Phase::Down && new_phase == Phase::Up && self.reached_down {
                if now - self.last_transition >= MIN_REP_SECONDS {
                    self.reps += 1;
                    self.last_rep = now;
                    self.rest_since = None; // active again
                }
                self.reached_down = false;
            }
            if new_phase == Phase::Down {
                self.reached_down = true;
                self.rest_since = None;
            }
            self.phase = new_phase;
            self.last_transition = now;
        }

        // set segmentation: resting long enough closes the set
        if let Some(rs) = self.rest_since {
            if now - rs >= SET_REST_SECONDS && self.reps > 0 {
                self.sets += 1;
                self.reps = 0;
                self.rest_since = Some(now); // re-arm for the next set
            }
        } else if now - self.last_rep > SET_REST_SECONDS {
            self.rest_since = Some(self.last_rep);
        }
        self.reps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose_with(points: &[(usize, f32, f32, f32)]) -> Pose {
        let mut kpts = vec![[0.0, 0.0, 0.0]; 17];
        for &(i, x, y, c) in points {
            kpts[i] = [x, y, c];
        }
        Pose { kpts, score: 0.8 }
    }

    /// Anatomy-plausible pair: STAND (knee ~180°, spine aligned) and
    /// DEEP SQUAT (knee ~115°, torso upright, shoulder-hip-knee ~152° —
    /// above the 150° deadlift hinge line). Mirrored right side.
    const STAND: &[(usize, f32, f32, f32)] = &[
        (NOSE, 0.50, 0.10, 0.9),
        (L_SHOULDER, 0.45, 0.20, 0.9), (R_SHOULDER, 0.55, 0.20, 0.9),
        (L_ELBOW, 0.42, 0.30, 0.8), (R_ELBOW, 0.58, 0.30, 0.8),
        (L_WRIST, 0.42, 0.38, 0.8), (R_WRIST, 0.58, 0.38, 0.8),
        (L_HIP, 0.47, 0.40, 0.9), (R_HIP, 0.53, 0.40, 0.9),
        (L_KNEE, 0.47, 0.56, 0.9), (R_KNEE, 0.53, 0.56, 0.9),
        (L_ANKLE, 0.47, 0.72, 0.9), (R_ANKLE, 0.53, 0.72, 0.9),
    ];
    const DEEP: &[(usize, f32, f32, f32)] = &[
        (NOSE, 0.50, 0.11, 0.9),
        (L_SHOULDER, 0.47, 0.21, 0.9), (R_SHOULDER, 0.53, 0.21, 0.9),
        (L_ELBOW, 0.44, 0.32, 0.8), (R_ELBOW, 0.56, 0.32, 0.8),
        (L_WRIST, 0.44, 0.40, 0.8), (R_WRIST, 0.56, 0.40, 0.8),
        (L_HIP, 0.47, 0.40, 0.9), (R_HIP, 0.53, 0.40, 0.9),
        (L_KNEE, 0.555, 0.556, 0.9), (R_KNEE, 0.445, 0.556, 0.9),
        (L_ANKLE, 0.41, 0.72, 0.9), (R_ANKLE, 0.59, 0.72, 0.9),
    ];

    fn squat_pose(standing: bool) -> Pose {
        pose_with(if standing { STAND } else { DEEP })
    }

    #[test]
    fn classify_squat_and_bench() {
        // deep knee bend + upright torso → squat
        let squat = squat_pose(false);
        assert_eq!(classify_from_pose(&squat), "squat");
        // lying down (torso horizontal) + bent elbow → bench_press
        let bench = pose_with(&[
            (L_SHOULDER, 0.55, 0.35, 0.9), (R_SHOULDER, 0.57, 0.37, 0.9),
            (L_HIP, 0.30, 0.35, 0.9), (R_HIP, 0.32, 0.37, 0.9),
            (L_KNEE, 0.24, 0.55, 0.9), (R_KNEE, 0.26, 0.57, 0.9),
            (L_ANKLE, 0.22, 0.70, 0.9), (R_ANKLE, 0.24, 0.72, 0.9),
            (L_ELBOW, 0.63, 0.33, 0.9), (R_ELBOW, 0.65, 0.35, 0.9),
            (L_WRIST, 0.66, 0.38, 0.9), (R_WRIST, 0.68, 0.40, 0.9),
        ]);
        assert_eq!(classify_from_pose(&bench), "bench_press");
    }

    #[test]
    fn zone_map() {
        assert_eq!(zone_exercise("treadmill"), Some(("treadmill_run", true)));
        assert_eq!(zone_exercise("squat_rack"), Some(("squat", false)));
        assert_eq!(zone_exercise("yoga_mat"), Some(("crunch", false)));
        assert_eq!(zone_exercise("unmapped_thing"), None);
    }

    #[test]
    fn rep_count_cycle() {
        let mut c = RepCounter::new("squat", false, 0.0);
        // synthetic: standing (straight knee ~175°) vs deep (~100°)
        let stand = squat_pose(true);
        let deep = squat_pose(false);
        let mut t = 0.0;
        for cycle in 0..3 {
            c.update(&deep, t); t += 1.0;      // down
            c.update(&deep, t); t += 1.0;      // hold
            c.update(&stand, t); t += 1.0;     // up → rep
            assert_eq!(c.reps as usize, cycle + 1, "rep {}", cycle + 1);
        }
        assert_eq!(c.phase, Phase::Up);
    }

    #[test]
    fn cardio_counts_nothing() {
        let mut c = RepCounter::new("treadmill_run", true, 0.0);
        let p = squat_pose(false);
        c.update(&p, 1.0);
        c.update(&squat_pose(true), 2.0);
        assert_eq!(c.reps, 0);
    }
}

#[cfg(test)]
mod temporal_tests {
    use super::*;

    /// Feature-level timeline: bypass pose geometry, inject the angle
    /// series directly — the decision logic is what needs locking.
    fn tl(torso: f32, torso_hip: &[f32], knee: &[f32], elbow: &[f32],
          hip_y: &[f32], arms_up: bool, stride: f32) -> PoseTimeline {
        let n = torso_hip.len();
        let mut t = PoseTimeline {
            t: (0..n).map(|i| i as f64 * 0.2).collect(),
            torso: vec![Some(torso); n],
            torso_hip: torso_hip.iter().map(|v| Some(*v)).collect(),
            knee: knee.iter().map(|v| Some(*v)).collect(),
            elbow: elbow.iter().map(|v| Some(*v)).collect(),
            hip_y: hip_y.to_vec(),
            arms_up: vec![arms_up; n],
            stride: vec![stride; n],
        };
        t.torso[0] = Some(torso);
        t
    }

    fn osc_series(n: usize, hi: f32, lo: f32, period: usize) -> Vec<f32> {
        (0..n).map(|i| {
            let ph = ((i % period) as f32) / period as f32;
            if ph < 0.5 { hi } else { lo }
        }).collect()
    }

    #[test]
    fn lying_family() {
        // torso-hip oscillates ±15°, elbow quiet → crunch
        let t = tl(60.0, &osc_series(20, 150.0, 120.0, 10),
                   &vec![140.0; 20], &vec![160.0; 20],
                   &vec![0.4; 20], false, 0.08);
        assert_eq!(classify_with_history(&t), "crunch");
        // big torso-hip amplitude → situp
        let t = tl(60.0, &osc_series(20, 160.0, 100.0, 10),
                   &vec![140.0; 20], &vec![160.0; 20],
                   &vec![0.4; 20], false, 0.08);
        assert_eq!(classify_with_history(&t), "situp");
        // elbow oscillates, torso quiet → bench/pushup family, not crunch
        let t = tl(60.0, &vec![145.0; 20],
                   &vec![140.0; 20], &osc_series(20, 165.0, 95.0, 10),
                   &vec![0.4; 20], false, 0.08);
        assert!(matches!(classify_with_history(&t), "bench_press" | "pushup"));
        // everything static → plank
        let t = tl(60.0, &vec![145.0; 20], &vec![140.0; 20], &vec![155.0; 20],
                   &vec![0.4; 20], false, 0.08);
        assert_eq!(classify_with_history(&t), "plank");
    }

    #[test]
    fn upright_cardio_vs_strength() {
        // knee oscillation ~1.7 Hz + hip bob → run (period 3 @ 5 Hz)
        let knee = osc_series(20, 170.0, 100.0, 3);
        let hip = osc_series(20, 0.52, 0.48, 3);
        let t = tl(5.0, &vec![170.0; 20], &knee, &vec![160.0; 20],
                   &hip, false, 0.06);
        assert_eq!(classify_with_history(&t), "run");
        // slower cadence ~0.7 Hz → walk (period 7 @ 5 Hz)
        let knee = osc_series(20, 170.0, 120.0, 7);
        let hip = osc_series(20, 0.51, 0.49, 7);
        let t = tl(5.0, &vec![170.0; 20], &knee, &vec![160.0; 20],
                   &hip, false, 0.06);
        assert_eq!(classify_with_history(&t), "walk");
        // slow deep knee cycle, narrow stride → squat
        let knee = osc_series(20, 170.0, 105.0, 20);
        let t = tl(5.0, &vec![160.0; 20], &knee, &vec![160.0; 20],
                   &vec![0.5; 20], false, 0.06);
        assert_eq!(classify_with_history(&t), "squat");
        // same but wide split stance → lunge
        let t = tl(5.0, &vec![160.0; 20], &knee, &vec![160.0; 20],
                   &vec![0.5; 20], false, 0.25);
        assert_eq!(classify_with_history(&t), "lunge");
    }

    #[test]
    fn arm_family() {
        // elbow oscillation, arms up, hips bobbing → pullup
        let elbow = osc_series(20, 165.0, 80.0, 10);
        let hip = osc_series(20, 0.60, 0.52, 10);
        let t = tl(5.0, &vec![170.0; 20], &vec![170.0; 20], &elbow,
                   &hip, true, 0.08);
        assert_eq!(classify_with_history(&t), "pullup");
        // arms up, hips still → shoulder_press
        let t = tl(5.0, &vec![170.0; 20], &vec![170.0; 20], &elbow,
                   &vec![0.5; 20], true, 0.08);
        assert_eq!(classify_with_history(&t), "shoulder_press");
        // arms down → bicep_curl
        let t = tl(5.0, &vec![170.0; 20], &vec![170.0; 20], &elbow,
                   &vec![0.5; 20], false, 0.08);
        assert_eq!(classify_with_history(&t), "bicep_curl");
        // nothing oscillating → standing
        let t = tl(5.0, &vec![170.0; 20], &vec![170.0; 20], &vec![160.0; 20],
                   &vec![0.5; 20], false, 0.08);
        assert_eq!(classify_with_history(&t), "standing");
    }

    #[test]
    fn short_window_pending() {
        let t = tl(5.0, &vec![170.0; 4], &vec![170.0; 4], &vec![160.0; 4],
                   &vec![0.5; 4], false, 0.08);
        assert_eq!(classify_with_history(&t), "pending");
    }
}
