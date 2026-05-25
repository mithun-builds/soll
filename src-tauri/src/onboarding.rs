//! Onboarding window — guides new users through the initial setup steps.
//!
//! `onboarding_status` is polled every 2 s by the frontend and returns a
//! snapshot of every prerequisite: model cached, permissions granted, Ollama
//! running, first dictation completed, and at least one skill created.
//!
//! `onboarding_dismiss` writes the dismissed flag to settings so the window
//! no longer opens automatically on subsequent launches.

use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::settings::{KEY_HAS_DICTATED, KEY_ONBOARDING_DISMISSED};
use crate::state::AppState;

/// Bundle ID Soll ships with — also the codesign identifier when the binary
/// is properly re-signed via `codesign --force --sign -`. Used to detect
/// the "linker-signed local build" case where the identifier ends up as a
/// hash like `soll-dc34b220ba651dd5` and TCC grants don't stick.
const EXPECTED_SIGNING_IDENTIFIER: &str = "com.soll.app";

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
    /// False when the running binary's codesign identifier doesn't match
    /// `com.soll.app`. This happens with locally-built dev binaries that
    /// weren't re-signed (`pnpm tauri build` alone leaves a linker-signed
    /// binary with a hash identifier). When this is false AND accessibility
    /// is false, the UI shows a specific hint explaining that the existing
    /// System Settings grant can't apply, instead of looping on "Pending".
    pub signing_identifier_ok: bool,
    // Step 4 — Ollama
    pub ollama_running: bool,
    /// True when Ollama is running AND the currently active model has been pulled.
    pub ollama_active_model_pulled: bool,
    /// True when an Ollama installation is detected on disk, regardless of
    /// whether it's currently running. Lets the wizard show "Open Ollama"
    /// instead of the install instructions when the user has it but quit it.
    pub ollama_installed: bool,
    /// True when the .app bundle is installed at `/Applications/Ollama.app`.
    /// Surfaced as an explicit checkmark on Step 4 so the user can see at a
    /// glance which install shape they have (or both, or neither).
    pub ollama_app_installed: bool,
    /// True when the Ollama CLI is on disk (`brew install ollama`). Surfaced
    /// alongside `ollama_app_installed` for the install-status checklist.
    pub ollama_cli_installed: bool,
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
    let signing_identifier_ok = current_signing_identifier() == Some(EXPECTED_SIGNING_IDENTIFIER);
    let ollama_running = check_ollama_running().await;
    let ollama_app_installed = check_ollama_app_installed();
    let ollama_cli_installed = check_ollama_cli_installed();
    let ollama_installed = ollama_app_installed || ollama_cli_installed;
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
        signing_identifier_ok,
        ollama_running,
        ollama_active_model_pulled,
        ollama_installed,
        ollama_app_installed,
        ollama_cli_installed,
        has_dictated,
        has_skills,
        dismissed,
    })
}

/// Return the codesign identifier of the currently running binary, or
/// `None` if it can't be determined. Cached via `OnceLock` because this
/// is polled every 2 seconds by the onboarding window and the answer
/// can't change without a process restart anyway.
///
/// `codesign -dv` writes display info to stderr including a line
/// `Identifier=...`. `--identifier` is a **signing** flag and is rejected
/// in display mode (it prints the usage banner and exits non-zero — an
/// earlier version of this function used that combination and ended up
/// always returning the usage banner as the "identifier", which made the
/// onboarding's mismatch warning fire for every user).
fn current_signing_identifier() -> Option<&'static str> {
    static IDENT: OnceLock<Option<String>> = OnceLock::new();
    IDENT
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?;
            let out = std::process::Command::new("/usr/bin/codesign")
                .args(["-dv"])
                .arg(&exe)
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .find_map(|line| line.strip_prefix("Identifier=").map(str::to_owned))
        })
        .as_deref()
}

