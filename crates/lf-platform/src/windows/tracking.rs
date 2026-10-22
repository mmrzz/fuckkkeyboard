//! Window and layout tracking: which app has the foreground window, which
//! input language it uses, and which layouts are installed.
//!
//! Known limitation: Windows hides windows running elevated (as
//! administrator) from a non-elevated app. Their input does not reach the
//! hooks and synthetic input cannot reach them, so Layout Fixer does
//! nothing there unless it is itself run elevated.

use std::collections::HashMap;
use std::sync::OnceLock;

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, HWND};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, GetKeyboardLayoutList, MapVirtualKeyExW, ToUnicodeEx, HKL,
    MAPVK_VSC_TO_VK_EX, VK_CAPITAL,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::keymap::{
    build_layout, char_from_to_unicode, key_state_for, klid_for_hkl, langid_for_hkl,
    language_for_langid, layout_id_for_klid, parse_hex_id, scan_for_key, unique_ids,
};
use super::translate::app_id_from_image_path;
use crate::LayoutInfo;

const LAYOUTS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts";

/// `ToUnicodeEx` flag: do not change the keyboard state (Windows 10 1607+),
/// so probing never disturbs a pending dead key.
const TO_UNICODE_NO_STATE_CHANGE: u32 = 0x4;

/// One installed layout and the handle Windows knows it by.
#[derive(Debug, Clone)]
pub(super) struct WinLayout {
    /// The input locale handle, as an integer so it can cross threads.
    pub hkl: usize,
    /// The layout as the engine sees it.
    pub info: LayoutInfo,
}

/// The foreground window and its thread and process ids.
fn foreground() -> Option<(HWND, u32, u32)> {
    // SAFETY: no arguments; returns a possibly-null window handle.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `hwnd` is a window handle (it may close meanwhile, in which
    // case the call returns 0) and `pid` is a valid u32 to write.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (thread != 0).then_some((hwnd, thread, pid))
}

/// The app id (lowercase exe name) of the foreground window.
pub(super) fn foreground_app() -> Option<String> {
    let (_, _, pid) = foreground()?;
    process_image_name(pid)
}

fn process_image_name(pid: u32) -> Option<String> {
    // SAFETY: plain call; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let name = image_path(process);
    close(process);
    app_id_from_image_path(&name?)
}

fn image_path(process: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is writable for `len` u16s and `len` is a valid u32;
    // the call writes at most `len` units and updates `len`.
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

fn close(handle: HANDLE) {
    // SAFETY: `handle` is an open handle we own, closed exactly once.
    let _ = unsafe { CloseHandle(handle) };
}

/// The input locale of the foreground window's thread (0 if unknown).
pub(super) fn foreground_hkl() -> usize {
    let thread = foreground().map_or(0, |(_, t, _)| t);
    // SAFETY: any thread id is accepted; 0 means the calling thread.
    unsafe { GetKeyboardLayout(thread) }.0 as usize
}

/// Every installed input locale, in the order Windows lists them.
pub(super) fn installed_hkls() -> Vec<usize> {
    // SAFETY: a None buffer asks only for the count.
    let n = unsafe { GetKeyboardLayoutList(None) };
    if n <= 0 {
        return Vec::new();
    }
    let mut list = vec![HKL::default(); n as usize];
    // SAFETY: `list` is writable for its full length, which is passed.
    let got = unsafe { GetKeyboardLayoutList(Some(&mut list)) };
    list.truncate(got.max(0) as usize);
    list.into_iter().map(|h| h.0 as usize).collect()
}

/// True if Caps Lock is toggled on.
pub(super) fn caps_lock_on() -> bool {
    // SAFETY: plain query of a virtual key's state.
    let state = unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) };
    state & 1 == 1
}

