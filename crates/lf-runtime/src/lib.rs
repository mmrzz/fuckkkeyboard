//! The shared runtime: one thread that owns the OS backend and the engine.
//!
//! Everyone else (the tray, the settings window, the headless daemon) talks
//! to it through a [`RuntimeHandle`]. Every OS reuses this crate; only the
//! [`Backend`] differs.
//!
//! Privacy: nothing here stores or logs typed text. Status carries only
//! counts, layout ids and app ids.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use lf_core::{
    Action, Command, Config, Engine, EngineError, InputEvent, LastWordScorer, LayoutId,
    ResetReason, SplitScorer, Trigger,
};
use lf_platform::{AppId, Backend, BackendError, LayoutInfo, PlatformEvent};

/// How many distinct recently focused apps [`RuntimeHandle::recent_apps`] keeps.
pub const RECENT_APPS: usize = 10;

/// Runtime settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Key sequence that asks for a correction.
    pub trigger: Trigger,
    /// The two layouts to convert between. None = the first two installed
    /// layouts whose languages differ.
    pub layout_pair: Option<(LayoutId, LayoutId)>,
    /// How long after a fix a second trigger undoes it.
    pub undo_window_ms: u64,
    /// Clear the buffer after this long without a key press.
    pub idle_reset_ms: u64,
    /// Switch the system layout to the target layout after a fix.
    pub switch_layout_after_fix: bool,
    /// App ids to ignore, compared case-insensitively.
    pub blocklist: Vec<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let c = Config::default();
        RuntimeConfig {
            trigger: c.trigger,
            layout_pair: None,
            undo_window_ms: c.undo_window_ms,
            idle_reset_ms: c.idle_reset_ms,
            switch_layout_after_fix: c.switch_layout_after_fix,
            blocklist: Vec::new(),
        }
    }
}

/// A snapshot of what the runtime is doing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// The runtime thread is running.
    pub running: bool,
    /// Input is being ignored at the user's request.
    pub paused: bool,
    /// The layout the engine believes is active.
    pub current_layout: Option<LayoutId>,
    /// The two layouts in use.
    pub layout_pair: Option<(LayoutId, LayoutId)>,
    /// The focused app is on the blocklist, so input is ignored.
    pub focused_app_blocked: bool,
    /// Number of fixes so far (counts only, never text).
    pub fixes: u64,
    /// Number of undos so far.
    pub undos: u64,
    /// The most recent error, cleared by the next successful step.
    pub last_error: Option<String>,
}

/// Error starting or reconfiguring the runtime.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    /// The backend failed.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// The engine rejected the configuration.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// No layout pair was given and no two layouts with different languages exist.
    #[error("no layout pair: need two installed layouts with different languages")]
    NoLayoutPair,
    /// The runtime thread is gone.
    #[error("runtime is not running")]
    Stopped,
}

/// Layouts as shown to the UI: (id, name, language).
type LayoutRow = (LayoutId, String, Option<String>);

#[derive(Default)]
struct Shared {
    status: Status,
    layouts: Vec<LayoutRow>,
    recent: VecDeque<AppId>,
}

enum Msg {
    Event(PlatformEvent, u64),
    Pause,
    Resume,
    SetConfig(RuntimeConfig, Sender<Result<(), RuntimeError>>),
    Stop,
}

