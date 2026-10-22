//! Character n-gram language model with interpolated Witten-Bell smoothing
//! (spec section 2.5).

use std::collections::HashMap;

use crate::alphabet::{Language, END, PAD};
use crate::format::{
    self, context_of, key_len, pack_key, word_hash, ModelData, ModelError, MAX_ORDER,
};

/// A parsed `.lfng` model.
#[derive(Debug, Clone)]
pub struct LanguageModel {
    data: ModelData,
}

impl LanguageModel {
    /// Parse and validate a model file.
    pub fn from_bytes(b: &[u8]) -> Result<Self, ModelError> {
        format::decode(b).map(LanguageModel::from_data)
    }

    /// Wrap already-decoded model data.
    pub fn from_data(data: ModelData) -> Self {
        LanguageModel { data }
    }

    /// The model's language.
    pub fn language(&self) -> Language {
        self.data.lang
    }

    /// Average log2 probability per char on held-out text.
    pub fn h(&self) -> f32 {
        self.data.h
    }

    /// N-gram order.
    pub fn order(&self) -> usize {
        usize::from(self.data.order)
    }

    /// The raw tables.
    pub fn data(&self) -> &ModelData {
        &self.data
    }

    /// log2 P of a normalized word, including ^ padding and $ end.
    ///
    /// Chars outside the alphabet are scored as `?`.
    pub fn word_log2prob(&self, normalized: &str) -> f64 {
        let lang = self.data.lang;
        let pad = self.order() - 1;
        let mut syms = vec![PAD; pad];
        syms.extend(normalized.chars().map(|c| lang.symbol(c)));
        syms.push(END);
        (pad..syms.len())
            .map(|i| self.prob(&syms[i - pad..i], syms[i]).log2())
            .sum()
    }

    /// P(c | history). Only the last `order - 1` symbols of `history` are used.
    pub fn prob(&self, history: &[u8], c: u8) -> f64 {
        let history = &history[history.len().saturating_sub(self.order() - 1)..];
        let mut p = 1.0 / self.data.lang.alphabet().len() as f64;
        // From the empty context up to the full history, each step
        // interpolating with the shorter context's estimate.
        let mut gram = Vec::with_capacity(history.len() + 1);
        for k in 0..=history.len() {
            gram.clear();
            gram.extend_from_slice(&history[history.len() - k..]);
            let Some((total, types)) = self.context(pack_key(&gram)) else {
                continue;
            };
            gram.push(c);
            let count = self.ngram(pack_key(&gram));
            p = (f64::from(count) + f64::from(types) * p) / (f64::from(total) + f64::from(types));
        }
        p
    }

    /// Frequency of a normalized word in the model's word list, in
    /// occurrences per million tokens. None if the word is not listed.
    pub fn word_ppm(&self, normalized: &str) -> Option<f64> {
        let words = &self.data.words;
        if self.data.word_total == 0 || normalized.is_empty() {
            return None;
        }
        let i = words
            .binary_search_by_key(&word_hash(normalized), |&(h, _)| h)
            .ok()?;
        Some(f64::from(words[i].1) * 1e6 / self.data.word_total as f64)
    }

    fn ngram(&self, key: u32) -> u32 {
        let ngrams = &self.data.ngrams;
        ngrams
            .binary_search_by_key(&key, |&(k, _)| k)
            .map_or(0, |i| ngrams[i].1)
    }

    /// `(C(h), T(h))`, or None if the context was never seen.
    fn context(&self, key: u32) -> Option<(u32, u32)> {
        let contexts = &self.data.contexts;
        let i = contexts.binary_search_by_key(&key, |&(k, _, _)| k).ok()?;
        let (_, total, types) = contexts[i];
        (total > 0).then_some((total, types))
    }
}

