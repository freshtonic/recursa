//! Assembly of one complete logical generated output from a validated emission plan.
//!
//! Capability renderers feed this boundary; production consumes the opaque plan validation proof,
//! while checked test plans retain the complete invariant and Rust-syntax failure contracts.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use quote::quote;

    use super::{
        super::test_support, WordClassifierPlan, apply_generated_lint_boundary,
        render_word_classifiers,
    };

    #[test]
    fn lint_policy_is_applied_once_per_generated_scope() {
        let generated = quote! {
            __recursa_generation_handshake!(g_1);
            pub struct NodeView<'view> { value: &'view Node }
            impl<'view> NodeView<'view> { fn view(self) {} }
            impl Parse for Node {}
            fn __recursa_parse_variant_0() {}
            static __RECURSA_ELEMENTS_0: [u8; 1] = [0];
            pub(crate) static __RECURSA_SCHEMA_0: u8 = 0;
            const CAPTURE_POLICY: u8 = 0;
            const _: fn() = || {};
            pub(super) fn keeps_its_level() {}
            pub type Alias = u8;
            __recursa_if_explorer! {
                fn __recursa_explorer_0() {}
                impl Explore for Node {}
            }
        };
        let bounded = apply_generated_lint_boundary(generated, "node_0")
            .expect("bounded output is valid Rust")
            .to_string();

        let policy_count = bounded.matches("# [allow (dead_code ,").count();
        assert_eq!(policy_count, 7, "output:\n{bounded}");
        assert!(bounded.contains("__recursa_generation_handshake ! (g_1) ;"));
        assert!(bounded.contains("mod __recursa_lint_scope_node_0_0 {"));
        assert!(bounded.contains("mod __recursa_lint_scope_node_0_1 {"));
        assert!(bounded.contains("pub (super) fn __recursa_parse_variant_0 ()"));
        assert!(bounded.contains("pub (super) static __RECURSA_ELEMENTS_0"));
        assert!(bounded.contains("pub (super) const CAPTURE_POLICY"));
        assert!(bounded.contains("pub (crate) static __RECURSA_SCHEMA_0"));
        // The gate body is bounded first, so its scope module takes index 0.
        assert!(bounded.contains("use __recursa_lint_scope_node_0_0 :: * ;"));
        assert!(!bounded.contains("pub (crate) use __recursa_lint_scope_node_0_0"));
        assert!(bounded.contains("pub (crate) use __recursa_lint_scope_node_0_1 :: * ;"));
        assert!(bounded.contains("pub (super) fn keeps_its_level ()"));
        assert!(!bounded.contains("pub (super) pub (super) fn"));
        assert!(bounded.contains("pub type Alias = u8 ;"));
        assert_eq!(bounded.matches("const _ : () = {").count(), 2);
        assert!(bounded.contains("impl < 'view > NodeView < 'view >"));
        assert!(bounded.contains("impl Parse for Node"));
        assert!(bounded.contains("impl Explore for Node"));
        assert!(bounded.contains("const _ : fn () = | | { } ;"));
        assert!(!bounded.contains(")] impl"), "impls carry no attribute of their own");
        assert!(!bounded.contains(")] const _ : fn"));
        assert!(!bounded.contains(")] fn "), "free functions carry no attribute of their own");
        assert!(!bounded.contains(")] static "));
        let allowed_prefix = bounded
            .split("pub struct NodeView")
            .next()
            .expect("struct prefix");
        assert!(allowed_prefix.trim_end().ends_with(")]"));
    }

    #[test]
    fn word_classification_reads_source_once_and_shards_large_vocabularies() {
        let classifiers = (0_u16..130)
            .map(|kind| WordClassifierPlan {
                canonical_spelling: format!("word_{kind}"),
                kind,
                ascii_insensitive: kind % 2 == 0,
            })
            .collect::<Vec<_>>();

        let facade: syn::Ident = syn::parse_quote!(recursa);
        let rendered = render_word_classifiers(&classifiers, &[7], &facade).to_string();

        assert_eq!(rendered.matches("source . get").count(), 1);
        assert_eq!(rendered.matches("is_some_and").count(), 0);
        assert_eq!(
            rendered.matches("fn __recursa_classify_words_0_").count(),
            3
        );
        assert_eq!(
            rendered.matches("fn __recursa_classify_words_1_").count(),
            1
        );
        assert!(rendered.contains("let Some (text) = source . get"));
        assert!(rendered.contains("record . kind = kind"));
        assert_eq!(rendered.matches("record . kind =").count(), 1);
        assert!(
            rendered
                .split("fn __recursa_classify_words_")
                .skip(1)
                .all(|body| body.matches("return Some").count() <= 64)
        );

        let classifiers = (0_u16..4_097)
            .map(|kind| WordClassifierPlan {
                canonical_spelling: format!("large_word_{kind}"),
                kind,
                ascii_insensitive: false,
            })
            .collect::<Vec<_>>();
        let rendered = render_word_classifiers(&classifiers, &[7], &facade).to_string();
        assert_eq!(
            rendered.matches("fn __recursa_classify_words_0_").count(),
            65
        );
        assert_eq!(
            rendered.matches("fn __recursa_classify_words_1_").count(),
            2
        );
        assert_eq!(
            rendered.matches("fn __recursa_classify_words_2_").count(),
            1
        );
        assert!(
            rendered
                .split("fn __recursa_classify_words_")
                .skip(1)
                .all(|body| body.matches("return Some").count() <= 64)
        );
    }

    #[test]
    fn validated_generation_is_not_revalidated_during_rendering() {
        test_support::validated_generation_is_not_revalidated_during_rendering();
    }

    #[test]
    fn checked_rendering_retains_invalid_rust_diagnostics() {
        test_support::checked_rendering_retains_invalid_rust_diagnostics();
    }

    #[test]
    fn generated_expectations_use_one_grammar_wide_pool() {
        test_support::generated_expectations_use_one_grammar_wide_pool();
    }
}

