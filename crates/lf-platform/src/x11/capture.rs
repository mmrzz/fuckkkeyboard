//! Turning X events into [`PlatformEvent`]s.
//!
//! A dedicated thread with its own X connection listens to:
//! - XInput2 raw key and button presses on the root window (seen whatever
//!   window has focus, and not stopped by grabs);
//! - XKB state notifications (modifiers, active group) and new-keyboard
//!   notifications (layouts changed);
//! - property changes on the root window (`_NET_ACTIVE_WINDOW`,
//!   `_XKB_RULES_NAMES`).
//!
//! Our own typing arrives through the XTEST device; [`OwnInput`] counts the
//! presses we sent so exactly those are skipped, while XTEST input from
//! other tools (e.g. the integration test) is still seen.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lf_core::{InputEvent, KeyPress, LayoutId, Modifiers, ResetReason};
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use super::keycodes::{self, evdev_from_x};
use super::keymap::{Keymap, Level};
use super::tracking::{self, USE_CORE_KBD};
use super::{connection_error, lock, Atoms, Shared};
use crate::{BackendError, PlatformEvent};

/// X modifier mask bits.
const SHIFT: u16 = 1 << 0;
const LOCK: u16 = 1 << 1;
const CONTROL: u16 = 1 << 2;
const MOD1_ALT: u16 = 1 << 3;
const MOD4_SUPER: u16 = 1 << 6;
const MOD5_LVL3: u16 = 1 << 7;

/// How long our own injected presses may take to come back.
const OWN_INPUT_TTL: Duration = Duration::from_secs(2);

/// Counts the key presses the backend injected and has not seen come back.
#[derive(Debug, Default)]
pub(crate) struct OwnInput {
    pending: u32,
    since: Option<Instant>,
}

impl OwnInput {
    /// We are about to send `presses` key presses.
    pub fn expect(&mut self, presses: u32, now: Instant) {
        self.pending = self.pending.saturating_add(presses);
        self.since = Some(now);
    }

    fn expire(&mut self, now: Instant) {
        if self
            .since
            .is_some_and(|t| now.duration_since(t) > OWN_INPUT_TTL)
        {
            self.pending = 0;
            self.since = None;
        }
    }

    /// An XTEST press arrived: true (and consumed) if it is one of ours.
    pub fn take(&mut self, now: Instant) -> bool {
        self.expire(now);
        if self.pending == 0 {
            return false;
        }
        self.pending -= 1;
        true
    }

    /// Some of our presses have not come back yet.
    pub fn is_pending(&mut self, now: Instant) -> bool {
        self.expire(now);
        self.pending > 0
    }
}

/// Turn one key press into engine input.
///
/// `keycode` is the X keycode, `mods` the effective X modifier mask.
pub(crate) fn classify(
    keycode: u32,
    mods: u16,
    keymap: &Keymap,
    group: u8,
    layout: &LayoutId,
) -> Option<InputEvent> {
    let evdev = evdev_from_x(keycode)?;
    let chord = mods & (CONTROL | MOD1_ALT | MOD4_SUPER) != 0;
    match evdev {
        keycodes::BACKSPACE if chord => return Some(InputEvent::Chord),
        keycodes::BACKSPACE => return Some(InputEvent::Backspace),
        keycodes::ENTER | keycodes::KP_ENTER | keycodes::TAB | keycodes::ESC => {
            return Some(InputEvent::Reset(ResetReason::Submit))
        }
        keycodes::HOME
        | keycodes::END
        | keycodes::PAGE_UP
        | keycodes::PAGE_DOWN
        | keycodes::UP
        | keycodes::DOWN
        | keycodes::LEFT
        | keycodes::RIGHT => return Some(InputEvent::Reset(ResetReason::Navigation)),
        _ => {}
    }
    let level = Level {
        shift: mods & SHIFT != 0,
        lvl3: mods & MOD5_LVL3 != 0,
        lock: mods & LOCK != 0,
    };
    let produced = u8::try_from(keycode)
        .ok()
        .and_then(|kc| keymap.char_for(kc, group, level));
    let code = keycodes::key_code(evdev);
    if chord && (code.is_some() || produced.is_some()) {
        return Some(InputEvent::Chord);
    }
    match code {
        Some(code) => Some(InputEvent::Key(KeyPress {
            code,
            mods: Modifiers {
                shift: level.shift,
                altgr: level.lvl3,
            },
            layout: layout.clone(),
            produced,
        })),
        // A key the engine can't map (e.g. the keypad) typed something: the
        // buffer no longer matches the screen.
        None if produced.is_some() => Some(InputEvent::Reset(ResetReason::Manual)),
        // Modifiers, function keys, media keys…
        None => None,
    }
}

