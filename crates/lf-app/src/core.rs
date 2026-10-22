//! The app's logic, independent of Tauri: owns the runtime and the
//! settings, and answers the settings window's commands.
//!
//! Privacy: nothing here returns, stores or logs typed text. Status carries
//! counts, layout ids and app ids; the trigger preview renders only the
//! trigger keys.

use std::sync::{Mutex, MutexGuard};

use lf_core::{render, KeyCode, KeyPress, LayoutId, Modifiers};
use lf_platform::Backend;
use lf_runtime::{RuntimeError, RuntimeHandle};
use serde::Serialize;

use crate::config::{AppConfig, ConfigStore};

/// Makes a fresh backend (the runtime consumes one each time it starts).
pub type BackendFactory = Box<dyn Fn() -> Box<dyn Backend> + Send + Sync>;

/// What the settings window and tray show.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StatusView {
    /// The runtime is running.
    pub running: bool,
    /// Fixing is paused.
    pub paused: bool,
    /// The active layout id.
    pub current_layout: Option<String>,
    /// The layout pair in use.
    pub layout_pair: Option<(String, String)>,
    /// The focused app is on the blocklist.
    pub focused_app_blocked: bool,
    /// Fixes so far (count only).
    pub fixes: u64,
    /// Undos so far (count only).
    pub undos: u64,
    /// The most recent error.
    pub last_error: Option<String>,
    /// Running on the preview backend: no real input is seen.
    pub preview: bool,
    /// A one-off message for the user (e.g. settings were reset).
    pub notice: Option<String>,
}

/// One installed layout, for the layout-pair pickers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayoutView {
    /// Layout id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Two-letter language code, if known.
    pub language: Option<String>,
}

/// How the trigger looks when typed in one layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriggerView {
    /// Layout id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The characters the trigger keys type in that layout.
    pub text: String,
}

