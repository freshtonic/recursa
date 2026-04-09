//! Recursa -- derive recursive descent parsers from Rust types.
//!
//! This crate re-exports everything from `recursa-core` (traits, types)
//! and `recursa-derive` (proc macros), plus `regex` and `miette` so
//! downstream crates need only depend on `recursa`.

pub use recursa_core::*;
pub use recursa_derive::*;

/// Re-export of the `miette` crate for error diagnostics.
pub use miette;

/// See [`keywords!`](macro.keywords.html) for full documentation.
///
/// ```
/// recursa::keywords! {
///     Let   => "let",
///     While => "while",
///     If    => "if",
/// }
/// ```
#[doc(inline)]
pub use recursa_core::keywords;

/// See [`punctuation!`](macro.punctuation.html) for full documentation.
///
/// ```
/// recursa::punctuation! {
///     Plus   => r"\+",
///     LParen => r"\(",
/// }
/// ```
#[doc(inline)]
pub use recursa_core::punctuation;

/// See [`literals!`](macro.literals.html) for full documentation.
///
/// ```
/// recursa::literals! {
///     IntLiteral => r"[0-9]+",
///     Ident      => r"[a-zA-Z_][a-zA-Z0-9_]*",
/// }
/// ```
#[doc(inline)]
pub use recursa_core::literals;
