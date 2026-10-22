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

use lf_core::LayoutId;

/// The embedded English model.
pub const EN_MODEL: &[u8] = include_bytes!("../../../models/en.lfng");
/// The embedded Persian model.
pub const FA_MODEL: &[u8] = include_bytes!("../../../models/fa.lfng");

/// Scorer with the embedded models, mapping "us" -> en, "ir" -> fa.
///
/// A model that fails to load is left out (the tests make sure both load);
/// without it the scorer falls back to converting the last word.
pub fn default_scorer() -> NgramScorer {
    let models = [("us", EN_MODEL), ("ir", FA_MODEL)]
        .into_iter()
        .filter_map(|(id, bytes)| Some((LayoutId::new(id), LanguageModel::from_bytes(bytes).ok()?)))
        .collect();
    NgramScorer::new(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_models_load() {
        let s = default_scorer();
        for (id, lang) in [("us", Language::En), ("ir", Language::Fa)] {
            let m = s.model(&LayoutId::new(id)).expect("model loads");
            assert_eq!(m.language(), lang);
            assert!(m.h() < 0.0 && m.h().is_finite(), "{id}: h = {}", m.h());
        }
    }

    #[test]
    fn embedded_models_know_their_words() {
        let s = default_scorer();
        let en = s.model(&LayoutId::new("us")).unwrap();
        let fa = s.model(&LayoutId::new("ir")).unwrap();
        assert!(en.word_log2prob("thanks") > en.word_log2prob("lvsd"));
        assert!(fa.word_log2prob("مرسی") > fa.word_log2prob("ثپسیض"));
    }
}
