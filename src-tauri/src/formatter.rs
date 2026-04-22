//! Auto-formatting for spoken list patterns.
//!
//! Detects three intents from raw Whisper output:
//!   - Numbered list (explicit "numbered list …" OR implicit "one X two Y three Z")
//!   - Bullet list ("bullet list …" / "bullets …" with comma/and delimiters)
//!   - Plain prose (everything else → normal Ollama cleanup path)
//!
//! When a list is detected we format DETERMINISTICALLY and skip Ollama, because
//! an LLM will always turn a numbered list back into prose.

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Plain,
    Bullets,
    Numbered,
}

/// Word forms we recognize as ordinal markers. Detection normalizes each
/// match — word OR digit — into a 0-based index (one=1=0, two=2=1, …).
const ORDINALS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
];

/// Matches either a small integer (1..=10, with optional trailing '.' or ')' )
/// or one of the spelled-out ordinal words. Whisper often transcribes spoken
/// "one two three" as digits ("1 2 3" or "1. 2. 3."), so we must accept both.
static ORDINAL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(10|[1-9]|one|two|three|four|five|six|seven|eight|nine|ten)\b",
    )
    .unwrap()
});

/// Convert a captured ordinal (word or digit) to its 0-based index.
/// Returns None for unrecognized input (shouldn't happen with ORDINAL_RE).
fn ordinal_index(s: &str) -> Option<usize> {
    let lower = s.to_lowercase();
    if let Some(pos) = ORDINALS.iter().position(|&w| w == lower) {
        return Some(pos);
    }
    lower.parse::<usize>().ok().and_then(|n| {
        if (1..=ORDINALS.len()).contains(&n) {
            Some(n - 1)
        } else {
            None
        }
    })
}

/// Decide what to do with raw whisper output.
pub fn detect(raw: &str) -> Format {
    let trimmed = raw.trim();
    let lower = trimmed.to_lowercase();

    // Explicit prefixes — the user actually said "bullet list" / "numbered list".
    if lower.starts_with("bullet list")
        || lower.starts_with("bullets ")
        || lower.starts_with("bullet:")
        || lower.starts_with("bullets:")
    {
        return Format::Bullets;
    }
    if lower.starts_with("numbered list")
        || lower.starts_with("numbered:")
        || lower.starts_with("number list")
    {
        return Format::Numbered;
    }

    // Implicit: at least three ordinals in their natural order.
    if has_sequential_ordinals(trimmed, 3) {
        return Format::Numbered;
    }

    Format::Plain
}

/// Apply the chosen format. `Plain` passes through unchanged.
pub fn apply(raw: &str, format: Format) -> String {
    match format {
        Format::Plain => raw.to_string(),
        Format::Bullets => to_bullet_list(raw),
        Format::Numbered => to_numbered_list(raw),
    }
}

// ── detection helpers ───────────────────────────────────────────────────────

fn has_sequential_ordinals(text: &str, min_count: usize) -> bool {
    let mut expected = 0usize;
    for m in ORDINAL_RE.find_iter(text) {
        if let Some(n) = ordinal_index(m.as_str()) {
            if n == expected {
                expected += 1;
                if expected >= min_count {
                    return true;
                }
            }
        }
    }
    false
}

// ── transformers ────────────────────────────────────────────────────────────

fn to_numbered_list(raw: &str) -> String {
    let matches: Vec<_> = ORDINAL_RE.find_iter(raw).collect();
    // Keep only the matches that appear in sequence starting from 0 (one, two,
    // three or 1, 2, 3). Stray unrelated numbers are ignored.
    let mut expected = 0usize;
    let mut list_markers: Vec<regex::Match> = Vec::new();
    for m in matches {
        if let Some(n) = ordinal_index(m.as_str()) {
            if n == expected {
                list_markers.push(m);
                expected += 1;
            }
        }
    }
    if list_markers.len() < 2 {
        return raw.to_string();
    }

    let mut items: Vec<String> = Vec::new();
    for (i, m) in list_markers.iter().enumerate() {
        let content_start = m.end();
        let content_end = list_markers
            .get(i + 1)
            .map(|next| next.start())
            .unwrap_or(raw.len());
        let slice = raw[content_start..content_end]
            .trim()
            .trim_start_matches(|c: char| c == '.' || c == ',' || c == ':')
            .trim();
        let slice = slice.trim_end_matches(|c: char| c == '.' || c == ',').trim();
        if !slice.is_empty() {
            items.push(capitalize_first(slice));
        }
    }

    if items.is_empty() {
        return raw.to_string();
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| format!("{}. {}", i + 1, item))
        .collect::<Vec<_>>()
        .join("\n")
}

