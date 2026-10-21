//! Keyboard layouts and rendering keys as text.

use std::collections::HashMap;
use std::fmt;

use crate::key::{KeyCode, KeyPress, Modifiers};

/// Identifier of a keyboard layout, e.g. "us" or "ir".
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LayoutId(pub String);

impl LayoutId {
    /// Build an id from a string slice.
    pub fn new(id: &str) -> Self {
        LayoutId(id.to_owned())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LayoutId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for LayoutId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A keyboard layout: which character each key + modifier combination produces.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Short id, e.g. "us".
    pub id: LayoutId,
    /// Human-readable name, e.g. "English (US)".
    pub name: String,
    map: HashMap<(KeyCode, Modifiers), char>,
    reverse: HashMap<char, (KeyCode, Modifiers)>,
}

impl Layout {
    /// Build a layout from `(key, modifiers, char)` entries.
    ///
    /// A later entry for the same key and modifiers replaces an earlier one.
    /// For reverse lookup, when a char appears more than once the entry with
    /// fewer modifiers wins, and among equals the first entry wins.
    pub fn new(id: &str, name: &str, entries: &[(KeyCode, Modifiers, char)]) -> Self {
        let mut map = HashMap::with_capacity(entries.len());
        for &(code, mods, ch) in entries {
            map.insert((code, mods), ch);
        }
        let mut reverse: HashMap<char, (KeyCode, Modifiers)> = HashMap::new();
        for &(code, mods, ch) in entries {
            if map.get(&(code, mods)) != Some(&ch) {
                continue;
            }
            match reverse.get(&ch) {
                Some(&(_, old)) if mod_rank(old) <= mod_rank(mods) => {}
                _ => {
                    reverse.insert(ch, (code, mods));
                }
            }
        }
        Layout {
            id: LayoutId::new(id),
            name: name.to_owned(),
            map,
            reverse,
        }
    }

    /// Char this key produces in this layout.
    pub fn char_for(&self, code: KeyCode, mods: Modifiers) -> Option<char> {
        self.map.get(&(code, mods)).copied()
    }

    /// Reverse lookup, used by the simulator to "type" a string.
    /// Prefers no-shift over shift when a char appears twice.
    pub fn key_for(&self, ch: char) -> Option<(KeyCode, Modifiers)> {
        self.reverse.get(&ch).copied()
    }
}

fn mod_rank(m: Modifiers) -> u8 {
    u8::from(m.shift) + 2 * u8::from(m.altgr)
}

/// Render keys as text under `layout`. If a key has no mapping in `layout`,
/// fall back to the key's original `produced` char; skip keys with neither.
pub fn render(keys: &[KeyPress], layout: &Layout) -> String {
    keys.iter()
        .filter_map(|k| layout.char_for(k.code, k.mods).or(k.produced))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: Modifiers = Modifiers::NONE;
    const S: Modifiers = Modifiers::SHIFT;

    fn tiny() -> Layout {
        Layout::new(
            "t",
            "Tiny",
            &[
                (KeyCode::KeyA, S, 'A'),
                (KeyCode::KeyA, N, 'a'),
                (KeyCode::KeyB, N, 'b'),
                (KeyCode::KeyB, S, 'x'),
                (KeyCode::KeyC, S, 'x'),
                (KeyCode::Digit1, S, '!'),
                (KeyCode::KeyX, N, 'x'),
            ],
        )
    }

    fn press(code: KeyCode, mods: Modifiers, produced: Option<char>) -> KeyPress {
        KeyPress {
            code,
            mods,
            layout: LayoutId::new("t"),
            produced,
        }
    }

    #[test]
    fn char_for_and_key_for_round_trip() {
        let l = tiny();
        for ch in ['a', 'A', 'b', 'x', '!'] {
            let (code, mods) = l.key_for(ch).unwrap();
            assert_eq!(l.char_for(code, mods), Some(ch));
        }
        assert_eq!(l.char_for(KeyCode::KeyZ, N), None);
        assert_eq!(l.key_for('z'), None);
    }

    #[test]
    fn key_for_prefers_no_shift_then_first_entry() {
        let l = tiny();
        assert_eq!(l.key_for('x'), Some((KeyCode::KeyX, N)));
        assert_eq!(l.key_for('A'), Some((KeyCode::KeyA, S)));
        let only_shifted = Layout::new(
            "t",
            "T",
            &[(KeyCode::KeyC, S, 'x'), (KeyCode::KeyB, S, 'x')],
        );
        assert_eq!(only_shifted.key_for('x'), Some((KeyCode::KeyC, S)));
    }

    #[test]
    fn render_falls_back_to_produced_and_skips_empty() {
        let l = tiny();
        let keys = [
            press(KeyCode::KeyA, S, Some('?')),
            press(KeyCode::KeyZ, N, Some('z')),
            press(KeyCode::KeyQ, N, None),
            press(KeyCode::KeyB, N, None),
        ];
        assert_eq!(render(&keys, &l), "Azb");
        assert_eq!(render(&[], &l), "");
    }

    #[test]
    fn layout_id_formatting() {
        let id = LayoutId::new("ir");
        assert_eq!(format!("{id:?}"), "\"ir\"");
        assert_eq!(id.to_string(), "ir");
        assert_eq!(id.as_str(), "ir");
    }
}
