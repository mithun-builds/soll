use anyhow::{anyhow, Result};
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry,
};

use crate::model::WhisperModel;
use crate::state::AppState;

const TRAY_ID: &str = "soll-tray";
const WORKING_BLINK_MS: u64 = 500;
const TRANSCRIBING_BLINK_MS: u64 = 400;
const DONE_REVERT_MS: u64 = 900;

static IMG_BLUE: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_blue.png")).unwrap());
static IMG_BLUE_DIM: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_blue_dim.png")).unwrap());
static IMG_YELLOW: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_yellow.png")).unwrap());
static IMG_RED: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_red.png")).unwrap());
static IMG_RED_DIM: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_red_dim.png")).unwrap());
static IMG_GREEN: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_green.png")).unwrap());

static EPOCH: AtomicU64 = AtomicU64::new(0);

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
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(IMG_BLUE.clone())
        .icon_as_template(false)
        .tooltip(TrayState::Loading.tooltip())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                "quit" => app.exit(0),
                "settings" => open_settings_window(app),
                _ => {}
            }
        })
        .build(app)?;

    set_state(app, TrayState::Loading);
    Ok(())
}

pub fn set_state(app: &AppHandle, state: TrayState) {
    let my_epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(state.tooltip()));
    }
    set_status_text(state.status_text());

    match state {
        TrayState::Idle => set_icon(app, IMG_YELLOW.clone()),
        TrayState::Transcribed => {
            set_icon(app, IMG_GREEN.clone());
            schedule_revert(app.clone(), my_epoch);
        }
        TrayState::Transcribing => {
            set_icon(app, IMG_RED.clone());
            start_blink(
                app.clone(),
                my_epoch,
                IMG_RED.clone(),
                IMG_RED_DIM.clone(),
                TRANSCRIBING_BLINK_MS,
            );
        }
        TrayState::Loading | TrayState::Initializing | TrayState::Processing => {
            set_icon(app, IMG_BLUE.clone());
            start_blink(
                app.clone(),
                my_epoch,
                IMG_BLUE.clone(),
                IMG_BLUE_DIM.clone(),
                WORKING_BLINK_MS,
            );
        }
    }
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

/// Rebuild the entire tray menu — used after a download finishes so the
/// completed model moves from the Download section into the main list.
pub fn refresh_menu(app: &AppHandle) {
    match build_menu(app) {
        Ok(menu) => {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_menu(Some(menu));
            }
        }
        Err(e) => log::error!("refresh_menu: {e:?}"),
    }
}

/// Toggle which cached model carries the checkmark. Call after every swap.
pub fn update_model_check(current: WhisperModel) {
    let items = CACHED_ITEMS.lock();
    for (m, item) in items.iter() {
        let _ = item.set_checked(*m == current);
    }
}

pub fn open_dictionary_window(app: &AppHandle) {
    open_window(app, "dictionary", "Soll — Dictionary", 520.0, 680.0);
}

pub fn open_legend_window(app: &AppHandle) {
    open_window(app, "legend", "Soll — Status Legend", 460.0, 560.0);
}

pub fn open_settings_window(app: &AppHandle) {
    open_window(app, "settings", "Soll — Settings", 860.0, 640.0);
}

/// Public for commands that need to refresh the tray after model state
/// changes. Settings-window initiated downloads use this to flip UI
/// indicators without going through the tray submenu.
pub fn refresh_settings_ui(_app: &AppHandle) {
    // Placeholder for future tauri event emission ("models_changed").
    // Settings window currently polls via settings_get / models_list.
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
    let quit = MenuItem::with_id(app, "quit", "Quit Soll", true, Some("Cmd+Q"))?;

    Menu::with_items(
        app,
        &[
            &status_item,
            &hotkey_item,
            &sep,
            &settings,
            &sep,
            &quit,
        ],
    )
    .map_err(Into::into)
}

