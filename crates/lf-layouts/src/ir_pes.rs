//! Persian Standard layout (ISIRI 9147).
//!
//! Source: `/usr/share/X11/xkb/symbols/ir`, section `xkb_symbols "pes"`
//! (the default Persian layout), levels 1 and 2 only. The Space key comes from
//! the `include "nbsp(zwnj2nb3nnb4)"` in that section (`/usr/share/X11/xkb/symbols/nbsp`).
//! Keysym code points were checked against `/usr/include/X11/keysymdef.h`.
//! AltGr (levels 3 and 4) is not included in Phase 1.

use lf_core::{KeyCode, Layout, Modifiers};

/// `(key, unshifted, shifted)`, in XKB order. Space comes first so that
/// reverse lookup of ZWNJ (also Shift+B) picks Shift+Space.
const TABLE: &[(KeyCode, char, char)] = &[
    // <SPCE>: space, U200C (ZWNJ)
    (KeyCode::Space, ' ', '\u{200C}'),
    // Persian digits
    (KeyCode::Digit1, '۱', '!'),
    (KeyCode::Digit2, '۲', '\u{066C}'), // Arabic thousands separator
    (KeyCode::Digit3, '۳', '\u{066B}'), // Arabic decimal separator
    (KeyCode::Digit4, '۴', '\u{FDFC}'), // Rial sign
    (KeyCode::Digit5, '۵', '\u{066A}'), // Arabic percent sign
    (KeyCode::Digit6, '۶', '×'),
    (KeyCode::Digit7, '۷', '،'),
    (KeyCode::Digit8, '۸', '*'),
    (KeyCode::Digit9, '۹', ')'),
    (KeyCode::Digit0, '۰', '('),
    // Persian letters and symbols
    (KeyCode::KeyQ, 'ض', '\u{0652}'), // sukun
    (KeyCode::KeyW, 'ص', '\u{064C}'), // dammatan
    (KeyCode::KeyE, 'ث', '\u{064D}'), // kasratan
    (KeyCode::KeyR, 'ق', '\u{064B}'), // fathatan
    (KeyCode::KeyT, 'ف', '\u{064F}'), // damma
    (KeyCode::KeyY, 'غ', '\u{0650}'), // kasra
    (KeyCode::KeyU, 'ع', '\u{064E}'), // fatha
    (KeyCode::KeyI, 'ه', '\u{0651}'), // shadda
    (KeyCode::KeyO, 'خ', ']'),
    (KeyCode::KeyP, 'ح', '['),
    (KeyCode::BracketLeft, 'ج', '}'),
    (KeyCode::BracketRight, 'چ', '{'),
    (KeyCode::KeyA, 'ش', 'ؤ'),
    (KeyCode::KeyS, 'س', 'ئ'),
    (KeyCode::KeyD, '\u{06CC}', '\u{064A}'), // Farsi yeh, Arabic yeh
    (KeyCode::KeyF, 'ب', 'إ'),
    (KeyCode::KeyG, 'ل', 'أ'),
    (KeyCode::KeyH, 'ا', 'آ'),
    (KeyCode::KeyJ, 'ت', 'ة'),
    (KeyCode::KeyK, 'ن', '»'),
    (KeyCode::KeyL, 'م', '«'),
    (KeyCode::Semicolon, '\u{06A9}', ':'), // keheh
    (KeyCode::Quote, 'گ', '؛'),
    (KeyCode::KeyZ, 'ظ', '\u{0643}'), // Arabic kaf
    (KeyCode::KeyX, 'ط', '\u{0653}'), // madda above
    (KeyCode::KeyC, 'ز', 'ژ'),
    (KeyCode::KeyV, 'ر', '\u{0670}'), // superscript alef
    (KeyCode::KeyB, 'ذ', '\u{200C}'), // ZWNJ
    (KeyCode::KeyN, 'د', '\u{0654}'), // hamza above
    (KeyCode::KeyM, 'پ', 'ء'),
    (KeyCode::Comma, 'و', '>'),
    (KeyCode::Period, '.', '<'),
    (KeyCode::Slash, '/', '؟'),
    (KeyCode::Backquote, '\u{200D}', '÷'), // ZWJ
    (KeyCode::Minus, '-', '\u{0640}'),     // tatweel
    (KeyCode::Equal, '=', '+'),
    (KeyCode::Backslash, '\\', '|'),
];

