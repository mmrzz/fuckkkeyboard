//! Carrying out a fix: XTEST key presses, XKB group locks, and the
//! temporary-keysym fallback for characters no group can type.
//!
//! The engine's commands are first turned into a plan of [`Step`]s (pure,
//! unit-tested), then sent in one go on the backend's connection.

use std::thread;
use std::time::Duration;

use lf_core::Command;
use x11rb::connection::Connection;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::keymap::{Keymap, Stroke};
use super::tracking::{self, USE_CORE_KBD};
use crate::BackendError;

/// How long clients get to notice a keymap change before we type with it.
const REMAP_SETTLE: Duration = Duration::from_millis(30);

/// One thing to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Lock this layout group.
    LockGroup(u8),
    /// Press and release a key, holding modifiers as the stroke says.
    Tap(Stroke),
    /// Type a char no group has, by binding it to a spare key for a moment.
    Unicode(char),
}

/// A planned fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// What to send, in order.
    pub steps: Vec<Step>,
    /// The group locked when the plan is done.
    pub final_group: u8,
    /// The group the engine asked to switch to, if any.
    pub switched_to: Option<u8>,
}

impl Plan {
    /// Number of key presses the plan sends (each comes back as a raw event).
    pub fn presses(&self) -> u32 {
        self.steps
            .iter()
            .map(|s| match s {
                Step::LockGroup(_) => 0,
                Step::Tap(t) => 1 + u32::from(t.shift) + u32::from(t.lvl3),
                Step::Unicode(_) => 1,
            })
            .sum()
    }
}

/// Turn one engine step's commands into key presses and group locks.
///
/// A layout switch is done first, so the replacement text (which belongs to
/// the target layout) is typed in its own group. Characters the active group
/// can't type are typed in another group that can (locked just for them), or
/// else through a temporary keysym.
pub(crate) fn plan(
    cmds: &[Command],
    keymap: &Keymap,
    group: u8,
    caps_lock: bool,
) -> Result<Plan, BackendError> {
    let mut switched_to = None;
    for cmd in cmds {
        if let Command::SetLayout(id) = cmd {
            let g = keymap.group_of(id).ok_or_else(|| {
                BackendError::LayoutSwitch(format!("layout {id} is not installed"))
            })?;
            switched_to = Some(g);
        }
    }
    let final_group = switched_to.unwrap_or(group);
    let mut steps = Vec::new();
    let mut locked = group;
    let mut lock_to = |g: u8, steps: &mut Vec<Step>| {
        if locked != g {
            steps.push(Step::LockGroup(g));
            locked = g;
        }
    };
    lock_to(final_group, &mut steps);

    let backspace = Stroke {
        keycode: keymap.backspace(),
        shift: false,
        lvl3: false,
    };
    for cmd in cmds {
        let Command::Replace { backspaces, text } = cmd else {
            continue;
        };
        steps.extend(std::iter::repeat_n(Step::Tap(backspace), *backspaces));
        for ch in text.chars() {
            let others = (0..keymap.group_count()).filter(|&g| g != final_group);
            let found = std::iter::once(final_group)
                .chain(others)
                .find_map(|g| keymap.find(ch, g, caps_lock).map(|s| (g, s)));
            match found {
                Some((g, stroke)) => {
                    lock_to(g, &mut steps);
                    steps.push(Step::Tap(stroke));
                }
                None => steps.push(Step::Unicode(ch)),
            }
        }
    }
    lock_to(final_group, &mut steps);
    Ok(Plan {
        steps,
        final_group,
        switched_to,
    })
}

fn injection_error(e: impl std::fmt::Display) -> BackendError {
    BackendError::Injection(e.to_string())
}

/// A keycode with no symbols, used for the temporary-keysym fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpareKey {
    keycode: u8,
    keysyms_per_keycode: u8,
}

/// Find an unused keycode (the highest one with no keysyms).
pub(crate) fn find_spare_key(conn: &RustConnection) -> Result<Option<SpareKey>, BackendError> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let map = conn
        .get_keyboard_mapping(min, max - min + 1)
        .map_err(injection_error)?
        .reply()
        .map_err(injection_error)?;
    let per = usize::from(map.keysyms_per_keycode);
    if per == 0 {
        return Ok(None);
    }
    Ok(map
        .keysyms
        .chunks(per)
        .enumerate()
        .rev()
        .find(|(_, syms)| syms.iter().all(|&s| s == 0))
        .map(|(i, _)| SpareKey {
            keycode: min + i as u8,
            keysyms_per_keycode: map.keysyms_per_keycode,
        }))
}

