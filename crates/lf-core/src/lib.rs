//! Platform-independent core of Layout Fixer: key types, layouts, the key
//! buffer, trigger matching, split scoring, the correction engine and a
//! screen simulator for tests.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod buffer;
pub mod engine;
pub mod event;
pub mod key;
pub mod layout;
pub mod sim;
pub mod split;
pub mod trigger;
