//! The correction engine.
//!
//! Known limitation: backspace counts assume one backspace deletes one
//! produced char. Some apps delete a base letter plus its combining marks
//! together, which would make the engine delete too much. Phase 1 ignores this.

use crate::buffer::KeyBuffer;
use crate::event::InputEvent;
use crate::key::KeyPress;
use crate::layout::{render, Layout, LayoutId};
use crate::split::{build_words, Candidate, SplitScorer};
use crate::trigger::Trigger;

/// Engine settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// Key sequence that asks for a correction.
    pub trigger: Trigger,
    /// The two layouts to convert between, e.g. ("us", "ir").
    pub layout_pair: (LayoutId, LayoutId),
    /// How long after a fix a second trigger undoes it.
    pub undo_window_ms: u64,
    /// Clear the buffer after this long without a key press.
    pub idle_reset_ms: u64,
    /// Switch the system layout to the target layout after a fix.
    pub switch_layout_after_fix: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            trigger: Trigger::default_trigger(),
            layout_pair: (LayoutId::new("us"), LayoutId::new("ir")),
            undo_window_ms: 10_000,
            idle_reset_ms: 30_000,
            switch_layout_after_fix: true,
        }
    }
}

/// Something the platform must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Send `backspaces` backspaces, then type `text`.
    Replace {
        /// Number of backspaces to send.
        backspaces: usize,
        /// Text to type after the backspaces.
        text: String,
    },
    /// Switch the active system layout.
    SetLayout(LayoutId),
}

/// What the last [`Engine::handle`] call did, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Converted text to the other layout.
    Fix,
    /// Put back the text the previous fix replaced.
    Undo,
}

/// Error building an [`Engine`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    /// A layout named in the pair was not supplied.
    #[error("layout {0} is in the layout pair but was not supplied")]
    MissingLayout(LayoutId),
    /// The starting layout is not one of the pair.
    #[error("current layout {0} is not in the layout pair")]
    CurrentNotInPair(LayoutId),
}

/// What is needed to undo the last fix.
#[derive(Debug)]
struct UndoRecord {
    /// The suffix keys as they were before the fix.
    original_keys: Vec<KeyPress>,
    from: LayoutId,
    /// Number of converted keys now at the end of the buffer.
    replaced_len: usize,
    at_ms: u64,
    /// How many trigger keys, in order, have been typed since the fix.
    /// Any other key drops the record.
    trigger_progress: usize,
}

/// Turns input events into correction commands.
pub struct Engine {
    config: Config,
    layouts: Vec<Layout>,
    scorer: Box<dyn SplitScorer>,
    buffer: KeyBuffer,
    current: LayoutId,
    last_key_ms: u64,
    undo: Option<UndoRecord>,
    last_action: Option<Action>,
}

impl Engine {
    /// Build an engine. Both layouts of the pair must be in `layouts`, and
    /// `current` must be one of them.
    pub fn new(
        config: Config,
        layouts: Vec<Layout>,
        scorer: Box<dyn SplitScorer>,
        current: LayoutId,
    ) -> Result<Self, EngineError> {
        let (a, b) = &config.layout_pair;
        for id in [a, b] {
            if !layouts.iter().any(|l| &l.id == id) {
                return Err(EngineError::MissingLayout(id.clone()));
            }
        }
        if &current != a && &current != b {
            return Err(EngineError::CurrentNotInPair(current));
        }
        Ok(Engine {
            config,
            layouts,
            scorer,
            buffer: KeyBuffer::new(),
            current,
            last_key_ms: 0,
            undo: None,
            last_action: None,
        })
    }

