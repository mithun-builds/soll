//! Tauri IPC commands — the surface the webview calls via `invoke(...)`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::dictionary::Entry;
use crate::settings::{DEFAULT_AI_CLEANUP, KEY_AI_CLEANUP, KEY_USER_NAME, KEY_WHISPER_MODEL};
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
    /// Plain-English activation description from `## Intent`. Present for
    /// intent-based (new) skills.
    pub intent: Option<String>,
    /// Legacy trigger phrases from `## Triggers`. Present for pattern-based
    /// (old) skills. Empty for intent-based skills.
    pub triggers: Vec<String>,
    pub source: String, // "default" | "custom"
    pub native: Option<String>,
    /// True if a factory/built-in version ships with the app under this name.
    pub has_builtin_default: bool,
}

#[tauri::command]
pub fn skill_list(state: State<'_, Arc<AppState>>) -> Vec<SkillInfo> {
    state
        .skills
        .lock()
        .iter()
        .map(|s| SkillInfo {
            name: s.name.clone(),
            description: s.description.clone(),
            intent: s.intent.clone(),
            triggers: s.trigger_templates(),
            source: s.source.as_str().to_string(),
            native: s.native.clone(),
            has_builtin_default: crate::skills::builtin_source(&s.name).is_some(),
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

/// Return the factory/default markdown for a built-in skill, regardless
/// of whether the user has an override. Used for the "Reset" preview.
#[tauri::command]
pub fn skill_get_default_source(name: String) -> Option<String> {
    crate::skills::builtin_source(&name).map(|s| s.to_string())
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
    // Forbid colliding with a built-in via "new" — user should click Edit
    // on the built-in to override it instead.
    if crate::skills::builtin_source(&name).is_some() {
        return Err(format!(
            "`{name}` is a built-in skill; edit it from its row instead of creating"
        ));
    }
    std::fs::write(&path, &markdown).map_err(|e| e.to_string())?;
    state.reload_skills();
    Ok(name)
}

/// Save edits to an existing skill. If it's a built-in, this creates a
/// user override; next reload will prefer the user file. Parses the
/// markdown first; the name in the markdown must match the target.
#[tauri::command]
pub fn skill_save(
    name: String,
    markdown: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let parsed = crate::skills::Skill::from_markdown(&markdown).map_err(|e| e.to_string())?;
    if parsed.name != name {
        return Err(format!(
            "name changed: file is `{name}` but markdown declares `{}`",
            parsed.name
        ));
    }
    let dir = state.user_skills_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.md", name));
    std::fs::write(&path, &markdown).map_err(|e| e.to_string())?;
    state.reload_skills();
    Ok(())
}

/// Remove a user override for a built-in (reverts to factory) or delete
/// a purely user-created skill entirely.
#[tauri::command]
pub fn skill_reset(
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let dir = state.user_skills_dir().map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.md", name));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
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
