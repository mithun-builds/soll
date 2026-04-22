//! Auto-formatting for spoken list patterns.
//!
//! Lists require an EXPLICIT trigger word at the start of the utterance:
//!
//!   "bullet list milk, bread and eggs"   -> - Milk
//!                                            - Bread
//!                                            - Eggs
//!
//!   "ordinal list coffee, tea, water"    -> 1. Coffee
//!                                            2. Tea
//!                                            3. Water
//!
//!   "numbered list apples, bananas"      -> 1. Apples
//!                                            2. Bananas
//!
//! No implicit "one X two Y three Z" detection — it was too fragile (Whisper
//! frequently mis-hears "two" as "blue", drops "three", or emits digits that
//! collide with ordinary prose like "3 people at 4 pm"). The explicit prefix
//! makes the feature predictable: users know exactly how to trigger it.
//!
//! When a list is detected we format DETERMINISTICALLY and skip Ollama, because
//! an LLM always turns a structured list back into prose.

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Plain,
    Bullets,
    Numbered,
}

/// Prefixes the user can say to request a bullet list.
const BULLET_PREFIXES: &[&str] = &["bullet list", "bullet point", "bullets", "bullet"];

/// Prefixes the user can say to request a numbered list.
const NUMBERED_PREFIXES: &[&str] = &[
    "ordinal list",
    "ordinal",
    "numbered list",
    "numbered",
    "number list",
];

/// Decide what to do with raw whisper output based on its leading keyword.
pub fn detect(raw: &str) -> Format {
    let lower = raw.trim().to_lowercase();
    for p in BULLET_PREFIXES {
        if starts_with_keyword(&lower, p) {
            return Format::Bullets;
        }
    }
    for p in NUMBERED_PREFIXES {
        if starts_with_keyword(&lower, p) {
            return Format::Numbered;
        }
    }
    Format::Plain
}

/// Apply the chosen format. `Plain` passes through unchanged.
pub fn apply(raw: &str, format: Format) -> String {
    match format {
        Format::Plain => raw.to_string(),
        Format::Bullets => format_as_list(raw, ListKind::Bullets),
        Format::Numbered => format_as_list(raw, ListKind::Numbered),
    }
}

#[derive(Copy, Clone)]
enum ListKind {
    Bullets,
    Numbered,
}

