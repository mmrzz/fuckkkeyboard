//! Languages, their normalization and their symbol alphabets.

/// Alphabet index of the start pad `^`.
pub const PAD: u8 = 0;
/// Alphabet index of the end marker `$`.
pub const END: u8 = 1;
/// Alphabet index of the out-of-alphabet symbol `?`.
pub const UNKNOWN: u8 = 2;

/// Largest allowed alphabet: an index must fit in 6 bits.
pub const MAX_ALPHABET: usize = 63;

const ZWNJ: char = '\u{200C}';

/// `^ $ ?`, then `a`–`z` and `'`.
const EN_ALPHABET: &[char] = &[
    '^', '$', '?', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '\'',
];

/// `^ $ ?`, the 32 Persian letters, then `آ ئ ء` and ZWNJ.
const FA_ALPHABET: &[char] = &[
    '^', '$', '?',        //
    '\u{0627}', // ا alef
    '\u{0628}', // ب beh
    '\u{067E}', // پ peh
    '\u{062A}', // ت teh
    '\u{062B}', // ث theh
    '\u{062C}', // ج jeem
    '\u{0686}', // چ tcheh
    '\u{062D}', // ح hah
    '\u{062E}', // خ khah
    '\u{062F}', // د dal
    '\u{0630}', // ذ thal
    '\u{0631}', // ر reh
    '\u{0632}', // ز zain
    '\u{0698}', // ژ jeh
    '\u{0633}', // س seen
    '\u{0634}', // ش sheen
    '\u{0635}', // ص sad
    '\u{0636}', // ض dad
    '\u{0637}', // ط tah
    '\u{0638}', // ظ zah
    '\u{0639}', // ع ain
    '\u{063A}', // غ ghain
    '\u{0641}', // ف feh
    '\u{0642}', // ق qaf
    '\u{06A9}', // ک keheh
    '\u{06AF}', // گ gaf
    '\u{0644}', // ل lam
    '\u{0645}', // م meem
    '\u{0646}', // ن noon
    '\u{0648}', // و waw
    '\u{0647}', // ه heh
    '\u{06CC}', // ی farsi yeh
    '\u{0622}', // آ alef with madda
    '\u{0626}', // ئ yeh with hamza
    '\u{0621}', // ء hamza
    ZWNJ,
];

/// A language with a character model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// English.
    En,
    /// Persian.
    Fa,
}

impl Language {
    /// Every language.
    pub const ALL: [Language; 2] = [Language::En, Language::Fa];

    /// Two-letter code: "en" / "fa".
    pub fn code(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Fa => "fa",
        }
    }

    /// Parse a two-letter code.
    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The symbols of this language: index 0 = `^`, 1 = `$`, 2 = `?`.
    pub fn alphabet(&self) -> &'static [char] {
        match self {
            Language::En => EN_ALPHABET,
            Language::Fa => FA_ALPHABET,
        }
    }

    /// Map a string to this language's alphabet (spec section 2.4): fold
    /// variants, then drop every char outside the alphabet.
    pub fn normalize(&self, s: &str) -> String {
        s.chars()
            .filter_map(|c| {
                let c = self.fold(c)?;
                (self.is_letter(c)).then_some(c)
            })
            .collect()
    }

    /// True if `c` is a real letter of this language (not `^ $ ?`).
    pub fn is_letter(&self, c: char) -> bool {
        self.alphabet()[3..].contains(&c)
    }

    /// Alphabet index of `c`, or `?` if `c` is not in the alphabet.
    pub fn symbol(&self, c: char) -> u8 {
        self.alphabet()
            .iter()
            .position(|&a| a == c)
            .map_or(UNKNOWN, |i| i as u8)
    }

    /// Fold one char to its canonical form; None drops it.
    fn fold(&self, c: char) -> Option<char> {
        match self {
            Language::En => Some(c.to_ascii_lowercase()),
            Language::Fa => match c {
                '\u{064A}' => Some('\u{06CC}'),               // ي -> ی
                '\u{0643}' => Some('\u{06A9}'),               // ك -> ک
                '\u{0629}' => Some('\u{0647}'),               // ة -> ه
                '\u{0623}' | '\u{0625}' => Some('\u{0627}'),  // أ إ -> ا
                '\u{0624}' => Some('\u{0648}'),               // ؤ -> و
                '\u{064B}'..='\u{0652}' | '\u{0640}' => None, // diacritics, tatweel
                _ => Some(c),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphabet_sizes() {
        assert_eq!(Language::En.alphabet().len(), 30);
        assert_eq!(Language::Fa.alphabet().len(), 39);
        for lang in Language::ALL {
            let a = lang.alphabet();
            assert!(a.len() <= MAX_ALPHABET);
            assert_eq!(&a[..3], &['^', '$', '?']);
            for (i, c) in a.iter().enumerate() {
                assert!(!a[i + 1..].contains(c), "{c:?} repeated");
            }
        }
    }

    #[test]
    fn codes() {
        assert_eq!(Language::En.code(), "en");
        assert_eq!(Language::Fa.code(), "fa");
        assert_eq!(Language::from_code("fa"), Some(Language::Fa));
        assert_eq!(Language::from_code("de"), None);
    }

    #[test]
    fn english_normalization() {
        let en = Language::En;
        assert_eq!(en.normalize("I'm"), "i'm");
        assert_eq!(en.normalize("Hello, World!"), "helloworld");
        assert_eq!(en.normalize("x2y_z-é"), "xyz");
        assert_eq!(en.normalize("سلام"), "");
        assert_eq!(en.normalize(""), "");
    }

    #[test]
    fn persian_folding() {
        let fa = Language::Fa;
        assert_eq!(fa.normalize("\u{064A}"), "\u{06CC}");
        assert_eq!(fa.normalize("\u{0643}"), "\u{06A9}");
        assert_eq!(fa.normalize("\u{0629}"), "\u{0647}");
        assert_eq!(fa.normalize("\u{0623}\u{0625}"), "\u{0627}\u{0627}");
        assert_eq!(fa.normalize("\u{0624}"), "\u{0648}");
        assert_eq!(fa.normalize("علي"), "علی");
    }

    #[test]
    fn persian_drops_diacritics_and_tatweel() {
        let fa = Language::Fa;
        for c in '\u{064B}'..='\u{0652}' {
            assert_eq!(fa.normalize(&format!("ب{c}ا")), "با");
        }
        assert_eq!(fa.normalize("بـــا"), "با");
    }

    #[test]
    fn persian_keeps_zwnj_and_extra_letters() {
        let fa = Language::Fa;
        assert_eq!(fa.normalize("می\u{200C}خواهم"), "می\u{200C}خواهم");
        assert_eq!(fa.normalize("آئء"), "آئء");
    }

    #[test]
    fn persian_drops_other_chars() {
        let fa = Language::Fa;
        assert_eq!(fa.normalize("سلام!۱۲ abc «»،؟."), "سلام");
        assert_eq!(fa.normalize("\u{200D}"), "");
    }

    #[test]
    fn symbols() {
        assert_eq!(Language::En.symbol('^'), PAD);
        assert_eq!(Language::En.symbol('$'), END);
        assert_eq!(Language::En.symbol('a'), 3);
        assert_eq!(Language::En.symbol('\''), 29);
        assert_eq!(Language::En.symbol('A'), UNKNOWN);
        assert_eq!(Language::Fa.symbol('\u{0627}'), 3);
        assert_eq!(Language::Fa.symbol(ZWNJ), 38);
        assert_eq!(Language::Fa.symbol('a'), UNKNOWN);
    }
}
