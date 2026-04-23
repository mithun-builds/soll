//! Smart mid-sentence corrections — rewrite "X actually Y" into just "Y".
//!
//! People self-correct naturally when dictating:
//!
//!   "meet at 5 pm actually 6 pm"   -> "meet at 6 pm"
//!   "3 apples, I mean 4 apples"     -> "4 apples"
//!   "due Tuesday no wait Wednesday" -> "due Wednesday"
//!
//! Ollama sometimes preserves the self-correction verbatim (it's
//! instructed to not paraphrase). This post-processor cleans it up
//! deterministically — no LLM, no hallucination risk.
//!
//! Scope is intentionally narrow in v1:
//!   - Numbers (with optional am/pm/% units)
//!   - Weekdays / short date tokens
//!   - Single-token names (John → Jane)
//! Anything more ambiguous (multi-word phrases, full sentences) is left
//! alone — the safe default is "don't touch the user's text".

use once_cell::sync::Lazy;
use regex::Regex;

/// Correction-marker phrases between the wrong value and the right one.
const MARKERS: &str = r"(?:actually|i\s+mean(?:t)?|no\s+wait|wait\s+no|correction|sorry\s+i\s+mean|scratch\s+that|rather|make\s+that)";

/// Number optionally tagged with %. Does NOT consume trailing whitespace.
/// Time units (am/pm) live in a separate word-pair pattern below.
const NUMBER_BARE: &str = r"\d+(?:[:.]\d+)?%?";

/// Weekday names (full + common abbreviations).
const WEEKDAY: &str =
    r"(?:monday|tuesday|wednesday|thursday|friday|saturday|sunday|mon|tue|tues|wed|thu|thur|thurs|fri|sat|sun)";

/// Main entrypoint. Runs every correction pattern in order.
pub fn apply(text: &str) -> String {
    let mut out = text.to_string();
    // Word-pair first (most specific): "5 pm actually 6 pm", "3 apples I mean 4 apples".
    out = apply_number_word_corrections(&out);
    // Then bare numbers: "5 actually 6", "5:30 actually 6:00", "5% actually 8%".
    out = apply_number_bare_corrections(&out);
    out = apply_weekday_corrections(&out);
    out = apply_single_word_corrections(&out);
    out
}

// ── number + same-word corrections ─────────────────────────────────────────
//
// Matches "N W (marker) M W" where W is the same word token on both sides.
// Replaces with "M W". Covers time units (5 pm / 6 pm), currencies
// (10 dollars / 15 dollars), and quantity nouns (3 apples / 4 apples).
// The word-equality guard is what prevents false positives — unrelated
// sentences don't have the same word surrounding both numbers.

// Rust's `regex` crate doesn't support backreferences, so we capture both
// surrounding words and check equality in the closure. If they don't match,
// we keep the original text — the pattern was a false trigger.
static NUMBER_WORD_RE: Lazy<Regex> = Lazy::new(|| {
    let pat = format!(
        r"(?i)\b(\d+(?:[:.]\d+)?)\s+(\w+)\s*,?\s+{MARKERS}\s*,?\s+(\d+(?:[:.]\d+)?)\s+(\w+)",
        MARKERS = MARKERS
    );
    Regex::new(&pat).unwrap()
});

fn apply_number_word_corrections(text: &str) -> String {
    NUMBER_WORD_RE
        .replace_all(text, |caps: &regex::Captures| {
            let word1 = &caps[2];
            let word2 = &caps[4];
            if word1.eq_ignore_ascii_case(word2) {
                format!("{} {}", &caps[3], word2)
            } else {
                caps.get(0).unwrap().as_str().to_string()
            }
        })
        .to_string()
}

// ── bare number corrections ────────────────────────────────────────────────
//
// Matches "N (marker) M" where N and M are numbers (optionally with colons
// for times or trailing %). Replaces with M.

static NUMBER_BARE_RE: Lazy<Regex> = Lazy::new(|| {
    let pat = format!(
        r"(?i)\b{NUMBER_BARE}\s*,?\s+{MARKERS}\s*,?\s+({NUMBER_BARE})",
        NUMBER_BARE = NUMBER_BARE,
        MARKERS = MARKERS
    );
    Regex::new(&pat).unwrap()
});

fn apply_number_bare_corrections(text: &str) -> String {
    NUMBER_BARE_RE
        .replace_all(text, |caps: &regex::Captures| caps[1].to_string())
        .to_string()
}

