//! Planning synthetic input for one engine step.
//!
//! Plain Rust with no OS calls, so it is unit-tested on every OS (see
//! `tests/windows_pure.rs`).

use lf_core::{Command, LayoutId};

/// One synthetic key stroke (press + release).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// The Backspace key.
    Backspace,
    /// One UTF-16 code unit, sent as a Unicode "packet" so it types the
    /// same character whatever layout is active.
    Unit(u16),
}

/// One thing to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Send these strokes in a single batch, so the user's own typing
    /// cannot land in the middle.
    Type(Vec<Stroke>),
    /// Ask the foreground window to switch to this layout.
    SetLayout(LayoutId),
}

/// Turn one engine step's commands into steps, keeping their order and
/// merging neighbouring text into one batch.
pub fn plan(cmds: &[Command]) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    for cmd in cmds {
        match cmd {
            Command::Replace { backspaces, text } => {
                let strokes = std::iter::repeat_n(Stroke::Backspace, *backspaces)
                    .chain(text.encode_utf16().map(Stroke::Unit));
                match steps.last_mut() {
                    Some(Step::Type(batch)) => batch.extend(strokes),
                    _ => steps.push(Step::Type(strokes.collect())),
                }
            }
            Command::SetLayout(id) => steps.push(Step::SetLayout(id.clone())),
        }
    }
    steps.retain(|s| !matches!(s, Step::Type(b) if b.is_empty()));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replace(backspaces: usize, text: &str) -> Command {
        Command::Replace {
            backspaces,
            text: text.into(),
        }
    }

    #[test]
    fn backspaces_then_text_then_layout() {
        let cmds = [replace(2, "سل"), Command::SetLayout(LayoutId::new("ir"))];
        assert_eq!(
            plan(&cmds),
            [
                Step::Type(vec![
                    Stroke::Backspace,
                    Stroke::Backspace,
                    Stroke::Unit(0x0633),
                    Stroke::Unit(0x0644),
                ]),
                Step::SetLayout(LayoutId::new("ir")),
            ]
        );
    }

    #[test]
    fn astral_chars_become_surrogate_pairs() {
        assert_eq!(
            plan(&[replace(0, "😀")]),
            [Step::Type(vec![Stroke::Unit(0xD83D), Stroke::Unit(0xDE00)])]
        );
    }

    #[test]
    fn neighbouring_replaces_merge_and_empty_ones_vanish() {
        let cmds = [replace(1, ""), replace(0, "a"), replace(0, "")];
        assert_eq!(
            plan(&cmds),
            [Step::Type(vec![Stroke::Backspace, Stroke::Unit(0x61)])]
        );
        assert_eq!(plan(&[replace(0, "")]), []);
        assert_eq!(plan(&[]), []);
    }

    #[test]
    fn order_is_kept_across_layout_switches() {
        let ir = Command::SetLayout(LayoutId::new("ir"));
        let steps = plan(&[ir.clone(), replace(1, "x")]);
        assert!(matches!(
            steps.as_slice(),
            [Step::SetLayout(_), Step::Type(_)]
        ));
    }
}
