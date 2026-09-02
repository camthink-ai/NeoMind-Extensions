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
        "squat" => (160.0, 110.0),        // knee angle
        "deadlift" => (160.0, 130.0),     // hip angle
        "bench_press" | "pushup" => (160.0, 100.0), // elbow
        "lat_pulldown" => (160.0, 110.0), // elbow
        "bicep_curl" => (150.0, 60.0),    // elbow
        _ => return None,                 // cardio / unknown: no reps
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
        "squat" => best((L_HIP, L_KNEE, L_ANKLE)),
        "deadlift" => best((L_SHOULDER, L_HIP, L_KNEE)),
        "bench_press" | "pushup" | "lat_pulldown" | "bicep_curl" => {
            best((L_SHOULDER, L_ELBOW, L_WRIST))
        }
        _ => None,
    }
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
        assert_eq!(zone_exercise("yoga_mat"), None);
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