/// The app's state.
pub struct Core {
    factory: BackendFactory,
    store: Option<ConfigStore>,
    preview: bool,
    runtime: Mutex<Option<RuntimeHandle>>,
    config: Mutex<AppConfig>,
    notice: Mutex<Option<String>>,
    start_error: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Core {
    /// Load the settings and start the runtime. Never fails: problems are
    /// reported through [`Core::status`].
    pub fn new(factory: BackendFactory, store: Option<ConfigStore>, preview: bool) -> Self {
        let (config, notice) = match &store {
            Some(s) => {
                let loaded = s.load();
                (loaded.config, loaded.notice)
            }
            None => (
                AppConfig::default(),
                Some("No settings folder was found; settings will not be saved.".into()),
            ),
        };
        let core = Core {
            factory,
            store,
            preview,
            runtime: Mutex::new(None),
            config: Mutex::new(config),
            notice: Mutex::new(notice),
            start_error: Mutex::new(None),
        };
        core.start_runtime();
        core
    }

    /// Start the runtime with the current settings. If the saved layout
    /// pair is no longer installed, fall back to the automatic pair.
    fn start_runtime(&self) {
        let config = lock(&self.config).clone();
        let runtime_config = match config.to_runtime() {
            Ok(c) => c,
            Err(e) => {
                *lock(&self.start_error) = Some(e.to_string());
                return;
            }
        };
        let result = lf_runtime::start((self.factory)(), runtime_config.clone()).or_else(|e| {
            if runtime_config.layout_pair.is_none() || !is_layout_problem(&e) {
                return Err(e);
            }
            let auto = lf_runtime::RuntimeConfig {
                layout_pair: None,
                ..runtime_config
            };
            let handle = lf_runtime::start((self.factory)(), auto)?;
            self.set_notice(format!(
                "The saved layout pair is not available ({e}); using the first two installed languages."
            ));
            Ok(handle)
        });
        match result {
            Ok(h) => {
                *lock(&self.runtime) = Some(h);
                *lock(&self.start_error) = None;
            }
            Err(e) => *lock(&self.start_error) = Some(e.to_string()),
        }
    }

    fn set_notice(&self, notice: String) {
        *lock(&self.notice) = Some(notice);
    }

    /// The current status.
    pub fn status(&self) -> StatusView {
        let runtime = lock(&self.runtime);
        let mut view = match runtime.as_ref() {
            Some(h) => {
                let s = h.status();
                StatusView {
                    running: s.running,
                    paused: s.paused,
                    current_layout: s.current_layout.map(|l| l.0),
                    layout_pair: s.layout_pair.map(|(a, b)| (a.0, b.0)),
                    focused_app_blocked: s.focused_app_blocked,
                    fixes: s.fixes,
                    undos: s.undos,
                    last_error: s.last_error,
                    ..StatusView::default()
                }
            }
            None => StatusView {
                last_error: lock(&self.start_error).clone(),
                ..StatusView::default()
            },
        };
        view.preview = self.preview;
        view.notice = lock(&self.notice).clone();
        view
    }

    /// Clear the notice once the user has seen it.
    pub fn dismiss_notice(&self) {
        *lock(&self.notice) = None;
    }

    /// Pause fixing.
    pub fn pause(&self) {
        if let Some(h) = lock(&self.runtime).as_ref() {
            h.pause();
        }
    }

    /// Resume fixing.
    pub fn resume(&self) {
        if let Some(h) = lock(&self.runtime).as_ref() {
            h.resume();
        }
    }

    /// Flip between paused and running; returns the new paused state.
    pub fn toggle_pause(&self) -> bool {
        let paused = self.status().paused;
        if paused {
            self.resume();
        } else {
            self.pause();
        }
        !paused
    }

    /// The current settings.
    pub fn config(&self) -> AppConfig {
        lock(&self.config).clone()
    }

    /// Validate, apply and save new settings. On error nothing changes.
    pub fn set_config(&self, new: AppConfig) -> Result<(), String> {
        let new = new.normalized();
        let runtime_config = new.to_runtime().map_err(|e| e.to_string())?;
        {
            let runtime = lock(&self.runtime);
            if let Some(h) = runtime.as_ref() {
                h.set_config(runtime_config).map_err(|e| e.to_string())?;
            }
        }
        *lock(&self.config) = new.clone();
        if lock(&self.runtime).is_none() {
            self.start_runtime();
        }
        if let Some(store) = &self.store {
            store
                .save(&new)
                .map_err(|e| format!("settings applied but not saved: {e}"))?;
        }
        Ok(())
    }

    /// Installed layouts.
    pub fn layouts(&self) -> Vec<LayoutView> {
        if let Some(h) = lock(&self.runtime).as_ref() {
            return h
                .layouts()
                .into_iter()
                .map(|(id, name, language)| LayoutView {
                    id: id.0,
                    name,
                    language,
                })
                .collect();
        }
        (self.factory)()
            .layouts()
            .unwrap_or_default()
            .into_iter()
            .map(|l| LayoutView {
                id: l.layout.id.0,
                name: l.layout.name,
                language: l.language,
            })
            .collect()
    }

    /// The last few distinct focused apps, most recent first.
    pub fn recent_apps(&self) -> Vec<String> {
        lock(&self.runtime)
            .as_ref()
            .map(|h| h.recent_apps().into_iter().map(|a| a.0).collect())
            .unwrap_or_default()
    }

    /// How a trigger (W3C key codes) looks typed in each installed layout.
    /// Unknown codes are skipped.
    pub fn trigger_preview(&self, keys: &[String]) -> Vec<TriggerView> {
        let codes: Vec<KeyCode> = keys.iter().filter_map(|k| KeyCode::from_w3c(k)).collect();
        (self.factory)()
            .layouts()
            .unwrap_or_default()
            .into_iter()
            .map(|l| {
                let presses: Vec<KeyPress> = codes
                    .iter()
                    .map(|&code| KeyPress {
                        code,
                        mods: Modifiers::NONE,
                        layout: LayoutId::new(""),
                        produced: None,
                    })
                    .collect();
                TriggerView {
                    text: render(&presses, &l.layout),
                    id: l.layout.id.0,
                    name: l.layout.name,
                }
            })
            .collect()
    }

    /// Stop the runtime (on quit).
    pub fn shutdown(&self) {
        if let Some(h) = lock(&self.runtime).take() {
            h.stop();
        }
    }
}

fn is_layout_problem(e: &RuntimeError) -> bool {
    matches!(e, RuntimeError::Engine(_) | RuntimeError::NoLayoutPair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FILE_NAME;
    use lf_platform::{AppId, MockBackend, PlatformEvent};
    use std::time::{Duration, Instant};

    fn temp_store(name: &str) -> ConfigStore {
        let dir = std::env::temp_dir().join(format!("lf-core-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        ConfigStore::at(dir.join(FILE_NAME))
    }

    fn core_with(mock: &MockBackend, store: Option<ConfigStore>) -> Core {
        let m = mock.clone();
        Core::new(Box::new(move || Box::new(m.clone())), store, true)
    }

    fn wait_for(mut cond: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(2);
        while !cond() {
            assert!(Instant::now() < end, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn starts_on_the_mock_backend() {
        let mock = MockBackend::us_ir();
        let core = core_with(&mock, None);
        let s = core.status();
        assert!(s.running && s.preview && !s.paused);
        assert_eq!(s.layout_pair, Some(("us".into(), "ir".into())));
        assert!(s.notice.is_some(), "no store → notice");
        core.dismiss_notice();
        assert!(core.status().notice.is_none());
        let ids: Vec<_> = core.layouts().into_iter().map(|l| l.id).collect();
        assert_eq!(ids, ["us", "ir"]);
        core.shutdown();
        assert!(!core.status().running);
    }

    #[test]
    fn pause_and_resume_reach_the_runtime() {
        let mock = MockBackend::us_ir();
        let core = core_with(&mock, None);
        core.pause();
        wait_for(|| core.status().paused);
        assert!(!core.toggle_pause());
        wait_for(|| !core.status().paused);
        assert!(core.toggle_pause());
        wait_for(|| core.status().paused);
    }

    #[test]
    fn changing_the_pair_applies_and_saves() {
        let mock = MockBackend::us_ir();
        let store = temp_store("pair");
        let core = core_with(&mock, Some(store.clone()));
        let config = AppConfig {
            layout_pair: Some(("ir".into(), "us".into())),
            ..core.config()
        };
        core.set_config(config.clone()).unwrap();
        assert_eq!(core.status().layout_pair, Some(("ir".into(), "us".into())));
        assert_eq!(store.load().config, config);

        let bad = AppConfig {
            layout_pair: Some(("us".into(), "xx".into())),
            ..core.config()
        };
        assert!(core.set_config(bad).is_err());
        assert_eq!(core.config(), config, "a rejected config changes nothing");
        assert_eq!(store.load().config, config);
    }

    #[test]
    fn missing_saved_pair_falls_back_to_auto() {
        let mock = MockBackend::us_ir();
        let store = temp_store("fallback");
        store
            .save(&AppConfig {
                layout_pair: Some(("de".into(), "us".into())),
                ..AppConfig::default()
            })
            .unwrap();
        let core = core_with(&mock, Some(store));
        let s = core.status();
        assert!(s.running);
        assert_eq!(s.layout_pair, Some(("us".into(), "ir".into())));
        assert!(s.notice.unwrap().contains("not available"));
    }

    #[test]
    fn recent_apps_and_blocklist() {
        let mock = MockBackend::us_ir();
        let core = core_with(&mock, None);
        mock.push(PlatformEvent::FocusChanged(Some(AppId::new("notepad.exe"))));
        wait_for(|| core.recent_apps() == ["notepad.exe"]);
        let config = AppConfig {
            blocklist: vec!["NOTEPAD.EXE".into()],
            ..core.config()
        };
        core.set_config(config).unwrap();
        assert_eq!(core.config().blocklist, ["notepad.exe"]);
        assert!(core.status().focused_app_blocked);
    }

    #[test]
    fn trigger_preview_renders_each_layout() {
        let mock = MockBackend::us_ir();
        let core = core_with(&mock, None);
        let keys: Vec<String> = ["KeyF", "KeyU", "KeyC", "KeyK", "Bogus"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let views = core.trigger_preview(&keys);
        assert_eq!(views[0].text, "fuck");
        assert_eq!(views[1].text, "بعزن");
    }
}