/// Count n-grams of `(normalized word, frequency)` pairs and build model
/// tables with `h = 0` (the caller measures `h` on held-out text).
///
/// `order` is clamped to 1..=4. N-grams of full order with a count below
/// `prune` are dropped after the context totals are computed. Empty words
/// are skipped.
pub fn count<'a>(
    lang: Language,
    words: impl IntoIterator<Item = (&'a str, u32)>,
    order: u8,
    prune: u32,
) -> ModelData {
    let order = order.clamp(1, MAX_ORDER);
    let pad = usize::from(order) - 1;
    let mut ngrams: HashMap<u32, u32> = HashMap::new();
    let mut syms = Vec::new();
    for (word, freq) in words {
        if word.is_empty() || freq == 0 {
            continue;
        }
        syms.clear();
        syms.resize(pad, PAD);
        syms.extend(word.chars().map(|c| lang.symbol(c)));
        syms.push(END);
        for i in pad..syms.len() {
            for k in 0..=pad {
                let key = pack_key(&syms[i - k..=i]);
                let n = ngrams.entry(key).or_insert(0);
                *n = n.saturating_add(freq);
            }
        }
    }

    let mut contexts: HashMap<u32, (u32, u32)> = HashMap::new();
    for (&key, &n) in &ngrams {
        let ctx = contexts.entry(context_of(key)).or_insert((0, 0));
        ctx.0 = ctx.0.saturating_add(n);
        ctx.1 += 1;
    }

    let full = usize::from(order);
    let mut ngrams: Vec<(u32, u32)> = ngrams
        .into_iter()
        .filter(|&(key, n)| key_len(key) < full || n >= prune)
        .collect();
    ngrams.sort_unstable();
    let mut contexts: Vec<(u32, u32, u32)> =
        contexts.into_iter().map(|(k, (t, n))| (k, t, n)).collect();
    contexts.sort_unstable();

    ModelData {
        order,
        lang,
        h: 0.0,
        ngrams,
        contexts,
        words: Vec::new(),
        word_total: 0,
    }
}

