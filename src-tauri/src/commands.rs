//! Tauri IPC commands — the surface the webview calls via `invoke(...)`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

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