/// Renders bounded private keyword classifiers and their single record-update entry point.
fn render_word_classifiers(
    classifiers: &[WordClassifierPlan],
    candidate_kinds: &[u16],
    facade: &syn::Ident,
) -> proc_macro2::TokenStream {
    const CLASSIFIERS_PER_HELPER: usize = 64;

    let mut helper_items = Vec::new();
    let mut level = classifiers
        .chunks(CLASSIFIERS_PER_HELPER)
        .enumerate()
        .map(|(index, chunk)| {
            let helper = format_ident!("__recursa_classify_words_0_{index}");
            let comparisons = chunk.iter().map(|classifier| {
                let canonical = &classifier.canonical_spelling;
                let kind = classifier.kind;
                let matches = if classifier.ascii_insensitive {
                    quote!(text.eq_ignore_ascii_case(#canonical))
                } else {
                    quote!(text == #canonical)
                };
                quote! {
                    if #matches {
                        return Some(#kind);
                    }
                }
            });
            helper_items.push(quote! {
                fn #helper(text: &str) -> Option<u16> {
                    #(#comparisons)*
                    None
                }
            });
            helper
        })
        .collect::<Vec<_>>();

    let mut level_index = 1_usize;
    while level.len() > 1 {
        let next_level = level
            .chunks(CLASSIFIERS_PER_HELPER)
            .enumerate()
            .map(|(index, children)| {
                let helper = format_ident!("__recursa_classify_words_{level_index}_{index}");
                let calls = children.iter().map(|child| {
                    quote! {
                        if let Some(kind) = #child(text) {
                            return Some(kind);
                        }
                    }
                });
                helper_items.push(quote! {
                    fn #helper(text: &str) -> Option<u16> {
                        #(#calls)*
                        None
                    }
                });
                helper
            })
            .collect::<Vec<_>>();
        level = next_level;
        level_index += 1;
    }

    let classification = level.first().map(|root| {
        quote! {
            let Some(text) = source.get(record.span.range()) else {
                return;
            };
            if let Some(kind) = #root(text) {
                record.kind = kind;
            }
        }
    });

    quote! {
        #(#helper_items)*

        fn __recursa_classify(
            source: &str,
            record: &mut #facade::__private::TokenRecord,
        ) {
            if ![#(#candidate_kinds),*].contains(&record.kind) {
                return;
            }
            #classification
        }
    }
}

