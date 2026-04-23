//! Whisper model catalog + downloader.
//!
//! Four models are selectable at runtime via the tray submenu:
//!
//! | id        | size    | approx speed vs base | use case                      |
//! |-----------|---------|----------------------|-------------------------------|
//! | tiny.en   | 39 MB   | 3x faster            | fastest, weakest accuracy     |
//! | base.en   | 142 MB  | baseline             | good balance (current default)|
//! | small.en  | 466 MB  | 1.8x slower          | big accuracy gain             |
//! | medium.en | 1.4 GB  | 4x slower            | best accuracy, still < 2 s    |
//!
//! Models are downloaded from HuggingFace (ggerganov/whisper.cpp) on first
//! selection and cached in `$APP_DATA/models/`. A progress callback fires
//! as bytes stream in so the tray can show "Downloading small.en… 37%".

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use log::info;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhisperModel {
    TinyEn,
    BaseEn,
    SmallEn,
    MediumEn,
}

impl WhisperModel {
    /// Preferred default for fresh installs — best quality-per-ms on M-series.
    pub const DEFAULT: Self = Self::SmallEn;

    pub const ALL: &'static [Self] = &[
        Self::TinyEn,
        Self::BaseEn,
        Self::SmallEn,
        Self::MediumEn,
    ];

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "tiny.en" => Some(Self::TinyEn),
            "base.en" => Some(Self::BaseEn),
            "small.en" => Some(Self::SmallEn),
            "medium.en" => Some(Self::MediumEn),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::TinyEn => "tiny.en",
            Self::BaseEn => "base.en",
            Self::SmallEn => "small.en",
            Self::MediumEn => "medium.en",
        }
    }

    pub fn filename(&self) -> String {
        format!("ggml-{}.bin", self.id())
    }

    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.filename()
        )
    }

    /// Approximate download size; used to validate cached files are complete.
    pub fn expected_size_bytes(&self) -> u64 {
        match self {
            Self::TinyEn => 39 * 1024 * 1024,
            Self::BaseEn => 142 * 1024 * 1024,
            Self::SmallEn => 466 * 1024 * 1024,
            Self::MediumEn => 1_420 * 1024 * 1024,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::TinyEn => "Tiny — fastest (39 MB)",
            Self::BaseEn => "Base — balanced (142 MB)",
            Self::SmallEn => "Small — good accuracy (466 MB)",
            Self::MediumEn => "Medium — best accuracy (1.4 GB)",
        }
    }
}

/// The exact message attached to the cancellation error. Callers match on
/// this string to decide whether the error was a supersession or a real
/// failure (thiserror would be cleaner but adds a dep for one variant).
pub const CANCELLED_MSG: &str = "download cancelled (superseded by newer pick)";

/// Ensure `model` is available on disk, downloading if necessary. Returns
/// the absolute path. Invokes `on_progress(done_bytes, total_bytes)` during
/// the download so callers can surface progress (total may be 0 if the
/// server doesn't send Content-Length).
///
/// `still_wanted` is polled between download chunks. When it returns false
/// (e.g. the user picked a different model), the current download is
/// aborted, the partial `.part` file is removed, and `Err(CANCELLED_MSG)`
/// is returned so the caller can treat it as a no-op rather than a failure.
/// For call sites that never need cancellation (warm-up on boot), pass
/// `|| true`.
pub async fn ensure_model(
    app: &AppHandle,
    model: WhisperModel,
    mut on_progress: impl FnMut(u64, u64) + Send,
    still_wanted: impl Fn() -> bool + Send,
) -> Result<PathBuf> {
    let dir = model_dir(app)?;
    tokio::fs::create_dir_all(&dir).await.ok();
    let path = dir.join(model.filename());
    let min_size = model.expected_size_bytes() * 9 / 10;

    if let Ok(meta) = tokio::fs::metadata(&path).await {
        if meta.len() >= min_size {
            info!("model {} cached at {}", model.id(), path.display());
            return Ok(path);
        }
        let _ = tokio::fs::remove_file(&path).await;
    }

    info!(
        "downloading {} from {} -> {}",
        model.id(),
        model.url(),
        path.display()
    );
    let client = reqwest::Client::builder().build().context("http client")?;
    let resp = client
        .get(model.url())
        .send()
        .await
        .with_context(|| format!("request {}", model.url()))?;
    if !resp.status().is_success() {
        return Err(anyhow!("download {} status {}", model.id(), resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);

    let tmp = path.with_extension("bin.part");
    let mut file = tokio::fs::File::create(&tmp).await.context("create tmp")?;
    let mut stream = resp.bytes_stream();
    let mut done: u64 = 0;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        // Check between chunks — keeps cancellation latency ≤ one chunk
        // (typically <64 KB, well under 100 ms on a hot connection).
        if !still_wanted() {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp).await;
            info!("cancelled {} at {} / {} bytes", model.id(), done, total);
            return Err(anyhow!(CANCELLED_MSG));
        }
        let chunk = chunk.context("download chunk")?;
        file.write_all(&chunk).await.context("write chunk")?;
        done += chunk.len() as u64;
        on_progress(done, total);
    }
    file.flush().await.ok();
    drop(file);
    tokio::fs::rename(&tmp, &path).await.context("rename tmp")?;
    info!("downloaded {} ({} bytes)", model.id(), done);
    Ok(path)
}

fn model_dir(app: &AppHandle) -> Result<PathBuf> {
    let base = app.path().app_data_dir().context("app data dir")?;
    Ok(base.join("models"))
}

/// Standalone model-path resolver for the bench tool (no AppHandle). Defaults
/// to base.en to preserve the bench's historical baseline; callers pass
/// `--model` on the CLI to override.
pub fn default_model_path_standalone() -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?;
    Some(
        data_dir
            .join("com.svara.app")
            .join("models")
            .join("ggml-base.en.bin"),
    )
}
