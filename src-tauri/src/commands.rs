//! Tauri IPC commands — the surface the webview calls via `invoke(...)`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::cleanup::OllamaModel;
use crate::dictionary::Entry;
use crate::settings::{
    DEFAULT_AI_CLEANUP, KEY_AI_CLEANUP, KEY_OLLAMA_MODEL, KEY_USER_NAME, KEY_WHISPER_MODEL,
};
use crate::state::AppState;

#[derive(Serialize)]
pub struct DictEntry {
    pub word: String,
    pub weight: i32,
    pub added_at: String,
}

impl From<Entry> for DictEntry {
    fn from(e: Entry) -> Self {
        Self {
            word: e.word,
            weight: e.weight,
            added_at: e.added_at,
        }
    }
}

#[tauri::command]
pub fn dict_list(state: State<'_, Arc<AppState>>) -> Result<Vec<DictEntry>, String> {
    state
        .dictionary
        .list()
        .map(|rows| rows.into_iter().map(DictEntry::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn dict_add(
    word: String,
    weight: Option<i32>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DictEntry>, String> {
    state
        .dictionary
        .add(&word, weight.unwrap_or(1))
        .map_err(|e| e.to_string())?;
    dict_list(state)
}

#[tauri::command]
pub fn dict_remove(
    word: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DictEntry>, String> {
    state
        .dictionary
        .remove(&word)
        .map_err(|e| e.to_string())?;
    dict_list(state)
}

// ── settings ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SettingsSnapshot {
    pub user_name: String,
    pub ai_cleanup_enabled: bool,
    pub whisper_model: String,
    pub dictionary_count: i64,
}

#[tauri::command]
pub fn settings_get(state: State<'_, Arc<AppState>>) -> Result<SettingsSnapshot, String> {
    let s = &state.settings;
    Ok(SettingsSnapshot {
        user_name: s.get_or_default(KEY_USER_NAME, ""),
        ai_cleanup_enabled: s
            .get(KEY_AI_CLEANUP)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(DEFAULT_AI_CLEANUP),
        whisper_model: s.get_or_default(KEY_WHISPER_MODEL, "small.en"),
        dictionary_count: state.dictionary.count().unwrap_or(0),
    })
}

#[derive(Deserialize)]
pub struct SettingsUpdate {
    pub user_name: Option<String>,
    pub ai_cleanup_enabled: Option<bool>,
}

// ── skills ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// Trigger phrases from `## Triggers` — how the skill activates.
    pub triggers: Vec<String>,
    pub native: Option<String>,
    /// False when the user has turned this skill off. Disabled skills are
    /// kept in the list (and preserve their markdown/edits) but never match
    /// at runtime.
    pub enabled: bool,
    /// "ai" (goes through Ollama) or "snippet" (literal paste, no LLM).
    pub kind: String,
}

#[tauri::command]
pub fn skill_list(state: State<'_, Arc<AppState>>) -> Vec<SkillInfo> {
    let disabled = state.settings.disabled_skills();
    state
        .skills
        .lock()
        .iter()
        .map(|s| SkillInfo {
            name: s.name.clone(),
            description: s.description.clone(),
            triggers: s.trigger_templates(),
            native: s.native.clone(),
            enabled: !disabled.contains(&s.name),
            kind: s.kind.as_str().to_string(),
        })
        .collect()
}

#[tauri::command]
pub fn skill_get_source(name: String, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let skills = state.skills.lock();
    skills
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.markdown_source.clone())
        .ok_or_else(|| format!("skill `{name}` not found"))
}

/// Create a brand-new skill from arbitrary markdown. Returns the parsed
/// name on success. Fails if a skill with that name already exists.
#[tauri::command]
pub fn skill_create(
    markdown: String,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let parsed = crate::skills::Skill::from_markdown(&markdown).map_err(|e| e.to_string())?;
    let name = parsed.name.clone();
    let dir = state.user_skills_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.md", name));
    if path.exists() {
        return Err(format!("skill `{name}` already exists"));
    }
    std::fs::write(&path, &markdown).map_err(|e| e.to_string())?;
    state.reload_skills();
    Ok(name)
}

/// Save edits to an existing skill. The name in the markdown may differ
/// from the target — in that case the file is renamed on disk and any
/// disabled flag migrates with it.
#[tauri::command]
pub fn skill_save(
    name: String,
    markdown: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let parsed = crate::skills::Skill::from_markdown(&markdown).map_err(|e| e.to_string())?;
    let dir = state.user_skills_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    if parsed.name == name {
        let path = dir.join(format!("{}.md", name));
        std::fs::write(&path, &markdown).map_err(|e| e.to_string())?;
        state.reload_skills();
        return Ok(());
    }

    // ── rename path ──────────────────────────────────────────────────────
    let new_path = dir.join(format!("{}.md", parsed.name));
    if new_path.exists() {
        return Err(format!("a skill named `{}` already exists.", parsed.name));
    }

    // Write the new file first.
    std::fs::write(&new_path, &markdown).map_err(|e| e.to_string())?;
    // Remove the old user file if it exists.
    let old_path = dir.join(format!("{}.md", name));
    if old_path.exists() {
        if let Err(e) = std::fs::remove_file(&old_path) {
            // Roll back the new file so we don't end up with both names.
            let _ = std::fs::remove_file(&new_path);
            return Err(format!("couldn't remove old file: {e}"));
        }
    }
    // Carry the disabled flag across the rename.
    let disabled = state.settings.disabled_skills();
    if disabled.contains(&name) {
        let _ = state.settings.set_skill_disabled(&name, false);
        let _ = state.settings.set_skill_disabled(&parsed.name, true);
    }
    state.reload_skills();
    Ok(())
}

/// Turn a skill on or off without deleting it. Disabled skills stay in the
/// list and keep their edits, but never match at runtime.
#[tauri::command]
pub fn skill_set_enabled(
    name: String,
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // Verify the skill actually exists so the UI can't drift silently.
    let exists = state.skills.lock().iter().any(|s| s.name == name);
    if !exists {
        return Err(format!("skill `{name}` not found"));
    }
    state
        .settings
        .set_skill_disabled(&name, !enabled)
        .map_err(|e| e.to_string())
}

/// Delete a skill: removes the markdown file and clears any disabled-state
/// entry (a deleted skill doesn't need to remember it was also turned off).
#[tauri::command]
pub fn skill_delete(
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let dir = state.user_skills_dir().map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.md", name));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    let _ = state.settings.set_skill_disabled(&name, false);
    state.reload_skills();
    Ok(())
}

// ── models ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub size: String,
    pub is_cached: bool,
    pub is_active: bool,
    pub is_downloading: bool,
    /// True for the model we recommend by default (best quality-per-ms on M-series).
    pub is_recommended: bool,
}

#[tauri::command]
pub fn models_list(state: State<'_, Arc<AppState>>) -> Vec<ModelInfo> {
    let current = state.current_model();
    let downloading = *state.downloading.lock();
    crate::model::WhisperModel::ALL
        .iter()
        .map(|m| ModelInfo {
            id: m.id().to_string(),
            label: m.short_name().to_string(),
            size: m.size_label().to_string(),
            is_cached: state.is_model_cached(*m),
            is_active: *m == current,
            is_downloading: downloading == Some(*m),
            is_recommended: *m == crate::model::WhisperModel::DEFAULT,
        })
        .collect()
}

#[tauri::command]
pub async fn model_activate(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let model = crate::model::WhisperModel::from_id(&id)
        .ok_or_else(|| format!("unknown model: {id}"))?;
    let st = state.inner().clone();
    st.switch_model(model).await.map_err(|e| e.to_string())
}

/// Pick which model the user *intends* to use. Persists the choice and updates
/// the in-memory selection without attempting to load the transcriber. Use
/// this from the onboarding picker before the file exists; switch to it via
/// `model_activate` once it's cached.
#[tauri::command]
pub fn model_select(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let model = crate::model::WhisperModel::from_id(&id)
        .ok_or_else(|| format!("unknown model: {id}"))?;
    state.set_preferred_model(model);
    Ok(())
}

#[tauri::command]
pub async fn model_download(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let model = crate::model::WhisperModel::from_id(&id)
        .ok_or_else(|| format!("unknown model: {id}"))?;
    let st = state.inner().clone();
    st.start_download(model).await.map_err(|e| e.to_string())
}

/// Cancel any in-flight Whisper model download. Bumping the download_epoch
/// atomic invalidates the running download's `wanted_fn` check inside
/// `ensure_model`, which then aborts on the next progress tick.
#[tauri::command]
pub fn model_cancel_download(state: State<'_, Arc<AppState>>) {
    use std::sync::atomic::Ordering;
    state.download_epoch.fetch_add(1, Ordering::SeqCst);
}

/// Delete a downloaded Whisper model file from disk, plus any partial
/// download left over from a cancelled fetch. The cancel path in
/// `ensure_model` deliberately preserves `.part` files so re-clicking a
/// model resumes from where it left off — but the user-facing Delete
/// button needs to wipe both, otherwise "delete + redownload" would
/// surprise-resume an old partial.
#[tauri::command]
pub fn model_delete(
    id: String,
    app: AppHandle,
) -> Result<(), String> {
    let model = crate::model::WhisperModel::from_id(&id)
        .ok_or_else(|| format!("unknown model: {id}"))?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let path = dir.join(model.filename());
    let part = path.with_extension("bin.part");
    let _ = std::fs::remove_file(&part);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── ollama models ──────────────────────────────────────────────────────────

/// One entry in the AI-model picker list.
#[derive(Serialize)]
pub struct OllamaModelInfo {
    /// Ollama tag, e.g. `"llama3.2:3b"`. Pass back to `ollama_model_set`.
    pub tag: String,
    pub display_name: String,
    pub author: String,
    pub size: String,
    /// True when this model is the currently selected one.
    pub is_active: bool,
    /// True when the model is already pulled locally in Ollama.
    pub is_pulled: bool,
}

/// List all known AI models with their active and pulled state.
#[tauri::command]
pub async fn ollama_models_list(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<OllamaModelInfo>, String> {
    let active = state.ollama.active_model();
    let pulled = state.ollama.list_pulled_tags().await;
    Ok(OllamaModel::ALL
        .iter()
        .map(|m| OllamaModelInfo {
            tag: m.tag().to_string(),
            display_name: m.display_name().to_string(),
            author: m.author().to_string(),
            size: m.size_label().to_string(),
            is_active: m.tag() == active,
            is_pulled: pulled.contains(m.tag()),
        })
        .collect())
}

/// Persist and activate a new Ollama model. The next LLM call will use it.
#[tauri::command]
pub fn ollama_model_set(tag: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    OllamaModel::from_tag(&tag).ok_or_else(|| format!("unknown model tag: {tag}"))?;
    state.ollama.set_model(&tag);
    state
        .settings
        .set(KEY_OLLAMA_MODEL, &tag)
        .map_err(|e| e.to_string())
}

/// Kick off a pull of the currently active Ollama model. Returns instantly —
/// the actual pull runs in a background task and can take 5–10+ minutes for a
/// 2 GB model. The frontend should switch to the "pulling" state immediately
/// and rely on `onboarding_status.ollama_active_model_pulled` (polled every
/// 2 s) to detect completion. Awaiting the HTTP response would block the
/// IPC call for the entire duration, freezing the wizard.
#[tauri::command]
pub fn ollama_pull_active(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let tag = state.ollama.active_model();
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60 * 60))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                log::error!("ollama_pull_active: build client failed: {e}");
                return;
            }
        };
        let body = serde_json::json!({ "name": tag, "stream": false });
        match client
            .post("http://127.0.0.1:11434/api/pull")
            .json(&body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                log::info!("ollama pull {tag} complete");
            }
            Ok(r) => log::error!("ollama pull {tag} returned {}", r.status()),
            Err(e) => log::error!("ollama pull {tag} failed: {e}"),
        }
    });
    Ok(())
}

