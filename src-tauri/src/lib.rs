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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{error, info};
use tauri::Manager;

/// Set by `restart_app` so the ExitRequested hook below doesn't veto the
/// shutdown that `app.restart()` triggers internally. Without this gate,
/// "Restart Soll" silently no-ops because we treat the close as just-another
/// window-close and prevent the exit.
pub(crate) static APP_RESTARTING: AtomicBool = AtomicBool::new(false);
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
            commands::model_select,
            commands::model_download,
            commands::model_cancel_download,
            commands::model_delete,
            commands::ollama_models_list,
            commands::ollama_model_set,
            commands::ollama_pull_active,
            commands::ollama_delete_active,
            commands::open_settings_window_cmd,
            commands::open_privacy_settings,
            commands::close_onboarding_window,
            commands::restart_app,
            commands::set_onboarding_indicator,
            onboarding::onboarding_status,
            onboarding::onboarding_dismiss,
            onboarding::request_mic_permission,
            onboarding::request_accessibility_permission,
            onboarding::open_ollama,
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

            // Onboarding indicator watcher.
            //
            // On launch: do one prereq check and open the Setup Guide if
            // incomplete (so first-time users see it). After that, the loop
            // only updates the tray indicator — it never reopens the window
            // on its own. If the user closes the guide, we respect that.
            // The guide reopens only when the user does something with
            // Soll — clicking the tray menu entry or hitting the PTT
            // shortcut (handled in `state::on_press`).
            let st_watcher = state.clone();
            let app_watcher = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use std::time::Duration;

                let initial = st_watcher.onboarding_complete().await;
                tray::set_setup_needed(&app_watcher, !initial);
                if !initial {
                    tray::open_onboarding_window(&app_watcher);
                }
                let mut prev = initial;

                loop {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let complete = st_watcher.onboarding_complete().await;
                    if complete != prev {
                        tray::set_setup_needed(&app_watcher, !complete);
                        prev = complete;
                    }
                }
            });

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
                // Let restart_app's shutdown through unconditionally, even
                // when Tauri reports `code = None`. Otherwise the user clicks
                // "Restart Soll" and nothing happens because we treat it as
                // just-another-window-close.
                if code.is_none() && !APP_RESTARTING.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
