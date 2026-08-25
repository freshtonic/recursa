//! Recursa — an AST-first parser generator. The AST is the grammar.
//!
//! Facade crate: re-exports the supported grammar-erased runtime surface.
//! Design: see the recursa-spike repo, docs/api-design.md.

pub use recursa_core::{
    ExpectedAtomId, FollowSet, GrammarInfo, Input, LexResult, OffsetOverflow, Parse, ParseError,
    Span, TokenKind, TokenRecord,
};

/// Runtime operations reserved for Recursa-generated code.
///
/// This is a matching-version ABI and is not a general-purpose parsing API.
#[doc(hidden)]
pub mod __private {
    pub use recursa_core::__private::*;
}
