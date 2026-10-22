//! Start at login, through `tauri-plugin-autostart`.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// The argument the login entry passes, so the app starts hidden.
pub const AUTOSTART_ARG: &str = "--autostart";

/// True if this process was started by the login entry.
pub fn launched_at_login() -> bool {
    std::env::args().any(|a| a == AUTOSTART_ARG)
}

/// Add or remove the login entry.
pub fn apply(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| format!("could not change start at login: {e}"))
}

/// Make the login entry match the settings (e.g. after the user removed
/// it by hand or moved the app).
pub fn sync(app: &AppHandle, enabled: bool) {
    let current = app.autolaunch().is_enabled().unwrap_or(false);
    if current != enabled {
        let _ = apply(app, enabled);
    }
}
