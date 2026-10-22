//! The settings file (`config.toml`) in the platform config directory.
//!
//! Holds the runtime settings plus `start_at_login`. Never holds typed text.
//! A file that cannot be read or fails validation is moved aside to
//! `config.toml.broken` and the app starts from defaults with a notice.

use std::fs;
use std::io;
use std::path::PathBuf;

use lf_core::{KeyCode, LayoutId, Trigger};
use lf_runtime::RuntimeConfig;
use serde::{Deserialize, Serialize};

/// The settings file name.
pub const FILE_NAME: &str = "config.toml";

/// Longest allowed undo window.
pub const MAX_UNDO_WINDOW_MS: u64 = 60_000;
/// Shortest and longest allowed idle reset.
pub const IDLE_RESET_RANGE_MS: (u64, u64) = (1_000, 600_000);

/// Apps ignored out of the box: password managers and terminals, where
/// replacing text is unwelcome or unsafe.
#[cfg(target_os = "windows")]
pub const DEFAULT_BLOCKLIST: &[&str] = &[
    "1password.exe",
    "bitwarden.exe",
    "keepass.exe",
    "keepassxc.exe",
    "cmd.exe",
    "conhost.exe",
    "openconsole.exe",
    "powershell.exe",
    "pwsh.exe",
    "windowsterminal.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "mintty.exe",
    "putty.exe",
];

/// Apps ignored out of the box: password managers and terminals, where
/// replacing text is unwelcome or unsafe.
#[cfg(not(target_os = "windows"))]
pub const DEFAULT_BLOCKLIST: &[&str] = &[
    "1password",
    "bitwarden",
    "keepassxc",
    "alacritty",
    "foot",
    "gnome-terminal-server",
    "kitty",
    "konsole",
    "org.wezfurlong.wezterm",
    "tilix",
    "xfce4-terminal",
    "xterm",
];

/// Error in the settings.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// A trigger key is not a known key code.
    #[error("unknown trigger key {0:?}")]
    UnknownKey(String),
    /// The trigger is too short.
    #[error(transparent)]
    Trigger(#[from] lf_core::TriggerError),
    /// The layout pair names the same layout twice or an empty id.
    #[error("the layout pair needs two different layouts")]
    BadPair,
    /// A number is out of range.
    #[error("{name} must be between {min} and {max} ms")]
    OutOfRange {
        /// The setting.
        name: &'static str,
        /// Smallest allowed value.
        min: u64,
        /// Largest allowed value.
        max: u64,
    },
}

/// Everything the user can set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Trigger keys as W3C key codes, e.g. `["KeyF", "KeyU", …]`.
    pub trigger: Vec<String>,
    /// The two layout ids to convert between; None picks automatically.
    pub layout_pair: Option<(String, String)>,
    /// How long after a fix a second trigger undoes it.
    pub undo_window_ms: u64,
    /// Clear the buffer after this long without a key press.
    pub idle_reset_ms: u64,
    /// Switch the system layout after a fix.
    pub switch_layout_after_fix: bool,
    /// App ids to ignore (lowercase).
    pub blocklist: Vec<String>,
    /// Start hidden in the tray when the user logs in.
    pub start_at_login: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let r = RuntimeConfig::default();
        AppConfig {
            trigger: r.trigger.keys().iter().map(|k| k.as_w3c().into()).collect(),
            layout_pair: None,
            undo_window_ms: r.undo_window_ms,
            idle_reset_ms: r.idle_reset_ms,
            switch_layout_after_fix: r.switch_layout_after_fix,
            blocklist: DEFAULT_BLOCKLIST.iter().map(|s| s.to_string()).collect(),
            start_at_login: false,
        }
    }
}

