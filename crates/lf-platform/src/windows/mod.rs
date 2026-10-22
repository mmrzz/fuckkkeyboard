//! Windows backend.
//!
//! The one module in this crate allowed to use `unsafe`: each FFI call is
//! wrapped in a small safe function with a `// SAFETY:` comment.
//!
//! - [`keymap`]: scan codes and what each installed layout types (pure).
//! - [`translate`]: hook events → engine input (pure).
//! - `capture`: low-level keyboard/mouse hooks and the foreground-window
//!   event hook, on their own thread with a message loop.
//! - `tracking`: the foreground app, its input language and the installed
//!   layouts.
//! - [`plan`]: engine commands → synthetic input steps (pure).
//! - `inject`: sending the strokes and requesting layout switches.
//!
//! Known limitations: windows running elevated are invisible to a
//! non-elevated Layout Fixer; UWP apps report their host process
//! (`applicationframehost.exe`) as the app id.
#![allow(unsafe_code)]

mod capture;
mod inject;
pub mod keymap;
pub mod plan;
mod tracking;
pub mod translate;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lf_core::{Command, InputEvent, LayoutId, ResetReason};

use crate::{AppId, Backend, BackendError, LayoutInfo, PlatformEvent};
use capture::{HookThread, Raw, Stamped};
use plan::Step;
use tracking::WinLayout;
use translate::Translator;

/// How often the worker checks whether the installed layouts changed.
const LAYOUT_POLL: Duration = Duration::from_secs(2);

/// Layout id reported for an input locale that is not in the installed list.
const UNKNOWN_LAYOUT: &str = "unknown";

#[derive(Debug, Default)]
struct Shared {
    layouts: Vec<WinLayout>,
    /// A layout this backend just switched to, so the worker does not
    /// report the switch back as a user's layout change.
    assumed: Option<LayoutId>,
}

impl Shared {
    fn id_for(&self, hkl: usize) -> Option<LayoutId> {
        self.layouts
            .iter()
            .find(|l| l.hkl == hkl)
            .map(|l| l.info.layout.id.clone())
    }

    fn hkls(&self) -> Vec<usize> {
        self.layouts.iter().map(|l| l.hkl).collect()
    }
}

