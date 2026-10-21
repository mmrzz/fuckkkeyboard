//! Input events fed to the engine.

use crate::key::KeyPress;
use crate::layout::LayoutId;

/// Why the buffer was reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResetReason {
    /// Keyboard focus moved to another control or window.
    FocusChanged,
    /// A mouse click, which may have moved the caret.
    MouseClick,
    /// Arrows, Home, End, PgUp, PgDn.
    Navigation,
    /// Enter, Tab, Esc.
    Submit,
    /// Focus is in a password field.
    PasswordField,
    /// Reset requested by the user or the app.
    Manual,
}

/// One input event as seen by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    /// A printable key press.
    Key(KeyPress),
    /// Backspace was pressed.
    Backspace,
    /// Any key pressed with Ctrl/Alt/Super/Cmd held (copy, paste, undo…).
    Chord,
    /// Something happened that makes the buffer unreliable.
    Reset(ResetReason),
    /// User (or the system) changed the active layout.
    LayoutChanged(LayoutId),
}
