//! US English QWERTY layout.

use lf_core::{KeyCode, Layout, Modifiers};

/// `(key, unshifted, shifted)` for every key.
const TABLE: &[(KeyCode, char, char)] = &[
    (KeyCode::KeyA, 'a', 'A'),
    (KeyCode::KeyB, 'b', 'B'),
    (KeyCode::KeyC, 'c', 'C'),
    (KeyCode::KeyD, 'd', 'D'),
    (KeyCode::KeyE, 'e', 'E'),
    (KeyCode::KeyF, 'f', 'F'),
    (KeyCode::KeyG, 'g', 'G'),
    (KeyCode::KeyH, 'h', 'H'),
    (KeyCode::KeyI, 'i', 'I'),
    (KeyCode::KeyJ, 'j', 'J'),
    (KeyCode::KeyK, 'k', 'K'),
    (KeyCode::KeyL, 'l', 'L'),
    (KeyCode::KeyM, 'm', 'M'),
    (KeyCode::KeyN, 'n', 'N'),
    (KeyCode::KeyO, 'o', 'O'),
    (KeyCode::KeyP, 'p', 'P'),
    (KeyCode::KeyQ, 'q', 'Q'),
    (KeyCode::KeyR, 'r', 'R'),
    (KeyCode::KeyS, 's', 'S'),
    (KeyCode::KeyT, 't', 'T'),
    (KeyCode::KeyU, 'u', 'U'),
    (KeyCode::KeyV, 'v', 'V'),
    (KeyCode::KeyW, 'w', 'W'),
    (KeyCode::KeyX, 'x', 'X'),
    (KeyCode::KeyY, 'y', 'Y'),
    (KeyCode::KeyZ, 'z', 'Z'),
    (KeyCode::Digit1, '1', '!'),
    (KeyCode::Digit2, '2', '@'),
    (KeyCode::Digit3, '3', '#'),
    (KeyCode::Digit4, '4', '$'),
    (KeyCode::Digit5, '5', '%'),
    (KeyCode::Digit6, '6', '^'),
    (KeyCode::Digit7, '7', '&'),
    (KeyCode::Digit8, '8', '*'),
    (KeyCode::Digit9, '9', '('),
    (KeyCode::Digit0, '0', ')'),
    (KeyCode::Backquote, '`', '~'),
    (KeyCode::Minus, '-', '_'),
    (KeyCode::Equal, '=', '+'),
    (KeyCode::BracketLeft, '[', '{'),
    (KeyCode::BracketRight, ']', '}'),
    (KeyCode::Backslash, '\\', '|'),
    (KeyCode::Semicolon, ';', ':'),
    (KeyCode::Quote, '\'', '"'),
    (KeyCode::Comma, ',', '<'),
    (KeyCode::Period, '.', '>'),
    (KeyCode::Slash, '/', '?'),
    (KeyCode::Space, ' ', ' '),
];

/// US English QWERTY: id "us", "English (US)".
pub fn us() -> Layout {
    let entries: Vec<_> = TABLE
        .iter()
        .flat_map(|&(code, plain, shifted)| {
            [
                (code, Modifiers::NONE, plain),
                (code, Modifiers::SHIFT, shifted),
            ]
        })
        .collect();
    Layout::new("us", "English (US)", &entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_both_levels() {
        let l = us();
        for &code in KeyCode::ALL {
            assert!(l.char_for(code, Modifiers::NONE).is_some(), "{code:?}");
            assert!(l.char_for(code, Modifiers::SHIFT).is_some(), "{code:?}");
        }
    }

    #[test]
    fn reverse_lookup() {
        let l = us();
        assert_eq!(l.key_for('A'), Some((KeyCode::KeyA, Modifiers::SHIFT)));
        assert_eq!(l.key_for('a'), Some((KeyCode::KeyA, Modifiers::NONE)));
        assert_eq!(l.key_for(' '), Some((KeyCode::Space, Modifiers::NONE)));
        assert_eq!(
            l.key_for('{'),
            Some((KeyCode::BracketLeft, Modifiers::SHIFT))
        );
        assert_eq!(l.key_for('"'), Some((KeyCode::Quote, Modifiers::SHIFT)));
    }

    #[test]
    fn printable_ascii_round_trips() {
        let l = us();
        for ch in ' '..='~' {
            let (code, mods) = l.key_for(ch).unwrap_or_else(|| panic!("{ch:?}"));
            assert_eq!(l.char_for(code, mods), Some(ch));
        }
    }

    #[test]
    fn identity() {
        let l = us();
        assert_eq!(l.id.as_str(), "us");
        assert_eq!(l.name, "English (US)");
    }
}
