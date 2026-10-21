//! Word splitting and deciding where the wrong-layout text starts.

use crate::key::{KeyCode, KeyPress};
use crate::layout::{render, Layout};

/// One word of the candidate range, rendered both ways.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordView {
    /// Index into the candidate key slice (inclusive).
    pub start: usize,
    /// Index into the candidate key slice (exclusive).
    pub end: usize,
    /// As typed (active layout A).
    pub text_from: String,
    /// Rendered in the other layout B.
    pub text_to: String,
}

/// The words a scorer chooses from.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    /// Words of the candidate range, in order.
    pub words: &'a [WordView],
    /// The active layout the text was typed in.
    pub from: &'a Layout,
    /// The layout the text would be converted to.
    pub to: &'a Layout,
}

/// Decides where the wrong-layout suffix starts.
pub trait SplitScorer: Send {
    /// Return the index (into `words`) of the first word to convert,
    /// or None to convert nothing.
    fn first_wrong_word(&self, c: &Candidate) -> Option<usize>;
}

/// Phase 1 scorer: always converts only the last word.
#[derive(Debug, Clone, Copy, Default)]
pub struct LastWordScorer;

impl SplitScorer for LastWordScorer {
    fn first_wrong_word(&self, c: &Candidate) -> Option<usize> {
        c.words.len().checked_sub(1)
    }
}

/// Test helper: converts every word in the candidate range.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllWordsScorer;

impl SplitScorer for AllWordsScorer {
    fn first_wrong_word(&self, c: &Candidate) -> Option<usize> {
        (!c.words.is_empty()).then_some(0)
    }
}

/// True if `k` separates words: `Space` with no modifiers.
///
/// Shift+Space is not a break: on Persian Standard it types ZWNJ, which
/// belongs inside a word like `می‌خواهم`.
pub fn is_word_break(k: &KeyPress) -> bool {
    k.code == KeyCode::Space && k.mods.is_none()
}

/// Split `keys` into words (maximal runs of non-break keys) and render each
/// under `from` and `to`.
pub fn split_words(keys: &[KeyPress], from: &Layout, to: &Layout) -> Vec<WordView> {
    let mut words = Vec::new();
    let mut start = None;
    for (i, k) in keys.iter().enumerate() {
        match (is_word_break(k), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                words.push(word_view(keys, s, i, from, to));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        words.push(word_view(keys, s, keys.len(), from, to));
    }
    words
}

fn word_view(keys: &[KeyPress], start: usize, end: usize, from: &Layout, to: &Layout) -> WordView {
    WordView {
        start,
        end,
        text_from: render(&keys[start..end], from),
        text_to: render(&keys[start..end], to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Modifiers;
    use crate::layout::LayoutId;

    fn layout(id: &str, entries: &[(KeyCode, Modifiers, char)]) -> Layout {
        Layout::new(id, id, entries)
    }

    fn from() -> Layout {
        layout(
            "a",
            &[
                (KeyCode::KeyA, Modifiers::NONE, 'a'),
                (KeyCode::KeyB, Modifiers::NONE, 'b'),
                (KeyCode::Space, Modifiers::NONE, ' '),
                (KeyCode::Space, Modifiers::SHIFT, ' '),
            ],
        )
    }

    fn to() -> Layout {
        layout(
            "b",
            &[
                (KeyCode::KeyA, Modifiers::NONE, 'x'),
                (KeyCode::KeyB, Modifiers::NONE, 'y'),
                (KeyCode::Space, Modifiers::NONE, ' '),
                (KeyCode::Space, Modifiers::SHIFT, '\u{200C}'),
            ],
        )
    }

    fn keys(spec: &str) -> Vec<KeyPress> {
        spec.chars()
            .map(|c| {
                let (code, mods) = match c {
                    'a' => (KeyCode::KeyA, Modifiers::NONE),
                    'b' => (KeyCode::KeyB, Modifiers::NONE),
                    ' ' => (KeyCode::Space, Modifiers::NONE),
                    '^' => (KeyCode::Space, Modifiers::SHIFT),
                    _ => unreachable!(),
                };
                KeyPress {
                    code,
                    mods,
                    layout: LayoutId::new("a"),
                    produced: from().char_for(code, mods),
                }
            })
            .collect()
    }

    fn spans(spec: &str) -> Vec<(usize, usize)> {
        split_words(&keys(spec), &from(), &to())
            .iter()
            .map(|w| (w.start, w.end))
            .collect()
    }

    #[test]
    fn words_split_on_plain_space() {
        assert_eq!(spans("ab ba"), vec![(0, 2), (3, 5)]);
        assert_eq!(spans("  ab  b "), vec![(2, 4), (6, 7)]);
        assert_eq!(spans(""), vec![]);
        assert_eq!(spans("   "), vec![]);
    }

    #[test]
    fn shift_space_is_not_a_break() {
        assert_eq!(spans("ab^ba"), vec![(0, 5)]);
        let words = split_words(&keys("a^b a"), &from(), &to());
        assert_eq!(words[0].text_from, "a b");
        assert_eq!(words[0].text_to, "x\u{200C}y");
        assert_eq!(words[1].text_to, "x");
    }

    #[test]
    fn scorers() {
        let (f, t) = (from(), to());
        let words = split_words(&keys("ab b a"), &f, &t);
        let c = Candidate {
            words: &words,
            from: &f,
            to: &t,
        };
        assert_eq!(LastWordScorer.first_wrong_word(&c), Some(2));
        assert_eq!(AllWordsScorer.first_wrong_word(&c), Some(0));
        let empty = Candidate {
            words: &[],
            from: &f,
            to: &t,
        };
        assert_eq!(LastWordScorer.first_wrong_word(&empty), None);
        assert_eq!(AllWordsScorer.first_wrong_word(&empty), None);
    }
}