/// Start the runtime thread: build the engine from the backend's live
/// layouts, then start the backend.
pub fn start(
    backend: Box<dyn Backend>,
    config: RuntimeConfig,
) -> Result<RuntimeHandle, RuntimeError> {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let (tx, rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = {
        let shared = Arc::clone(&shared);
        let tx = tx.clone();
        thread::Builder::new()
            .name("lf-runtime".into())
            .spawn(move || {
                let worker = match Worker::new(backend, config, shared, tx) {
                    Ok(w) => w,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                worker.run(rx);
            })
            .map_err(|_| RuntimeError::Stopped)?
    };
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(RuntimeHandle {
            tx,
            shared,
            thread: Some(thread),
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            let _ = thread.join();
            Err(RuntimeError::Stopped)
        }
    }
}

/// Controls a running runtime. Dropping it stops the runtime.
pub struct RuntimeHandle {
    tx: Sender<Msg>,
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
}

impl RuntimeHandle {
    fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    /// Ignore input until [`RuntimeHandle::resume`]. Clears the buffer.
    pub fn pause(&self) {
        let _ = self.tx.send(Msg::Pause);
    }

    /// Process input again.
    pub fn resume(&self) {
        let _ = self.tx.send(Msg::Resume);
    }

    /// Replace the settings and rebuild the engine. On error the old
    /// settings stay in effect.
    pub fn set_config(&self, c: RuntimeConfig) -> Result<(), RuntimeError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(Msg::SetConfig(c, reply_tx))
            .map_err(|_| RuntimeError::Stopped)?;
        reply_rx.recv().map_err(|_| RuntimeError::Stopped)?
    }

    /// What the runtime is doing now.
    pub fn status(&self) -> Status {
        self.shared().status.clone()
    }

    /// Installed layouts: (id, name, language).
    pub fn layouts(&self) -> Vec<(LayoutId, String, Option<String>)> {
        self.shared().layouts.clone()
    }

    /// The last [`RECENT_APPS`] distinct focused apps, most recent first.
    pub fn recent_apps(&self) -> Vec<AppId> {
        self.shared().recent.iter().cloned().collect()
    }

    /// Stop the backend and the runtime thread.
    pub fn stop(self) {}
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// Everything the engine is built from.
struct Built {
    engine: Engine,
    pair: (LayoutId, LayoutId),
    rows: Vec<LayoutRow>,
}

/// Layouts the engine may use: the live ones, then built-ins the backend
/// didn't supply (so a pair naming a built-in still works).
fn candidate_layouts(live: &[LayoutInfo]) -> Vec<LayoutInfo> {
    let builtins = [(lf_layouts::us(), "en"), (lf_layouts::ir_pes(), "fa")];
    let mut all = live.to_vec();
    for (layout, lang) in builtins {
        if !all.iter().any(|l| l.layout.id == layout.id) {
            all.push(LayoutInfo {
                layout,
                language: Some(lang.into()),
            });
        }
    }
    all
}

/// The first two layouts whose (known) languages differ.
fn auto_pair(layouts: &[LayoutInfo]) -> Option<(LayoutId, LayoutId)> {
    let mut known = layouts
        .iter()
        .filter_map(|l| Some((&l.layout.id, l.language.as_deref()?)));
    let (a, lang_a) = known.next()?;
    let (b, _) = known.find(|&(_, lang)| lang != lang_a)?;
    Some((a.clone(), b.clone()))
}

fn build(backend: &dyn Backend, config: &RuntimeConfig) -> Result<Built, RuntimeError> {
    let live = backend.layouts().unwrap_or_else(|e| {
        log::warn!("backend could not list layouts, using built-ins: {e}");
        Vec::new()
    });
    let candidates = candidate_layouts(&live);
    let pair = match &config.layout_pair {
        Some(p) => p.clone(),
        None => auto_pair(&live)
            .or_else(|| auto_pair(&candidates))
            .ok_or(RuntimeError::NoLayoutPair)?,
    };

    let lang_pairs: Vec<(LayoutId, String)> = candidates
        .iter()
        .filter_map(|l| Some((l.layout.id.clone(), l.language.clone()?)))
        .collect();
    let scorer: Box<dyn SplitScorer> = match lf_lang::scorer_for(&lang_pairs) {
        Ok(s) => Box::new(s),
        Err(e) => {
            log::warn!("no language models for these layouts, fixing the last word only: {e}");
            Box::new(LastWordScorer)
        }
    };

    let current = match backend.current_layout() {
        Ok(id) if id == pair.0 || id == pair.1 => id,
        _ => pair.0.clone(),
    };
    let engine_config = Config {
        trigger: config.trigger.clone(),
        layout_pair: pair.clone(),
        undo_window_ms: config.undo_window_ms,
        idle_reset_ms: config.idle_reset_ms,
        switch_layout_after_fix: config.switch_layout_after_fix,
    };
    let layouts = candidates.iter().map(|l| l.layout.clone()).collect();
    let engine = Engine::new(engine_config, layouts, scorer, current)?;

    let shown = if live.is_empty() { &candidates } else { &live };
    let rows = shown
        .iter()
        .map(|l| {
            (
                l.layout.id.clone(),
                l.layout.name.clone(),
                l.language.clone(),
            )
        })
        .collect();
    Ok(Built { engine, pair, rows })
}

fn is_blocked(blocklist: &[String], app: Option<&AppId>) -> bool {
    app.is_some_and(|app| {
        blocklist
            .iter()
            .any(|b| b.eq_ignore_ascii_case(app.as_str()))
    })
}

struct Worker {
    backend: Box<dyn Backend>,
    config: RuntimeConfig,
    engine: Engine,
    paused: bool,
    focused: Option<AppId>,
    shared: Arc<Mutex<Shared>>,
}

impl Worker {
    fn new(
        mut backend: Box<dyn Backend>,
        config: RuntimeConfig,
        shared: Arc<Mutex<Shared>>,
        tx: Sender<Msg>,
    ) -> Result<Self, RuntimeError> {
        let built = build(backend.as_ref(), &config)?;
        let focused = backend.focused_app();

        // The backend sends to its own channel; forward into ours so one
        // loop sees both platform events and control messages, in order.
        let (ptx, prx) = mpsc::channel::<(PlatformEvent, u64)>();
        backend.start(ptx)?;
        thread::Builder::new()
            .name("lf-runtime-events".into())
            .spawn(move || {
                for (ev, at) in prx {
                    if tx.send(Msg::Event(ev, at)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| RuntimeError::Stopped)?;

        let mut w = Worker {
            backend,
            config,
            engine: built.engine,
            paused: false,
            focused: None,
            shared,
        };
        w.install(built.pair, built.rows);
        w.set_focus(focused);
        w.shared().status.running = true;
        log::info!("runtime started");
        Ok(w)
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    fn run(mut self, rx: Receiver<Msg>) {
        for msg in rx {
            match msg {
                Msg::Event(ev, at) => self.on_event(ev, at),
                Msg::Pause => {
                    self.paused = true;
                    self.engine
                        .handle(InputEvent::Reset(ResetReason::Manual), 0);
                    self.shared().status.paused = true;
                    log::info!("paused");
                }
                Msg::Resume => {
                    self.paused = false;
                    self.shared().status.paused = false;
                    log::info!("resumed");
                }
                Msg::SetConfig(c, reply) => {
                    let _ = reply.send(self.reconfigure(c));
                }
                Msg::Stop => break,
            }
        }
        self.backend.stop();
        let s = &mut self.shared().status;
        s.running = false;
        log::info!("runtime stopped: {} fixes, {} undos", s.fixes, s.undos);
    }

    /// Swap in a freshly built engine and publish its pair and layouts.
    fn install(&mut self, pair: (LayoutId, LayoutId), rows: Vec<LayoutRow>) {
        let current = self.engine.current_layout().clone();
        let mut sh = self.shared();
        sh.status.layout_pair = Some(pair);
        sh.status.current_layout = Some(current);
        sh.layouts = rows;
    }

    fn rebuild(&mut self, config: &RuntimeConfig) -> Result<(), RuntimeError> {
        let built = build(self.backend.as_ref(), config)?;
        self.engine = built.engine;
        self.install(built.pair, built.rows);
        Ok(())
    }

    fn reconfigure(&mut self, c: RuntimeConfig) -> Result<(), RuntimeError> {
        self.rebuild(&c)?;
        self.config = c;
        let blocked = is_blocked(&self.config.blocklist, self.focused.as_ref());
        let mut sh = self.shared();
        sh.status.focused_app_blocked = blocked;
        sh.status.last_error = None;
        Ok(())
    }

    fn set_focus(&mut self, app: Option<AppId>) {
        let blocked = is_blocked(&self.config.blocklist, app.as_ref());
        let mut sh = self.shared();
        sh.status.focused_app_blocked = blocked;
        if let Some(app) = &app {
            sh.recent.retain(|a| a != app);
            sh.recent.push_front(app.clone());
            sh.recent.truncate(RECENT_APPS);
        }
        drop(sh);
        self.focused = app;
    }

    fn on_event(&mut self, ev: PlatformEvent, at: u64) {
        match ev {
            PlatformEvent::FocusChanged(app) => {
                self.engine
                    .handle(InputEvent::Reset(ResetReason::FocusChanged), at);
                self.set_focus(app);
            }
            PlatformEvent::LayoutsChanged => {
                let config = self.config.clone();
                if let Err(e) = self.rebuild(&config) {
                    log::error!("layouts changed, keeping the old engine: {e}");
                    self.shared().status.last_error = Some(e.to_string());
                }
            }
            PlatformEvent::Input(input) => self.on_input(input, at),
        }
    }

    fn on_input(&mut self, input: InputEvent, at: u64) {
        // Layout changes always reach the engine so it never loses track of
        // the active layout; only typing is dropped while paused or blocked.
        let is_layout = matches!(input, InputEvent::LayoutChanged(_));
        let blocked = is_blocked(&self.config.blocklist, self.focused.as_ref());
        if !is_layout && (self.paused || blocked) {
            return;
        }
        let cmds = self.engine.handle(input, at);
        let action = self.engine.last_action();
        let done = cmds.is_empty() || self.execute(&cmds, at);
        let current = self.engine.current_layout().clone();
        let mut sh = self.shared();
        sh.status.current_layout = Some(current);
        match action {
            Some(Action::Fix) if done => sh.status.fixes += 1,
            Some(Action::Undo) if done => sh.status.undos += 1,
            _ => {}
        }
    }

    /// Carry out one step's commands; false if the backend failed.
    fn execute(&mut self, cmds: &[Command], at: u64) -> bool {
        match self.backend.execute(cmds) {
            Ok(()) => {
                self.shared().status.last_error = None;
                true
            }
            Err(e) => {
                log::error!("could not carry out a fix: {e}");
                self.shared().status.last_error = Some(e.to_string());
                // The screen may not match the buffer any more, and the
                // engine may have assumed a layout switch that didn't happen.
                self.engine
                    .handle(InputEvent::Reset(ResetReason::Manual), at);
                if let Ok(id) = self.backend.current_layout() {
                    self.engine.handle(InputEvent::LayoutChanged(id), at);
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests;
