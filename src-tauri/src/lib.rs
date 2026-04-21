mod audio;
mod cleanup;
mod model;
mod paste;
mod pipeline;
mod state;
mod transcribe;
mod tray;

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
    let ptt = push_to_talk_shortcut();
    let ptt_for_handler = ptt.clone();

    tauri::Builder::default()
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

            let state = Arc::new(AppState::new(app.handle().clone()));
            app.manage(state.clone());

            tray::build_tray(app.handle())?;
            app.global_shortcut().register(ptt.clone())?;
            info!("registered push-to-talk: Ctrl+Shift+Space");

            let st = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = st.warm_up().await {
                    error!("warm-up failed: {e:?}");
                    tray::set_state(&st.app, TrayState::Idle);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Svara");
}