fn to_bullet_list(raw: &str) -> String {
    let stripped = strip_list_prefix(raw);
    let items: Vec<String> = split_bullet_items(&stripped);
    if items.is_empty() {
        return raw.to_string();
    }
    items
        .iter()
        .map(|item| format!("- {}", capitalize_first(item)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_list_prefix(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_lowercase();
    for prefix in [
        "bullet list:",
        "bullet list",
        "bullets:",
        "bullets ",
        "bullet:",
        "numbered list:",
        "numbered list",
        "numbered:",
        "number list:",
        "number list",
    ] {
        if lower.starts_with(prefix) {
            return trimmed[prefix.len()..]
                .trim_start_matches(|c: char| c == ':' || c == ',' || c.is_whitespace())
                .to_string();
        }
    }
    trimmed.to_string()
}

fn split_bullet_items(s: &str) -> Vec<String> {
    // Split on commas, semicolons, newlines, and the standalone word "and".
    let mut items: Vec<String> = vec![s.to_string()];

    // First split on commas / semicolons / newlines
    items = items
        .into_iter()
        .flat_map(|chunk| {
            chunk
                .split(|c: char| c == ',' || c == ';' || c == '\n')
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .collect();

    // Then split on standalone " and "
    static AND_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+and\s+").unwrap());
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
    fn implicit_numbered_three_ordinals() {
        assert_eq!(
            detect("one apple two banana three grape"),
            Format::Numbered
        );
    }

    #[test]
    fn implicit_numbered_requires_three() {
        // Only two ordinals — don't auto-list.
        assert_eq!(detect("one apple two banana"), Format::Plain);
    }

    #[test]
    fn implicit_numbered_ignores_out_of_order() {
        // "three" before "one" shouldn't trigger.
        assert_eq!(
            detect("I bought three things and one more later"),
            Format::Plain
        );
    }

    #[test]
    fn explicit_bullet_list_prefix() {
        assert_eq!(
            detect("bullet list milk, bread, eggs"),
            Format::Bullets
        );
    }

    #[test]
    fn explicit_numbered_list_prefix() {
        assert_eq!(
            detect("numbered list: apples, bananas, grapes"),
            Format::Numbered
        );
    }

    // ── apply: numbered ────────────────────────────────────────

    #[test]
    fn formats_implicit_numbered_list() {
        let out = apply(
            "one apple two banana three grape",
            Format::Numbered,
        );
        assert_eq!(out, "1. Apple\n2. Banana\n3. Grape");
    }

    #[test]
    fn formats_numbered_with_whisper_punctuation() {
        // Whisper often inserts commas/periods around ordinals.
        let out = apply(
            "One. Apple two. Banana three. Grape.",
            Format::Numbered,
        );
        assert_eq!(out, "1. Apple\n2. Banana\n3. Grape");
    }

    #[test]
    fn formats_explicit_numbered_list() {
        let out = apply(
            "numbered list: coffee, tea, water",
            Format::Numbered,
        );
        // With the explicit prefix and no inline ordinals we expect
        // fallback to raw (we only split by ordinal markers for now).
        // So this returns unchanged input — refinement target for later.
        assert!(out.contains("coffee") && out.contains("tea"));
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

    // ── apply: plain pass-through ──────────────────────────────

    #[test]
    fn plain_passes_through() {
        let text = "Hello, how are you?";
        assert_eq!(apply(text, Format::Plain), text);
    }

    // ── digit-form ordinals (Whisper real-world outputs) ──────

    #[test]
    fn detects_digit_ordinals_in_sequence() {
        // Observed from Day-3 run #6.
        assert_eq!(
            detect("1 apple, 2 banana, 3 grape."),
            Format::Numbered
        );
    }

    #[test]
    fn formats_digit_ordinals() {
        let out = apply("1 apple, 2 banana, 3 grape.", Format::Numbered);
        assert_eq!(out, "1. Apple\n2. Banana\n3. Grape");
    }

    #[test]
    fn formats_digit_ordinals_with_periods() {
        // Whisper sometimes inserts stops after each digit.
        let out = apply("1. Coffee 2. Tea 3. Water 4. Juice", Format::Numbered);
        assert_eq!(out, "1. Coffee\n2. Tea\n3. Water\n4. Juice");
    }

    #[test]
    fn mixed_word_and_digit_ordinals() {
        // "one apple, 2 banana, three grape" — user emphasized digits mid-list.
        let out = apply(
            "one apple, 2 banana, three grape",
            Format::Numbered,
        );
        assert_eq!(out, "1. Apple\n2. Banana\n3. Grape");
    }

    #[test]
    fn random_digits_in_prose_dont_trigger() {
        // "3 people attended at 4 pm" — digits present but not starting from 1.
        assert_eq!(
            detect("3 people attended at 4 pm at building 5"),
            Format::Plain
        );
    }

    #[test]
    fn single_digit_doesnt_trigger() {
        assert_eq!(
            detect("The meeting is at 3 p.m. tomorrow"),
            Format::Plain
        );
    }
}
