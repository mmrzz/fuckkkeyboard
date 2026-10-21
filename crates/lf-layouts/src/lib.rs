//! Built-in keyboard layout tables: US English and Persian Standard.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod ir_pes;
mod us;

pub use ir_pes::ir_pes;
pub use us::us;

use lf_core::Layout;

/// Every built-in layout.
pub fn all() -> Vec<Layout> {
    vec![us(), ir_pes()]
}
