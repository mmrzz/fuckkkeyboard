//! Character language models and the n-gram split scorer.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod alphabet;
pub mod format;
pub mod model;
pub mod patterns;
pub mod scorer;

pub use alphabet::Language;
pub use format::ModelError;
pub use model::LanguageModel;
pub use scorer::NgramScorer;
