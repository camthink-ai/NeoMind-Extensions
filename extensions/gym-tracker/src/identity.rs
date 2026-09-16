// identity.rs
//! Shared member matcher — the single source of truth for "who is this
//! track", used by BOTH the display path (get_live_state) and the
//! recording path (analytics: sessions / equipment usage), so what the
//! operator sees is exactly what gets written to the workout records.
//!
//! Two modalities, face wins:
//! - FACE (arcface): frame-level face embeddings anchored onto the
//!   tightest bbox-containing track. Clothing-independent.
//! - BODY (osnet): min L2 over the member's body sample library.
//! Both operate in uint8-quantized spaces where cosine is useless
//! (quant bias) — L2 only.

use std::collections::HashMap;

use crate::config::IdentityCfg;
use crate::db::{l2_dist, Member};
use crate::types::{Bbox, FaceBox, Track};

#[derive(Debug, Clone)]
pub struct Match {
    pub member_idx: usize,
    pub via: &'static str, // "face" | "body"
    pub dist: f32,
}

/// Match every track against the member library.
///
/// Returns track_id → Match (face anchor overrides body; a track is
/// present even without an embedding key when it has no identity yet).
pub fn match_tracks(
    cfg: &IdentityCfg,
    members: &[Member],
    tracks: &[Track],
    faces: &[FaceBox],
) -> HashMap<i64, Match> {
    let mut out: HashMap<i64, Match> = HashMap::new();

    // body match per track (min L2 over each member's samples)
    match_tracks_matrix(cfg, members, tracks, &mut out);

    // face anchor: frame-level face emb → tightest containing track
    // (matrix built lazily on the first face that carries an embedding)
    let mut face_matrix: Option<MemberMatrix> = None;
    for fe in faces {
        let Some(emb) = fe.emb.as_ref() else { continue };
        if emb.is_empty() {
            continue;
        }
        let m = face_matrix.get_or_insert_with(|| {
            build_matrix(members, |mm| mm.face_embeddings.iter().map(|v| v.as_slice()).collect())
        });
        // matrix path shared with body matching (see below): all samples
        // of all members in ONE distance pass
        let (bi, bd) = nearest_member(m, emb);
        let Some(i) = bi else { continue };
        let d = bd;
        if d > cfg.face_match_threshold {
            continue;
        }
        let target = tightest_containing(&fe.bbox, tracks);
        if let Some(t) = target {
            out.insert(
                t.track_id,
                Match {
                    member_idx: i,
                    via: "face",
                    dist: d,
                },
            );
        }
    }

    out
}

