//! Key buffer mirroring the text on screen.

use std::collections::VecDeque;

use crate::key::KeyPress;

/// Maximum number of key presses kept in a [`KeyBuffer`].
pub const BUFFER_CAPACITY: usize = 256;

/// Ring buffer of recent key presses mirroring what is on screen.
#[derive(Debug, Clone, Default)]
pub struct KeyBuffer {
    keys: VecDeque<KeyPress>,
}

impl KeyBuffer {
    /// An empty buffer.
    pub fn new() -> Self {
        KeyBuffer {
            keys: VecDeque::with_capacity(BUFFER_CAPACITY),
        }
    }

    /// Append a key, dropping the oldest one past [`BUFFER_CAPACITY`].
    pub fn push(&mut self, k: KeyPress) {
        if self.keys.len() == BUFFER_CAPACITY {
            if let Some(mut old) = self.keys.pop_front() {
                old.produced = None;
            }
        }
        self.keys.push_back(k);
    }

    /// Remove the last key; no-op when empty.
    pub fn backspace(&mut self) {
        if let Some(mut k) = self.keys.pop_back() {
            k.produced = None;
        }
    }

    /// Remove every key, overwriting the typed chars first.
    pub fn clear(&mut self) {
        for k in self.keys.iter_mut() {
            k.produced = None;
        }
        self.keys.clear();
    }

    /// Number of keys held.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// True if no keys are held.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Iterate over the keys, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &KeyPress> + ExactSizeIterator {
        self.keys.iter()
    }

    /// Remove and return the last `n` entries (oldest first).
    /// Returns fewer if the buffer holds fewer than `n`.
    pub fn pop_tail(&mut self, n: usize) -> Vec<KeyPress> {
        let at = self.keys.len().saturating_sub(n);
        self.keys.split_off(at).into()
    }

    /// Append keys in order, as if each were pushed.
    pub fn extend(&mut self, keys: impl IntoIterator<Item = KeyPress>) {
        for k in keys {
            self.push(k);
        }
    }
}

impl Drop for KeyBuffer {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{KeyCode, Modifiers};
    use crate::layout::LayoutId;

    fn key(ch: char) -> KeyPress {
        KeyPress {
            code: KeyCode::KeyA,
            mods: Modifiers::NONE,
            layout: LayoutId::new("us"),
            produced: Some(ch),
        }
    }

    fn chars(b: &KeyBuffer) -> String {
        b.iter().filter_map(|k| k.produced).collect()
    }

    #[test]
    fn push_and_backspace() {
        let mut b = KeyBuffer::new();
        assert!(b.is_empty());
        b.push(key('a'));
        b.push(key('b'));
        assert_eq!(b.len(), 2);
        b.backspace();
        assert_eq!(chars(&b), "a");
        b.backspace();
        b.backspace();
        assert!(b.is_empty());
    }

    #[test]
    fn capacity_drops_oldest() {
        let mut b = KeyBuffer::new();
        for i in 0..BUFFER_CAPACITY + 10 {
            b.push(key(char::from_u32(0x100 + i as u32).unwrap()));
        }
        assert_eq!(b.len(), BUFFER_CAPACITY);
        assert_eq!(
            b.iter().next().unwrap().produced,
            char::from_u32(0x100 + 10)
        );
        assert_eq!(
            b.iter().last().unwrap().produced,
            char::from_u32(0x100 + BUFFER_CAPACITY as u32 + 9)
        );
    }

    #[test]
    fn pop_tail_returns_oldest_first() {
        let mut b = KeyBuffer::new();
        b.extend("abcde".chars().map(key));
        let tail = b.pop_tail(3);
        let tail: String = tail.iter().filter_map(|k| k.produced).collect();
        assert_eq!(tail, "cde");
        assert_eq!(chars(&b), "ab");
        assert_eq!(b.pop_tail(10).len(), 2);
        assert!(b.is_empty());
        assert!(b.pop_tail(1).is_empty());
    }

    #[test]
    fn clear_empties() {
        let mut b = KeyBuffer::new();
        b.extend("abc".chars().map(key));
        b.clear();
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }
}
