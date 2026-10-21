//! Built-in keyboard layout tables: US English and Persian Standard.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod us;

pub use us::us;

use lf_core::Layout;

/// Every built-in layout.
pub fn all() -> Vec<Layout> {
    vec![us()]
}
