//! `lf-train train` and `lf-train inspect`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use lf_lang::format::{self, key_len, unpack_key};
use lf_lang::{model, Language, LanguageModel};

/// Training settings.
pub struct Settings {
    /// N-gram order.
    pub order: u8,
    /// Drop full-order n-grams with a count below this.
    pub prune: u32,
    /// Fraction of lines (from the end) held out for measuring `h`.
    pub heldout: f64,
    /// Folder of Leipzig `*-words.txt` files for the frequent-word list.
    pub words: Option<std::path::PathBuf>,
    /// Size of the frequent-word list.
    pub top_words: usize,
}

/// Read every `*-words.txt` in `dir` (Leipzig `id<TAB>word<TAB>count`) and
/// return `(word, count)` for single-word entries.
pub fn read_word_counts(dir: &Path) -> Result<Vec<(String, u64)>, String> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("-words.txt"))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no *-words.txt files in {}", dir.display()));
    }
    let mut entries = Vec::new();
    for f in &files {
        let bytes = fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        for line in String::from_utf8_lossy(&bytes).lines() {
            let mut cols = line.trim_end_matches('\r').split('\t');
            let (Some(_id), Some(word), Some(count), None) =
                (cols.next(), cols.next(), cols.next(), cols.next())
            else {
                continue;
            };
            // Multi-word entries ("on the") are not words a user types.
            if word.contains(' ') {
                continue;
            }
            if let Ok(n) = count.parse::<u64>() {
                entries.push((word.to_owned(), n));
            }
        }
    }
    Ok(entries)
}

/// Split cleaned lines into (training, held-out): the held-out part is the
/// last `fraction` of the lines, at least one line if there are two or more.
pub fn split_heldout(lines: &[String], fraction: f64) -> (&[String], &[String]) {
    let n = lines.len();
    let mut held = ((n as f64) * fraction.clamp(0.0, 1.0)).ceil() as usize;
    if n >= 2 {
        held = held.clamp(1, n - 1);
    }
    lines.split_at(n - held.min(n))
}

/// Read a cleaned corpus file into lines.
pub fn read_lines(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

/// The normalized, non-empty words of some lines.
pub fn words<'a>(lang: Language, lines: &'a [String]) -> impl Iterator<Item = String> + 'a {
    lines
        .iter()
        .flat_map(|l| l.split_whitespace())
        .map(move |w| lang.normalize(w))
        .filter(|w| !w.is_empty())
}

/// Build a model from cleaned lines, measuring `h` on the held-out part.
pub fn build(lang: Language, lines: &[String], s: &Settings) -> Result<LanguageModel, String> {
    let (train, held) = split_heldout(lines, s.heldout);
    let mut freq: HashMap<String, u32> = HashMap::new();
    for w in words(lang, train) {
        *freq.entry(w).or_insert(0) += 1;
    }
    if freq.is_empty() {
        return Err("no training words".into());
    }
    let mut data = model::count(
        lang,
        freq.iter().map(|(w, &n)| (w.as_str(), n)),
        s.order,
        s.prune,
    );
    let (mut log2p, mut chars) = (0.0f64, 0usize);
    let m = LanguageModel::from_data(data.clone());
    for w in words(lang, held) {
        log2p += m.word_log2prob(&w);
        chars += w.chars().count() + 1;
    }
    if chars == 0 {
        return Err("no held-out words".into());
    }
    data.h = (log2p / chars as f64) as f32;
    if let Some(dir) = &s.words {
        let entries = read_word_counts(dir)?;
        let (table, total) = model::top_words(
            lang,
            entries.iter().map(|(w, n)| (w.as_str(), *n)),
            s.top_words,
        );
        data.words = table;
        data.word_total = total;
    }
    Ok(LanguageModel::from_data(data))
}

/// `lf-train train`.
pub fn run(lang: Language, input: &Path, out: &Path, s: &Settings) -> Result<(), String> {
    let lines = read_lines(input)?;
    let m = build(lang, &lines, s)?;
    let bytes = format::encode(m.data());
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{}: order {} prune {} -> {} ({} bytes, {} n-grams, {} contexts, {} words), h = {:.4}",
        lang.code(),
        s.order,
        s.prune,
        out.display(),
        bytes.len(),
        m.data().ngrams.len(),
        m.data().contexts.len(),
        m.data().words.len(),
        m.h()
    );
    Ok(())
}

/// `lf-train inspect`: header, table sizes and the busiest contexts.
pub fn inspect(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let m = LanguageModel::from_bytes(&bytes).map_err(|e| e.to_string())?;
    let d = m.data();
    let alphabet = d.lang.alphabet();
    println!(
        "file        {} ({} bytes, crc ok)",
        path.display(),
        bytes.len()
    );
    println!("language    {}", d.lang.code());
    println!("order       {}", d.order);
    println!("alphabet    {} symbols", alphabet.len());
    println!("h           {:.4} bits/char", d.h);
    println!("n-grams     {}", d.ngrams.len());
    for len in 1..=usize::from(d.order) {
        let n = d.ngrams.iter().filter(|&&(k, _)| key_len(k) == len).count();
        println!("  order {len}   {n}");
    }
    println!("contexts    {}", d.contexts.len());
    println!(
        "words       {} (from {} tokens)",
        d.words.len(),
        d.word_total
    );
    let mut top = d.contexts.clone();
    top.sort_by_key(|&(k, total, _)| (std::cmp::Reverse(total), k));
    println!("top contexts (context, C(h), T(h)):");
    for &(key, total, types) in top.iter().take(15) {
        let text: String = unpack_key(key)
            .iter()
            .map(|&s| match alphabet.get(usize::from(s)) {
                Some('\u{200C}') => '|',
                Some(&c) => c,
                None => '?',
            })
            .collect();
        println!("  {:<6} {total:>10} {types:>4}", format!("{text:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn heldout_is_the_tail() {
        let l = lines(100);
        let (train, held) = split_heldout(&l, 0.02);
        assert_eq!(train.len(), 98);
        assert_eq!(held, &l[98..]);
        assert_eq!(split_heldout(&lines(10), 0.02).1.len(), 1);
        assert_eq!(split_heldout(&lines(1), 0.02).1.len(), 1);
        assert_eq!(split_heldout(&lines(0), 0.02).1.len(), 0);
    }

    #[test]
    fn build_measures_negative_h() {
        let text: Vec<String> = ["the cat sat", "the hat", "a cat and a hat", "that cat"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let s = Settings {
            order: 3,
            prune: 1,
            heldout: 0.25,
            words: None,
            top_words: 20_000,
        };
        let m = build(Language::En, &text, &s).unwrap();
        assert!(m.h() < 0.0 && m.h().is_finite());
        assert_eq!(m.order(), 3);
    }
}