/// What `code` + `mods` types in the layout `hkl`.
fn probe(hkl: usize, code: lf_core::KeyCode, mods: lf_core::Modifiers) -> Option<char> {
    let hkl = Some(HKL(hkl as *mut _));
    let scan = u32::from(scan_for_key(code));
    // SAFETY: plain translation query for an installed layout handle.
    let vk = unsafe { MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, hkl) };
    if vk == 0 {
        return None;
    }
    let state = key_state_for(mods);
    let mut buf = [0u16; 8];
    // SAFETY: `state` is a 256-byte key state and `buf` a writable buffer
    // whose length the wrapper passes. The no-state-change flag keeps the
    // system's dead-key state untouched.
    let ret = unsafe { ToUnicodeEx(vk, scan, &state, &mut buf, TO_UNICODE_NO_STATE_CHANGE, hkl) };
    char_from_to_unicode(ret, &buf)
}

/// Read every installed layout and the characters each key produces.
pub(super) fn load_layouts() -> Vec<WinLayout> {
    let hkls = installed_hkls();
    let special = special_layout_ids();
    let klids: Vec<u32> = hkls
        .iter()
        .map(|&h| klid_for_hkl(h, |id| special.get(&id).copied()))
        .collect();
    let raw: Vec<(String, u16)> = hkls
        .iter()
        .zip(&klids)
        .map(|(&h, &klid)| (layout_id_for_klid(klid), langid_for_hkl(h)))
        .collect();
    let ids = unique_ids(&raw);

    hkls.iter()
        .zip(klids)
        .zip(ids.iter().zip(&raw))
        .map(|((&hkl, klid), (id, (base_id, langid)))| {
            let language = language_for_langid(*langid).map(str::to_owned);
            let text = layout_text(klid).unwrap_or_else(|| base_id.clone());
            let name = if id == base_id {
                text
            } else {
                let lang = language.clone().unwrap_or_else(|| format!("{langid:04x}"));
                format!("{text} ({lang})")
            };
            WinLayout {
                hkl,
                info: LayoutInfo {
                    layout: build_layout(id, &name, |code, mods| probe(hkl, code, mods)),
                    language,
                },
            }
        })
        .collect()
}

/// "Layout Id" (the low 12 bits of a special HKL's high word) → KLID, read
/// once from the registry.
fn special_layout_ids() -> &'static HashMap<u16, u32> {
    static IDS: OnceLock<HashMap<u16, u32>> = OnceLock::new();
    IDS.get_or_init(|| {
        let mut map = HashMap::new();
        for klid_str in layout_subkeys() {
            let Some(klid) = parse_hex_id(&klid_str) else {
                continue;
            };
            let sub = format!(r"{LAYOUTS_KEY}\{klid_str}");
            if let Some(id) = reg_string(&sub, "Layout Id").and_then(|s| parse_hex_id(&s)) {
                map.insert(id as u16, klid);
            }
        }
        map
    })
}

/// The display name Windows uses for a KLID, e.g. "Persian (Standard)".
fn layout_text(klid: u32) -> Option<String> {
    reg_string(&format!(r"{LAYOUTS_KEY}\{klid:08X}"), "Layout Text")
}

fn layout_subkeys() -> Vec<String> {
    let mut key = HKEY::default();
    // SAFETY: `key` is a valid HKEY to write; it is closed below.
    let r = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(LAYOUTS_KEY),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if r != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for index in 0.. {
        let mut buf = [0u16; 64];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` is writable for `len` u16s; the optional out
        // parameters are not requested.
        let r = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if r != ERROR_SUCCESS {
            break; // ERROR_NO_MORE_ITEMS or a real error
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    // SAFETY: `key` was opened above and is closed once.
    let _ = unsafe { RegCloseKey(key) };
    out
}

fn reg_string(subkey: &str, value: &str) -> Option<String> {
    let mut buf = [0u16; 256];
    let mut size = std::mem::size_of_val(&buf) as u32;
    // SAFETY: `buf` is writable for `size` bytes; RRF_RT_REG_SZ makes the
    // call fail rather than write a non-string value.
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(subkey),
            &HSTRING::from(value),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let units = (size as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..units]);
    let s = s.trim_end_matches('\0').trim();
    // "@%SystemRoot%\..." values are resource references, not names.
    (!s.is_empty() && !s.starts_with('@')).then(|| s.to_owned())
}