/// The `n` most frequent words as a version 2 word table: `(hash, count)`
/// sorted by hash, plus the total token count.
///
/// Entries are normalized first and merged when they normalize alike
/// (`NV` and `nv`). Entries with a letter outside the language (`café`,
/// `fi` in a Persian list) or nothing left after normalization (`42`) are
/// left out of both the list and the total. Ties in count go to the word that sorts
/// first, so the output does not depend on input order. If two listed words
/// share a hash, the more frequent one wins.
pub fn top_words<'a>(
    lang: Language,
    entries: impl IntoIterator<Item = (&'a str, u64)>,
    n: usize,
) -> (Vec<(u32, u32)>, u64) {
    let mut counts: HashMap<String, u64> = HashMap::new();
    let mut total = 0u64;
    let mut buf = [0u8; 4];
    for (word, count) in entries {
        let foreign = word
            .chars()
            .any(|c| c.is_alphabetic() && lang.normalize(c.encode_utf8(&mut buf)).is_empty());
        let w = lang.normalize(word);
        if foreign || w.is_empty() || count == 0 {
            continue;
        }
        total = total.saturating_add(count);
        let c = counts.entry(w).or_insert(0);
        *c = c.saturating_add(count);
    }
    let mut ranked: Vec<(String, u64)> = counts.into_iter().collect();
    ranked.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(n);
    let mut table: Vec<(u32, u32)> = ranked
        .iter()
        .map(|(w, c)| (word_hash(w), u32::try_from(*c).unwrap_or(u32::MAX)))
        .collect();
    // Most frequent first within a hash, then keep the first of each hash.
    table.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    table.dedup_by_key(|e| e.0);
    (table, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::UNKNOWN;

    const TOY: &[(&str, u32)] = &[
        ("the", 5),
        ("then", 2),
        ("there", 1),
        ("cat", 3),
        ("hat", 1),
        ("that", 4),
        ("a", 6),
        ("at", 2),
        ("don't", 1),
    ];

    fn toy(order: u8, prune: u32) -> LanguageModel {
        LanguageModel::from_data(count(Language::En, TOY.iter().copied(), order, prune))
    }

    fn syms(s: &str) -> Vec<u8> {
        s.chars().map(|c| Language::En.symbol(c)).collect()
    }

    fn assert_sums_to_one(m: &LanguageModel, history: &[u8]) {
        let n = Language::En.alphabet().len() as u8;
        let sum: f64 = (0..n).map(|c| m.prob(history, c)).sum();
        assert!((sum - 1.0).abs() < 1e-6, "history {history:?}: sum {sum}");
    }

    #[test]
    fn probabilities_sum_to_one() {
        for order in 1..=4 {
            let m = toy(order, 1);
            for h in [
                "", "^", "^^", "^^^", "^^t", "^th", "the", "hat", "at", "zzz", "t'",
            ] {
                let h: Vec<u8> = h
                    .chars()
                    .map(|c| {
                        if c == '^' {
                            PAD
                        } else {
                            Language::En.symbol(c)
                        }
                    })
                    .collect();
                assert_sums_to_one(&m, &h);
            }
        }
    }

    #[test]
    fn witten_bell_by_hand() {
        // Unigram level: 13 symbol types seen out of 30.
        let m = toy(1, 1);
        let total: u32 = TOY
            .iter()
            .map(|&(w, f)| (w.chars().count() as u32 + 1) * f)
            .sum();
        let (t, types) = m.context(pack_key(&[])).unwrap();
        assert_eq!(t, total);
        let uniform = 1.0 / 30.0;
        let a = syms("a")[0];
        let c_a: u32 = TOY
            .iter()
            .map(|&(w, f)| w.matches('a').count() as u32 * f)
            .sum();
        let expected = (f64::from(c_a) + f64::from(types) * uniform) / f64::from(t + types);
        assert!((m.prob(&[], a) - expected).abs() < 1e-12);
    }

    #[test]
    fn unseen_context_backs_off() {
        let m = toy(4, 1);
        let z = syms("z");
        let e = syms("e")[0];
        assert_eq!(m.prob(&[z[0], z[0], z[0]], e), m.prob(&[], e));
    }

    #[test]
    fn word_scores() {
        let m = toy(4, 1);
        assert!(m.word_log2prob("the") > m.word_log2prob("teh"));
        assert!(m.word_log2prob("cat") > m.word_log2prob("qzx"));
        // Every word gets a finite score, even odd or long ones.
        for w in ["", "?", "ÿ", &"a".repeat(300)] {
            let s = m.word_log2prob(w);
            assert!(s.is_finite() && s < 0.0, "{w:?}: {s}");
        }
        assert_eq!(Language::En.symbol('ÿ'), UNKNOWN);
    }

    #[test]
    fn pruning_keeps_context_totals() {
        let full = toy(4, 1).data().clone();
        let pruned = toy(4, 2).data().clone();
        assert_eq!(full.contexts, pruned.contexts);
        assert!(pruned.ngrams.len() < full.ngrams.len());
        assert!(pruned.ngrams.iter().all(|&(k, n)| key_len(k) < 4 || n >= 2));
        // Lower orders are never pruned.
        let low = |d: &ModelData| d.ngrams.iter().filter(|&&(k, _)| key_len(k) < 4).count();
        assert_eq!(low(&full), low(&pruned));
    }

    #[test]
    fn top_words_ranks_merges_and_looks_up() {
        let entries = [
            ("The", 50),
            ("the", 30),
            ("NV", 2),
            ("nv", 1),
            ("cat", 10),
            ("42", 99),
            ("café", 70),
        ];
        let (table, total) = top_words(Language::En, entries, 2);
        assert_eq!(total, 93);
        assert_eq!(table.len(), 2);
        let mut data = toy(4, 1).data().clone();
        data.words = table;
        data.word_total = total;
        let m = LanguageModel::from_data(data);
        assert!((m.word_ppm("the").unwrap() - 80.0 * 1e6 / 93.0).abs() < 1e-6);
        assert!(m.word_ppm("cat").is_some());
        assert_eq!(m.word_ppm("nv"), None);
        assert_eq!(m.word_ppm(""), None);
        // Version 1 models have no list.
        assert_eq!(toy(4, 1).word_ppm("the"), None);
    }

    #[test]
    fn top_words_is_order_independent() {
        let a = [("b", 5), ("a", 5), ("c", 5)];
        let b = [("c", 5), ("b", 5), ("a", 5)];
        assert_eq!(top_words(Language::En, a, 2), top_words(Language::En, b, 2));
    }

    #[test]
    fn counted_model_round_trips() {
        let m = toy(4, 2);
        let bytes = format::encode(m.data());
        let back = LanguageModel::from_bytes(&bytes).unwrap();
        assert_eq!(back.data(), m.data());
        assert_eq!(back.word_log2prob("that"), m.word_log2prob("that"));
    }
}