/// Launch Terminal.app and run a brew install command for Ollama, so the
/// user can install without leaving Soll. We don't run brew as a Tauri
/// subprocess because (a) brew might prompt for the sudo password —
/// Terminal handles password prompts natively, and (b) the user benefits
/// from seeing the full install output in case anything fails.
///
/// `shape` is `"app"` (cask, menu-bar app) or `"cli"` (formula, CLI only).
/// After install, the next `onboarding_status` poll picks up the new
/// install via `check_ollama_app_installed` / `check_ollama_cli_installed`.
#[tauri::command]
pub fn install_ollama_via_terminal(shape: String) -> Result<(), String> {
    let cmd = match shape.as_str() {
        "app" => "brew install --cask ollama && open -a Ollama",
        "cli" => "brew install ollama && ollama serve",
        other => return Err(format!("unknown install shape: {other}")),
    };
    // `do script` in Terminal.app opens a new window and executes the
    // command immediately (sends Return). User watches it run; brew
    // prompts for the sudo password natively inside Terminal if needed.
    //
    // Embedded command is double-quoted in AppleScript; brew commands
    // never contain " so simple substitution is safe here.
    let applescript = format!(
        r#"tell application "Terminal"
            activate
            do script "{cmd}"
        end tell"#
    );
    std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &applescript])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("launch Terminal: {e}"))
}

/// Wipe the Accessibility TCC grant for `com.soll.app`. The next call to
/// the macOS accessibility APIs will re-prompt, giving the user a fresh
/// path to re-grant against the current binary's signature.
///
/// Used by the onboarding window's "Reset & re-grant" button — surfaced
/// when Accessibility is pending despite the user having toggled the
/// System Settings switch on, typically because of a code-signing
/// identifier mismatch (e.g. a stale grant from a previous local build).
#[tauri::command]
pub fn reset_accessibility_grant() -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/tccutil")
        .args(["reset", "Accessibility", "com.soll.app"])
        .status()
        .map_err(|e| format!("tccutil failed to launch: {e}"))?;
    if !status.success() {
        return Err(format!("tccutil exited with {status}"));
    }
    Ok(())
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

/// True when the Ollama macOS .app bundle is on disk (DMG drag-install or
/// `brew install --cask ollama`). The .app, when launched, starts a
/// background menu-bar process that serves `/api` on port 11434.
pub(crate) fn check_ollama_app_installed() -> bool {
    std::path::Path::new("/Applications/Ollama.app").exists()
}

/// True when the Ollama CLI binary is on disk (`brew install ollama` on
/// Apple Silicon or Intel). The CLI alone doesn't start a server — the
/// user (or our `open_ollama` command) has to run `ollama serve`.
pub(crate) fn check_ollama_cli_installed() -> bool {
    const CANDIDATES: &[&str] = &[
        "/opt/homebrew/bin/ollama",
        "/usr/local/bin/ollama",
    ];
    CANDIDATES
        .iter()
        .any(|p| std::path::Path::new(p).exists())
}

// The old `check_ollama_installed()` wrapper used to fold app+CLI detection
// into one boolean. It's gone — callers now use the two specific helpers
// directly and compute `ollama_installed = app || cli` inline in
// `onboarding_status`. Keeping the wrapper around triggered a dead-code
// warning and obscured which install shape any given call actually cares
// about (the .app and the CLI need different launch paths).

/// Launch Ollama so its daemon starts listening on 11434.
///
/// Two install shapes are supported:
///   * **App bundle** (`brew install --cask ollama` or the DMG) — prefer
///     `open -a Ollama`. LaunchServices handles the menu-bar icon, login
///     items, and lifecycle on logout.
///   * **CLI only** (`brew install ollama` — the path the README's
///     Step 5 actually recommends). Spawn `ollama serve` detached on the
///     same port 11434 the `.app` would use, so `check_ollama_running`
///     picks it up within the next 2-second poll tick.
///
/// Previously this was hard-coded to `open -a Ollama`, which silently
/// failed for CLI-only installs and left the setup-guide "Open Ollama"
/// button doing nothing.
#[tauri::command]
pub fn open_ollama() {
    if std::path::Path::new("/Applications/Ollama.app").exists() {
        let _ = std::process::Command::new("open")
            .arg("-a")
            .arg("Ollama")
            .spawn();
        return;
    }
    const CLI_CANDIDATES: &[&str] = &["/opt/homebrew/bin/ollama", "/usr/local/bin/ollama"];
    if let Some(bin) = CLI_CANDIDATES
        .iter()
        .find(|p| std::path::Path::new(p).exists())
    {
        // Detach stdio so the daemon survives Soll quitting and doesn't
        // spam the dev console with Ollama's startup logs.
        let _ = std::process::Command::new(bin)
            .arg("serve")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
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

