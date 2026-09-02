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

use crate::analytics::Analytics;
use crate::config::IdentityCfg;
use crate::db::{l2_dist, Db, Zone};
use crate::metrics::Metrics;
use crate::ne503::Ne503Client;
use crate::state::LiveState;

/// Dispatch context — shared handles the commands read from. `lib.rs` (Task 10)
/// builds this from its `Inner { state, db, ne }`.
pub struct Ctx {
    pub state: Arc<LiveState>,
    pub db: Arc<Db>,
    pub ne: Arc<Ne503Client>,
    /// Per-zone metric registry. `set_roi_zones` calls `sync_zones` on this so a
    /// zone-set change lands in the descriptors / produced values immediately,
    /// instead of waiting for the next `configure`.
    pub metrics: Arc<Metrics>,
    /// Trails / line crossings / heatmap accumulators (P1).
    pub analytics: Arc<Analytics>,
    /// Identity matching knobs (identity.* from the extension config).
    pub identity: IdentityCfg,
}

/// Handle one command. Returns a JSON value or an error string (the caller maps
/// this to `ExtensionError::Other`). No command owns I/O beyond what the
/// [`Ctx`] handles already do.
pub fn handle(ctx: &Ctx, cmd: &str, args: &Value) -> Result<Value, String> {
    match cmd {
        // Live mirror of tracks currently tracked by the device-app.
        "get_live_state" => {
            // Evict departed tracks (last_seen past TTL) before reading, so the
            // count / list reflect who's actually in-frame. The device-app owns
            // track_id assignment; we only mirror, so a person who leaves stops
            // being republished and ages out here (TTL = ingest.track_ttl_sec).
            let _ = ctx.state.evict_expired();
            let tracks = ctx.state.snapshot();
            let trails = ctx.analytics.get_trails(40);
            // Frame-level face boxes for the P2 mosaic (empty when the
            // producer stopped sending faces — the frontend then skips
            // mosaicking rather than acting on stale geometry).
            let faces = ctx.state.snapshot_faces().unwrap_or_default();
            // P3 member recognition: nearest member by L2 distance in the
            // osnet uint8-quantized space (cosine is useless there — quant
            // bias pins unrelated inputs at ~0.999), matched when the
            // distance is within identity.match_threshold. Members load per
            // poll — tiny (a gym's worth of rows) and always fresh.
            let mut members = ctx.db.list_members().map_err(|e| e.to_string())?;
            // (free fns, not closures: the borrow checker otherwise pins
            // `members` immutable while the auto-enroll loop pushes to it)
            let nearest_member = |ems: &[crate::db::Member], emb: &[f32]| -> Option<(usize, f32)> {
                let mut best: Option<(usize, f32)> = None;
                for (i, m) in ems.iter().enumerate() {
                    let d = l2_dist(emb, &m.embedding);
                    if d.is_finite() && best.map_or(true, |(_, b)| d < b) {
                        best = Some((i, d));
                    }
                }
                best
            };
            let match_one = |ems: &[crate::db::Member], emb: &[f32]| -> Option<(String, String, f32)> {
                nearest_member(ems, emb)
                    .filter(|(_, d)| *d <= ctx.identity.match_threshold)
                    .map(|(i, d)| (ems[i].id.clone(), ems[i].name.clone(), d))
            };
            // ---- auto-enrollment (identity.auto_capture_unknown) ----
            // A track with an embedding whose nearest member sits BEYOND
            // auto_capture_distance is a first-time visitor: enroll their
            // embedding now as `{prefix}-{n}` (source=auto, unnamed) so the
            // identity is captured on first sight; the display name is
            // filled in later via rename_member. The wide gap between
            // match_threshold (450) and auto_capture_distance (600) absorbs
            // embedding drift for people already in the library — an
            // enrolled person whose re-embed lands at 500 is "unknown but
            // not enrollable", not a duplicate entry.
            if ctx.identity.auto_capture_unknown {
                for t in &tracks {
                    let Some(f) = t.face.as_ref() else { continue };
                    if f.emb.is_empty() {
                        continue;
                    }
                    let enrollable = nearest_member(&members, &f.emb)
                        .map_or(true, |(_, d)| d > ctx.identity.auto_capture_distance);
                    if !enrollable {
                        continue;
                    }
                    match ctx.db.insert_auto_member(
                        &ctx.identity.unknown_prefix, &f.emb) {
                        Ok(m) => members.push(m),
                        Err(e) => tracing::warn!(
                            member_err = %e, "auto member insert failed"),
                    }
                }
            }
            // Opportunistic heatmap persistence — polls are frequent enough
            // that this bounds dirty-frame loss to one poll interval.
            ctx.analytics.maybe_save(&ctx.db);
            Ok(json!({
                "present_count": ctx.state.present_count(),
                "tracks": tracks.iter().map(|t| {
                    // emb is large (512 f32) — expose the matched member, not
                    // the raw vector; register_member reads it from state.
                    let member = t.face.as_ref()
                        .and_then(|f| match_one(&members, &f.emb))
                        .map(|(id, name, dist)| json!({
                            "id": id, "name": name,
                            "dist": (dist * 1000.0).round() / 1000.0,
                        }));
                    json!({
                        "track_id": t.track_id,
                        "bbox": t.bbox,
                        "foot": t.foot,
                        // Pose/face round-trip the producer's optional fields so
                        // debug tooling (and P2 rep counting) can see them without
                        // reading the ingest stream directly. Null when the
                        // device-app didn't supply them.
                        "pose": t.pose,
                        "has_emb": t.face.as_ref().map_or(false, |f| !f.emb.is_empty()),
                        // Matched member (P3) or null.
                        "member": member,
                        // Fading-tail points (oldest first) for the Monitor overlay.
                        "trail": trails.get(&t.track_id).cloned().unwrap_or_default(),
                    })
                }).collect::<Vec<_>>(),
                "faces": faces,
                "members_count": members.len(),
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
            // Re-sync the per-zone metric registry to the persisted zone set so a
            // runtime add / remove / rename lands in the descriptors + produced
            // values immediately. Without this the registry stayed frozen at the
            // set seeded by configure() until the next reload.
            ctx.metrics
                .sync_zones(&ctx.db.list_zones().map_err(|e| e.to_string())?);
            Ok(json!({ "saved": zones_in.len() }))
        }
        // Proxy to the NE503 REST client (network; covered by Task 7/8 live
        // tests).
        "get_device_status" => {
            let v = ctx.ne.get_device_status().map_err(|e| e.to_string())?;
            Ok(v)
        }
        // ---- P1: line crossing (出入口) ----
        "get_lines" => Ok(json!({ "lines": ctx.analytics.get_lines() })),
        "set_lines" => {
            let lines: Vec<crate::analytics::CrossLine> =
                serde_json::from_value(args["lines"].clone())
                    .map_err(|e| format!("invalid lines: {e}"))?;
            let n = lines.len();
            ctx.analytics.set_lines(&ctx.db, lines)?;
            Ok(json!({ "saved": n }))
        }
        "get_crossings" => Ok(json!({ "lines": ctx.analytics.get_crossings() })),
        // ---- P1: heatmap ----
        "get_heatmap" => Ok(ctx.analytics.get_heatmap()),
        // ---- P3: member library (body-ReID via osnet embeddings) ----
        // Register the CURRENT embedding of a live track as a named member.
        // Takes the track's latest face.emb from the mirror — the device
        // refreshes it every reid_interval (2 s), so a person standing in
        // view for a few seconds has a fresh embedding.
        "register_member" => {
            let track_id: i64 = args["track_id"].as_i64()
                .ok_or("register_member: missing track_id")?;
            let name = args["name"].as_str().map(str::trim)
                .ok_or("register_member: missing name")?;
            if name.is_empty() {
                return Err("register_member: name is empty".into());
            }
            let _ = ctx.state.evict_expired();
            let track = ctx.state.snapshot().into_iter()
                .find(|t| t.track_id == track_id)
                .ok_or_else(|| format!("track {track_id} not in frame"))?;
            let emb = track.face.as_ref().filter(|f| !f.emb.is_empty())
                .ok_or("track has no embedding yet — wait a few seconds and retry")?;
            let id = format!("member_{}", uuid::Uuid::new_v4().simple());
            ctx.db.insert_member(&id, name, &emb.emb).map_err(|e| e.to_string())?;
            Ok(json!({ "member": { "id": id, "name": name, "dim": emb.emb.len() } }))
        }
        "list_members" => {
            let members = ctx.db.list_members().map_err(|e| e.to_string())?;
            Ok(json!({ "members": members.iter().map(|m| json!({
                "id": m.id, "name": m.name, "dim": m.embedding.len(),
                "source": m.source, "created_at": m.created_at,
            })).collect::<Vec<_>>() }))
        }
        // Fill in / correct a member's display name (auto-enrolled entries
        // are created unnamed — prefix-numbered — precisely so this can
        // attach the real name later).
        "rename_member" => {
            let id = args["id"].as_str()
                .ok_or("rename_member: missing id")?;
            let name = args["name"].as_str().map(str::trim)
                .ok_or("rename_member: missing name")?;
            if name.is_empty() {
                return Err("rename_member: name is empty".into());
            }
            let n = ctx.db.rename_member(id, name).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err(format!("member {id} not found"));
            }
            Ok(json!({ "renamed": id, "name": name }))
        }
        "delete_member" => {
            let id = args["id"].as_str()
                .ok_or("delete_member: missing id")?;
            let n = ctx.db.delete_member(id).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err(format!("member {id} not found"));
            }
            Ok(json!({ "deleted": id }))
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
    fn make_ctx_with_ttl(ttl_sec: u32) -> Ctx {
        make_ctx_identity(ttl_sec, r#"{ "match_threshold": 0.1, "auto_capture_unknown": false, "unknown_prefix": "U", "auto_capture_distance": 0.5 }"#)
    }

    fn make_ctx() -> Ctx {
        make_ctx_with_ttl(30)
    }

    /// Ctx with a custom identity block — auto-capture tests flip the
    /// switch and tune thresholds to unit-scale test embeddings.
    fn make_ctx_identity(ttl_sec: u32, identity_json: &str) -> Ctx {
        let raw = format!(
            r#"{{"device":{{"host":"127.0.0.1","username":"x","password":"x","tls_insecure":false}},"device_id":"test","ingest":{{"topic":"gym/track","publish_hz":8,"track_ttl_sec":30,"reconnect_backoff_sec":[1,2,5]}},"identity":{identity_json},"roi":{{"dwell_debounce_sec":3,"hysteresis":true}},"data_dir":"/tmp/gym-test"}}"#);
        let cfg = Config::parse(&raw).expect("config parse");
        let db = Arc::new(Db::open(":memory:").expect("db open"));
        let identity = cfg.identity.clone();
        Ctx {
            state: Arc::new(LiveState::new(ttl_sec)),
            analytics: Arc::new(Analytics::new(&db)),
            db,
            ne: Arc::new(Ne503Client::new(&cfg)),
            metrics: Arc::new(Metrics::new()),
            identity,
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
        TrackFrame { device_id: "d".into(), frame_seq: 1, ts_ns: 0, tracks, faces: vec![] }
    }

    #[test]
    fn auto_enroll_capture_and_rename_flow() {
        // match 0.1 / auto-capture on beyond 0.5 / prefix U (unit scale)
        let ctx = make_ctx_identity(30,
            r#"{ "match_threshold": 0.1, "auto_capture_unknown": true, "unknown_prefix": "U", "auto_capture_distance": 0.5 }"#);
        let emb = |v: Vec<f32>| Some(crate::types::Face { emb: v, det: 0.8 });

        // first visitor: empty library → auto-enrolled as U-1 on first poll
        let mut t = track(1);
        t.face = emb(vec![1.0, 0.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["members_count"], 1);
        let by = |out: &Value, tid: i64| out["tracks"].as_array().unwrap().iter()
            .find(|t| t["track_id"] == tid).unwrap().clone();
        assert_eq!(by(&out, 1)["member"]["name"], "U-1",
            "auto entry immediately matches its own person");

        // same person re-detected (new track id, near embedding): matches
        // U-1, no duplicate entry
        let mut t2 = track(2);
        t2.face = emb(vec![0.99, 0.01, 0.0]);
        ctx.state.apply_frame(&frame(vec![t2]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["members_count"], 1);
        let by = |out: &Value, tid: i64| out["tracks"].as_array().unwrap().iter()
            .find(|t| t["track_id"] == tid).unwrap().clone();
        assert_eq!(by(&out, 2)["member"]["name"], "U-1");

        // drift band (distance ~0.35, between 0.1 and 0.5): neither matched
        // nor re-enrolled — the gap absorbs embedding drift
        let mut t3 = track(3);
        t3.face = emb(vec![0.95, 0.35, 0.0]);
        ctx.state.apply_frame(&frame(vec![t3]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["members_count"], 1, "drift band must not enroll");
        let by = |out: &Value, tid: i64| out["tracks"].as_array().unwrap().iter()
            .find(|t| t["track_id"] == tid).unwrap().clone();
        assert!(by(&out, 3)["member"].is_null());

        // second visitor (orthogonal embedding, distance ~1.19 > 0.5):
        // enrolled as U-2
        let mut t4 = track(4);
        t4.face = emb(vec![0.0, 1.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t4]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["members_count"], 2);
        let names: Vec<&str> = out["tracks"].as_array().unwrap().iter()
            .map(|t| t["member"]["name"].as_str().unwrap_or(""))
            .collect();
        assert!(names.contains(&"U-2"), "second visitor enrolled: {names:?}");

        // fill in the real name afterwards
        let list = handle(&ctx, "list_members", &json!({})).unwrap();
        let u1 = list["members"].as_array().unwrap().iter()
            .find(|m| m["name"] == "U-1").unwrap();
        let id = u1["id"].as_str().unwrap().to_string();
        assert_eq!(u1["source"], "auto");
        handle(&ctx, "rename_member", &json!({ "id": id, "name": "张三" }))
            .expect("rename ok");
        let mut t5 = track(5);
        t5.face = emb(vec![1.0, 0.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t5]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        let by = |out: &Value, tid: i64| out["tracks"].as_array().unwrap().iter()
            .find(|t| t["track_id"] == tid).unwrap().clone();
        assert_eq!(by(&out, 5)["member"]["name"], "张三",
            "renamed member matches under the new name");
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
    fn member_register_match_delete_flow() {
        let ctx = make_ctx();
        // threshold 0.1 in this fixture — unit-scale test embeddings
        // track 3 carries a fresh embedding (P3 wire: face {emb, det}).
        let mut t3 = track(3);
        t3.face = Some(crate::types::Face {
            emb: vec![1.0, 0.0, 0.0, 0.5],
            det: 0.8,
        });
        ctx.state.apply_frame(&frame(vec![t3, track(4)]));

        // register track 3's embedding as 张三
        let out = handle(&ctx, "register_member",
                         &json!({ "track_id": 3, "name": "张三" })).expect("ok");
        let mid = out["member"]["id"].as_str().unwrap().to_string();
        assert_eq!(out["member"]["name"], "张三");
        assert_eq!(out["member"]["dim"], 4);

        // list shows it
        let out = handle(&ctx, "list_members", &json!({})).expect("ok");
        assert_eq!(out["members"].as_array().unwrap().len(), 1);

        // a near-identical embedding matches (L2 ≈ 0.014 ≤ 0.1); an
        // orthogonal embedding must NOT match (L2 ≈ 1.19 > 0.1)
        let mut t3b = track(3);
        t3b.face = Some(crate::types::Face {
            emb: vec![0.99, 0.01, 0.0, 0.5],
            det: 0.7,
        });
        ctx.state.apply_frame(&frame(vec![t3b]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        let t3s = out["tracks"].as_array().unwrap().iter()
            .find(|t| t["track_id"] == 3).unwrap();
        assert_eq!(t3s["member"]["name"], "张三", "re-embedding still matches");
        assert_eq!(t3s["member"]["id"], mid.as_str());
        assert_eq!(t3s["has_emb"], true);

        let mut t3c = track(3);
        t3c.face = Some(crate::types::Face {
            emb: vec![0.0, 1.0, 0.0, 0.0],
            det: 0.6,
        });
        ctx.state.apply_frame(&frame(vec![t3c]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        let t3s = out["tracks"].as_array().unwrap().iter()
            .find(|t| t["track_id"] == 3).unwrap();
        assert!(t3s["member"].is_null(), "far embedding must not match");

        // register without an embedding is a clean error
        let err = handle(&ctx, "register_member",
                         &json!({ "track_id": 4, "name": "李四" }));
        assert!(err.is_err());

        // delete → matching gone
        handle(&ctx, "delete_member", &json!({ "id": mid })).expect("ok");
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        let t3s = out["tracks"].as_array().unwrap().iter()
            .find(|t| t["track_id"] == 3).unwrap();
        assert!(t3s["member"].is_null(), "no member after delete");
        assert_eq!(out["members_count"], 0);
    }

    #[test]
    fn get_live_state_evicts_stale_tracks() {
        // TTL wiring: a track whose `last_seen` is past the TTL must be evicted
        // before present_count / the track list are read, so a person who left
        // (track_id no longer republished) doesn't keep inflating the count.
        // Before the fix, get_live_state never evicted — evict_expired()
        // existed but was only called from a unit test, so the live mirror only
        // ever grew (apply_frame upserts, nothing removed the departed).
        let ctx = make_ctx_with_ttl(0);
        ctx.state.apply_frame(&frame(vec![track(7)]));
        // ttl=0, but let the clock actually tick so `now - last_seen > 0` holds
        // (two back-to-back Instant::now() calls can land in the same tick).
        std::thread::sleep(std::time::Duration::from_millis(5));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(
            out["present_count"].as_i64(),
            Some(0),
            "stale track must be evicted before reading present_count"
        );
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
    fn set_roi_zones_resyncs_metric_registry() {
        // Zone-set changes must land in the per-zone metric registry right away,
        // not stay frozen at whatever configure() seeded. Before the fix,
        // set_roi_zones only wrote to the db and never called metrics.sync_zones,
        // so a zone added at runtime never got a descriptor (and a removed one
        // leaked) until the extension was reloaded.
        let ctx = make_ctx();

        let dyn_names = |ctx: &Ctx| -> Vec<String> {
            ctx.metrics
                .descriptors()
                .iter()
                .filter(|d| d.name.starts_with("gym.equipment_occupied."))
                .map(|d| d.name.clone())
                .collect()
        };
        assert!(
            dyn_names(&ctx).is_empty(),
            "no per-zone metrics before any zone is added"
        );

        // Add one enabled zone via the command.
        let one = json!({"zones": [
            { "id": "z1", "name": "Treadmill", "equipment_type": "treadmill",
              "polygon": [[0.1,0.1],[0.4,0.1],[0.4,0.5],[0.1,0.5]], "enabled": true },
        ]});
        handle(&ctx, "set_roi_zones", &one).expect("set ok");
        assert_eq!(
            dyn_names(&ctx),
            vec!["gym.equipment_occupied.Treadmill"],
            "newly added zone must be advertised immediately"
        );

        // Full-replace with an empty set → the per-zone descriptor disappears.
        handle(&ctx, "set_roi_zones", &json!({"zones": []})).expect("clear ok");
        assert!(
            dyn_names(&ctx).is_empty(),
            "removing all zones must drop the per-zone metric"
        );
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
