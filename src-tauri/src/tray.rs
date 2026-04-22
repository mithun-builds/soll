use anyhow::Result;
use once_cell::sync::{Lazy, OnceCell};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry, WebviewUrl, WebviewWindowBuilder,
};

const TRAY_ID: &str = "svara-tray";
/// Every "working" state (Loading / Initializing / Processing) uses the same
/// blue blink cadence. The user only needs to distinguish "wait" from "speak".
const WORKING_BLINK_MS: u64 = 500;
const TRANSCRIBING_BLINK_MS: u64 = 400;
const DONE_REVERT_MS: u64 = 900;

// Four-color palette: blue (working), yellow (idle), red (speak), green (done).
// Gray/orange icons removed from the state machine — unused now.
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

/// Reference to the first (non-interactive) menu item, which we rewrite
/// every state change so users reading the menu see the live status.
static STATUS_ITEM: OnceCell<MenuItem<Wry>> = OnceCell::new();

#[derive(Copy, Clone, Debug)]
pub enum TrayState {
    /// App is still booting (downloading/loading model, compiling Metal kernels).
    Loading,
    /// Ready for the next dictation. The resting state.
    Idle,
    /// Hotkey pressed; mic is warming up. User should wait for next state.
    Initializing,
    /// Mic is live and capturing. User should be speaking now.
    Transcribing,
    /// Hotkey released; running whisper + optional AI cleanup.
    Processing,
    /// Text just pasted. Brief green flash then reverts to Idle.
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
            TrayState::Loading => "Svara — loading",
            TrayState::Idle => "Svara — hold ⌃⇧Space",
            TrayState::Initializing => "Svara — initializing…",
            TrayState::Transcribing => "Svara — speak now",
            TrayState::Processing => "Svara — processing",
            TrayState::Transcribed => "Svara — transcribed ✓",
        }
    }
}

pub fn build_tray(app: &AppHandle) -> Result<()> {
    let status_item = MenuItem::with_id(
        app,
        "status",
        TrayState::Loading.status_text(),
        false,
        None::<&str>,
    )?;
    let _ = STATUS_ITEM.set(status_item.clone());
    let initial_icon = IMG_BLUE.clone();

    let hotkey_item = MenuItem::with_id(
        app,
        "hotkey_info",
        "Hold ⌃⇧Space to dictate",
        false,
        None::<&str>,
    )?;
    let dictionary = MenuItem::with_id(app, "dictionary", "Dictionary…", true, None::<&str>)?;
    let legend = MenuItem::with_id(app, "legend", "Status Legend…", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Svara", true, Some("Cmd+Q"))?;

    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &hotkey_item,
            &sep,
            &dictionary,
            &legend,
            &sep,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(initial_icon)
        .icon_as_template(false)
        .tooltip(TrayState::Loading.tooltip())
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "dictionary" => open_dictionary_window(app),
            "legend" => open_legend_window(app),
            _ => {}
        })
        .build(app)?;

    set_state(app, TrayState::Loading);
    Ok(())
}

pub fn open_dictionary_window(app: &AppHandle) {
    const LABEL: &str = "dictionary";
    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    let url = WebviewUrl::App("index.html?view=dictionary".into());
    match WebviewWindowBuilder::new(app, LABEL, url)
        .title("Svara — Dictionary")
        .inner_size(520.0, 680.0)
        .min_inner_size(420.0, 480.0)
        .resizable(true)
        .build()
    {
        Ok(_) => log::info!("opened dictionary window"),
        Err(e) => log::error!("open dictionary window: {e:?}"),
    }
}

pub fn open_legend_window(app: &AppHandle) {
    const LABEL: &str = "legend";
    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    let url = WebviewUrl::App("index.html?view=legend".into());
    match WebviewWindowBuilder::new(app, LABEL, url)
        .title("Svara — Status Legend")
        .inner_size(460.0, 560.0)
        .min_inner_size(420.0, 480.0)
        .resizable(true)
        .build()
    {
        Ok(_) => log::info!("opened legend window"),
        Err(e) => log::error!("open legend window: {e:?}"),
    }
}

pub fn set_state(app: &AppHandle, state: TrayState) {
    let my_epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(state.tooltip()));
    }
    if let Some(item) = STATUS_ITEM.get() {
        let _ = item.set_text(state.status_text());
    }

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
        // All three "working" variants share the same blue blink so the
        // user sees a single consistent "wait" signal. The text in the
        // tray menu still distinguishes them for anyone who wants detail.
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
        if let Some(item) = STATUS_ITEM.get() {
            let _ = item.set_text(TrayState::Idle.status_text());
        }
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(TrayState::Idle.tooltip()));
        }
    });
}
