//! Grammar-erased runtime support for Recursa-generated parsers.

mod grammar;
mod input;
mod parse;
mod span;

pub use grammar::{ExpectedAtomId, FollowSet, FollowSetError, GrammarInfo, TokenKind};
pub use input::{Input, LexResult, TokenRecord};
pub use parse::{Parse, ParseError, ParseErrorKind};
pub use span::{OffsetOverflow, Span};

/// Runtime operations reserved for Recursa-generated code.
///
/// This module is an implementation ABI, not a general-purpose parsing API.
/// It is supported only between matching versions of generated code and the
/// Recursa runtime.
#[doc(hidden)]
pub mod __private {
    use crate::{ExpectedAtomId, FollowSet, Input, ParseError};

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
}