/// One complete logical output ready for filesystem publication.
///
/// Root and Node files come from typed token streams, anchors align with Node order, and the
/// selector names only this generation identity. Checked test plans additionally parse every
/// logical Rust file. This value contains no partially rendered state.
pub(super) struct RenderedGeneration {
    /// Structural root source shared by every Node loader.
    pub(super) root: String,
    /// Node filenames and sources in parse-plan order.
    pub(super) nodes: Vec<(String, String)>,
    /// Loader anchors positionally aligned with [`Self::nodes`].
    pub(super) node_anchors: Vec<String>,
    /// Loader source that selects this Generation identity.
    pub(super) selector: String,
}

/// Supplies an immutable emission plan and its boundary-specific checks to rendering.
pub(super) trait RenderingPlan {
    /// Borrows the plan consumed by capability renderers.
    fn emission_plan(&self) -> &EmissionPlan;

    /// Rechecks plan invariants when a test deliberately bypasses the validation proof.
    fn validate_for_rendering(&self) -> Result<(), String>;

    /// Checks rendered Rust when exercising the internal syntax-failure contract.
    fn validate_rendered_rust(&self, tokens: &proc_macro2::TokenStream) -> Result<(), syn::Error>;
}

impl RenderingPlan for ValidatedEmissionPlan {
    fn emission_plan(&self) -> &EmissionPlan {
        self
    }

    fn validate_for_rendering(&self) -> Result<(), String> {
        Ok(())
    }

    fn validate_rendered_rust(&self, _tokens: &proc_macro2::TokenStream) -> Result<(), syn::Error> {
        Ok(())
    }
}

#[cfg(test)]
impl RenderingPlan for EmissionPlan {
    fn emission_plan(&self) -> &EmissionPlan {
        self
    }

    fn validate_for_rendering(&self) -> Result<(), String> {
        self.validate()
    }

    fn validate_rendered_rust(&self, tokens: &proc_macro2::TokenStream) -> Result<(), syn::Error> {
        syn::parse2::<syn::File>(tokens.clone()).map(|_| ())
    }
}

