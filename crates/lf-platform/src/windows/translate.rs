//! Turning raw low-level hook events into engine input.
//!
//! Plain Rust with no OS calls, so it is unit-tested on every OS (see
//! `tests/windows_pure.rs`). The hook callback only forwards [`RawKey`]s;
//! a worker thread runs them through a [`Translator`].

use lf_core::{InputEvent, KeyCode, KeyPress, LayoutId, Modifiers, ResetReason};

use super::keymap::key_for_scan;

/// One key event as the low-level keyboard hook reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawKey {
    /// Virtual-key code.
    pub vk: u16,
    /// Hardware scan code (set 1).
    pub scan: u16,
    /// The extended-key flag (right-hand Ctrl/Alt, arrows, keypad Enter…).
    pub extended: bool,
    /// Key release rather than press (or auto-repeat).
    pub up: bool,
}

/// Virtual-key codes the translator cares about.
pub mod vk {
    /// Backspace.
    pub const BACK: u16 = 0x08;
    /// Tab.
    pub const TAB: u16 = 0x09;
    /// Enter.
    pub const RETURN: u16 = 0x0D;
    /// Shift (either side, when the side is not reported).
    pub const SHIFT: u16 = 0x10;
    /// Ctrl (either side).
    pub const CONTROL: u16 = 0x11;
    /// Alt (either side).
    pub const MENU: u16 = 0x12;
    /// Caps Lock.
    pub const CAPITAL: u16 = 0x14;
    /// Esc.
    pub const ESCAPE: u16 = 0x1B;
    /// Page Up.
    pub const PRIOR: u16 = 0x21;
    /// Down arrow.
    pub const DOWN: u16 = 0x28;
    /// Delete.
    pub const DELETE: u16 = 0x2E;
    /// Left Windows key.
    pub const LWIN: u16 = 0x5B;
    /// Right Windows key.
    pub const RWIN: u16 = 0x5C;
    /// Left Shift.
    pub const LSHIFT: u16 = 0xA0;
    /// Right Shift.
    pub const RSHIFT: u16 = 0xA1;
    /// Left Ctrl.
    pub const LCONTROL: u16 = 0xA2;
    /// Right Ctrl.
    pub const RCONTROL: u16 = 0xA3;
    /// Left Alt.
    pub const LMENU: u16 = 0xA4;
    /// Right Alt (AltGr on layouts that have it).
    pub const RMENU: u16 = 0xA5;
    /// An input method is processing the key.
    pub const PROCESSKEY: u16 = 0xE5;
    /// A Unicode character sent as a key (by another program).
    pub const PACKET: u16 = 0xE7;
}

/// Scan codes of keys that type text but have no [`KeyCode`]: the numeric
/// keypad and the extra ISO/ABNT/JIS keys. Typing one makes the buffer
/// unreliable, so it resets.
const UNTRACKED_TEXT_SCANS: [u16; 19] = [
    0x37, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x53, 0x56, 0x73,
    0x7D, 0x7E, 0x35,
];

/// Held modifiers and lock state, tracked from the event stream.
#[derive(Debug, Default, Clone)]
pub struct Translator {
    lshift: bool,
    rshift: bool,
    lctrl: bool,
    rctrl: bool,
    lalt: bool,
    ralt: bool,
    win: (bool, bool),
    caps_lock: bool,
    last_layout: Option<LayoutId>,
}

impl Translator {
    /// A translator with nothing held; `caps_lock` is the current toggle.
    pub fn new(caps_lock: bool) -> Self {
        Translator {
            caps_lock,
            ..Translator::default()
        }
    }

    /// Forget held modifiers (e.g. after the session was locked, when key
    /// releases may have been missed). Caps Lock and the layout are kept.
    pub fn release_all(&mut self) {
        *self = Translator {
            caps_lock: self.caps_lock,
            last_layout: self.last_layout.take(),
            ..Translator::default()
        };
    }

