use anyhow::Result;
use rusqlite::{Connection, params};
use crate::session::{ActivitySnapshot, NetworkConnection, AppConfig};

pub struct LocalDb {
    conn: Connection,
}

impl LocalDb {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = LocalDb { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS config (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS pending_snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id  TEXT NOT NULL,
                data        TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                synced      INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS pending_network (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id  TEXT NOT NULL,
                data        TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                synced      INTEGER NOT NULL DEFAULT 0
            );
        ")?;
        Ok(())
    }

    pub fn save_config(&self, cfg: &AppConfig) -> Result<()> {
        let pairs = [
            ("pb_url", &cfg.pb_url),
            ("pb_email", &cfg.pb_email),
            ("pb_token", &cfg.pb_token),
            ("user_id", &cfg.user_id),
        ];
        for (k, v) in &pairs {
            self.conn.execute(
                "INSERT INTO config (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![k, v],
            )?;
        }
        Ok(())
    }

    pub fn load_config(&self) -> Result<AppConfig> {
        let mut cfg = AppConfig::default();
        let mut stmt = self.conn.prepare("SELECT key, value FROM config")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows.flatten() {
            match row.0.as_str() {
                "pb_url" => cfg.pb_url = row.1,
                "pb_email" => cfg.pb_email = row.1,
                "pb_token" => cfg.pb_token = row.1,
                "user_id" => cfg.user_id = row.1,
                _ => {}
            }
        }
        Ok(cfg)
    }

    pub fn queue_snapshot(&self, session_id: &str, snap: &ActivitySnapshot) -> Result<()> {
        let data = serde_json::to_string(snap)?;
        self.conn.execute(
            "INSERT INTO pending_snapshots (session_id, data, created_at) VALUES (?1, ?2, ?3)",
            params![session_id, data, snap.timestamp.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn queue_network(&self, session_id: &str, conns: &[NetworkConnection]) -> Result<()> {
        for conn in conns {
            let data = serde_json::to_string(conn)?;
            self.conn.execute(
                "INSERT INTO pending_network (session_id, data, created_at) VALUES (?1, ?2, ?3)",
                params![session_id, data, conn.timestamp.to_rfc3339()],
            )?;
        }
        Ok(())
    }

    pub fn get_unsynced_snapshots(&self) -> Result<Vec<(i64, String, ActivitySnapshot)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, data FROM pending_snapshots WHERE synced = 0 LIMIT 20",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows.flatten() {
            if let Ok(snap) = serde_json::from_str::<ActivitySnapshot>(&row.2) {
                result.push((row.0, row.1, snap));
            }
        }
        Ok(result)
    }

    pub fn get_unsynced_network(&self) -> Result<Vec<(i64, String, NetworkConnection)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, data FROM pending_network WHERE synced = 0 LIMIT 50",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows.flatten() {
            if let Ok(conn) = serde_json::from_str::<NetworkConnection>(&row.2) {
                result.push((row.0, row.1, conn));
            }
        }
        Ok(result)
    }

    pub fn mark_snapshot_synced(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE pending_snapshots SET synced = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn mark_network_synced(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE pending_network SET synced = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }
}
