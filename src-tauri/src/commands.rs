//! Tauri IPC commands — the surface the webview calls via `invoke(...)`.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::dictionary::Entry;
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