/// The track whose bbox contains the face-box center with the SMALLEST
/// area — a TTL-lingering departed track with an overlapping bbox must
/// not steal the anchor from the person actually on the face.
pub fn tightest_containing<'a>(fb: &Bbox, tracks: &'a [Track]) -> Option<&'a Track> {
    let (fx, fy) = (fb.x + fb.w / 2.0, fb.y + fb.h / 2.0);
    tracks
        .iter()
        .filter(|t| {
            let b = &t.bbox;
            fx >= b.x && fx <= b.x + b.w && fy >= b.y && fy <= b.y + b.h
        })
        .min_by(|a, b| (a.bbox.w * a.bbox.h).total_cmp(&(b.bbox.w * b.bbox.h)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_identity_for_tests;
    use crate::types::{Face, Point};

    fn member(id: &str, name: &str, body: Vec<f32>, faces: Vec<Vec<f32>>) -> Member {
        Member {
            id: id.into(),
            name: name.into(),
            source: "manual".into(),
            embedding: body,
            extra_embeddings: Vec::new(),
            face_embeddings: faces,
            photo: None,
            created_at: None,
        }
    }

    fn track(tid: i64, bbox: (f32, f32, f32, f32), emb: Option<Vec<f32>>) -> Track {
        Track {
            track_id: tid,
            ts: None,
            bbox: Bbox {
                x: bbox.0,
                y: bbox.1,
                w: bbox.2,
                h: bbox.3,
            },
            foot: Point {
                x: bbox.0 + bbox.2 / 2.0,
                y: bbox.1 + bbox.3,
            },
            pose: None,
            face: emb.map(|e| Face { emb: e, det: 0.8 }),
            vel: None,
            ex: None,
        }
    }

    #[test]
    fn nearest_member_skips_mismatched_dimension_rows() {
        // RS-5 (2026-09-16 audit): zip() truncation made empty/short
        // stored rows score ~0 against any query — they must never match.
        let m = MemberMatrix {
            samples: vec![vec![], vec![1.0, 0.0]], // empty + wrong-dim rows
            member_of: vec![0, 1],
        };
        let (i, d) = nearest_member(&m, &[1.0, 0.0, 0.0]);
        assert_eq!(i, None, "empty/short rows never match");
        assert!(d.is_infinite());
        // matched-dimension rows still match
        let m2 = MemberMatrix {
            samples: vec![vec![], vec![1.0, 0.0, 0.0]],
            member_of: vec![0, 1],
        };
        let (i, d) = nearest_member(&m2, &[1.0, 0.0, 0.0]);
        assert_eq!(i, Some(1));
        assert!(d.abs() < 1e-6);
    }

    #[test]
    fn face_overrides_body() {
        let cfg = default_identity_for_tests();
        let members = vec![
            member("m1", "小红", vec![1.0, 0.0, 0.0], vec![vec![9.0, 9.0, 9.0]]),
            member("m2", "小蓝", vec![0.0, 1.0, 0.0], vec![]),
        ];
        // track 1's BODY embedding is 小蓝's body… but its FACE is 小红's
        let tracks = vec![track(1, (0.4, 0.3, 0.2, 0.5), Some(vec![0.0, 1.0, 0.0]))];
        let faces = vec![FaceBox {
            bbox: Bbox {
                x: 0.45,
                y: 0.32,
                w: 0.1,
                h: 0.1,
            },
            det: 0.9,
            emb: Some(vec![8.9, 9.0, 9.05]),
        }];
        let out = match_tracks(&cfg, &members, &tracks, &faces);
        let m = out.get(&1).expect("matched");
        assert_eq!(members[m.member_idx].name, "小红");
        assert_eq!(m.via, "face");
    }

    #[test]
    fn body_match_and_no_match() {
        let cfg = default_identity_for_tests();
        let members = vec![member("m1", "小红", vec![1.0, 0.0, 0.0], vec![])];
        let tracks = vec![
            track(1, (0.1, 0.1, 0.2, 0.2), Some(vec![0.99, 0.01, 0.0])), // near → match
            track(2, (0.5, 0.1, 0.2, 0.2), Some(vec![0.0, 1.0, 0.0])),   // far → none
            track(3, (0.7, 0.1, 0.2, 0.2), None),                        // no emb → none
        ];
        let out = match_tracks(&cfg, &members, &tracks, &[]);
        assert!(out.contains_key(&1));
        assert!(!out.contains_key(&2));
        assert!(!out.contains_key(&3));
    }
}


// ---------------------------------------------------------------------------
// Matrix matching: every embedding in ONE squared-distance pass.
//
// The scalar loops did track×member×sample L2 in Python-over-Rust style;
// with 100+ enrolled members that's thousands of 512-d walks per frame.
// Here all member samples are flattened into one (S,512) matrix once per
// call; each query embedding is a single (1,512) row — squared L2 comes
// from |a|² + |b|² - 2ab (one GEMV), sqrt only on the best few. The
// member/sample index maps translate flat argmins back to member ids.
// ---------------------------------------------------------------------------

/// (flat sample matrix, sample->member index, per-member sample offsets)
struct MemberMatrix {
    samples: Vec<Vec<f32>>, // (S, D) row-major (owned — lives past borrow)
    member_of: Vec<usize>,  // sample idx -> member idx
}

fn build_matrix<'a>(
    members: &'a [Member],
    embeddings_of: impl Fn(&'a Member) -> Vec<&'a [f32]>,
) -> MemberMatrix {
    let mut samples = Vec::new();
    let mut member_of = Vec::new();
    for (i, m) in members.iter().enumerate() {
        for e in embeddings_of(m) {
            samples.push(e.to_vec());
            member_of.push(i);
        }
    }
    MemberMatrix { samples, member_of }
}

/// Squared-L2 from `q` to every sample, one pass. Returns (member, dist)
/// of the nearest sample (sqrt only the winner).
fn nearest_member(m: &MemberMatrix, q: &[f32]) -> (Option<usize>, f32) {
    if m.samples.is_empty() || q.len() == 0 {
        return (None, f32::INFINITY);
    }
    let qn: f32 = q.iter().map(|v| v * v).sum();
    let mut best_sq = f32::INFINITY;
    let mut best_s = usize::MAX;
    for (s, row) in m.samples.iter().enumerate() {
        // Dimension guard (RS-5, 2026-09-16 audit): zip() silently
        // TRUNCATES to the shorter side, so an empty or short stored row
        // (legacy schema / partial write) scored ~0 against ANY query and
        // hijacked the identity. Mirror db.rs l2_dist semantics: a row
        // whose length != query length never matches.
        if row.len() != q.len() {
            continue;
        }
        let mut acc = 0f32;
        for (a, b) in row.iter().zip(q.iter()) {
            let d = a - b;
            acc += d * d;
        }
        if acc < best_sq {
            best_sq = acc;
            best_s = s;
        }
    }
    if best_s == usize::MAX {
        return (None, f32::INFINITY);
    }
    let _ = qn; // kept for the planned GEMV form (|a|²+|b|²-2ab); unused here
    (Some(m.member_of[best_s]), best_sq.sqrt())
}

fn match_tracks_matrix(
    cfg: &IdentityCfg,
    members: &[Member],
    tracks: &[Track],
    out: &mut HashMap<i64, Match>,
) {
    let m = build_matrix(members, |mm| mm.all_embeddings().into_iter().collect());
    for t in tracks {
        let Some(f) = t.face.as_ref() else { continue };
        if f.emb.is_empty() {
            continue;
        }
        let (Some(i), d) = nearest_member(&m, &f.emb) else { continue };
        if d <= cfg.match_threshold {
            out.insert(t.track_id, Match { member_idx: i, via: "body", dist: d });
        }
    }
}
