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

pub use buffer::{KeyBuffer, BUFFER_CAPACITY};
pub use engine::{Command, Config, Engine, EngineError};
pub use event::{InputEvent, ResetReason};
pub use key::{KeyCode, KeyPress, Modifiers};
pub use layout::{render, Layout, LayoutId};
pub use sim::{Harness, ScreenSim, SimError};
pub use split::{AllWordsScorer, Candidate, LastWordScorer, SplitScorer, WordView};
pub use trigger::{Trigger, TriggerError};
