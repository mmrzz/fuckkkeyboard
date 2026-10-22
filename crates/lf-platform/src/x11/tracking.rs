//! Following the focused window and the active layout group.
//!
//! The focused app comes from the EWMH `_NET_ACTIVE_WINDOW` root property
//! when the window manager maintains it, else from the X input focus. The
//! app id is the class half of the window's `WM_CLASS`.

use x11rb::connection::Connection;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use super::keymap::{Keymap, RulesNames};
use super::{connection_error, Atoms};
use crate::{AppId, BackendError};

/// The device spec for the core keyboard in XKB requests.
pub(crate) const USE_CORE_KBD: xkb::DeviceSpec = 0x100;

/// The class from a `WM_CLASS` value (`instance\0class\0`), else the instance.
pub(crate) fn parse_wm_class(bytes: &[u8]) -> Option<String> {
    let mut parts = bytes
        .split(|&b| b == 0)
        .map(|p| String::from_utf8_lossy(p).trim().to_owned())
        .filter(|p| !p.is_empty());
    let instance = parts.next()?;
    Some(parts.next().unwrap_or(instance))
}

/// Read `_XKB_RULES_NAMES` from the root window.
pub(crate) fn read_rules_names(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
) -> Result<RulesNames, BackendError> {
    let reply = conn
        .get_property(false, root, atoms._XKB_RULES_NAMES, AtomEnum::ANY, 0, 1024)
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    RulesNames::parse(&reply.value).ok_or_else(|| {
        BackendError::Connection(
            "the X server does not publish its keyboard layouts (_XKB_RULES_NAMES)".into(),
        )
    })
}

/// Read and compile the server's current keymap.
pub(crate) fn read_keymap(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
) -> Result<Keymap, BackendError> {
    Keymap::compile(&read_rules_names(conn, root, atoms)?)
}

/// Keyboard state as XKB reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct KbState {
    /// Effective layout group.
    pub group: u8,
    /// Effective modifiers (X mod mask).
    pub mods: u16,
    /// Locked modifiers (X mod mask).
    pub locked_mods: u16,
}

/// Ask the server for the core keyboard's state.
pub(crate) fn keyboard_state(conn: &RustConnection) -> Result<KbState, BackendError> {
    let s = conn
        .xkb_get_state(USE_CORE_KBD)
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    Ok(KbState {
        group: u8::from(s.group),
        mods: u16::from(s.mods),
        locked_mods: u16::from(s.locked_mods),
    })
}

/// The window in `_NET_ACTIVE_WINDOW`: Ok(None) if the property is missing
/// (no EWMH window manager), Ok(Some(0)) if nothing is active.
pub(crate) fn active_window(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
) -> Result<Option<Window>, BackendError> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms._NET_ACTIVE_WINDOW,
            AtomEnum::WINDOW,
            0,
            1,
        )
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    if reply.type_ == u32::from(AtomEnum::NONE) {
        return Ok(None);
    }
    Ok(Some(
        reply.value32().and_then(|mut v| v.next()).unwrap_or(0),
    ))
}

/// The window with the X input focus (0 if none or PointerRoot).
pub(crate) fn input_focus(conn: &RustConnection) -> Result<Window, BackendError> {
    let focus = conn
        .get_input_focus()
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?
        .focus;
    Ok(if focus <= 1 { 0 } else { focus })
}

/// The app owning `window`: the first `WM_CLASS` on the way up to the root.
pub(crate) fn app_of(conn: &RustConnection, window: Window) -> Option<AppId> {
    let mut w = window;
    for _ in 0..16 {
        if w == 0 {
            return None;
        }
        let class = conn
            .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        if let Some(c) = parse_wm_class(&class.value) {
            return Some(AppId(c));
        }
        let tree = conn.query_tree(w).ok()?.reply().ok()?;
        if tree.parent == tree.root || tree.parent == 0 {
            return None;
        }
        w = tree.parent;
    }
    None
}

/// The focused window, from EWMH if available, else the input focus.
pub(crate) fn focused_window(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
) -> Result<(Window, bool), BackendError> {
    match active_window(conn, root, atoms)? {
        Some(w) => Ok((w, true)),
        None => Ok((input_focus(conn)?, false)),
    }
}

/// Make sure every request so far has reached the server and been processed.
pub(crate) fn sync(conn: &RustConnection) -> Result<(), BackendError> {
    conn.flush().map_err(connection_error)?;
    conn.get_input_focus()
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    Ok(())
}

/// Decides when the engine must hear about a layout group change.
///
/// The engine already knows about switches the backend made itself (it asked
/// for them), and telling it again would cancel its undo record. So a group
/// change is reported only if it differs from the last group the engine
/// knows, and never while our own injected input is still arriving (a fix
/// may lock other groups briefly to type some characters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GroupTracker {
    current: u8,
    reported: u8,
}

impl GroupTracker {
    /// Start at `group`, which the engine is assumed to know.
    pub fn new(group: u8) -> Self {
        GroupTracker {
            current: group,
            reported: group,
        }
    }

    /// The server's group changed.
    pub fn set(&mut self, group: u8) {
        self.current = group;
    }

    /// The engine now believes `group` is active (the backend switched to it
    /// on the engine's request).
    pub fn mark_reported(&mut self, group: u8) {
        self.reported = group;
    }

    /// The group to report now, if the engine's belief is out of date.
    pub fn take_change(&mut self, own_input_pending: bool) -> Option<u8> {
        if own_input_pending || self.current == self.reported {
            return None;
        }
        self.reported = self.current;
        Some(self.current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_prefers_the_class() {
        assert_eq!(
            parse_wm_class(b"navigator\0firefox\0").as_deref(),
            Some("firefox")
        );
        assert_eq!(parse_wm_class(b"code\0Code\0").as_deref(), Some("Code"));
        assert_eq!(parse_wm_class(b"xterm\0").as_deref(), Some("xterm"));
        assert_eq!(parse_wm_class(b"\0\0"), None);
        assert_eq!(parse_wm_class(b""), None);
    }

    #[test]
    fn user_switches_are_reported_once() {
        let mut t = GroupTracker::new(0);
        assert_eq!(t.take_change(false), None);
        t.set(1);
        assert_eq!(t.take_change(false), Some(1));
        assert_eq!(t.take_change(false), None);
        t.set(0);
        t.set(1);
        assert_eq!(t.take_change(false), None, "back where the engine thinks");
    }

    #[test]
    fn own_switches_are_not_reported() {
        let mut t = GroupTracker::new(0);
        // A fix switches to group 1, typing one char in group 2 on the way.
        t.mark_reported(1);
        t.set(2);
        assert_eq!(t.take_change(true), None, "own input still pending");
        t.set(1);
        assert_eq!(t.take_change(false), None);
    }

    #[test]
    fn a_switch_during_own_input_is_reported_afterwards() {
        let mut t = GroupTracker::new(0);
        t.set(1);
        assert_eq!(t.take_change(true), None);
        assert_eq!(t.take_change(false), Some(1));
    }
}
