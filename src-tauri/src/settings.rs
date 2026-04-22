//! User-preference store — SQLite key-value, persists across restarts.
//!
//! Kept in its own DB file (`settings.db`) next to `dict.db` so the two
//! concerns stay separable. Settings are tiny and read-heavy; the dictionary
//! is append-heavy and queried per-dictation. Splitting avoids their locks
//! ever contending.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Arc;

pub const KEY_WHISPER_MODEL: &str = "whisper_model";

pub struct Settings {
    conn: Arc<Mutex<Connection>>,
}

impl Settings {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open settings db at {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS settings (
                 key        TEXT PRIMARY KEY,
                 value      TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
        )
        .context("init settings schema")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        match stmt.query_row(params![key], |row| row.get::<_, String>(0)) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_or_default(&self, key: &str, default: &str) -> String {
        self.get(key).ok().flatten().unwrap_or_else(|| default.to_string())
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings::in_memory().unwrap();
        assert!(s.get("x").unwrap().is_none());
        s.set("x", "hello").unwrap();
        assert_eq!(s.get("x").unwrap(), Some("hello".to_string()));
    }

    #[test]
    fn update_overwrites() {
        let s = Settings::in_memory().unwrap();
        s.set("k", "first").unwrap();
        s.set("k", "second").unwrap();
        assert_eq!(s.get("k").unwrap(), Some("second".to_string()));
    }

    #[test]
    fn get_or_default_works() {
        let s = Settings::in_memory().unwrap();
        assert_eq!(s.get_or_default("absent", "fallback"), "fallback");
        s.set("absent", "real").unwrap();
        assert_eq!(s.get_or_default("absent", "fallback"), "real");
    }
}
