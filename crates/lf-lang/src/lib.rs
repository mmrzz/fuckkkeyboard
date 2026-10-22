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
pub use scorer::{FrequencyBonus, NgramScorer};

use lf_core::LayoutId;

/// The embedded English model.
pub const EN_MODEL: &[u8] = include_bytes!("../../../models/en.lfng");
/// The embedded Persian model.
pub const FA_MODEL: &[u8] = include_bytes!("../../../models/fa.lfng");

/// Error building a scorer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LangError {
    /// Fewer than two layouts have a language with an embedded model.
    #[error("need two layouts with known languages, found {0}")]
    TooFewLanguages(usize),
    /// An embedded model failed to load.
    #[error("embedded model failed to load: {0}")]
    Model(#[from] ModelError),
}

/// The embedded model for a language.
pub fn embedded_model(lang: Language) -> &'static [u8] {
    match lang {
        Language::En => EN_MODEL,
        Language::Fa => FA_MODEL,
    }
}

/// Build a scorer from (layout id, language code) pairs using the embedded models.
///
/// Skips pairs with an unknown language; errors if fewer than two remain.
pub fn scorer_for(pairs: &[(LayoutId, String)]) -> Result<NgramScorer, LangError> {
    let known: Vec<(LayoutId, Language)> = pairs
        .iter()
        .filter_map(|(id, code)| Some((id.clone(), Language::from_code(code)?)))
        .collect();
    if known.len() < 2 {
        return Err(LangError::TooFewLanguages(known.len()));
    }
    let models = known
        .into_iter()
        .map(|(id, lang)| Ok((id, LanguageModel::from_bytes(embedded_model(lang))?)))
        .collect::<Result<_, LangError>>()?;
    Ok(NgramScorer::new(models))
}

/// Scorer with the embedded models, mapping "us" -> en, "ir" -> fa.
pub fn default_scorer() -> NgramScorer {
    scorer_for(&[
        (LayoutId::new("us"), "en".into()),
        (LayoutId::new("ir"), "fa".into()),
    ])
    .expect("embedded models load (checked by tests)")
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

    #[test]
    fn scorer_for_skips_unknown_languages() {
        let s = scorer_for(&[
            (LayoutId::new("gb"), "en".into()),
            (LayoutId::new("de"), "de".into()),
            (LayoutId::new("fa2"), "fa".into()),
        ])
        .unwrap();
        assert_eq!(
            s.model(&LayoutId::new("gb")).unwrap().language(),
            Language::En
        );
        assert_eq!(
            s.model(&LayoutId::new("fa2")).unwrap().language(),
            Language::Fa
        );
        assert!(s.model(&LayoutId::new("de")).is_none());
    }

    #[test]
    fn scorer_for_needs_two_languages() {
        let err = scorer_for(&[
            (LayoutId::new("us"), "en".into()),
            (LayoutId::new("de"), "de".into()),
        ])
        .unwrap_err();
        assert_eq!(err, LangError::TooFewLanguages(1));
    }
}
