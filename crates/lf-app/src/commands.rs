//! Tauri commands: the settings window's only way to reach the runtime.
//!
//! Each command is a thin wrapper over [`Core`]; the window never sees
//! keystrokes, only counts, layout ids and app ids.

use tauri::{AppHandle, State};

use crate::config::AppConfig;
use crate::core::{Core, LayoutView, StatusView, TriggerView};

/// What the runtime is doing.
#[tauri::command]
pub fn status(core: State<'_, Core>) -> StatusView {
    core.status()
}

/// The current settings.
#[tauri::command]
pub fn get_config(core: State<'_, Core>) -> AppConfig {
    core.config()
}

/// Apply and save new settings, and follow `start_at_login`.
#[tauri::command]
pub fn set_config(app: AppHandle, core: State<'_, Core>, config: AppConfig) -> Result<(), String> {
    let before = core.config().start_at_login;
    let wanted = config.start_at_login;
    core.set_config(config)?;
    if before != wanted {
        crate::autostart::apply(&app, wanted)?;
    }
    Ok(())
}

/// Pause fixing.
#[tauri::command]
pub fn pause(core: State<'_, Core>) {
    core.pause();
}

/// Resume fixing.
#[tauri::command]
pub fn resume(core: State<'_, Core>) {
    core.resume();
}

/// Installed layouts.
#[tauri::command]
pub fn layouts(core: State<'_, Core>) -> Vec<LayoutView> {
    core.layouts()
}

/// Recently focused apps, for adding to the blocklist.
#[tauri::command]
pub fn recent_apps(core: State<'_, Core>) -> Vec<String> {
    core.recent_apps()
}

/// How a trigger looks in each layout.
#[tauri::command]
pub fn trigger_preview(core: State<'_, Core>, keys: Vec<String>) -> Vec<TriggerView> {
    core.trigger_preview(&keys)
}

/// The user has read the notice.
#[tauri::command]
pub fn dismiss_notice(core: State<'_, Core>) {
    core.dismiss_notice();
}
