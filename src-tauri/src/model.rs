use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use log::info;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
const MODEL_FILE: &str = "ggml-base.en.bin";
// Expected size ~148MB; we only check that the file is non-trivial.
const MIN_SIZE_BYTES: u64 = 100 * 1024 * 1024;

pub async fn ensure_model(app: &AppHandle) -> Result<PathBuf> {
    let dir = model_dir(app)?;
    tokio::fs::create_dir_all(&dir).await.ok();
    let path = dir.join(MODEL_FILE);

    if let Ok(meta) = tokio::fs::metadata(&path).await {
        if meta.len() >= MIN_SIZE_BYTES {
            info!("model already present at {}", path.display());
            return Ok(path);
        } else {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }

    info!("downloading whisper base.en ({MODEL_URL}) → {}", path.display());
    let client = reqwest::Client::builder()
        .build()
        .context("build http client")?;
    let resp = client
        .get(MODEL_URL)
        .send()
        .await
        .context("start model download")?;
    if !resp.status().is_success() {
        return Err(anyhow!("model download status: {}", resp.status()));
    }

    let tmp = path.with_extension("bin.part");
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .context("create tmp file")?;
    let mut stream = resp.bytes_stream();
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download chunk")?;
        file.write_all(&chunk).await.context("write chunk")?;
    }
    file.flush().await.ok();
    drop(file);
    tokio::fs::rename(&tmp, &path)
        .await
        .context("rename tmp -> final")?;
    info!("model ready at {}", path.display());
    Ok(path)
}

fn model_dir(app: &AppHandle) -> Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .context("app data dir")?;
    Ok(base.join("models"))
}

/// Resolve the default model path WITHOUT an AppHandle — used by the
/// benchmark binary and other offline tools. On macOS this matches what
/// Tauri's `app_data_dir()` returns for identifier `com.svara.app`:
///   ~/Library/Application Support/com.svara.app/models/ggml-base.en.bin
pub fn default_model_path_standalone() -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?;
    Some(data_dir.join("com.svara.app").join("models").join(MODEL_FILE))
}
