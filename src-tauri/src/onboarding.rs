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
    /// True when Ollama is running AND the currently active model has been pulled.
    pub ollama_active_model_pulled: bool,
    /// True when an Ollama installation is detected on disk, regardless of
    /// whether it's currently running. Lets the wizard show "Open Ollama"
    /// instead of the install instructions when the user has it but quit it.
    pub ollama_installed: bool,
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
    let ollama_installed = check_ollama_installed();
    let ollama_active_model_pulled = if ollama_running {
        let active = state.ollama.active_model();
        state.ollama.list_pulled_tags().await.contains(&active)
    } else {
        false
    };
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
        ollama_active_model_pulled,
        ollama_installed,
        has_dictated,
        has_skills,
        dismissed,
    })
}

#[tauri::command]
pub fn onboarding_dismiss(
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    state
        .settings
        .set(KEY_ONBOARDING_DISMISSED, "true")
        .map_err(|e| e.to_string())?;
    // The user just confirmed they're done — clear the red indicator
    // immediately so the tray icon goes plain on the next paint.
    crate::tray::set_setup_needed(&app, false);
    Ok(())
}

/// Trigger the macOS microphone permission dialog via AVFoundation.
///
/// Uses AVCaptureDevice requestAccessForMediaType:completionHandler: — the only
/// reliable way to surface the TCC dialog on macOS 15+. The block is
/// heap-allocated via .copy() and then forgotten so AVFoundation owns its
/// lifetime; the 2-second frontend poll picks up the granted state.
#[tauri::command]
pub fn request_mic_permission() {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::os::raw::{c_char, c_void};
        use block::ConcreteBlock;
        use objc::runtime::Class;
        use objc::{msg_send, sel, sel_impl};

        extern "C" {
            fn dlopen(filename: *const c_char, flag: i32) -> *mut c_void;
        }
        dlopen(
            b"/System/Library/Frameworks/AVFoundation.framework/AVFoundation\0".as_ptr()
                as *const c_char,
            1,
        );

        let cls = match Class::get("AVCaptureDevice") {
            Some(c) => c,
            None => return,
        };
        let ns_cls = match Class::get("NSString") {
            Some(c) => c,
            None => return,
        };
        let media_type: *mut objc::runtime::Object = msg_send![
            ns_cls,
            stringWithUTF8String: b"soun\0".as_ptr() as *const c_char
        ];

        // .copy() heap-allocates the block so AVFoundation can retain it safely.
        // std::mem::forget transfers ownership to ObjC ARC — no use-after-free.
        let block = ConcreteBlock::new(|_granted: bool| {});
        let block = block.copy();
        let _: () = msg_send![
            cls,
            requestAccessForMediaType: media_type
            completionHandler: &*block
        ];
        std::mem::forget(block);
    }
}

/// Open System Settings → Privacy & Security → Accessibility.
///
/// Earlier this called `AXIsProcessTrustedWithOptions(prompt: true)` to surface
/// the macOS "Soll wants to control this computer" sheet. The side effect was
/// nasty: that call also refreshes the running process's trust cache, so if
/// any prior build of Soll had been granted, the next poll would flip the
/// step to "Done" instantly — the progress bar moved without the user
/// actually doing anything in Settings. Bypassing the prompt entirely keeps
/// the step's "Done" state honest: it only goes green after the user
/// explicitly toggles Soll on in Settings *and* restarts (because
/// AXIsProcessTrusted is cached for the process lifetime).
#[tauri::command]
pub fn request_accessibility_permission() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

// ── permission / connectivity checks ──────────────────────────────────────

#[cfg(target_os = "macos")]
pub(crate) fn check_accessibility() -> bool {
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn check_accessibility() -> bool {
    true
}

#[cfg(target_os = "macos")]
pub(crate) fn check_mic_permission() -> PermState {
    // [AVCaptureDevice authorizationStatusForMediaType: AVMediaTypeAudio]
    // AVMediaTypeAudio = NSString @"soun"
    // AVAuthorizationStatus values: 0=notDetermined 1=restricted 2=denied 3=authorized
    //
    // cpal uses CoreAudio, not AVFoundation — so AVCaptureDevice is never
    // loaded into the process automatically. Force-load AVFoundation via
    // dlopen before calling Class::get, otherwise it always returns None.
    use objc::runtime::Class;
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        extern "C" {
            fn dlopen(
                filename: *const std::os::raw::c_char,
                flag: std::os::raw::c_int,
            ) -> *mut std::os::raw::c_void;
        }
        let path = b"/System/Library/Frameworks/AVFoundation.framework/AVFoundation\0";
        dlopen(path.as_ptr() as *const _, 1 /* RTLD_LAZY */);
    }

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
pub(crate) fn check_mic_permission() -> PermState {
    PermState::Granted
}

/// True when an Ollama installation exists on disk, regardless of whether
/// the daemon is currently up. Detects the .app bundle (DMG/Cask install)
/// and the CLI (Homebrew install on Apple Silicon or Intel).
pub(crate) fn check_ollama_installed() -> bool {
    const CANDIDATES: &[&str] = &[
        "/Applications/Ollama.app",
        "/opt/homebrew/bin/ollama",
        "/usr/local/bin/ollama",
    ];
    CANDIDATES
        .iter()
        .any(|p| std::path::Path::new(p).exists())
}

/// Launch the Ollama .app via LaunchServices. No-op (well, an error logged
/// by `open`) when Ollama is CLI-only; the frontend should keep that case
/// on the "show install instructions" code path.
#[tauri::command]
pub fn open_ollama() {
    let _ = std::process::Command::new("open")
        .arg("-a")
        .arg("Ollama")
        .spawn();
}

/// Ping Ollama with a 1-second timeout. Called on every poll tick so the
/// timeout must be well under the 2 s polling interval.
pub(crate) async fn check_ollama_running() -> bool {
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

