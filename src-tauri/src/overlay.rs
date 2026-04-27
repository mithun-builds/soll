//! Floating status overlay — a transparent frameless pill that appears
//! center-screen whenever Soll is active (recording, processing, done).
//!
//! Architecture: the window is **always on screen** after startup, but its
//! content is transparent when idle. Events tell the frontend to show or
//! clear the pill — the window itself never hides.
//!
//! Multi-monitor: the pill is centered on the screen containing the mouse
//! cursor, using Tauri's cross-platform monitor APIs (no raw ObjC struct
//! calls, which crash via wrong calling convention in objc 0.2).
//!
//! macOS panel traits (non-struct msg_send! only — safe on objc 0.2):
//!   • `orderFrontRegardless` — surfaces without stealing keyboard focus.
//!   • `setIgnoresMouseEvents: YES` — clicks fall through to the app below.
//!   • Collection-behavior flags — visible on every Space, skips Cmd+`.

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "overlay";
const W: f64 = 255.0;
const H: f64 = 60.0;

// macOS NSWindowCollectionBehavior bit-flags (AppKit header values).
#[cfg(target_os = "macos")]
const COLLECTION_CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
#[cfg(target_os = "macos")]
const COLLECTION_STATIONARY: u64 = 1 << 4;
#[cfg(target_os = "macos")]
const COLLECTION_IGNORES_CYCLE: u64 = 1 << 6;

/// Events emitted to the overlay webview.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayEvent {
    Recording,
    Processing,
    SkillDone { name: String, is_phrase: bool },
    Transcribed,
    /// Clear the pill — window stays on screen but becomes fully transparent.
    Idle,
}

/// Create the overlay window at startup and make it permanently visible.
/// Content is driven entirely by `overlay-update` events.
pub fn build(app: &AppHandle) -> anyhow::Result<()> {
    let w = WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?view=overlay".into()),
    )
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .visible(false)       // shown immediately below via orderFrontRegardless
    .resizable(false)
    .inner_size(W, H)
    .center()
    .shadow(false)
    .build()?;

    // build() runs in the Tauri setup hook on the main thread — safe for AppKit.
    // IMPORTANT: only non-struct msg_send! calls here (integers / void only).
    #[cfg(target_os = "macos")]
    {
        apply_panel_style(&w);
        order_front_regardless(&w);
    }
    #[cfg(not(target_os = "macos"))]
    { let _ = w.show(); }

    Ok(())
}

// ── public API ─────────────────────────────────────────────────────────────

pub fn recording(app: &AppHandle) {
    emit(app, OverlayEvent::Recording);
}

pub fn processing(app: &AppHandle) {
    emit(app, OverlayEvent::Processing);
}

pub fn skill_done(app: &AppHandle, name: &str, is_phrase: bool) {
    emit(app, OverlayEvent::SkillDone { name: name.to_string(), is_phrase });
    schedule_idle(app.clone(), 2000);
}

pub fn transcribed(app: &AppHandle) {
    emit(app, OverlayEvent::Transcribed);
    schedule_idle(app.clone(), 1200);
}

/// Clear the pill (window stays on screen, content becomes transparent).
pub fn hide(app: &AppHandle) {
    emit(app, OverlayEvent::Idle);
}

// ── internals ──────────────────────────────────────────────────────────────

fn emit(app: &AppHandle, event: OverlayEvent) {
    let Some(w) = app.get_webview_window(LABEL) else { return };

    // Emit first so the pill appears immediately, before any repositioning.
    if let Err(e) = app.emit("overlay-update", &event) {
        log::warn!("overlay: emit failed: {e:?}");
    }

    // Reposition on the cursor's screen via the main thread (AppKit requirement).
    if !matches!(event, OverlayEvent::Idle) {
        let app2 = app.clone();
        let w2   = w.clone();
        let _ = app.run_on_main_thread(move || center_on_cursor_screen(&app2, &w2));
    }
}

fn schedule_idle(app: AppHandle, after_ms: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(after_ms)).await;
        hide(&app);
    });
}

/// Center the overlay on the screen the mouse cursor is currently on.
///
/// Tauri's `w.center()` always uses the primary screen (the one with the
/// menu bar). On a Mac + external monitor the user is often on the other
/// screen, so we look up the cursor position and find the right monitor.
///
/// Uses only Tauri's cross-platform APIs — no raw ObjC calls.
fn center_on_cursor_screen(app: &AppHandle, w: &tauri::WebviewWindow) {
    use tauri::LogicalPosition;

    let cursor = match app.cursor_position() {
        Ok(c)  => c,
        Err(_) => { let _ = w.center(); return; }
    };

    let monitors = match app.available_monitors() {
        Ok(m)  => m,
        Err(_) => { let _ = w.center(); return; }
    };

    // Find the monitor whose physical rect contains the cursor.
    let monitor = monitors
        .iter()
        .find(|m| {
            let pos  = m.position();
            let size = m.size();
            cursor.x >= pos.x as f64
                && cursor.x < pos.x as f64 + size.width  as f64
                && cursor.y >= pos.y as f64
                && cursor.y < pos.y as f64 + size.height as f64
        })
        .or_else(|| monitors.first());

    match monitor {
        Some(m) => {
            let pos   = m.position();
            let size  = m.size();
            let scale = m.scale_factor();
            let lx = pos.x as f64 / scale + (size.width  as f64 / scale - W) / 2.0;
            let ly = pos.y as f64 / scale + (size.height as f64 / scale - H) / 2.0;
            let _ = w.set_position(tauri::Position::Logical(LogicalPosition { x: lx, y: ly }));
        }
        None => {
            let _ = w.center();
        }
    }
}

// ── macOS helpers (non-struct msg_send! only) ──────────────────────────────

/// Set collection behavior and mouse-event passthrough on the NSWindow.
/// Only passes u64 and BOOL (i8) to msg_send! — no structs, no crash risk.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn apply_panel_style(w: &tauri::WebviewWindow) {
    use cocoa::base::{id, YES};
    use objc::{msg_send, sel, sel_impl};

    let Ok(ptr) = w.ns_window() else {
        log::warn!("overlay: ns_window() unavailable");
        return;
    };
    let ns_win: id = ptr as id;
    unsafe {
        let behavior: u64 =
            COLLECTION_CAN_JOIN_ALL_SPACES | COLLECTION_STATIONARY | COLLECTION_IGNORES_CYCLE;
        let _: () = msg_send![ns_win, setCollectionBehavior: behavior];
        let _: () = msg_send![ns_win, setIgnoresMouseEvents: YES];
    }
}

/// Show without activating our app or stealing keyboard focus.
/// No arguments, void return — safe with objc 0.2's msg_send!.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn order_front_regardless(w: &tauri::WebviewWindow) {
    use cocoa::base::id;
    use objc::{msg_send, sel, sel_impl};

    let Ok(ptr) = w.ns_window() else { return };
    let ns_win: id = ptr as id;
    unsafe {
        let _: () = msg_send![ns_win, orderFrontRegardless];
    }
}
