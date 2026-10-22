//! The tray icon and its menu, kept in step with the runtime's status.

use std::thread;
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::core::{Core, StatusView};

/// How often the tray re-reads the status.
const POLL: Duration = Duration::from_millis(300);

const ICON_ACTIVE: &[u8] = include_bytes!("../icons/tray-active.png");
const ICON_PAUSED: &[u8] = include_bytes!("../icons/tray-paused.png");

/// What the tray shows; recomputed from each status poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayView {
    /// Show the paused icon.
    pub paused: bool,
    /// Label of the pause/resume item.
    pub toggle_label: &'static str,
    /// Label of the (disabled) layout pair item.
    pub pair_label: String,
    /// Tooltip.
    pub tooltip: String,
}

impl TrayView {
    /// The tray's look for a status.
    pub fn from_status(s: &StatusView) -> Self {
        let pair_label = match &s.layout_pair {
            Some((a, b)) => format!("Layouts: {a} \u{21c4} {b}"),
            None => "Layouts: not set".into(),
        };
        let state = if !s.running {
            "not running"
        } else if s.paused {
            "paused"
        } else if s.focused_app_blocked {
            "off in this app"
        } else {
            "on"
        };
        let mut tooltip = format!("Layout Fixer: {state}");
        if s.preview {
            tooltip.push_str(" (preview)");
        }
        TrayView {
            paused: s.paused || !s.running,
            toggle_label: if s.paused { "Resume" } else { "Pause" },
            pair_label,
            tooltip,
        }
    }
}

/// Build the tray and start polling the status.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Pause", true, None::<&str>)?;
    let pair = MenuItem::with_id(app, "pair", "Layouts", false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings\u{2026}", true, None::<&str>)?;
    let privacy = MenuItem::with_id(app, "privacy", "What this app can see", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &pair,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &privacy,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let active = Image::from_bytes(ICON_ACTIVE)?;
    let paused = Image::from_bytes(ICON_PAUSED)?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(active.clone())
        .tooltip("Layout Fixer")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => {
                app.state::<Core>().toggle_pause();
            }
            "settings" => crate::show_settings(app, None),
            "privacy" => crate::show_settings(app, Some("privacy")),
            "quit" => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_settings(tray.app_handle(), None);
            }
        })
        .build(app)?;

    let app = app.clone();
    thread::Builder::new()
        .name("lf-tray".into())
        .spawn(move || {
            let mut shown: Option<TrayView> = None;
            loop {
                let view = TrayView::from_status(&app.state::<Core>().status());
                if shown.as_ref() != Some(&view) {
                    let icon = if view.paused { &paused } else { &active };
                    let _ = tray.set_icon(Some(icon.clone()));
                    let _ = tray.set_tooltip(Some(&view.tooltip));
                    let _ = toggle.set_text(view.toggle_label);
                    let _ = pair.set_text(&view.pair_label);
                    shown = Some(view);
                }
                thread::sleep(POLL);
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_view_follows_status() {
        let mut s = StatusView {
            running: true,
            layout_pair: Some(("us".into(), "ir".into())),
            ..StatusView::default()
        };
        let v = TrayView::from_status(&s);
        assert_eq!((v.paused, v.toggle_label), (false, "Pause"));
        assert_eq!(v.pair_label, "Layouts: us \u{21c4} ir");
        assert_eq!(v.tooltip, "Layout Fixer: on");

        s.paused = true;
        s.preview = true;
        let v = TrayView::from_status(&s);
        assert_eq!((v.paused, v.toggle_label), (true, "Resume"));
        assert_eq!(v.tooltip, "Layout Fixer: paused (preview)");

        let v = TrayView::from_status(&StatusView::default());
        assert!(v.paused);
        assert_eq!(v.pair_label, "Layouts: not set");
        assert_eq!(v.tooltip, "Layout Fixer: not running");
    }
}
