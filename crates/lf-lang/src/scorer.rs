//! The n-gram split scorer (spec sections 2.1–2.3).

use lf_core::{Candidate, LayoutId, SplitScorer};

use crate::model::LanguageModel;
use crate::patterns::is_pinned;

/// LLR given to pinned words, so they are never converted.
pub const PINNED_LLR: f64 = -50.0;

/// Chooses the split with per-word log-likelihood ratios from character
/// models, one per layout.
#[derive(Debug, Clone)]
pub struct NgramScorer {
    models: Vec<(LayoutId, LanguageModel)>,
}

impl NgramScorer {
    /// Build a scorer from `(layout, model of the language typed in it)` pairs.
    pub fn new(models: Vec<(LayoutId, LanguageModel)>) -> Self {
        NgramScorer { models }
    }

    /// The model for a layout, if any.
    pub fn model(&self, id: &LayoutId) -> Option<&LanguageModel> {
        self.models.iter().find(|(l, _)| l == id).map(|(_, m)| m)
    }

    /// Per-word LLRs, exposed for debugging and the CLI.
    ///
    /// Positive means the word was meant for the target layout. All zeros if
    /// either layout has no model.
    pub fn word_llrs(&self, c: &Candidate) -> Vec<f64> {
        let (Some(a), Some(b)) = (self.model(&c.from.id), self.model(&c.to.id)) else {
            return vec![0.0; c.words.len()];
        };
        c.words
            .iter()
            .map(|w| {
                if is_pinned(&w.text_from) {
                    return PINNED_LLR;
                }
                let from = a.language().normalize(&w.text_from);
                let to = b.language().normalize(&w.text_to);
                // Numbers and punctuation say nothing either way. A word with
                // letters on one side only (`,` typing `و`) is still scored.
                if from.is_empty() && to.is_empty() {
                    return 0.0;
                }
                calibrated(b, &to) - calibrated(a, &from)
            })
            .collect()
    }
}

/// `log2 P(word) - n * h`: how much likelier the word is than an average
/// word of its length in this language.
fn calibrated(m: &LanguageModel, normalized: &str) -> f64 {
    let n = normalized.chars().count() + 1;
    m.word_log2prob(normalized) - n as f64 * f64::from(m.h())
}

/// Index of the first word to convert, given per-word LLRs (spec 2.2).
///
/// Maximizes the suffix sum; ties go to the larger index. If no suffix has a
/// positive sum, only the last word is converted.
pub fn best_split(llrs: &[f64]) -> Option<usize> {
    let m = llrs.len();
    match m {
        0 => return None,
        1 => return Some(0),
        _ => {}
    }
    let (mut sum, mut best, mut best_j) = (0.0, f64::NEG_INFINITY, m - 1);
    for j in (0..m).rev() {
        sum += llrs[j];
        if sum > best {
            best = sum;
            best_j = j;
        }
    }
    Some(if best > 0.0 { best_j } else { m - 1 })
}

