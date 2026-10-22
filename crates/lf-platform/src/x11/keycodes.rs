//! Linux evdev key codes and their [`KeyCode`] equivalents.
//!
//! X servers using the `evdev` XKB rules (every modern Xorg and Xvfb) number
//! keys as `evdev code + 8`. Codes come from `linux/input-event-codes.h`.

use lf_core::KeyCode;

/// X keycode = evdev code + this offset.
pub const EVDEV_OFFSET: u8 = 8;

/// `KEY_ESC`.
pub const ESC: u16 = 1;
/// `KEY_BACKSPACE`.
pub const BACKSPACE: u16 = 14;
/// `KEY_TAB`.
pub const TAB: u16 = 15;
/// `KEY_ENTER`.
pub const ENTER: u16 = 28;
/// `KEY_KPENTER`.
pub const KP_ENTER: u16 = 96;
/// `KEY_HOME`.
pub const HOME: u16 = 102;
/// `KEY_UP`.
pub const UP: u16 = 103;
/// `KEY_PAGEUP`.
pub const PAGE_UP: u16 = 104;
/// `KEY_LEFT`.
pub const LEFT: u16 = 105;
/// `KEY_RIGHT`.
pub const RIGHT: u16 = 106;
/// `KEY_END`.
pub const END: u16 = 107;
/// `KEY_DOWN`.
pub const DOWN: u16 = 108;
/// `KEY_PAGEDOWN`.
pub const PAGE_DOWN: u16 = 109;

/// `(evdev code, key)` for every [`KeyCode`].
const TABLE: &[(u16, KeyCode)] = &[
    (2, KeyCode::Digit1),
    (3, KeyCode::Digit2),
    (4, KeyCode::Digit3),
    (5, KeyCode::Digit4),
    (6, KeyCode::Digit5),
    (7, KeyCode::Digit6),
    (8, KeyCode::Digit7),
    (9, KeyCode::Digit8),
    (10, KeyCode::Digit9),
    (11, KeyCode::Digit0),
    (12, KeyCode::Minus),
    (13, KeyCode::Equal),
    (16, KeyCode::KeyQ),
    (17, KeyCode::KeyW),
    (18, KeyCode::KeyE),
    (19, KeyCode::KeyR),
    (20, KeyCode::KeyT),
    (21, KeyCode::KeyY),
    (22, KeyCode::KeyU),
    (23, KeyCode::KeyI),
    (24, KeyCode::KeyO),
    (25, KeyCode::KeyP),
    (26, KeyCode::BracketLeft),
    (27, KeyCode::BracketRight),
    (30, KeyCode::KeyA),
    (31, KeyCode::KeyS),
    (32, KeyCode::KeyD),
    (33, KeyCode::KeyF),
    (34, KeyCode::KeyG),
    (35, KeyCode::KeyH),
    (36, KeyCode::KeyJ),
    (37, KeyCode::KeyK),
    (38, KeyCode::KeyL),
    (39, KeyCode::Semicolon),
    (40, KeyCode::Quote),
    (41, KeyCode::Backquote),
    (43, KeyCode::Backslash),
    (44, KeyCode::KeyZ),
    (45, KeyCode::KeyX),
    (46, KeyCode::KeyC),
    (47, KeyCode::KeyV),
    (48, KeyCode::KeyB),
    (49, KeyCode::KeyN),
    (50, KeyCode::KeyM),
    (51, KeyCode::Comma),
    (52, KeyCode::Period),
    (53, KeyCode::Slash),
    (57, KeyCode::Space),
];

/// The key with this evdev code, if it is one the engine knows.
pub fn key_code(evdev: u16) -> Option<KeyCode> {
    TABLE.iter().find(|&&(e, _)| e == evdev).map(|&(_, k)| k)
}

/// The evdev code of `key`.
pub fn evdev_code(key: KeyCode) -> u16 {
    TABLE
        .iter()
        .find(|&&(_, k)| k == key)
        .map(|&(e, _)| e)
        .expect("every KeyCode is in the evdev table")
}

/// The X keycode of `key` (evdev code + 8).
pub fn x_keycode(key: KeyCode) -> u8 {
    // Every table entry is below 248, so this cannot overflow.
    evdev_code(key) as u8 + EVDEV_OFFSET
}

/// The evdev code of an X keycode, if it has one.
pub fn evdev_from_x(keycode: u32) -> Option<u16> {
    keycode
        .checked_sub(u32::from(EVDEV_OFFSET))
        .and_then(|e| u16::try_from(e).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_round_trips() {
        for &key in KeyCode::ALL {
            let e = evdev_code(key);
            assert_eq!(key_code(e), Some(key), "{key:?}");
            assert_eq!(evdev_from_x(u32::from(x_keycode(key))), Some(e), "{key:?}");
        }
        assert_eq!(TABLE.len(), KeyCode::ALL.len());
    }

    #[test]
    fn codes_are_unique() {
        for (i, &(a, ka)) in TABLE.iter().enumerate() {
            for &(b, kb) in &TABLE[i + 1..] {
                assert_ne!(a, b, "{ka:?} and {kb:?} share a code");
                assert_ne!(ka, kb);
            }
        }
    }

    #[test]
    fn known_codes() {
        // The keys the "sghl" + trigger integration test replays.
        assert_eq!(x_keycode(KeyCode::KeyS), 39);
        assert_eq!(x_keycode(KeyCode::KeyG), 42);
        assert_eq!(x_keycode(KeyCode::KeyA), 38);
        assert_eq!(x_keycode(KeyCode::Space), 65);
        assert_eq!(key_code(BACKSPACE), None);
        assert_eq!(key_code(ESC), None);
        assert_eq!(evdev_from_x(3), None);
    }
}
