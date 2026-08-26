//! Grammar-erased runtime support for Recursa-generated parsers.

mod grammar;
mod input;
mod lex;
mod output;
mod parse;
mod parsed;
mod span;

pub(crate) use grammar::{ExpectedAtomId, FollowSet, GrammarInfo, TokenKind};
pub(crate) use input::{Input, LexResult, TokenRecord};
pub use output::{HasSpan, Vec1};
pub use parse::{ParseError, ParseErrorKind};
pub use parsed::{
    ElementMetadata, ElementRole, ElementView, Elements, Node, NodeView, Parse, Parsed, Repeated,
    RepeatedIter,
};
pub use span::{OffsetOverflow, Span};

/// Runtime operations reserved for Recursa-generated code.
///
/// This module is an implementation ABI, not a general-purpose parsing API.
/// It is supported only between matching versions of generated code and the
/// Recursa runtime.
#[doc(hidden)]
pub mod __private {
    pub use crate::grammar::{ExpectedAtomId, FollowSet, FollowSetError, GrammarInfo, TokenKind};
    pub use crate::input::{Input, LexResult, TokenRecord};
    pub use crate::lex::{LexAction, LexBumpError, LexContext, LexRule, lex};
    pub use crate::output::span_seal::Sealed as HasSpanSealed;
    pub use crate::parsed::{
        CapturePolicy, ContentText, ElementMetadata, GeneratedParse as Parse, NodeViewFactory,
        Occurrence, Occurrences, ParseContext, ParsedNode, Repeated, RepeatedIter, ViewCursor,
        parse, parse_seal::Sealed as ParseSealed, parse_spanned,
    };

    /// Constructs one generated element descriptor without exposing its fields.
    pub const fn element_metadata(
        name: Option<&'static str>,
        role: crate::ElementRole,
    ) -> ElementMetadata {
        ElementMetadata { name, role }
    }

    /// Constructs metadata emitted once by a matching grammar generation.
    pub const fn grammar_info(
        name: &'static str,
        kind_names: &'static [&'static str],
        atom_names: &'static [&'static str],
        lexical_error_kind: TokenKind,
    ) -> GrammarInfo {
        GrammarInfo {
            name,
            kind_names,
            atom_names,
            lexical_error_kind,
        }
    }

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

    /// Builds a borrowed set constant emitted by matching Recursa codegen.
    pub const fn follow_set(
        kind_count: u32,
        kinds: &'static [u64],
        expected: &'static [ExpectedAtomId],
    ) -> FollowSet {
        FollowSet::from_static(kind_count, kinds, expected)
    }

    /// Returns the source retained by a grammar-local generated lexing result.
    pub fn lex_source<'input>(lexed: &LexResult<'input>) -> &'input str {
        lexed.source()
    }

    /// Returns payload-free records for a generated typed token view.
    pub fn lex_records<'lexed>(lexed: &'lexed LexResult<'_>) -> &'lexed [TokenRecord] {
        lexed.records()
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
