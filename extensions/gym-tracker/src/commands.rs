// commands.rs
//! Command dispatch for the gym-tracker P1 API.
//!
//! Pure dispatch — no new I/O ownership. Each command reads from the shared
//! handles in [`Ctx`] (state / db / ne503), which Task 10 (`lib.rs`) wires up
//! from its `Inner`. The host calls [`handle`] from `execute_command` and maps
//! the `Err(String)` to `ExtensionError::Other`.
//!
//! P1 commands: `get_live_state`, `get_roi_zones`, `set_roi_zones`,
//! `get_device_status`, `get_snapshot`.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::db::{Db, Zone};
use crate::ne503::Ne503Client;
use crate::state::LiveState;

/// Dispatch context — shared handles the commands read from. `lib.rs` (Task 10)
/// builds this from its `Inner { state, db, ne }`.
pub struct Ctx {
    pub state: Arc<LiveState>,
    pub db: Arc<Db>,
    pub ne: Arc<Ne503Client>,
}

/// Handle one command. Returns a JSON value or an error string (the caller maps
/// this to `ExtensionError::Other`). No command owns I/O beyond what the
/// [`Ctx`] handles already do.
pub fn handle(ctx: &Ctx, cmd: &str, args: &Value) -> Result<Value, String> {
    match cmd {
        // Live mirror of tracks currently tracked by the device-app.
        "get_live_state" => {
            let tracks = ctx.state.snapshot();
            Ok(json!({
                "present_count": ctx.state.present_count(),
                "tracks": tracks.iter().map(|t| json!({
                    "track_id": t.track_id,
                    "bbox": t.bbox,
                    "foot": t.foot,
                })).collect::<Vec<_>>(),
            }))
        }
        // List all ROI zones (round-trips the full Zone struct incl. polygon).
        "get_roi_zones" => {
            let zones = ctx.db.list_zones().map_err(|e| e.to_string())?;
            Ok(json!({ "zones": zones }))
        }
        // Full-replace semantics: args["zones"] is the complete new zone set.
        // Zones whose id is absent from the incoming set are deleted; the rest
        // are upserted (so renames / polygon edits land in one call).
        "set_roi_zones" => {
            let zones_in: Vec<Zone> = serde_json::from_value(args["zones"].clone())
                .map_err(|e| format!("invalid zones: {e}"))?;
            let current: HashMap<String, Zone> = ctx
                .db
                .list_zones()
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|z| (z.id.clone(), z))
                .collect();
            let keep: HashSet<&str> = zones_in.iter().map(|z| z.id.as_str()).collect();
            // Delete zones no longer in the set BEFORE upserting, so a kept id
            // can safely reuse a name freed by a removed zone (zones.name is
            // UNIQUE — delete-first avoids transient collisions).
            for id in current.keys() {
                if !keep.contains(id.as_str()) {
                    ctx.db.delete_zone(id).map_err(|e| e.to_string())?;
                }
            }
            for z in &zones_in {
                ctx.db.upsert_zone(z).map_err(|e| e.to_string())?;
            }
            Ok(json!({ "saved": zones_in.len() }))
        }
        // Proxy to the NE503 REST client (network; covered by Task 7/8 live
        // tests).
        "get_device_status" => {
            let v = ctx.ne.get_device_status().map_err(|e| e.to_string())?;
            Ok(v)
        }
        // P1 stub: no single-frame REST endpoint on this firmware yet. Not
        // fatal — the dispatch maps the client error to an error object so the
        // frontend can render a placeholder. P2 will grab a frame via RTSP.
        "get_snapshot" => {
            let stream = args
                .get("stream")
                .and_then(|s| s.as_str())
                .unwrap_or("sub");
            match ctx.ne.get_snapshot(stream) {
                Ok(b64) => Ok(json!({ "image_base64": b64 })),
                Err(e) => Ok(json!({ "error": e.to_string() })),
            }
        }
        other => Err(format!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    //! Unit-test the PURE, no-network paths against an in-memory Db, a
    //! populated LiveState, and a Ne503Client pointed at an unreachable host.
    //! The no-network commands (`get_live_state` / `get_roi_zones` /
    //! `set_roi_zones` / unknown / the P1 `get_snapshot` stub) never perform an
    //! HTTP call. `get_device_status` is exercised by the Task 7/8 live tests.

    use super::*;
    use crate::config::Config;
    use crate::types::{Bbox, Point, Track, TrackFrame};

    /// Build a Ctx backed by an in-memory Db, a fresh LiveState, and a
    /// Ne503Client pointed at localhost. `Ne503Client::new` performs NO network
    /// I/O (it only constructs the agent + empty token), and the no-network
    /// commands below never call into it, so the host is irrelevant.
    fn make_ctx() -> Ctx {
        let raw = r#"{"device":{"host":"127.0.0.1","username":"x","password":"x","tls_insecure":false},"device_id":"test","ingest":{"topic":"gym/track","publish_hz":8,"track_ttl_sec":30,"reconnect_backoff_sec":[1,2,5]},"identity":{"match_threshold":0.55,"auto_capture_unknown":true,"unknown_prefix":"U"},"roi":{"dwell_debounce_sec":3,"hysteresis":true},"data_dir":"/tmp/gym-test"}"#;
        let cfg = Config::parse(raw).expect("config parse");
        Ctx {
            state: Arc::new(LiveState::new(30)),
            db: Arc::new(Db::open(":memory:").expect("db open")),
            ne: Arc::new(Ne503Client::new(&cfg)),
        }
    }

    fn track(tid: i64) -> Track {
        Track {
            track_id: tid,
            bbox: Bbox { x: 0.1, y: 0.2, w: 0.3, h: 0.4 },
            foot: Point { x: 0.25, y: 0.6 },
            pose: None,
            face: None,
        }
    }

    fn frame(tracks: Vec<Track>) -> TrackFrame {
        TrackFrame { device_id: "d".into(), frame_seq: 1, ts_ns: 0, tracks }
    }

    #[test]
    fn get_live_state_returns_count_and_tracks() {
        let ctx = make_ctx();
        ctx.state.apply_frame(&frame(vec![track(7), track(9)]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["present_count"].as_i64(), Some(2));

        let tracks = out["tracks"].as_array().expect("tracks array");
        assert_eq!(tracks.len(), 2);
        // snapshot() iterates a HashMap → order is not stable; key by track_id.
        let by_id: HashMap<i64, &Value> = tracks
            .iter()
            .map(|t| (t["track_id"].as_i64().unwrap(), t))
            .collect();
        assert!(by_id.contains_key(&7) && by_id.contains_key(&9), "tids={by_id:?}");

        // shape: each track carries bbox + foot (f32 → f64, so compare approx).
        let t7 = by_id[&7];
        let w = t7["bbox"]["w"].as_f64().expect("bbox.w number");
        assert!((w - 0.3).abs() < 1e-5, "bbox.w={w}");
        let fx = t7["foot"]["x"].as_f64().expect("foot.x number");
        assert!((fx - 0.25).abs() < 1e-5, "foot.x={fx}");
    }

    #[test]
    fn get_live_state_empty_when_no_tracks() {
        let ctx = make_ctx();
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["present_count"].as_i64(), Some(0));
        assert_eq!(out["tracks"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn get_roi_zones_round_trips() {
        let ctx = make_ctx();
        let z = Zone {
            id: "z1".into(),
            name: "Treadmill".into(),
            equipment_type: "treadmill".into(),
            polygon: vec![(0.1, 0.1), (0.4, 0.1), (0.4, 0.5), (0.1, 0.5)],
            enabled: true,
        };
        ctx.db.upsert_zone(&z).expect("upsert");

        let out = handle(&ctx, "get_roi_zones", &json!({})).expect("ok");
        let zones = out["zones"].as_array().expect("zones array");
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0]["id"], "z1");
        assert_eq!(zones[0]["name"], "Treadmill");
        assert_eq!(zones[0]["equipment_type"], "treadmill");
        assert_eq!(zones[0]["enabled"], true);
        // polygon: Vec<(f32,f32)> serializes as array of 2-element arrays.
        let poly = zones[0]["polygon"].as_array().expect("polygon array");
        assert_eq!(poly.len(), 4);
        assert_eq!(poly[0].as_array().unwrap().len(), 2);
    }

    #[test]
    fn get_roi_zones_empty_when_none() {
        let ctx = make_ctx();
        let out = handle(&ctx, "get_roi_zones", &json!({})).expect("ok");
        assert_eq!(out["zones"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn set_roi_zones_full_replace() {
        let ctx = make_ctx();

        // Seed with two zones via set_roi_zones.
        let two = json!({"zones": [
            { "id": "a", "name": "Zone A", "equipment_type": "treadmill",
              "polygon": [[0.1,0.1],[0.4,0.1],[0.4,0.5],[0.1,0.5]], "enabled": true },
            { "id": "b", "name": "Zone B", "equipment_type": "bench",
              "polygon": [[0.5,0.5],[0.9,0.5],[0.9,0.9],[0.5,0.9]], "enabled": true },
        ]});
        let r1 = handle(&ctx, "set_roi_zones", &two).expect("seed ok");
        assert_eq!(r1["saved"].as_i64(), Some(2));
        assert_eq!(ctx.db.list_zones().unwrap().len(), 2);

        // Full-replace with a single DIFFERENT id → a & b deleted, c kept.
        let one = json!({"zones": [
            { "id": "c", "name": "Zone C", "equipment_type": "squat",
              "polygon": [[0.0,0.0],[0.2,0.0],[0.2,0.2],[0.0,0.2]], "enabled": true },
        ]});
        let r2 = handle(&ctx, "set_roi_zones", &one).expect("replace ok");
        assert_eq!(r2["saved"].as_i64(), Some(1));

        // Verify via the command (not just db): only zone c remains.
        let out = handle(&ctx, "get_roi_zones", &json!({})).expect("list ok");
        let zones = out["zones"].as_array().expect("zones array");
        assert_eq!(zones.len(), 1, "only the kept zone remains");
        assert_eq!(zones[0]["id"], "c");
        let ids: Vec<&str> = zones.iter().map(|z| z["id"].as_str().unwrap()).collect();
        assert!(!ids.contains(&"a") && !ids.contains(&"b"), "removed ids gone, ids={ids:?}");
    }

    #[test]
    fn set_roi_zones_keeps_and_updates_existing_id() {
        // Replacing with a subset that includes an existing id should keep that
        // id (upsert, not delete) and apply field changes.
        let ctx = make_ctx();
        let seed = json!({"zones": [
            { "id": "a", "name": "Old", "equipment_type": "treadmill",
              "polygon": [[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]], "enabled": true },
            { "id": "b", "name": "Gone", "equipment_type": "bench",
              "polygon": [[0.0,0.0],[0.1,0.0],[0.1,0.1],[0.0,0.1]], "enabled": true },
        ]});
        handle(&ctx, "set_roi_zones", &seed).expect("seed ok");

        // Keep "a" (renamed + disabled), drop "b".
        let replace = json!({"zones": [
            { "id": "a", "name": "New", "equipment_type": "treadmill",
              "polygon": [[0.0,0.0],[0.5,0.0],[0.5,0.5],[0.0,0.5]], "enabled": false },
        ]});
        handle(&ctx, "set_roi_zones", &replace).expect("replace ok");

        let zones = ctx.db.list_zones().unwrap();
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].id, "a");
        assert_eq!(zones[0].name, "New", "upsert applied the rename");
        assert!(!zones[0].enabled, "upsert applied enabled=false");
    }

    #[test]
    fn set_roi_zones_rejects_missing_field() {
        let ctx = make_ctx();
        // No "zones" key → args["zones"] is Null → deser fails → Err.
        let r = handle(&ctx, "set_roi_zones", &json!({}));
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert!(e.contains("invalid zones"), "err={e}");
    }

    #[test]
    fn get_snapshot_returns_error_object_in_p1() {
        // P1 stub: get_snapshot short-circuits to Err on the client (no HTTP).
        // The dispatch maps that to a non-fatal { "error": ... } object, which
        // is exactly the shape the frontend relies on for the placeholder UI.
        let ctx = make_ctx();
        let out = handle(&ctx, "get_snapshot", &json!({"stream":"sub"})).expect("ok");
        assert!(out.get("error").is_some(), "expected error field, got {out}");
        assert!(out.get("image_base64").is_none());
    }

    #[test]
    fn get_snapshot_defaults_stream_to_sub() {
        // No "stream" arg → defaults to "sub"; still returns the error object
        // (proves the default branch is taken without panicking on None).
        let ctx = make_ctx();
        let out = handle(&ctx, "get_snapshot", &json!({})).expect("ok");
        assert!(out.get("error").is_some());
    }

    #[test]
    fn unknown_command_errors() {
        let ctx = make_ctx();
        let r = handle(&ctx, "frobnicate", &json!({}));
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert!(e.contains("unknown command"), "err={e}");
        assert!(e.contains("frobnicate"), "err={e}");
    }
}
