use serde::{Deserialize, Serialize};

/// Axis-aligned bounding box in normalized image coordinates (0..1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A 2D point in normalized image coordinates (0..1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Person pose: 17 COCO keypoints as `[x, y, score]` plus an aggregate score.
/// `kpts.len() == 17` when present, but not enforced at the type level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub kpts: Vec<[f32; 3]>,
    pub score: f32,
}

/// Face embedding + detection score for member identity (re-id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub emb: Vec<f32>,
    pub det: f32,
}

/// Frame-level face box from the secondary detector (P2 mosaic pipeline).
/// Frame-level, not per-track: the 4-class detector reports faces
/// independently of pose tracks, and association is left to consumers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceBox {
    pub bbox: Bbox,
    pub det: f32,
    /// arcface identity embedding (present only when the face was large
    /// enough to embed — near-field faces). Absent on older producers.
    #[serde(default)]
    pub emb: Option<Vec<f32>>,
}

/// A single tracked person within a frame.
///
/// `pose` and `face` are nullable per-frame: the P1 device-app only performs
/// person detection, so both will be `None` initially. Later phases populate
/// them when pose/face models run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub track_id: i64,
    pub bbox: Bbox,
    pub foot: Point,
    pub pose: Option<Pose>,
    pub face: Option<Face>,
    /// Tracker-smoothed velocity, normalized units per SECOND (absent on
    /// pre-vel producers). The Monitor extrapolates positions with it when
    /// the picture runs ahead of the track stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vel: Option<[f32; 2]>,
    /// Per-track TRUE capture timestamp (ns). Present on newer producers:
    /// far-field tile-sourced tracks carry the TILE grab time (0.4-0.9 s
    /// older than the frame ts) — keying the overlay history by it removes
    /// the systematic far-person box trail. Absent on old producers, where
    /// the frame-level ts applies to every track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<u64>,
    /// Exercise analytics bundle from the device's exercise engine
    /// (rep FSM + windowed joint-angle statistics): reps, squat depth,
    /// left/right symmetry, tempo. Present only on newer producers and
    /// only for tracks with enough window data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ex: Option<ExerciseMetrics>,
}

/// Windowed exercise metrics riding on a track (see device exercise.py).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExerciseMetrics {
    pub reps: u32,
    pub depth_deg: Option<f32>,
    pub symmetry_deg: Option<f32>,
    pub tempo_hz: Option<f32>,
    pub knee_min_deg: Option<f32>,
}

/// One frame of `gym/track` events published by the NE503 device-app.
///
/// This is the shared contract between the Python device-app and this Rust
/// extension. `ts_ns` is a u64 of Unix nanoseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackFrame {
    pub device_id: String,
    pub frame_seq: u64,
    pub ts_ns: u64,
    pub tracks: Vec<Track>,
    /// Absent on frames from pre-P2 producers (serde default keeps old
    /// payloads parsing).
    #[serde(default)]
    pub faces: Vec<FaceBox>,
    /// Downscaled JPEG preview (base64) of the exact frame — set by newer
    /// producers (PREVIEW=1). Old payloads parse without it.
    #[serde(default)]
    pub img_b64: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_track_frame() {
        let json = serde_json::json!({
            "device_id": "ne503-001",
            "frame_seq": 12345,
            "ts_ns": 1717545600000000000u64,
            "tracks": [{
                "track_id": 7,
                "bbox": {"x": 0.31, "y": 0.22, "w": 0.27, "h": 0.61},
                "foot": {"x": 0.445, "y": 0.83},
                "pose": null,
                "face": null
            }]
        });
        let f: TrackFrame = serde_json::from_value(json).unwrap();
        assert_eq!(f.device_id, "ne503-001");
        assert_eq!(f.frame_seq, 12345);
        assert_eq!(f.ts_ns, 1717545600000000000);
        assert_eq!(f.tracks.len(), 1);
        assert_eq!(f.tracks[0].track_id, 7);
        assert_eq!(f.tracks[0].bbox.w, 0.27);
        assert_eq!(f.tracks[0].foot.y, 0.83);
        assert!(f.tracks[0].pose.is_none());
        assert!(f.tracks[0].face.is_none());

        let back = serde_json::to_value(&f).unwrap();
        assert_eq!(back["device_id"], "ne503-001");
        assert_eq!(back["frame_seq"], 12345);
        assert_eq!(back["tracks"][0]["track_id"], 7);
        assert_eq!(back["tracks"][0]["pose"], serde_json::Value::Null);
    }

    #[test]
    fn roundtrip_with_pose_and_face() {
        let json = serde_json::json!({
            "device_id": "ne503-002",
            "frame_seq": 99,
            "ts_ns": 1717545600000000123u64,
            "tracks": [{
                "track_id": 3,
                "bbox": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4},
                "foot": {"x": 0.25, "y": 0.6},
                "pose": {
                    "kpts": [[0.5, 0.6, 0.9], [0.4, 0.5, 0.8]],
                    "score": 0.85
                },
                "face": {
                    "emb": [0.1, 0.2, 0.3],
                    "det": 0.77
                }
            }]
        });
        let f: TrackFrame = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(f.tracks.len(), 1);
        let pose = f.tracks[0].pose.as_ref().expect("pose present");
        assert_eq!(pose.kpts.len(), 2);
        assert_eq!(pose.kpts[0], [0.5, 0.6, 0.9]);
        assert_eq!(pose.score, 0.85);
        let face = f.tracks[0].face.as_ref().expect("face present");
        assert_eq!(face.emb, vec![0.1, 0.2, 0.3]);
        assert_eq!(face.det, 0.77);

        let back: TrackFrame = serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn missing_optional_fields_default_to_none() {
        // pose/face absent from JSON should deserialize as None.
        let json = serde_json::json!({
            "device_id": "ne503-003",
            "frame_seq": 1,
            "ts_ns": 0u64,
            "tracks": [{
                "track_id": 0,
                "bbox": {"x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0},
                "foot": {"x": 0.5, "y": 1.0}
            }]
        });
        let f: TrackFrame = serde_json::from_value(json).unwrap();
        assert!(f.tracks[0].pose.is_none());
        assert!(f.tracks[0].face.is_none());
        assert!(f.faces.is_empty());
    }

    #[test]
    fn frame_level_faces_parse_and_default() {
        // Absent field → empty vec (pre-P2 producer compatibility).
        let legacy = serde_json::json!({
            "device_id": "ne503-004",
            "frame_seq": 5,
            "ts_ns": 1u64,
            "tracks": []
        });
        let f: TrackFrame = serde_json::from_value(legacy).unwrap();
        assert!(f.faces.is_empty());

        // P2 frame: faces alongside tracks, det-scored bboxes.
        let json = serde_json::json!({
            "device_id": "ne503-004",
            "frame_seq": 6,
            "ts_ns": 2u64,
            "tracks": [],
            "faces": [{
                "bbox": {"x": 0.1, "y": 0.05, "w": 0.08, "h": 0.12},
                "det": 0.87,
                "emb": [1.0, 0.5]
            }]
        });
        let f: TrackFrame = serde_json::from_value(json).unwrap();
        assert_eq!(f.faces.len(), 1);
        assert_eq!(f.faces[0].bbox.w, 0.08);
        assert_eq!(f.faces[0].det, 0.87);
        assert_eq!(f.faces[0].emb.as_deref(), Some(&[1.0, 0.5][..]));

        let back: TrackFrame = serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap();
        assert_eq!(back, f);
    }
}
