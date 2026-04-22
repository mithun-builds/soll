//! Personal dictionary — SQLite-backed store for names, jargon, and other
//! terms the user wants Svara to get right every time.
//!
//! Two consumers:
//! 1. Whisper — top-N terms injected as `initial_prompt` to bias decoding.
//! 2. Ollama cleanup — full list passed as a "preserve these terms exactly"
//!    clause in the system prompt.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// Max tokens Whisper's `initial_prompt` will accept (roughly 224 tokens).
/// Stay comfortably below by capping word count.
const MAX_WHISPER_PROMPT_WORDS: usize = 40;

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub word: String,
    pub weight: i32,
    pub added_at: String,
}

pub struct Dictionary {
    conn: Arc<Mutex<Connection>>,
}

impl Dictionary {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("open sqlite at {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS dictionary (
                 word      TEXT PRIMARY KEY COLLATE NOCASE,
                 weight    INTEGER NOT NULL DEFAULT 1,
                 added_at  TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
        )
        .context("init dictionary schema")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// In-memory DB, for tests and throwaway contexts.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE dictionary (
                 word TEXT PRIMARY KEY COLLATE NOCASE,
                 weight INTEGER NOT NULL DEFAULT 1,
                 added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn add(&self, word: &str, weight: i32) -> Result<()> {
        let trimmed = word.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        self.conn.lock().execute(
            "INSERT INTO dictionary (word, weight) VALUES (?1, ?2)
             ON CONFLICT(word) DO UPDATE SET weight = excluded.weight",
            params![trimmed, weight],
        )?;
        Ok(())
    }

    pub fn remove(&self, word: &str) -> Result<usize> {
        Ok(self.conn.lock().execute(
            "DELETE FROM dictionary WHERE word = ?1 COLLATE NOCASE",
            params![word.trim()],
        )?)
    }

    pub fn list(&self) -> Result<Vec<Entry>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT word, weight, added_at
             FROM dictionary
             ORDER BY weight DESC, word COLLATE NOCASE ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Entry {
                    word: row.get(0)?,
                    weight: row.get(1)?,
                    added_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Return the top-N words by weight, as a comma-separated string ready
    /// to pass to `FullParams::set_initial_prompt`. Returns `None` when the
    /// dictionary is empty.
    pub fn whisper_prompt(&self) -> Result<Option<String>> {
        let words = self.top_n(MAX_WHISPER_PROMPT_WORDS)?;
        if words.is_empty() {
            Ok(None)
        } else {
            Ok(Some(words.join(", ")))
        }
    }

    fn top_n(&self, n: usize) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT word FROM dictionary
             ORDER BY weight DESC, added_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![n as i64], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .lock()
            .query_row("SELECT COUNT(*) FROM dictionary", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_list_remove() {
        let d = Dictionary::in_memory().unwrap();
        d.add("Homelane", 5).unwrap();
        d.add("Vrishti", 3).unwrap();
        d.add("GrowthBook", 1).unwrap();

        let list = d.list().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].word, "Homelane"); // highest weight first

        assert_eq!(d.remove("Homelane").unwrap(), 1);
        assert_eq!(d.count().unwrap(), 2);
    }

    #[test]
    fn case_insensitive_unique() {
        let d = Dictionary::in_memory().unwrap();
        d.add("John", 1).unwrap();
        d.add("john", 2).unwrap();
        assert_eq!(d.count().unwrap(), 1);
        let list = d.list().unwrap();
        assert_eq!(list[0].weight, 2); // second insert wins
    }

    #[test]
    fn trims_whitespace_and_skips_empty() {
        let d = Dictionary::in_memory().unwrap();
        d.add("  Alice  ", 1).unwrap();
        d.add("   ", 1).unwrap();
        d.add("", 1).unwrap();
        assert_eq!(d.count().unwrap(), 1);
        assert_eq!(d.list().unwrap()[0].word, "Alice");
    }

    #[test]
    fn whisper_prompt_is_none_when_empty() {
        let d = Dictionary::in_memory().unwrap();
        assert!(d.whisper_prompt().unwrap().is_none());
    }

    #[test]
    fn whisper_prompt_joins_top_n() {
        let d = Dictionary::in_memory().unwrap();
        d.add("Alpha", 10).unwrap();
        d.add("Beta", 5).unwrap();
        let p = d.whisper_prompt().unwrap().unwrap();
        assert_eq!(p, "Alpha, Beta");
    }
}
