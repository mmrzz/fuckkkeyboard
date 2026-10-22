//! Key map: physical keys and the characters each Windows layout produces.
//!
//! Everything here is plain Rust with no OS calls, so it is unit-tested on
//! every OS (see `tests/windows_pure.rs`). The FFI side probes each layout
//! with `ToUnicodeEx` and feeds the results to [`build_layout`].

use lf_core::{KeyCode, Layout, Modifiers};

/// Scan code set 1 (what `KBDLLHOOKSTRUCT::scanCode` reports) for every
/// [`KeyCode`]. All of these keys are non-extended.
pub const SCAN_CODES: [(KeyCode, u16); 48] = [
    (KeyCode::Backquote, 0x29),
    (KeyCode::Digit1, 0x02),
    (KeyCode::Digit2, 0x03),
    (KeyCode::Digit3, 0x04),
    (KeyCode::Digit4, 0x05),
    (KeyCode::Digit5, 0x06),
    (KeyCode::Digit6, 0x07),
    (KeyCode::Digit7, 0x08),
    (KeyCode::Digit8, 0x09),
    (KeyCode::Digit9, 0x0A),
    (KeyCode::Digit0, 0x0B),
    (KeyCode::Minus, 0x0C),
    (KeyCode::Equal, 0x0D),
    (KeyCode::KeyQ, 0x10),
    (KeyCode::KeyW, 0x11),
    (KeyCode::KeyE, 0x12),
    (KeyCode::KeyR, 0x13),
    (KeyCode::KeyT, 0x14),
    (KeyCode::KeyY, 0x15),
    (KeyCode::KeyU, 0x16),
    (KeyCode::KeyI, 0x17),
    (KeyCode::KeyO, 0x18),
    (KeyCode::KeyP, 0x19),
    (KeyCode::BracketLeft, 0x1A),
    (KeyCode::BracketRight, 0x1B),
    (KeyCode::KeyA, 0x1E),
    (KeyCode::KeyS, 0x1F),
    (KeyCode::KeyD, 0x20),
    (KeyCode::KeyF, 0x21),
    (KeyCode::KeyG, 0x22),
    (KeyCode::KeyH, 0x23),
    (KeyCode::KeyJ, 0x24),
    (KeyCode::KeyK, 0x25),
    (KeyCode::KeyL, 0x26),
    (KeyCode::Semicolon, 0x27),
    (KeyCode::Quote, 0x28),
    (KeyCode::Backslash, 0x2B),
    (KeyCode::KeyZ, 0x2C),
    (KeyCode::KeyX, 0x2D),
    (KeyCode::KeyC, 0x2E),
    (KeyCode::KeyV, 0x2F),
    (KeyCode::KeyB, 0x30),
    (KeyCode::KeyN, 0x31),
    (KeyCode::KeyM, 0x32),
    (KeyCode::Comma, 0x33),
    (KeyCode::Period, 0x34),
    (KeyCode::Slash, 0x35),
    (KeyCode::Space, 0x39),
];

/// The physical key for a scan code, or None for keys the engine does not
/// track (function keys, the numeric keypad, extended keys…).
pub fn key_for_scan(scan: u16, extended: bool) -> Option<KeyCode> {
    if extended {
        return None;
    }
    SCAN_CODES
        .iter()
        .find(|&&(_, s)| s == scan)
        .map(|&(code, _)| code)
}

/// The scan code of a physical key.
pub fn scan_for_key(code: KeyCode) -> u16 {
    SCAN_CODES
        .iter()
        .find(|&&(c, _)| c == code)
        .map(|&(_, s)| s)
        .expect("SCAN_CODES covers every KeyCode")
}

/// Every modifier combination probed per key.
pub const MOD_STATES: [Modifiers; 4] = [
    Modifiers::NONE,
    Modifiers::SHIFT,
    Modifiers {
        shift: false,
        altgr: true,
    },
    Modifiers {
        shift: true,
        altgr: true,
    },
];

