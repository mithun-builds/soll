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
    Settings, DEFAULT_AI_CLEANUP, KEY_AI_CLEANUP, KEY_USER_NAME, KEY_WHISPER_MODEL,
};
use crate::skills::{self, Skill};
use crate::transcribe::Transcriber;
use crate::tray::{self, TrayState};

static DICTATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// LLM-based skill router. Sends a short classification prompt to Ollama
/// listing all intent-based skills, gets back a JSON response with the
/// skill name and extracted variables.
///
/// Returns None on any failure (Ollama down, malformed JSON, unknown skill
/// name) — the caller falls through to the default cleanup path.
async fn classify_skill_with_llm(
    raw: &str,
    skills: &[Skill],
    ollama: &OllamaClient,
) -> Option<(String, std::collections::HashMap<String, String>)> {
    // Only consider skills that have a plain-English intent description.
    let intent_skills: Vec<&Skill> = skills.iter().filter(|s| s.intent.is_some()).collect();
    if intent_skills.is_empty() {
        return None;
    }

    // Build a compact list: "- name: description (extract hint if any)"
    let skill_list = intent_skills
        .iter()
        .map(|s| {
            let intent = s.intent.as_deref().unwrap_or("");
            format!("- {}: {}", s.name, intent)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are a skill router for a voice dictation app.\n\
         Match the transcription to exactly one skill, or null.\n\
         \n\
         Skills:\n{skill_list}\n\
         \n\
         Transcription: \"{raw}\"\n\
         \n\
         Reply with JSON only — no explanation, no markdown.\n\
         If a skill matches:\n\
         {{\"skill\":\"name\",\"vars\":{{\"key\":\"value\"}}}}\n\
         If nothing matches:\n\
         {{\"skill\":null}}\n\
         JSON:"
    );

    let response = ollama.generate(&prompt, 300).await.ok()?;

    // Extract the JSON object from the response (LLM may add preamble)
    let json_str = extract_json_object(&response)?;
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // skill null → no match
    let skill_val = value.get("skill")?;
    if skill_val.is_null() {
        return None;
    }
    let skill_name = skill_val.as_str()?.to_string();
    if skill_name.is_empty() {
        return None;
    }

    // Verify the named skill actually exists
    intent_skills.iter().find(|s| s.name == skill_name)?;

    let vars: std::collections::HashMap<String, String> = value
        .get("vars")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();

    Some((skill_name, vars))
}

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

/// Find the outermost `{...}` in a string, handling nesting.
fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let mut depth = 0i32;
    for (i, b) in s.as_bytes()[start..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
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
    ollama: OllamaClient,
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
        let skills_list = skills::load_all(Some(&user_skills_dir));
        log::info!(
            "loaded {} skills: {}",
            skills_list.len(),
            skills_list
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

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
            ollama: OllamaClient::new(),
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
        let new = skills::load_all(dir.as_deref());
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
            return Ok(());
        }
        self.set_tray(TrayState::Processing);

        let transcriber = {
            let guard = self.transcriber.lock().await;
            guard.clone()
        };
        let transcriber = transcriber.ok_or_else(|| {
            self.set_tray(TrayState::Idle);
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
        // Phase 1: instant trigger-pattern matching (legacy skills, 0 ms)
        // Phase 2: LLM intent classification (intent-based skills, ~400 ms)
        //          — only runs when AI cleanup is enabled (requires Ollama)

        let trigger_match = {
            let list = self.skills.lock();
            skills::match_skill(&list, &raw).map(|(s, v)| (s.clone(), v))
        };

        let skill_match = if trigger_match.is_some() {
            trigger_match
        } else if ai_on {
            let t_classify = Instant::now();
            let snapshot: Vec<Skill> = self.skills.lock().clone();
            let result = classify_skill_with_llm(&raw, &snapshot, &self.ollama).await;
            let classify_ms = t_classify.elapsed().as_millis() as u64;
            match &result {
                Some((name, _)) => info!(
                    "[latency #{n}] skill classify → {name} in {classify_ms}ms"
                ),
                None => info!("[latency #{n}] skill classify → no match in {classify_ms}ms"),
            }
            result.and_then(|(name, vars)| {
                let list = self.skills.lock();
                list.iter().find(|s| s.name == name).map(|s| (s.clone(), vars))
            })
        } else {
            None
        };

        // ── Skill execution ──────────────────────────────────────────────────

        if let Some((skill, mut vars)) = skill_match {
            info!("[latency #{n}] running skill: {}", skill.name);

            // Ensure [body] always resolves — use the full utterance if the
            // classifier didn't explicitly extract it.
            if !vars.contains_key("body") {
                vars.insert("body".into(), raw.clone());
            }
            vars.insert("name".into(), user_name.clone());

            let system_prompt = skills::interpolate(&skill.system_prompt, &vars);

            let t_ollama = Instant::now();
            let llm_output = if ai_on {
                match self.ollama.generate(&system_prompt, 1024).await {
                    Ok(s) => s,
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
            let ollama_ms = t_ollama.elapsed().as_millis() as u64;

            vars.insert("result".into(), strip_llm_preamble(&llm_output));
            let final_text = skills::interpolate(&skill.output_template, &vars);
            let with_dict = crate::dictionary::apply_to_text(&final_text, &preserve_terms);
            let trimmed = with_dict.trim().to_string();

            if trimmed.is_empty() {
                info!("[latency #{n}] skill {} produced empty output; skipping paste", skill.name);
                self.set_tray(TrayState::Idle);
                return Ok(());
            }

            let t_paste = Instant::now();
            paste_text(&trimmed)?;
            let paste_ms = t_paste.elapsed().as_millis() as u64;
            let total_ms = t_release.elapsed().as_millis() as u64;
            info!(
                "[latency #{n}] skill={} audio={audio_ms}ms whisper={whisper_ms}ms \
                 ollama={ollama_ms}ms paste={paste_ms}ms total={total_ms}ms text={trimmed:?}",
                skill.name
            );
            self.set_tray(TrayState::Transcribed);
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
        Ok(())
    }
}