impl AppConfig {
    /// Check every value and build the runtime settings.
    pub fn to_runtime(&self) -> Result<RuntimeConfig, ConfigError> {
        let keys = self
            .trigger
            .iter()
            .map(|k| KeyCode::from_w3c(k).ok_or_else(|| ConfigError::UnknownKey(k.clone())))
            .collect::<Result<Vec<_>, _>>()?;
        let trigger = Trigger::new(keys)?;
        let layout_pair = match &self.layout_pair {
            None => None,
            Some((a, b)) if a.is_empty() || b.is_empty() || a == b => {
                return Err(ConfigError::BadPair)
            }
            Some((a, b)) => Some((LayoutId::new(a), LayoutId::new(b))),
        };
        if self.undo_window_ms > MAX_UNDO_WINDOW_MS {
            return Err(ConfigError::OutOfRange {
                name: "undo window",
                min: 0,
                max: MAX_UNDO_WINDOW_MS,
            });
        }
        let (min, max) = IDLE_RESET_RANGE_MS;
        if !(min..=max).contains(&self.idle_reset_ms) {
            return Err(ConfigError::OutOfRange {
                name: "idle reset",
                min,
                max,
            });
        }
        Ok(RuntimeConfig {
            trigger,
            layout_pair,
            undo_window_ms: self.undo_window_ms,
            idle_reset_ms: self.idle_reset_ms,
            switch_layout_after_fix: self.switch_layout_after_fix,
            blocklist: self.blocklist.clone(),
        })
    }

    /// Tidy free-form values: trim and lowercase blocklist entries, drop
    /// empty ones and duplicates.
    pub fn normalized(mut self) -> Self {
        let mut seen = Vec::new();
        for entry in self.blocklist {
            let e = entry.trim().to_lowercase();
            if !e.is_empty() && !seen.contains(&e) {
                seen.push(e);
            }
        }
        self.blocklist = seen;
        self
    }
}

/// Where the settings live, and loading/saving them.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

/// The result of loading the settings.
#[derive(Debug)]
pub struct Loaded {
    /// The settings to use.
    pub config: AppConfig,
    /// A message for the user if the file was broken and replaced.
    pub notice: Option<String>,
}

impl ConfigStore {
    /// The store in the platform config directory, e.g.
    /// `%APPDATA%\Layout Fixer\config\config.toml` or
    /// `~/.config/layout-fixer/config.toml`.
    pub fn platform() -> Option<Self> {
        let dirs = directories::ProjectDirs::from("", "", "Layout Fixer")?;
        Some(ConfigStore::at(dirs.config_dir().join(FILE_NAME)))
    }

    /// A store at an explicit path.
    pub fn at(path: PathBuf) -> Self {
        ConfigStore { path }
    }

