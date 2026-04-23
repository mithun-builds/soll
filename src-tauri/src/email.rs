//! Email-mode detection and formatting.
//!
//! The user triggers email mode by starting their dictation with a phrase
//! like "email to John…" or "draft email to Jane about the Q3 budget…".
//! The recipient name and body are extracted; the body goes through the
//! normal Ollama polish pass; the final text is wrapped in a canonical
//! email template with greeting, body, and sign-off.
//!
//! Scope is deliberately minimal:
//!   - Single recipient (capitalized first name)
//!   - No inline subject line (pastes into a compose window that already
//!     has its own subject field)
//!   - Template is deterministic, no LLM-generated formatting

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailIntent {
    pub recipient: String,
    pub body_raw: String,
}

/// Detect the leading "email to …" / "draft email to …" / "compose email to …"
/// phrase. Returns the recipient name and everything after it (the body that
/// still needs Ollama polishing).
pub fn detect(raw: &str) -> Option<EmailIntent> {
    static RE: Lazy<Regex> = Lazy::new(|| {
        // (?i) case-insensitive; (draft|compose|send)? optional verb;
        // email; (to|for)? optional preposition; (\w+) the recipient word.
        Regex::new(
            r"(?i)^\s*(?:draft|compose|write|send)?\s*email\s+(?:to|for)?\s*([A-Za-z][a-zA-Z\-']{0,40})\s*[,.]?\s+(.*)",
        )
        .unwrap()
    });
    let caps = RE.captures(raw.trim())?;
    let recipient = cap_name(&caps[1]);
    let body_raw = caps[2].trim().to_string();
    if recipient.is_empty() || body_raw.is_empty() {
        return None;
    }
    Some(EmailIntent { recipient, body_raw })
}

/// Wrap a polished body in the canonical email template.
///   Hi {recipient},
///
///   {body}
///
///   {sign_off}[,]
///   [{user_name}]
pub fn format(intent: &EmailIntent, polished_body: &str, sign_off: &str, user_name: &str) -> String {
    let body = polished_body.trim();
    let sign_off = sign_off.trim();
    let user_name = user_name.trim();

    let mut out = String::new();
    out.push_str(&format!("Hi {},\n\n", intent.recipient));
    out.push_str(body);
    out.push_str("\n\n");
    if sign_off.is_empty() {
        out.push_str("Best,");
    } else {
        out.push_str(sign_off);
        out.push(',');
    }
    if !user_name.is_empty() {
        out.push('\n');
        out.push_str(user_name);
    }
    out
}

/// Words that would otherwise get captured as "recipient" when the user
/// ends the dictation at the preposition ("email to John" with no body).
/// Regex backtracking lets `(?:to|for)?` succeed without matching, then
/// the name capture picks up "to" itself. We reject these explicitly.
const RESERVED_NAMES: &[&str] = &[
    "to", "for", "the", "a", "an", "it", "me", "him", "her", "them", "us",
];

fn cap_name(raw: &str) -> String {
    let trimmed = raw.trim().to_lowercase();
    if trimmed.is_empty() || RESERVED_NAMES.contains(&trimmed.as_str()) {
        return String::new();
    }
    let mut chars = trimmed.chars();
    let first = chars.next().unwrap().to_uppercase().next().unwrap_or(' ');
    let rest: String = chars.collect();
    format!("{first}{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detection ──────────────────────────────────────────────

    #[test]
    fn detects_email_to() {
        let got = detect("email to John about the Q3 budget thanks").unwrap();
        assert_eq!(got.recipient, "John");
        assert_eq!(got.body_raw, "about the Q3 budget thanks");
    }

    #[test]
    fn detects_draft_email_to() {
        let got = detect("draft email to Jane can we push the launch by a week").unwrap();
        assert_eq!(got.recipient, "Jane");
        assert!(got.body_raw.contains("push the launch"));
    }

    #[test]
    fn detects_email_without_to() {
        let got = detect("Email Vrishti hey did you get my message").unwrap();
        assert_eq!(got.recipient, "Vrishti");
    }

    #[test]
    fn normalizes_recipient_case() {
        let got = detect("email to JOHN give me a call").unwrap();
        assert_eq!(got.recipient, "John");
    }

    #[test]
    fn plain_prose_is_not_email() {
        assert!(detect("hello world how are you").is_none());
    }

    #[test]
    fn the_word_email_alone_is_not_a_trigger() {
        // "i need to email john tomorrow" isn't a dictation-to-email intent.
        // We require "email" near the start (after an optional verb) + name.
        assert!(detect("i need to email john tomorrow").is_none());
    }

    #[test]
    fn empty_body_is_not_email() {
        assert!(detect("email to John").is_none());
    }

    // ── formatting ─────────────────────────────────────────────

    #[test]
    fn formats_with_name_and_sign_off() {
        let intent = EmailIntent {
            recipient: "John".into(),
            body_raw: "ignored".into(),
        };
        let out = format(&intent, "Can you send the report by EOD?", "Best", "Mithun");
        assert_eq!(
            out,
            "Hi John,\n\nCan you send the report by EOD?\n\nBest,\nMithun"
        );
    }

    #[test]
    fn formats_without_name() {
        let intent = EmailIntent {
            recipient: "Jane".into(),
            body_raw: "ignored".into(),
        };
        let out = format(&intent, "Let's meet Monday.", "Thanks", "");
        assert_eq!(out, "Hi Jane,\n\nLet's meet Monday.\n\nThanks,");
    }

    #[test]
    fn default_sign_off_when_empty() {
        let intent = EmailIntent {
            recipient: "Alice".into(),
            body_raw: "ignored".into(),
        };
        let out = format(&intent, "Hi.", "", "Bob");
        assert_eq!(out, "Hi Alice,\n\nHi.\n\nBest,\nBob");
    }
}