    /// Feed one event; returns the commands the platform must execute, in order.
    pub fn handle(&mut self, ev: InputEvent, now_ms: u64) -> Vec<Command> {
        self.last_action = None;
        if now_ms.saturating_sub(self.last_key_ms) > self.config.idle_reset_ms
            && !self.buffer.is_empty()
        {
            self.reset();
        }
        match ev {
            InputEvent::Key(k) => {
                // Keys typed after a fix keep the undo record only while they
                // spell the start of the trigger.
                match &mut self.undo {
                    Some(u) if self.config.trigger.key_matches(u.trigger_progress, &k) => {
                        u.trigger_progress += 1;
                    }
                    _ => self.undo = None,
                }
                self.buffer.push(k);
                self.last_key_ms = now_ms;
                if !self.config.trigger.matches_tail(&self.buffer) {
                    return Vec::new();
                }
                match self.undo.take() {
                    Some(u) if now_ms.saturating_sub(u.at_ms) <= self.config.undo_window_ms => {
                        self.undo_fix(u)
                    }
                    _ => self.fix(now_ms),
                }
            }
            InputEvent::Backspace => {
                self.buffer.backspace();
                self.undo = None;
                Vec::new()
            }
            InputEvent::Chord | InputEvent::Reset(_) => {
                self.reset();
                Vec::new()
            }
            InputEvent::LayoutChanged(id) => {
                self.current = id;
                self.undo = None;
                Vec::new()
            }
        }
    }

    /// The engine's settings.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The layout the engine believes is active.
    pub fn current_layout(&self) -> &LayoutId {
        &self.current
    }

    /// Number of keys in the buffer (for tests and debugging).
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// What the last [`Engine::handle`] call did: a fix, an undo, or nothing.
    pub fn last_action(&self) -> Option<Action> {
        self.last_action
    }

    /// True if the last fix can still be undone by the trigger (for tests and debugging).
    pub fn has_undo_record(&self) -> bool {
        self.undo.is_some()
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.undo = None;
    }

    fn layout_index(&self, id: &LayoutId) -> usize {
        self.layouts
            .iter()
            .position(|l| &l.id == id)
            .expect("pair layouts are checked in Engine::new")
    }

    /// The other layout of the pair, or None if `id` is not in the pair.
    fn other_in_pair(&self, id: &LayoutId) -> Option<&LayoutId> {
        let (a, b) = &self.config.layout_pair;
        match id {
            _ if id == a => Some(b),
            _ if id == b => Some(a),
            _ => None,
        }
    }

    /// Pop the trigger keys off the buffer; return how many chars they typed.
    fn pop_trigger(&mut self) -> usize {
        let trigger = self.buffer.pop_tail(self.config.trigger.len());
        trigger.iter().filter(|k| k.produced.is_some()).count()
    }

    /// Section 6.2: convert the wrong-layout suffix to the other layout.
    fn fix(&mut self, now_ms: u64) -> Vec<Command> {
        let trigger_chars = self.pop_trigger();

        // A layout outside the pair (e.g. the user switched to a third one)
        // has nothing to convert to.
        let Some(to_id) = self.other_in_pair(&self.current).cloned() else {
            return Vec::new();
        };
        let from_id = self.current.clone();

        // Text typed before a deliberate layout switch is assumed correct.
        let range_len = self
            .buffer
            .iter()
            .rev()
            .take_while(|k| k.layout == from_id)
            .count();
        if range_len == 0 {
            return Vec::new();
        }
        let range: Vec<KeyPress> = self
            .buffer
            .iter()
            .skip(self.buffer.len() - range_len)
            .cloned()
            .collect();

        let from = &self.layouts[self.layout_index(&from_id)];
        let to = &self.layouts[self.layout_index(&to_id)];
        let words = build_words(&range, from, to);
        let candidate = Candidate {
            words: &words,
            from,
            to,
        };
        let Some(first) = self.scorer.first_wrong_word(&candidate) else {
            return Vec::new();
        };
        let Some(word) = words.get(first) else {
            return Vec::new();
        };

        let suffix = self.buffer.pop_tail(range_len - word.start);
        let suffix_chars = suffix.iter().filter(|k| k.produced.is_some()).count();
        let converted: Vec<KeyPress> = suffix
            .iter()
            .map(|k| KeyPress {
                layout: to_id.clone(),
                produced: to.char_for(k.code, k.mods).or(k.produced),
                ..k.clone()
            })
            .collect();

        let mut cmds = vec![Command::Replace {
            backspaces: suffix_chars + trigger_chars,
            text: render(&suffix, to),
        }];
        if self.config.switch_layout_after_fix {
            cmds.push(Command::SetLayout(to_id.clone()));
            self.current = to_id;
        }

        self.buffer.extend(converted);
        self.last_action = Some(Action::Fix);
        self.undo = Some(UndoRecord {
            replaced_len: suffix.len(),
            original_keys: suffix,
            from: from_id,
            at_ms: now_ms,
            trigger_progress: 0,
        });
        cmds
    }

