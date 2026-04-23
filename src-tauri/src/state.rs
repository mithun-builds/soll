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
    /// Model currently being fetched in the background (if any). Exposed
    /// so tray::handle_download_click can dedupe repeat clicks and
    /// tray::build_model_submenu can label the item "Downloading…".
    pub downloading: Mutex<Option<WhisperModel>>,
    /// Bumped whenever a new download starts. In-flight downloads poll
    /// this between chunks and abort on divergence.
    download_epoch: std::sync::atomic::AtomicU64,
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

    /// Synchronous cache check — used by the tray handler to decide whether
    /// the swap can be foreground (cached, <1 s) or must be background
    /// (needs a download). `std::fs::metadata` is stat-level fast.
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

        if self.is_model_cached(model) {
            self.clone().load_and_activate(model).await
        } else {
            self.clone().boot_download_and_activate(model).await
        }
    }

    /// Activate a cached model. Fast foreground swap (<1 s). Errors if the
    /// model isn't cached — the tray prevents that click path by only
    /// rendering cached models as selectable, but we guard here anyway.
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

    /// Start a background download of an uncached model. Returns as soon
    /// as the background task is spawned. The download does NOT auto-
    /// activate the model — the user has to click it in the cached
    /// section afterward. This keeps "fetch" and "activate" as
    /// independent intents.
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
                // Suppress noisy log throttling — tray label is enough.
                let _ = &me_progress;
            },
            move || me_wanted.download_epoch.load(Ordering::SeqCst) == my_epoch,
        )
        .await;

        // Clear the downloading flag if it's still us (a newer download
        // may have superseded us).
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

    /// Load a cached model into memory and make it active. Used by
    /// `switch_model` (user clicked a cached item) and `warm_up` (boot
    /// path when the preferred model is already cached).
    async fn load_and_activate(self: Arc<Self>, model: WhisperModel) -> Result<()> {
        let _swap = self.swap_guard.lock().await;
        let t0 = std::time::Instant::now();
        self.set_tray(TrayState::Loading);
        tray::set_status_text(&format!("Loading {}…", model.id()));

        // Ensure-on-disk is a no-op for cached models (fast metadata check).
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

    /// Blocking foreground download used ONLY by `warm_up` on first
    /// boot when the preferred model isn't yet cached. There's no existing
    /// transcriber to fall back on, so we must block until ready.
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
            move || true, // boot downloads are never superseded
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

        // Mid-sentence corrections: "5 pm actually 6 pm" -> "6 pm",
        // "Tuesday I mean Wednesday" -> "Wednesday". Only run on prose;
        // list formatting already handles its own structure.
        let after_corrections = if matches!(format, Format::Plain) {
            corrections::apply(&polished)
        } else {
            polished
        };
        // Deterministic dictionary post-processor: rewrites "home lane" ->
        // "HomeLane", "homelane" -> "HomeLane" etc. Runs AFTER cleanup /
        // corrections because both can reshape the surface text.
        let with_dict = crate::dictionary::apply_to_text(&after_corrections, &preserve_terms);
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
