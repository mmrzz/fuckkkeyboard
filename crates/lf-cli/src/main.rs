//! `lf-cli`: simulate typing through the Layout Fixer engine.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use lf_core::{
    build_words, render, AllWordsScorer, Candidate, Config, Engine, Harness, KeyPress,
    LastWordScorer, Layout, LayoutId, SplitScorer,
};

#[derive(Parser)]
#[command(
    name = "lf-cli",
    about = "Simulate typing through the Layout Fixer engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a script through the engine and print the commands it emits.
    ///
    /// Script syntax: plain chars are typed in the current layout; {bs},
    /// {click}, {chord}, {enter}, {layout:ID}, {wait:MS}, {trigger},
    /// and {{ / }} for literal braces.
    Simulate {
        /// Layout active at the start.
        #[arg(long, default_value = "us")]
        layout: String,
        /// How to choose which words to convert.
        #[arg(long, value_enum, default_value_t = ScorerKind::Ngram)]
        scorer: ScorerKind,
        /// The script to type.
        script: String,
    },
    /// Show the n-gram scorer's per-word LLRs and chosen split for some text.
    Score {
        /// Layout the text was typed in (converted to the other of us/ir).
        #[arg(long, default_value = "us")]
        layout: String,
        /// The text as it appeared on screen.
        text: String,
    },
    /// Print the W3C key codes that type a string in a layout.
    Keys {
        /// Layout to look the chars up in.
        #[arg(long, default_value = "us")]
        layout: String,
        /// Text to look up.
        text: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ScorerKind {
    /// Find the split with the embedded character models.
    Ngram,
    /// Convert only the last word.
    Last,
    /// Convert every word typed in the current layout.
    All,
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Cmd::Simulate {
            layout,
            scorer,
            script,
        } => simulate(&layout, scorer, &script),
        Cmd::Score { layout, text } => score(&layout, &text),
        Cmd::Keys { layout, text } => keys(&layout, &text),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn find_layout(id: &str) -> Result<Layout, String> {
    lf_layouts::all()
        .into_iter()
        .find(|l| l.id.as_str() == id)
        .ok_or_else(|| format!("unknown layout {id:?}"))
}

fn simulate(layout: &str, scorer: ScorerKind, script: &str) -> Result<(), String> {
    let scorer: Box<dyn SplitScorer> = match scorer {
        ScorerKind::Ngram => Box::new(lf_lang::default_scorer()),
        ScorerKind::Last => Box::new(LastWordScorer),
        ScorerKind::All => Box::new(AllWordsScorer),
    };
    let layouts = lf_layouts::all();
    let engine = Engine::new(
        Config::default(),
        layouts.clone(),
        scorer,
        LayoutId::new(layout),
    )
    .map_err(|e| e.to_string())?;
    let mut harness = Harness::new(engine, layouts);
    harness
        .run_with(script, |cmds| {
            for cmd in cmds {
                println!("{cmd:?}");
            }
        })
        .map_err(|e| e.to_string())?;
    println!(
        "screen: {:?}   layout: {}",
        harness.screen.text, harness.screen.layout
    );
    Ok(())
}

fn score(layout: &str, text: &str) -> Result<(), String> {
    let from = find_layout(layout)?;
    let (a, b) = Config::default().layout_pair;
    let to_id = match &from.id {
        id if *id == a => b,
        id if *id == b => a,
        id => return Err(format!("layout {id} is not in the {a}/{b} pair")),
    };
    let to = find_layout(to_id.as_str())?;
    let keys = text
        .chars()
        .map(|ch| {
            let (code, mods) = from
                .key_for(ch)
                .ok_or_else(|| format!("{ch:?} cannot be typed in layout {}", from.id))?;
            Ok(KeyPress {
                code,
                mods,
                layout: from.id.clone(),
                produced: Some(ch),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let words = build_words(&keys, &from, &to);
    let candidate = Candidate {
        words: &words,
        from: &from,
        to: &to,
    };
    let scorer = lf_lang::default_scorer();
    let llrs = scorer.word_llrs(&candidate);
    let width = |f: fn(&lf_core::WordView) -> &str| {
        words
            .iter()
            .map(|w| f(w).chars().count())
            .chain([8])
            .max()
            .unwrap_or(8)
    };
    let (wf, wt) = (width(|w| &w.text_from), width(|w| &w.text_to));
    println!(
        "{:<6}  {:<wf$}  {:<wt$}  LLR",
        "word", "as-typed", "rendered"
    );
    for (i, (w, llr)) in words.iter().zip(&llrs).enumerate() {
        println!("{i:<6}  {:<wf$}  {:<wt$}  {llr:.1}", w.text_from, w.text_to);
    }
    match scorer
        .first_wrong_word(&candidate)
        .and_then(|j| words.get(j).map(|w| (j, w)))
    {
        Some((j, w)) => {
            let fixed = render(&keys[..w.start], &from) + &render(&keys[w.start..], &to);
            println!("split at word {j} → {fixed:?}");
        }
        None => println!("nothing to convert"),
    }
    Ok(())
}

fn keys(layout: &str, text: &str) -> Result<(), String> {
    let layout = find_layout(layout)?;
    let codes = text
        .chars()
        .map(|ch| {
            let (code, mods) = layout
                .key_for(ch)
                .ok_or_else(|| format!("{ch:?} cannot be typed in layout {}", layout.id))?;
            let mut name = String::new();
            if mods.altgr {
                name.push_str("AltGr+");
            }
            if mods.shift {
                name.push_str("Shift+");
            }
            name.push_str(code.as_w3c());
            Ok(name)
        })
        .collect::<Result<Vec<_>, String>>()?;
    println!("{}", codes.join(" "));
    Ok(())
}
