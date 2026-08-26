//! Grammar-erased runtime support for Recursa-generated parsers.

mod grammar;
mod input;
mod lex;
mod output;
mod parse;
mod parsed;
mod span;

pub use grammar::{ExpectedAtomId, FollowSet, FollowSetError, GrammarInfo, TokenKind};
pub use input::{Input, LexResult, TokenRecord};
pub use output::{HasSpan, SpanBlock, Vec1};
pub use parse::{Parse, ParseError, ParseErrorKind};
pub use parsed::{Node, NodeView, Parsed};
pub use span::{OffsetOverflow, Span};

/// Runtime operations reserved for Recursa-generated code.
///
/// This module is an implementation ABI, not a general-purpose parsing API.
/// It is supported only between matching versions of generated code and the
/// Recursa runtime.
#[doc(hidden)]
pub mod __private {
    use crate::{ExpectedAtomId, FollowSet, Input, ParseError, Span, SpanBlock};

    pub use crate::lex::{LexRule, lex};
    pub use crate::parsed::{
        CapturePolicy, ContentText, NodeViewFactory, Occurrence, Parse, ParseContext, ParsedNode,
        ViewCursor, parse, parse_spanned,
    };

    /// Type-level identity of one exact facade release and private protocol.
    ///
    /// Matching generated root code names its producing release and protocol as
    /// const arguments and assigns [`FACADE_COMPATIBILITY`] to that exact type.
    #[derive(Clone, Copy, Debug)]
    pub struct FacadeCompatibility<const RELEASE: u64, const PRIVATE_PROTOCOL: u32> {
        _private: (),
    }

    /// Stable type-level encoding of an exact Cargo package release string.
    pub const fn release_stamp(release: &str) -> u64 {
        let bytes = release.as_bytes();
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut index = 0;
        while index < bytes.len() {
            hash ^= bytes[index] as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
            index += 1;
        }
        hash
    }

    /// Compatibility capability supplied by this exact facade/runtime build.
    pub const FACADE_COMPATIBILITY: FacadeCompatibility<
        { release_stamp(env!("CARGO_PKG_VERSION")) },
        1,
    > = FacadeCompatibility { _private: () };

    /// Opaque cursor state used by generated speculative parsing.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Checkpoint(usize);

    /// Builds a borrowed set constant emitted by matching Recursa codegen.
    pub const fn follow_set(
        kind_count: u32,
        kinds: &'static [u64],
        expected: &'static [ExpectedAtomId],
    ) -> FollowSet {
        FollowSet::from_static(kind_count, kinds, expected)
    }

    /// Creates the structured failure at the current generated-parser cursor.
    pub fn unexpected<A>(input: &Input<'_, A>, expected: &FollowSet) -> ParseError {
        ParseError::unexpected(input, expected)
    }

    /// Captures the current generated-parser cursor.
    pub fn checkpoint<A>(input: &Input<'_, A>) -> Checkpoint {
        Checkpoint(input.checkpoint())
    }

    /// Restores a cursor captured from this lexing result.
    pub fn rewind<A>(input: &mut Input<'_, A>, checkpoint: Checkpoint) {
        input.rewind(checkpoint.0);
    }

    /// Whether generated parsers should populate optional span metadata.
    pub fn captures_spans<A>(input: &Input<'_, A>) -> bool {
        input.captures_spans()
    }

    /// Tight end of the most recently consumed token.
    pub fn consumed_end<A>(input: &Input<'_, A>) -> usize {
        input.consumed_end()
    }

    /// Returns the source retained by a grammar-local generated lexing result.
    pub fn lex_source<'input>(lexed: &crate::LexResult<'input>) -> &'input str {
        lexed.source()
    }

    /// Returns payload-free records for a generated typed token view.
    pub fn lex_records<'lexed>(
        lexed: &'lexed crate::LexResult<'_>,
    ) -> &'lexed [crate::TokenRecord] {
        lexed.records()
    }

    /// Populates a generated node's public extent.
    pub fn set_node_span<const N: usize>(block: &mut SpanBlock<N>, span: Span) {
        block.set_node(span);
    }

    /// Reads a generated grammar element's span by its emitted index.
    pub fn element_span<const N: usize>(block: &SpanBlock<N>, index: usize) -> Option<Span> {
        block.element(index)
    }

    /// Populates a generated grammar element's span by its emitted index.
    pub fn set_element_span<const N: usize>(
        block: &mut SpanBlock<N>,
        index: usize,
        span: Span,
    ) -> bool {
        block.set_element(index, span)
    }
}

#[cfg(test)]
mod compatibility_tests {
    #[test]
    fn core_exposes_the_exact_release_and_private_protocol_capability() {
        let _: super::__private::FacadeCompatibility<
            { super::__private::release_stamp("0.1.0") },
            1,
        > = super::__private::FACADE_COMPATIBILITY;
    }
}
