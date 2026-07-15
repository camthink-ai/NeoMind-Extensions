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
}
