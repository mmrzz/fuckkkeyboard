//! Windows backend (Phase 3, tasks 4–7).
//!
//! The one module in this crate allowed to use `unsafe`: each FFI call is
//! wrapped in a small safe function with a `// SAFETY:` comment.
#![allow(unsafe_code)]
