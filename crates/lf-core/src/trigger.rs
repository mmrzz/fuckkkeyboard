//! Trigger key sequence matching.

use crate::buffer::KeyBuffer;
use crate::key::{KeyCode, KeyPress};

/// Minimum number of keys in a trigger.
pub const MIN_TRIGGER_LEN: usize = 4;

/// Error building a [`Trigger`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TriggerError {
    /// The trigger has fewer than [`MIN_TRIGGER_LEN`] keys.
    #[error("trigger needs at least {MIN_TRIGGER_LEN} keys, got {0}")]
    TooShort(usize),
}

/// The physical key sequence that asks for a correction.
///
/// Matching is on key codes, so it works whatever layout is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    keys: Vec<KeyCode>,
}

impl Trigger {
    /// Default: [KeyF, KeyU, KeyC, KeyK, KeyK, KeyK].
    pub fn default_trigger() -> Self {
        Trigger {
            keys: vec![
                KeyCode::KeyF,
                KeyCode::KeyU,
                KeyCode::KeyC,
                KeyCode::KeyK,
                KeyCode::KeyK,
                KeyCode::KeyK,
            ],
        }
    }

    /// Build a trigger. Errors if fewer than 4 keys.
    pub fn new(keys: Vec<KeyCode>) -> Result<Self, TriggerError> {
        if keys.len() < MIN_TRIGGER_LEN {
            return Err(TriggerError::TooShort(keys.len()));
        }
        Ok(Trigger { keys })
    }

    /// Number of keys in the trigger.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Always false: a trigger has at least [`MIN_TRIGGER_LEN`] keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The trigger keys in order.
    pub fn keys(&self) -> &[KeyCode] {
        &self.keys
    }

    /// True if `k` is the trigger's key at position `pos`, pressed with no modifiers.
    pub fn key_matches(&self, pos: usize, k: &KeyPress) -> bool {
        self.keys.get(pos) == Some(&k.code) && k.mods.is_none()
    }

    /// True if the buffer ends with the trigger keys, all pressed with no modifiers.
    pub fn matches_tail(&self, buffer: &KeyBuffer) -> bool {
        let n = self.keys.len();
        if buffer.len() < n {
            return false;
        }
        buffer
            .iter()
            .skip(buffer.len() - n)
            .enumerate()
            .all(|(i, k)| self.key_matches(i, k))
    }
}

impl Default for Trigger {
    fn default() -> Self {
        Trigger::default_trigger()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Modifiers;
    use crate::layout::LayoutId;

    fn press(code: KeyCode, mods: Modifiers, layout: &str) -> KeyPress {
        KeyPress {
            code,
            mods,
            layout: LayoutId::new(layout),
            produced: Some('?'),
        }
    }

    fn buffer_of(codes: &[KeyCode], layout: &str) -> KeyBuffer {
        let mut b = KeyBuffer::new();
        b.extend(codes.iter().map(|&c| press(c, Modifiers::NONE, layout)));
        b
    }

    fn fuckkk() -> Vec<KeyCode> {
        Trigger::default_trigger().keys().to_vec()
    }

    #[test]
    fn matches_in_us_and_ir() {
        let t = Trigger::default_trigger();
        assert!(t.matches_tail(&buffer_of(&fuckkk(), "us")));
        assert!(t.matches_tail(&buffer_of(&fuckkk(), "ir")));
    }

    #[test]
    fn too_short_does_not_match() {
        let t = Trigger::default_trigger();
        assert!(!t.matches_tail(&buffer_of(&fuckkk()[..5], "us")));
        assert!(!t.matches_tail(&KeyBuffer::new()));
    }

    #[test]
    fn shift_on_any_key_does_not_match() {
        let t = Trigger::default_trigger();
        for i in 0..t.len() {
            let mut b = KeyBuffer::new();
            for (j, &c) in fuckkk().iter().enumerate() {
                let mods = if i == j {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                b.push(press(c, mods, "us"));
            }
            assert!(!t.matches_tail(&b), "shift on key {i}");
        }
    }

    #[test]
    fn matches_after_other_text() {
        let t = Trigger::default_trigger();
        let mut codes = vec![KeyCode::KeyH, KeyCode::KeyI, KeyCode::Space];
        codes.extend(fuckkk());
        assert!(t.matches_tail(&buffer_of(&codes, "us")));
        codes.push(KeyCode::KeyX);
        assert!(!t.matches_tail(&buffer_of(&codes, "us")));
    }

    #[test]
    fn new_rejects_short_triggers() {
        let three = vec![KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC];
        assert_eq!(Trigger::new(three), Err(TriggerError::TooShort(3)));
        let four = vec![KeyCode::KeyA; 4];
        assert_eq!(Trigger::new(four).unwrap().len(), 4);
    }
}
