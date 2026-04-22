//! Personal dictionary — SQLite-backed store for names, jargon, and other
//! terms the user wants Svara to get right every time.
//!
//! Two consumers:
//! 1. Whisper — top-N terms injected as `initial_prompt` to bias decoding.
//! 2. Ollama cleanup — full list passed as a "preserve these terms exactly"
//!    clause in the system prompt.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use regex::Regex;
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

/// Deterministic post-processor: rewrite occurrences of dictionary terms
/// (in any casing, with or without spaces/hyphens at camelCase boundaries)
/// to the canonical form stored in the dictionary.
///
/// Example: term "HomeLane" matches "HomeLane", "homelane", "HOMELANE",
/// "Home Lane", "home lane", "Home-Lane". All become "HomeLane".
///
/// Word-boundary anchored (`\b`) so "homelane" inside "homelaner" is left
/// alone. Idempotent — running twice produces the same output.
pub fn apply_to_text(text: &str, terms: &[String]) -> String {
    let mut out = text.to_string();
    for term in terms {
        let canonical = term.trim();
        if canonical.is_empty() {
            continue;
        }
        out = apply_one_term(&out, canonical);
    }
    out
}

fn apply_one_term(text: &str, canonical: &str) -> String {
    let variants = variants_of(canonical);
    let escaped: Vec<String> = variants.iter().map(|v| regex::escape(v)).collect();
    let pattern = format!(r"(?i)\b(?:{})\b", escaped.join("|"));
    match Regex::new(&pattern) {
        Ok(re) => re.replace_all(text, canonical).to_string(),
        Err(_) => text.to_string(),
    }
}

/// Generate surface forms Whisper is likely to emit for a given canonical
/// term. For "HomeLane" that's ["HomeLane", "Home Lane", "Home-Lane"].
/// The `(?i)` flag in the regex handles lowercase/uppercase variation,
/// so we only enumerate the *shape* variants here.
fn variants_of(canonical: &str) -> Vec<String> {
    let mut v = vec![canonical.to_string()];
    let spaced = split_camel_case(canonical);
    if spaced != canonical {
        v.push(spaced.clone());
        v.push(spaced.replace(' ', "-"));
    }
    v
}

fn split_camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
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

    // === apply_to_text tests ===

    fn apply(text: &str, terms: &[&str]) -> String {
        let owned: Vec<String> = terms.iter().map(|s| s.to_string()).collect();
        apply_to_text(text, &owned)
    }

    #[test]
    fn rewrites_simple_case_difference() {
        assert_eq!(
            apply("i spoke with vrishti today.", &["Vrishti"]),
            "i spoke with Vrishti today."
        );
    }

    #[test]
    fn rewrites_split_camelcase_term() {
        // The failing HomeLane case: Whisper emits two lowercase words,
        // post-processor must rejoin into the canonical spelling.
        assert_eq!(
            apply("home lane is shipping tomorrow.", &["HomeLane"]),
            "HomeLane is shipping tomorrow."
        );
    }

    #[test]
    fn rewrites_hyphenated_variant() {
        assert_eq!(
            apply("the Home-Lane team met.", &["HomeLane"]),
            "the HomeLane team met."
        );
    }

    #[test]
    fn rewrites_lowercase_joined_variant() {
        assert_eq!(
            apply("homelane shipped.", &["HomeLane"]),
            "HomeLane shipped."
        );
    }

    #[test]
    fn respects_word_boundaries() {
        // "homelaner" should NOT be rewritten to "HomeLaner"
        assert_eq!(
            apply("she's a homelaner through and through.", &["HomeLane"]),
            "she's a homelaner through and through."
        );
    }

    #[test]
    fn is_idempotent() {
        let once = apply("home lane home lane", &["HomeLane"]);
        let twice = apply(&once, &["HomeLane"]);
        assert_eq!(once, twice);
    }

    #[test]
    fn applies_multiple_terms() {
        let out = apply(
            "home lane is using growthbook with vrishti.",
            &["HomeLane", "GrowthBook", "Vrishti"],
        );
        assert_eq!(out, "HomeLane is using GrowthBook with Vrishti.");
    }

    #[test]
    fn leaves_unrelated_text_alone() {
        assert_eq!(
            apply("nothing here matches", &["HomeLane", "Vrishti"]),
            "nothing here matches"
        );
    }
}
