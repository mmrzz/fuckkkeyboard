//! `lf-train gen-bench` and `lf-train bench` (spec section 7).

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use lf_core::{build_words, render, Candidate, KeyPress, LastWordScorer, Layout, SplitScorer};
use lf_lang::Language;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::train::{read_lines, split_heldout};

/// Overall accuracy the ngram scorer must reach.
pub const GATE: f64 = 0.95;

/// Which scorer to benchmark.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ScorerKind {
    /// The n-gram scorer with the embedded models.
    Ngram,
    /// Phase 1: convert only the last word.
    Last,
}

/// One benchmark case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// Layout the text was typed in.
    pub layout_from: String,
    /// What appeared on screen.
    pub typed: String,
    /// Index of the first wrong word.
    pub expected: usize,
    /// Hard cases only: must pass.
    pub must: bool,
    /// Hard cases only: free-form note.
    pub note: String,
}

/// `(language, layout id)` of each side of the pair.
const SIDES: [(Language, &str); 2] = [(Language::En, "us"), (Language::Fa, "ir")];

fn layout(id: &str) -> Result<Layout, String> {
    lf_layouts::all()
        .into_iter()
        .find(|l| l.id.as_str() == id)
        .ok_or_else(|| format!("unknown layout {id:?}"))
}

/// The other layout of the us/ir pair.
fn other(id: &str) -> Result<&'static str, String> {
    match id {
        "us" => Ok("ir"),
        "ir" => Ok("us"),
        _ => Err(format!("layout {id:?} is not in the us/ir pair")),
    }
}

/// The keys that type `text` in `l`, or None if a char cannot be typed.
pub fn type_keys(text: &str, l: &Layout) -> Option<Vec<KeyPress>> {
    text.chars()
        .map(|ch| {
            let (code, mods) = l.key_for(ch)?;
            Some(KeyPress {
                code,
                mods,
                layout: l.id.clone(),
                produced: Some(ch),
            })
        })
        .collect()
}

/// `lf-train gen-bench`.
pub fn generate(
    en: &Path,
    fa: &Path,
    heldout: f64,
    seed: u64,
    count: usize,
    out: &Path,
) -> Result<(), String> {
    let en_lines = read_lines(en)?;
    let fa_lines = read_lines(fa)?;
    let pools = [
        split_heldout(&en_lines, heldout).1,
        split_heldout(&fa_lines, heldout).1,
    ];
    let layouts = [layout("us")?, layout("ir")?];
    let cases = generate_cases(pools, &layouts, seed, count)?;

    let mut text = String::from("# layout_from\ttyped_text\texpected_index\n");
    for c in &cases {
        text.push_str(&format!("{}\t{}\t{}\n", c.layout_from, c.typed, c.expected));
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{} cases from {} en / {} fa held-out sentences -> {}",
        cases.len(),
        pools[0].len(),
        pools[1].len(),
        out.display()
    );
    Ok(())
}

