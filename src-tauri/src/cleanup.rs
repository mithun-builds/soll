use anyhow::{anyhow, Result};
use log::{info, warn};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

const OLLAMA_GENERATE: &str = "http://127.0.0.1:11434/api/generate";
const OLLAMA_TAGS: &str = "http://127.0.0.1:11434/api/tags";
const MODEL: &str = "llama3.2:3b";

const LIVE_TIMEOUT: Duration = Duration::from_secs(4);
const WARMUP_TIMEOUT: Duration = Duration::from_secs(90);

/// Tight prompt — no examples, because 3B models regurgitate example tokens
/// verbatim (observed: a "five pm -> 5 pm" example poisoned outputs with
/// literal "5 pm" in every response). Abstract rules only.
const SYSTEM_PROMPT: &str = "You clean up voice transcription. Apply ONLY these operations:
- Fix capitalization where needed (start of sentences, proper names).
- Fix punctuation where needed (periods, commas, question marks).
- Remove standalone filler tokens (\"um\", \"uh\", \"er\", \"hmm\") from the input.

Hard rules:
- Keep every content word from the input (nouns, verbs, adjectives, times, numbers).
- Do not replace any word with a synonym.
- Do not paraphrase or reword.
- Do not add words that are not in the input.
- Do not merge, split, or reorder sentences.
- If you are uncertain about any change, output the input UNCHANGED.

Return ONLY the cleaned text. No preface, no quotes, no explanation, no markdown.";

/// Safety-net thresholds. Two independent metrics must both pass:
///   1. Jaccard word-similarity ≥ MIN_JACCARD — catches deletions & mass rewrites.
///   2. New-word ratio ≤ MAX_NEW_RATIO — catches word-swap hallucinations
///      that preserve overall token count (e.g. "tomorrow" → "5 pm").
/// Length bounds catch empty/runaway outputs cheaply before the token math.
const MIN_JACCARD: f32 = 0.65;
const MAX_NEW_RATIO: f32 = 0.15;
const MIN_LEN_RATIO: f32 = 0.60;
const MAX_LEN_RATIO: f32 = 1.60;

#[derive(Serialize)]
struct GenReq<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
    keep_alive: &'a str,
    options: GenOptions,
}

#[derive(Serialize)]
struct GenOptions {
    temperature: f32,
    num_predict: i32,
}

#[derive(Deserialize)]
struct GenResp {
    response: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupState {
    Unknown,
    WarmingUp,
    Ready,
    Unavailable,
}

pub struct OllamaClient {
    live: reqwest::Client,
    warm: reqwest::Client,
    state: Arc<RwLock<CleanupState>>,
}

impl OllamaClient {
    pub fn new() -> Self {
        let live = reqwest::Client::builder()
            .timeout(LIVE_TIMEOUT)
            .build()
            .expect("build live client");
        let warm = reqwest::Client::builder()
            .timeout(WARMUP_TIMEOUT)
            .build()
            .expect("build warm client");
        Self {
            live,
            warm,
            state: Arc::new(RwLock::new(CleanupState::Unknown)),
        }
    }

    pub fn state(&self) -> CleanupState {
        *self.state.read()
    }

    pub async fn warm_up(&self) {
        *self.state.write() = CleanupState::WarmingUp;

        match self.live.get(OLLAMA_TAGS).send().await {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => {
                warn!("Ollama /api/tags status {}, cleanup disabled", r.status());
                *self.state.write() = CleanupState::Unavailable;
                return;
            }
            Err(e) => {
                info!("Ollama not reachable ({e}); cleanup disabled (raw transcripts only)");
                *self.state.write() = CleanupState::Unavailable;
                return;
            }
        }

        let t0 = Instant::now();
        let body = GenReq {
            model: MODEL,
            prompt: "ok".into(),
            stream: false,
            keep_alive: "30m",
            options: GenOptions {
                temperature: 0.0,
                num_predict: 2,
            },
        };
        match self.warm.post(OLLAMA_GENERATE).json(&body).send().await {
            Ok(r) if r.status().is_success() => {
                info!("Ollama model {MODEL} warmed up in {:?}", t0.elapsed());
                *self.state.write() = CleanupState::Ready;
            }
            Ok(r) => {
                warn!(
                    "Ollama warm-up status {} (model {MODEL} likely not pulled); cleanup disabled",
                    r.status()
                );
                *self.state.write() = CleanupState::Unavailable;
            }
            Err(e) => {
                warn!("Ollama warm-up failed ({e}); cleanup disabled");
                *self.state.write() = CleanupState::Unavailable;
            }
        }
    }

