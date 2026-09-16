//! Grammar-erased runtime support for Recursa-generated parsers.

#[cfg(feature = "arbitrary")]
/// Recursion-depth guard bounding `Arbitrary`-generated Node construction.
mod arbitrary_depth;
#[cfg(feature = "explorer")]
pub mod explorer;
pub mod flat;
/// The storage-selection contracts a generated Flat AST owner implements.
pub mod flat_owner;
/// The rendering contract a generated flat Node type implements per Flat AST store.
pub mod flat_pretty;
/// Grammar-erased Flat AST traversal: the children contract and the depth-first walk.
pub mod flat_visit;
#[cfg(all(feature = "explorer", feature = "spans"))]
pub mod framing;
/// Static grammar identity, token kinds, and compact follow-set representation.
mod grammar;
/// Grammar-bound lexical output and parser cursor state.
mod input;
/// Generated lexer execution protocol.
mod lex;
#[cfg(test)]
mod lex_tests;
/// The table-driven parser's runtime loop, tables and value stack.
pub mod lr;
/// Span-bearing output traits and non-empty semantic sequences.
mod output;
/// Structured low-level parse failures.
mod parse;
/// Parsed provenance, occurrence views, and generated parsing protocol.
mod parsed;
/// Structured Pretty rendering and source maps.
mod pretty;
/// Checked source byte ranges.
mod span;
/// Grammar-independent AST traversal contracts.
mod visitor;

pub use bumpalo::boxed::Box as ArenaBox;
pub use bumpalo::collections::Vec as ArenaVec;
pub use flat::{
    FlatNode, Handle, List, List1, ListItem, ListScratch, ListStore, OpenList, SharedPool,
    SideTable, Text, TextPool, TypedPool,
};
pub(crate) use grammar::{ExpectedAtomId, FollowSet, GrammarInfo, TokenKind};
pub(crate) use input::Input;
pub use output::{ArenaVec1, HasSpan, Vec1};
pub use parse::{ParseError, ParseErrorKind};
pub use parsed::{
    ArenaAstFamily, ArenaParse, ArenaParsed, DebugProvenance, ElementMetadata, ElementRole,
    ElementView, Elements, Node, NodeView, Parse, Parsed, Repeated, RepeatedIter,
};
pub use pretty::{Pretty, PrettyConfig, PrettyContext, PrettySink, SourceMap, SourceMapEntry};
pub use span::{OffsetOverflow, Span};
pub use visitor::{
    TotalVisitor, TotalVisitorMut, Visit, VisitBreak, VisitMut, Visitor, VisitorMut,
};