/// Sends plans over XTEST.
pub(crate) struct Injector<'a> {
    pub conn: &'a RustConnection,
    pub root: Window,
    pub keymap: &'a Keymap,
    pub spare: Option<SpareKey>,
}

impl Injector<'_> {
    fn key(&self, press: bool, keycode: u8) -> Result<(), BackendError> {
        let kind = if press {
            xproto::KEY_PRESS_EVENT
        } else {
            xproto::KEY_RELEASE_EVENT
        };
        self.conn
            .xtest_fake_input(kind, keycode, x11rb::CURRENT_TIME, self.root, 0, 0, 0)
            .map_err(injection_error)?;
        Ok(())
    }

    fn lock_group(&self, group: u8) -> Result<(), BackendError> {
        self.conn
            .xkb_latch_lock_state(
                USE_CORE_KBD,
                xproto::ModMask::from(0u16),
                xproto::ModMask::from(0u16),
                true,
                xkb::Group::from(group),
                xproto::ModMask::from(0u16),
                false,
                0,
            )
            .map_err(|e| BackendError::LayoutSwitch(e.to_string()))?;
        Ok(())
    }

    fn tap(&self, s: Stroke, group: u8) -> Result<(), BackendError> {
        let shift = s.shift.then(|| self.keymap.shift_key(group)).flatten();
        let lvl3 = s.lvl3.then(|| self.keymap.lvl3_key(group)).flatten();
        for m in [shift, lvl3].into_iter().flatten() {
            self.key(true, m)?;
        }
        self.key(true, s.keycode)?;
        self.key(false, s.keycode)?;
        for m in [lvl3, shift].into_iter().flatten() {
            self.key(false, m)?;
        }
        Ok(())
    }

    fn bind_spare(&self, spare: SpareKey, keysym: u32) -> Result<(), BackendError> {
        let syms = vec![keysym; usize::from(spare.keysyms_per_keycode)];
        self.conn
            .change_keyboard_mapping(1, spare.keycode, spare.keysyms_per_keycode, &syms)
            .map_err(injection_error)?;
        tracking::sync(self.conn)?;
        thread::sleep(REMAP_SETTLE);
        Ok(())
    }

    /// Send every step, then wait until the server has processed them.
    pub fn run(&self, plan: &Plan, start_group: u8) -> Result<(), BackendError> {
        let mut group = start_group;
        let mut used_spare = None;
        let result = (|| {
            for step in &plan.steps {
                match *step {
                    Step::LockGroup(g) => {
                        self.lock_group(g)?;
                        group = g;
                    }
                    Step::Tap(s) => self.tap(s, group)?,
                    Step::Unicode(ch) => {
                        let spare = self.spare.ok_or_else(|| {
                            BackendError::Injection(
                                "no free keycode to type a character outside the layouts".into(),
                            )
                        })?;
                        let keysym = xkbcommon::xkb::utf32_to_keysym(u32::from(ch)).raw();
                        self.bind_spare(spare, keysym)?;
                        used_spare = Some(spare);
                        self.key(true, spare.keycode)?;
                        self.key(false, spare.keycode)?;
                        tracking::sync(self.conn)?;
                    }
                }
            }
            tracking::sync(self.conn)
        })();
        if let Some(spare) = used_spare {
            // Let the target read the key with the temporary mapping first.
            thread::sleep(REMAP_SETTLE);
            let restored = self.bind_spare(spare, 0);
            if result.is_ok() {
                restored?;
            }
        }
        result
    }
}

/// The desktop environment, for keeping its layout indicator in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Desktop {
    /// KDE Plasma: switch through `org.kde.keyboard` too.
    Kde,
    /// GNOME: no public D-Bus method to switch input sources.
    Gnome,
    /// Anything else: the XKB group lock is all there is.
    Other,
}

impl Desktop {
    /// Detect from `XDG_CURRENT_DESKTOP` (a colon-separated list).
    pub fn detect(current_desktop: Option<&str>) -> Desktop {
        let names = current_desktop.unwrap_or("");
        let has = |want: &str| {
            names
                .split(':')
                .any(|n| n.trim().eq_ignore_ascii_case(want))
        };
        if has("KDE") {
            Desktop::Kde
        } else if has("GNOME") {
            Desktop::Gnome
        } else {
            Desktop::Other
        }
    }
}

