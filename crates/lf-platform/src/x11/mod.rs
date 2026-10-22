//! Linux X11 backend (Phase 4).
//!
//! Built on standard X protocol extensions, each a documented public API:
//! XInput2 raw events for capture, XKB for layouts and layout switching,
//! XTEST for typing. On a Wayland session [`X11Backend::new`] reports
//! [`BackendError::Unsupported`] so the app can fall back to the preview.

pub mod keycodes;

use x11rb::connection::RequestConnection;
use x11rb::protocol::{xinput, xkb, xtest};
use x11rb::rust_connection::RustConnection;

use crate::BackendError;

/// Shown when the session is Wayland.
pub const WAYLAND_MESSAGE: &str =
    "Wayland isn't supported yet — log in with an Xorg/X11 session, or wait for Phase 5.";

/// Decide from the environment whether this is an X11 session.
///
/// `var` looks up an environment variable. Unsupported on a Wayland session
/// (`XDG_SESSION_TYPE=wayland`, or `WAYLAND_DISPLAY` set without an X
/// `DISPLAY`) and when there is no X display at all.
pub(crate) fn check_session(var: impl Fn(&str) -> Option<String>) -> Result<(), BackendError> {
    let set = |name: &str| var(name).is_some_and(|v| !v.trim().is_empty());
    let session = var("XDG_SESSION_TYPE").unwrap_or_default();
    if session.trim().eq_ignore_ascii_case("wayland") {
        return Err(BackendError::Unsupported(WAYLAND_MESSAGE.into()));
    }
    let display = set("DISPLAY");
    if set("WAYLAND_DISPLAY") && !display {
        return Err(BackendError::Unsupported(WAYLAND_MESSAGE.into()));
    }
    if !display {
        return Err(BackendError::Unsupported(
            "No X11 display: DISPLAY is not set.".into(),
        ));
    }
    Ok(())
}

pub(crate) fn connection_error(e: impl std::fmt::Display) -> BackendError {
    BackendError::Connection(e.to_string())
}

/// Connect to the X server and check every extension the backend needs.
pub(crate) fn connect() -> Result<(RustConnection, usize), BackendError> {
    let (conn, screen) = RustConnection::connect(None)
        .map_err(|e| BackendError::Unsupported(format!("cannot connect to the X server: {e}")))?;
    check_extensions(&conn)?;
    Ok((conn, screen))
}

fn check_extensions(conn: &RustConnection) -> Result<(), BackendError> {
    for name in [
        xinput::X11_EXTENSION_NAME,
        xkb::X11_EXTENSION_NAME,
        xtest::X11_EXTENSION_NAME,
    ] {
        let present = conn
            .extension_information(name)
            .map_err(connection_error)?
            .is_some();
        if !present {
            return Err(BackendError::Unsupported(format!(
                "the X server lacks the {name} extension"
            )));
        }
    }
    let xi = xinput::xi_query_version(conn, 2, 2)
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    if (xi.major_version, xi.minor_version) < (2, 1) {
        return Err(BackendError::Unsupported(format!(
            "XInput {}.{} is too old, need 2.1",
            xi.major_version, xi.minor_version
        )));
    }
    let kb = xkb::use_extension(conn, 1, 0)
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    if !kb.supported {
        return Err(BackendError::Unsupported("XKB 1.0 is not supported".into()));
    }
    xtest::get_version(conn, 2, 2)
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    Ok(())
}

/// The X11 backend.
pub struct X11Backend {
    #[allow(dead_code)] // used by the capture and injection tasks
    conn: RustConnection,
    #[allow(dead_code)]
    screen: usize,
}

impl X11Backend {
    /// Connect to the X server of the current X11 session.
    ///
    /// Errors with [`BackendError::Unsupported`] on Wayland, without an X
    /// display, or when a needed extension is missing.
    pub fn new() -> Result<Self, BackendError> {
        check_session(|name| std::env::var(name).ok())?;
        let (conn, screen) = connect()?;
        Ok(X11Backend { conn, screen })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    fn unsupported(r: Result<(), BackendError>) -> String {
        match r {
            Err(BackendError::Unsupported(m)) => m,
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn x11_session_is_accepted() {
        let vars = [("XDG_SESSION_TYPE", "x11"), ("DISPLAY", ":0")];
        assert_eq!(check_session(env(&vars)), Ok(()));
        // No session type (e.g. started from a plain xinit) is fine too.
        assert_eq!(check_session(env(&[("DISPLAY", ":1")])), Ok(()));
    }

    #[test]
    fn wayland_session_is_unsupported_even_with_xwayland() {
        let vars = [
            ("XDG_SESSION_TYPE", "wayland"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0"),
        ];
        assert_eq!(unsupported(check_session(env(&vars))), WAYLAND_MESSAGE);
        let vars = [("XDG_SESSION_TYPE", "Wayland"), ("DISPLAY", ":0")];
        assert_eq!(unsupported(check_session(env(&vars))), WAYLAND_MESSAGE);
    }

    #[test]
    fn wayland_display_without_x_display_is_unsupported() {
        let vars = [("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", "")];
        assert_eq!(unsupported(check_session(env(&vars))), WAYLAND_MESSAGE);
    }

    #[test]
    fn missing_display_is_unsupported() {
        let msg = unsupported(check_session(env(&[("XDG_SESSION_TYPE", "tty")])));
        assert!(msg.contains("DISPLAY"), "{msg}");
    }
}
