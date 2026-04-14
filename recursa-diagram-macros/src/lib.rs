//! Proc-macro attribute for `recursa-diagram`.
//!
//! This crate will provide the `#[railroad]` attribute macro that generates
//! railroad syntax diagrams for recursa-derived AST types. The attribute will
//! be added in Phase 4 of the implementation plan; for now this crate is an
//! empty shell so the workspace wiring is in place.
//!
//! Proc-macro crates cannot export non-proc-macro items, which is why the
//! runtime layout and SVG rendering code lives in the sibling `recursa-diagram`
//! crate (the same split serde uses between `serde` and `serde_derive`).
