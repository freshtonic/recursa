//! Railroad syntax diagrams for recursa-derived AST types.
//!
//! This is a facade crate. Layout primitives and SVG rendering live in
//! `recursa-diagram-core`; the `#[railroad]` attribute macro lives in
//! `recursa-diagram-macros`. Users import everything from here.

pub use recursa_diagram_core::{layout, render};
// pub use recursa_diagram_macros::railroad; // added in Phase 4 Task 13