    /// Section 6.3: put back the text the last fix replaced.
    fn undo_fix(&mut self, undo: UndoRecord) -> Vec<Command> {
        let trigger_chars = self.pop_trigger();
        let converted = self.buffer.pop_tail(undo.replaced_len);
        let replaced_chars = converted.iter().filter(|k| k.produced.is_some()).count();
        let from = &self.layouts[self.layout_index(&undo.from)];

        let mut cmds = vec![Command::Replace {
            backspaces: replaced_chars + trigger_chars,
            text: render(&undo.original_keys, from),
        }];
        if self.config.switch_layout_after_fix {
            cmds.push(Command::SetLayout(undo.from.clone()));
            self.current = undo.from;
        }
        self.buffer.extend(undo.original_keys);
        self.last_action = Some(Action::Undo);
        cmds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ResetReason;
    use crate::key::{KeyCode, Modifiers};
    use crate::split::{AllWordsScorer, LastWordScorer};

    /// Two toy layouts: "lat" types a b c f u k / space, "cyr" types а б ц ф у к.
    fn layouts() -> Vec<Layout> {
        let keys = [
            (KeyCode::KeyA, 'a', 'а'),
            (KeyCode::KeyB, 'b', 'б'),
            (KeyCode::KeyC, 'c', 'ц'),
            (KeyCode::KeyF, 'f', 'ф'),
            (KeyCode::KeyU, 'u', 'у'),
            (KeyCode::KeyK, 'k', 'к'),
            (KeyCode::Space, ' ', ' '),
        ];
        let lat: Vec<_> = keys
            .iter()
            .map(|&(c, l, _)| (c, Modifiers::NONE, l))
            .collect();
        let cyr: Vec<_> = keys
            .iter()
            .map(|&(c, _, r)| (c, Modifiers::NONE, r))
            .collect();
        vec![
            Layout::new("lat", "Latin", &lat),
            Layout::new("cyr", "Cyrillic", &cyr),
        ]
    }

    fn config() -> Config {
        Config {
            layout_pair: (LayoutId::new("lat"), LayoutId::new("cyr")),
            ..Config::default()
        }
    }

    fn engine(scorer: Box<dyn SplitScorer>) -> Engine {
        Engine::new(config(), layouts(), scorer, LayoutId::new("lat")).unwrap()
    }

    /// Type `text` in the engine's current layout, 50 ms apart starting at `t`.
    fn type_text(e: &mut Engine, text: &str, t: &mut u64) -> Vec<Command> {
        let mut out = Vec::new();
        for ch in text.chars() {
            let layout = e.layouts[e.layout_index(e.current_layout())].clone();
            let (code, mods) = layout.key_for(ch).unwrap();
            let k = KeyPress {
                code,
                mods,
                layout: layout.id.clone(),
                produced: Some(ch),
            };
            *t += 50;
            out.extend(e.handle(InputEvent::Key(k), *t));
        }
        out
    }

    fn replace(backspaces: usize, text: &str) -> Command {
        Command::Replace {
            backspaces,
            text: text.to_owned(),
        }
    }

    #[test]
    fn new_validates_layouts() {
        let missing = Engine::new(
            config(),
            layouts()[..1].to_vec(),
            Box::new(LastWordScorer),
            LayoutId::new("lat"),
        );
        assert_eq!(
            missing.err(),
            Some(EngineError::MissingLayout(LayoutId::new("cyr")))
        );
        let outside = Engine::new(
            config(),
            layouts(),
            Box::new(LastWordScorer),
            LayoutId::new("xx"),
        );
        assert_eq!(
            outside.err(),
            Some(EngineError::CurrentNotInPair(LayoutId::new("xx")))
        );
    }

    #[test]
    fn fix_converts_last_word_and_switches() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        assert!(type_text(&mut e, "ab ba", &mut t).is_empty());
        let cmds = type_text(&mut e, "fuckkk", &mut t);
        assert_eq!(
            cmds,
            vec![replace(8, "ба"), Command::SetLayout(LayoutId::new("cyr"))]
        );
        assert_eq!(e.current_layout(), &LayoutId::new("cyr"));
        assert_eq!(e.buffer_len(), 5);
        assert!(e.has_undo_record());
        let converted: String = e.buffer.iter().filter_map(|k| k.produced).collect();
        assert_eq!(converted, "ab ба");
        assert!(e.buffer.iter().skip(3).all(|k| k.layout.as_str() == "cyr"));
    }

