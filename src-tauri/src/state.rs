use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use log::{info, warn};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex as AsyncMutex;

use crate::audio::AudioRecorder;
use crate::cleanup::OllamaClient;
use crate::corrections;
use crate::dictionary::Dictionary;
use crate::formatter::{self, Format};
use crate::model::{ensure_model, WhisperModel, CANCELLED_MSG};
use crate::paste::paste_text;
use crate::settings::{
    Settings, DEFAULT_AI_CLEANUP, KEY_AI_CLEANUP, KEY_OLLAMA_MODEL, KEY_USER_NAME,
    KEY_WHISPER_MODEL,
};
use crate::skills::{self, Skill};
use crate::transcribe::Transcriber;
use crate::tray::{self, TrayState};

static DICTATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Strip common LLM preamble sentences that appear before the actual content,
/// e.g. "Here's the polished email:", "Sure! Here you go:", "Certainly!".
/// Looks for the first blank line or a sentence-ending preamble marker and
/// returns everything after it. Falls back to the original string.
fn strip_llm_preamble(s: &str) -> String {
    let s = s.trim();

    // Patterns that strongly indicate a preamble line
    let preamble_triggers: &[&str] = &[
        "here's", "here is", "sure", "certainly", "of course",
        "below is", "the following", "polished", "revised", "rewritten",
    ];

    // Check if the first non-empty line looks like a preamble
    let mut lines = s.lines().peekable();
    if let Some(first) = lines.peek() {
        let lower = first.to_lowercase();
        let looks_like_preamble = preamble_triggers
            .iter()
            .any(|p| lower.starts_with(p) || lower.contains(p))
            && (first.trim_end().ends_with(':') || first.trim_end().ends_with('!') || first.trim_end().ends_with('.'));

        if looks_like_preamble {
            // Drop the first line and any immediately following blank lines
            lines.next();
            let rest: String = lines
                .skip_while(|l| l.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if !rest.trim().is_empty() {
                return rest.trim().to_string();
            }
        }
    }
    s.to_string()
}

/// Lowercase any "shouty" ALL-CAPS word (3+ consecutive uppercase ASCII
/// letters). Small local LLMs sometimes preserve or invent emphasis by
/// shouting a word like "TOMORROW" or "MONDAY"; users rarely want that in a
/// polished email. Known technical acronyms are preserved. A word that is
/// at the very start of a sentence is lower-cased then re-capitalised so
/// "TOMORROW is…" becomes "Tomorrow is…" rather than "tomorrow is…".
fn normalize_shouty_caps(s: &str) -> String {
    // Preserve well-known acronyms users actually say (add as needed).
    const KEEP: &[&str] = &[
        "API", "URL", "HTTP", "HTTPS", "PDF", "JSON", "HTML", "CSS", "SQL",
        "USA", "UK", "EU", "NYC", "LA", "SF",
        "CEO", "CTO", "CFO", "COO", "VP", "HR", "PR",
        "ASAP", "FYI", "TBD", "TBH", "IMO", "IMHO", "IIRC", "AKA", "ETA",
        "AI", "ML", "UI", "UX", "OS", "IT",
    ];

    let re = regex::Regex::new(r"\b[A-Z]{3,}\b").unwrap();
    let mut out = String::with_capacity(s.len());
    let mut last = 0usize;

    for m in re.find_iter(s) {
        out.push_str(&s[last..m.start()]);
        let word = m.as_str();

        if KEEP.contains(&word) {
            out.push_str(word);
        } else {
            // Sentence-start: previous non-whitespace char is '.', '!' or '?',
            // or the word is at the very beginning.
            let before = s[..m.start()].trim_end();
            let at_sentence_start = before.is_empty()
                || before.ends_with('.')
                || before.ends_with('!')
                || before.ends_with('?')
                || before.ends_with('\n');

            let lower = word.to_lowercase();
            if at_sentence_start {
                let mut chars = lower.chars();
                if let Some(first) = chars.next() {
                    out.extend(first.to_uppercase());
                    out.push_str(chars.as_str());
                }
            } else {
                out.push_str(&lower);
            }
        }

        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

pub struct AppState {
    pub app: AppHandle,
    pub dictionary: Arc<Dictionary>,
    pub settings: Arc<Settings>,
    pub skills: Mutex<Vec<Skill>>,
    current_model: Mutex<WhisperModel>,
    pub downloading: Mutex<Option<WhisperModel>>,
    download_epoch: std::sync::atomic::AtomicU64,
    recorder: Mutex<Option<AudioRecorder>>,
    transcriber: AsyncMutex<Option<Arc<Transcriber>>>,
    swap_guard: AsyncMutex<()>,
    pub ollama: OllamaClient,
    is_recording: Mutex<bool>,
}

impl AppState {
    pub fn new(app: AppHandle) -> Self {
        let data_dir = match app.path().app_data_dir() {
            Ok(d) => d,
            Err(e) => panic!("no app_data_dir: {e:?}"),
        };
        let dict = match Dictionary::open(&data_dir.join("dict.db")) {
            Ok(d) => Arc::new(d),
            Err(e) => panic!("cannot open dictionary db: {e:?}"),
        };
        let settings = match Settings::open(&data_dir.join("settings.db")) {
            Ok(s) => Arc::new(s),
            Err(e) => panic!("cannot open settings db: {e:?}"),
        };
        let preferred = settings.get_or_default(KEY_WHISPER_MODEL, WhisperModel::DEFAULT.id());
        let model = WhisperModel::from_id(&preferred).unwrap_or(WhisperModel::DEFAULT);

        let user_skills_dir = data_dir.join("skills");
        let skills_list: Vec<Skill> = skills::load_all(Some(&user_skills_dir));
        log::info!(
            "loaded {} skills: {}",
            skills_list.len(),
            skills_list
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let ollama = OllamaClient::new();
        // Restore persisted Ollama model choice, falling back to the default.
        let saved_ollama = settings.get_or_default(KEY_OLLAMA_MODEL, crate::cleanup::OllamaModel::DEFAULT.tag());
        if let Some(m) = crate::cleanup::OllamaModel::from_tag(&saved_ollama) {
            ollama.set_model(m.tag());
        }

        Self {
            app,
            dictionary: dict,
            settings,
            skills: Mutex::new(skills_list),
            current_model: Mutex::new(model),
            downloading: Mutex::new(None),
            download_epoch: std::sync::atomic::AtomicU64::new(0),
            recorder: Mutex::new(None),
            transcriber: AsyncMutex::new(None),
            swap_guard: AsyncMutex::new(()),
            ollama,
            is_recording: Mutex::new(false),
        }
    }

    pub fn current_model(&self) -> WhisperModel {
        *self.current_model.lock()
    }

    pub fn user_skills_dir(&self) -> Result<std::path::PathBuf> {
        let d = self
            .app
            .path()
            .app_data_dir()
            .map_err(|e| anyhow!("app_data_dir: {e:?}"))?
            .join("skills");
        Ok(d)
    }

    pub fn reload_skills(&self) {
        let dir = self.user_skills_dir().ok();
        let new: Vec<Skill> = skills::load_all(dir.as_deref());
        log::info!(
            "reloaded {} skills: {}",
            new.len(),
            new.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
        );
        *self.skills.lock() = new;
    }

    pub fn is_model_cached(&self, model: WhisperModel) -> bool {
        let dir = match self.app.path().app_data_dir() {
            Ok(d) => d.join("models"),
            Err(_) => return false,
        };
        let path = dir.join(model.filename());
        let min_size = model.expected_size_bytes() * 9 / 10;
        std::fs::metadata(&path)
            .map(|m| m.len() >= min_size)
            .unwrap_or(false)
    }

    fn set_tray(&self, state: TrayState) {
        tray::set_state(&self.app, state);
    }

    pub async fn warm_up(self: &Arc<Self>) -> Result<()> {
        let model = self.current_model();
        info!("warming up: whisper={} + cleanup", model.id());

        let me = self.clone();
        tokio::spawn(async move {
            me.ollama.warm_up().await;
        });

        if self.is_model_cached(model) {
            self.clone().load_and_activate(model).await
        } else {
            self.clone().boot_download_and_activate(model).await
        }
    }

    pub async fn switch_model(self: Arc<Self>, model: WhisperModel) -> Result<()> {
        if model == self.current_model() && self.transcriber.lock().await.is_some() {
            info!("switch_model: {} already active", model.id());
            return Ok(());
        }
        if !self.is_model_cached(model) {
            return Err(anyhow!(
                "switch_model({}) called on uncached model — call start_download first",
                model.id()
            ));
        }
        info!("switch_model -> {} (cached swap)", model.id());
        self.load_and_activate(model).await
    }

    pub async fn start_download(self: Arc<Self>, model: WhisperModel) -> Result<()> {
        use std::sync::atomic::Ordering;

        {
            let mut dl = self.downloading.lock();
            if *dl == Some(model) {
                info!("start_download({}): already downloading", model.id());
                return Ok(());
            }
            *dl = Some(model);
        }
        let my_epoch = self.download_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        info!("start_download -> {} (epoch={my_epoch})", model.id());

        let me_progress = self.clone();
        let me_wanted = self.clone();
        let result = ensure_model(
            &self.app,
            model,
            move |done, total| {
                tray::set_download_progress(model, done, total);
                let _ = &me_progress;
            },
            move || me_wanted.download_epoch.load(Ordering::SeqCst) == my_epoch,
        )
        .await;

        {
            let mut dl = self.downloading.lock();
            if *dl == Some(model) {
                *dl = None;
            }
        }
        match result {
            Ok(_) => {
                info!("start_download({}) complete; model now cached", model.id());
                Ok(())
            }
            Err(e) if e.to_string().contains(CANCELLED_MSG) => {
                info!("start_download({}) cancelled", model.id());
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn load_and_activate(self: Arc<Self>, model: WhisperModel) -> Result<()> {
        let _swap = self.swap_guard.lock().await;
        let t0 = std::time::Instant::now();
        self.set_tray(TrayState::Loading);
        tray::set_status_text(&format!("Loading {}…", model.id()));

        let me_unused = self.clone();
        let path = ensure_model(
            &self.app,
            model,
            |_, _| {},
            move || {
                let _ = &me_unused;
                true
            },
        )
        .await?;

        let t_load_start = std::time::Instant::now();
        let model_id = model.id();
        let transcriber = tokio::task::spawn_blocking(move || -> Result<Transcriber> {
            let t = Transcriber::load(&path)?;
            info!("{} loaded in {:?}; warming Metal…", model_id, t_load_start.elapsed());
            let t_warm = std::time::Instant::now();
            t.warm()?;
            info!("{} Metal compiled in {:?}", model_id, t_warm.elapsed());
            Ok(t)
        })
        .await
        .map_err(|e| anyhow!("join error: {e}"))??;

        *self.transcriber.lock().await = Some(Arc::new(transcriber));
        *self.current_model.lock() = model;
        tray::update_model_check(model);
        let _ = self.settings.set(KEY_WHISPER_MODEL, model.id());
        info!("{} fully ready in {:?}", model.id(), t0.elapsed());
        self.set_tray(TrayState::Idle);
        Ok(())
    }

    async fn boot_download_and_activate(self: Arc<Self>, model: WhisperModel) -> Result<()> {
        use std::sync::atomic::Ordering;

        let _swap = self.swap_guard.lock().await;
        let t0 = std::time::Instant::now();
        self.set_tray(TrayState::Loading);
        tray::set_status_text(&format!("Downloading {}…", model.id()));

        let my_epoch = self.download_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        *self.downloading.lock() = Some(model);

        let path = ensure_model(
            &self.app,
            model,
            move |done, total| tray::set_download_progress(model, done, total),
            move || true,
        )
        .await;

        {
            let mut dl = self.downloading.lock();
            if *dl == Some(model) {
                *dl = None;
            }
        }
        let _ = my_epoch;
        let path = path?;

        tray::set_status_text(&format!("Loading {}…", model.id()));
        let t_load_start = std::time::Instant::now();
        let model_id = model.id();
        let transcriber = tokio::task::spawn_blocking(move || -> Result<Transcriber> {
            let t = Transcriber::load(&path)?;
            info!("{} loaded in {:?}; warming Metal…", model_id, t_load_start.elapsed());
            t.warm()?;
            Ok(t)
        })
        .await
        .map_err(|e| anyhow!("join error: {e}"))??;

        *self.transcriber.lock().await = Some(Arc::new(transcriber));
        *self.current_model.lock() = model;
        tray::update_model_check(model);
        let _ = self.settings.set(KEY_WHISPER_MODEL, model.id());
        info!("{} fully ready in {:?}", model.id(), t0.elapsed());
        self.set_tray(TrayState::Idle);
        Ok(())
    }

    pub async fn on_press(self: Arc<Self>) -> Result<()> {
        {
            let mut rec = self.is_recording.lock();
            if *rec {
                return Ok(());
            }
            *rec = true;
        }
        self.set_tray(TrayState::Initializing);
        let recorder = AudioRecorder::start()?;
        *self.recorder.lock() = Some(recorder);
        self.set_tray(TrayState::Transcribing);
        crate::overlay::recording(&self.app);
        Ok(())
    }

    pub async fn on_release(self: Arc<Self>) -> Result<()> {
        let t_release = Instant::now();
        {
            let mut rec = self.is_recording.lock();
            if !*rec {
                return Ok(());
            }
            *rec = false;
        }
        let n = DICTATION_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;

        let recorder = self.recorder.lock().take();
        let samples = match recorder {
            Some(r) => r.stop()?,
            None => return Ok(()),
        };
        let audio_ms = (samples.len() as f64 / 16_000.0 * 1000.0) as u64;

        if samples.len() < 16_000 / 4 {
            info!("[latency #{n}] audio={audio_ms}ms — skipped (tap too short)");
            self.set_tray(TrayState::Idle);
            crate::overlay::hide(&self.app);
            return Ok(());
        }
        self.set_tray(TrayState::Processing);
        crate::overlay::processing(&self.app);

        let transcriber = {
            let guard = self.transcriber.lock().await;
            guard.clone()
        };
        let transcriber = transcriber.ok_or_else(|| {
            self.set_tray(TrayState::Idle);
            crate::overlay::hide(&self.app);
            anyhow!("transcriber not ready; still loading model")
        })?;

        let whisper_prompt = self.dictionary.whisper_prompt().unwrap_or(None);
        let dict_words = self.dictionary.list().unwrap_or_default();

        let t_whisper_start = Instant::now();
        let prompt_clone = whisper_prompt.clone();
        let raw = tokio::task::spawn_blocking(move || {
            transcriber.transcribe_with_prompt(&samples, prompt_clone.as_deref())
        })
        .await
        .map_err(|e| anyhow!("transcribe join: {e}"))??;
        let whisper_ms = t_whisper_start.elapsed().as_millis() as u64;

        let preserve_terms: Vec<String> = dict_words.into_iter().map(|e| e.word).collect();
        let user_name = self.settings.get_or_default(KEY_USER_NAME, "");
        let ai_on = self
            .settings
            .get(KEY_AI_CLEANUP)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(DEFAULT_AI_CLEANUP);

        // ── Skill routing ────────────────────────────────────────────────────
        //
        // Triggers are the only activation path: the first trigger phrase
        // that matches the utterance wins, instantly (0 ms, no LLM call).
        // Disabled skills never match even though they stay loaded so their
        // markdown + edits survive the toggle.
        let disabled = self.settings.disabled_skills();

        let skill_match = {
            let list = self.skills.lock();
            let enabled: Vec<Skill> = list
                .iter()
                .filter(|s| !disabled.contains(&s.name))
                .cloned()
                .collect();
            // Try explicit "use [skill-name] [body]" first — more reliable
            // than trigger matching because Whisper's punctuation/capitalisation
            // variations can't break it. Fall back to trigger phrases.
            skills::direct_invoke(&enabled, &raw)
                .map(|(s, v)| (s.clone(), v))
                .or_else(|| skills::match_skill(&enabled, &raw).map(|(s, v)| (s.clone(), v)))
        };

        // ── Skill execution ──────────────────────────────────────────────────

        if let Some((skill, mut vars)) = skill_match {
            let via_direct = raw.trim().to_lowercase().contains("use ");
            info!("[latency #{n}] running skill: {} (via {})", skill.name,
                if via_direct { "direct invoke" } else { "trigger" });

            let t_run = Instant::now();

            // Inject universal vars: [body] = captured `<body>` (if any) or
            // the raw utterance; [name] = user's name from Settings.
            if !vars.contains_key("body") {
                vars.insert("body".into(), raw.clone());
            }
            vars.insert("name".into(), user_name.clone());

            let (final_text, run_label, run_ms) = match &skill.kind {
                skills::SkillKind::Ai { instructions } => {
                    let prompt = skills::interpolate(instructions, &vars);
                    let out = if ai_on {
                        match self.ollama.skill_generate(&prompt).await {
                            Ok(s) => normalize_shouty_caps(&strip_llm_preamble(&s)),
                            Err(e) => {
                                warn!(
                                    "[latency #{n}] skill {}: ollama error ({e:?}); using body",
                                    skill.name
                                );
                                vars.get("body").cloned().unwrap_or_default()
                            }
                        }
                    } else {
                        vars.get("body").cloned().unwrap_or_default()
                    };
                    (out, "ollama", t_run.elapsed().as_millis() as u64)
                }
                skills::SkillKind::Phrase { text } => {
                    // Pure paste — no LLM call, no ollama latency.
                    let out = skills::interpolate(text, &vars);
                    (out, "phrase", t_run.elapsed().as_millis() as u64)
                }
            };

            let with_dict = crate::dictionary::apply_to_text(&final_text, &preserve_terms);
            let trimmed = with_dict.trim().to_string();

            if trimmed.is_empty() {
                info!("[latency #{n}] skill {} produced empty output; skipping paste", skill.name);
                self.set_tray(TrayState::Idle);
                crate::overlay::hide(&self.app);
                return Ok(());
            }

            let t_paste = Instant::now();
            paste_text(&trimmed)?;
            let paste_ms = t_paste.elapsed().as_millis() as u64;
            let total_ms = t_release.elapsed().as_millis() as u64;
            info!(
                "[latency #{n}] skill={} ({run_label}) audio={audio_ms}ms whisper={whisper_ms}ms \
                 run={run_ms}ms paste={paste_ms}ms total={total_ms}ms text={trimmed:?}",
                skill.name
            );
            tray::set_skill_done(&self.app, &skill.name);
            crate::overlay::skill_done(&self.app, &skill.name);
            return Ok(());
        }

        // ── Default cleanup path (no skill matched) ──────────────────────────
        //
        // Handles plain prose (Ollama polish + corrections) and list formats
        // (bullets / numbered, detected deterministically).

        let format = formatter::detect(&raw);

        let body_for_cleanup = if matches!(format, Format::Plain) {
            corrections::apply(&raw)
        } else {
            raw.clone()
        };

        let t_ollama_start = Instant::now();
        let (polished, ollama_ms, ollama_used) = match format {
            Format::Plain if ai_on => {
                match self
                    .ollama
                    .polish_with_terms(&body_for_cleanup, &preserve_terms)
                    .await
                {
                    Ok(p) => {
                        let ms = t_ollama_start.elapsed().as_millis() as u64;
                        (p, ms, true)
                    }
                    Err(e) => {
                        let ms = t_ollama_start.elapsed().as_millis() as u64;
                        warn!("[latency #{n}] cleanup skipped ({e:?}), using corrected transcript");
                        (body_for_cleanup.clone(), ms, false)
                    }
                }
            }
            Format::Plain => {
                info!("[latency #{n}] AI cleanup disabled — using corrected transcript");
                (body_for_cleanup.clone(), 0, false)
            }
            Format::Bullets | Format::Numbered => {
                info!("[latency #{n}] format={:?} (skipping ollama)", format);
                (formatter::apply(&raw, format), 0, false)
            }
        };

        let with_dict = crate::dictionary::apply_to_text(&polished, &preserve_terms);
        let trimmed = with_dict.trim().to_string();

        if trimmed.is_empty() {
            info!("[latency #{n}] audio={audio_ms}ms whisper={whisper_ms}ms — empty transcript");
            self.set_tray(TrayState::Idle);
            crate::overlay::hide(&self.app);
            return Ok(());
        }

        let t_paste_start = Instant::now();
        paste_text(&trimmed)?;
        let paste_ms = t_paste_start.elapsed().as_millis() as u64;
        let total_ms = t_release.elapsed().as_millis() as u64;
        let char_count = trimmed.chars().count();
        let ollama_tag = if ollama_used { "ollama" } else { "ollama-skipped" };

        info!(
            "[latency #{n}] audio={audio_ms}ms whisper={whisper_ms}ms \
             {ollama_tag}={ollama_ms}ms paste={paste_ms}ms total={total_ms}ms \
             chars={char_count} text={trimmed:?}"
        );

        self.set_tray(TrayState::Transcribed);
        crate::overlay::transcribed(&self.app);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_shouty_word_in_middle() {
        assert_eq!(
            normalize_shouty_caps("About our meeting TOMORROW."),
            "About our meeting tomorrow."
        );
    }

    #[test]
    fn normalizes_multiple_shouty_words() {
        assert_eq!(
            normalize_shouty_caps("MONDAY or TUESDAY, not FRIDAY"),
            "Monday or tuesday, not friday"
        );
    }

    #[test]
    fn preserves_technical_acronyms() {
        assert_eq!(
            normalize_shouty_caps("Please send the API docs ASAP"),
            "Please send the API docs ASAP"
        );
    }

    #[test]
    fn capitalizes_shouty_word_at_sentence_start() {
        assert_eq!(
            normalize_shouty_caps("TOMORROW is Friday."),
            "Tomorrow is Friday."
        );
    }

    #[test]
    fn capitalizes_after_sentence_end() {
        assert_eq!(
            normalize_shouty_caps("That's fine. MONDAY works."),
            "That's fine. Monday works."
        );
    }

    #[test]
    fn leaves_two_letter_caps_alone() {
        // Words shorter than 3 letters aren't matched (things like "US", "AI"
        // are common and often intentional).
        assert_eq!(
            normalize_shouty_caps("I went to NY and DC"),
            "I went to NY and DC"
        );
    }

    #[test]
    fn leaves_sentence_case_alone() {
        assert_eq!(
            normalize_shouty_caps("Hi Nikita, About our meeting tomorrow."),
            "Hi Nikita, About our meeting tomorrow."
        );
    }

    #[test]
    fn leaves_empty_string_alone() {
        assert_eq!(normalize_shouty_caps(""), "");
    }

    #[test]
    fn handles_shouty_at_start_of_string() {
        assert_eq!(
            normalize_shouty_caps("MEETING at 3pm"),
            "Meeting at 3pm"
        );
    }

    #[test]
    fn preserves_mixed_case_in_surrounding_text() {
        // Only fully-caps words are touched.
        assert_eq!(
            normalize_shouty_caps("Let's meet on Monday, not TUESDAY."),
            "Let's meet on Monday, not tuesday."
        );
    }
}
