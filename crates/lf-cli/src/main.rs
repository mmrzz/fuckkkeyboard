//! `lf-cli`: simulate typing through the Layout Fixer engine.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use lf_core::{
    AllWordsScorer, Config, Engine, Harness, LastWordScorer, Layout, LayoutId, SplitScorer,
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
        #[arg(long, value_enum, default_value_t = ScorerKind::Last)]
        scorer: ScorerKind,
        /// The script to type.
        script: String,
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
