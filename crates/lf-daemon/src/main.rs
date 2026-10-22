//! Headless Layout Fixer: runs the X11 backend under the shared runtime
//! until Ctrl-C. For testing the backend without the tray app.
//!
//! Privacy: logs carry layout ids, app ids and counts, never typed text.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::process::ExitCode;

use clap::Parser;
use lf_core::LayoutId;

/// Fix text typed in the wrong keyboard layout (headless, Linux X11).
#[derive(Debug, Parser)]
#[command(name = "lf-daemon", version)]
struct Args {
    /// The two layouts to convert between, e.g. "us,ir". Default: the first
    /// two installed layouts with different languages.
    #[arg(long, value_name = "A,B", value_parser = parse_pair)]
    pair: Option<(LayoutId, LayoutId)>,
    /// Don't switch the system layout after a fix.
    #[arg(long)]
    no_switch: bool,
    /// App ids (X11 WM_CLASS class) to ignore, comma-separated.
    #[arg(long, value_name = "APP,APP", value_delimiter = ',')]
    block: Vec<String>,
    /// Log filter, as for RUST_LOG (error, warn, info, debug…).
    #[arg(long, value_name = "LEVEL", default_value = "info")]
    log: String,
    /// List the installed layouts and their detected languages, then exit.
    #[arg(long)]
    print_layouts: bool,
}

/// Parse "us,ir" into two distinct layout ids.
fn parse_pair(s: &str) -> Result<(LayoutId, LayoutId), String> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    match parts.as_slice() {
        [a, b] if !a.is_empty() && !b.is_empty() && a != b => {
            Ok((LayoutId::new(a), LayoutId::new(b)))
        }
        _ => Err(format!(
            "expected two different layout ids like \"us,ir\", got \"{s}\""
        )),
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    env_logger::Builder::new()
        .parse_filters(&args.log)
        .format_target(false)
        .init();
    run(args)
}

#[cfg(not(target_os = "linux"))]
fn run(_args: Args) -> ExitCode {
    eprintln!("lf-daemon supports Linux X11 only; use the Layout Fixer app on this OS.");
    ExitCode::from(2)
}

#[cfg(target_os = "linux")]
fn run(args: Args) -> ExitCode {
    use std::sync::mpsc;
    use std::time::Duration;

    use lf_platform::x11::X11Backend;
    use lf_platform::{Backend, BackendError};
    use lf_runtime::RuntimeConfig;

    let backend = match X11Backend::new() {
        Ok(b) => b,
        Err(BackendError::Unsupported(msg)) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("cannot start the X11 backend: {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.print_layouts {
        let current = backend.current_layout().ok();
        let layouts = match backend.layouts() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("cannot list layouts: {e}");
                return ExitCode::FAILURE;
            }
        };
        println!("{:<12} {:<8} name", "id", "language");
        for l in layouts {
            let mark = if current.as_ref() == Some(&l.layout.id) {
                "  (active)"
            } else {
                ""
            };
            println!(
                "{:<12} {:<8} {}{mark}",
                l.layout.id.as_str(),
                l.language.as_deref().unwrap_or("?"),
                l.layout.name
            );
        }
        return ExitCode::SUCCESS;
    }

    let config = RuntimeConfig {
        layout_pair: args.pair,
        switch_layout_after_fix: !args.no_switch,
        blocklist: args.block,
        ..RuntimeConfig::default()
    };
    let handle = match lf_runtime::start(Box::new(backend), config) {
        Ok(h) => h,
        Err(e) => {
            log::error!("cannot start: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (stop_tx, stop_rx) = mpsc::channel();
    if let Err(e) = ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    }) {
        log::error!("cannot install the Ctrl-C handler: {e}");
        return ExitCode::FAILURE;
    }

    let status = handle.status();
    match &status.layout_pair {
        Some((a, b)) => log::info!("running: fixing between {a} and {b}; Ctrl-C to stop"),
        None => log::info!("running; Ctrl-C to stop"),
    }

    let mut last = status;
    let code = loop {
        match stop_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break ExitCode::SUCCESS,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = handle.status();
        if (now.fixes, now.undos) != (last.fixes, last.undos) {
            log::info!("{} fixes, {} undos", now.fixes, now.undos);
        }
        if now.last_error != last.last_error {
            if let Some(e) = &now.last_error {
                log::error!("{e}");
            }
        }
        if now.current_layout != last.current_layout {
            if let Some(id) = &now.current_layout {
                log::debug!("layout: {id}");
            }
        }
        if !now.running {
            log::error!("the runtime stopped unexpectedly");
            break ExitCode::FAILURE;
        }
        last = now;
    };

    let s = handle.status();
    handle.stop();
    log::info!("stopped: {} fixes, {} undos", s.fixes, s.undos);
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_parsing() {
        assert_eq!(
            parse_pair("us,ir"),
            Ok((LayoutId::new("us"), LayoutId::new("ir")))
        );
        assert_eq!(
            parse_pair(" us , ir "),
            Ok((LayoutId::new("us"), LayoutId::new("ir")))
        );
        assert!(parse_pair("us").is_err());
        assert!(parse_pair("us,us").is_err());
        assert!(parse_pair("us,ir,de").is_err());
        assert!(parse_pair(",ir").is_err());
    }

    #[test]
    fn args_parse() {
        let a = Args::try_parse_from([
            "lf-daemon",
            "--pair",
            "us,ir",
            "--no-switch",
            "--block",
            "xterm,Alacritty",
            "--log",
            "debug",
        ])
        .unwrap();
        assert_eq!(a.pair, Some((LayoutId::new("us"), LayoutId::new("ir"))));
        assert!(a.no_switch);
        assert_eq!(a.block, ["xterm", "Alacritty"]);
        assert_eq!(a.log, "debug");
        assert!(!a.print_layouts);
        let d = Args::try_parse_from(["lf-daemon", "--print-layouts"]).unwrap();
        assert!(d.print_layouts && d.pair.is_none() && d.block.is_empty());
    }
}
