use anyhow::{anyhow, Result};
use log::{info, warn};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

const OLLAMA_GENERATE: &str = "http://127.0.0.1:11434/api/generate";
const OLLAMA_TAGS: &str = "http://127.0.0.1:11434/api/tags";
const MODEL: &str = "llama3.2:3b";

/// Max time we'll wait on a live polish request. After this we fall back to raw.
const LIVE_TIMEOUT: Duration = Duration::from_secs(4);
/// Time budget for the first warm-up (cold-loading the model can take 30s+).
const WARMUP_TIMEOUT: Duration = Duration::from_secs(90);

const SYSTEM_PROMPT: &str = "You are a dictation polisher. The user spoke the following text aloud; \
rewrite it as polished written text. Rules:
- Fix punctuation and capitalization.
- Remove filler words (um, uh, like, you know) and self-corrections.
- Preserve the speaker's meaning, tone, and specific terms exactly.
- Do NOT add content the user did not say.
- Do NOT wrap the output in quotes or add prefaces.
- Output only the polished text, nothing else.";

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

    /// Pre-load the cleanup model so the first live dictation isn't slow.
    /// Safe to call concurrently with `polish` — we just gate polish on state.
    pub async fn warm_up(&self) {
        *self.state.write() = CleanupState::WarmingUp;

        // 1. Quick reachability check (Ollama running at all?)
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

        // 2. Tiny generate call to force model load into RAM.
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

    /// Polish raw dictation. Returns Err if cleanup is unavailable or the request
    /// fails — callers fall back to raw. Never blocks longer than LIVE_TIMEOUT.
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

        let prompt = format!("{SYSTEM_PROMPT}\n\nRaw dictation:\n{raw}\n\nPolished:");
        let body = GenReq {
            model: MODEL,
            prompt,
            stream: false,
            keep_alive: "30m",
            options: GenOptions {
                temperature: 0.2,
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
            // If Ollama unloaded the model (low memory), re-enter warming.
            if resp.status().as_u16() == 404 || resp.status().as_u16() >= 500 {
                *self.state.write() = CleanupState::Unknown;
            }
            return Err(anyhow!("ollama status: {}", resp.status()));
        }
        let parsed: GenResp = resp.json().await?;
        Ok(parsed.response.trim().to_string())
    }
}