/// Build `count` synthetic cases (spec 7.1) from held-out sentences:
/// `pools[i]` holds the sentences of `SIDES[i]`, typed with `layouts[i]`.
pub fn generate_cases(
    pools: [&[String]; 2],
    layouts: &[Layout; 2],
    seed: u64,
    count: usize,
) -> Result<Vec<Case>, String> {
    if pools.iter().any(|p| p.is_empty()) {
        return Err("no held-out sentences".into());
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let mut cases = Vec::with_capacity(count);
    let mut attempts = 0usize;
    while cases.len() < count {
        attempts += 1;
        if attempts > count * 100 {
            return Err(format!("only {} usable cases found", cases.len()));
        }
        let a = rng.random_range(0..2usize);
        let b = 1 - a;
        let k = rng.random_range(0..=4usize);
        let s = rng.random_range(1..=6usize);

        let prefix_sentence = &pools[a][rng.random_range(0..pools[a].len())];
        let prefix: Vec<&str> = prefix_sentence.split_whitespace().take(k).collect();
        let suffix_sentence = &pools[b][rng.random_range(0..pools[b].len())];
        let words: Vec<&str> = suffix_sentence.split_whitespace().collect();
        if prefix.len() < k || words.len() < s {
            continue;
        }
        let start = rng.random_range(0..=words.len() - s);
        let suffix = words[start..start + s].join(" ");

        // What the user saw: the suffix's keys in B, rendered under A.
        let Some(keys) = type_keys(&suffix, &layouts[b]) else {
            continue;
        };
        let shown = render(&keys, &layouts[a]);
        // The runner re-types the text with A's reverse map; keep the case
        // only if that gives back the same suffix and word count.
        let Some(retyped) = type_keys(&shown, &layouts[a]) else {
            continue;
        };
        if render(&retyped, &layouts[b]) != suffix
            || build_words(&retyped, &layouts[a], &layouts[b]).len() != s
        {
            continue;
        }
        let prefix = prefix.join(" ");
        if type_keys(&prefix, &layouts[a]).is_none() {
            continue;
        }
        let typed = if k == 0 {
            shown
        } else {
            format!("{prefix} {shown}")
        };
        cases.push(Case {
            layout_from: SIDES[a].1.to_owned(),
            typed,
            expected: k,
            must: false,
            note: String::new(),
        });
    }
    Ok(cases)
}

/// Parse a cases file. `hard` files have two more columns: must, note.
pub fn load_cases(path: &Path, hard: bool) -> Result<Vec<Case>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut cases = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || format!("{}:{}: bad line {line:?}", path.display(), n + 1);
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < if hard { 4 } else { 3 } {
            return Err(bad());
        }
        cases.push(Case {
            layout_from: cols[0].to_owned(),
            typed: cols[1].to_owned(),
            expected: cols[2].parse().map_err(|_| bad())?,
            must: hard
                && match cols[3] {
                    "yes" => true,
                    "no" => false,
                    _ => return Err(bad()),
                },
            note: cols.get(4).copied().unwrap_or("").to_owned(),
        });
    }
    Ok(cases)
}

/// The outcome of one case.
pub struct Outcome {
    /// The scorer's answer.
    pub got: Option<usize>,
    /// Number of words in the case.
    pub words: usize,
    /// Time spent building words and scoring.
    pub time: Duration,
}

/// Run one case through a scorer.
pub fn run_case(c: &Case, scorer: &dyn SplitScorer) -> Result<Outcome, String> {
    let from = layout(&c.layout_from)?;
    let to = layout(other(&c.layout_from)?)?;
    let keys = type_keys(&c.typed, &from)
        .ok_or_else(|| format!("{:?} cannot be typed in {}", c.typed, from.id))?;
    let t = Instant::now();
    let words = build_words(&keys, &from, &to);
    let got = scorer.first_wrong_word(&Candidate {
        words: &words,
        from: &from,
        to: &to,
    });
    let time = t.elapsed();
    Ok(Outcome {
        got,
        words: words.len(),
        time,
    })
}

#[derive(Default, Clone, Copy)]
struct Tally {
    ok: usize,
    n: usize,
}

impl Tally {
    fn add(&mut self, ok: bool) {
        self.n += 1;
        self.ok += usize::from(ok);
    }

    fn rate(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.ok as f64 / self.n as f64
        }
    }

    fn pct(&self) -> String {
        if self.n == 0 {
            "-".into()
        } else {
            format!("{:.1}%", 100.0 * self.rate())
        }
    }
}

