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
use crate::model::ensure_model;
use crate::paste::paste_text;
use crate::transcribe::Transcriber;
use crate::tray::{self, TrayState};

static DICTATION_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct AppState {
    pub app: AppHandle,
    pub dictionary: Arc<Dictionary>,
    recorder: Mutex<Option<AudioRecorder>>,
    transcriber: AsyncMutex<Option<Arc<Transcriber>>>,
    ollama: OllamaClient,
    is_recording: Mutex<bool>,
}

impl AppState {
    pub fn new(app: AppHandle) -> Self {
        let dict = match app.path().app_data_dir() {
            Ok(dir) => {
                let db = dir.join("dict.db");
                match Dictionary::open(&db) {
                    Ok(d) => Arc::new(d),
                    Err(e) => {
                        log::error!("dictionary open failed at {}: {e:?}", db.display());
                        panic!("cannot open dictionary db: {e:?}");
                    }
                }
            }
            Err(e) => panic!("no app_data_dir for dictionary: {e:?}"),
        };

        Self {
            app,
            dictionary: dict,
            recorder: Mutex::new(None),
            transcriber: AsyncMutex::new(None),
            ollama: OllamaClient::new(),
            is_recording: Mutex::new(false),
        }
    }

    fn set_tray(&self, state: TrayState) {
        tray::set_state(&self.app, state);
    }

    /// Download the model (if needed), load whisper, compile Metal kernels,
    /// and pre-warm Ollama — all in parallel where possible. Tray shows
    /// Loading until this fully completes.
    pub async fn warm_up(self: &Arc<Self>) -> Result<()> {
        let t0 = std::time::Instant::now();
        info!("warming up transcriber + cleanup…");

        // Ollama warm-up runs concurrently — we don't block on it.
        let me = self.clone();
        tokio::spawn(async move {
            me.ollama.warm_up().await;
        });

        let path = ensure_model(&self.app).await?;
        info!("model ready ({:?}); loading whisper…", t0.elapsed());

        // Load + warm on the blocking pool so we don't starve the runtime.
        let t = tokio::task::spawn_blocking(move || -> Result<Transcriber> {
            let t_load = std::time::Instant::now();
            let t = Transcriber::load(&path)?;
            info!("whisper loaded in {:?}; compiling Metal kernels…", t_load.elapsed());
            let t_warm = std::time::Instant::now();
            t.warm()?;
            info!("Metal kernels compiled in {:?}", t_warm.elapsed());
            Ok(t)
        })
        .await
        .map_err(|e| anyhow!("join error: {e}"))??;

        *self.transcriber.lock().await = Some(Arc::new(t));
        info!("transcriber fully ready in {:?}", t0.elapsed());
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
        self.set_tray(TrayState::Recording);
        let recorder = AudioRecorder::start()?;
        *self.recorder.lock() = Some(recorder);
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

        let t_ollama_start = Instant::now();
        let preserve_terms: Vec<String> = dict_words.into_iter().map(|e| e.word).collect();
        let (polished, ollama_ms, ollama_used) = match self.ollama.polish_with_terms(&raw, &preserve_terms).await {
            Ok(p) => {
                let ms = t_ollama_start.elapsed().as_millis() as u64;
                (p, ms, true)
            }
            Err(e) => {
                let ms = t_ollama_start.elapsed().as_millis() as u64;
                warn!("[latency #{n}] cleanup skipped ({e:?}), using raw transcript");
                (raw.clone(), ms, false)
            }
        };

        let trimmed = polished.trim().to_string();
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

        self.set_tray(TrayState::Done);
        Ok(())
    }
}