    pub async fn polish(&self, raw: &str) -> Result<String> {
        if raw.trim().is_empty() {
            return Ok(String::new());
        }
        match self.state() {
            CleanupState::Unavailable => {
                return Err(anyhow!("cleanup unavailable (skipped)"));
            }
            CleanupState::WarmingUp | CleanupState::Unknown => {
                return Err(anyhow!("cleanup still warming up (skipped)"));
            }
            CleanupState::Ready => {}
        }

        let prompt = format!(
            "{SYSTEM_PROMPT}\n\n--- INPUT ---\n{raw}\n--- CLEANED ---\n"
        );
        let body = GenReq {
            model: MODEL,
            prompt,
            stream: false,
            keep_alive: "30m",
            options: GenOptions {
                temperature: 0.0, // deterministic — no inventing words
                num_predict: 512,
            },
        };
        let resp = self
            .live
            .post(OLLAMA_GENERATE)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("ollama send: {e}"))?;
        if !resp.status().is_success() {
            if resp.status().as_u16() == 404 || resp.status().as_u16() >= 500 {
                *self.state.write() = CleanupState::Unknown;
            }
            return Err(anyhow!("ollama status: {}", resp.status()));
        }
        let parsed: GenResp = resp.json().await?;
        let polished = parsed.response.trim().to_string();

        // Safety net: reject hallucinations and over-aggressive rewrites.
        match is_safe_rewrite(raw, &polished) {
            (true, reason) => {
                info!("cleanup accepted: {reason}");
                Ok(polished)
            }
            (false, reason) => {
                warn!("cleanup REJECTED: {reason} | raw={raw:?} polished={polished:?}");
                Err(anyhow!("rewrite rejected: {reason}"))
            }
        }
    }
}

/// Decide whether a model rewrite is safe to paste. Returns (accepted, reason).
fn is_safe_rewrite(raw: &str, polished: &str) -> (bool, String) {
    let raw_chars = raw.chars().count() as f32;
    let pol_chars = polished.chars().count() as f32;
    if raw_chars == 0.0 {
        return (false, "raw is empty".into());
    }
    let len_ratio = pol_chars / raw_chars;
    if len_ratio < MIN_LEN_RATIO {
        return (
            false,
            format!("polished too short (len_ratio={:.2})", len_ratio),
        );
    }
    if len_ratio > MAX_LEN_RATIO {
        return (
            false,
            format!("polished too long (len_ratio={:.2})", len_ratio),
        );
    }

    let raw_words = tokenize(raw);
    let pol_words = tokenize(polished);
    if raw_words.is_empty() {
        return (true, "no words in raw".into());
    }
    let intersection = raw_words.intersection(&pol_words).count() as f32;
    let union = raw_words.union(&pol_words).count() as f32;
    let jaccard = if union == 0.0 { 1.0 } else { intersection / union };

    if jaccard < MIN_JACCARD {
        return (
            false,
            format!("jaccard={:.2} < {:.2} (len_ratio={:.2})", jaccard, MIN_JACCARD, len_ratio),
        );
    }

    // Count tokens that appear in polished but not in raw. These are words
    // the model invented. A tiny number are fine (punctuation quirks, recase);
    // a large ratio means substantive paraphrasing we should reject.
    let new_words: HashSet<_> = pol_words.difference(&raw_words).collect();
    let new_ratio = if pol_words.is_empty() {
        0.0
    } else {
        new_words.len() as f32 / pol_words.len() as f32
    };
    if new_ratio > MAX_NEW_RATIO {
        let sample: Vec<_> = new_words.iter().take(5).collect();
        return (
            false,
            format!(
                "new_ratio={:.2} > {:.2} (invented: {sample:?}, jaccard={:.2})",
                new_ratio, MAX_NEW_RATIO, jaccard
            ),
        );
    }

    (
        true,
        format!(
            "jaccard={:.2}, new_ratio={:.2}, len_ratio={:.2}",
            jaccard, new_ratio, len_ratio
        ),
    )
}