/// `lf-train bench`: print the report; Err if the gate is not met.
pub fn run(cases: &Path, hard: &Path, kind: ScorerKind) -> Result<(), String> {
    let scorer: Box<dyn SplitScorer> = match kind {
        ScorerKind::Ngram => Box::new(lf_lang::default_scorer()),
        ScorerKind::Last => Box::new(LastWordScorer),
    };
    let cases = load_cases(cases, false)?;
    if cases.is_empty() {
        return Err("no benchmark cases".into());
    }

    let (mut all, mut us, mut ir) = (Tally::default(), Tally::default(), Tally::default());
    let mut by_k = [Tally::default(); 5];
    let mut by_s = [Tally::default(); 3];
    let mut time = Duration::ZERO;
    for c in &cases {
        let o = run_case(c, scorer.as_ref())?;
        let ok = o.got == Some(c.expected);
        time += o.time;
        all.add(ok);
        if c.layout_from == "us" {
            &mut us
        } else {
            &mut ir
        }
        .add(ok);
        by_k[c.expected.min(4)].add(ok);
        let s = o.words.saturating_sub(c.expected);
        by_s[match s {
            0 | 1 => 0,
            2 | 3 => 1,
            _ => 2,
        }]
        .add(ok);
    }

    let name = match kind {
        ScorerKind::Ngram => "ngram",
        ScorerKind::Last => "last",
    };
    println!("scorer: {name:<9} cases: {}", all.n);
    println!("overall exact-split accuracy:   {}", all.pct());
    println!("by direction    us→ir {}   ir→us {}", us.pct(), ir.pct());
    let ks: Vec<String> = by_k
        .iter()
        .enumerate()
        .map(|(k, t)| format!("{k}: {}", t.pct()))
        .collect();
    println!("by prefix k     {}", ks.join("  "));
    println!(
        "by suffix s     1: {}  2–3: {}  4–6: {}",
        by_s[0].pct(),
        by_s[1].pct(),
        by_s[2].pct()
    );

    let mut failed_must = 0;
    if hard.exists() {
        let hard_cases = load_cases(hard, true)?;
        let (mut h, mut must) = (Tally::default(), Tally::default());
        let mut failures = Vec::new();
        for c in &hard_cases {
            let o = run_case(c, scorer.as_ref())?;
            let ok = o.got == Some(c.expected);
            h.add(ok);
            if c.must {
                must.add(ok);
            }
            if !ok {
                failures.push(format!(
                    "  FAIL{} {} {:?}: expected {}, got {:?}  {}",
                    if c.must { " (must)" } else { "" },
                    c.layout_from,
                    c.typed,
                    c.expected,
                    o.got,
                    c.note
                ));
            }
        }
        failed_must = must.n - must.ok;
        println!(
            "hard cases      {}/{} (must: {}/{})",
            h.ok, h.n, must.ok, must.n
        );
        for f in failures {
            println!("{f}");
        }
    } else {
        println!("hard cases      none ({} not found)", hard.display());
    }
    println!(
        "avg time per case: {:.3} ms",
        time.as_secs_f64() * 1000.0 / all.n as f64
    );

    let mut problems = Vec::new();
    if all.rate() < GATE {
        problems.push(format!(
            "accuracy {} is below {:.0}%",
            all.pct(),
            GATE * 100.0
        ));
    }
    if failed_must > 0 {
        problems.push(format!("{failed_must} must case(s) failed"));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("gate not met: {}", problems.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pools() -> (Vec<String>, Vec<String>) {
        let en = ["I am at work right now", "thanks for the meeting today"];
        let fa = ["من دارم میام خونه", "ممنون برای جلسه امروز"];
        (
            en.iter().map(|s| s.to_string()).collect(),
            fa.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn generated_cases_are_faithful_and_reproducible() {
        let (en, fa) = pools();
        let layouts = [layout("us").unwrap(), layout("ir").unwrap()];
        let a = generate_cases([&en, &fa], &layouts, 42, 200).unwrap();
        assert_eq!(a, generate_cases([&en, &fa], &layouts, 42, 200).unwrap());
        assert!(a.iter().any(|c| c.layout_from == "us"));
        assert!(a.iter().any(|c| c.layout_from == "ir"));
        for c in &a {
            assert!(c.expected <= 4);
            // Re-typing gives at least one suffix word after the prefix.
            let from = layout(&c.layout_from).unwrap();
            let to = layout(other(&c.layout_from).unwrap()).unwrap();
            let keys = type_keys(&c.typed, &from).unwrap();
            let words = build_words(&keys, &from, &to);
            assert!(words.len() > c.expected, "{c:?}");
            assert!(words.len() - c.expected <= 6, "{c:?}");
        }
        // The suffix shows up rendered: "میام" typed on us is "ldhl".
        assert!(a.iter().any(|c| c.typed.contains("ldhl")));
    }

    #[test]
    fn parse_case_files() {
        let p = std::env::temp_dir().join(format!("lf-bench-parse-{}.tsv", std::process::id()));
        fs::write(&p, "# header\nus\tlvsd\t0\tyes\tmersi\n\nir\tخن\t0\tno\n").unwrap();
        let cases = load_cases(&p, true).unwrap();
        assert_eq!(cases.len(), 2);
        assert!(cases[0].must && !cases[1].must);
        assert_eq!(cases[0].note, "mersi");
        fs::write(&p, "us\tlvsd\tx\n").unwrap();
        assert!(load_cases(&p, false).is_err());
        let _ = fs::remove_file(&p);
    }
}
