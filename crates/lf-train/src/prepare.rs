//! `lf-train prepare`: clean raw corpus files into one sentence per line.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use lf_lang::Language;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;

/// Most lines kept per language.
pub const MAX_LINES: usize = 1_000_000;
/// Shortest line kept, in chars.
const MIN_CHARS: usize = 10;
/// Least share of letters that must belong to the target language.
const MIN_LANG_SHARE: f64 = 0.6;

/// Read every `.txt` in `dir`, clean, and write the result to `out`.
pub fn run(lang: Language, dir: &Path, out: &Path, seed: u64) -> Result<(), String> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no .txt files in {}", dir.display()));
    }
    let mut raw = String::new();
    for f in &files {
        let bytes = fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?;
        raw.push_str(&String::from_utf8_lossy(&bytes));
        raw.push('\n');
    }
    let lines = clean(lang, raw.lines(), seed);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut text = lines.join("\n");
    text.push('\n');
    fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{}: {} files -> {} lines in {}",
        lang.code(),
        files.len(),
        lines.len(),
        out.display()
    );
    Ok(())
}

/// Strip ids, filter, deduplicate, shuffle with `seed`, then cap.
pub fn clean<'a>(lang: Language, lines: impl Iterator<Item = &'a str>, seed: u64) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for line in lines {
        let line = strip_id(line).trim();
        if line.chars().count() < MIN_CHARS || lang_share(lang, line) < MIN_LANG_SHARE {
            continue;
        }
        let line = fold_quotes(lang, line);
        if seen.insert(line.clone()) {
            kept.push(line);
        }
    }
    kept.shuffle(&mut StdRng::seed_from_u64(seed));
    kept.truncate(MAX_LINES);
    kept
}

/// Leipzig lines are `id<TAB>sentence`; drop a numeric id column if present.
fn strip_id(line: &str) -> &str {
    match line.split_once('\t') {
        Some((id, rest)) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => rest,
        _ => line,
    }
}

/// News text often writes `don’t`; normalization would drop the `’`, so
/// fold curly apostrophes into the `'` people actually type.
fn fold_quotes(lang: Language, line: &str) -> String {
    match lang {
        Language::En => line.replace(['\u{2019}', '\u{2018}'], "'"),
        Language::Fa => line.to_owned(),
    }
}

/// Share of the line's letters that belong to `lang` (0 if it has none).
fn lang_share(lang: Language, line: &str) -> f64 {
    let (mut ours, mut all) = (0usize, 0usize);
    for c in line.chars().filter(|c| c.is_alphabetic()) {
        all += 1;
        if !lang.normalize(c.encode_utf8(&mut [0; 4])).is_empty() {
            ours += 1;
        }
    }
    if all == 0 {
        0.0
    } else {
        ours as f64 / all as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_numeric_ids_only() {
        assert_eq!(strip_id("12\tHello there"), "Hello there");
        assert_eq!(strip_id("ab\tHello"), "ab\tHello");
        assert_eq!(strip_id("Hello"), "Hello");
    }

    #[test]
    fn language_share() {
        assert_eq!(lang_share(Language::En, "Hello world"), 1.0);
        assert_eq!(lang_share(Language::En, "سلام"), 0.0);
        assert_eq!(lang_share(Language::Fa, "123 ..."), 0.0);
        assert!(lang_share(Language::Fa, "سلام دنیا hi") > 0.6);
    }

    #[test]
    fn clean_filters_dedups_and_is_reproducible() {
        let input = [
            "1\tThis is a fine sentence.",
            "2\tThis is a fine sentence.",
            "3\tshort",
            "4\tاین یک جمله فارسی است",
            "5\tAnother good English line",
            "6\tMostly English with کمی فارسی",
            "7\tDon\u{2019}t stop me now",
            "8\tDon't stop me now",
        ];
        let a = clean(Language::En, input.iter().copied(), 7);
        let b = clean(Language::En, input.iter().copied(), 7);
        assert_eq!(a, b);
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            vec![
                "Another good English line",
                "Don't stop me now",
                "Mostly English with کمی فارسی",
                "This is a fine sentence.",
            ]
        );
    }
}
