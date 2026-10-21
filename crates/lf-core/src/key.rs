//! Physical keys and modifiers.

use crate::layout::LayoutId;

macro_rules! key_codes {
    ($($variant:ident),+ $(,)?) => {
        /// Physical key, named after W3C UI Events `KeyboardEvent.code` values.
        /// Platform backends (later phases) map OS scan codes to these.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum KeyCode {
            $(
                #[doc = concat!("The `", stringify!($variant), "` key.")]
                $variant,
            )+
        }

        impl KeyCode {
            /// Every key code, in declaration order.
            pub const ALL: &'static [KeyCode] = &[$(KeyCode::$variant),+];

            /// Parse a W3C code string, e.g. "KeyF" -> KeyCode::KeyF.
            pub fn from_w3c(s: &str) -> Option<KeyCode> {
                match s {
                    $(stringify!($variant) => Some(KeyCode::$variant),)+
                    _ => None,
                }
            }

            /// The W3C code string for this key, e.g. KeyCode::KeyF -> "KeyF".
            pub fn as_w3c(&self) -> &'static str {
                match self {
                    $(KeyCode::$variant => stringify!($variant),)+
                }
            }
        }
    };
}

key_codes! {
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    Backquote, Minus, Equal, BracketLeft, BracketRight, Backslash,
    Semicolon, Quote, Comma, Period, Slash, Space,
}

/// Modifier state relevant to which character a key produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift is held.
    pub shift: bool,
    /// AltGr (third level) is held.
    pub altgr: bool,
}

impl Modifiers {
    /// No modifiers.
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        altgr: false,
    };
    /// Shift only.
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        altgr: false,
    };

    /// True if no modifier is held.
    pub fn is_none(&self) -> bool {
        *self == Modifiers::NONE
    }
}

/// One printable key press as seen by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    /// The physical key.
    pub code: KeyCode,
    /// Modifiers held while pressing it.
    pub mods: Modifiers,
    /// Layout active when the key was pressed.
    pub layout: LayoutId,
    /// What actually appeared on screen (None = dead key / nothing typed).
    pub produced: Option<char>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn w3c_round_trip() {
        for &code in KeyCode::ALL {
            assert_eq!(KeyCode::from_w3c(code.as_w3c()), Some(code));
        }
        assert_eq!(KeyCode::ALL.len(), 48);
    }

    #[test]
    fn w3c_examples() {
        assert_eq!(KeyCode::from_w3c("KeyF"), Some(KeyCode::KeyF));
        assert_eq!(KeyCode::from_w3c("Space"), Some(KeyCode::Space));
        assert_eq!(KeyCode::BracketLeft.as_w3c(), "BracketLeft");
        assert_eq!(KeyCode::from_w3c("keyf"), None);
        assert_eq!(KeyCode::from_w3c("Enter"), None);
        assert_eq!(KeyCode::from_w3c(""), None);
    }

    #[test]
    fn modifiers_default_is_none() {
        assert!(Modifiers::default().is_none());
        assert!(!Modifiers::SHIFT.is_none());
    }
}
