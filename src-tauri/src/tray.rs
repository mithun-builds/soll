use anyhow::Result;
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
};

use crate::model::WhisperModel;

const TRAY_ID: &str = "soll-tray";
const DONE_REVERT_MS: u64 = 900;
/// Skills show their name in the status line — give the user time to read it.
const SKILL_DONE_REVERT_MS: u64 = 2000;

/// Default — pure white wave mark, static, used for every state.
static IMG_WHITE: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_white.png")).unwrap());
/// Loading / Initializing — same white mark with a small red badge top-right.
static IMG_BADGE: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_badge.png")).unwrap());

static EPOCH: AtomicU64 = AtomicU64::new(0);

/// True while at least one onboarding step is unfinished. When true the tray
/// icon shows the red badge regardless of what TrayState the runtime sets.
static SETUP_NEEDED: AtomicBool = AtomicBool::new(false);

/// Most recent TrayState pushed via `set_state`. Tracked so `apply_icon` can
/// re-decide the icon when SETUP_NEEDED toggles independently of the state.
static CURRENT_STATE: Lazy<Mutex<TrayState>> = Lazy::new(|| Mutex::new(TrayState::Loading));

/// Live handle to the "Setup Guide…" menu item. Tracked so we can locate
/// it for removal when prereqs become complete (we want the entry hidden
/// rather than just relabelled while the user is fully set up).
static ONBOARDING_ITEM: Lazy<Mutex<Option<MenuItem<Wry>>>> = Lazy::new(|| Mutex::new(None));

/// The active tray menu, kept around so `set_setup_needed` can swap it for
/// a freshly-built one when the Setup Guide entry needs to appear/disappear.
static TRAY_MENU: Lazy<Mutex<Option<Menu<Wry>>>> = Lazy::new(|| Mutex::new(None));

/// Status line in the tray menu, rewritten on every state change.
static STATUS_ITEM: OnceCell<MenuItem<Wry>> = OnceCell::new();

/// Live handles to the downloadable model items so we can update their
/// label ("Downloading 37%…") without rebuilding the whole menu.
static DOWNLOAD_ITEMS: Lazy<Mutex<HashMap<WhisperModel, MenuItem<Wry>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// CheckMenuItems for cached models — used to keep the active model's
/// checkmark in sync with `current_model`.
static CACHED_ITEMS: Lazy<Mutex<HashMap<WhisperModel, CheckMenuItem<Wry>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Copy, Clone, Debug)]
pub enum TrayState {
    Loading,
    Idle,
    Initializing,
    Transcribing,
    Processing,
    Transcribed,
}

impl TrayState {
    fn status_text(&self) -> &'static str {
        match self {
            TrayState::Loading => "Loading…",
            TrayState::Idle => "Idle — hold ⌃⇧Space to dictate",
            TrayState::Initializing => "Initializing…",
            TrayState::Transcribing => "Transcribing — speak now",
            TrayState::Processing => "Processing…",
            TrayState::Transcribed => "Transcribed ✓",
        }
    }

    fn tooltip(&self) -> &'static str {
        match self {
            TrayState::Loading => "Soll — loading",
            TrayState::Idle => "Soll — hold ⌃⇧Space",
            TrayState::Initializing => "Soll — initializing…",
            TrayState::Transcribing => "Soll — speak now",
            TrayState::Processing => "Soll — processing",
            TrayState::Transcribed => "Soll — transcribed ✓",
        }
    }
}

// ── public tray API ────────────────────────────────────────────────────────

pub fn build_tray(app: &AppHandle) -> Result<()> {
    let menu = build_menu(app)?;
    *TRAY_MENU.lock() = Some(menu.clone());
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(IMG_BADGE.clone())
        .icon_as_template(false)
        .tooltip(TrayState::Loading.tooltip())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                "quit" => app.exit(0),
                "settings" => open_settings_window(app),
                "onboarding" => open_onboarding_window(app),
                _ => {}
            }
        })
        .build(app)?;

    set_state(app, TrayState::Loading);
    Ok(())
}

pub fn set_state(app: &AppHandle, state: TrayState) {
    *CURRENT_STATE.lock() = state;
    let my_epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(state.tooltip()));
    }
    set_status_text(state.status_text());

    apply_icon(app);
    if matches!(state, TrayState::Transcribed) {
        schedule_revert(app.clone(), my_epoch, DONE_REVERT_MS);
    }
}

