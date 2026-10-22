//! Typed words that are clearly intentional and must never be converted
//! (spec section 2.3).

/// True if the as-typed word looks like a URL, email, identifier or path.
pub fn is_pinned(typed: &str) -> bool {
    is_url(typed)
        || is_email(typed)
        || typed.contains('_')
        || is_camel_case(typed)
        || is_path(typed)
}

fn is_url(w: &str) -> bool {
    w.contains("://") || w.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("www."))
}

fn is_email(w: &str) -> bool {
    w.find('@').is_some_and(|at| w[at + 1..].contains('.'))
}

fn is_camel_case(w: &str) -> bool {
    w.as_bytes()
        .windows(2)
        .any(|p| p[0].is_ascii_lowercase() && p[1].is_ascii_uppercase())
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

    #[test]
    fn urls() {
        assert!(is_pinned("https://x.com"));
        assert!(is_pinned("ftp://host"));
        assert!(is_pinned("www.example.org"));
        assert!(is_pinned("WWW.example.org"));
        assert!(!is_pinned("www"));
        assert!(!is_pinned("wwwx"));
    }

    #[test]
    fn emails() {
        assert!(is_pinned("me@site.com"));
        assert!(!is_pinned("@handle"));
        assert!(!is_pinned("a.b@host"));
        assert!(!is_pinned("me@"));
    }

    #[test]
    fn underscores_and_camel_case() {
        assert!(is_pinned("my_var"));
        assert!(is_pinned("_"));
        assert!(is_pinned("getUserName"));
        assert!(is_pinned("iPhone"));
        assert!(!is_pinned("Hello"));
        assert!(!is_pinned("NASA"));
        assert!(!is_pinned("lvsd"));
    }

    #[test]
    fn paths() {
        assert!(is_pinned("src/main.rs"));
        assert!(is_pinned("C:\\Users\\me"));
        // The rule needs letters on both sides; `:\` alone does not count.
        assert!(!is_pinned("C:\\"));
        assert!(is_pinned("and/or"));
        assert!(!is_pinned("1/2"));
        assert!(!is_pinned("/"));
        assert!(!is_pinned("a/"));
        assert!(!is_pinned("/usr"));
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
            let _ = is_pinned(w);
        }
        assert!(is_pinned("é/ü"));
        assert!(!is_pinned("ابپ"));
    }
}