const VK_SHIFT: usize = 0x10;
const VK_CONTROL: usize = 0x11;
const VK_MENU: usize = 0x12;
const VK_LSHIFT: usize = 0xA0;
const VK_LCONTROL: usize = 0xA2;
const VK_RMENU: usize = 0xA5;

/// The 256-byte key state array `ToUnicodeEx` expects for `mods`.
/// AltGr is Ctrl + right Alt, as Windows reports it.
pub fn key_state_for(mods: Modifiers) -> [u8; 256] {
    let mut state = [0u8; 256];
    if mods.shift {
        state[VK_SHIFT] = 0x80;
        state[VK_LSHIFT] = 0x80;
    }
    if mods.altgr {
        for vk in [VK_CONTROL, VK_MENU, VK_LCONTROL, VK_RMENU] {
            state[vk] = 0x80;
        }
    }
    state
}

/// Interpret a `ToUnicodeEx` result: `ret` is its return value, `buf` the
/// UTF-16 output buffer.
///
/// Only a single printable character counts. Dead keys (`ret < 0`), keys
/// that produce nothing, control characters and multi-character output
/// (ligature keys) give None: the engine models one character per key.
pub fn char_from_to_unicode(ret: i32, buf: &[u16]) -> Option<char> {
    if ret != 1 {
        return None;
    }
    let unit = *buf.first()?;
    let ch = char::from_u32(u32::from(unit))?;
    (!ch.is_control()).then_some(ch)
}

/// Build a layout by asking `probe` what each key produces under each
/// modifier combination in [`MOD_STATES`].
pub fn build_layout(
    id: &str,
    name: &str,
    mut probe: impl FnMut(KeyCode, Modifiers) -> Option<char>,
) -> Layout {
    let mut entries = Vec::new();
    for &(code, _) in &SCAN_CODES {
        for mods in MOD_STATES {
            if let Some(ch) = probe(code, mods) {
                entries.push((code, mods, ch));
            }
        }
    }
    Layout::new(id, name, &entries)
}

/// The keyboard layout id (KLID, e.g. `0x0000_0409`) behind an input
/// locale handle (HKL).
///
/// The HKL's high word is either the KLID's low word (a plain layout) or,
/// when its top nibble is `0xF`, a "Layout Id" that `layout_id_lookup`
/// resolves to a KLID through the registry. The low word is the language.
pub fn klid_for_hkl(hkl: usize, layout_id_lookup: impl Fn(u16) -> Option<u32>) -> u32 {
    // HKLs are handle-sized; the information is in the low 32 bits.
    let device = ((hkl >> 16) & 0xFFFF) as u16;
    let lang = (hkl & 0xFFFF) as u16;
    if device & 0xF000 == 0xF000 {
        if let Some(klid) = layout_id_lookup(device & 0x0FFF) {
            return klid;
        }
        return u32::from(lang);
    }
    if device == 0 {
        return u32::from(lang);
    }
    if device & 0xF000 == 0xE000 {
        // Input method editors: the KLID is the whole HKL, e.g. E0010804.
        return (u32::from(device) << 16) | u32::from(lang);
    }
    u32::from(device)
}

/// The language id (LANGID) half of an HKL.
pub fn langid_for_hkl(hkl: usize) -> u16 {
    (hkl & 0xFFFF) as u16
}