/// Runtime operations reserved for Recursa-generated code.
///
/// This module is an implementation ABI, not a general-purpose parsing API.
/// It is supported only between matching versions of generated code and the
/// Recursa runtime.
#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "arbitrary")]
    pub use crate::arbitrary_depth::{
        DepthGuard as ArbitraryDepthGuard, MAX_DEPTH as ARBITRARY_MAX_DEPTH,
    };
    #[cfg(all(feature = "explorer", feature = "arbitrary"))]
    pub use crate::explorer::ArenaArbitraryRootFamily;
    #[cfg(all(feature = "explorer", feature = "arbitrary"))]
    pub use crate::explorer::sample_arena_root as explorer_sample_arena_root;
    #[cfg(all(feature = "explorer", feature = "arbitrary"))]
    pub use crate::explorer::sample_root as explorer_sample_root;
    #[cfg(feature = "explorer")]
    pub use crate::explorer::{
        ArenaExplorerRootFamily, ElementSchemaData as ExplorerElementSchemaData,
        ElementTargetData as ExplorerElementTargetData, ExplorerField, ExplorerNode, ExplorerRoot,
        ExplorerRootFamily, ExplorerSeq, GrammarTablesData as ExplorerGrammarTablesData,
        NodeData as ExplorerNodeData, NodeSchemaData as ExplorerNodeSchemaData,
        RawLexFn as ExplorerRawLexFn, SampleFn as ExplorerSampleFn,
        TokenSchemaData as ExplorerTokenSchemaData, VariantSchemaData as ExplorerVariantSchemaData,
        construct_adapter as explorer_adapter,
        content_token_target as explorer_content_token_target,
        element_schema as explorer_element_schema, field_many as explorer_field_many,
        field_one as explorer_field_one, field_optional as explorer_field_optional,
        field_optional_many as explorer_field_optional_many,
        grammar_tables as explorer_grammar_tables, lex_record as explorer_lex_record,
        lex_result as explorer_lex_result, node as explorer_node,
        node_child as explorer_node_child, node_schema as explorer_node_schema,
        node_target as explorer_node_target, parse_arena_root as explorer_parse_arena_root,
        parse_arena_typed_root as explorer_parse_arena_typed_root,
        parse_root as explorer_parse_root, pretty_root as explorer_pretty_root,
        token_node as explorer_token_node, token_schema as explorer_token_schema,
        variant_schema as explorer_variant_schema,
    };
    pub use crate::flat::{
        FlatNode, Handle, List, List1, ListItem, ListScratch, ListStore, OpenList, SharedPool,
        SideTable, Text, TextPool, TypedPool,
    };
    pub use crate::flat_owner::{FlatPool, FlatStorage};
    pub use crate::flat_visit::{
        FlatChildren, FlatVisitor, OpaqueRef, OpaqueTable, PostOrderViolation,
        check_post_order_pair, walk,
    };
    #[cfg(all(feature = "explorer", feature = "spans"))]
    pub use crate::framing::{
        PayloadPlanData as FramingPayloadPlanData, PlanData as FramingPlanData,
        frame_arena_document, frame_document, payload_plan as framing_payload_plan,
        plan as framing_plan,
    };
    pub use crate::grammar::{ExpectedAtomId, FollowSet, GrammarInfo, TokenKind};
    pub use crate::input::{
        Input, LexResult, SourceGap, SourceGaps, TokenRecord, TriviaRecord, TriviaRecordView,
        TriviaRecords,
    };
    pub use crate::lex::{
        ByteSet, FixedLexLiteral, LexClosedMatch, LexDiagnostic, LexDiagnosticCode, LexExtras,
        LexFailure, LexPlan, LexRule, LexTokenIndex, OperatorRunRule, lex, lex_nested,
        lex_next_exclusion, lex_physical_line, lex_same_delimiter, scan_closed,
    };
    pub use crate::lr::{
        LrTables, Reduction as LrReduction, run as lr_run, run_flat as lr_run_flat,
    };
    pub use crate::output::span_seal::Sealed as HasSpanSealed;
    #[cfg(feature = "arbitrary")]
    pub use crate::parsed::ArenaSynthetic;
    pub use crate::parsed::{
        ArenaGeneratedParse, CapturePolicy, ContentText, ElementMetadata, GeneratedInput,
        GeneratedParse as Parse, NodeElements, NodeViewFactory, Occurrence, Occurrences,
        ParseContext, ParsedNode, Repeated, RepeatedIter, ViewCursor, parse, parse_arena,
        parse_arena_spanned, parse_arena_without_spans, parse_flat_with,
        parse_seal::Sealed as ParseSealed, parse_spanned, parse_without_spans, pretty_view,
    };
    pub use crate::visitor::{VisitEventFamily, VisitEventFamilyMut, VisitNode};
    #[cfg(feature = "arbitrary")]
    pub use arbitrary;
    #[cfg(feature = "arbitrary")]
    pub use bumpalo;
    pub use logos;
    /// Perfect-hash runtime backing generated word-keyword classification.
    ///
    /// Re-exported so generated code never depends on `phf` directly: it stays an
    /// internal implementation detail of the facade/runtime pair.
    pub use phf;
    #[cfg(feature = "serde")]
    pub use serde;

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
        /// Prevents external construction of compatibility witnesses.
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
        33,
    > = FacadeCompatibility { _private: () };

    /// Builds a borrowed set constant emitted by matching Recursa codegen.
    pub const fn follow_set(
        kind_count: u32,
        kinds: &'static [u64],
        expected: &'static [ExpectedAtomId],
    ) -> FollowSet {
        FollowSet::from_static(kind_count, kinds, expected)
    }

    /// Returns the empty root FOLLOW used by a table-driven parser.
    ///
    /// Table-driven parsing never composes caller FOLLOW sets. Its only root
    /// use is the tail-equivalence check, where an empty set rejects every
    /// remaining token. A width-zero set has precisely that membership
    /// behavior for every grammar-local kind, so table-only generations need
    /// not emit a grammar-sized empty bitset merely to implement `Parse`.
    #[inline]
    pub fn empty_follow() -> &'static FollowSet {
        static EMPTY: FollowSet = FollowSet::from_static(0, &[], &[]);
        &EMPTY
    }

    /// Expands a compact generated bitset into a promoted static expectation array.
    pub const fn expected_atoms<const N: usize>(words: &[u64]) -> [ExpectedAtomId; N] {
        let mut output = [ExpectedAtomId(0); N];
        let mut output_index = 0;
        let mut word_index = 0;
        while word_index < words.len() {
            let mut bits = words[word_index];
            while bits != 0 {
                assert!(output_index < N);
                let bit = bits.trailing_zeros() as usize;
                output[output_index] = ExpectedAtomId((word_index * 64 + bit) as u32);
                output_index += 1;
                bits &= bits - 1;
            }
            word_index += 1;
        }
        assert!(output_index == N);
        output
    }

    /// Rebuilds the shared-pool handle of one Node an opaque traversal reference names.
    ///
    /// Raw handle construction is private to this crate, because a forged handle would break
    /// invariant I2 of `docs/flat-ast-design.md`. This function is the one door through it, and
    /// generated traversal code is the only caller that needs it. A Node the grammar root cannot
    /// name has no `NodeRef` variant, so an [`OpaqueRef`] carries the raw handle value instead of
    /// a typed handle. The children function beside that Node's declaration receives the raw
    /// value and must rebuild the handle to read the shared pool.
    ///
    /// The door cannot reach a typed pool: a pooled declaration is always publicly reachable, so
    /// a Node the root cannot name is always in the shared pool.
    pub const fn opaque_handle<T>(raw: ::core::num::NonZeroU32) -> Handle<T> {
        Handle::from_raw(raw)
    }

    /// Returns the source retained by a grammar-local generated lexing result.
    pub fn lex_source<'input>(lexed: &LexResult<'input>) -> &'input str {
        lexed.source()
    }

    /// Returns payload-free records for a generated typed token view.
    pub fn lex_records<'lexed>(lexed: &'lexed LexResult<'_>) -> &'lexed [TokenRecord] {
        lexed.records()
    }

    /// Returns classified trivia records when the grammar opted into retention.
    pub fn lex_trivia<'lexed>(lexed: &'lexed LexResult<'_>) -> Option<&'lexed [TriviaRecord]> {
        lexed.trivia()
    }

    /// Transfers raw lexical records into generated grammar finalization.
    pub fn lex_into_parts(lexed: LexResult<'_>) -> (Vec<TokenRecord>, Option<Vec<TriviaRecord>>) {
        lexed.into_parts()
    }

    /// Returns immutable source gaps when the grammar opted into classified trivia.
    pub fn lex_source_gaps<'lexed>(lexed: &'lexed LexResult<'_>) -> Option<SourceGaps<'lexed>> {
        lexed.source_gaps()
    }
    pub use crate::{
        __recursa_visit_children as visit_children, __recursa_visit_node as visit_node,
        __recursa_visit_node_mut as visit_node_mut,
    };
}

#[cfg(test)]
mod compatibility_tests {
    #[test]
    fn compact_expected_atoms_expand_in_sorted_identity_order() {
        assert_eq!(
            super::__private::expected_atoms::<3>(&[0b1001, 0b10]),
            [
                super::__private::ExpectedAtomId(0),
                super::__private::ExpectedAtomId(3),
                super::__private::ExpectedAtomId(65),
            ]
        );
    }

    #[test]
    fn core_exposes_the_exact_release_and_private_protocol_capability() {
        let _: super::__private::FacadeCompatibility<
            { super::__private::release_stamp("0.1.0") },
            33,
        > = super::__private::FACADE_COMPATIBILITY;
    }
}
