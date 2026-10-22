//! Event source: low-level keyboard and mouse hooks plus a foreground-window
//! event hook, on a dedicated thread with a message loop.
//!
//! The callbacks must return quickly (Windows drops a low-level hook that
//! stalls), so they only timestamp the event and forward it into a channel.
//! The backend's worker thread does the translation.

use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, EVENT_SYSTEM_FOREGROUND, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, LLKHF_UP, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
};

use super::translate::RawKey;
use crate::BackendError;

/// Marks input this app injects (`dwExtraInfo`), so the hook skips it.
pub(super) const INJECT_MARK: usize = 0x4C46_4958; // "LFIX"

/// What the hooks observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Raw {
    /// A key press or release.
    Key(RawKey),
    /// A mouse button went down (the caret may have moved).
    Click,
    /// Another window came to the foreground.
    Foreground,
}

/// A raw event and its time in ms since the backend's clock started.
pub(super) type Stamped = (Raw, u64);

thread_local! {
    /// Where the hook callbacks on this thread send events.
    static SINK: RefCell<Option<(Sender<Stamped>, Instant)>> = const { RefCell::new(None) };
}

fn forward(raw: Raw) {
    SINK.with_borrow(|sink| {
        if let Some((tx, clock)) = sink {
            let at = clock.elapsed().as_millis() as u64;
            let _ = tx.send((raw, at));
        }
    });
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a
        // KBDLLHOOKSTRUCT that is valid for the duration of this call.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if info.dwExtraInfo != INJECT_MARK {
            forward(Raw::Key(RawKey {
                vk: info.vkCode as u16,
                scan: info.scanCode as u16,
                extended: info.flags.contains(LLKHF_EXTENDED),
                up: info.flags.contains(LLKHF_UP),
            }));
        }
    }
    next_hook(code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let msg = wparam.0 as u32;
        if [
            WM_LBUTTONDOWN,
            WM_RBUTTONDOWN,
            WM_MBUTTONDOWN,
            WM_XBUTTONDOWN,
        ]
        .contains(&msg)
        {
            forward(Raw::Click);
        }
    }
    next_hook(code, wparam, lparam)
}

unsafe extern "system" fn foreground_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND {
        forward(Raw::Foreground);
    }
}

fn next_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: passes a hook callback's own arguments on unchanged, as the
    // hook documentation requires.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn module_handle() -> Option<HINSTANCE> {
    // SAFETY: a null name asks for this executable's handle; nothing is
    // borrowed beyond the call.
    unsafe { GetModuleHandleW(None) }.ok().map(HINSTANCE::from)
}

fn current_thread_id() -> u32 {
    // SAFETY: no arguments, no preconditions.
    unsafe { GetCurrentThreadId() }
}

/// Make sure this thread has a message queue, so `PostThreadMessageW` to it
/// cannot fail once its id is published.
fn ensure_message_queue() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid, writable MSG for the call's duration.
    let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
}

fn install_hooks() -> Result<(HHOOK, HHOOK, HWINEVENTHOOK), BackendError> {
    let module = module_handle();
    // SAFETY: the callbacks are `extern "system"` fns with the HOOKPROC
    // signature and live for the whole program.
    let keyboard = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) }
        .map_err(|e| BackendError::Connection(format!("keyboard hook: {e}")))?;
    // SAFETY: as above.
    let mouse = match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) } {
        Ok(h) => h,
        Err(e) => {
            unhook(keyboard);
            return Err(BackendError::Connection(format!("mouse hook: {e}")));
        }
    };
    // SAFETY: out-of-context hook with a WINEVENTPROC callback that lives
    // for the whole program; events arrive through this thread's queue.
    let foreground = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if foreground.is_invalid() {
        unhook(keyboard);
        unhook(mouse);
        return Err(BackendError::Connection("foreground event hook".into()));
    }
    Ok((keyboard, mouse, foreground))
}

fn unhook(hook: HHOOK) {
    // SAFETY: `hook` came from SetWindowsHookExW and is unhooked once.
    let _ = unsafe { UnhookWindowsHookEx(hook) };
}

fn unhook_event(hook: HWINEVENTHOOK) {
    // SAFETY: `hook` came from SetWinEventHook and is unhooked once.
    let _ = unsafe { UnhookWinEvent(hook) };
}

/// Run the message loop until WM_QUIT. Low-level hooks are called from it.
fn message_loop() {
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a valid, writable MSG for the call's duration.
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 {
            break; // 0 = WM_QUIT, -1 = error
        }
    }
}

/// The hook thread. Dropping it without [`HookThread::stop`] also stops it.
pub(super) struct HookThread {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl HookThread {
    /// Install the hooks on a new thread; events go to `tx`, stamped with
    /// ms since `clock`.
    pub(super) fn start(tx: Sender<Stamped>, clock: Instant) -> Result<Self, BackendError> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("lf-win-hooks".into())
            .spawn(move || {
                ensure_message_queue();
                let hooks = match install_hooks() {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                SINK.set(Some((tx, clock)));
                let _ = ready_tx.send(Ok(current_thread_id()));
                message_loop();
                let (keyboard, mouse, foreground) = hooks;
                unhook(keyboard);
                unhook(mouse);
                unhook_event(foreground);
                // Dropping the sender ends the worker thread.
                SINK.set(None);
            })
            .map_err(|e| BackendError::Connection(format!("hook thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(thread_id)) => Ok(HookThread {
                thread_id,
                join: Some(join),
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => {
                let _ = join.join();
                Err(BackendError::Connection("hook thread exited".into()))
            }
        }
    }

    /// Remove the hooks and end the thread.
    pub(super) fn stop(&mut self) {
        if let Some(join) = self.join.take() {
            // SAFETY: plain message post to a thread id we own; the thread
            // created its queue before publishing the id.
            let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = join.join();
        }
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        self.stop();
    }
}