/// Parse a registry KLID string such as `"00000409"` or a "Layout Id" such
/// as `"00c0"` (both hexadecimal).
pub fn parse_hex_id(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || s.len() > 8 {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

/// The layout id used in settings for a KLID: `us` and `ir` for the layouts
/// matching the built-in tables, otherwise the KLID as 8 hex digits.
pub fn layout_id_for_klid(klid: u32) -> String {
    match klid {
        0x0000_0409 => "us".into(),
        0x0005_0429 => "ir".into(),
        _ => format!("{klid:08x}"),
    }
}

/// Two-letter language code for a LANGID, for the languages Layout Fixer
/// can tell apart (only those with a model are used for scoring).
pub fn language_for_langid(langid: u16) -> Option<&'static str> {
    let code = match langid & 0x03FF {
        0x01 => "ar",
        0x07 => "de",
        0x08 => "el",
        0x09 => "en",
        0x0A => "es",
        0x0C => "fr",
        0x0D => "he",
        0x10 => "it",
        0x16 => "pt",
        0x19 => "ru",
        0x1F => "tr",
        0x20 => "ur",
        0x22 => "uk",
        0x29 => "fa",
        _ => return None,
    };
    Some(code)
}

/// Make layout ids unique: when two input locales share a keyboard layout
/// (e.g. US keys under both English and Persian), later ones get the
/// LANGID appended, e.g. `us-0429`.
pub fn unique_ids(raw: &[(String, u16)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for (id, langid) in raw {
        let mut candidate = id.clone();
        if out.contains(&candidate) {
            candidate = format!("{id}-{langid:04x}");
        }
        let mut n = 2;
        while out.contains(&candidate) {
            candidate = format!("{id}-{langid:04x}-{n}");
            n += 1;
        }
        out.push(candidate);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn scan_table_covers_every_key_once() {
        let codes: HashSet<_> = SCAN_CODES.iter().map(|&(c, _)| c).collect();
        let scans: HashSet<_> = SCAN_CODES.iter().map(|&(_, s)| s).collect();
        assert_eq!(codes.len(), KeyCode::ALL.len());
        assert_eq!(scans.len(), KeyCode::ALL.len());
    }

    #[test]
    fn scan_code_round_trip() {
        for &code in KeyCode::ALL {
            let scan = scan_for_key(code);
            assert_eq!(key_for_scan(scan, false), Some(code), "{code:?}");
            assert_eq!(key_for_scan(scan, true), None, "{code:?} extended");
        }
    }

    #[test]
    fn scan_code_examples() {
        assert_eq!(key_for_scan(0x21, false), Some(KeyCode::KeyF));
        assert_eq!(key_for_scan(0x39, false), Some(KeyCode::Space));
        assert_eq!(scan_for_key(KeyCode::KeyA), 0x1E);
        assert_eq!(key_for_scan(0x1C, false), None, "Enter");
        assert_eq!(key_for_scan(0x3B, false), None, "F1");
        // Extended 0x35 is keypad divide, not Slash.
        assert_eq!(key_for_scan(0x35, true), None);
    }

    #[test]
    fn key_state_sets_the_right_virtual_keys() {
        assert!(key_state_for(Modifiers::NONE).iter().all(|&b| b == 0));
        let s = key_state_for(Modifiers::SHIFT);
        assert_eq!((s[VK_SHIFT], s[VK_CONTROL], s[VK_MENU]), (0x80, 0, 0));
        let g = key_state_for(MOD_STATES[3]);
        for vk in [VK_SHIFT, VK_CONTROL, VK_MENU, VK_RMENU] {
            assert_eq!(g[vk], 0x80);
        }
    }

    #[test]
    fn to_unicode_results() {
        assert_eq!(char_from_to_unicode(1, &[0x0634, 0]), Some('ش'));
        assert_eq!(char_from_to_unicode(1, &['a' as u16]), Some('a'));
        assert_eq!(char_from_to_unicode(-1, &['^' as u16]), None, "dead key");
        assert_eq!(char_from_to_unicode(0, &[0]), None);
        assert_eq!(char_from_to_unicode(2, &[0x0644, 0x0627]), None);
        assert_eq!(char_from_to_unicode(1, &[0x1B]), None, "control");
        assert_eq!(char_from_to_unicode(1, &[0xD83D]), None, "lone surrogate");
        assert_eq!(char_from_to_unicode(1, &[]), None);
    }

    #[test]
    fn build_layout_matches_builtin_us() {
        let us = lf_layouts::us();
        let built = build_layout("us", "US", |code, mods| us.char_for(code, mods));
        for &code in KeyCode::ALL {
            for mods in MOD_STATES {
                assert_eq!(built.char_for(code, mods), us.char_for(code, mods));
            }
        }
        assert_eq!(built.key_for('f'), Some((KeyCode::KeyF, Modifiers::NONE)));
    }

    #[test]
    fn build_layout_skips_keys_that_produce_nothing() {
        let l = build_layout("x", "X", |code, mods| {
            (code == KeyCode::KeyA && mods.is_none()).then_some('ا')
        });
        assert_eq!(l.char_for(KeyCode::KeyA, Modifiers::NONE), Some('ا'));
        assert_eq!(l.char_for(KeyCode::KeyA, Modifiers::SHIFT), None);
        assert_eq!(l.char_for(KeyCode::KeyB, Modifiers::NONE), None);
    }

    #[test]
    fn hkl_decoding() {
        let none = |_| None;
        // US English: 0x04090409.
        assert_eq!(klid_for_hkl(0x0409_0409, none), 0x0409);
        // Persian language with the plain Persian layout.
        assert_eq!(klid_for_hkl(0x0429_0429, none), 0x0429);
        // English language, US-International via a special layout id.
        let lookup = |id: u16| (id == 0x001).then_some(0x0002_0409);
        assert_eq!(klid_for_hkl(0xF001_0409, lookup), 0x0002_0409);
        // Unknown special id falls back to the language's default.
        assert_eq!(klid_for_hkl(0xF0FF_0429, lookup), 0x0429);
        // Sign-extended 64-bit handle: only the low 32 bits matter.
        let hkl64 = 0xFFFF_FFFF_F03A_0429_u64 as usize;
        let persian_std = |id: u16| (id == 0x03A).then_some(0x0005_0429);
        assert_eq!(klid_for_hkl(hkl64, persian_std), 0x0005_0429);
        assert_eq!(langid_for_hkl(hkl64), 0x0429);
        // A Chinese input method.
        assert_eq!(klid_for_hkl(0xE001_0804, none), 0xE001_0804);
        // UK keys under the en-US language.
        assert_eq!(klid_for_hkl(0x0809_0409, none), 0x0809);
    }

    #[test]
    fn hex_ids() {
        assert_eq!(parse_hex_id("00000409"), Some(0x0409));
        assert_eq!(parse_hex_id("00050429"), Some(0x0005_0429));
        assert_eq!(parse_hex_id("00c0"), Some(0xC0));
        assert_eq!(parse_hex_id(" 003a\0".trim_end_matches('\0')), Some(0x3A));
        assert_eq!(parse_hex_id(""), None);
        assert_eq!(parse_hex_id("xyz"), None);
        assert_eq!(parse_hex_id("123456789"), None);
    }

    #[test]
    fn layout_ids_and_languages() {
        assert_eq!(layout_id_for_klid(0x0409), "us");
        assert_eq!(layout_id_for_klid(0x0005_0429), "ir");
        assert_eq!(layout_id_for_klid(0x0429), "00000429");
        assert_eq!(layout_id_for_klid(0x0001_0409), "00010409");
        assert_eq!(language_for_langid(0x0409), Some("en"));
        assert_eq!(language_for_langid(0x0809), Some("en"), "en-GB");
        assert_eq!(language_for_langid(0x0429), Some("fa"));
        assert_eq!(language_for_langid(0x0411), None, "ja");
    }

    #[test]
    fn duplicate_ids_get_the_language_appended() {
        let raw = vec![
            ("us".to_string(), 0x0409),
            ("ir".to_string(), 0x0429),
            ("us".to_string(), 0x0429),
            ("us".to_string(), 0x0429),
        ];
        assert_eq!(unique_ids(&raw), ["us", "ir", "us-0429", "us-0429-2"]);
    }
}
