//! Carrying out a fix: synthetic key strokes and layout-switch requests.
//!
//! Text is sent as Unicode packets (`KEYEVENTF_UNICODE`), so it does not
//! depend on the active layout. Every injected event carries
//! [`INJECT_MARK`] so the keyboard hook skips it.
//!
//! Known limitation: Windows blocks synthetic input into windows running
//! elevated (as administrator) when this app is not elevated, and does so
//! silently; such windows are also invisible to the hooks, so no fix is
//! attempted there.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_INPUTLANGCHANGEREQUEST};

use super::capture::INJECT_MARK;
use super::plan::Stroke;
use crate::BackendError;

/// Backspace's scan code.
const SCAN_BACKSPACE: u16 = 0x0E;

fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_MARK,
            },
        },
    }
}

fn inputs_for(strokes: &[Stroke]) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(strokes.len() * 2);
    for stroke in strokes {
        let (vk, scan, flags) = match *stroke {
            Stroke::Backspace => (VK_BACK, SCAN_BACKSPACE, KEYBD_EVENT_FLAGS(0)),
            Stroke::Unit(u) => (VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE),
        };
        inputs.push(key_input(vk, scan, flags));
        inputs.push(key_input(vk, scan, flags | KEYEVENTF_KEYUP));
    }
    inputs
}

/// Send the strokes as one uninterruptible batch.
pub(super) fn send_strokes(strokes: &[Stroke]) -> Result<(), BackendError> {
    let inputs = inputs_for(strokes);
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: `inputs` is a fully initialised slice of keyboard INPUTs and
    // the size passed is that of one INPUT, as the API requires.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(BackendError::Injection(format!(
            "sent {sent} of {} key events (another program may be blocking input)",
            inputs.len()
        )));
    }
    Ok(())
}

/// Ask `hwnd` to switch its input language to `hkl`.
pub(super) fn request_layout(hwnd: isize, hkl: usize) -> Result<(), BackendError> {
    // SAFETY: posting a message never dereferences the handles; a stale
    // window handle makes the call fail, which is reported.
    unsafe {
        PostMessageW(
            Some(HWND(hwnd as *mut _)),
            WM_INPUTLANGCHANGEREQUEST,
            WPARAM(0),
            LPARAM(hkl as isize),
        )
    }
    .map_err(|e| BackendError::LayoutSwitch(e.to_string()))
}