fn format_as_list(raw: &str, kind: ListKind) -> String {
    let stripped = strip_list_prefix(raw);
    let items = split_list_items(&stripped);
    if items.is_empty() {
        return raw.to_string();
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| match kind {
            ListKind::Bullets => format!("- {}", capitalize_first(item)),
            ListKind::Numbered => format!("{}. {}", i + 1, capitalize_first(item)),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── helpers ────────────────────────────────────────────────────────────────

/// True iff `text` begins with `keyword` followed by a non-alphanumeric
/// boundary (whitespace, colon, comma, end-of-string). Prevents "bulletproof"
/// from matching "bullet".
fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    if !text.starts_with(keyword) {
        return false;
    }
    let rest = &text[keyword.len()..];
    rest.chars().next().map_or(true, |c| !c.is_alphanumeric())
}

/// Remove a recognized list-prefix keyword + any immediately following
/// punctuation/whitespace. Returns the untouched string when no prefix hits.
fn strip_list_prefix(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_lowercase();
    for prefix in BULLET_PREFIXES.iter().chain(NUMBERED_PREFIXES.iter()) {
        if starts_with_keyword(&lower, prefix) {
            let remainder = &trimmed[prefix.len()..];
            return remainder
                .trim_start_matches(|c: char| {
                    c == ':' || c == ',' || c == '.' || c.is_whitespace()
                })
                .to_string();
        }
    }
    trimmed.to_string()
}

/// Split a list body into items on: commas, semicolons, newlines, ` and `.
/// Trims each item and drops trailing punctuation.
fn split_list_items(s: &str) -> Vec<String> {
    static AND_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+and\s+").unwrap());

    let mut items: Vec<String> = vec![s.to_string()];
    items = items
        .into_iter()
        .flat_map(|chunk| {
            chunk
                .split(|c: char| c == ',' || c == ';' || c == '\n')
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    items = items
        .into_iter()
        .flat_map(|chunk| {
            AND_RE
                .split(&chunk)
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .collect();

    items
        .into_iter()
        .map(|s| {
            s.trim()
                .trim_end_matches(|c: char| c == '.' || c == ',')
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detection ───────────────────────────────────────────────

    #[test]
    fn plain_prose_stays_plain() {
        assert_eq!(detect("hello world, how are you today?"), Format::Plain);
    }

    #[test]
    fn spoken_ordinals_alone_do_not_trigger() {
        // Without an explicit prefix, "one two three" is just prose.
        assert_eq!(
            detect("one apple two banana three grape"),
            Format::Plain
        );
    }

    #[test]
    fn digits_alone_do_not_trigger() {
        // Day-3 regression guard: "1 apple, 2 banana" without prefix must
        // stay plain (previously triggered implicitly — too unpredictable).
        assert_eq!(detect("1 apple, 2 banana, 3 grape."), Format::Plain);
    }

    #[test]
    fn meeting_time_stays_plain() {
        assert_eq!(
            detect("The meeting is at 3pm tomorrow."),
            Format::Plain
        );
    }

    #[test]
    fn bullet_list_prefix_triggers_bullets() {
        assert_eq!(
            detect("bullet list milk, bread, eggs"),
            Format::Bullets
        );
    }

    #[test]
    fn bullets_prefix_triggers_bullets() {
        assert_eq!(detect("bullets milk and bread"), Format::Bullets);
    }

    #[test]
    fn numbered_list_prefix_triggers_numbered() {
        assert_eq!(
            detect("numbered list: apples, bananas, grapes"),
            Format::Numbered
        );
    }

    #[test]
    fn ordinal_list_prefix_triggers_numbered() {
        assert_eq!(
            detect("ordinal list coffee, tea, water, juice"),
            Format::Numbered
        );
    }

    #[test]
    fn ordinal_colon_prefix_triggers_numbered() {
        assert_eq!(
            detect("ordinal: alpha, beta, gamma"),
            Format::Numbered
        );
    }

    #[test]
    fn bulletproof_does_not_match_bullet() {
        // Word-boundary guard: "bullet" prefix must not match "bulletproof".
        assert_eq!(
            detect("bulletproof vest saves lives"),
            Format::Plain
        );
    }

    #[test]
    fn numbered_word_inside_sentence_does_not_trigger() {
        assert_eq!(
            detect("The numbered tickets are in the drawer."),
            Format::Plain
        );
    }

    // ── apply: bullets ─────────────────────────────────────────

    #[test]
    fn formats_explicit_bullets_with_commas() {
        let out = apply("bullet list milk, bread, eggs", Format::Bullets);
        assert_eq!(out, "- Milk\n- Bread\n- Eggs");
    }

    #[test]
    fn formats_bullets_with_and() {
        let out = apply(
            "bullets milk and bread and eggs",
            Format::Bullets,
        );
        assert_eq!(out, "- Milk\n- Bread\n- Eggs");
    }

    #[test]
    fn formats_bullets_mixed_delimiters() {
        let out = apply(
            "bullet list: milk, bread and eggs.",
            Format::Bullets,
        );
        assert_eq!(out, "- Milk\n- Bread\n- Eggs");
    }

    // ── apply: numbered / ordinal ─────────────────────────────

    #[test]
    fn formats_ordinal_list_with_commas() {
        let out = apply(
            "ordinal list coffee, tea, water, juice",
            Format::Numbered,
        );
        assert_eq!(out, "1. Coffee\n2. Tea\n3. Water\n4. Juice");
    }

    #[test]
    fn formats_numbered_list_with_and() {
        let out = apply(
            "numbered list apples and bananas and grapes",
            Format::Numbered,
        );
        assert_eq!(out, "1. Apples\n2. Bananas\n3. Grapes");
    }

    #[test]
    fn formats_ordinal_with_colon_prefix() {
        let out = apply(
            "ordinal: one, two, three",
            Format::Numbered,
        );
        assert_eq!(out, "1. One\n2. Two\n3. Three");
    }

    // ── apply: plain pass-through ──────────────────────────────

    #[test]
    fn plain_passes_through() {
        let text = "Hello, how are you?";
        assert_eq!(apply(text, Format::Plain), text);
    }
}
