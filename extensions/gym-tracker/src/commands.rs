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
use std::collections::HashMap;
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
        "get_push_diag" => {
            use std::sync::atomic::Ordering as O;
            let d = crate::state::push_diag();
            return Ok(json!({
                "ingested": d.ingested.load(O::Relaxed),
                "ingest_gaps": d.ingest_gaps.load(O::Relaxed),
                "queue_overflow": d.queue_overflow.load(O::Relaxed),
                "pushed": d.pushed.load(O::Relaxed),
            }));
        }
        "get_ingest_diag" => {
            // Ingest liveness trace: where the reconnect loop currently is.
            // Zeroed stamps = that step never ran (thread dead / config
            // never spawned it); an old early stamp with zeroed later ones
            // = the exact step it's stuck on.
            let d = crate::ingest::ingest_dbg().lock().clone();
            Ok(json!({
                "started_at": d.started_at,
                "cycles": d.cycles,
                "login_at": d.login_at,
                "login_ok": d.login_ok,
                "ws_connect_at": d.ws_connect_at,
                "ws_url": d.ws_url,
                "ws_ok": d.ws_ok,
                "last_frame_at": d.last_frame_at,
                "frames_total": d.total,
                "h264_no_viewer": d.h264_no_viewer,
                "gap_cam_ms": d.gap_cam_ms,
                "gap_bus_ms": d.gap_bus_ms,
                "gap_n": d.gap_n,
                "parsed_ok": d.parsed_ok,
                "parse_fail": d.parse_fail,
                "topic_counts": d.topic_counts,
                "last_error": d.last_error,
                "first_track_payload": d.first_track_payload,
                "now": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|x| x.as_secs()).unwrap_or(0),
            }))
        }
        "get_live_state" => {
            let workouts = ctx.analytics.workout_snapshot();
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
            // ---- face identity anchor (arcface) ----
            // A frame-level face emb matching a member's FACE library
            // anchors that member's identity on the geometrically enclosing
            // track — clothing-independent. The anchored track's BODY emb
            // then auto-appends to that member's body library WITHOUT the
            // body confidence gate (the face already confirmed identity;
            // that is precisely how new outfits get recorded), and the
            // face emb itself appends under the usual diversity/cooldown
            // gates. Face overrides body when both speak.
            let now = chrono::Utc::now().timestamp();
            // track_id -> (member_idx, face_dist)
            let mut face_anchored: HashMap<i64, (usize, f32)> = HashMap::new();
            for fe in &faces {
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
                let Some((i, d)) = best.filter(|(_, d)| *d <= ctx.identity.face_match_threshold)
                else {
                    continue;
                };
                let m = &members[i];
                // grow the face library: diverse + cooldown gated
                if m.face_embeddings
                    .iter()
                    .all(|s| l2_dist(emb, s) >= ctx.identity.append_min_dist)
                    && (m.face_embeddings.is_empty()
                        || now - ctx.db.last_embedding_append_kind(&m.id, "face")
                            >= ctx.identity.append_cooldown_sec)
                {
                    if ctx
                        .db
                        .append_member_embedding_kind(&m.id, emb, "face")
                        .unwrap_or(false)
                    {
                        members[i].face_embeddings.push(emb.clone());
                    }
                }
                // anchor: among tracks whose bbox contains the face center,
                // pick the TIGHTEST fit — a departed person's track can
                // linger inside the TTL with an overlapping bbox, and the
                // smallest containing box is the one actually on the face.
                let (fx, fy) = (fe.bbox.x + fe.bbox.w / 2.0, fe.bbox.y + fe.bbox.h / 2.0);
                let tightest = tracks
                    .iter()
                    .filter(|t| {
                        let b = &t.bbox;
                        fx >= b.x && fx <= b.x + b.w && fy >= b.y && fy <= b.y + b.h
                    })
                    .min_by(|a, b| (a.bbox.w * a.bbox.h).total_cmp(&(b.bbox.w * b.bbox.h)));
                if let Some(t) = tightest {
                    face_anchored.insert(t.track_id, (i, d));
                }
            }
            // face-confirmed body-append: identity is certain, record the outfit
            for (tid, (i, _)) in &face_anchored {
                let Some(t) = tracks.iter().find(|t| t.track_id == *tid) else {
                    continue;
                };
                let Some(f) = t.face.as_ref() else { continue };
                if f.emb.is_empty() {
                    continue;
                }
                let m = &members[*i];
                let diverse = m
                    .all_embeddings()
                    .all(|s| l2_dist(&f.emb, s) >= ctx.identity.append_min_dist);
                if diverse
                    && m.extra_embeddings.len() + 1 < crate::config::MAX_EMBEDDINGS_PER_MEMBER
                    && now - ctx.db.last_embedding_append_kind(&m.id, "body")
                        >= ctx.identity.append_cooldown_sec
                {
                    if ctx
                        .db
                        .append_member_embedding(&m.id, &f.emb)
                        .unwrap_or(false)
                    {
                        tracing::info!(member = %m.id,
                            "outfit recorded (face-confirmed identity)");
                        members[*i].extra_embeddings.push(f.emb.clone());
                    }
                }
            }
            // (free fns, not closures: the borrow checker otherwise pins
            // `members` immutable while the auto-enroll loop pushes to it)
            // Nearest member = min L2 over ALL of that member's samples
            // (primary + accumulated extras) — a member recognized in any
            // of their recorded outfits.
            let nearest_member = |ems: &[crate::db::Member], emb: &[f32]| -> Option<(usize, f32)> {
                let mut best: Option<(usize, f32)> = None;
                for (i, m) in ems.iter().enumerate() {
                    for sample in m.all_embeddings() {
                        let d = l2_dist(emb, sample);
                        if d.is_finite() && best.map_or(true, |(_, b)| d < b) {
                            best = Some((i, d));
                        }
                    }
                }
                best
            };
            let match_one =
                |ems: &[crate::db::Member], emb: &[f32]| -> Option<(String, String, f32)> {
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
                    // Persistence gate: enroll only after the track has been
                    // unknown for N consecutive observations. Far-field /
                    // moving people flicker between "unknown" and "matched"
                    // as embeddings drift — enrolling on the first unknown
                    // reading forks a duplicate member per dropout.
                    {
                        let mut streaks = ctx.state.unknown_streaks_write();
                        if nearest_member(&members, &f.emb)
                            .map_or(false, |(_, d)| d <= ctx.identity.auto_capture_distance)
                        {
                            streaks.insert(t.track_id, 0);
                            continue;
                        }
                        let s = streaks.entry(t.track_id).or_insert(0);
                        *s += 1;
                        if *s < 3 {
                            continue; // need 3 consecutive unknown readings
                        }
                    }
                    // Size gate: far-field bodies produce unstable
                    // embeddings — don't build a member library from them.
                    // 0.15 (was 0.10): with 4K tiles feeding far-field
                    // tracks, h 0.10–0.14 embeddings drifted >600 and each
                    // drift forked a new 访客 (154 members, 54 with no
                    // session, ~18/day from 2-3 real people). Those tracks
                    // still track + match, they just don't enroll.
                    if t.bbox.h < 0.15 {
                        continue;
                    }
                    match ctx
                        .db
                        .insert_auto_member(&ctx.identity.unknown_prefix, &f.emb)
                    {
                        Ok(m) => {
                            ctx.state.unknown_streaks_write().insert(t.track_id, 0);
                            members.push(m)
                        }
                        Err(e) => tracing::warn!(
                            member_err = %e, "auto member insert failed"),
                    }
                }
            }
            // ---- library growth: strong-confidence sample appending ----
            // A MATCHED member (d ≤ match_threshold) seen with STRONG
            // confidence (d ≤ append_confidence, much tighter) whose
            // current embedding is far from every stored sample
            // (≥ append_min_dist — i.e. a new look, not a redundant
            // re-sample) gets the new sample appended, subject to a
            // cooldown. This is how the library persists across outfits:
            // day 1 red shirt (auto-enrolled), day 2 blue shirt → new
            // visitor → renamed/confirmed → the blue-shirt embedding
            // accumulates once identity is certain.
            for t in &tracks {
                let Some(f) = t.face.as_ref() else { continue };
                if f.emb.is_empty() {
                    continue;
                }
                let Some((i, d)) = nearest_member(&members, &f.emb) else {
                    continue;
                };
                if d > ctx.identity.append_confidence {
                    continue; // matched but not certain enough to write
                }
                let m = &members[i];
                let diverse = m
                    .all_embeddings()
                    .all(|s| l2_dist(&f.emb, s) >= ctx.identity.append_min_dist);
                if !diverse {
                    continue;
                }
                if m.extra_embeddings.len() as i64
                    >= crate::config::MAX_EMBEDDINGS_PER_MEMBER as i64 - 1
                {
                    continue; // library full
                }
                if now - ctx.db.last_embedding_append_at(&m.id) < ctx.identity.append_cooldown_sec {
                    continue; // cooldown — one sample per minute max
                }
                match ctx.db.append_member_embedding(&m.id, &f.emb) {
                    Ok(true) => {
                        tracing::info!(
                            member_id = %m.id, dist = d,
                            samples = m.extra_embeddings.len() + 2,
                            "member embedding library extended");
                        members[i].extra_embeddings.push(f.emb.clone());
                    }
                    Ok(false) => {}
                    Err(e) => tracing::warn!(member_err = %e, "append failed"),
                }
            }
            // Opportunistic heatmap persistence + workout record flush /
            // session close — polls are frequent enough that this bounds
            // dirty-frame loss to one poll interval.
            ctx.analytics.maybe_save(&ctx.db);
            ctx.analytics.close_expired_workouts(&ctx.db, 15);
            Ok(json!({
                "present_count": ctx.state.present_count(),
                "tracks": tracks.iter().map(|t| {
                    // emb is large (512 f32) — expose the matched member, not
                    // the raw vector; register_member reads it from state.
                    // Face anchor wins over the body match; `via` tells the UI
                    // which modality identified the person.
                    let member = face_anchored.get(&t.track_id)
                        .map(|(i, d)| json!({
                            "id": members[*i].id, "name": members[*i].name,
                            "dist": (d * 1000.0).round() / 1000.0, "via": "face",
                        }))
                        .or_else(|| t.face.as_ref()
                            .and_then(|f| match_one(&members, &f.emb))
                            .map(|(id, name, dist)| json!({
                                "id": id, "name": name,
                                "dist": (dist * 1000.0).round() / 1000.0,
                                "via": "body",
                            })));
                    json!({
                        "track_id": t.track_id,
                        "bbox": t.bbox,
                        "foot": t.foot,
                        // live workout: exercise, reps/sets, current zone
                        // + continuous dwell there (equipment BUSY gate)
                        "exercise": workouts.get(&t.track_id).map(|w| json!({
                            "name": w.0, "reps": w.1, "sets": w.2, "zone": w.3,
                            "zone_hold": (w.4 * 10.0).round() / 10.0,
                        })),
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
        // Latest producer frame preview + the tracks OF THAT SAME FRAME —
        // the Monitor draws image and overlay from one source, so the
        // separate video/data pipelines (and their clock desync) are gone.
        // Null img_b64 when the producer doesn't attach previews (PREVIEW=0).
        "get_frame" => {
            // Prefer the display-rate preview stream (gym/preview); fall back
            // to the (now rare) TrackFrame-carried image. The REST shape must
            // stay identical to the WS push payload so the Monitor's polling
            // fallback renders the same way.
            let (img, tracks, faces) = match ctx.state.snapshot_preview() {
                Some((ts, i)) => {
                    let hist = ctx.state.tracks_near(ts);
                    let tracks = hist.last().map(|(_, t)| t.clone()).unwrap_or_default();
                    let faces = ctx.state.snapshot_faces().unwrap_or_default();
                    return Ok(json!({
                        // REST stays string-shaped for the polling fallback
                        // (and old frontends); only the WS push leg is binary.
                        "img_b64": crate::frame::encode_b64(&i),
                        "ts_ns": ts,
                        "tracks_hist": hist,
                        "tracks_ts": ctx.state.snapshot_tracks_ts(),
                        "tracks": tracks,
                        "faces": faces,
                        "present_count": tracks.len(),
                    }));
                }
                None => match ctx.state.snapshot_frame() {
                    Some(b) => b,
                    None => return Ok(json!({ "img_b64": serde_json::Value::Null })),
                },
            };
            let members = ctx.db.list_members().map_err(|e| e.to_string())?;
            let workouts = ctx.analytics.workout_snapshot();
            let matched = crate::identity::match_tracks(&ctx.identity, &members, &tracks, &faces);
            let tracks_json = tracks
                .iter()
                .map(|t| {
                    let member = matched.get(&t.track_id).map(|m| {
                        json!({
                            "id": members[m.member_idx].id,
                            "name": members[m.member_idx].name,
                            "dist": (m.dist * 1000.0).round() / 1000.0,
                            "via": m.via,
                        })
                    });
                    json!({
                        "track_id": t.track_id,
                        "bbox": t.bbox,
                        "foot": t.foot,
                        "pose": t.pose,
                        "member": member,
                        "exercise": workouts.get(&t.track_id).map(|w| json!({
                            "name": w.0, "reps": w.1, "sets": w.2, "zone": w.3,
                            "zone_hold": (w.4 * 10.0).round() / 10.0,
                        })),
                    })
                })
                .collect::<Vec<_>>();
            Ok(json!({
                "img_b64": img,
                "faces": faces,
                "tracks": tracks_json,
                "present_count": tracks.len(),
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
            // Atomic replace: absent-zone deletes + upserts commit together;
            // deletes run first inside the tx so a kept id can reuse a name
            // freed by a removed zone (zones.name is UNIQUE).
            ctx.db.replace_zones(&zones_in).map_err(|e| e.to_string())?;
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
        "get_ingest_dbg" => {
            let g = crate::ingest::ingest_dbg().lock();
            Ok(
                json!({ "total": g.total, "parsed_ok": g.parsed_ok, "parse_fail": g.parse_fail,
                       "topics": g.topic_counts }),
            )
        }
        "get_crossings" => Ok(json!({ "lines": ctx.analytics.get_crossings() })),
        // ---- door-flow history (per-day, all lines aggregated) ----
        "get_crossing_history" => {
            let days = args["days"].as_i64().unwrap_or(7).clamp(1, 90);
            let today = crate::analytics::today();
            let since = today - (days - 1);
            let mut rows: Vec<(i64, u64, u64)> = ctx.db.list_crossing_history(since);
            // today's persisted row can lag the live counters (flush rides
            // the poll cadence) — overlay the in-memory tally so "now"
            // reads live while history stays from SQLite
            let live: (u64, u64) = ctx
                .analytics
                .get_crossings()
                .iter()
                .fold((0, 0), |(i, o), s| (i + s.in_count, o + s.out_count));
            match rows.iter_mut().find(|(d, _, _)| *d == today) {
                Some(r) => r.1 = r.1.max(live.0),
                None => {
                    if live.0 > 0 || live.1 > 0 {
                        rows.push((today, live.0, live.1))
                    }
                }
            }
            Ok(json!({
                "days": days,
                // reconciliation: door counters vs live presence — the two
                // drift apart (partial crossings, line loitering) and the
                // card showed contradictory numbers with no explanation
                "today": { "in": live.0, "out": live.1, "net": (live.0 as i64 - live.1 as i64) },
                "presence": {
                    "live": ctx.state.present_count() as i64,
                    "drift": ctx.state.present_count() as i64 - (live.0 as i64 - live.1 as i64),
                },
                "history": rows.iter().map(|(d, i, o)| json!({
                    "day": d, "in": i, "out": o, "net": (*i as i64 - *o as i64),
                })).collect::<Vec<_>>(),
            }))
        }
        // ---- P1: heatmap ----
        "get_heatmap" => Ok(ctx.analytics.get_heatmap()),
        // ---- time-range activity query (trails + heatmap over a window) ----
        "get_activity_window" => {
            let end = args["end"].as_i64().unwrap_or_else(|| chrono::Utc::now().timestamp());
            let span = args["span_sec"].as_i64().unwrap_or(3600).clamp(60, 24 * 3600);
            let start = args["start"]
                .as_i64()
                .unwrap_or_else(|| end.saturating_sub(span));
            let grid = ctx.db.activity_grid(start, end);
            let trails = ctx.db.activity_trails(start, end, 20000);
            let samples: i64 = ctx
                .db
                .activity_trails_count(start, end);
            Ok(json!({
                "start": start, "end": end,
                "cols": 64, "rows": 36,
                "grid": grid,
                "samples": samples,
                "tracks": trails.len(),
                "trails": trails.iter().map(|(tid, pts)| json!({
                    "track_id": tid,
                    "pts": pts.iter().map(|(x, y)| json!({"x": x, "y": y})).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            }))
        }
        // ---- per-member workout detail (exercises / zones / sessions) ----
        "get_member_workout_detail" => {
            let id = args["member_id"].as_str().ok_or("get_member_workout_detail: missing member_id")?;
            // days=all (0) → entire history
            let days = args["days"].as_i64().unwrap_or(7);
            let all_history = days <= 0;
            let days = days.clamp(1, 90);
            let since = if all_history {
                0
            } else {
                chrono::Utc::now().timestamp() - days * 86400
            };
            let (body_samples, face_samples) = ctx.db.member_sample_counts(id);
            let daily = if all_history {
                ctx.db.member_daily_history(id, since).map_err(|e| e.to_string())?
            } else {
                Vec::new()
            };
            let exercises = ctx
                .db
                .member_exercise_stats(id, since)
                .map_err(|e| e.to_string())?;
            let zones = ctx
                .db
                .equipment_stats(Some(id), since)
                .map_err(|e| e.to_string())?;
            let sessions = ctx.db.list_sessions(Some(id), since, 500).map_err(|e| e.to_string())?;
            let total: i64 = sessions
                .iter()
                .map(|s| s["duration_sec"].as_i64().unwrap_or(0))
                .sum();
            Ok(json!({
                "days": if all_history { json!(0) } else { json!(days) },
                "all_history": all_history,
                "total_duration_sec": total,
                "visits": sessions.len(),
                "identity": { "body_samples": body_samples, "face_samples": face_samples },
                // 动作明细: label, sets, reps, minutes, session appearances
                "exercises": exercises.iter().map(|(ex, sets, reps, secs, sess)| json!({
                    "exercise": ex, "sets": sets, "reps": reps,
                    "duration_sec": secs, "sessions": sess,
                })).collect::<Vec<_>>(),
                "zones": zones.iter().map(|(z, sec, reps, ex)| json!({
                    "zone": z, "duration_sec": sec, "reps": reps, "exercise": ex,
                })).collect::<Vec<_>>(),
                "sessions": sessions.iter().rev().take(20).map(|s| json!({
                    "id": s["id"], "started_at": s["started_at"],
                    "duration_sec": s["duration_sec"],
                })).collect::<Vec<_>>(),
                // per-day grouped history (all_history only)
                "daily": daily.iter().map(|(day, ex, sets, reps, secs)| json!({
                    "day": day, "exercise": ex, "sets": sets,
                    "reps": reps, "duration_sec": secs,
                })).collect::<Vec<_>>(),
            }))
        }
        // ---- P3: member library (body-ReID via osnet embeddings) ----
        // Register the CURRENT embedding of a live track as a named member.
        // Takes the track's latest face.emb from the mirror — the device
        // refreshes it every reid_interval (2 s), so a person standing in
        // view for a few seconds has a fresh embedding.
        "register_member" => {
            let track_id: i64 = args["track_id"]
                .as_i64()
                .ok_or("register_member: missing track_id")?;
            let name = args["name"]
                .as_str()
                .map(str::trim)
                .ok_or("register_member: missing name")?;
            if name.is_empty() {
                return Err("register_member: name is empty".into());
            }
            let _ = ctx.state.evict_expired();
            let track = ctx
                .state
                .snapshot()
                .into_iter()
                .find(|t| t.track_id == track_id)
                .ok_or_else(|| format!("track {track_id} not in frame"))?;
            // EMBEDDING FALLBACK: the face embed cycle runs at ~8Hz while
            // tracks arrive at 10-15Hz — the CURRENT frame often has no
            // face.emb even though we saw one recently. Use the cached
            // embedding from the last face-bearing frame.
            let emb_vec: Vec<f32> = track
                .face
                .as_ref()
                .filter(|f| !f.emb.is_empty())
                .map(|f| f.emb.clone())
                .or_else(|| ctx.state.cached_face_emb(track_id))
                .ok_or("track has no embedding yet — step closer to the camera so your face is visible, then retry")?;
            let id = format!("member_{}", uuid::Uuid::new_v4().simple());
            ctx.db
                .insert_member(&id, name, &emb_vec)
                .map_err(|e| e.to_string())?;
            // seed the FACE library too: a frame-level face emb whose box
            // center falls inside this track's bbox is this person's face.
            let b = &track.bbox;
            let face_emb = ctx
                .state
                .snapshot_faces()
                .unwrap_or_default()
                .into_iter()
                .find(|fe| {
                    let (fx, fy) = (fe.bbox.x + fe.bbox.w / 2.0, fe.bbox.y + fe.bbox.h / 2.0);
                    fx >= b.x
                        && fx <= b.x + b.w
                        && fy >= b.y
                        && fy <= b.y + b.h
                        && fe.emb.as_ref().map_or(false, |e| !e.is_empty())
                })
                .and_then(|fe| fe.emb);
            if let Some(femb) = &face_emb {
                ctx.db
                    .append_member_embedding_kind(&id, femb, "face")
                    .map_err(|e| e.to_string())?;
            }
            Ok(
                json!({ "member": { "id": id, "name": name, "dim": emb_vec.len(),
                                   "face": face_emb.is_some() } }),
            )
        }
        "list_members" => {
            let members = ctx.db.list_members().map_err(|e| e.to_string())?;
            Ok(json!({ "members": members.iter().map(|m| json!({
                "id": m.id, "name": m.name, "dim": m.embedding.len(),
                "source": m.source, "created_at": m.created_at,
                // total samples in the member's embedding library
                "samples": m.extra_embeddings.len() + 1,
                // avatar thumbnail (base64 JPEG) — absent until captured
                "photo": m.photo,
            })).collect::<Vec<_>>() }))
        }
        // Store / clear a member avatar photo (base64 JPEG). The Monitor
        // captures it from the raw video frame via its own canvas.
        "set_member_photo" => {
            let id = args["id"].as_str().ok_or("set_member_photo: missing id")?;
            let photo = args["photo_base64"].as_str().filter(|s| !s.is_empty());
            if let Some(p) = photo {
                if p.len() > 400_000 {
                    return Err("set_member_photo: photo too large (>300KB base64)".into());
                }
            }
            let ok = ctx
                .db
                .set_member_photo(id, photo)
                .map_err(|e| e.to_string())?;
            if !ok {
                return Err(format!("member {id} not found"));
            }
            Ok(json!({ "id": id, "photo_set": photo.is_some() }))
        }
        // Fill in / correct a member's display name (auto-enrolled entries
        // are created unnamed — prefix-numbered — precisely so this can
        // attach the real name later).
        "rename_member" => {
            let id = args["id"].as_str().ok_or("rename_member: missing id")?;
            let name = args["name"]
                .as_str()
                .map(str::trim)
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
        // Merge src into dst (outfit-change confirmation): src's samples
        // join dst's library and src disappears. Name always stays dst's.
        "merge_members" => {
            let src = args["src_id"]
                .as_str()
                .ok_or("merge_members: missing src_id")?;
            let dst = args["dst_id"]
                .as_str()
                .ok_or("merge_members: missing dst_id")?;
            if src == dst {
                return Err("merge_members: src and dst are the same member".into());
            }
            ctx.db.merge_members(src, dst).map_err(|e| e.to_string())?;
            let out = ctx.db.list_members().map_err(|e| e.to_string())?;
            let target = out
                .iter()
                .find(|m| m.id == dst)
                .ok_or("mergeMembers: dst vanished")?;
            Ok(json!({ "merged": src, "into": dst,
                       "name": target.name,
                       "samples": target.extra_embeddings.len() + 1 }))
        }
        // ---- P4: workout records ----
        // Aggregated workout data: sessions + per-equipment usage,
        // optionally scoped to one member. day_from = epoch seconds.
        "get_workout_summary" => {
            let member_id = args["member_id"].as_str();
            let day = args["day_from"].as_i64().unwrap_or_else(|| {
                // midnight local time today
                use chrono::{Local, Timelike};
                let n = Local::now();
                (n - chrono::Duration::seconds(
                    n.hour() as i64 * 3600 + n.minute() as i64 * 60 + n.second() as i64,
                ))
                .timestamp()
            });
            let sessions = ctx
                .db
                .list_sessions(member_id, day, 50)
                .map_err(|e| e.to_string())?;
            let equipment = ctx
                .db
                .equipment_stats(member_id, day)
                .map_err(|e| e.to_string())?;
            let total: i64 = sessions
                .iter()
                .map(|s| s["duration_sec"].as_i64().unwrap_or(0))
                .sum();
            Ok(json!({
                "since": day,
                "sessions": sessions,
                "equipment": equipment.iter().map(|(z, sec, reps, ex)| json!({
                    "zone_id": z, "duration_sec": sec, "reps": reps,
                    "exercise": ex,
                })).collect::<Vec<_>>(),
                "total_duration_sec": total,
                "visit_count": sessions.len(),
            }))
        }
        "delete_member" => {
            let id = args["id"].as_str().ok_or("delete_member: missing id")?;
            let n = ctx.db.delete_member(id).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err(format!("member {id} not found"));
            }
            Ok(json!({ "deleted": id }))
        }
        // Member visit report for the weekly card: per-member visits /
        // minutes / last-seen over the last N days, plus library totals.
        "get_member_report" => {
            let days = args["days"].as_i64().unwrap_or(7).clamp(1, 90);
            let since = chrono::Utc::now().timestamp() - days * 86400;
            let members = ctx.db.list_members().map_err(|e| e.to_string())?;
            let sessions = ctx
                .db
                .list_sessions(None, since, 20000)
                .map_err(|e| e.to_string())?;
            use std::collections::BTreeMap;
            let mut agg: BTreeMap<String, (String, u32, i64, i64)> = BTreeMap::new();
            for s in &sessions {
                let mid = match s["member_id"].as_str() {
                    Some(m) if !m.is_empty() => m.to_string(),
                    _ => continue, // anonymous walk-in sessions don't rank
                };
                let name = s["member_name"].as_str().unwrap_or("会员").to_string();
                let dur = s["duration_sec"].as_i64().unwrap_or(0).max(0);
                let started = s["started_at"].as_i64().unwrap_or(0);
                let e = agg.entry(mid).or_insert((name, 0, 0, 0));
                e.1 += 1;
                e.2 += dur;
                e.3 = e.3.max(started);
            }
            let mut rows: Vec<_> = agg
                .into_iter()
                .map(|(id, (name, visits, secs, last))| json!({
                    "member_id": id, "name": name, "visits": visits,
                    "duration_sec": secs, "last_seen": last,
                }))
                .collect();
            rows.sort_by(|a, b| {
                b["duration_sec"].as_i64().cmp(&a["duration_sec"].as_i64())
            });
            // Avatar thumbnails + enrollment source per row. Photos ride
            // along only for the head of the (sorted) list — the report card
            // renders ~10 rows and each photo is ~3 KB of base64; shipping
            // one per member would turn a 30 s poll into hundreds of KB.
            let info: std::collections::HashMap<&str, (Option<&str>, &str)> = members
                .iter()
                .map(|m| (m.id.as_str(), (m.photo.as_deref(), m.source.as_str())))
                .collect();
            for (i, r) in rows.iter_mut().enumerate() {
                let Some((photo, source)) = info.get(r["member_id"].as_str().unwrap_or(""))
                else {
                    continue;
                };
                let obj = r.as_object_mut().unwrap();
                obj.insert("source".into(), json!(source));
                if i < 12 {
                    if let Some(p) = *photo {
                        obj.insert("photo".into(), json!(p));
                    }
                }
            }
            let total_visits: u32 = rows.iter().filter_map(|r| r["visits"].as_u64()).sum::<u64>() as u32;
            Ok(json!({
                "days": days,
                "members_total": members.len(),
                "active_members": rows.len(),
                "total_visits": total_visits,
                "rows": rows,
            }))
        }
        // Recent safety/ops alerts (fall-suspect, long-occupancy).
        "get_alerts" => Ok(ctx.analytics.alerts_snapshot(&ctx.db)),
        // acknowledge / dismiss an alert (false positives included)
        "resolve_alert" => {
            let id = args["id"].as_i64().ok_or("resolve_alert: missing id")?;
            let n = ctx.db.resolve_alert(id).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err(format!("alert {id} not found"));
            }
            Ok(json!({ "resolved": id }))
        }
        // P1 stub: no single-frame REST endpoint on this firmware yet. Not
        // fatal — the dispatch maps the client error to an error object so the
        // frontend can render a placeholder. P2 will grab a frame via RTSP.
        "get_snapshot" => {
            let stream = args.get("stream").and_then(|s| s.as_str()).unwrap_or("sub");
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
        make_ctx_identity(
            ttl_sec,
            r#"{ "match_threshold": 0.1, "auto_capture_unknown": false, "unknown_prefix": "U", "auto_capture_distance": 0.5 }"#,
        )
    }

    fn make_ctx() -> Ctx {
        make_ctx_with_ttl(30)
    }

    /// Ctx with a custom identity block — auto-capture tests flip the
    /// switch and tune thresholds to unit-scale test embeddings.
    fn make_ctx_identity(ttl_sec: u32, identity_json: &str) -> Ctx {
        let raw = format!(
            r#"{{"device":{{"host":"127.0.0.1","username":"x","password":"x","tls_insecure":false}},"device_id":"test","ingest":{{"topic":"gym/track","publish_hz":8,"track_ttl_sec":30,"reconnect_backoff_sec":[1,2,5]}},"identity":{identity_json},"roi":{{"dwell_debounce_sec":3,"hysteresis":true}},"data_dir":"/tmp/gym-test"}}"#
        );
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
            ts: None,
            bbox: Bbox {
                x: 0.1,
                y: 0.2,
                w: 0.3,
                h: 0.4,
            },
            foot: Point { x: 0.25, y: 0.6 },
            pose: None,
            face: None,
            vel: None,
            ex: None,
        }
    }

    fn frame(tracks: Vec<Track>) -> TrackFrame {
        TrackFrame {
            device_id: "d".into(),
            frame_seq: 1,
            ts_ns: 0,
            tracks,
            faces: vec![],
            img_b64: None,
        }
    }

    #[test]
    fn auto_enroll_skips_far_field() {
        // far-field bodies (h<0.15, from the 4K tile passes) must NOT fork
        // new members — their embeddings drift past auto_capture_distance
        // and every drift created a fresh 访客
        let ctx = make_ctx_identity(
            30,
            r#"{ "match_threshold": 0.1, "auto_capture_unknown": true, "unknown_prefix": "U", "auto_capture_distance": 0.5 }"#,
        );
        let mut t = track(7);
        t.bbox.h = 0.12; // far-field
        t.face = Some(crate::types::Face {
            emb: vec![1.0, 0.0, 0.0],
            det: 0.8,
        });
        for _ in 0..5 {
            ctx.state.apply_frame(&frame(vec![t.clone()]));
            let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
            assert_eq!(out["members_count"], 0, "far-field unknown never enrolls");
        }
        // same body walks closer (h=0.3) → enrolls after the streak gate
        t.bbox.h = 0.3;
        for _ in 0..3 {
            ctx.state.apply_frame(&frame(vec![t.clone()]));
            let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        }
        let out = handle(&ctx, "get_live_state", &json!({})).unwrap();
        assert_eq!(out["members_count"], 1, "near-field unknown enrolls");
    }

    #[test]
    fn auto_enroll_capture_and_rename_flow() {
        // match 0.1 / auto-capture on beyond 0.5 / prefix U (unit scale)
        let ctx = make_ctx_identity(
            30,
            r#"{ "match_threshold": 0.1, "auto_capture_unknown": true, "unknown_prefix": "U", "auto_capture_distance": 0.5 }"#,
        );
        let emb = |v: Vec<f32>| Some(crate::types::Face { emb: v, det: 0.8 });

        // first visitor: empty library → auto-enrolled as U-1 on first poll
        let mut t = track(1);
        t.face = emb(vec![1.0, 0.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t.clone()]));
        // persistence gate: enrollment needs 3 consecutive unknown polls
        let mut out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(
            out["members_count"], 0,
            "first unknown reading does not enroll"
        );
        for _ in 0..2 {
            ctx.state.apply_frame(&frame(vec![t.clone()]));
            out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        }
        assert_eq!(out["members_count"], 1);
        let by = |out: &Value, tid: i64| {
            out["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["track_id"] == tid)
                .unwrap()
                .clone()
        };
        assert_eq!(
            by(&out, 1)["member"]["name"],
            "U-1",
            "auto entry immediately matches its own person"
        );

        // same person re-detected (new track id, near embedding): matches
        // U-1, no duplicate entry
        let mut t2 = track(2);
        t2.face = emb(vec![0.99, 0.01, 0.0]);
        ctx.state.apply_frame(&frame(vec![t2]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["members_count"], 1);
        let by = |out: &Value, tid: i64| {
            out["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["track_id"] == tid)
                .unwrap()
                .clone()
        };
        assert_eq!(by(&out, 2)["member"]["name"], "U-1");

        // drift band (distance ~0.35, between 0.1 and 0.5): neither matched
        // nor re-enrolled — the gap absorbs embedding drift
        let mut t3 = track(3);
        t3.face = emb(vec![0.95, 0.35, 0.0]);
        ctx.state.apply_frame(&frame(vec![t3]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        assert_eq!(out["members_count"], 1, "drift band must not enroll");
        let by = |out: &Value, tid: i64| {
            out["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["track_id"] == tid)
                .unwrap()
                .clone()
        };
        assert!(by(&out, 3)["member"].is_null());

        // second visitor (orthogonal embedding, distance ~1.19 > 0.5):
        // enrolled as U-2
        let mut t4 = track(4);
        t4.face = emb(vec![0.0, 1.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t4.clone()]));
        let mut out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        for _ in 0..2 {
            // persistence gate again
            ctx.state.apply_frame(&frame(vec![t4.clone()]));
            out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        }
        assert_eq!(out["members_count"], 2);
        let names: Vec<&str> = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["member"]["name"].as_str().unwrap_or(""))
            .collect();
        assert!(names.contains(&"U-2"), "second visitor enrolled: {names:?}");

        // fill in the real name afterwards
        let list = handle(&ctx, "list_members", &json!({})).unwrap();
        let u1 = list["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "U-1")
            .unwrap();
        let id = u1["id"].as_str().unwrap().to_string();
        assert_eq!(u1["source"], "auto");
        handle(&ctx, "rename_member", &json!({ "id": id, "name": "张三" })).expect("rename ok");
        let mut t5 = track(5);
        t5.face = emb(vec![1.0, 0.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t5]));
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        let by = |out: &Value, tid: i64| {
            out["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["track_id"] == tid)
                .unwrap()
                .clone()
        };
        assert_eq!(
            by(&out, 5)["member"]["name"],
            "张三",
            "renamed member matches under the new name"
        );
    }

    #[test]
    fn merge_members_unifies_outfit_identities() {
        let ctx = make_ctx_identity(
            30,
            r#"{ "match_threshold": 0.1, "auto_capture_unknown": false, "unknown_prefix": "U", "auto_capture_distance": 0.5 }"#,
        );
        let emb = |v: Vec<f32>| Some(crate::types::Face { emb: v, det: 0.8 });

        // day 1 outfit: 小王 registered manually
        let mut t = track(1);
        t.face = emb(vec![1.0, 0.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t]));
        let r1 = handle(
            &ctx,
            "register_member",
            &json!({ "track_id": 1, "name": "小王" }),
        )
        .unwrap();
        let wang = r1["member"]["id"].as_str().unwrap().to_string();

        // day 2 outfit: auto-enrolled as a separate 访客 entry
        let mut t2 = track(2);
        t2.face = emb(vec![0.0, 1.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t2]));
        let r2 = handle(
            &ctx,
            "register_member",
            &json!({ "track_id": 2, "name": "访客X" }),
        )
        .unwrap();
        let guest = r2["member"]["id"].as_str().unwrap().to_string();

        // human confirms: 访客X is 小王 in different clothes → merge
        let out = handle(
            &ctx,
            "merge_members",
            &json!({ "src_id": guest, "dst_id": wang }),
        )
        .unwrap();
        assert_eq!(out["name"], "小王");
        assert_eq!(out["samples"], 2);

        // both outfits now resolve to 小王
        for (tid, e) in [(3, vec![1.0, 0.0, 0.0]), (4, vec![0.0, 1.0, 0.0])] {
            let mut t = track(tid);
            t.face = emb(e);
            ctx.state.apply_frame(&frame(vec![t]));
        }
        let st = handle(&ctx, "get_live_state", &json!({})).unwrap();
        for tid in [3, 4] {
            let t = st["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["track_id"] == tid)
                .unwrap();
            assert_eq!(
                t["member"]["name"],
                "小王",
                "outfit {} resolves after merge",
                tid - 2
            );
        }
        let list = handle(&ctx, "list_members", &json!({})).unwrap();
        assert_eq!(list["members"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn face_anchor_records_new_outfit() {
        // face threshold 0.3, body match 0.1, auto-capture off; appends
        // gated by append_min_dist 0.15 / cooldown 60
        let ctx = make_ctx_identity(
            30,
            r#"{ "match_threshold": 0.1, "auto_capture_unknown": false, "unknown_prefix": "U", "auto_capture_distance": 0.5, "append_confidence": 0.1, "append_min_dist": 0.15, "append_cooldown_sec": 60, "face_match_threshold": 0.3 }"#,
        );

        // register 小红 with a body emb AND a face emb (frame-level face
        // box centered inside her track bbox)
        let mut t1 = track(1);
        t1.bbox = Bbox {
            x: 0.4,
            y: 0.3,
            w: 0.2,
            h: 0.5,
        };
        t1.face = Some(crate::types::Face {
            emb: vec![1.0, 0.0, 0.0],
            det: 0.8,
        });
        let mut f1 = crate::types::TrackFrame {
            device_id: "d".into(),
            frame_seq: 1,
            ts_ns: 0,
            tracks: vec![t1],
            faces: vec![crate::types::FaceBox {
                bbox: Bbox {
                    x: 0.45,
                    y: 0.32,
                    w: 0.1,
                    h: 0.1,
                },
                det: 0.9,
                emb: Some(vec![9.0, 9.0, 9.0]),
            }],
            img_b64: None,
        };
        ctx.state.apply_frame(&f1);
        let r = handle(
            &ctx,
            "register_member",
            &json!({ "track_id": 1, "name": "小红" }),
        )
        .unwrap();
        assert_eq!(r["member"]["face"], true, "face sample seeded at register");

        // next day: COMPLETELY different body (orthogonal emb — no body
        // match possible) but the SAME face → face anchors identity and
        // records the new outfit into her body library
        let mut t2 = track(2);
        // slightly tighter than t1's lingering bbox so the tightest-fit
        // anchor picks the live track deterministically
        t2.bbox = Bbox {
            x: 0.41,
            y: 0.3,
            w: 0.18,
            h: 0.5,
        };
        t2.face = Some(crate::types::Face {
            emb: vec![0.0, 1.0, 0.0],
            det: 0.7,
        });
        f1.tracks = vec![t2];
        f1.faces = vec![crate::types::FaceBox {
            bbox: Bbox {
                x: 0.45,
                y: 0.32,
                w: 0.1,
                h: 0.1,
            },
            det: 0.88,
            emb: Some(vec![8.9, 9.0, 9.05]), // d≈0.15 face drift
        }];
        ctx.state.apply_frame(&f1);
        let out = handle(&ctx, "get_live_state", &json!({})).unwrap();
        let t2s = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == 2)
            .unwrap();
        assert_eq!(t2s["member"]["name"], "小红", "face overrides body");
        assert_eq!(t2s["member"]["via"], "face");

        let list = handle(&ctx, "list_members", &json!({})).unwrap();
        let m = &list["members"][0];
        assert_eq!(m["samples"], 2, "new outfit auto-recorded (face-confirmed)");
        assert_eq!(m["samples"], 2); // body: primary + new outfit
                                     // the second face sample (0.15 drift) is below append_min_dist →
                                     // face library stays at 1
    }

    #[test]
    fn workout_tracking_flow() {
        // zone: treadmill at left half; person on it 5 s (frames at 1 Hz),
        // then zone removed person walks out; session should record usage.
        let ctx = make_ctx_identity(
            30,
            r#"{ "match_threshold": 0.1, "auto_capture_unknown": false, "unknown_prefix": "U", "auto_capture_distance": 0.5, "roi": { "dwell_debounce_sec": 0, "hysteresis": true } }"#,
        );
        handle(
            &ctx,
            "set_roi_zones",
            &json!({ "zones": [{
                "id": "z1", "name": "跑步机1", "equipment_type": "treadmill",
                "polygon": [[0.0,0.3],[0.3,0.3],[0.3,1.0],[0.0,1.0]], "enabled": true,
            }]}),
        )
        .expect("zones");

        // feed 6 frames @1Hz: person standing in the zone with body emb
        for i in 0..6 {
            let mut t = track(1);
            t.foot = Point { x: 0.15, y: 0.8 };
            t.face = Some(crate::types::Face {
                emb: vec![1.0, 0.0, 0.0],
                det: 0.8,
            });
            // standing pose (all major joints visible) — the classifier
            // runs only when the producer supplied keypoints
            t.pose = Some(crate::types::Pose {
                kpts: vec![[0.5, 0.1, 0.9]; 17],
                score: 0.85,
            });
            let f = TrackFrame {
                device_id: "d".into(),
                frame_seq: i,
                ts_ns: (i as u64) * 1_000_000_000,
                tracks: vec![t],
                faces: vec![],
                img_b64: None,
            };
            ctx.state.apply_frame(&f);
            let zones = ctx.db.list_zones().unwrap();
            let members = ctx.db.list_members().unwrap();
            ctx.analytics
                .on_workout_frame(&f, &zones, &members, &ctx.identity, 0);
        }
        let out = handle(&ctx, "get_live_state", &json!({})).unwrap();
        let t1 = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == 1)
            .unwrap();
        let ex = &t1["exercise"];
        assert_eq!(ex["name"], "treadmill_run", "zone maps to cardio exercise");
        assert_eq!(ex["zone"], "z1");

        // summary after the person departs (close via TTL)
        std::thread::sleep(std::time::Duration::from_millis(20));
        ctx.analytics.close_expired_workouts(&ctx.db, 0);
        let sum = handle(&ctx, "get_workout_summary", &json!({ "day_from": 0 })).unwrap();
        assert!(
            sum["visit_count"].as_i64().unwrap() >= 1,
            "session recorded"
        );
        let eq = sum["equipment"].as_array().unwrap();
        assert_eq!(eq.len(), 1);
        // the label is the zone NAME (denormalized; survives zone deletion)
        assert_eq!(eq[0]["zone_id"], "跑步机1");
        assert!(
            eq[0]["duration_sec"].as_i64().unwrap() >= 1,
            "treadmill time accumulated: {:?}",
            eq[0]
        );
    }

    #[test]
    fn get_frame_returns_img_and_its_tracks() {
        let ctx = make_ctx();
        let mut t = track(7);
        t.face = None;
        let f = TrackFrame {
            device_id: "d".into(),
            frame_seq: 1,
            ts_ns: 0,
            tracks: vec![t],
            faces: vec![],
            img_b64: Some("QUJD".into()),
        };
        ctx.state.apply_frame(&f);
        let out = handle(&ctx, "get_frame", &json!({})).unwrap();
        assert_eq!(out["img_b64"], "QUJD");
        let tracks = out["tracks"].as_array().unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0]["track_id"], 7);
        // frames without a preview (PREVIEW=0 producers) yield null img
        let f2 = TrackFrame {
            device_id: "d".into(),
            frame_seq: 2,
            ts_ns: 1,
            tracks: vec![],
            faces: vec![],
            img_b64: None,
        };
        ctx.state.apply_frame(&f2);
        let out2 = handle(&ctx, "get_frame", &json!({})).unwrap();
        // last preview persists until a new one arrives — stale-but-consistent
        assert_eq!(out2["img_b64"], "QUJD");
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
        assert!(
            by_id.contains_key(&7) && by_id.contains_key(&9),
            "tids={by_id:?}"
        );

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
        let out = handle(
            &ctx,
            "register_member",
            &json!({ "track_id": 3, "name": "张三" }),
        )
        .expect("ok");
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
        let t3s = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == 3)
            .unwrap();
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
        let t3s = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == 3)
            .unwrap();
        assert!(t3s["member"].is_null(), "far embedding must not match");

        // register without an embedding is a clean error
        let err = handle(
            &ctx,
            "register_member",
            &json!({ "track_id": 4, "name": "李四" }),
        );
        assert!(err.is_err());

        // delete → matching gone
        handle(&ctx, "delete_member", &json!({ "id": mid })).expect("ok");
        let out = handle(&ctx, "get_live_state", &json!({})).expect("ok");
        let t3s = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == 3)
            .unwrap();
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
        assert!(
            !ids.contains(&"a") && !ids.contains(&"b"),
            "removed ids gone, ids={ids:?}"
        );
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
        assert!(
            out.get("error").is_some(),
            "expected error field, got {out}"
        );
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

    #[test]
    fn embedding_library_grows_on_strong_confident_match() {
        // unit-scale: match 0.3, append confidence 0.3, diversity 0.15,
        // cooldown 60 s, auto-capture off (library growth under test)
        let ctx = make_ctx_identity(
            30,
            r#"{ "match_threshold": 0.3, "auto_capture_unknown": false, "unknown_prefix": "U", "auto_capture_distance": 0.5, "append_confidence": 0.3, "append_min_dist": 0.15, "append_cooldown_sec": 60 }"#,
        );
        let emb = |v: Vec<f32>| Some(crate::types::Face { emb: v, det: 0.8 });

        // day 1: red shirt — enrolled as the primary sample
        let mut t1 = track(1);
        t1.face = emb(vec![1.0, 0.0, 0.0]);
        ctx.state.apply_frame(&frame(vec![t1]));
        handle(
            &ctx,
            "register_member",
            &json!({ "track_id": 1, "name": "小王" }),
        )
        .expect("register");

        // day 2: blue shirt — same person, new look. Distance 0.2 sits in
        // the [diversity 0.15, confidence 0.3] band → appended
        let mut t2 = track(2);
        t2.face = emb(vec![1.0, 0.2, 0.0]);
        ctx.state.apply_frame(&frame(vec![t2]));
        let out = handle(&ctx, "get_live_state", &json!({})).unwrap();
        let t2s = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == 2)
            .unwrap();
        assert_eq!(t2s["member"]["name"], "小王");
        let list = handle(&ctx, "list_members", &json!({})).unwrap();
        assert_eq!(list["members"][0]["samples"], 2, "blue shirt appended");

        // min-over-samples: the blue-shirt embedding itself now matches
        // (d=0 via its own sample), even though it started 0.2 away
        let mut t3 = track(3);
        t3.face = emb(vec![1.0, 0.2, 0.0]);
        ctx.state.apply_frame(&frame(vec![t3]));
        let out = handle(&ctx, "get_live_state", &json!({})).unwrap();
        let t3s = out["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == 3)
            .unwrap();
        assert_eq!(t3s["member"]["name"], "小王");

        // redundant sample (0.1 from both stored) does NOT append
        let mut t4 = track(4);
        t4.face = emb(vec![1.0, 0.1, 0.0]);
        ctx.state.apply_frame(&frame(vec![t4]));
        handle(&ctx, "get_live_state", &json!({})).unwrap();
        let list = handle(&ctx, "list_members", &json!({})).unwrap();
        assert_eq!(list["members"][0]["samples"], 2, "redundant sample skipped");

        // another diverse look (distance 0.28) IS diverse but the cooldown
        // (60 s) blocks a same-minute second append
        let mut t5 = track(5);
        t5.face = emb(vec![1.0, -0.28, 0.0]);
        ctx.state.apply_frame(&frame(vec![t5]));
        handle(&ctx, "get_live_state", &json!({})).unwrap();
        let list = handle(&ctx, "list_members", &json!({})).unwrap();
        assert_eq!(
            list["members"][0]["samples"], 2,
            "cooldown blocks rapid append"
        );
    }
}