/// Delete the currently active Ollama model from local Ollama storage.
#[tauri::command]
pub async fn ollama_delete_active(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let tag = state.ollama.active_model();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "name": tag });
    let resp = client
        .delete("http://127.0.0.1:11434/api/delete")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("ollama delete failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ollama delete returned {}", resp.status()));
    }
    Ok(())
}

// ── window helpers (called from the onboarding frontend) ──────────────────

/// Open the Settings window (any section — the sidebar handles navigation).
#[tauri::command]
pub fn open_settings_window_cmd(app: AppHandle) {
    crate::tray::open_settings_window(&app);
}

/// Open System Settings to a specific Privacy pane.
/// `section` is the URL fragment, e.g. "Privacy_Microphone" or
/// "Privacy_Accessibility".
#[tauri::command]
pub fn open_privacy_settings(section: String) -> Result<(), String> {
    let url = format!(
        "x-apple.systempreferences:com.apple.preference.security?{section}"
    );
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Close the onboarding window programmatically (called after dismiss).
#[tauri::command]
pub fn close_onboarding_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.close();
    }
}

/// Restart Soll. Used after the user grants Accessibility — `AXIsProcessTrusted`
/// caches the "untrusted" verdict for the process lifetime, so a fresh launch
/// is the only reliable way to pick up the new permission.
///
/// We don't use `app.restart()` because on macOS it relaunches the binary
/// inside the .app bundle directly, which bypasses LaunchServices and tends
/// to die before initialisation completes on macOS 16. Instead, schedule a
/// detached `open` of the .app for a moment after this process exits — by
/// then the bundle ID is free, and macOS launches the new instance cleanly.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    use std::sync::atomic::Ordering;

    crate::APP_RESTARTING.store(true, Ordering::SeqCst);

    // Walk current_exe = .../Soll.app/Contents/MacOS/soll up three levels
    // to get the .app bundle path. Fall back to /Applications/Soll.app if
    // the structure is unexpected (e.g. running via `cargo run`).
    let app_path = std::env::current_exe()
        .ok()
        .and_then(|p| {
            let contents = p.parent()?.parent()?;
            let bundle = contents.parent()?;
            bundle.to_str().map(String::from)
        })
        .unwrap_or_else(|| "/Applications/Soll.app".to_string());

    log::info!("restart_app: relaunching via `open {}`", app_path);

    // Detached shell job. The 1-second sleep is the important bit — by then
    // our process has fully exited, the bundle id is no longer registered
    // as running, and `open` launches a fresh Soll cleanly.
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("sleep 1 && /usr/bin/open '{}'", app_path))
        .spawn();

    app.exit(0);
}

/// Tell the tray whether onboarding still has unfinished steps. The frontend
/// calls this from its poll loop so the tray icon's red badge and the
/// "Setup Guide… ●" menu label stay in sync with reality.
#[tauri::command]
pub fn set_onboarding_indicator(needed: bool, app: AppHandle) {
    crate::tray::set_setup_needed(&app, needed);
}

// ── settings (existing) ────────────────────────────────────────────────────

#[tauri::command]
pub fn settings_set(
    update: SettingsUpdate,
    state: State<'_, Arc<AppState>>,
) -> Result<SettingsSnapshot, String> {
    if let Some(v) = update.user_name {
        state
            .settings
            .set(KEY_USER_NAME, v.trim())
            .map_err(|e| e.to_string())?;
    }
    if let Some(v) = update.ai_cleanup_enabled {
        state
            .settings
            .set(KEY_AI_CLEANUP, if v { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
    }
    settings_get(state)
}