    #[test]
    fn fix_all_words_includes_spaces() {
        let mut e = engine(Box::new(AllWordsScorer));
        let mut t = 0;
        type_text(&mut e, "ab  ba ", &mut t);
        let cmds = type_text(&mut e, "fuckkk", &mut t);
        assert_eq!(cmds[0], replace(13, "аб  ба "));
    }

    #[test]
    fn fix_without_layout_switch() {
        let mut e = Engine::new(
            Config {
                switch_layout_after_fix: false,
                ..config()
            },
            layouts(),
            Box::new(LastWordScorer),
            LayoutId::new("lat"),
        )
        .unwrap();
        let mut t = 0;
        let cmds = type_text(&mut e, "abfuckkk", &mut t);
        assert_eq!(cmds, vec![replace(8, "аб")]);
        assert_eq!(e.current_layout(), &LayoutId::new("lat"));
    }

    #[test]
    fn nothing_to_fix_drops_trigger_keys() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        assert!(type_text(&mut e, "fuckkk", &mut t).is_empty());
        assert_eq!(e.buffer_len(), 0);
        // Only spaces: a range with no words.
        assert!(type_text(&mut e, "  fuckkk", &mut t).is_empty());
        assert_eq!(e.buffer_len(), 2);
    }

    #[test]
    fn range_stops_at_layout_switch() {
        let mut e = engine(Box::new(AllWordsScorer));
        let mut t = 0;
        type_text(&mut e, "ab ", &mut t);
        e.handle(InputEvent::LayoutChanged(LayoutId::new("cyr")), t);
        type_text(&mut e, "ба", &mut t);
        let cmds = type_text(&mut e, "фуцккк", &mut t);
        assert_eq!(
            cmds,
            vec![replace(8, "ba"), Command::SetLayout(LayoutId::new("lat"))]
        );
    }

    #[test]
    fn layout_outside_pair_does_nothing() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "ab", &mut t);
        e.handle(InputEvent::LayoutChanged(LayoutId::new("xx")), t);
        let mut cmds = Vec::new();
        for code in Trigger::default_trigger().keys() {
            let k = KeyPress {
                code: *code,
                mods: Modifiers::NONE,
                layout: LayoutId::new("xx"),
                produced: Some('?'),
            };
            t += 50;
            cmds.extend(e.handle(InputEvent::Key(k), t));
        }
        assert!(cmds.is_empty());
        assert_eq!(e.buffer_len(), 2);
    }

    #[test]
    fn second_trigger_undoes() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "ab ab", &mut t);
        type_text(&mut e, "fuckkk", &mut t);
        assert_eq!(e.last_action(), Some(Action::Fix));
        let cmds = type_text(&mut e, "фуцккк", &mut t);
        assert_eq!(
            cmds,
            vec![replace(8, "ab"), Command::SetLayout(LayoutId::new("lat"))]
        );
        assert_eq!(e.last_action(), Some(Action::Undo));
        assert!(!e.has_undo_record());
        let restored: Vec<_> = e
            .buffer
            .iter()
            .map(|k| (k.produced, k.layout.as_str()))
            .collect();
        assert_eq!(restored.len(), 5);
        assert!(restored.iter().all(|&(_, l)| l == "lat"));
        // A third trigger right after an undo is a normal fix.
        let cmds = type_text(&mut e, "fuckkk", &mut t);
        assert_eq!(cmds[0], replace(8, "аб"));
        assert_eq!(e.last_action(), Some(Action::Fix));
        assert!(e.has_undo_record());
        e.handle(InputEvent::Backspace, t + 1);
        assert_eq!(e.last_action(), None);
    }

    #[test]
    fn undo_window_expired_is_normal_fix() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "ab", &mut t);
        type_text(&mut e, "fuckkk", &mut t);
        t += 10_000;
        let cmds = type_text(&mut e, "фуцккк", &mut t);
        assert_eq!(cmds[0], replace(8, "ab"));
        // A fix leaves a fresh undo record; an undo would have dropped it.
        assert!(e.has_undo_record());
    }

    #[test]
    fn undo_window_is_inclusive() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "ab", &mut t);
        type_text(&mut e, "fuckkk", &mut t);
        t += 10_000 - 300;
        type_text(&mut e, "фуцккк", &mut t);
        assert!(!e.has_undo_record());
    }

    #[test]
    fn other_key_or_event_drops_undo_record() {
        let other_key = InputEvent::Key(KeyPress {
            code: KeyCode::KeyA,
            mods: Modifiers::NONE,
            layout: LayoutId::new("cyr"),
            produced: Some('а'),
        });
        for interrupt in [
            other_key,
            InputEvent::Backspace,
            InputEvent::LayoutChanged(LayoutId::new("cyr")),
        ] {
            let mut e = engine(Box::new(LastWordScorer));
            let mut t = 0;
            type_text(&mut e, "ab", &mut t);
            type_text(&mut e, "fuckkk", &mut t);
            type_text(&mut e, "фу", &mut t);
            e.handle(interrupt, t);
            assert!(!e.has_undo_record());
        }
    }

    #[test]
    fn partial_trigger_keeps_undo_record() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "ab", &mut t);
        type_text(&mut e, "fuckkk", &mut t);
        type_text(&mut e, "фуцкк", &mut t);
        assert!(e.has_undo_record());
        // A stray key before the trigger drops the record, so the trigger
        // that follows is a fix (which leaves a new record), not an undo.
        let mut e = engine(Box::new(AllWordsScorer));
        let mut t = 0;
        type_text(&mut e, "ab", &mut t);
        type_text(&mut e, "fuckkk", &mut t);
        type_text(&mut e, "кфуцккк", &mut t);
        assert!(e.has_undo_record());
    }

    #[test]
    fn resets_clear_buffer() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        for ev in [
            InputEvent::Chord,
            InputEvent::Reset(ResetReason::MouseClick),
            InputEvent::Reset(ResetReason::Submit),
        ] {
            type_text(&mut e, "ab", &mut t);
            assert_eq!(e.buffer_len(), 2);
            e.handle(ev, t);
            assert_eq!(e.buffer_len(), 0);
        }
    }

    #[test]
    fn backspace_pops_one() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "abc", &mut t);
        e.handle(InputEvent::Backspace, t);
        assert_eq!(e.buffer_len(), 2);
        let cmds = type_text(&mut e, "fuckkk", &mut t);
        assert_eq!(cmds[0], replace(8, "аб"));
    }

    #[test]
    fn idle_reset_before_event() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "ab", &mut t);
        // Non-key events do not refresh the idle timer.
        t += 20_000;
        e.handle(InputEvent::LayoutChanged(LayoutId::new("lat")), t);
        assert_eq!(e.buffer_len(), 2);
        t += 10_001;
        assert!(type_text(&mut e, "fuckkk", &mut t).is_empty());
        assert_eq!(e.buffer_len(), 0);
    }

    #[test]
    fn idle_exactly_at_limit_keeps_buffer() {
        let mut e = engine(Box::new(LastWordScorer));
        let mut t = 0;
        type_text(&mut e, "ab", &mut t);
        t += 30_000 - 50;
        let cmds = type_text(&mut e, "f", &mut t);
        assert!(cmds.is_empty());
        assert_eq!(e.buffer_len(), 3);
    }
}