// ── weekdays ───────────────────────────────────────────────────────────────

static WEEKDAY_RE: Lazy<Regex> = Lazy::new(|| {
    let pat = format!(
        r"(?i)\b{WEEKDAY}\s*,?\s+{MARKERS}\s*,?\s+({WEEKDAY})",
        WEEKDAY = WEEKDAY,
        MARKERS = MARKERS
    );
    Regex::new(&pat).unwrap()
});

fn apply_weekday_corrections(text: &str) -> String {
    WEEKDAY_RE
        .replace_all(text, |caps: &regex::Captures| caps[1].to_string())
        .to_string()
}

// ── single-word (name-like) corrections ────────────────────────────────────
//
// Restricted to Capitalized-word swaps so we don't eat common prose like
// "I love it actually a lot" (lowercase words are left alone).

static NAME_RE: Lazy<Regex> = Lazy::new(|| {
    let pat = format!(
        r"\b([A-Z][a-z]{{1,20}})\s*,?\s+(?i:{MARKERS})\s*,?\s+([A-Z][a-z]{{1,20}})",
        MARKERS = MARKERS
    );
    Regex::new(&pat).unwrap()
});

fn apply_single_word_corrections(text: &str) -> String {
    NAME_RE
        .replace_all(text, |caps: &regex::Captures| caps[2].to_string())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── numbers ────────────────────────────────────────────────

    #[test]
    fn corrects_time_with_units() {
        assert_eq!(
            apply("meet at 5 pm actually 6 pm"),
            "meet at 6 pm"
        );
    }

    #[test]
    fn corrects_bare_number() {
        assert_eq!(apply("I want 3 apples I mean 4 apples"), "I want 4 apples");
    }

    #[test]
    fn corrects_no_wait_pattern() {
        assert_eq!(
            apply("the price is 50 no wait 60 dollars"),
            "the price is 60 dollars"
        );
    }

    #[test]
    fn corrects_with_punctuation() {
        assert_eq!(
            apply("let's meet at 5, actually 6, tomorrow"),
            "let's meet at 6, tomorrow"
        );
    }

    #[test]
    fn corrects_percent() {
        assert_eq!(apply("tax is 5% actually 8%"), "tax is 8%");
    }

    #[test]
    fn corrects_colon_time() {
        assert_eq!(
            apply("meeting at 5:30 actually 6:00"),
            "meeting at 6:00"
        );
    }

    // ── weekdays ───────────────────────────────────────────────

    #[test]
    fn corrects_weekday() {
        assert_eq!(
            apply("report due Tuesday, I mean Wednesday"),
            "report due Wednesday"
        );
    }

    #[test]
    fn corrects_weekday_abbrev() {
        assert_eq!(
            apply("deadline Mon no wait Tue"),
            "deadline Tue"
        );
    }

    // ── names ──────────────────────────────────────────────────

    #[test]
    fn corrects_single_name() {
        assert_eq!(
            apply("send it to John actually Jane"),
            "send it to Jane"
        );
    }

    #[test]
    fn corrects_name_with_comma() {
        assert_eq!(
            apply("meet at Starbucks, actually Peets"),
            "meet at Peets"
        );
    }

    // ── non-triggers (regression guards) ───────────────────────

    #[test]
    fn leaves_plain_prose_alone() {
        assert_eq!(apply("Hello world how are you"), "Hello world how are you");
    }

    #[test]
    fn does_not_swallow_common_word_actually() {
        // No number/weekday/capital-name pair around "actually" — don't fire.
        assert_eq!(
            apply("that is actually a pretty good idea"),
            "that is actually a pretty good idea"
        );
    }

    #[test]
    fn does_not_fire_on_lowercase_words() {
        // Lowercase words are common prose, not names; leave alone.
        assert_eq!(
            apply("I love it actually a lot"),
            "I love it actually a lot"
        );
    }

    #[test]
    fn multiple_corrections_chain() {
        assert_eq!(
            apply("meet at 5 pm actually 6 pm on Tuesday I mean Wednesday"),
            "meet at 6 pm on Wednesday"
        );
    }

    #[test]
    fn preserves_sentence_end() {
        assert_eq!(
            apply("The price is 10 dollars, actually 15 dollars."),
            "The price is 15 dollars."
        );
    }
}