    /// The settings file's path.
    #[cfg(test)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn backup_path(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".broken");
        self.path.with_file_name(name)
    }

    /// Load the settings. A missing file gives defaults; a broken one is
    /// moved aside and also gives defaults, with a notice.
    pub fn load(&self) -> Loaded {
        let text = match fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Loaded {
                    config: AppConfig::default(),
                    notice: None,
                }
            }
            Err(e) => return self.recover(&format!("could not be read ({e})")),
        };
        let config = match toml::from_str::<AppConfig>(&text) {
            Ok(c) => c.normalized(),
            Err(e) => return self.recover(&format!("is not valid TOML ({})", e.message())),
        };
        match config.to_runtime() {
            Ok(_) => Loaded {
                config,
                notice: None,
            },
            Err(e) => self.recover(&format!("has an invalid value: {e}")),
        }
    }

    fn recover(&self, why: &str) -> Loaded {
        let backup = self.backup_path();
        let moved = fs::rename(&self.path, &backup).is_ok();
        let mut notice = format!("Your settings file {why}. Layout Fixer started with defaults");
        if moved {
            notice.push_str(&format!("; the old file was saved as {}", backup.display()));
        }
        notice.push('.');
        Loaded {
            config: AppConfig::default(),
            notice: Some(notice),
        }
    }

    /// Save the settings, replacing the file atomically.
    pub fn save(&self, config: &AppConfig) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(config).map_err(io::Error::other)?;
        let tmp = self.path.with_extension("toml.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> ConfigStore {
        let dir = std::env::temp_dir().join(format!("lf-app-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        ConfigStore::at(dir.join(FILE_NAME))
    }

    #[test]
    fn defaults_match_phase_1() {
        let c = AppConfig::default();
        let r = c.to_runtime().unwrap();
        assert_eq!(
            r,
            RuntimeConfig {
                blocklist: c.blocklist.clone(),
                ..RuntimeConfig::default()
            }
        );
        assert_eq!(c.trigger, ["KeyF", "KeyU", "KeyC", "KeyK", "KeyK", "KeyK"]);
        assert!(!c.start_at_login);
        assert!(!c.blocklist.is_empty());
    }

    #[test]
    fn missing_file_gives_defaults_without_notice() {
        let store = temp_store("missing");
        let loaded = store.load();
        assert_eq!(loaded.config, AppConfig::default());
        assert!(loaded.notice.is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let store = temp_store("round-trip");
        let config = AppConfig {
            trigger: vec!["KeyJ".into(), "KeyJ".into(), "KeyJ".into(), "KeyJ".into()],
            layout_pair: Some(("us".into(), "ir".into())),
            undo_window_ms: 5_000,
            idle_reset_ms: 20_000,
            switch_layout_after_fix: false,
            blocklist: vec!["code.exe".into()],
            start_at_login: true,
        };
        store.save(&config).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.config, config);
        assert!(loaded.notice.is_none());
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("layout_pair = ["), "{text}");
    }

    #[test]
    fn partial_file_fills_in_defaults() {
        let store = temp_store("partial");
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(
            store.path(),
            "start_at_login = true\nblocklist = [\" Foo.EXE \", \"foo.exe\", \"\"]\n",
        )
        .unwrap();
        let loaded = store.load();
        assert!(loaded.config.start_at_login);
        assert_eq!(loaded.config.blocklist, ["foo.exe"]);
        assert_eq!(loaded.config.trigger, AppConfig::default().trigger);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_replaced_by_defaults() {
        let store = temp_store("corrupt");
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), "trigger = [unterminated").unwrap();
        let loaded = store.load();
        assert_eq!(loaded.config, AppConfig::default());
        let notice = loaded.notice.unwrap();
        assert!(notice.contains("not valid TOML"), "{notice}");
        assert!(!store.path().exists());
        let backup = fs::read_to_string(store.backup_path()).unwrap();
        assert_eq!(backup, "trigger = [unterminated");
    }

    #[test]
    fn invalid_values_are_rejected_on_load() {
        for bad in [
            "trigger = [\"KeyF\", \"KeyU\"]",
            "trigger = [\"KeyF\", \"KeyU\", \"KeyC\", \"Nope\"]",
            "layout_pair = [\"us\", \"us\"]",
            "undo_window_ms = 999999",
            "idle_reset_ms = 5",
            "undo_window_ms = \"ten\"",
        ] {
            let store = temp_store("invalid");
            fs::create_dir_all(store.path().parent().unwrap()).unwrap();
            fs::write(store.path(), bad).unwrap();
            let loaded = store.load();
            assert_eq!(loaded.config, AppConfig::default(), "{bad}");
            assert!(loaded.notice.is_some(), "{bad}");
        }
    }

    #[test]
    fn to_runtime_errors() {
        let mut c = AppConfig {
            trigger: vec!["KeyA".into(); 3],
            ..AppConfig::default()
        };
        assert!(matches!(c.to_runtime(), Err(ConfigError::Trigger(_))));
        c.trigger = vec!["Enter".into(); 4];
        assert_eq!(c.to_runtime(), Err(ConfigError::UnknownKey("Enter".into())));
        c = AppConfig {
            layout_pair: Some(("".into(), "ir".into())),
            ..AppConfig::default()
        };
        assert_eq!(c.to_runtime(), Err(ConfigError::BadPair));
    }
}