    /// Translate one key event. `layout` is the layout active in the
    /// focused window; `char_for` says what a key produces in a layout.
    ///
    /// A changed layout is reported as [`InputEvent::LayoutChanged`] before
    /// the key that revealed it.
    pub fn key(
        &mut self,
        k: RawKey,
        layout: &LayoutId,
        char_for: impl Fn(&LayoutId, KeyCode, Modifiers) -> Option<char>,
    ) -> Vec<InputEvent> {
        if self.track_modifier(k) {
            return Vec::new();
        }
        if k.up {
            return Vec::new();
        }
        if k.vk == vk::CAPITAL {
            self.caps_lock = !self.caps_lock;
            return Vec::new();
        }

        let ctrl = self.lctrl || self.rctrl;
        // Windows reports AltGr as Ctrl + right Alt.
        let altgr = self.ralt && ctrl;
        let chord =
            self.win.0 || self.win.1 || self.lalt || (ctrl && !altgr) || (self.ralt && !ctrl);
        if chord {
            return vec![InputEvent::Chord];
        }

        let reset = |r| vec![InputEvent::Reset(r)];
        match k.vk {
            vk::BACK => return vec![InputEvent::Backspace],
            vk::RETURN | vk::TAB | vk::ESCAPE => return reset(ResetReason::Submit),
            vk::PRIOR..=vk::DOWN | vk::DELETE => return reset(ResetReason::Navigation),
            vk::PACKET | vk::PROCESSKEY => return reset(ResetReason::Manual),
            _ => {}
        }

        let Some(code) = key_for_scan(k.scan, k.extended) else {
            if UNTRACKED_TEXT_SCANS.contains(&k.scan) {
                return reset(ResetReason::Manual);
            }
            return Vec::new();
        };

        let mut out = Vec::with_capacity(2);
        if self.last_layout.as_ref() != Some(layout) {
            self.last_layout = Some(layout.clone());
            out.push(InputEvent::LayoutChanged(layout.clone()));
        }
        let mods = Modifiers {
            shift: (self.lshift || self.rshift) ^ (self.caps_lock && is_letter(code)),
            altgr,
        };
        out.push(InputEvent::Key(KeyPress {
            code,
            mods,
            layout: layout.clone(),
            produced: char_for(layout, code, mods),
        }));
        out
    }

    /// Note that the system switched to `layout` without a key press (e.g.
    /// after this app asked for the switch), so no change is reported for it.
    pub fn assume_layout(&mut self, layout: LayoutId) {
        self.last_layout = Some(layout);
    }

    /// Update modifier state; true if `k` was a modifier key.
    fn track_modifier(&mut self, k: RawKey) -> bool {
        let down = !k.up;
        let slot = match k.vk {
            vk::LSHIFT => &mut self.lshift,
            vk::RSHIFT => &mut self.rshift,
            vk::SHIFT if k.scan == 0x36 => &mut self.rshift,
            vk::SHIFT => &mut self.lshift,
            vk::LCONTROL => &mut self.lctrl,
            vk::RCONTROL => &mut self.rctrl,
            vk::CONTROL if k.extended => &mut self.rctrl,
            vk::CONTROL => &mut self.lctrl,
            vk::LMENU => &mut self.lalt,
            vk::RMENU => &mut self.ralt,
            vk::MENU if k.extended => &mut self.ralt,
            vk::MENU => &mut self.lalt,
            vk::LWIN => &mut self.win.0,
            vk::RWIN => &mut self.win.1,
            _ => return false,
        };
        *slot = down;
        true
    }
}

/// Keys whose case Caps Lock flips. On the layouts Layout Fixer targets,
/// these are the letter keys.
fn is_letter(code: KeyCode) -> bool {
    code.as_w3c().starts_with("Key")
}