/// Split text into lowercase word-set on whitespace + common punctuation.
fn tokenize(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '.' | ',' | '?' | '!' | ';' | ':' | '"' | '\'' | '(' | ')' | '[' | ']'
                )
        })
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_minor_edits() {
        let (ok, _) = is_safe_rewrite(
            "please send John a quarterly report by end of day",
            "Send John the quarterly report by the end of the day.",
        );
        assert!(ok);
    }

    #[test]
    fn rejects_content_drop() {
        // The "we should ship it tomorrow" case — Ollama dropped "I was thinking maybe"
        let (ok, reason) = is_safe_rewrite(
            "So, like I was thinking maybe we should ship it tomorrow.",
            "We should ship it tomorrow.",
        );
        assert!(!ok, "should reject; got: {reason}");
    }

    #[test]
    fn rejects_hallucinated_words() {
        // The "coefficient" hallucination
        let (ok, reason) = is_safe_rewrite(
            "meet me at 5 pm actually 6 at the coffee shop",
            "Meet me at 5 p.m. Actually, maybe set the coefficient to six.",
        );
        assert!(!ok, "should reject; got: {reason}");
    }

    #[test]
    fn accepts_identical() {
        let (ok, _) = is_safe_rewrite("Hello world.", "Hello world.");
        assert!(ok);
    }

    #[test]
    fn rejects_empty_input() {
        let (ok, _) = is_safe_rewrite("", "anything");
        assert!(!ok);
    }

    #[test]
    fn rejects_word_swap_hallucination() {
        // Day-1 run #3 redux: "should"→"could", "tomorrow"→"5 pm".
        // Either metric is allowed to fire; just require the overall reject.
        let (ok, reason) = is_safe_rewrite(
            "So, like I was thinking maybe we should ship it tomorrow.",
            "So, like I was thinking maybe we could ship it 5 pm.",
        );
        assert!(!ok, "should reject word swap; got: {reason}");
    }

    #[test]
    fn new_ratio_catches_swap_that_jaccard_misses() {
        // Contrived case where Jaccard is high but new words are present.
        // Raw: 10 tokens, polished: 10 tokens, 8 overlap, 2 new. Jaccard ≈ 0.67.
        // new_ratio = 2/10 = 0.20 > 0.15 → rejected by the new-word gate.
        let raw = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let polished =
            "alpha beta gamma delta epsilon zeta eta theta SWAPPED REPLACED";
        let (ok, reason) = is_safe_rewrite(raw, polished);
        assert!(!ok, "new_ratio should fire; got: {reason}");
        assert!(reason.contains("new_ratio"), "expected new_ratio; got: {reason}");
    }

    #[test]
    fn rejects_prompt_regurgitation() {
        // Day-1 run #2 redux: model spat back just "5 pm" ignoring the input.
        let (ok, reason) = is_safe_rewrite(
            "What's on my calendar for tomorrow morning?",
            "5 pm",
        );
        assert!(!ok, "should reject prompt regurgitation; got: {reason}");
    }

    #[test]
    fn accepts_filler_removal() {
        // The legitimate case Ollama is supposed to handle.
        let (ok, reason) = is_safe_rewrite(
            "um so I was thinking we should ship it tomorrow",
            "So I was thinking we should ship it tomorrow.",
        );
        assert!(ok, "should accept filler removal; got: {reason}");
    }
}
