//! Assembly and validation of one complete logical generated output.
//!
//! Capability renderers feed this boundary; it validates every logical Rust file and
//! returns one complete payload without performing filesystem publication.

use super::*;

/// One complete logical output ready for filesystem publication.
///
/// The root and Node files have already parsed as Rust, anchors align with Node order, and the
/// selector names only this generation identity. This value contains no partially rendered state.
pub(super) struct RenderedGeneration {
    /// Validated structural root source shared by every Node loader.
    pub(super) root: String,
    /// Validated Node filenames and sources in parse-plan order.
    pub(super) nodes: Vec<(String, String)>,
    /// Loader anchors positionally aligned with [`Self::nodes`].
    pub(super) node_anchors: Vec<String>,
    /// Validated loader source that selects this Generation identity.
    pub(super) selector: String,
}

/// Validates and renders every logical file without performing filesystem publication.
pub(super) fn render(plan: &EmissionPlan) -> Result<RenderedGeneration, String> {
    plan.validate()?;
    let root = render_root(plan).to_string();
    let mut instrumentation = RenderInstrumentation::default();
    let mut nodes = Vec::new();
    for (index, node) in plan.nodes.iter().enumerate() {
        let name = format!("node-{index}.rs");
        let rendered = render_node(plan, node, index, &mut instrumentation).to_string();
        syn::parse_file(&rendered)
            .map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
        nodes.push((name, rendered));
    }
    if instrumentation.frozen_dispatch_nodes_rendered
        != plan.instrumentation.frozen_dispatch_nodes_consumed
    {
        return Err("the renderer did not consume exactly the frozen dispatch trie".to_owned());
    }
    let selected_root = format!("/recursa-generations/{}/root.rs", plan.identity);
    let selector_body = quote! {
        include!(concat!(env!("OUT_DIR"), #selected_root));
    }
    .to_string();
    let selector = format!("// recursa-generation:{}\n{selector_body}", plan.identity);
    syn::parse_file(&root).map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    syn::parse_file(&selector)
        .map_err(|error| format!("Generation selector rendering is invalid Rust: {error}"))?;
    Ok(RenderedGeneration {
        root,
        nodes,
        node_anchors: plan
            .nodes
            .iter()
            .map(|node| node.loader_anchor.clone())
            .collect(),
        selector,
    })
}
/// Renders the structural root, preserving protocol sections and exports in their planned order.
///
/// # Long-function contract
///
/// Rationale: assembles the structural root as one ordered protocol unit.
/// Invariants: loader checks precede exports and logical file order does not affect generation identity.
/// Phases: renders compatibility checks, grammar services, exports, diagrams, and extension loaders.
pub(super) fn render_root(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let release_text = &plan.private_abi.release;
    let release = format!("release_{release_text}").replace(['.', '-'], "_");
    let release = format_ident!("{release}");
    let private_protocol = plan.private_abi.protocol;
    let protocol = format_ident!("protocol_{private_protocol}");
    let grammar_name = &plan.grammar_name;
    #[cfg(feature = "diagrams")]
    let aggregate_diagram = plan.diagrams.as_ref().map(|diagrams| {
        let documentation = syn::LitStr::new(
            &render_aggregate_diagram_document(diagrams),
            proc_macro2::Span::call_site(),
        );
        quote! {
            #[doc = #documentation]
            pub mod grammar_docs {}
        }
    });
    #[cfg(not(feature = "diagrams"))]
    let aggregate_diagram = proc_macro2::TokenStream::new();
    let kind_count = plan.kind_count;
    let lexical_error_kind = plan.lexical_error_kind;
    let kind_names = &plan.kind_names;
    let atom_names = &plan.atom_names;
    let kind_variants = &plan.kind_variants;
    let kind_match_arms = kind_variants.iter().enumerate().map(|(index, variant)| {
        let index = index as u16;
        quote!(#index => TokenKind::#variant,)
    });
    let raw_kind_arms = kind_variants.iter().enumerate().map(|(index, variant)| {
        let index = index as u16;
        quote!(TokenKind::#variant => #index,)
    });
    let kind_static_arms = kind_variants
        .iter()
        .zip(&plan.kind_static_texts)
        .filter_map(|(variant, text)| {
            let text = text.as_ref()?;
            Some(quote!(TokenKind::#variant => Some(#text),))
        });
    let kind_display_arms =
        kind_variants
            .iter()
            .zip(&plan.kind_static_texts)
            .map(|(variant, text)| {
                if let Some(text) = text {
                    quote!(TokenKind::#variant => formatter.write_str(#text),)
                } else {
                    quote!(TokenKind::#variant => formatter.write_str(stringify!(#variant)),)
                }
            });
    let word_candidate_kinds = &plan.word_candidate_kinds;
    let word_classifiers = plan.word_classifiers.iter().map(|classifier| {
        let canonical = &classifier.canonical_spelling;
        let kind = classifier.kind;
        let matches = if classifier.ascii_insensitive {
            quote!(text.eq_ignore_ascii_case(#canonical))
        } else {
            quote!(text == #canonical)
        };
        quote! {
            if source
                .get(record.span.range())
                .is_some_and(|text| #matches)
            {
                record.kind = #kind;
                return;
            }
        }
    });
    let post_lex = plan.post_lex.as_ref().map(|hook| {
        quote! {
            let mut editor = LexEditor { source, records };
            #hook(&mut editor);
            records = editor.records;
        }
    });
    let matchers = plan
        .lexical_rules
        .iter()
        .map(|rule| match rule.closed_matcher.as_ref() {
            None => {
                let pattern = &rule.pattern;
                quote!(#facade::__private::LexMatcher::Regex(#pattern))
            }
            Some(ClosedMatcher::SameDelimiter { opener }) => {
                quote!(#facade::__private::LexMatcher::SameDelimiter { opener: #opener })
            }
            Some(ClosedMatcher::Nested { opener, closer }) => {
                quote!(#facade::__private::LexMatcher::Nested {
                    opener: #opener,
                    closer: #closer,
                })
            }
            Some(ClosedMatcher::NextExclusion { pattern, excluded }) => {
                quote!(#facade::__private::LexMatcher::NextExclusion {
                    pattern: #pattern,
                    excluded: #excluded,
                })
            }
            Some(ClosedMatcher::OperatorRun {
                characters,
                fences,
                trailing,
                qualifying,
            }) => {
                quote!(#facade::__private::LexMatcher::OperatorRun {
                    characters: #characters,
                    fences: &[#(#fences),*],
                    trailing: #trailing,
                    qualifying: #qualifying,
                })
            }
            Some(ClosedMatcher::PhysicalLine { prefix }) => {
                quote!(#facade::__private::LexMatcher::PhysicalLine { prefix: #prefix })
            }
        });
    let kinds = plan.lexical_rules.iter().map(|rule| rule.kind);
    let skips = plan.lexical_rules.iter().map(|rule| rule.skip);
    let fixed = plan.lexical_rules.iter().map(|rule| rule.fixed);
    let priorities = plan.lexical_rules.iter().map(|rule| rule.priority);
    let callbacks = plan.lexical_rules.iter().enumerate().map(|(index, rule)| {
        if rule.callback.is_some() {
            let shim = format_ident!("__recursa_lex_callback_{index}");
            quote!(Some(#shim))
        } else {
            quote!(None)
        }
    });
    let callback_shims = plan
        .lexical_rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| {
            let callback = rule.callback.as_ref()?;
            let shim = format_ident!("__recursa_lex_callback_{index}");
            Some(quote! {
                fn #shim(
                    context: &mut #facade::__private::LexContext<'_>,
                ) -> #facade::__private::LexAction {
                    let initial_end = context.end();
                    let mut typed = LexContext {
                        source: context.source(),
                        start: context.start(),
                        end: initial_end,
                    };
                    let action = #callback(&mut typed);
                    context
                        .bump(typed.end - initial_end)
                        .expect("generated typed callback context validated its bump");
                    match action {
                        super::LexAction::Emit => #facade::__private::LexAction::Emit,
                        super::LexAction::EmitAs(kind) => {
                            #facade::__private::LexAction::EmitAs(__recursa_raw_kind(kind))
                        }
                        super::LexAction::Skip => #facade::__private::LexAction::Skip,
                        super::LexAction::Error => #facade::__private::LexAction::Error,
                    }
                }
            })
        });
    let empty_words = vec![0_u64; kind_count.div_ceil(64)];
    let token_starts = plan.tokens.iter().enumerate().map(|(index, token)| {
        let name = format_ident!("__RECURSA_TOKEN_START_{index}");
        render_expected_static(&name, &token.expected, plan, facade)
    });
    let token_items = plan
        .tokens
        .iter()
        .enumerate()
        .map(|(index, token)| render_token(plan, token, index));
    let visit_events = render_visit_events(plan);
    let explorer_schema = plan
        .explorer
        .as_ref()
        .map(|explorer| render_explorer_static_schema(plan, explorer));
    let explorer_lex = plan.explorer.as_ref().map(|_| render_explorer_lex(plan));
    let loader_arms = plan.nodes.iter().enumerate().map(|(index, node)| {
        let ident = &node.ident;
        let anchor = syn::LitStr::new(&node.loader_anchor, proc_macro2::Span::call_site());
        let path = format!("/recursa-generations/{}/node-{index}.rs", plan.identity);
        quote! {
            (#ident, #anchor) => {
                include!(concat!(env!("OUT_DIR"), #path));
            };
        }
    });
    let capability_arms = plan.capabilities.iter().map(|capability| {
        let diagrams = u8::from(capability.diagrams);
        let explorer = u8::from(capability.explorer);
        let arbitrary = u8::from(capability.arbitrary);
        let serde = u8::from(capability.serde);
        let capability = format_ident!("cap_core_d{diagrams}_e{explorer}_a{arbitrary}_s{serde}");
        quote! {
            (#release, #protocol, #capability) => {
                const _: #facade::__private::FacadeCompatibility<
                    { #facade::__private::release_stamp(#release_text) },
                    #private_protocol,
                > = #facade::__private::FACADE_COMPATIBILITY;
            };
        }
    });

    quote! {
        #[doc(hidden)]
        pub(crate) struct __RecursaGenerationAnchor;
        const _: fn() = || {
            let _: #module_anchor::__RecursaGenerationAnchor = __RecursaGenerationAnchor;
        };

        #aggregate_diagram

        static __RECURSA_GRAMMAR: #facade::__private::GrammarInfo =
            #facade::__private::grammar_info(
                #grammar_name,
                &[#(#kind_names),*],
                &[#(#atom_names),*],
                #lexical_error_kind,
            );
        static __RECURSA_EMPTY_FOLLOW: #facade::__private::FollowSet =
            #facade::__private::follow_set(
                #kind_count as u32,
                &[#(#empty_words),*],
                &[],
            );
        #(#token_starts)*

        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[allow(non_camel_case_types)]
        pub enum TokenKind {
            #(#kind_variants,)*
            LexicalError,
        }

        impl TokenKind {
            /// Static spelling for a data-free fixed token kind.
            pub const fn static_text(self) -> Option<&'static str> {
                match self {
                    #(#kind_static_arms)*
                    _ => None,
                }
            }
        }

        impl ::core::fmt::Display for TokenKind {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    #(#kind_display_arms)*
                    TokenKind::LexicalError => formatter.write_str("LexicalError"),
                }
            }
        }

        /// The complete set of decisions a lexer callback can make.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum LexAction {
            /// Emit the rule's declared token kind.
            Emit,
            /// Emit a different named kind from this grammar.
            EmitAs(TokenKind),
            /// Consume the matched source without emitting a record.
            Skip,
            /// Emit this grammar's lexical-error record.
            Error,
        }

        /// A cursor bound to this generated grammar's token identities.
        pub struct Input<'input, A = ()> {
            pub(crate) inner: #facade::__private::Input<'input, A>,
        }

        impl<'input, A> Input<'input, A> {
            pub fn source(&self) -> &'input str {
                self.inner.source()
            }

            pub fn builder(&mut self) -> &mut A {
                self.inner.builder()
            }

            pub fn cursor(&self) -> usize {
                self.inner.cursor()
            }

            pub fn is_eof(&self) -> bool {
                self.inner.is_eof()
            }
        }

        mod __recursa_lexical {
            use super::{Input, TokenKind};

            static __RECURSA_LEX_RULES: &[#facade::__private::LexRule] = &[
                #(
                    #facade::__private::LexRule {
                        matcher: #matchers,
                        kind: #kinds,
                        skip: #skips,
                        callback: #callbacks,
                        fixed: #fixed,
                        priority: #priorities,
                    }
                ),*
            ];

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct LexBumpError {
            _private: (),
        }

        impl ::core::fmt::Display for LexBumpError {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(
                    "lexer callback bump is outside the remaining UTF-8 source",
                )
            }
        }

        impl ::std::error::Error for LexBumpError {}

        /// Backend-independent access to one callback's current match.
        pub struct LexContext<'source> {
            source: &'source str,
            start: usize,
            end: usize,
        }

        impl<'source> LexContext<'source> {
            /// The complete source slice currently claimed by this callback.
            pub fn slice(&self) -> &str {
                &self.source[self.start..self.end]
            }

            /// Source following the callback's current slice.
            pub fn remainder(&self) -> &str {
                &self.source[self.end..]
            }

            /// Extends the current slice by a validated byte count.
            ///
            /// Failure leaves the slice unchanged. Bumps may not exceed the
            /// source or end between UTF-8 code units.
            pub fn bump(&mut self, bytes: usize) -> Result<(), LexBumpError> {
                let Some(end) = self.end.checked_add(bytes) else {
                    return Err(LexBumpError { _private: () });
                };
                if end > self.source.len() || !self.source.is_char_boundary(end) {
                    return Err(LexBumpError { _private: () });
                }
                self.end = end;
                Ok(())
            }
        }

        #(#callback_shims)*

        fn __recursa_raw_kind(kind: TokenKind) -> u16 {
            match kind {
                #(#raw_kind_arms)*
                TokenKind::LexicalError => #lexical_error_kind,
            }
        }

        fn __recursa_typed_kind(kind: u16) -> TokenKind {
            match kind {
                #(#kind_match_arms)*
                _ => TokenKind::LexicalError,
            }
        }

        fn __recursa_classify(
            source: &str,
            record: &mut #facade::__private::TokenRecord,
        ) {
            if ![#(#word_candidate_kinds),*].contains(&record.kind) {
                return;
            }
            #(#word_classifiers)*
        }

        fn __recursa_finalize<'input>(
            source: &'input str,
            mut records: Vec<#facade::__private::TokenRecord>,
        ) -> #facade::__private::LexResult<'input> {
            for record in &mut records {
                __recursa_classify(source, record);
            }
            #post_lex
            for record in &mut records {
                if record.kind == #lexical_error_kind {
                    if record.diagnostic.is_none() {
                        record.diagnostic = Some(#facade::__private::LexDiagnostic {
                            code: #facade::__private::LexDiagnosticCode::Unrecognized,
                            anchor: record.span,
                        });
                    }
                } else {
                    record.diagnostic = None;
                }
            }
            #facade::__private::LexResult::new(source, records)
        }

        /// Read-only metadata for this generated grammar.
        pub struct GrammarInfo {
            _private: (),
        }

        impl GrammarInfo {
            pub fn name(&self) -> &'static str {
                super::__RECURSA_GRAMMAR.name()
            }

        }

        static __RECURSA_PUBLIC_GRAMMAR_INFO: GrammarInfo = GrammarInfo { _private: () };

        pub fn grammar_info() -> &'static GrammarInfo {
            &__RECURSA_PUBLIC_GRAMMAR_INFO
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct Token<'input> {
            source: &'input str,
            record: #facade::__private::TokenRecord,
        }

        impl<'input> Token<'input> {
            pub fn kind(self) -> TokenKind {
                __recursa_typed_kind(self.record.kind)
            }

            pub fn text(self) -> &'input str {
                self.source
                    .get(self.record.span.range())
                    .expect("generated lexer produced a valid UTF-8 span")
            }

            pub fn span(self) -> #facade::Span {
                self.record.span
            }

        }

        impl ::core::fmt::Display for Token<'_> {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(self.source.get(self.record.span.range()).ok_or(::core::fmt::Error)?)
            }
        }

        pub struct Tokens<'lexed, 'input> {
            source: &'input str,
            records: ::core::slice::Iter<'lexed, #facade::__private::TokenRecord>,
            lexical_error_kind: u16,
        }

        impl<'input> Iterator for Tokens<'_, 'input> {
            type Item = Token<'input>;

            fn next(&mut self) -> Option<Self::Item> {
                self.records
                    .find(|record| record.kind != self.lexical_error_kind)
                    .copied()
                    .map(|record| Token {
                        source: self.source,
                        record,
                    })
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = self
                    .records
                    .as_slice()
                    .iter()
                    .filter(|record| record.kind != self.lexical_error_kind)
                    .count();
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for Tokens<'_, '_> {}

        /// Stable mechanism-specific classification of a lexical failure.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum LexErrorCode {
            /// No lexical rule accepted the current source scalar.
            Unrecognized,
            /// A same-delimiter region reached EOF without its matching closer.
            UnterminatedSameDelimiter,
            /// A nested region reached EOF with nonzero nesting depth.
            UnterminatedNested,
            /// Nested depth exceeded its checked internal representation.
            NestedDepthOverflow,
            /// A token candidate was immediately followed by an excluded scalar.
            ExcludedFollowingCharacter,
        }

        impl LexErrorCode {
            /// Returns the append-only Recursa diagnostic-code spelling.
            pub const fn as_str(self) -> &'static str {
                match self {
                    Self::Unrecognized => "RCA4000",
                    Self::UnterminatedSameDelimiter => "RCA4001",
                    Self::UnterminatedNested => "RCA4002",
                    Self::NestedDepthOverflow => "RCA4003",
                    Self::ExcludedFollowingCharacter => "RCA4004",
                }
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct LexError<'input> {
            source: &'input str,
            record: #facade::__private::TokenRecord,
        }

        impl<'input> LexError<'input> {
            pub const fn kind(self) -> TokenKind {
                TokenKind::LexicalError
            }

            pub fn text(self) -> &'input str {
                self.source
                    .get(self.record.span.range())
                    .expect("generated lexer produced a valid UTF-8 error span")
            }

            pub fn span(self) -> #facade::Span {
                self.record.span
            }

            /// Stable diagnostic code for this lexical failure.
            pub fn code(self) -> &'static str {
                self.diagnostic_code().as_str()
            }

            /// Typed mechanism-specific classification for this lexical failure.
            pub fn diagnostic_code(self) -> LexErrorCode {
                match self.record.diagnostic.map(|diagnostic| diagnostic.code) {
                    Some(#facade::__private::LexDiagnosticCode::UnterminatedSameDelimiter) => {
                        LexErrorCode::UnterminatedSameDelimiter
                    }
                    Some(#facade::__private::LexDiagnosticCode::UnterminatedNested) => {
                        LexErrorCode::UnterminatedNested
                    }
                    Some(#facade::__private::LexDiagnosticCode::NestedDepthOverflow) => {
                        LexErrorCode::NestedDepthOverflow
                    }
                    Some(#facade::__private::LexDiagnosticCode::ExcludedFollowingCharacter) => {
                        LexErrorCode::ExcludedFollowingCharacter
                    }
                    Some(#facade::__private::LexDiagnosticCode::Unrecognized) | None => {
                        LexErrorCode::Unrecognized
                    }
                }
            }

            /// Primary source range identifying the construct that began the failure.
            pub fn anchor(self) -> #facade::Span {
                self.record
                    .diagnostic
                    .map_or(self.record.span, |diagnostic| diagnostic.anchor)
            }
        }

        impl ::core::fmt::Display for LexError<'_> {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                formatter.write_str(self.source.get(self.record.span.range()).ok_or(::core::fmt::Error)?)
            }
        }

        pub struct LexErrors<'lexed, 'input> {
            source: &'input str,
            records: ::core::slice::Iter<'lexed, #facade::__private::TokenRecord>,
            lexical_error_kind: u16,
        }

        impl<'input> Iterator for LexErrors<'_, 'input> {
            type Item = LexError<'input>;

            fn next(&mut self) -> Option<Self::Item> {
                self.records
                    .find(|record| record.kind == self.lexical_error_kind)
                    .copied()
                    .map(|record| LexError {
                        source: self.source,
                        record,
                    })
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = self
                    .records
                    .as_slice()
                    .iter()
                    .filter(|record| record.kind == self.lexical_error_kind)
                    .count();
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for LexErrors<'_, '_> {}

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum LexBuildError {
            InvalidSpan { span: #facade::Span },
            OutOfOrder {
                previous: #facade::Span,
                span: #facade::Span,
            },
        }

        impl ::core::fmt::Display for LexBuildError {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::InvalidSpan { span } => write!(
                        formatter,
                        "token span {}..{} is not a UTF-8 range within the lexer source",
                        span.start(),
                        span.end(),
                    ),
                    Self::OutOfOrder { previous, span } => write!(
                        formatter,
                        "token span {}..{} overlaps or precedes {}..{}",
                        span.start(),
                        span.end(),
                        previous.start(),
                        previous.end(),
                    ),
                }
            }
        }

        impl ::std::error::Error for LexBuildError {}

        /// Validated input for an alternative lexer.
        pub struct LexBuilder<'input> {
            source: &'input str,
            records: Vec<#facade::__private::TokenRecord>,
        }

        impl<'input> LexBuilder<'input> {
            pub fn new(source: &'input str) -> Self {
                Self {
                    source,
                    records: Vec::new(),
                }
            }

            pub fn append(
                &mut self,
                kind: TokenKind,
                span: #facade::Span,
            ) -> Result<&mut Self, LexBuildError> {
                if self.source.get(span.range()).is_none() {
                    return Err(LexBuildError::InvalidSpan { span });
                }
                if let Some(previous) = self.records.last().map(|record| record.span) {
                    if span.start() < previous.end() {
                        return Err(LexBuildError::OutOfOrder { previous, span });
                    }
                }
                self.records.push(#facade::__private::TokenRecord {
                    kind: __recursa_raw_kind(kind),
                    span,
                    diagnostic: None,
                });
                Ok(self)
            }

            pub fn finish(self) -> LexResult<'input> {
                LexResult {
                    inner: __recursa_finalize(self.source, self.records),
                }
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct LexRecord<'input> {
            source: &'input str,
            record: #facade::__private::TokenRecord,
        }

        impl<'input> LexRecord<'input> {
            pub fn kind(self) -> TokenKind {
                __recursa_typed_kind(self.record.kind)
            }

            pub fn text(self) -> &'input str {
                self.source
                    .get(self.record.span.range())
                    .expect("the lexical editor preserves valid UTF-8 spans")
            }

            pub fn span(self) -> #facade::Span {
                self.record.span
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum LexEditError {
            MissingRecord { index: usize },
            EmptyReplacement,
            InvalidSpan { span: #facade::Span },
            OutOfOrder {
                previous: #facade::Span,
                span: #facade::Span,
            },
            InvalidRange { start: usize, end: usize },
        }

        impl ::core::fmt::Display for LexEditError {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self {
                    Self::MissingRecord { index } => {
                        write!(formatter, "lexical record index {index} does not exist")
                    }
                    Self::EmptyReplacement => {
                        formatter.write_str("lexical replacement must contain a record")
                    }
                    Self::InvalidSpan { span } => write!(
                        formatter,
                        "replacement span {}..{} is outside its source record",
                        span.start(),
                        span.end(),
                    ),
                    Self::OutOfOrder { previous, span } => write!(
                        formatter,
                        "replacement span {}..{} overlaps or precedes {}..{}",
                        span.start(),
                        span.end(),
                        previous.start(),
                        previous.end(),
                    ),
                    Self::InvalidRange { start, end } => {
                        write!(formatter, "lexical record range {start}..{end} is invalid")
                    }
                }
            }
        }

        impl ::std::error::Error for LexEditError {}

        /// Validated, grammar-typed mutation available only during post-lex.
        pub struct LexEditor<'input> {
            source: &'input str,
            records: Vec<#facade::__private::TokenRecord>,
        }

        impl<'input> LexEditor<'input> {
            pub fn source(&self) -> &'input str {
                self.source
            }

            pub fn len(&self) -> usize {
                self.records.len()
            }

            pub fn is_empty(&self) -> bool {
                self.records.is_empty()
            }

            pub fn record(&self, index: usize) -> Option<LexRecord<'input>> {
                self.records.get(index).copied().map(|record| LexRecord {
                    source: self.source,
                    record,
                })
            }

            pub fn reclassify(
                &mut self,
                index: usize,
                kind: TokenKind,
            ) -> Result<(), LexEditError> {
                let Some(record) = self.records.get_mut(index) else {
                    return Err(LexEditError::MissingRecord { index });
                };
                record.kind = __recursa_raw_kind(kind);
                __recursa_classify(self.source, record);
                Ok(())
            }

            pub fn replace<I>(
                &mut self,
                index: usize,
                replacement: I,
            ) -> Result<(), LexEditError>
            where
                I: IntoIterator<Item = (TokenKind, #facade::Span)>,
            {
                let Some(original) = self.records.get(index).copied() else {
                    return Err(LexEditError::MissingRecord { index });
                };
                let mut replacement = replacement
                    .into_iter()
                    .map(|(kind, span)| #facade::__private::TokenRecord {
                        kind: __recursa_raw_kind(kind),
                        span,
                        diagnostic: None,
                    })
                    .collect::<Vec<_>>();
                if replacement.is_empty() {
                    return Err(LexEditError::EmptyReplacement);
                }
                let mut previous: Option<#facade::Span> = None;
                for record in &replacement {
                    if record.span.start() < original.span.start()
                        || record.span.end() > original.span.end()
                        || self.source.get(record.span.range()).is_none()
                    {
                        return Err(LexEditError::InvalidSpan { span: record.span });
                    }
                    if let Some(previous_span) = previous {
                        if record.span.start() < previous_span.end() {
                            return Err(LexEditError::OutOfOrder {
                                previous: previous_span,
                                span: record.span,
                            });
                        }
                    }
                    previous = Some(record.span);
                }
                for record in &mut replacement {
                    __recursa_classify(self.source, record);
                }
                self.records.splice(index..index + 1, replacement);
                Ok(())
            }

            pub fn remove(
                &mut self,
                range: ::core::ops::Range<usize>,
            ) -> Result<(), LexEditError> {
                if range.start > range.end || range.end > self.records.len() {
                    return Err(LexEditError::InvalidRange {
                        start: range.start,
                        end: range.end,
                    });
                }
                self.records.drain(range);
                Ok(())
            }

            pub fn classify_word(&mut self, index: usize) -> Result<(), LexEditError> {
                let Some(record) = self.records.get_mut(index) else {
                    return Err(LexEditError::MissingRecord { index });
                };
                __recursa_classify(self.source, record);
                Ok(())
            }
        }

        pub struct LexResult<'input> {
            inner: #facade::__private::LexResult<'input>,
        }

        impl<'input> LexResult<'input> {
            pub fn tokens(&self) -> Tokens<'_, 'input> {
                Tokens {
                    source: #facade::__private::lex_source(&self.inner),
                    records: #facade::__private::lex_records(&self.inner).iter(),
                    lexical_error_kind: #lexical_error_kind,
                }
            }

            pub fn errors(&self) -> LexErrors<'_, 'input> {
                LexErrors {
                    source: #facade::__private::lex_source(&self.inner),
                    records: #facade::__private::lex_records(&self.inner).iter(),
                    lexical_error_kind: #lexical_error_kind,
                }
            }

            pub fn source(&self) -> &'input str {
                #facade::__private::lex_source(&self.inner)
            }

            pub fn input(&self) -> Input<'input> {
                Input {
                    inner: self.inner.input(&super::__RECURSA_GRAMMAR),
                }
            }

            pub fn input_with_builder<A>(&self, builder: A) -> Input<'input, A> {
                Input {
                    inner: self.inner.input_with_builder(&super::__RECURSA_GRAMMAR, builder),
                }
            }
        }

        pub fn lex(source: &str) -> LexResult<'_> {
            let scanned = #facade::__private::lex(
                source,
                __RECURSA_LEX_RULES,
                #lexical_error_kind,
            );
            let records = #facade::__private::lex_records(&scanned).to_vec();
            LexResult { inner: __recursa_finalize(source, records) }
        }

        }

        pub use __recursa_lexical::{
            GrammarInfo, LexBuildError, LexBuilder, LexBumpError, LexContext, LexEditError,
            LexEditor, LexError, LexErrorCode, LexErrors, LexRecord, LexResult, Token, Tokens,
            grammar_info, lex,
        };

        #[doc(hidden)]
        pub(crate) mod __recursa_content_tokens {
            #(#token_items)*
        }

        #visit_events
        #explorer_schema
        #explorer_lex

        macro_rules! __recursa_root_handshake {
            #(#capability_arms)*
            (#release, #protocol, $missing_capability:ident) => {
                compile_error!("error[RCA9005]: generated grammar requires a missing Recursa facade capability");
            };
            ($other_release:ident, $other_protocol:ident, $other_capability:ident) => {
                compile_error!("error[RCA9003]: Recursa release/private-protocol mismatch");
            };
        }

        macro_rules! __recursa_generation_handshake {
            (#generation) => {};
            ($other:ident) => {
                compile_error!("error[RCA9004]: root and Node selected different Generations");
            };
        }

        macro_rules! __recursa_load_node {
            #(#loader_arms)*
            ($other:ident, $other_anchor:literal) => {
                compile_error!("error[RCA9002]: selected Generation has no body for this Node");
            };
        }
    }
}
/// Renders one Node file from its frozen parse, capability, and loader plans.
///
/// # Long-function contract
///
/// Rationale: assembles every capability for one source Node beside its structural loader.
/// Invariants: source visibility, generic applications, and configured recipes remain mutually consistent.
/// Phases: renders loader and views, parse applications, traits, diagrams, and Explorer adapters.
pub(super) fn render_node(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
    instrumentation: &mut RenderInstrumentation,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let node_ident = &node.ident;
    let visibility = &node.visibility;
    let view_ident = sibling_ident(node_ident, "View");
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let generic_arguments = generic_arguments(generics);
    let mut view_generics = generics.clone();
    view_generics.params.insert(0, syn::parse_quote!('view));
    let (view_impl_generics, view_type_generics, view_where_clause) =
        view_generics.split_for_impl();
    let view_application = quote!(#view_ident<'view #(, #generic_arguments)*>);
    let input_lifetime = &node.input_lifetime;
    let inherent_method_generics = if generics
        .params
        .iter()
        .any(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
    {
        quote!(<A>)
    } else {
        quote!(<#input_lifetime, A>)
    };
    let metadata = render_metadata(plan, node, node_index);
    let accessors = render_accessors(plan, node);
    let span_elements = render_span_elements(node, node_index);
    let parse_implementations = node
        .parse_applications
        .iter()
        .map(|application| render_node_parse_application(plan, node, application, instrumentation))
        .collect::<Vec<_>>();
    let token_bindings = if plan.nodes[..node_index]
        .iter()
        .any(|candidate| candidate.module == node.module)
    {
        Vec::new()
    } else {
        plan.token_bindings
            .iter()
            .filter(|binding| binding.module == node.module)
            .map(|binding| render_token_binding(plan, binding))
            .collect::<Vec<_>>()
    };
    let configured_traits = render_node_configured_traits(plan, node, node_index);
    let explorer = render_node_explorer(plan, node, node_index);
    #[cfg(feature = "diagrams")]
    let diagram_documentation = plan.diagrams.as_ref().map(|diagrams| {
        let diagram = &diagrams.nodes[node_index];
        syn::LitStr::new(
            &render_node_diagram_document(diagram),
            proc_macro2::Span::call_site(),
        )
    });
    #[cfg(feature = "diagrams")]
    let diagram_attribute = diagram_documentation
        .as_ref()
        .map(|documentation| quote!(#[doc = #documentation]));
    #[cfg(not(feature = "diagrams"))]
    let diagram_attribute = proc_macro2::TokenStream::new();
    let view_pretty = plan.pretty.nodes[node_index].is_some().then(|| {
        let mut pretty_generics = view_generics.clone();
        pretty_generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#node_ident #type_generics: #facade::Pretty));
        let (pretty_impl_generics, pretty_type_generics, pretty_where_clause) =
            pretty_generics.split_for_impl();
        quote! {
            impl #pretty_impl_generics #facade::Pretty
                for #view_ident #pretty_type_generics #pretty_where_clause
            {
                fn pretty(
                    &self,
                    context: &mut #facade::PrettyContext,
                ) -> ::core::fmt::Result {
                    context.with_cursor(self.cursor, |context| {
                        #facade::Pretty::pretty(self.value, context)
                    })
                }
            }
        }
    });

    quote! {
        __recursa_generation_handshake!(#generation);

        #(#token_bindings)*

        #metadata

        #[derive(Clone, Copy)]
        #visibility struct #view_ident #view_generics {
            value: &'view #node_ident #type_generics,
            cursor: #facade::__private::ViewCursor<'view>,
        }

        impl #view_impl_generics #view_ident #view_type_generics #view_where_clause {
            pub fn span_elements(self) -> #facade::Elements<'view> {
                #span_elements
            }

            #accessors
        }

        impl #view_impl_generics #facade::__private::HasSpanSealed
            for #view_ident #view_type_generics #view_where_clause
        {}

        impl #view_impl_generics #facade::HasSpan
            for #view_ident #view_type_generics #view_where_clause
        {
            fn span(&self) -> Option<#facade::Span> { self.cursor.span() }
        }

        impl #view_impl_generics #facade::NodeView
            for #view_ident #view_type_generics #view_where_clause
        {
            type Value = #node_ident #type_generics;
            fn value(&self) -> &Self::Value { self.value }
            fn span_elements(&self) -> #facade::Elements<'_> { #span_elements }
        }

        #view_pretty

        impl #impl_generics #facade::__private::NodeViewFactory for #node_ident #type_generics
            #where_clause
        {
            type View<'view> = #view_application where Self: 'view;
            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> Self::View<'view> { #view_ident { value: self, cursor } }
        }

        #(#parse_implementations)*

        #configured_traits
        #explorer

        #diagram_attribute
        #[allow(dead_code)]
        impl #impl_generics #node_ident #type_generics #where_clause {
            pub fn parse #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError>
            where
                Self: #facade::Parse<
                    #module_anchor::Input<#input_lifetime, A>,
                    Output = #facade::Parsed<#input_lifetime, Self>,
                >,
            {
                <Self as #facade::Parse<#module_anchor::Input<#input_lifetime, A>>>::parse(input)
            }
            pub fn parse_spanned #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError>
            where
                Self: #facade::__private::Parse<#input_lifetime, A>,
            {
                #facade::__private::parse_spanned(&mut input.inner)
            }
        }
    }
}