/// Pick the right tray icon based on (a) the latest TrayState and (b) whether
/// onboarding is incomplete. The badged icon wins when either signal asks
/// for attention; otherwise plain white.
fn apply_icon(app: &AppHandle) {
    let state = *CURRENT_STATE.lock();
    let busy = matches!(state, TrayState::Loading | TrayState::Initializing);
    let needs_setup = SETUP_NEEDED.load(Ordering::SeqCst);
    let img = if busy || needs_setup {
        IMG_BADGE.clone()
    } else {
        IMG_WHITE.clone()
    };
    set_icon(app, img);
}

/// Flip the "user has unfinished onboarding" indicator. When `needed` is true
/// the tray icon shows a red badge and the menu shows a "🔴 Setup Guide…"
/// entry. When false the entry is removed entirely so the menu reads as
/// clean Settings → Quit. Called by the backend's prereq watcher whenever
/// the polled prerequisite state crosses the boundary.
pub fn set_setup_needed(app: &AppHandle, needed: bool) {
    let prev = SETUP_NEEDED.swap(needed, Ordering::SeqCst);
    if prev != needed {
        apply_icon(app);
        rebuild_menu(app);
    }
}

/// Rebuild the tray menu in place. Cheap operation but needs to happen on
/// the main thread (Tauri dispatches internally), so callable from any
/// async context safely.
fn rebuild_menu(app: &AppHandle) {
    match build_menu(app) {
        Ok(menu) => {
            *TRAY_MENU.lock() = Some(menu.clone());
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_menu(Some(menu));
            }
        }
        Err(e) => log::error!("rebuild_menu: {e:?}"),
    }
}

/// Show a skill-specific completion banner — "skill: commit ✓" — in the tray
/// status line, then revert to idle after a longer pause so the user can read
/// which skill fired and confirm it worked as expected.
pub fn set_skill_done(app: &AppHandle, skill_name: &str) {
    let my_epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    let status = format!("skill: {skill_name} ✓");
    let tooltip = format!("Soll — {skill_name} ✓");
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip.as_str()));
    }
    set_status_text(&status);
    set_icon(app, IMG_WHITE.clone());
    schedule_revert(app.clone(), my_epoch, SKILL_DONE_REVERT_MS);
}

/// Rewrite the first (non-interactive) menu item that shows the live state.
pub fn set_status_text(text: &str) {
    if let Some(item) = STATUS_ITEM.get() {
        let _ = item.set_text(text);
    }
}

/// Live-update the "Download…" menu item for a specific model during a
/// running download. Call with (done, total) in bytes; if total is 0 we
/// just show an indeterminate marker.
pub fn set_download_progress(model: WhisperModel, done: u64, total: u64) {
    let items = DOWNLOAD_ITEMS.lock();
    if let Some(item) = items.get(&model) {
        let label = if total == 0 {
            format!("{} — Downloading… ({})", model.short_name(), model.size_label())
        } else {
            let pct = done * 100 / total;
            format!(
                "{} — Downloading {}% ({})",
                model.short_name(),
                pct,
                model.size_label()
            )
        };
        let _ = item.set_text(&label);
    }
}

/// Toggle which cached model carries the checkmark. Call after every swap.
pub fn update_model_check(current: WhisperModel) {
    let items = CACHED_ITEMS.lock();
    for (m, item) in items.iter() {
        let _ = item.set_checked(*m == current);
    }
}

pub fn open_settings_window(app: &AppHandle) {
    open_window(app, "settings", "Soll — Settings", 860.0, 640.0);
}

pub fn open_onboarding_window(app: &AppHandle) {
    activate_app();
    if let Some(existing) = app.get_webview_window("onboarding") {
        let _ = existing.show();
        center_on_active_screen(&existing, 560.0, 680.0);
        let _ = existing.set_focus();
        return;
    }
    let url = WebviewUrl::App("index.html?view=onboarding".into());
    match WebviewWindowBuilder::new(app, "onboarding", url)
        .title("Soll — Setup Guide")
        .inner_size(560.0, 680.0)
        .min_inner_size(420.0, 500.0)
        .resizable(true)
        .build()
    {
        Ok(window) => {
            // Show first so the window is realised, *then* set position —
            // some macOS versions ignore set_position before the window has
            // been shown. We center on the monitor under the cursor (the
            // user's active screen) rather than the primary monitor.
            let _ = window.show();
            center_on_active_screen(&window, 560.0, 680.0);
            let _ = window.set_focus();
            log::info!("opened onboarding window");
        }
        Err(e) => log::error!("open onboarding window: {e:?}"),
    }
}

