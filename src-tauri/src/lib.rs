mod audio;
mod commands;
mod onboarding;
mod overlay;
mod paste;
mod pipeline;
mod settings;
mod state;
mod tray;

// Exposed for the benchmark harness (`examples/bench_pipeline.rs`).
// Everything under these modules is re-entered by the bench, so it
// must run the same code paths production does.
pub mod cleanup;
pub mod corrections;
pub mod dictionary;
pub mod email;
pub mod formatter;
pub mod metal;
pub mod model;
pub mod skills;
pub mod transcribe;

use std::sync::Arc;

use log::{error, info};
use tauri::Manager;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};

use crate::state::AppState;
use crate::tray::TrayState;

fn push_to_talk_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)
}

pub fn run() {
    // Must happen before whisper-rs spins up its Metal context.
    metal::ensure_metal_resources();

    let ptt = push_to_talk_shortcut();
    let ptt_for_handler = ptt.clone();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::dict_list,
            commands::dict_add,
            commands::dict_remove,
            commands::settings_get,
            commands::settings_set,
            commands::skill_list,
            commands::skill_get_source,
            commands::skill_create,
            commands::skill_save,
            commands::skill_delete,
            commands::skill_set_enabled,
            commands::models_list,
            commands::model_activate,
            commands::model_download,
            commands::ollama_models_list,
            commands::ollama_model_set,
            commands::open_settings_window_cmd,
            commands::open_privacy_settings,
            commands::close_onboarding_window,
            onboarding::onboarding_status,
            onboarding::onboarding_dismiss,
        ])
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if shortcut != &ptt_for_handler {
                        return;
                    }
                    let state = match app.try_state::<Arc<AppState>>() {
                        Some(s) => s.inner().clone(),
                        None => return,
                    };
                    match event.state() {
                        ShortcutState::Pressed => {
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = state.on_press().await {
                                    error!("on_press: {e:?}");
                                }
                            });
                        }
                        ShortcutState::Released => {
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = state.on_release().await {
                                    error!("on_release: {e:?}");
                                }
                            });
                        }
                    }
                })
                .build(),
        )
        .setup(move |app| {
            // macOS: run as a background/menu-bar app — no Dock icon. Tray is the only UI.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // One-time data directory migration from com.svara.app → com.soll.app.
            // Runs on the first launch after the rename, becomes a no-op thereafter.
            if let Some(base) = dirs::data_dir() {
                let old = base.join("com.svara.app");
                let new = base.join("com.soll.app");
                if old.exists() && !new.exists() {
                    match std::fs::rename(&old, &new) {
                        Ok(()) => info!("migrated data dir: com.svara.app → com.soll.app"),
                        Err(e) => log::warn!("data dir migration failed: {e:?}"),
                    }
                }
            }

            let state = Arc::new(AppState::new(app.handle().clone()));
            app.manage(state.clone());

            tray::build_tray(app.handle())?;
            overlay::build(app.handle())?;
            app.global_shortcut().register(ptt.clone())?;
            info!("registered push-to-talk: Ctrl+Shift+Space");

            let st = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = st.warm_up().await {
                    error!("warm-up failed: {e:?}");
                    tray::set_state(&st.app, TrayState::Idle);
                }
            });

            // Always open the onboarding window on startup so it can be reviewed.
            // TODO: restore first-launch-only guard before shipping:
            //   only open when KEY_ONBOARDING_DISMISSED != "true"
            tray::open_onboarding_window(app.handle());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Soll")
        .run(|_app_handle, event| {
            // Tauri's default is to exit when the last window closes, but
            // Soll is a tray app — the only legitimate quit path is the
            // tray's "Quit Soll" menu item (which calls `app.exit(0)` and
            // sets `code = Some(0)`). Any other ExitRequested means a window
            // just got closed; keep the app alive.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
