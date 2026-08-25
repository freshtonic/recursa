//! Grammar-erased runtime support for Recursa-generated parsers.

mod grammar;
mod input;
mod output;
mod parse;
mod span;

pub use grammar::{ExpectedAtomId, FollowSet, FollowSetError, GrammarInfo, TokenKind};
pub use input::{Input, LexResult, TokenRecord};
pub use output::{HasSpan, SpanBlock, Vec1};
pub use parse::{Parse, ParseError, ParseErrorKind};
pub use span::{OffsetOverflow, Span};

/// Runtime operations reserved for Recursa-generated code.
///
/// This module is an implementation ABI, not a general-purpose parsing API.
/// It is supported only between matching versions of generated code and the
/// Recursa runtime.
#[doc(hidden)]
pub mod __private {
    use crate::{ExpectedAtomId, FollowSet, Input, ParseError, Span, SpanBlock};

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
