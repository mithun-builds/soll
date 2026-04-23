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
    /// Most recent model the user clicked in the submenu. A background
    /// download auto-activates only if this still equals the downloaded
    /// model — otherwise the download silently becomes a cache entry for
    /// later use.
    pending_target: Mutex<WhisperModel>,
    /// Model currently being downloaded (if any). Used to dedupe repeat
    /// clicks and to know whether starting a new download should cancel
    /// a running one.
    downloading: Mutex<Option<WhisperModel>>,
    /// Bumped whenever a NEW download starts. In-flight downloads poll
    /// this between chunks and abort on divergence. Cached-model swaps
    /// do NOT bump it — ongoing downloads continue unaffected.
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
            pending_target: Mutex::new(model),
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

        // Boot-time load: always foreground (no existing transcriber to
        // fall back on), always honored (no concurrent user picks yet).
        if self.is_model_cached(model) {
            self.clone().load_and_swap(model, None, true).await
        } else {
            use std::sync::atomic::Ordering;
            let my_epoch = self.download_epoch.fetch_add(1, Ordering::SeqCst) + 1;
            *self.downloading.lock() = Some(model);
            let result = self.clone().load_and_swap(model, Some(my_epoch), true).await;
            let mut dl = self.downloading.lock();
            if *dl == Some(model) {
                *dl = None;
            }
            result
        }
    }

    /// Switch to a different model.
    ///
    /// Behavior depends on whether the model is already on disk:
    ///
    /// - **Cached model** → quick foreground swap (<1 s). Does NOT cancel
    ///   any in-flight background download; that keeps running so the
    ///   user doesn't lose progress.
    /// - **Uncached model** → spawn a background download. The user can
    ///   keep dictating with the current model. When the download finishes,
    ///   the new model auto-activates *iff* the user hasn't clicked
    ///   something else in the meantime (`pending_target`). Otherwise it's
    ///   silently cached for later.
    /// - **Repeat click on the model already downloading** → no-op. Just
    ///   updates the intent marker.
    /// - **New uncached click while a different model is downloading** →
    ///   cancels the in-flight download and starts the new one.
    pub async fn switch_model(self: Arc<Self>, model: WhisperModel) -> Result<()> {
        *self.pending_target.lock() = model;

        if model == self.current_model() && self.transcriber.lock().await.is_some() {
            info!("switch_model: {} already active", model.id());
            return Ok(());
        }

        if self.is_model_cached(model) {
            info!("switch_model -> {} (cached, foreground swap)", model.id());
            self.clone().load_and_swap(model, /*epoch=*/ None, true).await
        } else {
            // Check if this model is already being downloaded — avoid
            // cancel-and-restart on repeat clicks.
            {
                let mut dl = self.downloading.lock();
                if *dl == Some(model) {
                    info!(
                        "switch_model -> {} (already downloading; pending only)",
                        model.id()
                    );
                    return Ok(());
                }
                *dl = Some(model);
            }
            // Cancel any previous download and stamp a new epoch.
            use std::sync::atomic::Ordering;
            let my_epoch = self.download_epoch.fetch_add(1, Ordering::SeqCst) + 1;
            info!(
                "switch_model -> {} (background download, epoch={my_epoch})",
                model.id()
            );
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                let result = me.clone().load_and_swap(model, Some(my_epoch), false).await;
                // Clear the downloading flag if it's still us. A newer
                // download may have overwritten it — in that case leave
                // it alone.
                {
                    let mut dl = me.downloading.lock();
                    if *dl == Some(model) {
                        *dl = None;
                    }
                }
                if let Err(e) = result {
                    if !e.to_string().contains(CANCELLED_MSG) {
                        log::error!("background load {}: {e:?}", model.id());
                    }
                }
            });
            Ok(())
        }
    }

    /// Core load routine.
    ///
    /// `epoch`:
    /// - `Some(e)` — in-flight download; polls `download_epoch` and aborts
    ///   if it diverges (the user started a different download).
    /// - `None` — cached-path swap; no cancellation check (nothing is
    ///   actually downloading).
    ///
    /// `foreground` controls whether the tray icon enters the Loading
    /// state during the download phase. Cached swaps are foreground;
    /// background downloads stay out of the way so dictation continues.
    ///
    /// Before committing the swap we check `pending_target` — if the user
    /// moved on to a different model during this load, the downloaded/
    /// reloaded model is silently cached for future use and not activated.
    async fn load_and_swap(
        self: Arc<Self>,
        model: WhisperModel,
        epoch: Option<u64>,
        foreground: bool,
    ) -> Result<()> {
        use std::sync::atomic::Ordering;

        let _swap = self.swap_guard.lock().await;
        if let Some(e) = epoch {
            if self.download_epoch.load(Ordering::SeqCst) != e {
                info!("load_and_swap({}) superseded before lock", model.id());
                return Ok(());
            }
        }
        let t0 = std::time::Instant::now();

        if foreground {
            self.set_tray(TrayState::Loading);
        }

        let me_progress = self.clone();
        let me_wanted = self.clone();
        let still_wanted_epoch = epoch;
        let path_result = ensure_model(
            &self.app,
            model,
            move |done, total| me_progress.report_download_progress(model, done, total),
            move || match still_wanted_epoch {
                Some(e) => me_wanted.download_epoch.load(Ordering::SeqCst) == e,
                None => true,
            },
        )
        .await;
        let path = match path_result {
            Ok(p) => p,
            Err(e) => {
                if e.to_string().contains(CANCELLED_MSG) {
                    info!("load_and_swap({}) cancelled mid-download", model.id());
                    tray::set_title(&self.app, None);
                    return Ok(());
                }
                return Err(e);
            }
        };

        // Before doing the expensive whisper load, check whether the user
        // still wants this model. They may have clicked something else
        // during the download; if so, just leave the file on disk cached
        // for next time.
        if *self.pending_target.lock() != model {
            info!(
                "load_and_swap({}): download done but pending_target is now {}; caching only",
                model.id(),
                self.pending_target.lock().id()
            );
            tray::set_title(&self.app, None);
            return Ok(());
        }

        if !foreground {
            self.set_tray(TrayState::Loading);
        }
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
            info!(
                "{} loaded in {:?}; warming Metal kernels…",
                model_id,
                t_load_start.elapsed()
            );
            let t_warm = std::time::Instant::now();
            t.warm()?;
            info!(
                "{} Metal kernels compiled in {:?}",
                model_id,
                t_warm.elapsed()
            );
            Ok(t)
        })
        .await
        .map_err(|e| anyhow!("join error: {e}"))??;

        // Final pending-target check — during Metal warm (~1 s) the user
        // may have clicked elsewhere.
        if *self.pending_target.lock() != model {
            info!(
                "load_and_swap({}): whisper loaded but pending_target moved to {}; discarding",
                model.id(),
                self.pending_target.lock().id()
            );
            tray::set_title(&self.app, None);
            // Restore the tray to whatever the foreground state wants.
            self.set_tray(TrayState::Idle);
            return Ok(());
        }

        *self.transcriber.lock().await = Some(Arc::new(transcriber));
        *self.current_model.lock() = model;
        tray::update_model_check(model);
        let _ = self.settings.set(KEY_WHISPER_MODEL, model.id());
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
