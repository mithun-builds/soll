//! Floating status overlay — a transparent frameless pill that appears
//! center-screen whenever Soll is active (recording, processing, done)
//! and hides itself when the app returns to idle.

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "overlay";
const W: f64 = 340.0;
const H: f64 = 80.0;

/// Events emitted to the overlay webview. `tag = "kind"` lets the frontend
/// switch on a single discriminant field.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayEvent {
    Recording,
    Processing,
    SkillDone { name: String },
    Transcribed,
}

/// Create the overlay window at startup — hidden, transparent, always-on-top.
/// It is never destroyed; subsequent calls to `show`/`hide` toggle visibility.
pub fn build(app: &AppHandle) -> anyhow::Result<()> {
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?view=overlay".into()),
    )
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .visible(false)
    .resizable(false)
    .inner_size(W, H)
    .center()
    .shadow(true)
    .build()?;
    Ok(())
}

// ── public API ─────────────────────────────────────────────────────────────

pub fn recording(app: &AppHandle) {
    emit(app, OverlayEvent::Recording);
}

pub fn processing(app: &AppHandle) {
    emit(app, OverlayEvent::Processing);
}

pub fn skill_done(app: &AppHandle, name: &str) {
    emit(app, OverlayEvent::SkillDone { name: name.to_string() });
    schedule_hide(app.clone(), 2000);
}

pub fn transcribed(app: &AppHandle) {
    emit(app, OverlayEvent::Transcribed);
    schedule_hide(app.clone(), 1200);
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

// ── internals ──────────────────────────────────────────────────────────────

fn emit(app: &AppHandle, event: OverlayEvent) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    // Re-center each time in case the user changed display arrangement.
    let _ = w.center();
    let _ = w.show();
    let _ = w.emit("overlay-update", &event);
}

fn schedule_hide(app: AppHandle, after_ms: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(after_ms)).await;
        hide(&app);
    });
}
