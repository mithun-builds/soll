//! Tauri IPC commands — the surface the webview calls via `invoke(...)`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::dictionary::Entry;
use crate::settings::{
    DEFAULT_AI_CLEANUP, DEFAULT_SIGN_OFF, KEY_AI_CLEANUP, KEY_EMAIL_SIGN_OFF, KEY_USER_NAME,
    KEY_WHISPER_MODEL,
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
    pub email_sign_off: String,
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
        email_sign_off: s.get_or_default(KEY_EMAIL_SIGN_OFF, DEFAULT_SIGN_OFF),
        whisper_model: s.get_or_default(KEY_WHISPER_MODEL, "small.en"),
        dictionary_count: state.dictionary.count().unwrap_or(0),
    })
}

#[derive(Deserialize)]
pub struct SettingsUpdate {
    pub user_name: Option<String>,
    pub ai_cleanup_enabled: Option<bool>,
    pub email_sign_off: Option<String>,
}

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
    if let Some(v) = update.email_sign_off {
        let trimmed = v.trim();
        let value = if trimmed.is_empty() {
            DEFAULT_SIGN_OFF
        } else {
            trimmed
        };
        state
            .settings
            .set(KEY_EMAIL_SIGN_OFF, value)
            .map_err(|e| e.to_string())?;
    }
    settings_get(state)
}
