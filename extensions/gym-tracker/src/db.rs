// db.rs
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub equipment_type: String,
    pub polygon: Vec<(f32, f32)>, // normalized [[x,y]...]
    pub enabled: bool,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        // v2 of member_embeddings: tag samples body|face (pre-existing
        // installs got the table without the column)
        let _ = db.conn.lock().execute(
            "ALTER TABLE member_embeddings ADD COLUMN kind TEXT NOT NULL DEFAULT 'body'",
            [],
        );
        // v3: member avatar photo (base64 JPEG thumbnail, captured by the
        // Monitor from the live video — mosaic-free source frame)
        let _ = db
            .conn
            .lock()
            .execute("ALTER TABLE members ADD COLUMN photo TEXT", []);
        Ok(db)
    }
    fn migrate(&self) -> Result<(), rusqlite::Error> {
        self.conn.lock().execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS members (
                id TEXT PRIMARY KEY, display_name TEXT NOT NULL, phone TEXT,
                is_enrolled INTEGER NOT NULL DEFAULT 0, source TEXT NOT NULL DEFAULT 'auto_capture',
                embedding BLOB, first_seen_at INTEGER, created_at INTEGER, updated_at INTEGER);
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY, member_id TEXT, device_id TEXT,
                started_at INTEGER, ended_at INTEGER, duration_sec INTEGER,
                status TEXT, summary TEXT,
                FOREIGN KEY(member_id) REFERENCES members(id));
            CREATE TABLE IF NOT EXISTS equipment_usage (
                id TEXT PRIMARY KEY, session_id TEXT, member_id TEXT, zone_id TEXT, zone_name TEXT,
                started_at INTEGER, ended_at INTEGER, duration_sec INTEGER,
                primary_action TEXT, reps INTEGER,
                FOREIGN KEY(zone_id) REFERENCES zones(id));
            CREATE TABLE IF NOT EXISTS zones (
                id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE,
                equipment_type TEXT NOT NULL, polygon TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1, created_at INTEGER, updated_at INTEGER);
            CREATE TABLE IF NOT EXISTS cross_lines (
                id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE,
                ax REAL NOT NULL, ay REAL NOT NULL, bx REAL NOT NULL, by REAL NOT NULL,
                created_at INTEGER, updated_at INTEGER);
            CREATE TABLE IF NOT EXISTS heatmap_day (
                day INTEGER PRIMARY KEY, grid TEXT NOT NULL, updated_at INTEGER);
            -- Per-line daily crossing counters. The in-memory counters reset
            -- on every extension reload/restart — without this table each
            -- deploy wipes the day's in/out tally mid-shift.
            -- Timestamped foot samples (2 s per track) powering the
            -- trails/heatmap TIME-RANGE queries; pruned past 24 h.
            CREATE TABLE IF NOT EXISTS foot_log (
                ts INTEGER NOT NULL,
                track_id INTEGER NOT NULL,
                x REAL NOT NULL,
                y REAL NOT NULL);
            CREATE INDEX IF NOT EXISTS idx_foot_log_ts ON foot_log(ts);
            CREATE TABLE IF NOT EXISTS crossing_day (
                line_id TEXT NOT NULL, day INTEGER NOT NULL,
                in_count INTEGER NOT NULL DEFAULT 0, out_count INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER,
                PRIMARY KEY(line_id, day));
            -- Per-EXERCISE accumulation (one row per session × exercise):
            -- the live tracker classifies every rep into an exercise label
            -- (zone tier + temporal tier); this keeps the per-label totals
            -- that equipment_usage's single primary_action column loses.
            CREATE TABLE IF NOT EXISTS exercise_usage (
                session_id TEXT NOT NULL,
                member_id TEXT,
                exercise TEXT NOT NULL,
                sets INTEGER NOT NULL DEFAULT 0,
                reps INTEGER NOT NULL DEFAULT 0,
                duration_sec INTEGER NOT NULL DEFAULT 0,
                started_at INTEGER, ended_at INTEGER,
                PRIMARY KEY(session_id, exercise));
            CREATE TABLE IF NOT EXISTS kv_settings (key TEXT PRIMARY KEY, value TEXT);
            -- Safety alerts (fall-suspect, occupancy) PERSIST across
            -- restarts: a wiped alert log after every reload made the
            -- GymAlerts card claim 一切正常 over a history of falls.
            CREATE TABLE IF NOT EXISTS alerts_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts REAL NOT NULL,
                kind TEXT NOT NULL,
                level TEXT NOT NULL,
                track_id INTEGER NOT NULL,
                message TEXT NOT NULL,
                resolved INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER);
            -- P4: per-member embedding LIBRARY. members.embedding stays as the
            -- primary (first) sample; this table holds the additional samples
            -- accumulated over sessions/outfits so matching takes the min
            -- distance over all of them.
            CREATE TABLE IF NOT EXISTS member_embeddings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                member_id TEXT NOT NULL,
                embedding TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'body',
                created_at INTEGER,
                FOREIGN KEY(member_id) REFERENCES members(id));
        "#,
        )?;
        Ok(())
    }
    pub fn list_zones(&self) -> Result<Vec<Zone>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id,name,equipment_type,polygon,enabled FROM zones ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            let poly_str: String = r.get(3)?;
            let poly: Vec<(f32,f32)> = match serde_json::from_str(&poly_str) {
                Ok(p) => p,
                Err(e) => { tracing::warn!(zone_id = %r.get::<_,String>(0).unwrap_or_default(), error = %e, "corrupt polygon JSON, using empty"); Vec::new() }
            };
            Ok(Zone { id: r.get(0)?, name: r.get(1)?, equipment_type: r.get(2)?, polygon: poly, enabled: r.get::<_,i64>(4)? != 0 })
        })?;
        rows.collect()
    }
    pub fn upsert_zone(&self, z: &Zone) -> Result<(), rusqlite::Error> {
        let poly = serde_json::to_string(&z.polygon).unwrap();
        self.conn.lock().execute(
            "INSERT INTO zones(id,name,equipment_type,polygon,enabled,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, equipment_type=excluded.equipment_type, polygon=excluded.polygon, enabled=excluded.enabled, updated_at=excluded.updated_at",
            params![z.id, z.name, z.equipment_type, poly, z.enabled as i64, now_secs()],
        )?;
        Ok(())
    }
    /// Deleting a zone keeps its equipment_usage history: rows are detached
    /// (zone_id → NULL, the denormalized zone_name survives) — otherwise the
    /// zones(id) FK makes any zone that ever accumulated usage immortal.
    pub fn delete_zone(&self, id: &str) -> Result<(), rusqlite::Error> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE equipment_usage SET zone_id=NULL WHERE zone_id=?1",
            params![id],
        )?;
        tx.execute("DELETE FROM zones WHERE id=?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Atomic full-replace for set_roi_zones: zones absent from the incoming
    /// set are deleted (usage detached, see `delete_zone`), the rest upserted —
    /// one transaction, so a mid-set failure (UNIQUE(name) collision, FK)
    /// rolls back instead of leaving a half-replaced zone table.
    pub fn replace_zones(&self, zones: &[Zone]) -> Result<(), rusqlite::Error> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("SELECT id FROM zones")?;
            let ids: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            for id in ids {
                if !zones.iter().any(|z| z.id == id) {
                    tx.execute(
                        "UPDATE equipment_usage SET zone_id=NULL WHERE zone_id=?1",
                        params![&id],
                    )?;
                    tx.execute("DELETE FROM zones WHERE id=?1", params![&id])?;
                }
            }
        }
        let mut stmt = tx.prepare(
            "INSERT INTO zones(id,name,equipment_type,polygon,enabled,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, equipment_type=excluded.equipment_type, polygon=excluded.polygon, enabled=excluded.enabled, updated_at=excluded.updated_at",
        )?;
        for z in zones {
            let poly = serde_json::to_string(&z.polygon).unwrap();
            stmt.execute(params![
                z.id,
                z.name,
                z.equipment_type,
                poly,
                z.enabled as i64,
                now_secs()
            ])?;
        }
        drop(stmt);
        tx.commit()?;
        Ok(())
    }

    // ---- cross lines (P1: line-crossing counting) ----

    pub fn list_lines(&self) -> Result<Vec<crate::analytics::CrossLine>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id,name,ax,ay,bx,by FROM cross_lines ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::analytics::CrossLine {
                id: r.get(0)?,
                name: r.get(1)?,
                a: (r.get(2)?, r.get(3)?),
                b: (r.get(4)?, r.get(5)?),
            })
        })?;
        rows.collect()
    }

    /// Full-replace semantics (mirrors set_roi_zones): lines absent from the
    /// incoming set are deleted first, then the rest upserted.
    pub fn replace_lines(
        &self,
        lines: &[crate::analytics::CrossLine],
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM cross_lines", [])?;
        let mut stmt = conn.prepare(
            "INSERT INTO cross_lines(id,name,ax,ay,bx,by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?7)",
        )?;
        for l in lines {
            stmt.execute(params![
                l.id,
                l.name,
                l.a.0,
                l.a.1,
                l.b.0,
                l.b.1,
                now_secs()
            ])?;
        }
        Ok(())
    }

    // ---- workout records (P4: sessions + equipment usage) ----

    pub fn upsert_session(
        &self,
        id: &str,
        member_id: Option<&str>,
        device_id: &str,
        started: i64,
        ended: i64,
        duration_sec: i64,
        status: &str,
        member_name: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let summary = serde_json::to_string(&serde_json::json!({
            "member_name": member_name.unwrap_or(""),
        }))
        .unwrap();
        self.conn.lock().execute(
            "INSERT INTO sessions(id, member_id, device_id, started_at, ended_at, duration_sec, status, summary)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET member_id=excluded.member_id, ended_at=excluded.ended_at,
               duration_sec=excluded.duration_sec, status=excluded.status, summary=excluded.summary",
            params![id, member_id, device_id, started, ended, duration_sec, status, summary],
        )?;
        Ok(())
    }

    pub fn upsert_equipment_usage(
        &self,
        id: &str,
        session_id: &str,
        member_id: Option<&str>,
        zone_id: &str,
        duration_sec: i64,
        exercise: &str,
        reps: i64,
    ) -> Result<(), rusqlite::Error> {
        self.conn.lock().execute(
            "INSERT INTO equipment_usage(id, session_id, member_id, zone_id, zone_name, started_at, ended_at, duration_sec, primary_action, reps)
             VALUES(?1,?2,?3,?4, COALESCE((SELECT name FROM zones WHERE id=?4), ?4), ?5,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET duration_sec=excluded.duration_sec,
               primary_action=excluded.primary_action, reps=excluded.reps, ended_at=excluded.ended_at",
            params![id, session_id, member_id, zone_id, chrono::Utc::now().timestamp(),
                    duration_sec, exercise, reps],
        )?;
        Ok(())
    }

    /// Per-equipment usage totals since `day_from`, optionally one member.
    /// Returns (zone label, total_sec, reps, primary exercise). The label
    /// prefers the denormalized zone_name, then zone_id, then a placeholder —
    /// rows survive their zone being deleted (zone_id detached to NULL).
    pub fn equipment_stats(
        &self,
        member_id: Option<&str>,
        day_from: i64,
    ) -> Result<Vec<(String, i64, i64, String)>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = if member_id.is_some() {
            conn.prepare(
                "SELECT COALESCE(zone_name, zone_id, '已删除区域'), SUM(duration_sec), MAX(reps), MAX(primary_action)
                 FROM equipment_usage WHERE member_id=?1 AND ended_at>=?2
                 GROUP BY zone_id ORDER BY 2 DESC",
            )?
        } else {
            conn.prepare(
                "SELECT COALESCE(zone_name, zone_id, '已删除区域'), SUM(duration_sec), MAX(reps), MAX(primary_action)
                 FROM equipment_usage WHERE ended_at>=?1
                 GROUP BY zone_id ORDER BY 2 DESC",
            )?
        };
        let mut map = |p: &[&dyn rusqlite::ToSql]| -> Result<Vec<_>, _> {
            stmt.query_map(p, |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                ))
            })?
            .collect()
        };
        match member_id {
            Some(mid) => map(&[&mid, &day_from]),
            None => map(&[&day_from]),
        }
    }

    /// Sessions since `day_from`, newest first, optionally one member.
    pub fn list_sessions(
        &self,
        member_id: Option<&str>,
        day_from: i64,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = if member_id.is_some() {
            conn.prepare(
                "SELECT s.id, s.member_id, s.started_at, s.ended_at, s.duration_sec, s.summary
                 FROM sessions s WHERE s.member_id=?1 AND s.started_at>=?2
                 ORDER BY s.started_at DESC LIMIT ?3",
            )?
        } else {
            conn.prepare(
                "SELECT s.id, s.member_id, s.started_at, s.ended_at, s.duration_sec, s.summary
                 FROM sessions s WHERE s.started_at>=?1
                 ORDER BY s.started_at DESC LIMIT ?2",
            )?
        };
        let mut map = |p: &[&dyn rusqlite::ToSql]| -> Result<Vec<_>, _> {
            stmt.query_map(p, |r| {
                let summary: Option<String> = r.get(5)?;
                let name =
                    serde_json::from_str::<serde_json::Value>(summary.as_deref().unwrap_or("{}"))
                        .ok()
                        .and_then(|v| {
                            v.get("member_name")
                                .and_then(|n| n.as_str())
                                .map(String::from)
                        });
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "member_id": r.get::<_, Option<String>>(1)?,
                    "member_name": name,
                    "started_at": r.get::<_, Option<i64>>(2)?,
                    "ended_at": r.get::<_, Option<i64>>(3)?,
                    "duration_sec": r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                }))
            })?
            .collect()
        };
        match member_id {
            Some(mid) => map(&[&mid, &day_from, &limit]),
            None => map(&[&day_from, &limit]),
        }
    }

    // ---- heatmap persistence (P1) ----

    /// Returns (grid, day) for `day`; empty grid when absent.
    pub fn load_heatmap(&self, day: i64) -> Option<(Vec<u32>, i64)> {
        let conn = self.conn.lock();
        let raw: Option<String> = conn
            .query_row(
                "SELECT grid FROM heatmap_day WHERE day=?1",
                params![day],
                |r| r.get(0),
            )
            .ok();
        let raw = raw?;
        let grid: Vec<u32> = serde_json::from_str(&raw).ok()?;
        Some((grid, day))
    }

    pub fn save_heatmap(&self, day: i64, grid: &[u32]) -> Result<(), rusqlite::Error> {
        let raw = serde_json::to_string(grid).unwrap();
        self.conn.lock().execute(
            "INSERT INTO heatmap_day(day,grid,updated_at) VALUES(?1,?2,?3)
             ON CONFLICT(day) DO UPDATE SET grid=excluded.grid, updated_at=excluded.updated_at",
            params![day, raw, now_secs()],
        )?;
        Ok(())
    }

    /// Per-DAY, per-exercise history for one member:
    /// (day CE, exercise, sets, reps, secs). Powers the modal's 健身历史.
    pub fn member_daily_history(
        &self,
        member_id: &str,
        since: i64,
    ) -> Result<Vec<(i64, String, i64, i64, i64)>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT eu.started_at, eu.exercise, SUM(eu.sets), SUM(eu.reps), SUM(eu.duration_sec)
             FROM exercise_usage eu
             JOIN sessions s ON s.id = eu.session_id
             WHERE eu.member_id=?1 AND eu.ended_at>=?2
             GROUP BY eu.started_at, eu.exercise
             ORDER BY eu.started_at DESC",
        )?;
        // bucket by local day from started_at
        use chrono::{Datelike, Local, TimeZone};
        let mut out: Vec<(i64, String, i64, i64, i64)> = Vec::new();
        let rows = stmt.query_map(params![member_id, since], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            ))
        })?;
        let mut by_day: std::collections::BTreeMap<i64, Vec<(String, i64, i64, i64)>> =
            std::collections::BTreeMap::new();
        for (started, ex, sets, reps, secs) in rows.flatten() {
            let day = if started > 0 {
                Local
                    .timestamp_opt(started, 0)
                    .single()
                    .map(|d| d.num_days_from_ce() as i64)
                    .unwrap_or(0)
            } else {
                0
            };
            by_day.entry(day).or_default().push((ex, sets, reps, secs));
        }
        for (day, mut exs) in by_day {
            exs.sort_by(|a, b| b.3.cmp(&a.3));
            for (ex, sets, reps, secs) in exs {
                out.push((day, ex, sets, reps, secs));
            }
        }
        Ok(out)
    }

    /// Identity-sample counts for one member: (body, face).
    pub fn member_sample_counts(&self, member_id: &str) -> (i64, i64) {
        self.conn
            .lock()
            .query_row(
                "SELECT
                   SUM(CASE WHEN kind='face' THEN 0 ELSE 1 END),
                   SUM(CASE WHEN kind='face' THEN 1 ELSE 0 END)
                 FROM member_embeddings WHERE member_id=?1",
                params![member_id],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    ))
                },
            )
            .unwrap_or((0, 0))
    }

    /// Append an alert (called from push_alert via maybe_save flush).
    pub fn insert_alert(
        &self,
        ts: f64,
        kind: &str,
        level: &str,
        track_id: i64,
        message: &str,
    ) -> Result<(), rusqlite::Error> {
        self.conn.lock().execute(
            "INSERT INTO alerts_log(ts,kind,level,track_id,message,created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![ts, kind, level, track_id, message, now_secs()],
        )?;
        Ok(())
    }

    /// Newest-first persisted alerts, unresolved first, capped.
    pub fn list_alerts(&self, limit: i64) -> Vec<serde_json::Value> {
        self.conn
            .lock()
            .prepare(
                "SELECT id, ts, kind, level, track_id, message, resolved
                 FROM alerts_log ORDER BY resolved ASC, id DESC LIMIT ?1",
            )
            .and_then(|mut stmt| {
                let rows = stmt.query_map(params![limit], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "ts": r.get::<_, f64>(1)?,
                        "kind": r.get::<_, String>(2)?,
                        "level": r.get::<_, String>(3)?,
                        "track_id": r.get::<_, i64>(4)?,
                        "message": r.get::<_, String>(5)?,
                        "resolved": r.get::<_, i64>(6)? != 0,
                    }))
                })?;
                rows.collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_default()
    }

    /// Mark an alert resolved (acknowledged / false positive).
    pub fn resolve_alert(&self, id: i64) -> Result<usize, rusqlite::Error> {
        Ok(self
            .conn
            .lock()
            .execute("UPDATE alerts_log SET resolved=1 WHERE id=?1", params![id])?)
    }

    /// Prune resolved alerts older than `days`.
    pub fn prune_alerts(&self, days: i64) {
        let _ = self.conn.lock().execute(
            "DELETE FROM alerts_log WHERE resolved=1 AND ts < ?1",
            params![chrono::Utc::now().timestamp() as f64 - (days * 86400) as f64],
        );
    }

    /// Per-exercise totals for one member since `since`: (exercise, sets,
    /// reps, secs, session_count). The panel's 动作明细 rides this.
    pub fn member_exercise_stats(
        &self,
        member_id: &str,
        since: i64,
    ) -> Result<Vec<(String, i64, i64, i64, i64)>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT exercise, SUM(sets), SUM(reps), SUM(duration_sec), COUNT(DISTINCT session_id)
             FROM exercise_usage WHERE member_id=?1 AND ended_at>=?2
             GROUP BY exercise ORDER BY 4 DESC",
        )?;
        let rows = stmt.query_map(params![member_id, since], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            ))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Batch-insert foot samples (ts unix seconds).
    pub fn insert_foot_log(&self, rows: &[(i64, i64, f32, f32)]) {
        if rows.is_empty() {
            return;
        }
        let mut conn = self.conn.lock();
        let _ = conn.execute("BEGIN", []);
        let mut stmt = match conn
            .prepare("INSERT INTO foot_log(ts,track_id,x,y) VALUES(?1,?2,?3,?4)")
        {
            Ok(s) => s,
            Err(_) => {
                let _ = conn.execute("ROLLBACK", []);
                return;
            }
        };
        for (ts, tid, x, y) in rows {
            let _ = stmt.execute(params![ts, tid, x, y]);
        }
        drop(stmt);
        let _ = conn.execute("COMMIT", []);
    }

    /// Drop samples older than `before` (unix seconds).
    pub fn prune_foot_log(&self, before: i64) {
        let _ = self
            .conn
            .lock()
            .execute("DELETE FROM foot_log WHERE ts < ?1", params![before]);
    }

    /// 64×36 heatmap grid aggregated over [start, end] (unix seconds).
    pub fn activity_grid(&self, start: i64, end: i64) -> Vec<u32> {
        const COLS: i64 = 64;
        const ROWS: i64 = 36;
        let mut grid = vec![0u32; (COLS * ROWS) as usize];
        let ok = self
            .conn
            .lock()
            .prepare(
                "SELECT (CAST(MIN(x, 0.9999) * 64 AS INTEGER))
                       + (CAST(MIN(y, 0.9999) * 36 AS INTEGER)) * 64 AS b,
                        COUNT(*)
                 FROM foot_log WHERE ts BETWEEN ?1 AND ?2 GROUP BY b",
            )
            .and_then(|mut stmt| {
                let rows = stmt.query_map(params![start, end], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u32))
                })?;
                for row in rows.flatten() {
                    let (b, n) = row;
                    if (0 as usize) <= (b as usize) && (b as usize) < grid.len() {
                        grid[b as usize] = n;
                    }
                }
                Ok(())
            });
        match ok {
            Ok(()) => grid,
            Err(_) => grid,
        }
    }

    /// Row count over the window (for the UI's sample badge).
    pub fn activity_trails_count(&self, start: i64, end: i64) -> i64 {
        self.conn
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM foot_log WHERE ts BETWEEN ?1 AND ?2",
                params![start, end],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// Per-track polylines over [start, end], oldest first, capped.
    pub fn activity_trails(
        &self,
        start: i64,
        end: i64,
        cap: usize,
    ) -> Vec<(i64, Vec<(f32, f32)>)> {
        use std::collections::BTreeMap;
        let mut by_track: BTreeMap<i64, Vec<(f32, f32)>> = BTreeMap::new();
        let _ = self
            .conn
            .lock()
            .prepare(
                "SELECT track_id, x, y FROM foot_log
                 WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts ASC LIMIT ?3",
            )
            .and_then(|mut stmt| {
                let rows = stmt.query_map(params![start, end, cap as i64], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, f32>(1)?, r.get::<_, f32>(2)?))
                })?;
                for (tid, x, y) in rows.flatten() {
                    by_track.entry(tid).or_default().push((x, y));
                }
                Ok(())
            });
        by_track.into_iter().collect()
    }

    /// Upsert one (session × exercise) accumulation row.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_exercise_usage(
        &self,
        session_id: &str,
        member_id: Option<&str>,
        exercise: &str,
        sets: i64,
        reps: i64,
        duration_sec: i64,
    ) -> Result<(), rusqlite::Error> {
        let now = chrono::Utc::now().timestamp();
        self.conn.lock().execute(
            "INSERT INTO exercise_usage(session_id,member_id,exercise,sets,reps,duration_sec,started_at,ended_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?7)
             ON CONFLICT(session_id,exercise) DO UPDATE SET
               member_id=excluded.member_id, sets=excluded.sets, reps=excluded.reps,
               duration_sec=excluded.duration_sec, ended_at=excluded.ended_at",
            params![session_id, member_id, exercise, sets, reps, duration_sec, now],
        )?;
        Ok(())
    }

    /// Today's persisted crossing counters: (line_id, in_count, out_count).
    pub fn load_crossings(&self, day: i64) -> Vec<(String, u64, u64)> {
        self.conn
            .lock()
            .prepare("SELECT line_id, in_count, out_count FROM crossing_day WHERE day=?1")
            .and_then(|mut stmt| {
                let rows = stmt.query_map(params![day], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)? as u64,
                        r.get::<_, i64>(2)? as u64,
                    ))
                })?;
                rows.collect()
            })
            .unwrap_or_default()
    }

    /// Upsert one line's daily counter.
    pub fn save_crossing(
        &self,
        line_id: &str,
        day: i64,
        in_count: u64,
        out_count: u64,
    ) -> Result<(), rusqlite::Error> {
        self.conn.lock().execute(
            "INSERT INTO crossing_day(line_id,day,in_count,out_count,updated_at)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(line_id,day) DO UPDATE SET
               in_count=excluded.in_count, out_count=excluded.out_count, updated_at=excluded.updated_at",
            params![line_id, day, in_count as i64, out_count as i64, now_secs()],
        )?;
        Ok(())
    }

    /// Daily in/out totals across all lines since `since_day` (CE days,
    /// ascending). Gaps simply have no row — the frontend fills zeros.
    pub fn list_crossing_history(&self, since_day: i64) -> Vec<(i64, u64, u64)> {
        self.conn
            .lock()
            .prepare(
                "SELECT day, SUM(in_count), SUM(out_count) FROM crossing_day
                 WHERE day >= ?1 GROUP BY day ORDER BY day ASC",
            )
            .and_then(|mut stmt| {
                let rows = stmt.query_map(params![since_day], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                        r.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                    ))
                })?;
                rows.collect()
            })
            .unwrap_or_default()
    }

    // ---- members (P3: body-ReID member library) ----

    pub fn insert_member(
        &self,
        id: &str,
        name: &str,
        embedding: &[f32],
    ) -> Result<(), rusqlite::Error> {
        // embedding rides the BLOB column as JSON — self-describing and
        // debuggable; a raw f32 LE blob would be smaller but opaque.
        let raw = serde_json::to_string(embedding).unwrap();
        self.conn.lock().execute(
            "INSERT INTO members(id, display_name, is_enrolled, source, embedding, first_seen_at, created_at, updated_at)
             VALUES(?1, ?2, 1, 'manual', ?3, ?4, ?4, ?4)",
            params![id, name, raw, now_secs()],
        )?;
        Ok(())
    }

    /// Auto-enrollment variant (source='auto'): an unnamed library entry
    /// created when an unknown person's embedding first appears. The name
    /// is `{prefix}-{n}` with n past the highest existing auto index —
    /// stable across deletions (no reuse of freed numbers).
    pub fn insert_auto_member(
        &self,
        prefix: &str,
        embedding: &[f32],
    ) -> Result<Member, rusqlite::Error> {
        let conn = self.conn.lock();
        let hi: i64 = conn.query_row(
            "SELECT COALESCE(MAX(CAST(SUBSTR(display_name, LENGTH(?1)+2) AS INTEGER)), 0)
             FROM members WHERE source='auto' AND display_name LIKE ?1 || '-%'",
            params![prefix],
            |r| r.get(0),
        )?;
        let name = format!("{prefix}-{}", hi + 1);
        let id = format!("member_{}", uuid::Uuid::new_v4().simple());
        let raw = serde_json::to_string(embedding).unwrap();
        let now = now_secs();
        conn.execute(
            "INSERT INTO members(id, display_name, is_enrolled, source, embedding, first_seen_at, created_at, updated_at)
             VALUES(?1, ?2, 0, 'auto', ?3, ?4, ?4, ?4)",
            params![id, name, raw, now],
        )?;
        Ok(Member {
            id,
            name,
            source: "auto".into(),
            embedding: embedding.to_vec(),
            extra_embeddings: Vec::new(),
            face_embeddings: Vec::new(),
            photo: None,
            created_at: Some(now),
        })
    }

    /// Store / clear a member's avatar photo (base64 JPEG thumbnail).
    /// Pass `None` to remove. Returns false when the member id is unknown.
    pub fn set_member_photo(&self, id: &str, photo: Option<&str>) -> Result<bool, rusqlite::Error> {
        let n = self.conn.lock().execute(
            "UPDATE members SET photo=?2, updated_at=?3 WHERE id=?1",
            params![id, photo, now_secs()],
        )?;
        Ok(n > 0)
    }

    /// Rename a member (fills in / corrects the display name later).
    pub fn rename_member(&self, id: &str, name: &str) -> Result<usize, rusqlite::Error> {
        let n = self.conn.lock().execute(
            "UPDATE members SET display_name=?2, is_enrolled=1, updated_at=?3 WHERE id=?1",
            params![id, name, now_secs()],
        )?;
        Ok(n)
    }

    pub fn list_members(&self) -> Result<Vec<Member>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, display_name, source, embedding, created_at, photo FROM members ORDER BY created_at")?;
        let members: Vec<Member> = {
            let rows = stmt.query_map([], |r| {
                let raw: Option<String> = r.get(3)?;
                let embedding: Vec<f32> = raw
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                Ok(Member {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    source: r.get(2)?,
                    embedding,
                    extra_embeddings: Vec::new(),
                    face_embeddings: Vec::new(),
                    photo: r.get(5)?,
                    created_at: r.get(4)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        drop(stmt);
        // attach the accumulated samples, split body|face (one query)
        let mut extras: HashMap<String, (Vec<Vec<f32>>, Vec<Vec<f32>>)> = HashMap::new();
        {
            let mut stmt = conn
                .prepare("SELECT member_id, embedding, kind FROM member_embeddings ORDER BY id")?;
            let rows = stmt.query_map([], |r| {
                let mid: String = r.get(0)?;
                let raw: String = r.get(1)?;
                let kind: String = r.get(2)?;
                let emb: Vec<f32> = serde_json::from_str(&raw).unwrap_or_default();
                Ok((mid, kind, emb))
            })?;
            for row in rows {
                let (mid, kind, emb) = row?;
                if emb.is_empty() {
                    continue;
                }
                let e = extras.entry(mid).or_default();
                if kind == "face" {
                    e.1.push(emb)
                } else {
                    e.0.push(emb)
                }
            }
        }
        Ok(members
            .into_iter()
            .map(|mut m| {
                let (body, face) = extras.remove(&m.id).unwrap_or_default();
                m.extra_embeddings = body;
                m.face_embeddings = face;
                m
            })
            .collect())
    }

    /// Append a new sample to a member's embedding library.
    ///
    /// Gated by the caller (confidence + diversity + cooldown live in
    /// commands.rs where the live state is); this only enforces the hard
    /// cap. Returns true when the sample was stored.
    pub fn append_member_embedding(
        &self,
        member_id: &str,
        embedding: &[f32],
    ) -> Result<bool, rusqlite::Error> {
        self.append_member_embedding_kind(member_id, embedding, "body")
    }

    /// kind = body (osnet) | face (arcface). The per-kind cap reserves the
    /// library for both modalities independently.
    pub fn append_member_embedding_kind(
        &self,
        member_id: &str,
        embedding: &[f32],
        kind: &str,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM member_embeddings WHERE member_id=?1 AND kind=?2",
            params![member_id, kind],
            |r| r.get(0),
        )?;
        // +1: members.embedding is body sample #0 (face has no primary)
        let cap = if kind == "face" {
            crate::config::MAX_FACE_EMBEDDINGS_PER_MEMBER
        } else {
            crate::config::MAX_EMBEDDINGS_PER_MEMBER - 1
        } as i64;
        if n >= cap {
            return Ok(false);
        }
        let raw = serde_json::to_string(embedding).unwrap();
        conn.execute(
            "INSERT INTO member_embeddings(member_id, embedding, kind, created_at) VALUES(?1, ?2, ?3, ?4)",
            params![member_id, raw, kind, now_secs()],
        )?;
        Ok(true)
    }

    /// Timestamp of the member's most recent sample append (0 when none).
    pub fn last_embedding_append_at(&self, member_id: &str) -> i64 {
        self.last_embedding_append_kind(member_id, "body")
    }

    pub fn last_embedding_append_kind(&self, member_id: &str, kind: &str) -> i64 {
        self.conn.lock().query_row(
            "SELECT COALESCE(MAX(created_at), 0) FROM member_embeddings WHERE member_id=?1 AND kind=?2",
            params![member_id, kind], |r| r.get(0))
            .unwrap_or(0)
    }

    /// Merge `src` into `dst`: src's primary embedding becomes an extra
    /// sample of dst, src's extras move over, src is deleted. This is the
    /// human confirmation path for outfit changes — a new outfit
    /// legitimately looks like a stranger to body-ReID and lands as its
    /// own auto entry; merging reunifies the identities.
    pub fn merge_members(&self, src_id: &str, dst_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        let raw: Option<String> = conn
            .query_row(
                "SELECT embedding FROM members WHERE id=?1",
                params![src_id],
                |r| r.get(0),
            )
            .ok();
        if let Some(raw) = raw {
            conn.execute(
                "INSERT INTO member_embeddings(member_id, embedding, created_at) VALUES(?1, ?2, ?3)",
                params![dst_id, raw, now_secs()],
            )?;
        }
        conn.execute(
            "UPDATE member_embeddings SET member_id=?2 WHERE member_id=?1",
            params![src_id, dst_id],
        )?;
        // respect the per-kind caps: keep the newest samples if over
        for (kind, cap) in [
            ("body", crate::config::MAX_EMBEDDINGS_PER_MEMBER as i64 - 1),
            ("face", crate::config::MAX_FACE_EMBEDDINGS_PER_MEMBER as i64),
        ] {
            conn.execute(
                "DELETE FROM member_embeddings WHERE member_id=?1 AND kind=?3 AND id NOT IN (
                     SELECT id FROM member_embeddings WHERE member_id=?1 AND kind=?3
                     ORDER BY id DESC LIMIT ?2)",
                params![dst_id, cap, kind],
            )?;
        }
        conn.execute("DELETE FROM members WHERE id=?1", params![src_id])?;
        Ok(())
    }

    pub fn delete_member(&self, id: &str) -> Result<usize, rusqlite::Error> {
        let mut conn = self.conn.lock();
        let _ = conn.execute("BEGIN", []);
        // history stays (traffic analytics), but anonymized: sessions keep
        // their member_name snapshot, member_id cleared so the report
        // stops listing the deleted member as a ghost row
        let _ = conn.execute(
            "UPDATE sessions SET member_id=NULL WHERE member_id=?1",
            params![id],
        );
        let _ = conn.execute(
            "UPDATE exercise_usage SET member_id=NULL WHERE member_id=?1",
            params![id],
        );
        conn.execute(
            "DELETE FROM member_embeddings WHERE member_id=?1",
            params![id],
        )?;
        let n = conn.execute("DELETE FROM members WHERE id=?1", params![id])?;
        let _ = conn.execute("COMMIT", []);
        Ok(n)
    }
}

/// A registered gym member with their body-ReID embedding (osnet, 512-d).
/// `source` is "manual" (registered from the Monitor) or "auto"
/// (auto-enrolled unknown, name filled in later via rename).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source: String,
    /// Primary (first) sample — members.embedding column.
    pub embedding: Vec<f32>,
    /// Additional accumulated BODY samples (member_embeddings, kind=body).
    #[serde(default)]
    pub extra_embeddings: Vec<Vec<f32>>,
    /// FACE identity samples (arcface; kind=face). The identity ANCHOR:
    /// a face match confirms who the person is regardless of clothing.
    #[serde(default)]
    pub face_embeddings: Vec<Vec<f32>>,
    /// Avatar thumbnail — base64 JPEG captured by the Monitor from the
    /// raw video frame (before mosaic). None until a photo is captured.
    #[serde(default)]
    pub photo: Option<String>,
    pub created_at: Option<i64>,
}

