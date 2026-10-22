//! A backend with no OS access, for tests and for previewing the app.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use lf_core::{Command, LayoutId};

use crate::{AppId, Backend, BackendError, LayoutInfo, PlatformEvent};

#[derive(Debug)]
struct State {
    layouts: Vec<LayoutInfo>,
    current: LayoutId,
    focused: Option<AppId>,
    tx: Option<Sender<(PlatformEvent, u64)>>,
    clock_ms: u64,
    executed: Vec<Vec<Command>>,
    fail_next: Option<BackendError>,
}

/// A backend that replays pushed events and records commands.
///
/// Clones share one state, so a test can keep a clone after handing the
/// backend to the runtime and keep pushing events into it.
#[derive(Debug, Clone)]
pub struct MockBackend {
    state: Arc<Mutex<State>>,
}

impl MockBackend {
    /// A backend with these layouts, starting on `current`.
    pub fn new(layouts: Vec<LayoutInfo>, current: LayoutId) -> Self {
        MockBackend {
            state: Arc::new(Mutex::new(State {
                layouts,
                current,
                focused: None,
                tx: None,
                clock_ms: 0,
                executed: Vec::new(),
                fail_next: None,
            })),
        }
    }

    /// The built-in "us" (en) and "ir" (fa) layouts, starting on "us".
    pub fn us_ir() -> Self {
        let layouts = vec![
            LayoutInfo {
                layout: lf_layouts::us(),
                language: Some("en".into()),
            },
            LayoutInfo {
                layout: lf_layouts::ir_pes(),
                language: Some("fa".into()),
            },
        ];
        MockBackend::new(layouts, LayoutId::new("us"))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Send an event, timestamped 10 ms after the previous one.
    ///
    /// Returns false if the backend is not started.
    pub fn push(&self, ev: PlatformEvent) -> bool {
        let at = self.state().clock_ms + 10;
        self.push_at(ev, at)
    }

    /// Send an event with an explicit timestamp.
    ///
    /// A `FocusChanged` event also updates [`Backend::focused_app`].
    /// Returns false if the backend is not started.
    pub fn push_at(&self, ev: PlatformEvent, at_ms: u64) -> bool {
        let mut s = self.state();
        s.clock_ms = at_ms;
        if let PlatformEvent::FocusChanged(app) = &ev {
            s.focused = app.clone();
        }
        match &s.tx {
            Some(tx) => tx.send((ev, at_ms)).is_ok(),
            None => false,
        }
    }

    /// Every `execute` call so far, one entry per call.
    pub fn executed(&self) -> Vec<Vec<Command>> {
        self.state().executed.clone()
    }

    /// Make the next `execute` call fail with `err` (and do nothing).
    pub fn fail_next_execute(&self, err: BackendError) {
        self.state().fail_next = Some(err);
    }

    /// Replace the installed layouts. Push [`PlatformEvent::LayoutsChanged`]
    /// to tell the runtime.
    pub fn set_layouts(&self, layouts: Vec<LayoutInfo>) {
        self.state().layouts = layouts;
    }

    /// True between `start` and `stop`.
    pub fn is_started(&self) -> bool {
        self.state().tx.is_some()
    }
}

impl Backend for MockBackend {
    fn start(&mut self, tx: Sender<(PlatformEvent, u64)>) -> Result<(), BackendError> {
        self.state().tx = Some(tx);
        Ok(())
    }

    fn stop(&mut self) {
        self.state().tx = None;
    }

    fn execute(&mut self, cmds: &[Command]) -> Result<(), BackendError> {
        let mut s = self.state();
        if let Some(err) = s.fail_next.take() {
            return Err(err);
        }
        for cmd in cmds {
            if let Command::SetLayout(id) = cmd {
                s.current = id.clone();
            }
        }
        s.executed.push(cmds.to_vec());
        Ok(())
    }

    fn current_layout(&self) -> Result<LayoutId, BackendError> {
        Ok(self.state().current.clone())
    }

    fn layouts(&self) -> Result<Vec<LayoutInfo>, BackendError> {
        Ok(self.state().layouts.clone())
    }

    fn focused_app(&self) -> Option<AppId> {
        self.state().focused.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_core::InputEvent;
    use std::sync::mpsc;

    #[test]
    fn push_reaches_the_channel_only_while_started() {
        let mut b = MockBackend::us_ir();
        assert!(!b.push(PlatformEvent::LayoutsChanged));
        let (tx, rx) = mpsc::channel();
        b.start(tx).unwrap();
        assert!(b.push(PlatformEvent::Input(InputEvent::Chord)));
        assert!(b.push_at(PlatformEvent::LayoutsChanged, 500));
        assert_eq!(
            rx.recv().unwrap(),
            (PlatformEvent::Input(InputEvent::Chord), 20)
        );
        assert_eq!(rx.recv().unwrap(), (PlatformEvent::LayoutsChanged, 500));
        b.stop();
        assert!(!b.is_started());
        assert!(rx.recv().is_err(), "stop drops the sender");
    }

    #[test]
    fn execute_records_and_applies_set_layout() {
        let mut b = MockBackend::us_ir();
        let cmds = vec![
            Command::Replace {
                backspaces: 2,
                text: "سل".into(),
            },
            Command::SetLayout(LayoutId::new("ir")),
        ];
        b.execute(&cmds).unwrap();
        assert_eq!(b.executed(), vec![cmds]);
        assert_eq!(b.current_layout().unwrap(), LayoutId::new("ir"));
    }

    #[test]
    fn fail_next_execute_fails_once() {
        let mut b = MockBackend::us_ir();
        b.fail_next_execute(BackendError::Injection("boom".into()));
        assert!(b.execute(&[]).is_err());
        assert!(b.execute(&[]).is_ok());
        assert_eq!(b.executed().len(), 1);
    }

    #[test]
    fn focus_events_update_focused_app_and_clones_share_state() {
        let mut b = MockBackend::us_ir();
        let probe = b.clone();
        let (tx, _rx) = mpsc::channel();
        b.start(tx).unwrap();
        probe.push(PlatformEvent::FocusChanged(Some(AppId::new("firefox"))));
        assert_eq!(b.focused_app(), Some(AppId::new("firefox")));
        assert_eq!(b.layouts().unwrap().len(), 2);
    }
}
