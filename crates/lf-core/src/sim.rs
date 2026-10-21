//! Screen simulator and script harness for tests and the CLI.

use crate::engine::{Command, Engine};
use crate::event::{InputEvent, ResetReason};
use crate::key::{KeyCode, KeyPress, Modifiers};
use crate::layout::{Layout, LayoutId};

/// Milliseconds the harness clock advances per event.
pub const EVENT_STEP_MS: u64 = 50;

/// A tiny model of a text field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSim {
    /// Text in the field.
    pub text: String,
    /// Active system layout.
    pub layout: LayoutId,
}

impl ScreenSim {
    /// Apply engine commands: remove `backspaces` chars from the end, append `text`,
    /// switch layout on SetLayout.
    pub fn apply(&mut self, cmds: &[Command]) {
        for cmd in cmds {
            match cmd {
                Command::Replace { backspaces, text } => {
                    for _ in 0..*backspaces {
                        self.text.pop();
                    }
                    self.text.push_str(text);
                }
                Command::SetLayout(id) => self.layout = id.clone(),
            }
        }
    }
}

/// Error running a harness script.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    /// A plain char that the current layout cannot type.
    #[error("{ch:?} cannot be typed in layout {layout}")]
    UnknownChar {
        /// The char.
        ch: char,
        /// The layout that was active.
        layout: LayoutId,
    },
    /// `{layout:…}` names a layout the harness does not have.
    #[error("unknown layout {0}")]
    UnknownLayout(LayoutId),
    /// A `{…}` token that is not part of the syntax.
    #[error("unknown token {{{0}}}")]
    UnknownToken(String),
    /// A `{` with no closing `}`.
    #[error("unclosed '{{' in script")]
    UnclosedBrace,
    /// A single `}` outside a token (use `}}` for a literal brace).
    #[error("stray '}}' in script; use '}}}}' for a literal brace")]
    StrayCloseBrace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Type(char),
    Backspace,
    Event(InputEvent),
    Layout(LayoutId),
    Wait(u64),
    Trigger,
}

fn parse(script: &str) -> Result<Vec<Step>, SimError> {
    let mut steps = Vec::new();
    let mut chars = script.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                steps.push(Step::Type('{'));
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                steps.push(Step::Type('}'));
            }
            '}' => return Err(SimError::StrayCloseBrace),
            '{' => {
                let mut token = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => token.push(c),
                        None => return Err(SimError::UnclosedBrace),
                    }
                }
                steps.push(parse_token(&token)?);
            }
            c => steps.push(Step::Type(c)),
        }
    }
    Ok(steps)
}

fn parse_token(token: &str) -> Result<Step, SimError> {
    let step = match token {
        "bs" => Step::Backspace,
        "click" => Step::Event(InputEvent::Reset(ResetReason::MouseClick)),
        "chord" => Step::Event(InputEvent::Chord),
        "enter" => Step::Event(InputEvent::Reset(ResetReason::Submit)),
        "trigger" => Step::Trigger,
        _ => {
            if let Some(id) = token.strip_prefix("layout:") {
                Step::Layout(LayoutId::new(id))
            } else if let Some(ms) = token.strip_prefix("wait:").and_then(|n| n.parse().ok()) {
                Step::Wait(ms)
            } else {
                return Err(SimError::UnknownToken(token.to_owned()));
            }
        }
    };
    Ok(step)
}

/// Drives an Engine + ScreenSim from a script.
///
/// Script syntax: plain chars are typed with the current layout's reverse
/// map; `{bs}` backspace, `{click}` mouse-click reset, `{chord}` Ctrl chord,
/// `{enter}` submit reset, `{layout:ID}` user layout switch, `{wait:N}`
/// advance time by N ms, `{trigger}` type the trigger keys, `{{` and `}}`
/// literal braces.
pub struct Harness {
    /// The engine under test.
    pub engine: Engine,
    /// The simulated text field.
    pub screen: ScreenSim,
    /// Current simulated time.
    pub now_ms: u64,
    layouts: Vec<Layout>,
}

