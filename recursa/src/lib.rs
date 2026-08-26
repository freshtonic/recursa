//! Recursa — an AST-first parser generator. The AST is the grammar.
//!
//! Facade crate: re-exports the supported grammar-erased runtime surface.
//! Design: see the recursa-spike repo, docs/api-design.md.

pub use recursa_core::{
    ExpectedAtomId, FollowSet, FollowSetError, GrammarInfo, HasSpan, Input, LexResult, Node,
    NodeView, OffsetOverflow, Parse, ParseError, ParseErrorKind, Parsed, Span, SpanBlock,
    TokenKind, TokenRecord, Vec1,
};
pub use recursa_macros::{Node, grammar, tokens};

/// Runtime operations reserved for Recursa-generated code.
///
/// This is a matching-version ABI and is not a general-purpose parsing API.
#[doc(hidden)]
pub mod __private {
    pub use recursa_core::__private::*;
}

#[cfg(test)]
mod compatibility_tests {
    #[test]
    fn facade_reexports_the_exact_release_and_private_protocol_capability() {
        let _: super::__private::FacadeCompatibility<
            { super::__private::release_stamp("0.1.0") },
            1,
        > = super::__private::FACADE_COMPATIBILITY;
    }
}