impl SplitScorer for NgramScorer {
    fn first_wrong_word(&self, c: &Candidate) -> Option<usize> {
        best_split(&self.word_llrs(c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::Language;
    use crate::model::count;
    use lf_core::{KeyCode, Layout, Modifiers, WordView};

    #[test]
    fn best_split_max_suffix() {
        assert_eq!(best_split(&[-5.0, -3.0, 4.0, 6.0]), Some(2));
        assert_eq!(best_split(&[-5.0, 8.0, -1.0, 6.0]), Some(1));
        assert_eq!(best_split(&[3.0, 2.0, 1.0]), Some(0));
        assert_eq!(best_split(&[-9.0, 10.0, -2.0]), Some(1));
    }

    #[test]
    fn best_split_fallbacks() {
        assert_eq!(best_split(&[]), None);
        assert_eq!(best_split(&[-20.0]), Some(0));
        assert_eq!(best_split(&[20.0]), Some(0));
        // Nothing positive: convert the last word.
        assert_eq!(best_split(&[-1.0, -2.0, -3.0]), Some(2));
        assert_eq!(best_split(&[0.0, 0.0]), Some(1));
        assert_eq!(best_split(&[5.0, -5.0]), Some(1));
    }

    #[test]
    fn best_split_ties_go_to_larger_index() {
        // Suffixes from 1 and 2 both sum to 4.
        assert_eq!(best_split(&[-1.0, 0.0, 4.0]), Some(2));
        assert_eq!(best_split(&[2.0, -2.0, 3.0]), Some(2));
        assert_eq!(best_split(&[-3.0, 2.0, -2.0, 2.0]), Some(3));
    }

    /// Two toy layouts, "lat" and "cyr", each with a model trained on words
    /// typed with the same keys.
    fn setup() -> (NgramScorer, Layout, Layout) {
        let keys = [
            (KeyCode::KeyA, 'a', 'ا'),
            (KeyCode::KeyB, 'b', 'ب'),
            (KeyCode::KeyC, 'c', 'ت'),
            (KeyCode::KeyD, 'd', 'د'),
            (KeyCode::Comma, ',', 'و'),
            (KeyCode::Period, '.', '.'),
            (KeyCode::Digit1, '1', '۱'),
            (KeyCode::Minus, '_', '-'),
            (KeyCode::Slash, '/', '/'),
            (KeyCode::Space, ' ', ' '),
        ];
        let lat: Vec<_> = keys
            .iter()
            .map(|&(k, l, _)| (k, Modifiers::NONE, l))
            .collect();
        let fa: Vec<_> = keys
            .iter()
            .map(|&(k, _, f)| (k, Modifiers::NONE, f))
            .collect();
        let en_words = [("abc", 50), ("cab", 30), ("bad", 20), ("dab", 10)];
        let fa_words = [("داب", 50), ("باد", 30), ("و", 40), ("تاب", 10)];
        let en = LanguageModel::from_data(count(Language::En, en_words, 4, 1));
        let mut fa_data = count(Language::Fa, fa_words, 4, 1);
        fa_data.h = -2.0;
        let mut en_data = en.data().clone();
        en_data.h = -2.0;
        let scorer = NgramScorer::new(vec![
            (LayoutId::new("lat"), LanguageModel::from_data(en_data)),
            (LayoutId::new("fa"), LanguageModel::from_data(fa_data)),
        ]);
        (
            scorer,
            Layout::new("lat", "lat", &lat),
            Layout::new("fa", "fa", &fa),
        )
    }

    fn word(from: &str, to: &str) -> WordView {
        WordView {
            start: 0,
            end: 0,
            text_from: from.into(),
            text_to: to.into(),
        }
    }

    fn llrs(words: &[WordView]) -> Vec<f64> {
        let (s, lat, fa) = setup();
        s.word_llrs(&Candidate {
            words,
            from: &lat,
            to: &fa,
        })
    }

    #[test]
    fn llr_sign_follows_language() {
        let l = llrs(&[word("abc", "ابت"), word("dab", "داب")]);
        assert!(l[0] < 0.0, "{l:?}");
        assert!(l[1] > 0.0, "{l:?}");
    }

    #[test]
    fn pinned_and_letterless_words() {
        let l = llrs(&[
            word("a_b", "ا-ب"),
            word("a/b", "ا/ب"),
            word("dab@x.c", "داب"),
            word("1.", "۱."),
            word(",", "و"),
        ]);
        assert_eq!(&l[..4], &[PINNED_LLR, PINNED_LLR, PINNED_LLR, 0.0]);
        // Letters on the target side only: still scored, and here strongly.
        assert!(l[4] > 0.0, "{l:?}");
    }

    #[test]
    fn scorer_splits_and_handles_missing_models() {
        let (s, lat, fa) = setup();
        let words = [word("abc", "ابت"), word("cab", "تاب"), word("dab", "داب")];
        let c = Candidate {
            words: &words,
            from: &lat,
            to: &fa,
        };
        assert_eq!(s.first_wrong_word(&c), Some(2));
        let empty = Candidate { words: &[], ..c };
        assert_eq!(s.first_wrong_word(&empty), None);

        let other = Layout::new("xx", "xx", &[]);
        let c = Candidate {
            words: &words,
            from: &other,
            to: &fa,
        };
        assert_eq!(s.word_llrs(&c), vec![0.0; 3]);
        assert_eq!(s.first_wrong_word(&c), Some(2));
    }

    #[test]
    fn long_and_odd_words_do_not_panic() {
        let long = "a".repeat(300);
        let l = llrs(&[
            word(&long, &"ا".repeat(300)),
            word("", ""),
            word("\u{200C}", "?"),
        ]);
        assert!(l.iter().all(|x| x.is_finite()), "{l:?}");
    }
}