/// What the capture thread needs.
pub(crate) struct Capture {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    wakeup: Window,
    xtest_devices: HashSet<u16>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    epoch: Instant,
    tx: Sender<(PlatformEvent, u64)>,
    ewmh: bool,
    focus_window: Window,
}

impl Capture {
    /// Open a connection and subscribe to every event we need.
    ///
    /// Returns the capture state and the window that wakes it up.
    pub fn open(
        root: Window,
        atoms: Atoms,
        shared: Arc<Mutex<Shared>>,
        stop: Arc<AtomicBool>,
        epoch: Instant,
        tx: Sender<(PlatformEvent, u64)>,
    ) -> Result<(Capture, Window), BackendError> {
        let (conn, _) = super::connect()?;

        // XInput2: raw key and button presses from every master device.
        conn.xinput_xi_select_events(
            root,
            &[xinput::EventMask {
                deviceid: u16::from(xinput::Device::ALL_MASTER),
                mask: vec![
                    xinput::XIEventMask::RAW_KEY_PRESS | xinput::XIEventMask::RAW_BUTTON_PRESS,
                ],
            }],
        )
        .map_err(connection_error)?;

        // XKB: modifier/group state and keymap replacement.
        conn.xkb_select_events(
            USE_CORE_KBD,
            xkb::EventType::from(0u16),
            xkb::EventType::STATE_NOTIFY | xkb::EventType::NEW_KEYBOARD_NOTIFY,
            xkb::MapPart::from(0u16),
            xkb::MapPart::from(0u16),
            &xkb::SelectEventsAux::new(),
        )
        .map_err(connection_error)?;

        // Root properties: active window and layout names.
        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(connection_error)?;

        // A tiny unmapped window: a ClientMessage to it wakes the thread.
        let wakeup = conn.generate_id().map_err(connection_error)?;
        conn.create_window(
            0,
            wakeup,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )
        .map_err(connection_error)?;

        let xtest_devices = xtest_keyboards(&conn)?;
        let (focus_window, ewmh) = tracking::focused_window(&conn, root, &atoms)?;
        tracking::sync(&conn)?;
        // Start from the state as of now; notifications keep it current.
        let kb = tracking::keyboard_state(&conn)?;
        {
            let mut sh = lock(&shared);
            sh.kb = kb;
            sh.groups.set(kb.group);
        }

        Ok((
            Capture {
                conn,
                root,
                atoms,
                wakeup,
                xtest_devices,
                shared,
                stop,
                epoch,
                tx,
                ewmh,
                focus_window,
            },
            wakeup,
        ))
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    /// Send one event; false if the receiver is gone.
    fn send(&self, ev: PlatformEvent) -> bool {
        self.tx.send((ev, self.now_ms())).is_ok()
    }

    /// Run until stopped or the connection fails.
    pub fn run(mut self) {
        log::debug!("x11 capture started");
        loop {
            let ev = match self.conn.wait_for_event() {
                Ok(ev) => ev,
                Err(e) => {
                    log::error!("x11 capture: connection lost: {e}");
                    break;
                }
            };
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            if !self.on_event(ev) {
                break;
            }
        }
        let _ = self.conn.destroy_window(self.wakeup);
        let _ = self.conn.flush();
        log::debug!("x11 capture stopped");
    }

    /// Handle one event; false to stop.
    fn on_event(&mut self, ev: Event) -> bool {
        match ev {
            Event::XinputRawKeyPress(e) => self.on_key(e.detail, e.sourceid),
            Event::XinputRawButtonPress(e) if (1..=3).contains(&e.detail) => self.send(
                PlatformEvent::Input(InputEvent::Reset(ResetReason::MouseClick)),
            ),
            Event::XkbStateNotify(e) => {
                let mut sh = lock(&self.shared);
                sh.kb.mods = u16::from(e.mods);
                sh.kb.locked_mods = u16::from(e.locked_mods);
                sh.kb.group = u8::from(e.group);
                sh.groups.set(u8::from(e.group));
                drop(sh);
                self.report_group()
            }
            Event::XkbNewKeyboardNotify(_) => self.reload_keymap(),
            Event::PropertyNotify(e) if e.window == self.root => {
                if e.atom == self.atoms._NET_ACTIVE_WINDOW {
                    self.ewmh = true;
                    self.check_focus()
                } else if e.atom == self.atoms._XKB_RULES_NAMES {
                    self.reload_keymap()
                } else {
                    true
                }
            }
            Event::ClientMessage(e) if e.window == self.wakeup => !self.stop.load(Ordering::SeqCst),
            _ => true,
        }
    }

    fn on_key(&mut self, keycode: u32, source: u16) -> bool {
        let now = Instant::now();
        if self.xtest_devices.contains(&source) && lock(&self.shared).own.take(now) {
            return true;
        }
        if !self.ewmh && !self.check_focus() {
            return false;
        }
        if !self.report_group() {
            return false;
        }
        let input = {
            let sh = lock(&self.shared);
            let group = sh.kb.group;
            let Some(layout) = sh.keymap.id(group).cloned() else {
                return true;
            };
            classify(keycode, sh.kb.mods, &sh.keymap, group, &layout)
        };
        match input {
            Some(input) => self.send(PlatformEvent::Input(input)),
            None => true,
        }
    }

    /// Tell the engine about a group change it doesn't know about.
    fn report_group(&mut self) -> bool {
        let change = {
            let mut sh = lock(&self.shared);
            let pending = sh.own.is_pending(Instant::now());
            sh.groups
                .take_change(pending)
                .and_then(|g| sh.keymap.id(g).cloned())
        };
        match change {
            Some(id) => self.send(PlatformEvent::Input(InputEvent::LayoutChanged(id))),
            None => true,
        }
    }

    /// Emit `FocusChanged` if the focused window changed.
    fn check_focus(&mut self) -> bool {
        let window = if self.ewmh {
            tracking::active_window(&self.conn, self.root, &self.atoms)
                .ok()
                .flatten()
                .unwrap_or(0)
        } else {
            tracking::input_focus(&self.conn).unwrap_or(0)
        };
        if window == self.focus_window {
            return true;
        }
        self.focus_window = window;
        let app = tracking::app_of(&self.conn, window);
        lock(&self.shared).focused = app.clone();
        self.send(PlatformEvent::FocusChanged(app))
    }

    /// Re-read the keymap; emit `LayoutsChanged` if the layouts differ.
    fn reload_keymap(&mut self) -> bool {
        let names = match tracking::read_rules_names(&self.conn, self.root, &self.atoms) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("x11: {e}");
                return true;
            }
        };
        if lock(&self.shared).keymap.names() == &names {
            return true;
        }
        let keymap = match super::keymap::Keymap::compile(&names) {
            Ok(k) => k,
            Err(e) => {
                log::warn!("x11: keeping the old layouts: {e}");
                return true;
            }
        };
        let changed = {
            let mut sh = lock(&self.shared);
            let changed = !sh.keymap.same_layouts(&keymap);
            sh.keymap = Arc::new(keymap);
            changed
        };
        if changed {
            log::info!("x11: installed layouts changed");
            self.send(PlatformEvent::LayoutsChanged)
        } else {
            true
        }
    }
}

