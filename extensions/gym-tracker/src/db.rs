// db.rs
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub equipment_type: String,
    pub polygon: Vec<(f32,f32)>,   // normalized [[x,y]...]
    pub enabled: bool,
}

pub struct Db { conn: Mutex<Connection> }

impl Db {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = if path == ":memory:" { Connection::open_in_memory()? } else { Connection::open(path)? };
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }
    fn migrate(&self) -> Result<(), rusqlite::Error> {
        self.conn.lock().execute_batch(r#"
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
            CREATE TABLE IF NOT EXISTS kv_settings (key TEXT PRIMARY KEY, value TEXT);
        "#)?;
        Ok(())
    }
    pub fn list_zones(&self) -> Result<Vec<Zone>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id,name,equipment_type,polygon,enabled FROM zones ORDER BY name")?;
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
    pub fn delete_zone(&self, id: &str) -> Result<(), rusqlite::Error> {
        self.conn.lock().execute("DELETE FROM zones WHERE id=?1", params![id])?;
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
    pub fn replace_lines(&self, lines: &[crate::analytics::CrossLine]) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM cross_lines", [])?;
        let mut stmt = conn.prepare(
            "INSERT INTO cross_lines(id,name,ax,ay,bx,by,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?7)",
        )?;
        for l in lines {
            stmt.execute(params![l.id, l.name, l.a.0, l.a.1, l.b.0, l.b.1, now_secs()])?;
        }
        Ok(())
    }

    // ---- heatmap persistence (P1) ----

    /// Returns (grid, day) for `day`; empty grid when absent.
    pub fn load_heatmap(&self, day: i64) -> Option<(Vec<u32>, i64)> {
        let conn = self.conn.lock();
        let raw: Option<String> = conn
            .query_row("SELECT grid FROM heatmap_day WHERE day=?1", params![day], |r| r.get(0))
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

    // ---- members (P3: body-ReID member library) ----

    pub fn insert_member(&self, id: &str, name: &str, embedding: &[f32])
        -> Result<(), rusqlite::Error> {
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

    pub fn list_members(&self) -> Result<Vec<Member>, rusqlite::Error> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, display_name, embedding, created_at FROM members ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| {
            let raw: Option<String> = r.get(2)?;
            let embedding: Vec<f32> = raw
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            Ok(Member {
                id: r.get(0)?,
                name: r.get(1)?,
                embedding,
                created_at: r.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete_member(&self, id: &str) -> Result<usize, rusqlite::Error> {
        let n = self.conn.lock().execute("DELETE FROM members WHERE id=?1", params![id])?;
        Ok(n)
    }
}

/// A registered gym member with their body-ReID embedding (osnet, 512-d).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub embedding: Vec<f32>,
    pub created_at: Option<i64>,
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

fn now_secs() -> i64 { chrono::Utc::now().timestamp() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zone_crud_and_unique_name() {
        let db = Db::open(":memory:").unwrap();
        let z = Zone { id: "u1".into(), name: "跑步机区".into(), equipment_type: "treadmill".into(), polygon: vec![(0.1,0.1),(0.4,0.1),(0.4,0.5),(0.1,0.5)], enabled: true };
        db.upsert_zone(&z).unwrap();
        assert_eq!(db.list_zones().unwrap().len(), 1);
        assert_eq!(db.list_zones().unwrap()[0].polygon, z.polygon, "polygon round-trips");
        // duplicate name (different id) must fail due to UNIQUE(name)
        let mut z2 = z.clone(); z2.id = "u2".into();
        assert!(db.upsert_zone(&z2).is_err(), "duplicate name must fail");
        // rename via upsert(same id) preserves association (FK is zones.id, stable uuid)
        let mut renamed = z.clone(); renamed.name = "有氧区".into();
        db.upsert_zone(&renamed).unwrap();
        assert_eq!(db.list_zones().unwrap()[0].name, "有氧区");
        db.delete_zone("u1").unwrap();
        assert!(db.list_zones().unwrap().is_empty());
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
        assert_eq!(l2_dist(&[1.0], &[1.0, 2.0]), f32::INFINITY, "length mismatch → ∞");
        assert_eq!(l2_dist(&[], &[]), f32::INFINITY);

        assert_eq!(db.delete_member("m1").unwrap(), 1);
        assert_eq!(db.delete_member("m1").unwrap(), 0);
        assert_eq!(db.list_members().unwrap().len(), 1);
    }
}
