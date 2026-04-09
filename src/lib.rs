//! Recursa -- derive recursive descent parsers from Rust types.
//!
//! This crate re-exports everything from `recursa-core` (traits, types)
//! and `recursa-derive` (proc macros), plus `regex` and `miette` so
//! downstream crates need only depend on `recursa`.

pub use recursa_core::*;
pub use recursa_derive::*;

/// Re-export of the `miette` crate for error diagnostics.
pub use miette;

/// Declare keyword token types and a combined `Keyword` enum.
///
/// Each entry generates a unit struct with `#[derive(Scan)]` and the
/// specified pattern. A combined `Keyword` enum is also generated
/// with all variants.
///
/// # Example
///
/// ```
/// recursa::keywords! {
///     Let   => "let",
///     While => "while",
///     If    => "if",
/// }
/// ```
///
/// Expands to unit structs `Let`, `While`, `If` (each implementing `Scan`)
/// plus an enum `Keyword` with variants `Keyword::Let(Let)`, etc.
#[doc(inline)]
pub use recursa_core::keywords;

/// Declare punctuation token types and a combined `Punctuation` enum.
///
/// Each entry generates a unit struct with `#[derive(Scan)]` and the
/// specified pattern. A combined `Punctuation` enum is also generated
/// with all variants.
///
/// Patterns must be valid regex. For literal punctuation characters that
/// are regex metacharacters, provide already-escaped patterns
/// (e.g., `r"\+"` not `"+"`).
///
/// # Example
///
/// ```
/// recursa::punctuation! {
///     Plus   => r"\+",
///     LParen => r"\(",
/// }
/// ```
#[doc(inline)]
pub use recursa_core::punctuation;

/// Declare literal/capturing token types and a combined `Literal` enum.
///
/// Each entry generates a tuple struct wrapping `&'input str` with
/// `#[derive(Scan)]` and the specified pattern. A combined `Literal`
/// enum is also generated with all variants.
///
/// # Example
///
/// ```
/// recursa::literals! {
///     IntLiteral => r"[0-9]+",
///     Ident      => r"[a-zA-Z_][a-zA-Z0-9_]*",
/// }
/// ```
#[doc(inline)]
pub use recursa_core::literals;
