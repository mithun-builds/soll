use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex as AsyncMutex;

use crate::audio::AudioRecorder;
use crate::cleanup::OllamaClient;
use crate::dictionary::Dictionary;
use crate::formatter::{self, Format};
use crate::model::{ensure_model, WhisperModel, CANCELLED_MSG};
use crate::paste::paste_text;
use crate::settings::{Settings, KEY_WHISPER_MODEL};
use crate::transcribe::Transcriber;
use crate::tray::{self, TrayState};

static DICTATION_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct AppState {
    pub app: AppHandle,
    pub dictionary: Arc<Dictionary>,
    pub settings: Arc<Settings>,
    /// Currently-loaded whisper model, kept in sync with the Transcriber.
    current_model: Mutex<WhisperModel>,
    /// Bumped on every `switch_model` call. In-flight downloads poll this
    /// between chunks and abort if it diverges from their captured value,
    /// so picking a new model mid-download instantly cancels the old one.
    switch_epoch: std::sync::atomic::AtomicU64,
    recorder: Mutex<Option<AudioRecorder>>,
    transcriber: AsyncMutex<Option<Arc<Transcriber>>>,
    /// Held exclusively during model swaps so dictations don't start on a
    /// half-loaded transcriber.
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
        let preferred = settings
            .get_or_default(KEY_WHISPER_MODEL, WhisperModel::DEFAULT.id());
        let model = WhisperModel::from_id(&preferred).unwrap_or(WhisperModel::DEFAULT);

        Self {
            app,
            dictionary: dict,
            settings,
            current_model: Mutex::new(model),
            switch_epoch: std::sync::atomic::AtomicU64::new(0),
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

    fn set_tray(&self, state: TrayState) {
        tray::set_state(&self.app, state);
    }

    /// Download the preferred model (if needed), load whisper, compile Metal
    /// kernels, and pre-warm Ollama. Tray stays in Loading until this completes.
    pub async fn warm_up(self: &Arc<Self>) -> Result<()> {
        let model = self.current_model();
        info!("warming up: whisper={} + cleanup", model.id());

        // Ollama warm-up runs concurrently.
        let me = self.clone();
        tokio::spawn(async move {
            me.ollama.warm_up().await;
        });

        use std::sync::atomic::Ordering;
        let my_epoch = self.switch_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.load_model(model, my_epoch).await
    }

    /// Switch to a different model. Downloads if not cached; loads + warms;
    /// swaps atomically. Bumps `switch_epoch` so any in-flight download
    /// aborts on its next chunk. Blocks on `swap_guard` so concurrent
    /// dictations can't observe a half-loaded state.
    pub async fn switch_model(self: Arc<Self>, model: WhisperModel) -> Result<()> {
        use std::sync::atomic::Ordering;
        let my_epoch = self.switch_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        if model == self.current_model() && self.transcriber.lock().await.is_some() {
            info!("switch_model: {} already active", model.id());
            return Ok(());
        }
        info!("switch_model -> {} (epoch={my_epoch})", model.id());
        self.load_model(model, my_epoch).await?;
        self.settings
            .set(KEY_WHISPER_MODEL, model.id())
            .context("persist whisper_model setting")?;
        Ok(())
    }

    /// Core load routine used by both warm_up and switch_model. `epoch` is
    /// the switch generation we were spawned for; if the global epoch
    /// advances before we finish, we bail out early without touching the
    /// live transcriber.
    async fn load_model(self: &Arc<Self>, model: WhisperModel, epoch: u64) -> Result<()> {
        use std::sync::atomic::Ordering;

        let _swap = self.swap_guard.lock().await;
        // Another switch may have run while we waited on the lock — bail
        // before we start any IO.
        if self.switch_epoch.load(Ordering::SeqCst) != epoch {
            info!("load_model({}) superseded before lock acquired", model.id());
            return Ok(());
        }
        let t0 = std::time::Instant::now();
        self.set_tray(TrayState::Loading);

        let me_progress = self.clone();
        let me_wanted = self.clone();
        let path_result = ensure_model(
            &self.app,
            model,
            move |done, total| me_progress.report_download_progress(model, done, total),
            move || me_wanted.switch_epoch.load(Ordering::SeqCst) == epoch,
        )
        .await;
        let path = match path_result {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains(CANCELLED_MSG) {
                    info!("load_model({}) cancelled mid-download", model.id());
                    tray::set_title(&self.app, None);
                    // Leave the tray in Loading — a newer load_model is
                    // already queued behind us and will take over.
                    return Ok(());
                }
                return Err(e);
            }
        };
        // Download finished (or cache hit). The next phase (whisper file
        // load + Metal kernel warm) takes 0.5–2 s depending on model size.
        // Show a visible indicator in the menu bar so the user knows why
        // the blue dot is still blinking — otherwise a cached swap feels
        // like it hangs.
        tray::set_title(&self.app, Some(&format!(" ⟳ {}", model.id())));
        tray::set_status_text(&format!("Loading {}…", model.id()));
        info!(
            "{} on disk in {:?} — loading into whisper…",
            model.id(),
            t0.elapsed()
        );

        let t_load_start = std::time::Instant::now();
        let model_id = model.id();
        let transcriber = tokio::task::spawn_blocking(move || -> Result<Transcriber> {
            let t = Transcriber::load(&path)?;
            info!("{} loaded in {:?}; warming Metal kernels…", model_id, t_load_start.elapsed());
            let t_warm = std::time::Instant::now();
            t.warm()?;
            info!("{} Metal kernels compiled in {:?}", model_id, t_warm.elapsed());
            Ok(t)
        })
        .await
        .map_err(|e| anyhow!("join error: {e}"))??;

        // Final epoch check before swap — if the user clicked elsewhere
        // while Metal was compiling, don't clobber with a stale model.
        if self.switch_epoch.load(Ordering::SeqCst) != epoch {
            info!("load_model({}) superseded after whisper load", model.id());
            return Ok(());
        }

        *self.transcriber.lock().await = Some(Arc::new(transcriber));
        *self.current_model.lock() = model;
        info!("{} fully ready in {:?}", model.id(), t0.elapsed());
        self.set_tray(TrayState::Idle);
        Ok(())
    }

    fn report_download_progress(&self, model: WhisperModel, done: u64, total: u64) {
        if total == 0 {
            return;
        }
        // Throttle updates to every 1% so the menu-bar title doesn't flicker
        // on every chunk. Log every 5%.
        static LAST_TITLE_PCT: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(u64::MAX);
        static LAST_LOG_BUCKET: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(u64::MAX);
        let pct = done * 100 / total;

        let prev_title = LAST_TITLE_PCT.load(std::sync::atomic::Ordering::Relaxed);
        if pct != prev_title {
            LAST_TITLE_PCT.store(pct, std::sync::atomic::Ordering::Relaxed);
            tray::set_title(&self.app, Some(&format!(" ↓ {pct}%")));
            tray::set_status_text(&format!(
                "Downloading {}… {pct}% ({} / {} MB)",
                model.id(),
                done / (1024 * 1024),
                total / (1024 * 1024)
            ));
        }
        let log_bucket = pct / 5;
        if log_bucket != LAST_LOG_BUCKET.load(std::sync::atomic::Ordering::Relaxed) {
            LAST_LOG_BUCKET.store(log_bucket, std::sync::atomic::Ordering::Relaxed);
            info!(
                "download {}: {pct}% ({} / {} MB)",
                model.id(),
                done / (1024 * 1024),
                total / (1024 * 1024)
            );
        }
    }

    pub async fn on_press(self: Arc<Self>) -> Result<()> {
        {
            let mut rec = self.is_recording.lock();
            if *rec {
                return Ok(());
            }
            *rec = true;
        }
        // Show "Initializing" first — the cpal stream takes ~50–120 ms to
        // build, and the user shouldn't start speaking until capture is
        // actually live.
        self.set_tray(TrayState::Initializing);
        let recorder = AudioRecorder::start()?;
        *self.recorder.lock() = Some(recorder);
        // Mic is now capturing — signal the user to speak.
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
            // Under 250ms — accidental tap.
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

        // Build whisper initial_prompt from the user's personal dictionary.
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

        // Auto-formatting: if the raw transcript looks like a numbered or
        // bullet list, format it deterministically and skip Ollama entirely.
        // An LLM always turns structured lists back into prose; a regex never.
        let format = formatter::detect(&raw);
        let t_ollama_start = Instant::now();
        let (polished, ollama_ms, ollama_used) = match format {
            Format::Plain => match self.ollama.polish_with_terms(&raw, &preserve_terms).await {
                Ok(p) => {
                    let ms = t_ollama_start.elapsed().as_millis() as u64;
                    (p, ms, true)
                }
                Err(e) => {
                    let ms = t_ollama_start.elapsed().as_millis() as u64;
                    warn!("[latency #{n}] cleanup skipped ({e:?}), using raw transcript");
                    (raw.clone(), ms, false)
                }
            },
            Format::Bullets | Format::Numbered => {
                info!("[latency #{n}] format detected: {:?} (skipping ollama)", format);
                (formatter::apply(&raw, format), 0, false)
            }
        };

        // Deterministic dictionary post-processor: rewrites "home lane" ->
        // "HomeLane", "homelane" -> "HomeLane" etc. Runs AFTER cleanup (or
        // formatter) because both can split/re-wrap casings unpredictably.
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
            "[latency #{n}] audio={audio_ms}ms whisper={whisper_ms}ms {ollama_tag}={ollama_ms}ms paste={paste_ms}ms total={total_ms}ms chars={char_count} text={trimmed:?}"
        );

        self.set_tray(TrayState::Transcribed);
        Ok(())
    }
}