fn build_model_submenu(app: &AppHandle) -> Result<Submenu<Wry>> {
    let state = app
        .try_state::<Arc<AppState>>()
        .ok_or_else(|| anyhow!("AppState not managed yet"))?;
    let state = state.inner();
    let current = state.current_model();
    let downloading = *state.downloading.lock();

    let mut cached_map: HashMap<WhisperModel, CheckMenuItem<Wry>> = HashMap::new();
    let mut download_map: HashMap<WhisperModel, MenuItem<Wry>> = HashMap::new();
    let mut builder = SubmenuBuilder::new(app, "Whisper model");

    // Section 1: cached/available models. Only these are selectable as the
    // active model. Radio checkmark on the currently-loaded one.
    let mut has_cached = false;
    for &m in WhisperModel::ALL {
        if state.is_model_cached(m) {
            has_cached = true;
            let label = format!("{} ({})", m.short_name(), m.size_label());
            let item = CheckMenuItem::with_id(
                app,
                format!("model.{}", m.id()),
                &label,
                true,
                m == current,
                None::<&str>,
            )?;
            builder = builder.item(&item);
            cached_map.insert(m, item);
        }
    }

    // Section 2: models available for download. Clickable entries that
    // trigger a confirmation dialog before starting the fetch.
    let uncached: Vec<WhisperModel> = WhisperModel::ALL
        .iter()
        .copied()
        .filter(|m| !state.is_model_cached(*m))
        .collect();
    if !uncached.is_empty() {
        if has_cached {
            let sep = PredefinedMenuItem::separator(app)?;
            builder = builder.item(&sep);
        }
        for m in uncached {
            let label = if downloading == Some(m) {
                format!("{} — Downloading… ({})", m.short_name(), m.size_label())
            } else {
                format!("{} — Download ({})", m.short_name(), m.size_label())
            };
            let item = MenuItem::with_id(
                app,
                format!("download.{}", m.id()),
                &label,
                true,
                None::<&str>,
            )?;
            builder = builder.item(&item);
            download_map.insert(m, item);
        }
    }

    let submenu = builder.build()?;
    *CACHED_ITEMS.lock() = cached_map;
    *DOWNLOAD_ITEMS.lock() = download_map;
    Ok(submenu)
}

// ── click handlers ─────────────────────────────────────────────────────────

fn parse_model_id(raw: &str, prefix: &str) -> Option<WhisperModel> {
    raw.strip_prefix(prefix).and_then(WhisperModel::from_id)
}

fn handle_active_model_click(app: &AppHandle, model: WhisperModel) {
    let state = match app.try_state::<Arc<AppState>>() {
        Some(s) => s.inner().clone(),
        None => return,
    };
    // Radio invariant — ensure exactly one cached item stays checked.
    update_model_check(model);

    if state.current_model() == model {
        return;
    }
    log::info!("tray: activate cached {}", model.id());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = state.clone().switch_model(model).await {
            log::error!("switch_model({}) failed: {e:?}", model.id());
            update_model_check(state.current_model());
        }
    });
}

fn handle_download_click(app: &AppHandle, model: WhisperModel) {
    let state = match app.try_state::<Arc<AppState>>() {
        Some(s) => s.inner().clone(),
        None => return,
    };
    // Already downloading this model — nothing to do.
    if *state.downloading.lock() == Some(model) {
        log::info!("tray: download.{} — already running", model.id());
        return;
    }
    // Raced with completion: refresh menu so it moves to the cached
    // section and the user can click to activate.
    if state.is_model_cached(model) {
        refresh_menu(app);
        return;
    }
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let confirmed = confirm_download_dialog(model).await;
        if !confirmed {
            log::info!("tray: download.{} cancelled by user", model.id());
            return;
        }
        if let Err(e) = state.start_download(model).await {
            log::error!("start_download({}): {e:?}", model.id());
        }
        // Refresh menu when download completes (success or failure) so
        // the UI accurately reflects on-disk state.
        refresh_menu(&app_clone);
    });
}

async fn confirm_download_dialog(model: WhisperModel) -> bool {
    let message = format!(
        "Download the {} Whisper model?\\n\\nSize: {}. The download runs in the background — Soll keeps working on your current model while it fetches.",
        model.display_name(),
        model.size_label()
    );
    let script = format!(
        "display dialog \"{}\" buttons {{\"Cancel\", \"Download\"}} default button \"Download\" with title \"Soll\"",
        message.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let output = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).contains("Download")
        }
        _ => false,
    }
}

// ── internal tray helpers ──────────────────────────────────────────────────

fn set_icon(app: &AppHandle, image: Image<'static>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(image));
    }
}

fn start_blink(
    app: AppHandle,
    epoch: u64,
    a: Image<'static>,
    b: Image<'static>,
    period_ms: u64,
) {
    tauri::async_runtime::spawn(async move {
        let mut toggle = false;
        loop {
            tokio::time::sleep(Duration::from_millis(period_ms)).await;
            if EPOCH.load(Ordering::SeqCst) != epoch {
                return;
            }
            let img = if toggle { b.clone() } else { a.clone() };
            set_icon(&app, img);
            toggle = !toggle;
        }
    });
}

fn schedule_revert(app: AppHandle, epoch: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(DONE_REVERT_MS)).await;
        if EPOCH.load(Ordering::SeqCst) != epoch {
            return;
        }
        EPOCH.fetch_add(1, Ordering::SeqCst);
        set_icon(&app, IMG_YELLOW.clone());
        set_status_text(TrayState::Idle.status_text());
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(TrayState::Idle.tooltip()));
        }
    });
}

fn open_window(app: &AppHandle, label: &str, title: &str, w: f64, h: f64) {
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
        Ok(_) => log::info!("opened {label} window"),
        Err(e) => log::error!("open {label} window: {e:?}"),
    }
}