/// The Windows implementation of [`Backend`].
pub struct WindowsBackend {
    shared: Arc<Mutex<Shared>>,
    clock: Instant,
    hooks: Option<HookThread>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsBackend {
    /// A backend for the current desktop session. Nothing is captured
    /// until [`Backend::start`].
    pub fn new() -> Self {
        WindowsBackend {
            shared: Arc::new(Mutex::new(Shared {
                layouts: tracking::load_layouts(),
                assumed: None,
            })),
            clock: Instant::now(),
            hooks: None,
            worker: None,
        }
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    /// The layout id for an input locale, reloading the list once if the
    /// locale is new.
    fn id_for(&self, hkl: usize) -> Option<LayoutId> {
        if let Some(id) = self.shared().id_for(hkl) {
            return Some(id);
        }
        let layouts = tracking::load_layouts();
        let mut sh = self.shared();
        sh.layouts = layouts;
        sh.id_for(hkl)
    }
}

impl Default for WindowsBackend {
    fn default() -> Self {
        WindowsBackend::new()
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

impl Backend for WindowsBackend {
    fn start(&mut self, tx: Sender<(PlatformEvent, u64)>) -> Result<(), BackendError> {
        if self.hooks.is_some() {
            return Ok(());
        }
        let (raw_tx, raw_rx) = mpsc::channel();
        let hooks = HookThread::start(raw_tx, self.clock)?;
        let shared = Arc::clone(&self.shared);
        let clock = self.clock;
        let worker = thread::Builder::new()
            .name("lf-win-events".into())
            .spawn(move || Worker::new(shared, clock, tx).run(raw_rx))
            .map_err(|e| BackendError::Connection(format!("event thread: {e}")))?;
        self.hooks = Some(hooks);
        self.worker = Some(worker);
        Ok(())
    }

    fn stop(&mut self) {
        // Ending the hook thread drops its sender, which ends the worker,
        // which drops the runtime's sender.
        if let Some(mut hooks) = self.hooks.take() {
            hooks.stop();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    fn execute(&mut self, cmds: &[Command]) -> Result<(), BackendError> {
        for step in plan::plan(cmds) {
            match step {
                Step::Type(strokes) => inject::send_strokes(&strokes)?,
                Step::SetLayout(id) => {
                    let hkl = self
                        .shared()
                        .layouts
                        .iter()
                        .find(|l| l.info.layout.id == id)
                        .map(|l| l.hkl)
                        .ok_or_else(|| {
                            BackendError::LayoutSwitch(format!("layout {id} is not installed"))
                        })?;
                    let hwnd = tracking::foreground_window()
                        .ok_or_else(|| BackendError::LayoutSwitch("no foreground window".into()))?;
                    inject::request_layout(hwnd, hkl)?;
                    self.shared().assumed = Some(id);
                }
            }
        }
        Ok(())
    }

    fn current_layout(&self) -> Result<LayoutId, BackendError> {
        let hkl = tracking::foreground_hkl();
        self.id_for(hkl)
            .ok_or_else(|| BackendError::Connection(format!("unknown input locale {hkl:#x}")))
    }

    fn layouts(&self) -> Result<Vec<LayoutInfo>, BackendError> {
        let layouts = tracking::load_layouts();
        if layouts.is_empty() {
            return Err(BackendError::Connection(
                "no keyboard layouts installed".into(),
            ));
        }
        let infos = layouts.iter().map(|l| l.info.clone()).collect();
        self.shared().layouts = layouts;
        Ok(infos)
    }

    fn focused_app(&self) -> Option<AppId> {
        tracking::foreground_app().map(AppId)
    }
}

impl Drop for WindowsBackend {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Turns raw hook events into [`PlatformEvent`]s, off the hook thread.
struct Worker {
    shared: Arc<Mutex<Shared>>,
    clock: Instant,
    tx: Sender<(PlatformEvent, u64)>,
    translator: Translator,
    known_hkls: Vec<usize>,
}

impl Worker {
    fn new(shared: Arc<Mutex<Shared>>, clock: Instant, tx: Sender<(PlatformEvent, u64)>) -> Self {
        let known_hkls = lock(&shared).hkls();
        Worker {
            shared,
            clock,
            tx,
            translator: Translator::new(tracking::caps_lock_on()),
            known_hkls,
        }
    }

    fn run(mut self, rx: Receiver<Stamped>) {
        loop {
            let ok = match rx.recv_timeout(LAYOUT_POLL) {
                Ok((raw, at)) => self.on_raw(raw, at),
                Err(RecvTimeoutError::Timeout) => self.poll_layouts(),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if !ok {
                break; // the runtime is gone
            }
        }
    }

    fn send(&self, ev: PlatformEvent, at: u64) -> bool {
        self.tx.send((ev, at)).is_ok()
    }

    fn on_raw(&mut self, raw: Raw, at: u64) -> bool {
        match raw {
            Raw::Click => self.send(
                PlatformEvent::Input(InputEvent::Reset(ResetReason::MouseClick)),
                at,
            ),
            Raw::Foreground => {
                // Releases may have gone to another desktop (Win+L, UAC).
                self.translator.release_all();
                let app = tracking::foreground_app().map(AppId);
                self.send(PlatformEvent::FocusChanged(app), at)
            }
            Raw::Key(key) => {
                if let Some(id) = lock(&self.shared).assumed.take() {
                    self.translator.assume_layout(id);
                }
                let layout = self.layout_for_key(at);
                let shared = lock(&self.shared);
                let events = self.translator.key(key, &layout, |id, code, mods| {
                    let l = shared.layouts.iter().find(|l| &l.info.layout.id == id)?;
                    l.info.layout.char_for(code, mods)
                });
                drop(shared);
                events
                    .into_iter()
                    .all(|ev| self.send(PlatformEvent::Input(ev), at))
            }
        }
    }

    /// The layout of the foreground window; a locale not seen before means
    /// the installed set changed.
    fn layout_for_key(&mut self, at: u64) -> LayoutId {
        let hkl = tracking::foreground_hkl();
        if let Some(id) = lock(&self.shared).id_for(hkl) {
            return id;
        }
        self.reload(at);
        lock(&self.shared)
            .id_for(hkl)
            .unwrap_or_else(|| LayoutId::new(UNKNOWN_LAYOUT))
    }

    fn poll_layouts(&mut self) -> bool {
        if tracking::installed_hkls() == self.known_hkls {
            return true;
        }
        let at = self.clock.elapsed().as_millis() as u64;
        self.reload(at)
    }

    fn reload(&mut self, at: u64) -> bool {
        let layouts = tracking::load_layouts();
        let mut sh = lock(&self.shared);
        sh.layouts = layouts;
        self.known_hkls = sh.hkls();
        drop(sh);
        self.send(PlatformEvent::LayoutsChanged, at)
    }
}
