use anyhow::Result;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

const TRAY_ID: &str = "svara-tray";
const BLINK_MS: u64 = 400;
const LOADING_BLINK_MS: u64 = 800;
const DONE_REVERT_MS: u64 = 900;

static IMG_LOADING: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_loading.png")).unwrap());
static IMG_LOADING_DIM: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_loading_dim.png")).unwrap());
static IMG_YELLOW: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_yellow.png")).unwrap());
static IMG_RED: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_red.png")).unwrap());
static IMG_RED_DIM: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_red_dim.png")).unwrap());
static IMG_ORANGE: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_orange.png")).unwrap());
static IMG_ORANGE_DIM: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_orange_dim.png")).unwrap());
static IMG_GREEN: Lazy<Image<'static>> =
    Lazy::new(|| Image::from_bytes(include_bytes!("../icons/tray_green.png")).unwrap());

static EPOCH: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug)]
pub enum TrayState {
    Loading,
    Idle,
    Recording,
    Processing,
    Done,
}

pub fn build_tray(app: &AppHandle) -> Result<()> {
    let status_item =
        MenuItem::with_id(app, "status", "Svara — starting…", false, None::<&str>)?;
    let hotkey_item = MenuItem::with_id(
        app,
        "hotkey_info",
        "Hold ⌃⇧Space to dictate",
        false,
        None::<&str>,
    )?;
    let dictionary = MenuItem::with_id(app, "dictionary", "Dictionary…", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Svara", true, Some("Cmd+Q"))?;

    let menu = Menu::with_items(
        app,
        &[&status_item, &hotkey_item, &sep, &dictionary, &sep, &quit],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(IMG_LOADING.clone())
        .icon_as_template(false)
        .tooltip("Svara — starting…")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "dictionary" => open_dictionary_window(app),
            _ => {}
        })
        .build(app)?;

    set_state(app, TrayState::Loading);
    Ok(())
}

/// Open (or focus, if already open) the dictionary CRUD window.
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

pub fn set_state(app: &AppHandle, state: TrayState) {
    let my_epoch = EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    let tooltip = match state {
        TrayState::Loading => "Svara — loading model…",
        TrayState::Idle => "Svara — hold ⌃⇧Space",
        TrayState::Recording => "Svara — listening",
        TrayState::Processing => "Svara — polishing",
        TrayState::Done => "Svara — pasted",
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip));
    }

    match state {
        TrayState::Loading => {
            set_icon(app, IMG_LOADING.clone());
            start_blink(
                app.clone(),
                my_epoch,
                IMG_LOADING.clone(),
                IMG_LOADING_DIM.clone(),
                LOADING_BLINK_MS,
            );
        }
        TrayState::Idle => set_icon(app, IMG_YELLOW.clone()),
        TrayState::Recording => {
            set_icon(app, IMG_RED.clone());
            start_blink(
                app.clone(),
                my_epoch,
                IMG_RED.clone(),
                IMG_RED_DIM.clone(),
                BLINK_MS,
            );
        }
        TrayState::Processing => {
            set_icon(app, IMG_ORANGE.clone());
            start_blink(
                app.clone(),
                my_epoch,
                IMG_ORANGE.clone(),
                IMG_ORANGE_DIM.clone(),
                BLINK_MS,
            );
        }
        TrayState::Done => {
            set_icon(app, IMG_GREEN.clone());
            schedule_revert(app.clone(), my_epoch);
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
    });
}
