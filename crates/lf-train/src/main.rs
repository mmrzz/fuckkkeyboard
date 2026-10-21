//! `lf-train`: offline tools to build language models and benchmark scorers.
#![forbid(unsafe_code)]

mod prepare;
mod train;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use lf_lang::Language;

#[derive(Parser)]
#[command(
    name = "lf-train",
    about = "Build Layout Fixer language models and benchmark scorers"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Clean raw corpus files into one shuffled sentence per line.
    Prepare {
        /// Language code: en or fa.
        #[arg(long, value_parser = parse_lang)]
        lang: Language,
        /// Folder of raw .txt files.
        #[arg(long = "in")]
        input: PathBuf,
        /// Cleaned output file.
        #[arg(long)]
        out: PathBuf,
        /// Shuffle seed.
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Train a character model from a cleaned corpus.
    Train {
        /// Language code: en or fa.
        #[arg(long, value_parser = parse_lang)]
        lang: Language,
        /// Cleaned corpus file.
        #[arg(long = "in")]
        input: PathBuf,
        /// Output .lfng file.
        #[arg(long)]
        out: PathBuf,
        /// N-gram order (1-4).
        #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u8).range(1..=4))]
        order: u8,
        /// Drop full-order n-grams with a count below this.
        #[arg(long, default_value_t = 2)]
        prune: u32,
        /// Fraction of lines held out (from the end) to measure h.
        #[arg(long, default_value_t = 0.02)]
        heldout: f64,
    },
    /// Print a model's header, table sizes and top contexts.
    Inspect {
        /// The .lfng file.
        model: PathBuf,
    },
}

fn parse_lang(s: &str) -> Result<Language, String> {
    Language::from_code(s).ok_or_else(|| format!("unknown language {s:?} (expected en or fa)"))
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Cmd::Prepare {
            lang,
            input,
            out,
            seed,
        } => prepare::run(lang, &input, &out, seed),
        Cmd::Train {
            lang,
            input,
            out,
            order,
            prune,
            heldout,
        } => train::run(
            lang,
            &input,
            &out,
            &train::Settings {
                order,
                prune,
                heldout,
            },
        ),
        Cmd::Inspect { model } => train::inspect(&model),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