/// Renders every logical file without performing filesystem publication.
pub(super) fn render(plan: &impl RenderingPlan) -> Result<RenderedGeneration, String> {
    plan.validate_for_rendering()?;
    let rendering = plan;
    let plan = rendering.emission_plan();
    let root_tokens = render_root(plan);
    rendering
        .validate_rendered_rust(&root_tokens)
        .map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    let root_tokens = apply_generated_lint_boundary(root_tokens, "root")
        .map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    let root = root_tokens.to_string();
    let mut instrumentation = RenderInstrumentation::default();
    let mut nodes = Vec::new();
    for (index, node) in plan.nodes.iter().enumerate() {
        let name = format!("node-{index}.rs");
        let tokens = render_node(plan, node, index, &mut instrumentation);
        rendering
            .validate_rendered_rust(&tokens)
            .map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
        let tokens = apply_generated_lint_boundary(tokens, &format!("node_{index}"))
            .map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
        nodes.push((name, tokens.to_string()));
    }
    if instrumentation.frozen_dispatch_nodes_rendered
        != plan.instrumentation.frozen_dispatch_nodes_consumed
    {
        return Err("the renderer did not consume exactly the frozen dispatch trie".to_owned());
    }
    let selected_root = format!("/recursa-generations/{}/root.rs", plan.identity);
    let selector_tokens = quote! {
        include!(concat!(env!("OUT_DIR"), #selected_root));
    };
    rendering
        .validate_rendered_rust(&selector_tokens)
        .map_err(|error| format!("Generation selector rendering is invalid Rust: {error}"))?;
    let selector_body = selector_tokens.to_string();
    let selector = format!("// recursa-generation:{}\n{selector_body}", plan.identity);
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

/// Renders the closed generated-code lint policy as one outer attribute.
fn generated_lint_policy() -> proc_macro2::TokenStream {
    quote! {
        #[allow(
            dead_code,
            non_camel_case_types,
            private_bounds,
            private_interfaces,
            unreachable_patterns,
            unused_variables,
            clippy::blocks_in_conditions,
            clippy::borrowed_box,
            clippy::collapsible_if,
            clippy::match_single_binding,
            clippy::needless_borrow,
            clippy::needless_return,
            clippy::never_loop,
            clippy::new_ret_no_self,
            clippy::possible_missing_else,
            clippy::unnecessary_literal_unwrap
        )]
    }
}

/// Applies the closed generated-code lint policy once per generated scope boundary.
///
/// The source files are loaded into a downstream module with `include!`, where a file-level inner
/// attribute is not permitted and an attribute on the `include!` invocation is ignored. The policy
/// is therefore applied to scope boundaries that the generated file owns, which keeps handwritten
/// downstream items under their crate's lint settings while avoiding one attribute per item:
///
/// - `impl` blocks and anonymous constants move into one `const _: () = { ... };` block. Impls are
///   global, so the move changes no path.
/// - Private and `pub(crate)` functions, statics, and named constants move into one private
///   module that is glob-imported back into the enclosing scope. Private items become
///   `pub(super)` there, which is the same reach they had as private items of the enclosing
///   module. `pub(crate)` items keep their visibility and the glob import is `pub(crate)`, so
///   absolute `crate::` paths from other generated files still resolve.
/// - Every other item is nameable by downstream code at its original path and keeps an
///   individual attribute.
///
/// Rust ignores lint attributes on macro invocations, so the known pass-through capability gates
/// are traversed and the same boundaries are built inside their token bodies. Structural
/// handshake and loader macros remain unchanged. The `scope` name keeps the private module names
/// unique when several generated files load into one downstream module.
fn apply_generated_lint_boundary(
    tokens: proc_macro2::TokenStream,
    scope: &str,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut next_scope = 0usize;
    bound_lint_scope(tokens, scope, &mut next_scope)
}

/// Builds the lint scope boundaries for one item list and, recursively, its capability gates.
fn bound_lint_scope(
    tokens: proc_macro2::TokenStream,
    scope: &str,
    next_scope: &mut usize,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let file = syn::parse2::<syn::File>(tokens)?;
    let policy = generated_lint_policy();
    let mut rendered = proc_macro2::TokenStream::new();
    let mut scoped_impls = Vec::new();
    let mut scoped_privates = Vec::new();
    let mut reexport_crate_wide = false;
    for item in file.items {
        match item {
            syn::Item::Macro(mut item) => {
                if item.mac.path.is_ident("__recursa_if_arbitrary")
                    || item.mac.path.is_ident("__recursa_if_serde")
                    || item.mac.path.is_ident("__recursa_if_explorer")
                {
                    item.mac.tokens = bound_lint_scope(item.mac.tokens, scope, next_scope)?;
                }
                rendered.extend(quote!(#item));
            }
            syn::Item::Impl(_) => scoped_impls.push(item),
            syn::Item::Const(constant) if constant.ident == "_" => {
                scoped_impls.push(syn::Item::Const(constant));
            }
            syn::Item::Fn(mut function) if is_scoped_visibility(&function.vis) => {
                reexport_crate_wide |= scope_visibility(&mut function.vis);
                scoped_privates.push(syn::Item::Fn(function));
            }
            syn::Item::Static(mut statik) if is_scoped_visibility(&statik.vis) => {
                reexport_crate_wide |= scope_visibility(&mut statik.vis);
                scoped_privates.push(syn::Item::Static(statik));
            }
            syn::Item::Const(mut constant) if is_scoped_visibility(&constant.vis) => {
                reexport_crate_wide |= scope_visibility(&mut constant.vis);
                scoped_privates.push(syn::Item::Const(constant));
            }
            item => rendered.extend(quote!(#policy #item)),
        }
    }
    if !scoped_privates.is_empty() {
        let module = format_ident!("__recursa_lint_scope_{scope}_{next_scope}");
        *next_scope += 1;
        let import = if reexport_crate_wide {
            quote!(pub(crate) use #module::*;)
        } else {
            quote!(use #module::*;)
        };
        rendered.extend(quote! {
            #policy
            mod #module {
                #[allow(unused_imports)]
                use super::*;
                #(#scoped_privates)*
            }
            #[allow(unused_imports)]
            #import
        });
    }
    if !scoped_impls.is_empty() {
        rendered.extend(quote! {
            #policy
            const _: () = {
                #(#scoped_impls)*
            };
        });
    }
    Ok(rendered)
}

/// Reports whether an item may move into a lint scope module.
///
/// Private and `pub(crate)` items move. Any other visibility stays at its original module level
/// because a nested module would change the meaning of `pub(super)` and of `pub(in path)`, and
/// public API must keep its documented path.
fn is_scoped_visibility(visibility: &Visibility) -> bool {
    match visibility {
        Visibility::Inherited => true,
        Visibility::Restricted(_) => is_crate_visibility(visibility),
        Visibility::Public(_) => false,
    }
}

/// Rewrites a scoped item's visibility for its new module and reports crate-wide reach.
///
/// Private items become `pub(super)`, which restores their original reach from inside the scope
/// module. `pub(crate)` items keep their visibility and return `true` so the enclosing scope
/// re-exports the module crate-wide.
fn scope_visibility(visibility: &mut Visibility) -> bool {
    if is_crate_visibility(visibility) {
        return true;
    }
    *visibility = syn::parse_quote!(pub(super));
    false
}

/// Reports whether a visibility is exactly `pub(crate)`.
fn is_crate_visibility(visibility: &Visibility) -> bool {
    matches!(
        visibility,
        Visibility::Restricted(restricted)
            if restricted.in_token.is_none() && restricted.path.is_ident("crate")
    )
}

/// Generated root fragments that exist only when a grammar retains classified trivia.
struct TriviaRootFragments {
    /// Initial sidecar state for the alternative-lexer builder.
    state_initial: proc_macro2::TokenStream,
    /// Alternative-lexer method that appends one classified trivia record.
    builder_append_method: Option<proc_macro2::TokenStream>,
    /// Public grammar-local trivia kinds and immutable gap views.
    public_api: Option<proc_macro2::TokenStream>,
    /// Lex-result method exposing the retained source gaps.
    lex_result_method: Option<proc_macro2::TokenStream>,
    /// Public trivia names re-exported from the private lexical module.
    public_exports: Option<proc_macro2::TokenStream>,
}

/// Renders the complete opt-in trivia surface as one cohesive root-generation phase.
fn render_trivia_root_fragments(plan: &EmissionPlan, facade: &syn::Ident) -> TriviaRootFragments {
    let retained_trivia = !plan.trivia_kinds.is_empty();
    let trivia_kind_variants = &plan.trivia_kinds;
    let trivia_kind_match_arms = trivia_kind_variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let index = index as u16;
            quote!(#index => TriviaKind::#variant,)
        });
    let state_initial = if retained_trivia {
        quote!(Some(Vec::new()))
    } else {
        quote!(None)
    };
    let builder_append_method = retained_trivia.then(|| {
        quote! {
            /// Appends one explicitly classified trivia range without rescanning source gaps.
            pub fn append_trivia(
                &mut self,
                kind: TriviaKind,
                span: #facade::Span,
            ) -> Result<&mut Self, LexBuildError> {
                self.validate_next(span)?;
                self.trivia
                    .as_mut()
                    .expect("this generated grammar retains trivia")
                    .push(#facade::__private::TriviaRecord {
                        kind: __recursa_raw_trivia_kind(kind),
                        span,
                    });
                self.previous = Some(span);
                Ok(self)
            }
        }
    });
    let public_api = retained_trivia.then(|| {
        quote! {
            /// One grammar-local classified trivia kind.
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
            pub enum TriviaKind {
                #(#trivia_kind_variants,)*
            }

            fn __recursa_raw_trivia_kind(kind: TriviaKind) -> u16 {
                match kind {
                    #(TriviaKind::#trivia_kind_variants => {
                        TriviaKind::#trivia_kind_variants as u16
                    },)*
                }
            }

            fn __recursa_typed_trivia_kind(kind: u16) -> TriviaKind {
                match kind {
                    #(#trivia_kind_match_arms)*
                    _ => unreachable!("generated trivia records use a planned kind"),
                }
            }

            /// Immutable beginning, inter-record, and end source gaps.
            pub struct TriviaGaps<'view> {
                pub(crate) inner: #facade::__private::SourceGaps<'view>,
            }

            impl<'view> Iterator for TriviaGaps<'view> {
                type Item = TriviaGap<'view>;

                fn next(&mut self) -> Option<Self::Item> {
                    self.inner.next().map(|inner| TriviaGap { inner })
                }

                fn size_hint(&self) -> (usize, Option<usize>) {
                    self.inner.size_hint()
                }
            }

            impl ExactSizeIterator for TriviaGaps<'_> {}
            impl ::core::iter::FusedIterator for TriviaGaps<'_> {}

            /// One exact source gap with its ordered classified trivia.
            #[derive(Clone, Copy)]
            pub struct TriviaGap<'view> {
                inner: #facade::__private::SourceGap<'view>,
            }

            impl<'view> TriviaGap<'view> {
                /// Returns the checked exact gap extent.
                pub fn span(self) -> #facade::Span {
                    self.inner.span()
                }

                /// Returns the exact gap source text, including explicitly unrecorded bytes.
                pub fn text(self) -> &'view str {
                    self.inner.text()
                }

                /// Iterates explicitly classified trivia in source order.
                pub fn trivia(self) -> Trivia<'view> {
                    Trivia { inner: self.inner.trivia() }
                }
            }

            /// Exact-size iterator over classified trivia in one gap.
            pub struct Trivia<'view> {
                inner: #facade::__private::TriviaRecords<'view>,
            }

            impl<'view> Iterator for Trivia<'view> {
                type Item = TriviaRecord<'view>;

                fn next(&mut self) -> Option<Self::Item> {
                    self.inner.next().map(|inner| TriviaRecord { inner })
                }

                fn size_hint(&self) -> (usize, Option<usize>) {
                    self.inner.size_hint()
                }
            }

            impl ExactSizeIterator for Trivia<'_> {}
            impl ::core::iter::FusedIterator for Trivia<'_> {}

            /// One immutable classified trivia occurrence.
            #[derive(Clone, Copy)]
            pub struct TriviaRecord<'view> {
                inner: #facade::__private::TriviaRecordView<'view>,
            }

            impl<'view> TriviaRecord<'view> {
                /// Returns the generated grammar-local classification.
                pub fn kind(self) -> TriviaKind {
                    __recursa_typed_trivia_kind(self.inner.raw_kind())
                }

                /// Returns the checked exact trivia extent.
                pub fn span(self) -> #facade::Span {
                    self.inner.span()
                }

                /// Returns exact matched trivia text.
                pub fn text(self) -> &'view str {
                    self.inner.text()
                }
            }
        }
    });
    let lex_result_method = retained_trivia.then(|| {
        quote! {
            /// Iterates immutable beginning, inter-record, and end source gaps.
            pub fn trivia_gaps(&self) -> TriviaGaps<'_> {
                TriviaGaps {
                    inner: #facade::__private::lex_source_gaps(&self.inner)
                        .expect("this generated grammar retains trivia"),
                }
            }
        }
    });
    let public_exports = retained_trivia.then(|| {
        quote! {
            Trivia, TriviaGap, TriviaGaps, TriviaKind, TriviaRecord,
        }
    });

    TriviaRootFragments {
        state_initial,
        builder_append_method,
        public_api,
        lex_result_method,
        public_exports,
    }
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
    let word_classification =
        render_word_classifiers(&plan.word_classifiers, word_candidate_kinds, facade);
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
    let trivia_kinds = plan
        .lexical_rules
        .iter()
        .map(|rule| match rule.trivia_kind {
            Some(kind) => quote!(Some(#kind)),
            None => quote!(None),
        });
    let fixed = plan.lexical_rules.iter().map(|rule| rule.fixed);
    let priorities = plan.lexical_rules.iter().map(|rule| rule.priority);
    let empty_words = vec![0_u64; kind_count.div_ceil(64)];
    let expected_set_count = plan.expected_sets.values().len();
    let expected_sets = plan
        .expected_sets
        .values()
        .iter()
        .map(|expected| render_expected_value(expected, plan, facade));
    let token_items = plan
        .tokens
        .iter()
        .enumerate()
        .map(|(index, token)| render_token(plan, token, index));
    let content_token_pretty_helper = render_content_token_pretty_helper(plan);
    let content_token_optional_trait_helpers = render_content_token_optional_trait_helpers(plan);
    let TriviaRootFragments {
        state_initial: trivia_state_initial,
        builder_append_method: builder_append_trivia,
        public_api: trivia_api,
        lex_result_method: lex_result_trivia_gaps,
        public_exports: trivia_exports,
    } = render_trivia_root_fragments(plan, facade);
    let visit_events = render_visit_events(plan);
    let explorer_schema = plan
        .explorer
        .as_ref()
        .map(|explorer| render_explorer_static_schema(plan, explorer));
    let explorer_lex = plan.explorer.as_ref().map(|_| render_explorer_lex(plan));
    let framing_lex = plan.framing.as_ref().map(|_| {
        quote! {
            pub(super) fn __recursa_frame_lex(
                source: &str,
            ) -> #facade::__private::LexResult<'_> {
                lex(source).inner
            }
        }
    });
    let framing_root = plan.framing.as_ref().map(|framing| {
        let boundary_kind = framing.boundary_kind;
        let line_kinds = &framing.line_kinds;
        let delimited_kinds = &framing.delimited_kinds;
        let mut boundary_words = vec![0_u64; plan.kind_count.div_ceil(64)];
        boundary_words[usize::from(boundary_kind) / 64] |=
            1_u64 << (usize::from(boundary_kind) % 64);
        let node = &plan.nodes[framing.node_index];
        let application = &node.parse_applications[framing.application_index];
        let recovery_plan = explorer_recovery_plan_static(application.parse_index);
        let payload = framing.payload.as_ref().map_or_else(
            || quote!(None),
            |payload| {
                let variant =
                    explorer_schema_static(application.parse_index, Some(payload.variant_index));
                let terminator = &payload.terminator;
                quote!(Some(#facade::__private::framing_payload_plan(
                    &#variant,
                    #terminator,
                )))
            },
        );
        quote! {
            static __RECURSA_FRAMING_BOUNDARY_FOLLOW: #facade::__private::FollowSet =
                #facade::__private::follow_set(
                    #kind_count as u32,
                    &[#(#boundary_words),*],
                    &[],
                );
            static __RECURSA_FRAMING_PLAN: #facade::__private::FramingPlanData =
                #facade::__private::framing_plan(
                    &__RECURSA_GRAMMAR,
                    #boundary_kind,
                    &[#(#line_kinds),*],
                    &[#(#delimited_kinds),*],
                    #payload,
                    &#recovery_plan,
                    &__RECURSA_FRAMING_BOUNDARY_FOLLOW,
                );

            pub use #facade::framing::{
                CompleteFrame, CompleteIsland, CompletePart, FrameDiagnostic,
                FrameDiagnosticKind, FrameError, FrameResult, PayloadRegion,
                RecoveredFrame, RecoveredIsland, RecoveredPart, SourceRegion,
            };
        }
    });
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
        static __RECURSA_EXPECTED_SETS: [#facade::__private::FollowSet; #expected_set_count] = [
            #(#expected_sets),*
        ];
        #framing_root

        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
                        trivia_kind: #trivia_kinds,
                        fixed: #fixed,
                        priority: #priorities,
                    }
                ),*
            ];

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

        #word_classification

        fn __recursa_finalize<'input>(
            source: &'input str,
            mut records: Vec<#facade::__private::TokenRecord>,
            trivia: Option<Vec<#facade::__private::TriviaRecord>>,
        ) -> #facade::__private::LexResult<'input> {
            for record in &mut records {
                __recursa_classify(source, record);
            }
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
            #facade::__private::LexResult::new(source, records, trivia)
        }

        #trivia_api

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
        ///
        /// This grammar-local type deliberately repeats the matching-version core protocol rather
        /// than exposing the private core enum as part of a generated grammar's public API. Future
        /// lexical mechanisms may add classifications without making downstream matches exhaustive.
        #[non_exhaustive]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum LexErrorCode {
            /// No lexical rule accepted the scalar, or no public mechanism-specific class exists.
            Unrecognized,
            /// A same-delimiter region reached EOF without its matching closer.
            UnterminatedSameDelimiter,
            /// A nested region reached EOF with nonzero nesting depth.
            UnterminatedNested,
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
                // Keep this conversion exhaustive and explicit: generated output and core are a
                // matching-version protocol, while their public and private enum types remain
                // intentionally decoupled.
                match self.record.diagnostic.map(|diagnostic| diagnostic.code) {
                    Some(#facade::__private::LexDiagnosticCode::UnterminatedSameDelimiter) => {
                        LexErrorCode::UnterminatedSameDelimiter
                    }
                    Some(#facade::__private::LexDiagnosticCode::UnterminatedNested) => {
                        LexErrorCode::UnterminatedNested
                    }
                    Some(#facade::__private::LexDiagnosticCode::ExcludedFollowingCharacter) => {
                        LexErrorCode::ExcludedFollowingCharacter
                    }
                    Some(#facade::__private::LexDiagnosticCode::MatcherInvariant) => {
                        LexErrorCode::Unrecognized
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
            trivia: Option<Vec<#facade::__private::TriviaRecord>>,
            previous: Option<#facade::Span>,
        }

        impl<'input> LexBuilder<'input> {
            pub fn new(source: &'input str) -> Self {
                Self {
                    source,
                    records: Vec::new(),
                    trivia: #trivia_state_initial,
                    previous: None,
                }
            }

            fn validate_next(&self, span: #facade::Span) -> Result<(), LexBuildError> {
                if self.source.get(span.range()).is_none() {
                    return Err(LexBuildError::InvalidSpan { span });
                }
                if let Some(previous) = self.previous {
                    if span.start() < previous.end() {
                        return Err(LexBuildError::OutOfOrder { previous, span });
                    }
                }
                Ok(())
            }

            pub fn append(
                &mut self,
                kind: TokenKind,
                span: #facade::Span,
            ) -> Result<&mut Self, LexBuildError> {
                self.validate_next(span)?;
                self.records.push(#facade::__private::TokenRecord {
                    kind: __recursa_raw_kind(kind),
                    span,
                    diagnostic: None,
                });
                self.previous = Some(span);
                Ok(self)
            }

            #builder_append_trivia

            pub fn finish(self) -> LexResult<'input> {
                LexResult {
                    inner: __recursa_finalize(self.source, self.records, self.trivia),
                }
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

            #lex_result_trivia_gaps

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
            let trivia = #facade::__private::lex_trivia(&scanned).map(<[_]>::to_vec);
            LexResult { inner: __recursa_finalize(source, records, trivia) }
        }

        #framing_lex

        }

        pub use __recursa_lexical::{
            GrammarInfo, LexBuildError, LexBuilder, LexError, LexErrorCode, LexErrors, LexResult,
            Token, Tokens, #trivia_exports grammar_info, lex,
        };

        #[doc(hidden)]
        pub(crate) mod __recursa_content_tokens {
            #(#token_items)*
        }

        #content_token_pretty_helper
        #content_token_optional_trait_helpers
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
    let trivia_gaps = (!plan.trivia_kinds.is_empty()).then(|| {
        quote! {
            /// Iterates immutable source gaps retained by the complete parsed document.
            pub fn trivia_gaps(self) -> #module_anchor::TriviaGaps<'view> {
                #module_anchor::TriviaGaps {
                    inner: self.cursor.source_gaps()
                        .expect("this generated grammar retains trivia"),
                }
            }
        }
    });
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
            &render_node_diagram_document(diagram, diagrams.anchor_depth),
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

            #trivia_gaps

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
            ) -> <Self as #facade::__private::NodeViewFactory>::View<'view> {
                #view_ident { value: self, cursor }
            }
        }

        #(#parse_implementations)*

        #configured_traits
        #explorer

        #diagram_attribute
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
