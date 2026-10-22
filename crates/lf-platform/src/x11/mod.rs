//! Linux X11 backend (Phase 4).
//!
//! Built on standard X protocol extensions, each a documented public API:
//! XInput2 raw events for capture, XKB for layouts and layout switching,
//! XTEST for typing. On a Wayland session [`X11Backend::new`] reports
//! [`BackendError::Unsupported`] so the app can fall back to the preview.

mod capture;
mod inject;
pub mod keycodes;
mod keymap;
mod tracking;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

use lf_core::{Command, LayoutId};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt as _, EventMask, Window};
use x11rb::protocol::{xinput, xkb, xtest};
use x11rb::rust_connection::RustConnection;

use crate::{AppId, Backend, BackendError, LayoutInfo, PlatformEvent};
use capture::{Capture, OwnInput};
use inject::{Desktop, Injector, SpareKey};
use keymap::Keymap;
use tracking::{GroupTracker, KbState};

x11rb::atom_manager! {
    /// Atoms the backend uses.
    pub(crate) Atoms: AtomsCookie {
        _NET_ACTIVE_WINDOW,
        _XKB_RULES_NAMES,
    }
}

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

/// State shared by the capture thread and the backend.
pub(crate) struct Shared {
    keymap: Arc<Keymap>,
    kb: KbState,
    groups: GroupTracker,
    focused: Option<AppId>,
    own: OwnInput,
}

pub(crate) fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

struct Running {
    thread: JoinHandle<()>,
    stop: Arc<AtomicBool>,
    wakeup: Window,
}

/// The X11 backend: XInput2 capture, XKB layouts, XTEST typing.
pub struct X11Backend {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    shared: Arc<Mutex<Shared>>,
    epoch: Instant,
    running: Option<Running>,
    desktop: Desktop,
    bus: Option<zbus::blocking::Connection>,
    spare: Option<Option<SpareKey>>,
}

impl X11Backend {
    /// Connect to the X server of the current X11 session and read its
    /// layouts. Self-contained: the app just calls this and, on error,
    /// falls back to the mock backend.
    ///
    /// Errors with [`BackendError::Unsupported`] on Wayland, without an X
    /// display, or when a needed extension is missing; with
    /// [`BackendError::Connection`] if the keymap can't be read.
    pub fn new() -> Result<Self, BackendError> {
        check_session(|name| std::env::var(name).ok())?;
        let (conn, screen) = connect()?;
        let root = conn
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| BackendError::Connection("no such X screen".into()))?
            .root;
        let atoms = Atoms::new(&conn)
            .map_err(connection_error)?
            .reply()
            .map_err(connection_error)?;
        let keymap = tracking::read_keymap(&conn, root, &atoms)?;
        let kb = tracking::keyboard_state(&conn)?;
        let focused = tracking::focused_window(&conn, root, &atoms)
            .ok()
            .and_then(|(w, _)| tracking::app_of(&conn, w));
        let desktop = Desktop::detect(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref());
        if desktop == Desktop::Gnome {
            log::info!(
                "x11: GNOME has no public D-Bus method to switch input sources; \
                 switching through XKB only, the top-bar indicator may lag"
            );
        }
        Ok(X11Backend {
            conn,
            root,
            atoms,
            shared: Arc::new(Mutex::new(Shared {
                keymap: Arc::new(keymap),
                kb,
                groups: GroupTracker::new(kb.group),
                focused,
                own: OwnInput::default(),
            })),
            epoch: Instant::now(),
            running: None,
            desktop,
            bus: None,
            spare: None,
        })
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    fn spare_key(&mut self) -> Option<SpareKey> {
        if self.spare.is_none() {
            let found = inject::find_spare_key(&self.conn).unwrap_or_else(|e| {
                log::warn!("x11: {e}");
                None
            });
            self.spare = Some(found);
        }
        self.spare.flatten()
    }

    /// Keep the desktop's layout indicator in sync after a switch.
    fn sync_desktop(&mut self, group: u8) {
        if self.desktop == Desktop::Kde {
            if let Err(e) = inject::kde_set_layout(&mut self.bus, group) {
                log::warn!("x11: could not tell KDE about the layout switch: {e}");
            }
        }
    }
}

impl Backend for X11Backend {
    fn start(&mut self, tx: Sender<(PlatformEvent, u64)>) -> Result<(), BackendError> {
        self.stop();
        // The engine is built from current_layout() right before start.
        {
            let mut sh = self.shared();
            let group = sh.kb.group;
            sh.groups = GroupTracker::new(group);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (capture, wakeup) = Capture::open(
            self.root,
            self.atoms,
            Arc::clone(&self.shared),
            Arc::clone(&stop),
            self.epoch,
            tx,
        )?;
        let thread = std::thread::Builder::new()
            .name("lf-x11-capture".into())
            .spawn(move || capture.run())
            .map_err(connection_error)?;
        self.running = Some(Running {
            thread,
            stop,
            wakeup,
        });
        log::info!("x11 backend started");
        Ok(())
    }

    fn stop(&mut self) {
        let Some(r) = self.running.take() else {
            return;
        };
        r.stop.store(true, Ordering::SeqCst);
        let wake = ClientMessageEvent::new(32, r.wakeup, self.atoms._NET_ACTIVE_WINDOW, [0u32; 5]);
        let sent = self
            .conn
            .send_event(false, r.wakeup, EventMask::NO_EVENT, wake)
            .and_then(|_| self.conn.flush());
        if let Err(e) = sent {
            log::warn!("x11: could not wake the capture thread: {e}");
        }
        let _ = r.thread.join();
        log::info!("x11 backend stopped");
    }

    fn execute(&mut self, cmds: &[Command]) -> Result<(), BackendError> {
        if cmds.is_empty() {
            return Ok(());
        }
        let kb = tracking::keyboard_state(&self.conn)?;
        let keymap = Arc::clone(&self.shared().keymap);
        let caps_lock = kb.locked_mods & 0x2 != 0;
        let plan = inject::plan(cmds, &keymap, kb.group, caps_lock)?;
        let spare = if plan
            .steps
            .iter()
            .any(|s| matches!(s, inject::Step::Unicode(_)))
        {
            self.spare_key()
        } else {
            None
        };
        {
            let mut sh = self.shared();
            sh.own.expect(plan.presses(), Instant::now());
            sh.groups.mark_reported(plan.final_group);
        }
        let injector = Injector {
            conn: &self.conn,
            root: self.root,
            keymap: &keymap,
            spare,
        };
        injector.run(&plan, kb.group)?;
        if let Some(g) = plan.switched_to {
            self.sync_desktop(g);
        }
        Ok(())
    }

    fn current_layout(&self) -> Result<LayoutId, BackendError> {
        let kb = tracking::keyboard_state(&self.conn)?;
        let mut sh = self.shared();
        sh.kb.group = kb.group;
        sh.keymap
            .id(kb.group)
            .cloned()
            .ok_or_else(|| BackendError::Connection(format!("no layout for group {}", kb.group)))
    }

    fn layouts(&self) -> Result<Vec<LayoutInfo>, BackendError> {
        Ok(self.shared().keymap.layouts())
    }

    fn focused_app(&self) -> Option<AppId> {
        self.shared().focused.clone()
    }
}

impl Drop for X11Backend {
    fn drop(&mut self) {
        self.stop();
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
