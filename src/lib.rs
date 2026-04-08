//! Recursa -- derive recursive descent parsers from Rust types.
//!
//! This crate re-exports everything from `recursa-core` (traits, types)
//! and `recursa-derive` (proc macros), plus `regex` and `miette` so
//! downstream crates need only depend on `recursa`.

pub use recursa_core::*;
pub use recursa_derive::*;

/// Re-export of the `miette` crate for error diagnostics.
pub use miette;