/// Centre the window on the monitor under the cursor (falling back to the
/// primary monitor). Logical-size aware: we know the window's logical size
/// from the builder — `outer_size()` is unreliable before the window is
/// fully realised — and convert to physical coords using the monitor's
/// scale factor so HiDPI screens don't end up off-centre.
fn center_on_active_screen(
    window: &tauri::WebviewWindow,
    logical_w: f64,
    logical_h: f64,
) {
    let app = window.app_handle();

    // Pick the monitor under the cursor. Falls back to primary if the
    // cursor is somehow off all monitors (rare, but happens during fast
    // monitor reconfiguration).
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let mpos = monitor.position();
    let msize = monitor.size();
    let scale = monitor.scale_factor();
    let win_phys_w = (logical_w * scale) as i32;
    let win_phys_h = (logical_h * scale) as i32;

    let x = mpos.x + ((msize.width as i32 - win_phys_w) / 2).max(0);
    let y = mpos.y + ((msize.height as i32 - win_phys_h) / 2).max(0);
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

// ── menu construction ──────────────────────────────────────────────────────

fn build_menu(app: &AppHandle) -> Result<Menu<Wry>> {
    let status_item = MenuItem::with_id(
        app,
        "status",
        TrayState::Loading.status_text(),
        false,
        None::<&str>,
    )?;
    let _ = STATUS_ITEM.set(status_item.clone());

    let hotkey_item = MenuItem::with_id(
        app,
        "hotkey_info",
        "Hold ⌃⇧Space to dictate",
        false,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Soll", true, Some("Cmd+Q"))?;

    // The Setup Guide entry only exists while onboarding is incomplete. Once
    // every prereq is satisfied it disappears from the menu entirely — no
    // greyed-out row, no clutter.
    let onboarding = if SETUP_NEEDED.load(Ordering::SeqCst) {
        let item = MenuItem::with_id(
            app,
            "onboarding",
            "🔴  Setup Guide…",
            true,
            None::<&str>,
        )?;
        *ONBOARDING_ITEM.lock() = Some(item.clone());
        Some(item)
    } else {
        *ONBOARDING_ITEM.lock() = None;
        None
    };

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = vec![
        &status_item,
        &hotkey_item,
        &sep,
        &settings,
    ];
    if let Some(ref ob) = onboarding {
        items.push(ob);
    }
    items.push(&sep2);
    items.push(&quit);

    Menu::with_items(app, &items).map_err(Into::into)
}

// ── internal tray helpers ──────────────────────────────────────────────────

fn set_icon(app: &AppHandle, image: Image<'static>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(image));
    }
}

fn schedule_revert(app: AppHandle, epoch: u64, after_ms: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(after_ms)).await;
        if EPOCH.load(Ordering::SeqCst) != epoch {
            return;
        }
        EPOCH.fetch_add(1, Ordering::SeqCst);
        set_icon(&app, IMG_WHITE.clone());
        set_status_text(TrayState::Idle.status_text());
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(TrayState::Idle.tooltip()));
        }
    });
}

fn open_window(app: &AppHandle, label: &str, title: &str, w: f64, h: f64) {
    activate_app();
    if let Some(existing) = app.get_webview_window(label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    let url = WebviewUrl::App(format!("index.html?view={label}").into());
    match WebviewWindowBuilder::new(app, label, url)
        .title(title)
        .inner_size(w, h)
        .min_inner_size(420.0, 480.0)
        .resizable(true)
        .build()
    {
        Ok(window) => {
            let _ = window.show();
            let _ = window.set_focus();
            log::info!("opened {label} window");
        }
        Err(e) => log::error!("open {label} window: {e:?}"),
    }
}

/// Bring Soll to the front. Accessory-mode apps (no Dock icon) don't get
/// activated by `set_focus()` alone — newly-shown windows end up *behind*
/// whatever app currently owns the menu bar. Calling
/// `NSApp.activateIgnoringOtherApps:YES` first promotes Soll to the
/// foreground for the duration of this window-open.
//
// `cocoa` is deprecated in favour of `objc2-app-kit` but we lean on it in
// just one spot here. Allow inline rather than migrate the whole module.
#[allow(deprecated)]
fn activate_app() {
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::appkit::NSApp;
        use objc::{msg_send, sel, sel_impl};
        let nsapp = NSApp();
        let _: () = msg_send![nsapp, activateIgnoringOtherApps: true];
    }
}
