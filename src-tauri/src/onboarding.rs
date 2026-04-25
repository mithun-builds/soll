//! Onboarding window — guides new users through the initial setup steps.
//!
//! `onboarding_status` is polled every 2 s by the frontend and returns a
//! snapshot of every prerequisite: model cached, permissions granted, Ollama
//! running, first dictation completed, and at least one skill created.
//!
//! `onboarding_dismiss` writes the dismissed flag to settings so the window
//! no longer opens automatically on subsequent launches.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::settings::{KEY_HAS_DICTATED, KEY_ONBOARDING_DISMISSED};
use crate::state::AppState;

// ── public types ───────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum PermState {
    Granted,
    Denied,
    Unknown,
}

#[derive(Serialize)]
pub struct OnboardingStatus {
    // Step 1 — Whisper model
    pub model_cached: bool,
    pub model_downloading: bool,
    /// 0–100 while a download is active; null otherwise.
    pub model_download_pct: Option<u8>,
    // Step 2 — Microphone
    pub mic_permission: PermState,
    // Step 3 — Accessibility
    pub accessibility: bool,
    // Step 4 — Ollama
    pub ollama_running: bool,
    // Step 5 — First dictation
    pub has_dictated: bool,
    // Step 6 — Skills (optional)
    pub has_skills: bool,
    // Meta
    pub dismissed: bool,
}

// ── commands ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn onboarding_status(
    state: State<'_, Arc<AppState>>,
) -> Result<OnboardingStatus, String> {
    let current = state.current_model();
    let downloading = *state.downloading.lock();

    let model_cached = state.is_model_cached(current);
    let model_downloading = downloading.is_some();
    let model_download_pct = {
        let done = state.download_bytes_done.load(Ordering::Relaxed);
        let total = state.download_bytes_total.load(Ordering::Relaxed);
        if model_downloading {
            if total > 0 {
                Some(((done * 100) / total).min(100) as u8)
            } else {
                Some(0)
            }
        } else {
            None
        }
    };

    let mic_permission = check_mic_permission();
    let accessibility = check_accessibility();
    let ollama_running = check_ollama_running().await;
    let has_dictated =
        state.settings.get_or_default(KEY_HAS_DICTATED, "false") == "true";
    let has_skills = !state.skills.lock().is_empty();
    let dismissed =
        state.settings.get_or_default(KEY_ONBOARDING_DISMISSED, "false") == "true";

    Ok(OnboardingStatus {
        model_cached,
        model_downloading,
        model_download_pct,
        mic_permission,
        accessibility,
        ollama_running,
        has_dictated,
        has_skills,
        dismissed,
    })
}

#[tauri::command]
pub fn onboarding_dismiss(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state
        .settings
        .set(KEY_ONBOARDING_DISMISSED, "true")
        .map_err(|e| e.to_string())
}

/// Trigger the macOS microphone permission dialog.
///
/// On macOS, an app appears in System Settings → Privacy → Microphone only
/// *after* it has requested access. This command calls
/// `[AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:]`
/// which shows the system prompt. The onboarding frontend polls status every
/// 2 s and will update automatically once the user grants or denies.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn request_mic_permission() -> Result<(), String> {
    use block::ConcreteBlock;
    use objc::runtime::Class;
    use objc::{msg_send, sel, sel_impl};

    tokio::task::spawn_blocking(|| unsafe {
        let cls = match Class::get("AVCaptureDevice") {
            Some(c) => c,
            None => return,
        };
        let ns_cls = match Class::get("NSString") {
            Some(c) => c,
            None => return,
        };
        // AVMediaTypeAudio = @"soun"
        let media_type: *mut objc::runtime::Object = msg_send![
            ns_cls,
            stringWithUTF8String: b"soun\0".as_ptr() as *const std::os::raw::c_char
        ];
        // Only request if not yet determined (status 0 = notDetermined)
        let status: i64 = msg_send![cls, authorizationStatusForMediaType: media_type];
        if status != 0 {
            return; // already granted or denied — no dialog needed
        }
        // Fire the system dialog; the frontend poll picks up the new status.
        let block = ConcreteBlock::new(|_granted: bool| {});
        let block = block.copy();
        let _: () = msg_send![
            cls,
            requestAccessForMediaType: media_type
            completionHandler: &*block
        ];
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn request_mic_permission() -> Result<(), String> {
    Ok(())
}

// ── permission / connectivity checks ──────────────────────────────────────

#[cfg(target_os = "macos")]
fn check_accessibility() -> bool {
    // AXIsProcessTrusted() is a C function in ApplicationServices, which is
    // linked transitively by AppKit / Tauri on macOS. Returns false until the
    // user grants the permission in System Settings › Privacy › Accessibility.
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

#[cfg(not(target_os = "macos"))]
fn check_accessibility() -> bool {
    true
}

#[cfg(target_os = "macos")]
fn check_mic_permission() -> PermState {
    // [AVCaptureDevice authorizationStatusForMediaType: AVMediaTypeAudio]
    // AVMediaTypeAudio = NSString @"soun"
    // AVAuthorizationStatus values: 0=notDetermined 1=restricted 2=denied 3=authorized
    //
    // We use Class::get (returns Option) so that if AVFoundation hasn't been
    // loaded into the process yet, we return Unknown instead of panicking.
    use objc::runtime::Class;
    use objc::{msg_send, sel, sel_impl};

    let status: i64 = unsafe {
        let cls = match Class::get("AVCaptureDevice") {
            Some(c) => c,
            None => return PermState::Unknown,
        };
        let ns_cls = match Class::get("NSString") {
            Some(c) => c,
            None => return PermState::Unknown,
        };
        // AVMediaTypeAudio = @"soun"
        let media_type: *mut objc::runtime::Object = msg_send![
            ns_cls,
            stringWithUTF8String: b"soun\0".as_ptr() as *const std::os::raw::c_char
        ];
        msg_send![cls, authorizationStatusForMediaType: media_type]
    };

    match status {
        3 => PermState::Granted,
        1 | 2 => PermState::Denied,
        _ => PermState::Unknown,
    }
}

#[cfg(not(target_os = "macos"))]
fn check_mic_permission() -> PermState {
    PermState::Granted
}

/// Ping Ollama with a 1-second timeout. Called on every poll tick so the
/// timeout must be well under the 2 s polling interval.
async fn check_ollama_running() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get("http://127.0.0.1:11434/api/tags")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

// ── window helper (called from tray and lib) ───────────────────────────────

pub fn open_window(app: &AppHandle) {
    crate::tray::open_onboarding_window(app);
}