impl Member {
    pub fn all_embeddings(&self) -> impl Iterator<Item = &[f32]> {
        std::iter::once(self.embedding.as_slice())
            .chain(self.extra_embeddings.iter().map(|v| v.as_slice()))
    }
}

/// Euclidean (L2) distance between two embeddings (∞ when lengths mismatch —
/// callers treat anything above the threshold as no-match).
///
/// L2, not cosine: the device's raw osnet output is a uint8 quantization
/// whose shared bias direction gives unrelated inputs ~0.999 cosine; L2
/// separates them cleanly (probe: same input 0.0, different ~52).
pub fn l2_dist(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::INFINITY;
    }
    let mut s = 0.0f32;
    for i in 0..a.len() {
        let d = a[i] - b[i];
        s += d * d;
    }
    s.sqrt()
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zone_crud_and_unique_name() {
        let db = Db::open(":memory:").unwrap();
        let z = Zone {
            id: "u1".into(),
            name: "跑步机区".into(),
            equipment_type: "treadmill".into(),
            polygon: vec![(0.1, 0.1), (0.4, 0.1), (0.4, 0.5), (0.1, 0.5)],
            enabled: true,
        };
        db.upsert_zone(&z).unwrap();
        assert_eq!(db.list_zones().unwrap().len(), 1);
        assert_eq!(
            db.list_zones().unwrap()[0].polygon,
            z.polygon,
            "polygon round-trips"
        );
        // duplicate name (different id) must fail due to UNIQUE(name)
        let mut z2 = z.clone();
        z2.id = "u2".into();
        assert!(db.upsert_zone(&z2).is_err(), "duplicate name must fail");
        // rename via upsert(same id) preserves association (FK is zones.id, stable uuid)
        let mut renamed = z.clone();
        renamed.name = "有氧区".into();
        db.upsert_zone(&renamed).unwrap();
        assert_eq!(db.list_zones().unwrap()[0].name, "有氧区");
        db.delete_zone("u1").unwrap();
        assert!(db.list_zones().unwrap().is_empty());
    }

    #[test]
    fn zone_delete_keeps_usage_history() {
        let db = Db::open(":memory:").unwrap();
        let fk_on: i64 = db
            .conn
            .lock()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            fk_on, 1,
            "FK enforcement must be on or the detach path is untested"
        );
        let z = Zone {
            id: "z1".into(),
            name: "力量区".into(),
            equipment_type: "squat_rack".into(),
            polygon: vec![(0.1, 0.1), (0.4, 0.1), (0.4, 0.5), (0.1, 0.5)],
            enabled: true,
        };
        db.upsert_zone(&z).unwrap();
        db.upsert_equipment_usage("u1", "s1", None, "z1", 30, "squat", 8)
            .unwrap();
        // Previously this DELETE hit the zones(id) FK and failed forever.
        db.delete_zone("z1").unwrap();
        assert!(db.list_zones().unwrap().is_empty());
        let (zone_id, dur): (Option<String>, i64) = db
            .conn
            .lock()
            .query_row(
                "SELECT zone_id, duration_sec FROM equipment_usage WHERE id='u1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(zone_id, None, "usage row detached, not deleted");
        assert_eq!(dur, 30);
        // zone_name resolves to the real name while the zone exists
        let name: String = db
            .conn
            .lock()
            .query_row(
                "SELECT zone_name FROM equipment_usage WHERE id='u1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "力量区", "upsert stores the zone NAME, not the id");
        // stats must survive the detached row (zone_id NULL) — this read
        // crashed with "Invalid column type Null" before the COALESCE fix
        let stats = db.equipment_stats(None, 0).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(
            stats[0].0, "力量区",
            "detached row keeps its label via zone_name"
        );
    }

    #[test]
    fn replace_zones_atomic_and_fk_safe() {
        let db = Db::open(":memory:").unwrap();
        let old = Zone {
            id: "z-old".into(),
            name: "旧区".into(),
            equipment_type: "treadmill".into(),
            polygon: vec![(0.1, 0.1), (0.2, 0.1), (0.2, 0.2)],
            enabled: true,
        };
        db.upsert_zone(&old).unwrap();
        db.upsert_equipment_usage("u1", "s1", None, "z-old", 60, "walk", 0)
            .unwrap();
        let new = Zone {
            id: "z-new".into(),
            name: "新区".into(),
            equipment_type: "bench".into(),
            polygon: vec![(0.5, 0.5), (0.6, 0.5), (0.6, 0.6)],
            enabled: true,
        };
        db.replace_zones(&[new]).unwrap();
        let zones = db.list_zones().unwrap();
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].id, "z-new");
        let n: i64 = db
            .conn
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM equipment_usage WHERE id='u1' AND zone_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "old zone's usage detached by replace");
        // A failing replace (UNIQUE name collision within the set) must roll
        // back entirely — no half-replaced table.
        let a = Zone {
            id: "a".into(),
            name: "同名".into(),
            equipment_type: "bench".into(),
            polygon: vec![(0.1, 0.1), (0.2, 0.1), (0.2, 0.2)],
            enabled: true,
        };
        let b = Zone {
            id: "b".into(),
            name: "同名".into(),
            equipment_type: "bench".into(),
            polygon: vec![(0.3, 0.3), (0.4, 0.3), (0.4, 0.4)],
            enabled: true,
        };
        assert!(db.replace_zones(&[a, b]).is_err());
        assert_eq!(db.list_zones().unwrap().len(), 1, "rollback kept prior set");
        assert_eq!(db.list_zones().unwrap()[0].id, "z-new");
    }

    #[test]
    fn member_crud_and_l2() {
        let db = Db::open(":memory:").unwrap();
        db.insert_member("m1", "张三", &[1.0, 0.0, 0.0]).unwrap();
        db.insert_member("m2", "李四", &[0.0, 1.0, 0.0]).unwrap();
        let ms = db.list_members().unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].name, "张三");
        assert_eq!(ms[0].embedding, vec![1.0, 0.0, 0.0]);

        assert_eq!(l2_dist(&[1.0, 0.0], &[1.0, 0.0]), 0.0);
        assert!((l2_dist(&[1.0, 0.0], &[0.0, 1.0]) - std::f32::consts::SQRT_2).abs() < 1e-5);
        assert_eq!(
            l2_dist(&[1.0], &[1.0, 2.0]),
            f32::INFINITY,
            "length mismatch → ∞"
        );
        assert_eq!(l2_dist(&[], &[]), f32::INFINITY);

        assert_eq!(db.delete_member("m1").unwrap(), 1);
        assert_eq!(db.delete_member("m1").unwrap(), 0);
        assert_eq!(db.list_members().unwrap().len(), 1);
    }
}
