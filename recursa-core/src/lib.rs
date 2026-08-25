//! Grammar-erased runtime support for Recursa-generated parsers.

mod grammar;
mod input;
mod parse;
mod span;

pub use grammar::{ExpectedAtomId, FollowSet, GrammarInfo, TokenKind};
pub use input::{Input, LexResult, TokenRecord};
pub use parse::{Parse, ParseError};
pub use span::{OffsetOverflow, Span};

/// Runtime operations reserved for Recursa-generated code.
///
/// This module is an implementation ABI, not a general-purpose parsing API.
/// It is supported only between matching versions of generated code and the
/// Recursa runtime.
#[doc(hidden)]
pub mod __private {
    use crate::{ExpectedAtomId, FollowSet};

    /// Builds a borrowed set constant emitted by matching Recursa codegen.
    pub const fn follow_set(
        kind_count: u32,
        kinds: &'static [u64],
        expected: &'static [ExpectedAtomId],
    ) -> FollowSet {
        FollowSet::from_static(kind_count, kinds, expected)
    }
}