/// The app id for a process image path: the file name, lowercased.
pub fn app_id_from_image_path(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?.trim();
    (!name.is_empty()).then(|| name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::super::keymap::scan_for_key;
    use super::*;

    fn us() -> LayoutId {
        LayoutId::new("us")
    }

    fn chars(id: &LayoutId, code: KeyCode, mods: Modifiers) -> Option<char> {
        match id.as_str() {
            "us" => lf_layouts::us().char_for(code, mods),
            "ir" => lf_layouts::ir_pes().char_for(code, mods),
            _ => None,
        }
    }

    fn down(vk: u16, scan: u16) -> RawKey {
        RawKey {
            vk,
            scan,
            extended: false,
            up: false,
        }
    }

    fn up(vk: u16, scan: u16) -> RawKey {
        RawKey {
            up: true,
            ..down(vk, scan)
        }
    }

    fn letter(code: KeyCode) -> RawKey {
        let scan = scan_for_key(code);
        // A-Z virtual keys equal their ASCII capital.
        let vk = code.as_w3c().as_bytes()[3] as u16;
        down(vk, scan)
    }

    fn key_events(t: &mut Translator, k: RawKey, layout: &LayoutId) -> Vec<InputEvent> {
        t.key(k, layout, chars)
    }

    fn only_key(evs: &[InputEvent]) -> &KeyPress {
        match evs.last() {
            Some(InputEvent::Key(k)) => k,
            other => panic!("expected a key, got {other:?}"),
        }
    }

    #[test]
    fn first_key_reports_the_layout_then_only_changes() {
        let mut t = Translator::new(false);
        let evs = key_events(&mut t, letter(KeyCode::KeyS), &us());
        assert_eq!(evs[0], InputEvent::LayoutChanged(us()));
        assert_eq!(only_key(&evs).produced, Some('s'));

        let evs = key_events(&mut t, letter(KeyCode::KeyS), &us());
        assert_eq!(evs.len(), 1);

        let ir = LayoutId::new("ir");
        let evs = key_events(&mut t, letter(KeyCode::KeyS), &ir);
        assert_eq!(evs[0], InputEvent::LayoutChanged(ir.clone()));
        let k = only_key(&evs);
        assert_eq!(
            (k.code, k.layout.clone(), k.produced),
            (KeyCode::KeyS, ir, Some('س'))
        );
    }

    #[test]
    fn assume_layout_suppresses_the_change_event() {
        let mut t = Translator::new(false);
        t.assume_layout(LayoutId::new("ir"));
        let evs = key_events(&mut t, letter(KeyCode::KeyA), &LayoutId::new("ir"));
        assert_eq!(evs.len(), 1);
    }

    #[test]
    fn shift_and_caps_lock() {
        let mut t = Translator::new(false);
        t.assume_layout(us());
        assert!(key_events(&mut t, down(vk::LSHIFT, 0x2A), &us()).is_empty());
        let k = key_events(&mut t, letter(KeyCode::KeyA), &us());
        assert_eq!(only_key(&k).mods, Modifiers::SHIFT);
        assert_eq!(only_key(&k).produced, Some('A'));
        key_events(&mut t, up(vk::LSHIFT, 0x2A), &us());

        key_events(&mut t, down(vk::CAPITAL, 0x3A), &us());
        let k = key_events(&mut t, letter(KeyCode::KeyA), &us());
        assert_eq!(only_key(&k).produced, Some('A'), "caps lock shifts letters");
        let digit = down(0x31, scan_for_key(KeyCode::Digit1));
        let k = key_events(&mut t, digit, &us());
        assert_eq!(only_key(&k).produced, Some('1'), "but not digits");
        key_events(&mut t, down(vk::CAPITAL, 0x3A), &us());
        let k = key_events(&mut t, letter(KeyCode::KeyA), &us());
        assert_eq!(only_key(&k).produced, Some('a'));
    }

    #[test]
    fn ctrl_alt_and_win_make_chords() {
        for (vk, scan, extended) in [
            (vk::LCONTROL, 0x1D, false),
            (vk::RCONTROL, 0x1D, true),
            (vk::LMENU, 0x38, false),
            (vk::RMENU, 0x38, true),
            (vk::LWIN, 0x5B, true),
        ] {
            let mut t = Translator::new(false);
            let m = RawKey {
                vk,
                scan,
                extended,
                up: false,
            };
            assert!(key_events(&mut t, m, &us()).is_empty());
            assert_eq!(
                key_events(&mut t, letter(KeyCode::KeyC), &us()),
                [InputEvent::Chord],
                "vk {vk:#x}"
            );
            assert_eq!(
                key_events(&mut t, down(vk::BACK, 0x0E), &us()),
                [InputEvent::Chord]
            );
            key_events(&mut t, RawKey { up: true, ..m }, &us());
            assert!(matches!(
                key_events(&mut t, letter(KeyCode::KeyC), &us()).last(),
                Some(InputEvent::Key(_))
            ));
        }
    }

    #[test]
    fn altgr_is_a_modifier_not_a_chord() {
        let mut t = Translator::new(false);
        t.assume_layout(us());
        // Windows sends a fake left Ctrl before right Alt for AltGr.
        key_events(&mut t, down(vk::LCONTROL, 0x21D), &us());
        key_events(
            &mut t,
            RawKey {
                extended: true,
                ..down(vk::RMENU, 0x38)
            },
            &us(),
        );
        let evs = key_events(&mut t, letter(KeyCode::KeyE), &us());
        let k = only_key(&evs);
        assert!(k.mods.altgr && !k.mods.shift);
    }

    #[test]
    fn special_keys() {
        let mut t = Translator::new(false);
        let cases = [
            (down(vk::BACK, 0x0E), Some(InputEvent::Backspace)),
            (
                down(vk::RETURN, 0x1C),
                Some(InputEvent::Reset(ResetReason::Submit)),
            ),
            (
                down(vk::TAB, 0x0F),
                Some(InputEvent::Reset(ResetReason::Submit)),
            ),
            (
                down(vk::ESCAPE, 0x01),
                Some(InputEvent::Reset(ResetReason::Submit)),
            ),
            (
                down(0x25, 0x4B),
                Some(InputEvent::Reset(ResetReason::Navigation)),
            ),
            (
                down(0x24, 0x47),
                Some(InputEvent::Reset(ResetReason::Navigation)),
            ),
            (
                down(vk::DELETE, 0x53),
                Some(InputEvent::Reset(ResetReason::Navigation)),
            ),
            (
                down(vk::PACKET, 0),
                Some(InputEvent::Reset(ResetReason::Manual)),
            ),
            (
                down(vk::PROCESSKEY, 0x1E),
                Some(InputEvent::Reset(ResetReason::Manual)),
            ),
            // Keypad 7 with Num Lock on types text the engine can't track.
            (
                down(0x67, 0x47),
                Some(InputEvent::Reset(ResetReason::Manual)),
            ),
            // F1 is ignored; keypad Insert (Num Lock off) resets like the keypad.
            (down(0x70, 0x3B), None),
            (
                down(0x2D, 0x52),
                Some(InputEvent::Reset(ResetReason::Manual)),
            ),
        ];
        for (k, want) in cases {
            let got = key_events(&mut t, k, &us());
            assert_eq!(got.last(), want.as_ref(), "{k:?}");
        }
        assert!(
            key_events(&mut t, up(vk::BACK, 0x0E), &us()).is_empty(),
            "releases"
        );
    }

    #[test]
    fn release_all_clears_modifiers_only() {
        let mut t = Translator::new(true);
        t.assume_layout(us());
        key_events(&mut t, down(vk::LCONTROL, 0x1D), &us());
        t.release_all();
        let evs = key_events(&mut t, letter(KeyCode::KeyB), &us());
        assert_eq!(evs.len(), 1);
        assert_eq!(only_key(&evs).produced, Some('B'), "caps lock kept");
    }

    #[test]
    fn app_ids() {
        let p = r"C:\Program Files\Mozilla Firefox\Firefox.EXE";
        assert_eq!(app_id_from_image_path(p).as_deref(), Some("firefox.exe"));
        assert_eq!(
            app_id_from_image_path("code.exe").as_deref(),
            Some("code.exe")
        );
        assert_eq!(app_id_from_image_path(r"C:\dir\").as_deref(), None);
        assert_eq!(app_id_from_image_path("").as_deref(), None);
    }
}