/// Ids of the XTEST slave keyboards (where synthetic key presses come from).
fn xtest_keyboards(conn: &RustConnection) -> Result<HashSet<u16>, BackendError> {
    let devices = conn
        .xinput_xi_query_device(xinput::Device::ALL)
        .map_err(connection_error)?
        .reply()
        .map_err(connection_error)?;
    Ok(devices
        .infos
        .iter()
        .filter(|d| d.type_ == xinput::DeviceType::SLAVE_KEYBOARD)
        .filter(|d| String::from_utf8_lossy(&d.name).contains("XTEST"))
        .map(|d| d.deviceid)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x11::keymap::RulesNames;
    use lf_core::KeyCode;

    fn us_ir() -> Keymap {
        Keymap::compile(&RulesNames {
            rules: "evdev".into(),
            model: "pc105".into(),
            layout: "us,ir".into(),
            variant: ",".into(),
            options: String::new(),
        })
        .unwrap()
    }

    fn kc(code: KeyCode) -> u32 {
        u32::from(keycodes::x_keycode(code))
    }

    fn key(code: KeyCode, mods: Modifiers, layout: &str, produced: Option<char>) -> InputEvent {
        InputEvent::Key(KeyPress {
            code,
            mods,
            layout: LayoutId::new(layout),
            produced,
        })
    }

    #[test]
    fn printable_keys_become_key_presses() {
        let km = us_ir();
        let us = LayoutId::new("us");
        let ir = LayoutId::new("ir");
        assert_eq!(
            classify(kc(KeyCode::KeyS), 0, &km, 0, &us),
            Some(key(KeyCode::KeyS, Modifiers::NONE, "us", Some('s')))
        );
        assert_eq!(
            classify(kc(KeyCode::KeyS), SHIFT, &km, 0, &us),
            Some(key(KeyCode::KeyS, Modifiers::SHIFT, "us", Some('S')))
        );
        assert_eq!(
            classify(kc(KeyCode::KeyG), 0, &km, 1, &ir),
            Some(key(KeyCode::KeyG, Modifiers::NONE, "ir", Some('ل')))
        );
        // Caps Lock changes what appears, not the modifiers the engine sees.
        assert_eq!(
            classify(kc(KeyCode::KeyA), LOCK, &km, 0, &us),
            Some(key(KeyCode::KeyA, Modifiers::NONE, "us", Some('A')))
        );
        // NumLock (Mod2) is ignored.
        assert_eq!(
            classify(kc(KeyCode::Space), 1 << 4, &km, 0, &us),
            Some(key(KeyCode::Space, Modifiers::NONE, "us", Some(' ')))
        );
    }

    #[test]
    fn shortcuts_are_chords() {
        let km = us_ir();
        let us = LayoutId::new("us");
        for mods in [CONTROL, MOD1_ALT, MOD4_SUPER, CONTROL | SHIFT] {
            assert_eq!(
                classify(kc(KeyCode::KeyC), mods, &km, 0, &us),
                Some(InputEvent::Chord)
            );
        }
        assert_eq!(
            classify(u32::from(km.backspace()), CONTROL, &km, 0, &us),
            Some(InputEvent::Chord)
        );
    }

    #[test]
    fn editing_and_navigation_keys() {
        let km = us_ir();
        let us = LayoutId::new("us");
        let x = |evdev: u16| u32::from(evdev) + 8;
        assert_eq!(
            classify(x(keycodes::BACKSPACE), SHIFT, &km, 0, &us),
            Some(InputEvent::Backspace)
        );
        for k in [
            keycodes::ENTER,
            keycodes::TAB,
            keycodes::ESC,
            keycodes::KP_ENTER,
        ] {
            assert_eq!(
                classify(x(k), 0, &km, 0, &us),
                Some(InputEvent::Reset(ResetReason::Submit))
            );
        }
        for k in [keycodes::LEFT, keycodes::HOME, keycodes::PAGE_DOWN] {
            assert_eq!(
                classify(x(k), 0, &km, 0, &us),
                Some(InputEvent::Reset(ResetReason::Navigation))
            );
        }
    }

    #[test]
    fn other_keys() {
        let km = us_ir();
        let us = LayoutId::new("us");
        let x = |evdev: u16| u32::from(evdev) + 8;
        // Left Shift, Left Ctrl, F1: nothing.
        for k in [42, 29, 59] {
            assert_eq!(classify(x(k), 0, &km, 0, &us), None, "{k}");
        }
        // Keypad minus types a char the engine can't track.
        assert_eq!(
            classify(x(74), 0, &km, 0, &us),
            Some(InputEvent::Reset(ResetReason::Manual))
        );
        assert_eq!(classify(3, 0, &km, 0, &us), None);
    }

    #[test]
    fn own_input_is_counted_and_expires() {
        let t0 = Instant::now();
        let mut own = OwnInput::default();
        assert!(!own.take(t0));
        own.expect(2, t0);
        assert!(own.is_pending(t0));
        assert!(own.take(t0));
        assert!(own.take(t0));
        assert!(!own.take(t0));
        own.expect(5, t0);
        let later = t0 + OWN_INPUT_TTL + Duration::from_millis(1);
        assert!(!own.is_pending(later));
        assert!(!own.take(later));
    }
}
