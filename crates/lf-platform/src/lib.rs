//! Platform backend interface.
//!
//! A backend connects the engine to the operating system: it captures key
//! presses and other input, turns them into [`lf_core::InputEvent`]s, and
//! carries out the engine's [`lf_core::Command`]s (sending backspaces, typing
//! text, switching the system layout).
//!
//! Phase 1 defines only the trait. Implementations for Linux, Windows and
//! macOS arrive in later phases.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::mpsc::Sender;

use lf_core::{Command, InputEvent, LayoutId};

/// Error reported by a platform backend.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// The backend cannot do this on the current system.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The operating system reported an error.
    #[error("platform error: {0}")]
    Platform(String),
    /// The backend is not started, or has stopped.
    #[error("backend is not running")]
    NotRunning,
}

/// Connects the engine to one operating system.
pub trait Backend {
    /// Start capturing input. Each event is sent with its timestamp in
    /// milliseconds, taken from a monotonic clock.
    fn start(&mut self, tx: Sender<(InputEvent, u64)>) -> Result<(), BackendError>;

    /// Carry out engine commands, in order.
    fn execute(&mut self, cmds: &[Command]) -> Result<(), BackendError>;

    /// The layout currently active on the system.
    fn current_layout(&self) -> Result<LayoutId, BackendError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A backend that replays canned events and records commands.
    struct Fake {
        executed: Vec<Command>,
    }

    impl Backend for Fake {
        fn start(&mut self, tx: Sender<(InputEvent, u64)>) -> Result<(), BackendError> {
            tx.send((InputEvent::Chord, 7))
                .map_err(|_| BackendError::NotRunning)
        }

        fn execute(&mut self, cmds: &[Command]) -> Result<(), BackendError> {
            self.executed.extend_from_slice(cmds);
            Ok(())
        }

        fn current_layout(&self) -> Result<LayoutId, BackendError> {
            Ok(LayoutId::new("us"))
        }
    }

    #[test]
    fn trait_is_object_safe() {
        let mut backend: Box<dyn Backend> = Box::new(Fake {
            executed: Vec::new(),
        });
        let (tx, rx) = mpsc::channel();
        backend.start(tx).unwrap();
        assert_eq!(rx.recv().unwrap(), (InputEvent::Chord, 7));
        backend
            .execute(&[Command::SetLayout(LayoutId::new("ir"))])
            .unwrap();
        assert_eq!(backend.current_layout().unwrap(), LayoutId::new("us"));
    }
}
