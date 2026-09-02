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
    for t in tracks {
        let Some(f) = t.face.as_ref() else { continue };
        if f.emb.is_empty() {
            continue;
        }
        let mut best: Option<(usize, f32)> = None;
        for (i, m) in members.iter().enumerate() {
            for sample in m.all_embeddings() {
                let d = l2_dist(&f.emb, sample);
                if d.is_finite() && best.map_or(true, |(_, b)| d < b) {
                    best = Some((i, d));
                }
            }
        }
        if let Some((i, d)) = best.filter(|(_, d)| *d <= cfg.match_threshold) {
            out.insert(t.track_id, Match { member_idx: i, via: "body", dist: d });
        }
    }

    // face anchor: frame-level face emb → tightest containing track
    for fe in faces {
        let Some(emb) = fe.emb.as_ref() else { continue };
        if emb.is_empty() {
            continue;
        }
        let mut best: Option<(usize, f32)> = None;
        for (i, m) in members.iter().enumerate() {
            for sample in &m.face_embeddings {
                let d = l2_dist(emb, sample);
                if d.is_finite() && best.map_or(true, |(_, b)| d < b) {
                    best = Some((i, d));
                }
            }
        }
        let Some((i, d)) = best.filter(|(_, d)| *d <= cfg.face_match_threshold) else {
            continue;
        };
        let target = tightest_containing(&fe.bbox, tracks);
        if let Some(t) = target {
            out.insert(t.track_id, Match { member_idx: i, via: "face", dist: d });
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
        .min_by(|a, b| {
            (a.bbox.w * a.bbox.h).total_cmp(&(b.bbox.w * b.bbox.h))
        })
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
            created_at: None,
        }
    }

    fn track(tid: i64, bbox: (f32, f32, f32, f32), emb: Option<Vec<f32>>) -> Track {
        Track {
            track_id: tid,
            bbox: Bbox { x: bbox.0, y: bbox.1, w: bbox.2, h: bbox.3 },
            foot: Point { x: bbox.0 + bbox.2 / 2.0, y: bbox.1 + bbox.3 },
            pose: None,
            face: emb.map(|e| Face { emb: e, det: 0.8 }),
        }
    }

    #[test]
    fn face_overrides_body() {
        let cfg = default_identity_for_tests();
        let members = vec![
            member("m1", "小红", vec![1.0, 0.0, 0.0], vec![vec![9.0, 9.0, 9.0]]),
            member("m2", "小蓝", vec![0.0, 1.0, 0.0], vec![]),
        ];
        // track 1's BODY embedding is 小蓝's body… but its FACE is 小红's
        let tracks = vec![
            track(1, (0.4, 0.3, 0.2, 0.5), Some(vec![0.0, 1.0, 0.0])),
        ];
        let faces = vec![FaceBox {
            bbox: Bbox { x: 0.45, y: 0.32, w: 0.1, h: 0.1 },
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
            track(2, (0.5, 0.1, 0.2, 0.2), Some(vec![0.0, 1.0, 0.0])),  // far → none
            track(3, (0.7, 0.1, 0.2, 0.2), None),                       // no emb → none
        ];
        let out = match_tracks(&cfg, &members, &tracks, &[]);
        assert!(out.contains_key(&1));
        assert!(!out.contains_key(&2));
        assert!(!out.contains_key(&3));
    }
}