impl Harness {
    /// Start with an empty screen in the engine's current layout.
    pub fn new(engine: Engine, layouts: Vec<Layout>) -> Self {
        let screen = ScreenSim {
            text: String::new(),
            layout: engine.current_layout().clone(),
        };
        Harness {
            engine,
            screen,
            now_ms: 0,
            layouts,
        }
    }

    /// Run a script (syntax above). Each typed char becomes a Key event
    /// using the CURRENT layout's reverse map, is appended to the screen as typed,
    /// then fed to the engine; resulting commands are applied to the screen.
    /// Advances now_ms by 50 per event.
    ///
    /// The whole script is parsed before anything runs, so a syntax error
    /// leaves the harness untouched.
    pub fn run(&mut self, script: &str) -> Result<(), SimError> {
        self.run_with(script, |_| {})
    }

    /// Like [`Harness::run`], also passing every non-empty batch of engine
    /// commands to `on_commands` before it is applied to the screen.
    pub fn run_with(
        &mut self,
        script: &str,
        mut on_commands: impl FnMut(&[Command]),
    ) -> Result<(), SimError> {
        for step in parse(script)? {
            let cmds = match step {
                Step::Type(ch) => {
                    let layout = self.current_layout()?;
                    let (code, mods) = layout.key_for(ch).ok_or_else(|| SimError::UnknownChar {
                        ch,
                        layout: layout.id.clone(),
                    })?;
                    self.press(code, mods)?
                }
                Step::Backspace => {
                    self.screen.text.pop();
                    self.feed(InputEvent::Backspace)
                }
                Step::Event(ev) => self.feed(ev),
                Step::Layout(id) => {
                    if !self.layouts.iter().any(|l| l.id == id) {
                        return Err(SimError::UnknownLayout(id));
                    }
                    self.screen.layout = id.clone();
                    self.feed(InputEvent::LayoutChanged(id))
                }
                Step::Wait(ms) => {
                    self.now_ms += ms;
                    Vec::new()
                }
                Step::Trigger => {
                    for code in self.engine.config().trigger.keys().to_vec() {
                        let cmds = self.press(code, Modifiers::NONE)?;
                        self.apply(&cmds, &mut on_commands);
                    }
                    Vec::new()
                }
            };
            self.apply(&cmds, &mut on_commands);
        }
        Ok(())
    }

    fn apply(&mut self, cmds: &[Command], on_commands: &mut impl FnMut(&[Command])) {
        if !cmds.is_empty() {
            on_commands(cmds);
            self.screen.apply(cmds);
        }
    }

    fn current_layout(&self) -> Result<&Layout, SimError> {
        self.layouts
            .iter()
            .find(|l| l.id == self.screen.layout)
            .ok_or_else(|| SimError::UnknownLayout(self.screen.layout.clone()))
    }

    /// Press a key in the current layout: show it on screen, then feed it.
    fn press(&mut self, code: KeyCode, mods: Modifiers) -> Result<Vec<Command>, SimError> {
        let layout = self.current_layout()?;
        let key = KeyPress {
            code,
            mods,
            layout: layout.id.clone(),
            produced: layout.char_for(code, mods),
        };
        if let Some(ch) = key.produced {
            self.screen.text.push(ch);
        }
        Ok(self.feed(InputEvent::Key(key)))
    }

