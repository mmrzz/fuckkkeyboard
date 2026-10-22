//! Layout Fixer's tray app: a tray icon, a settings window and the shared
//! runtime, in one process.
//!
//! The backend is picked by target OS: Windows gets the real
//! `WindowsBackend`; other systems run the mock backend and the UI shows a
//! "preview" banner until their backends land.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod autostart;
mod commands;
mod config;
mod core;
mod tray;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::config::ConfigStore;
use crate::core::{BackendFactory, Core};

/// The settings window's label in `tauri.conf.json`.
const MAIN_WINDOW: &str = "main";

/// The backend for this OS, and whether it is the preview (mock) one.
#[cfg(target_os = "windows")]
fn backend() -> (BackendFactory, bool) {
    use lf_platform::windows::WindowsBackend;
    (Box::new(|| Box::new(WindowsBackend::new())), false)
}

/// The backend for this OS, and whether it is the preview (mock) one.
#[cfg(not(target_os = "windows"))]
fn backend() -> (BackendFactory, bool) {
    let mock = lf_platform::MockBackend::us_ir();
    (Box::new(move || Box::new(mock.clone())), true)
}

/// Show and focus the settings window, optionally on a given page.
pub(crate) fn show_settings(app: &AppHandle, page: Option<&str>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(page) = page {
        let _ = window.emit("show-page", page);
    }
}

/// Stop the runtime and exit.
pub(crate) fn quit(app: &AppHandle) {
    app.state::<Core>().shutdown();
    app.exit(0);
}

fn main() {
    let (factory, preview) = backend();
    tauri::Builder::default()
        // Must be first: a second launch just shows the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_settings(app, None);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![autostart::AUTOSTART_ARG]),
        ))
        .setup(move |app| {
            let core = Core::new(factory, ConfigStore::platform(), preview);
            let start_at_login = core.config().start_at_login;
            app.manage(core);
            autostart::sync(app.handle(), start_at_login);
            tray::install(app.handle())?;
            if !autostart::launched_at_login() {
                show_settings(app.handle(), None);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window keeps the app in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == MAIN_WINDOW {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::get_config,
            commands::set_config,
            commands::pause,
            commands::resume,
            commands::layouts,
            commands::recent_apps,
            commands::trigger_preview,
            commands::dismiss_notice,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Layout Fixer");
}
