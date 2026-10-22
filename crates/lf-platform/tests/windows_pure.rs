//! The OS-independent parts of the Windows backend (scan-code table, key
//! map parsing, event translation, injection planning), tested on every OS.
//!
//! The Windows module itself only compiles on Windows, so its pure files are
//! included here by path. On Windows they also run as unit tests of the
//! crate, so this file is skipped there.
#![cfg(not(target_os = "windows"))]

#[allow(dead_code)]
#[path = "../src/windows/keymap.rs"]
mod keymap;

#[allow(dead_code)]
#[path = "../src/windows/translate.rs"]
mod translate;