    fn feed(&mut self, ev: InputEvent) -> Vec<Command> {
        self.now_ms += EVENT_STEP_MS;
        self.engine.handle(ev, self.now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Config;
    use crate::split::LastWordScorer;

    #[test]
    fn parse_tokens_and_braces() {
        let steps = parse("a{bs}{{}}{wait:5}{layout:ir}{trigger}{click}{chord}{enter}").unwrap();
        assert_eq!(
            steps,
            vec![
                Step::Type('a'),
                Step::Backspace,
                Step::Type('{'),
                Step::Type('}'),
                Step::Wait(5),
                Step::Layout(LayoutId::new("ir")),
                Step::Trigger,
                Step::Event(InputEvent::Reset(ResetReason::MouseClick)),
                Step::Event(InputEvent::Chord),
                Step::Event(InputEvent::Reset(ResetReason::Submit)),
            ]
        );
    }

    #[test]
    fn parse_errors() {
        assert_eq!(parse("a{bs"), Err(SimError::UnclosedBrace));
        assert_eq!(parse("a}b"), Err(SimError::StrayCloseBrace));
        assert_eq!(parse("{nope}"), Err(SimError::UnknownToken("nope".into())));
        assert_eq!(
            parse("{wait:x}"),
            Err(SimError::UnknownToken("wait:x".into()))
        );
    }

    #[test]
    fn screen_apply() {
        let mut s = ScreenSim {
            text: "héllo".into(),
            layout: LayoutId::new("us"),
        };
        s.apply(&[
            Command::Replace {
                backspaces: 4,
                text: "ey".into(),
            },
            Command::SetLayout(LayoutId::new("ir")),
        ]);
        assert_eq!(s.text, "hey");
        assert_eq!(s.layout, LayoutId::new("ir"));
        s.apply(&[Command::Replace {
            backspaces: 10,
            text: String::new(),
        }]);
        assert_eq!(s.text, "");
    }

    fn harness() -> Harness {
        let letters = [
            (KeyCode::KeyA, 'a', 'а'),
            (KeyCode::KeyC, 'c', 'ц'),
            (KeyCode::KeyF, 'f', 'ф'),
            (KeyCode::KeyU, 'u', 'у'),
            (KeyCode::KeyK, 'k', 'к'),
        ];
        let lat: Vec<_> = letters
            .iter()
            .map(|&(c, l, _)| (c, Modifiers::NONE, l))
            .collect();
        let cyr: Vec<_> = letters
            .iter()
            .map(|&(c, _, r)| (c, Modifiers::NONE, r))
            .collect();
        let layouts = vec![
            Layout::new("lat", "Latin", &lat),
            Layout::new("cyr", "Cyrillic", &cyr),
        ];
        let config = Config {
            layout_pair: (LayoutId::new("lat"), LayoutId::new("cyr")),
            ..Config::default()
        };
        let engine = Engine::new(
            config,
            layouts.clone(),
            Box::new(LastWordScorer),
            LayoutId::new("lat"),
        )
        .unwrap();
        Harness::new(engine, layouts)
    }

    #[test]
    fn harness_types_backspaces_and_fixes() {
        let mut h = harness();
        h.run("aca{bs}").unwrap();
        assert_eq!(h.screen.text, "ac");
        assert_eq!(h.now_ms, 4 * EVENT_STEP_MS);
        let mut seen = Vec::new();
        h.run_with("{trigger}", |c| seen.extend_from_slice(c))
            .unwrap();
        assert_eq!(h.screen.text, "ац");
        assert_eq!(h.screen.layout, LayoutId::new("cyr"));
        assert_eq!(seen.len(), 2);
        h.run("{wait:100}").unwrap();
        assert_eq!(h.now_ms, 10 * EVENT_STEP_MS + 100);
    }

    #[test]
    fn harness_rejects_untypable_chars_and_layouts() {
        let mut h = harness();
        assert_eq!(
            h.run("z"),
            Err(SimError::UnknownChar {
                ch: 'z',
                layout: LayoutId::new("lat")
            })
        );
        assert_eq!(
            h.run("{layout:xx}"),
            Err(SimError::UnknownLayout(LayoutId::new("xx")))
        );
        h.run("{layout:cyr}а").unwrap();
        assert_eq!(h.screen.text, "а");
    }
}