/// Tell KDE's keyboard daemon about a switch so its indicator follows.
pub(crate) fn kde_set_layout(
    bus: &mut Option<zbus::blocking::Connection>,
    group: u8,
) -> Result<(), String> {
    if bus.is_none() {
        *bus = Some(zbus::blocking::Connection::session().map_err(|e| e.to_string())?);
    }
    let conn = bus.as_ref().expect("connected above");
    let reply = conn
        .call_method(
            Some("org.kde.keyboard"),
            "/Layouts",
            Some("org.kde.KeyboardLayouts"),
            "setLayout",
            &(u32::from(group)),
        )
        .map_err(|e| e.to_string())?;
    let ok: bool = reply.body().deserialize().map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else {
        Err("org.kde.KeyboardLayouts.setLayout refused".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x11::keycodes::x_keycode;
    use crate::x11::keymap::RulesNames;
    use lf_core::{KeyCode, LayoutId};

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

    fn tap(code: KeyCode, shift: bool) -> Step {
        Step::Tap(Stroke {
            keycode: x_keycode(code),
            shift,
            lvl3: false,
        })
    }

    const BS: Step = Step::Tap(Stroke {
        keycode: 22,
        shift: false,
        lvl3: false,
    });

    #[test]
    fn fix_switches_first_then_types_in_the_target_group() {
        let km = us_ir();
        let cmds = [
            Command::Replace {
                backspaces: 3,
                text: "سلام".into(),
            },
            Command::SetLayout(LayoutId::new("ir")),
        ];
        let p = plan(&cmds, &km, 0, false).unwrap();
        assert_eq!(p.final_group, 1);
        assert_eq!(p.switched_to, Some(1));
        let mut want = vec![Step::LockGroup(1), BS, BS, BS];
        want.extend(
            [KeyCode::KeyS, KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyL].map(|k| tap(k, false)),
        );
        assert_eq!(p.steps, want);
        assert_eq!(p.presses(), 7);
    }

    #[test]
    fn undo_back_to_us_with_shift() {
        let km = us_ir();
        let cmds = [
            Command::Replace {
                backspaces: 1,
                text: "Hi".into(),
            },
            Command::SetLayout(LayoutId::new("us")),
        ];
        let p = plan(&cmds, &km, 1, false).unwrap();
        assert_eq!(
            p.steps,
            vec![
                Step::LockGroup(0),
                BS,
                tap(KeyCode::KeyH, true),
                tap(KeyCode::KeyI, false)
            ]
        );
        assert_eq!(p.presses(), 4);
    }

    #[test]
    fn without_a_switch_other_groups_are_borrowed_and_restored() {
        let km = us_ir();
        let cmds = [Command::Replace {
            backspaces: 0,
            text: "aس".into(),
        }];
        let p = plan(&cmds, &km, 0, false).unwrap();
        assert_eq!(p.final_group, 0);
        assert_eq!(p.switched_to, None);
        assert_eq!(
            p.steps,
            vec![
                tap(KeyCode::KeyA, false),
                Step::LockGroup(1),
                tap(KeyCode::KeyS, false),
                Step::LockGroup(0),
            ]
        );
    }

    #[test]
    fn unknown_chars_use_the_keysym_fallback() {
        let km = us_ir();
        let cmds = [Command::Replace {
            backspaces: 0,
            text: "☃".into(),
        }];
        let p = plan(&cmds, &km, 0, false).unwrap();
        assert_eq!(p.steps, vec![Step::Unicode('☃')]);
        assert_eq!(p.presses(), 1);
    }

    #[test]
    fn unknown_layout_is_a_switch_error() {
        let km = us_ir();
        let cmds = [Command::SetLayout(LayoutId::new("de"))];
        assert!(matches!(
            plan(&cmds, &km, 0, false),
            Err(BackendError::LayoutSwitch(_))
        ));
    }

    #[test]
    fn desktop_detection() {
        assert_eq!(Desktop::detect(Some("KDE")), Desktop::Kde);
        assert_eq!(Desktop::detect(Some("ubuntu:GNOME")), Desktop::Gnome);
        assert_eq!(Desktop::detect(Some("XFCE")), Desktop::Other);
        assert_eq!(Desktop::detect(None), Desktop::Other);
    }
}
