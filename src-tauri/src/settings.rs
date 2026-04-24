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
pub const KEY_OLLAMA_MODEL: &str = "ollama_model";
pub const KEY_USER_NAME: &str = "user_name";
pub const KEY_AI_CLEANUP: &str = "ai_cleanup_enabled";
/// Comma-separated list of skill names the user has turned off. Disabled
/// skills are loaded but skipped during trigger matching. Stored in settings
/// (not deleted) so toggling them back on is instant and preserves any edits.
pub const KEY_DISABLED_SKILLS: &str = "disabled_skills";

pub const DEFAULT_AI_CLEANUP: bool = true;

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

    /// Return the set of skill names the user has turned off.
    pub fn disabled_skills(&self) -> std::collections::HashSet<String> {
        let raw = self.get_or_default(KEY_DISABLED_SKILLS, "");
        raw.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    /// Add or remove `name` from the disabled-skills list.
    pub fn set_skill_disabled(&self, name: &str, disabled: bool) -> Result<()> {
        let mut set = self.disabled_skills();
        if disabled {
            set.insert(name.to_string());
        } else {
            set.remove(name);
        }
        // Stable alphabetical order so the DB diff stays predictable.
        let mut list: Vec<String> = set.into_iter().collect();
        list.sort();
        self.set(KEY_DISABLED_SKILLS, &list.join(","))
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

    #[test]
    fn disabled_skills_roundtrip() {
        let s = Settings::in_memory().unwrap();
        assert!(s.disabled_skills().is_empty());
        s.set_skill_disabled("email", true).unwrap();
        s.set_skill_disabled("prompt-better", true).unwrap();
        let set = s.disabled_skills();
        assert!(set.contains("email"));
        assert!(set.contains("prompt-better"));
        s.set_skill_disabled("email", false).unwrap();
        let set = s.disabled_skills();
        assert!(!set.contains("email"));
        assert!(set.contains("prompt-better"));
    }

    #[test]
    fn disabled_skills_ignores_blank_entries() {
        let s = Settings::in_memory().unwrap();
        // Simulate a corrupted/empty setting: double-comma, trailing comma.
        s.set(KEY_DISABLED_SKILLS, ",,email,, ,").unwrap();
        let set = s.disabled_skills();
        assert_eq!(set.len(), 1);
        assert!(set.contains("email"));
    }

}
