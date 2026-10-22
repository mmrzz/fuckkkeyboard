//! Typed words that are clearly intentional and must never be converted
//! (spec section 2.3).

use crate::alphabet::Language;

/// True if the as-typed word looks like a URL, email, identifier or path.
///
/// `rendered` is the same keys in the target layout and `target` its
/// language; they refine the camelCase rule (see [`is_camel_case`]).
pub fn is_pinned(typed: &str, rendered: &str, target: Language) -> bool {
    is_url(typed)
        || is_email(typed)
        || typed.contains('_')
        || is_camel_case(typed, rendered, target)
        || is_path(typed)
}

fn is_url(w: &str) -> bool {
    w.contains("://") || w.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("www."))
}

fn is_email(w: &str) -> bool {
    w.find('@').is_some_and(|at| w[at + 1..].contains('.'))
}

/// A lowercase ASCII letter directly followed by an uppercase one that is not
/// the last char, where the uppercase key does not type a letter in the
/// target layout.
///
/// Persian Standard puts letters on Shift (`آ` on H, `ئ` on S, `ء` on M), so
/// Persian typed on US looks camelCase: `درآمد` comes out as `nvHln`. A hump
/// whose key types a target-language letter is explained by that, so it is
/// not evidence of an identifier and the LLR decides instead. A hump on the
/// last char is skipped too: that is word-final tanwin (`مسلماً` is `lsglhR`),
/// which identifiers almost never look like. `getUserName` stays pinned: U
/// and N type a fatha and a hamza mark, not letters. When `typed` and
/// `rendered` do not line up char for char, every hump counts.
fn is_camel_case(typed: &str, rendered: &str, target: Language) -> bool {
    let typed: Vec<char> = typed.chars().collect();
    let rendered: Vec<char> = rendered.chars().collect();
    let aligned = typed.len() == rendered.len();
    let mut buf = [0u8; 4];
    (1..typed.len()).any(|i| {
        let hump = typed[i - 1].is_ascii_lowercase() && typed[i].is_ascii_uppercase();
        let explained = aligned
            && (i == typed.len() - 1
                || !target
                    .normalize(rendered[i].encode_utf8(&mut buf))
                    .is_empty());
        hump && !explained
    })
}

/// A `/` or `\` directly between two letters, e.g. `src/main`.
fn is_path(w: &str) -> bool {
    let chars: Vec<char> = w.chars().collect();
    chars
        .windows(3)
        .any(|t| matches!(t[1], '/' | '\\') && t[0].is_alphabetic() && t[2].is_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lf_core::{render, KeyPress};

    /// The spec 2.3 rule alone: with no aligned rendering, every hump counts.
    fn pinned(w: &str) -> bool {
        is_pinned(w, "", Language::Fa)
    }

    /// Pinned when `w` is typed on US and would be converted to Persian.
    fn pinned_us(w: &str) -> bool {
        let us = lf_layouts::us();
        let keys: Vec<KeyPress> = w
            .chars()
            .map(|ch| {
                let (code, mods) = us.key_for(ch).unwrap();
                KeyPress {
                    code,
                    mods,
                    layout: us.id.clone(),
                    produced: Some(ch),
                }
            })
            .collect();
        is_pinned(w, &render(&keys, &lf_layouts::ir_pes()), Language::Fa)
    }

    #[test]
    fn camel_case_humps_on_persian_letter_keys_do_not_count() {
        // Identifiers whose humps type marks, not letters, stay pinned.
        assert!(pinned_us("getUserName"));
        assert!(pinned_us("iPhone"));
        assert!(pinned_us("myVar"));
        assert!(pinned_us("getDataById"));
        // Persian typed on US: آ ئ ء and tanwin are on Shift.
        assert!(!pinned_us("nvHln")); // درآمد
        assert!(!pinned_us("lsSgi")); // مسئله
        assert!(!pinned_us("lsglhR")); // مسلماً, tanwin is the last char
        assert!(!pinned_us("lh,vhM")); // ماوراء
                                       // Humps only on letter keys or at the end: left to the LLR.
        assert!(!pinned_us("getData"));
        assert!(!pinned_us("getX"));
        assert!(!pinned_us("Hello"));
    }

    #[test]
    fn urls() {
        assert!(pinned("https://x.com"));
        assert!(pinned("ftp://host"));
        assert!(pinned("www.example.org"));
        assert!(pinned("WWW.example.org"));
        assert!(!pinned("www"));
        assert!(!pinned("wwwx"));
    }

    #[test]
    fn emails() {
        assert!(pinned("me@site.com"));
        assert!(!pinned("@handle"));
        assert!(!pinned("a.b@host"));
        assert!(!pinned("me@"));
    }

    #[test]
    fn underscores_and_camel_case() {
        assert!(pinned("my_var"));
        assert!(pinned("_"));
        assert!(pinned("getUserName"));
        assert!(pinned("iPhone"));
        assert!(!pinned("Hello"));
        assert!(!pinned("NASA"));
        assert!(!pinned("lvsd"));
    }

    #[test]
    fn paths() {
        assert!(pinned("src/main.rs"));
        assert!(pinned("C:\\Users\\me"));
        // The rule needs letters on both sides; `:\` alone does not count.
        assert!(!pinned("C:\\"));
        assert!(pinned("and/or"));
        assert!(!pinned("1/2"));
        assert!(!pinned("/"));
        assert!(!pinned("a/"));
        assert!(!pinned("/usr"));
    }

    #[test]
    fn odd_input_does_not_panic() {
        for w in [
            "",
            "@",
            "w",
            "ww",
            "سلام",
            "é/ü",
            "\u{200C}",
            &"x".repeat(1000),
        ] {
            let _ = pinned(w);
        }
        assert!(pinned("é/ü"));
        assert!(!pinned("ابپ"));
    }
}
