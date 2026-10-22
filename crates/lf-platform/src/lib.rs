//! Platform backend interface.
//!
//! A backend connects the engine to the operating system: it captures key
//! presses and other input, turns them into [`PlatformEvent`]s, and carries
//! out the engine's [`lf_core::Command`]s (sending backspaces, typing text,
//! switching the system layout).
//!
//! [`MockBackend`] is always compiled so tests and the app's UI can run with
//! no real OS access. Each OS backend lives in its own `cfg`-gated module.
//!
//! This file is the contract every backend implements. It is frozen once the
//! shared foundation is merged: changing it affects every backend in flight.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod mock;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod x11;

pub use mock::MockBackend;

use std::sync::mpsc::Sender;

use lf_core::{Command, InputEvent, Layout, LayoutId};

/// Identifies an application, for the blocklist and the UI.
///
/// Windows: the executable file name, lowercase. X11: the `WM_CLASS` class.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AppId(pub String);

impl AppId {
    /// Build an id from a string slice.
    pub fn new(id: &str) -> Self {
        AppId(id.to_owned())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One keyboard layout installed on the system.
#[derive(Debug, Clone)]
pub struct LayoutInfo {
    /// The layout's key map.
    pub layout: Layout,
    /// Two-letter language code ("en", "fa"), or None if unknown.
    pub language: Option<String>,
}

/// Something the backend observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEvent {
    /// Input for the engine.
    Input(InputEvent),
    /// Keyboard focus moved to another application (None if unknown).
    FocusChanged(Option<AppId>),
    /// The set of installed layouts changed; call [`Backend::layouts`] again.
    LayoutsChanged,
}

/// Error reported by a platform backend.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// The backend cannot run on the current system.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Connecting to the OS input system failed or was lost.
    #[error("connection error: {0}")]
    Connection(String),
    /// Sending synthetic input failed.
    #[error("injection failed: {0}")]
    Injection(String),
    /// Switching the system layout failed.
    #[error("layout switch failed: {0}")]
    LayoutSwitch(String),
    /// The backend is not started, or has stopped.
    #[error("backend is not running")]
    NotRunning,
}

/// Connects the engine to one operating system.
pub trait Backend: Send {
    /// Start capturing input. Each event is sent with its timestamp in
    /// milliseconds, taken from a monotonic clock.
    fn start(&mut self, tx: Sender<(PlatformEvent, u64)>) -> Result<(), BackendError>;

    /// Stop capturing input and drop the sender given to [`Backend::start`].
    fn stop(&mut self);

    /// Carry out all commands from ONE engine step, in order.
    fn execute(&mut self, cmds: &[Command]) -> Result<(), BackendError>;

    /// The layout currently active on the system.
    fn current_layout(&self) -> Result<LayoutId, BackendError>;

    /// Every installed layout.
    fn layouts(&self) -> Result<Vec<LayoutInfo>, BackendError>;

    /// The application that has keyboard focus, if known.
    fn focused_app(&self) -> Option<AppId>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_is_object_safe() {
        let backend: Box<dyn Backend> = Box::new(MockBackend::us_ir());
        assert_eq!(backend.current_layout().unwrap(), LayoutId::new("us"));
    }
}