/// Persian Standard: id "ir", "Persian (Standard)".
pub fn ir_pes() -> Layout {
    let entries: Vec<_> = TABLE
        .iter()
        .flat_map(|&(code, plain, shifted)| {
            [
                (code, Modifiers::NONE, plain),
                (code, Modifiers::SHIFT, shifted),
            ]
        })
        .collect();
    Layout::new("ir", "Persian (Standard)", &entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::us;
    use lf_core::{render, KeyPress, LayoutId};

    #[test]
    fn letter_table() {
        let l = ir_pes();
        let table = [
            ("KeyQ", 'ض'),
            ("KeyW", 'ص'),
            ("KeyE", 'ث'),
            ("KeyR", 'ق'),
            ("KeyT", 'ف'),
            ("KeyY", 'غ'),
            ("KeyU", 'ع'),
            ("KeyI", 'ه'),
            ("KeyO", 'خ'),
            ("KeyP", 'ح'),
            ("BracketLeft", 'ج'),
            ("BracketRight", 'چ'),
            ("KeyA", 'ش'),
            ("KeyS", 'س'),
            ("KeyD", 'ی'),
            ("KeyF", 'ب'),
            ("KeyG", 'ل'),
            ("KeyH", 'ا'),
            ("KeyJ", 'ت'),
            ("KeyK", 'ن'),
            ("KeyL", 'م'),
            ("Semicolon", 'ک'),
            ("Quote", 'گ'),
            ("KeyZ", 'ظ'),
            ("KeyX", 'ط'),
            ("KeyC", 'ز'),
            ("KeyV", 'ر'),
            ("KeyB", 'ذ'),
            ("KeyN", 'د'),
            ("KeyM", 'پ'),
            ("Comma", 'و'),
        ];
        for (code, ch) in table {
            let code = KeyCode::from_w3c(code).unwrap();
            assert_eq!(l.char_for(code, Modifiers::NONE), Some(ch), "{code:?}");
        }
    }

    #[test]
    fn space_and_zwnj() {
        let l = ir_pes();
        assert_eq!(l.char_for(KeyCode::Space, Modifiers::NONE), Some(' '));
        assert_eq!(
            l.char_for(KeyCode::Space, Modifiers::SHIFT),
            Some('\u{200C}')
        );
        assert_eq!(
            l.key_for('\u{200C}'),
            Some((KeyCode::Space, Modifiers::SHIFT))
        );
    }

    #[test]
    fn persian_digits() {
        let l = ir_pes();
        let digits = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
            KeyCode::Digit0,
        ];
        let rendered: String = digits
            .iter()
            .map(|&d| l.char_for(d, Modifiers::NONE).unwrap())
            .collect();
        assert_eq!(rendered, "۱۲۳۴۵۶۷۸۹۰");
    }

    #[test]
    fn every_key_has_both_levels() {
        let l = ir_pes();
        for &code in KeyCode::ALL {
            assert!(l.char_for(code, Modifiers::NONE).is_some(), "{code:?}");
            assert!(l.char_for(code, Modifiers::SHIFT).is_some(), "{code:?}");
        }
    }

    #[test]
    fn sghl_renders_as_salam() {
        let us = us();
        let keys: Vec<KeyPress> = "sghl"
            .chars()
            .map(|ch| {
                let (code, mods) = us.key_for(ch).unwrap();
                KeyPress {
                    code,
                    mods,
                    layout: LayoutId::new("us"),
                    produced: Some(ch),
                }
            })
            .collect();
        assert_eq!(render(&keys, &ir_pes()), "سلام");
    }

    #[test]
    fn identity() {
        let l = ir_pes();
        assert_eq!(l.id.as_str(), "ir");
        assert_eq!(l.name, "Persian (Standard)");
    }
}
