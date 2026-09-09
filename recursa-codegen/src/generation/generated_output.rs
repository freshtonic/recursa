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
        deduplicated_word_classifier_keys, render_word_classifiers, word_classifier_key,
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
        assert!(bounded.contains("const _ : fn () = || { } ;"));
        assert!(
            !bounded.contains(")] impl"),
            "impls carry no attribute of their own"
        );
        assert!(!bounded.contains(")] const _ : fn"));
        assert!(
            !bounded.contains(")] fn "),
            "free functions carry no attribute of their own"
        );
        assert!(!bounded.contains(")] static "));
        let allowed_prefix = bounded
            .split("pub struct NodeView")
            .next()
            .expect("struct prefix");
        assert!(allowed_prefix.trim_end().ends_with(")]"));
    }

    #[test]
    fn word_classification_buckets_normal_vocabularies_and_bounds_dense_ones() {
        let mut classifiers = (0_u16..4_097)
            .map(|kind| WordClassifierPlan {
                canonical_spelling: format!("large_word_{kind}"),
                kind,
                ascii_insensitive: true,
            })
            .collect::<Vec<_>>();
        classifiers.push(WordClassifierPlan {
            canonical_spelling: "unique".to_owned(),
            kind: 5_000,
            ascii_insensitive: true,
        });

        let facade: syn::Ident = syn::parse_quote!(recursa);
        let rendered = render_word_classifiers(&classifiers, &[7], &facade).to_string();

        // Exactly one bucket classifier and one bounded fallback, regardless of
        // vocabulary size: no sharded linear helper tree is emitted.
        assert_eq!(rendered.matches("fn __recursa_classify_words (").count(), 1);
        assert_eq!(
            rendered.matches("static __RECURSA_WORD_KEYWORDS").count(),
            1
        );
        assert_eq!(rendered.matches("source . get").count(), 1);
        assert!(rendered.contains("let Some (text) = source . get"));
        assert!(rendered.contains("record . kind = kind"));
        assert_eq!(rendered.matches("record . kind =").count(), 1);
        assert!(rendered.contains("match record . kind"));
        assert!(!rendered.contains(". contains"));
        assert!(rendered.contains("match (bytes . len () , first)"));
        assert!(rendered.contains("bytes . eq_ignore_ascii_case"));
        assert!(rendered.contains("recursa :: __private :: phf :: Map"));
        assert!(rendered.contains("__RECURSA_WORD_KEYWORDS . get"));
        // Empty candidates and over-length fallback candidates are rejected
        // before probing the map.
        assert!(rendered.contains("bytes . first () . copied () ?"));
        assert!(rendered.contains("bytes . len () > MAX_KEYWORD_LEN"));
        assert!(rendered.contains("to_ascii_lowercase ()"));
    }

    #[test]
    fn word_classification_is_case_sensitive_without_folding_when_the_grammar_demands_it() {
        let classifiers = vec![WordClassifierPlan {
            canonical_spelling: "Loop".to_owned(),
            kind: 3,
            ascii_insensitive: false,
        }];
        let facade: syn::Ident = syn::parse_quote!(recursa);
        let rendered = render_word_classifiers(&classifiers, &[7], &facade).to_string();

        assert!(!rendered.contains("to_ascii_lowercase"));
        let (keys, kinds) = deduplicated_word_classifier_keys(&classifiers, false);
        assert_eq!(keys, vec![b"Loop".to_vec()], "exact case is preserved");
        assert_eq!(kinds, vec![3]);
    }

    #[test]
    fn word_classification_folds_a_duplicate_key_to_its_first_declared_kind() {
        // Two spellings that fold to the same key under ASCII-insensitive comparison
        // must keep only the first declaration's kind, matching the flat if-chain this
        // rendering replaced (earlier keywords shadowed later duplicate spellings).
        let classifiers = vec![
            WordClassifierPlan {
                canonical_spelling: "SELECT".to_owned(),
                kind: 11,
                ascii_insensitive: true,
            },
            WordClassifierPlan {
                canonical_spelling: "select".to_owned(),
                kind: 22,
                ascii_insensitive: true,
            },
        ];
        let (keys, kinds) = deduplicated_word_classifier_keys(&classifiers, true);
        assert_eq!(keys, vec![b"select".to_vec()]);
        assert_eq!(
            kinds,
            vec![11],
            "the first declared kind wins the collision"
        );

        // The rendering itself must not panic on the surviving single-entry map, and
        // must still expose the winning kind number literally as an entry value.
        let facade: syn::Ident = syn::parse_quote!(recursa);
        let rendered = render_word_classifiers(&classifiers, &[7], &facade).to_string();
        assert_eq!(rendered.matches("fn __recursa_classify_words (").count(), 1);
    }

    #[test]
    fn word_classifier_key_folds_only_ascii_bytes() {
        assert_eq!(word_classifier_key("SELECT", true), b"select".to_vec());
        assert_eq!(word_classifier_key("SELECT", false), b"SELECT".to_vec());
        assert_eq!(
            word_classifier_key("Ä", true),
            "Ä".bytes().collect::<Vec<_>>(),
            "non-ASCII bytes pass through untouched under ASCII-only folding"
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
}

/// Folds one canonical spelling into the byte key used by the generated perfect-hash map.
///
/// ASCII-insensitive grammars lowercase only ASCII letters, leaving any non-ASCII byte
/// untouched; the same folding is applied to candidate text bytes at lookup time, so the
/// comparison is equivalent to the ASCII-case-insensitive `eq_ignore_ascii_case` it replaces.
/// Sensitive grammars keep the exact bytes.
fn word_classifier_key(canonical_spelling: &str, ascii_insensitive: bool) -> Vec<u8> {
    canonical_spelling
        .bytes()
        .map(|byte| {
            if ascii_insensitive {
                byte.to_ascii_lowercase()
            } else {
                byte
            }
        })
        .collect()
}

/// Folds and deduplicates one grammar's word classifiers into lookup keys.
///
/// First occurrence wins on a folded-key collision, matching the flat if-chain this
/// rendering replaces: earlier keywords in declaration order shadowed later duplicate
/// spellings. Returns parallel `(keys, kinds)` vectors, one pair per surviving classifier.
fn deduplicated_word_classifier_keys(
    classifiers: &[WordClassifierPlan],
    ascii_insensitive: bool,
) -> (Vec<Vec<u8>>, Vec<u16>) {
    let mut seen_keys = std::collections::HashSet::new();
    let mut keys = Vec::new();
    let mut kinds = Vec::new();
    for classifier in classifiers {
        let key = word_classifier_key(&classifier.canonical_spelling, ascii_insensitive);
        if seen_keys.insert(key.clone()) {
            keys.push(key);
            kinds.push(classifier.kind);
        }
    }
    (keys, kinds)
}

/// Renders bounded private keyword classifiers and their single record-update entry point.
///
/// Classification first partitions folded UTF-8 keys by byte length and first byte. Small
/// buckets compare directly; unusually dense buckets use a perfect-hash lookup so generated
/// work remains bounded independently of vocabulary shape.
fn render_word_classifiers(
    classifiers: &[WordClassifierPlan],
    candidate_kinds: &[u16],
    facade: &syn::Ident,
) -> proc_macro2::TokenStream {
    let ascii_insensitive = classifiers
        .first()
        .is_none_or(|classifier| classifier.ascii_insensitive);
    debug_assert!(
        classifiers
            .iter()
            .all(|classifier| classifier.ascii_insensitive == ascii_insensitive),
        "one grammar's word classifiers share one case-equivalence policy"
    );

    let (keys, kinds) = deduplicated_word_classifier_keys(classifiers, ascii_insensitive);
    let max_key_len = keys.iter().map(Vec::len).max().unwrap_or(0);

    // Most real keyword vocabularies become tiny buckets after partitioning by
    // byte length and first folded byte. Compare those few candidates directly,
    // while retaining the bounded perfect-hash fallback for unusually dense
    // generated vocabularies.
    const DIRECT_BUCKET_LIMIT: usize = 8;
    let mut buckets = std::collections::BTreeMap::<(usize, u8), Vec<usize>>::new();
    for (index, key) in keys.iter().enumerate() {
        if let Some(first) = key.first() {
            buckets.entry((key.len(), *first)).or_default().push(index);
        }
    }
    let bucket_arms = buckets.iter().map(|(&(length, first), indexes)| {
        if indexes.len() > DIRECT_BUCKET_LIMIT {
            return quote! {
                (#length, #first) => __recursa_classify_words_phf(bytes),
            };
        }
        let comparisons = indexes.iter().map(|&index| {
            let key = proc_macro2::Literal::byte_string(&keys[index]);
            let kind = kinds[index];
            let matches = if ascii_insensitive {
                quote!(bytes.eq_ignore_ascii_case(#key))
            } else {
                quote!(bytes == #key)
            };
            quote! {
                if #matches {
                    return Some(#kind);
                }
            }
        });
        quote! {
            (#length, #first) => {
                #(#comparisons)*
                None
            }
        }
    });

    let phf_path = format!("{facade}::__private::phf");
    let mut builder = phf_codegen::Map::<&[u8]>::new();
    builder.phf_path(&phf_path);
    for (key, kind) in keys.iter().zip(kinds.iter()) {
        builder.entry(key.as_slice(), &kind.to_string());
    }
    let map_source = builder.build().to_string();
    let map_tokens: proc_macro2::TokenStream = map_source
        .parse()
        .expect("phf_codegen renders the generated map as valid Rust tokens");

    let fold_byte = if ascii_insensitive {
        quote!(bytes[index].to_ascii_lowercase())
    } else {
        quote!(bytes[index])
    };
    let fold_first = if ascii_insensitive {
        quote!(first.to_ascii_lowercase())
    } else {
        quote!(first)
    };

    quote! {
        static __RECURSA_WORD_KEYWORDS: #facade::__private::phf::Map<&'static [u8], u16> =
            #map_tokens;

        fn __recursa_classify_words_phf(bytes: &[u8]) -> Option<u16> {
            const MAX_KEYWORD_LEN: usize = #max_key_len;
            if bytes.len() > MAX_KEYWORD_LEN {
                return None;
            }
            let mut folded = [0_u8; MAX_KEYWORD_LEN];
            let mut index = 0_usize;
            while index < bytes.len() {
                folded[index] = #fold_byte;
                index += 1;
            }
            __RECURSA_WORD_KEYWORDS.get(&folded[..bytes.len()]).copied()
        }

        fn __recursa_classify_words(text: &str) -> Option<u16> {
            let bytes = text.as_bytes();
            let first = bytes.first().copied()?;
            let first = #fold_first;
            match (bytes.len(), first) {
                #(#bucket_arms)*
                _ => None,
            }
        }

        fn __recursa_classify(
            source: &str,
            record: &mut #facade::__private::TokenRecord,
        ) {
            match record.kind {
                #(#candidate_kinds => {},)*
                _ => return,
            }
            let Some(text) = source.get(record.span.range()) else {
                return;
            };
            if let Some(kind) = __recursa_classify_words(text) {
                record.kind = kind;
            }
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
    /// Non-Rust binary payloads referenced by the structural root.
    pub(super) assets: Vec<(String, Vec<u8>)>,
    /// Node filenames and sources in parse-plan order.
    pub(super) nodes: Vec<(String, String)>,
    /// Loader anchors positionally aligned with [`Self::nodes`].
    pub(super) node_anchors: Vec<String>,
    /// Loader source that selects this Generation identity.
    pub(super) selector: String,
    /// Encoded diagram plan rendered by the facade while `cargo doc` expands generated attributes.
    pub(super) diagram_plan: Option<String>,
    /// Bison-style automaton dump of a table-driven generation, for the debug bundle only.
    pub(super) automaton_dump: Option<String>,
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

/// Renders every logical file of a generation after constructing the LALR(1) parser.
pub(super) fn render_declared(
    plan: &impl RenderingPlan,
    lowered: &crate::materialization::LoweredGrammar,
) -> Result<RenderedGeneration, String> {
    let parsers = ParserSet::declared(plan.emission_plan(), lowered)?;
    render_generation(plan, &parsers)
}

#[cfg(test)]
/// Renders a unit-test plan with the LR lowering captured when the plan was joined.
pub(super) fn render(plan: &impl RenderingPlan) -> Result<RenderedGeneration, String> {
    let identity = &plan.emission_plan().identity;
    let lowered = test_lowered(identity)
        .ok_or_else(|| format!("unit-test LR lowering is missing for generation {identity}"))?;
    render_declared(plan, &lowered)
}

/// Renders every logical file without performing filesystem publication; `parsers` names the
/// parsers whose bodies every node and content token carry.
fn render_generation(
    plan: &impl RenderingPlan,
    parsers: &ParserSet,
) -> Result<RenderedGeneration, String> {
    plan.validate_for_rendering()?;
    let rendering = plan;
    let plan = rendering.emission_plan();
    let root_tokens = render_root(plan, parsers)?;
    rendering
        .validate_rendered_rust(&root_tokens)
        .map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    let root_tokens = apply_generated_lint_boundary(root_tokens, "root")
        .map_err(|error| format!("root rendering is invalid Rust: {error}"))?;
    let root = root_tokens.to_string();
    let mut nodes = Vec::new();
    for (index, node) in plan.nodes.iter().enumerate() {
        let name = format!("node-{index}.rs");
        let tokens = render_node(plan, node, index, parsers)?;
        rendering
            .validate_rendered_rust(&tokens)
            .map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
        let tokens = apply_generated_lint_boundary(tokens, &format!("node_{index}"))
            .map_err(|error| format!("Node rendering is invalid Rust: {error}"))?;
        nodes.push((name, tokens.to_string()));
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
    #[cfg(feature = "diagrams")]
    let diagram_plan = plan.diagrams.as_ref().map(DiagramPlan::encode);
    #[cfg(not(feature = "diagrams"))]
    let diagram_plan = None;
    Ok(RenderedGeneration {
        root,
        assets: parsers
            .table_driven()
            .map(TableDrivenRendering::assets)
            .unwrap_or_default(),
        nodes,
        node_anchors: plan
            .nodes
            .iter()
            .map(|node| node.loader_anchor.clone())
            .collect(),
        selector,
        diagram_plan,
        automaton_dump: parsers.table_driven().map(|lr| lr.dump.clone()),
    })
}

/// Renders the grammar-local input newtype and its private cursor accessor.
pub(super) fn render_input_newtype(
    plan: &EmissionPlan,
    doc: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let accessors = input_accessors();
    quote! {
        #doc
        pub struct Input<'input, A = ()> {
            pub(crate) inner: #facade::__private::Input<'input, A>,
        }

        impl<'input, A> #facade::__private::GeneratedInput for Input<'input, A> {
            type Inner = #facade::__private::Input<'input, A>;
            fn __recursa_inner(&mut self) -> &mut Self::Inner {
                &mut self.inner
            }
        }

        impl<'input, A> Input<'input, A> {
            #accessors
        }
    }
}

/// The accessor methods of a grammar-local input newtype: the source, the builder, the
/// cursor and the end-of-input test.
fn input_accessors() -> proc_macro2::TokenStream {
    quote! {
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
///
/// Items are split at the token level. Only each item's head (attributes, visibility, and the
/// item keyword) is inspected, so impl and function bodies are never re-parsed: the generated
/// bodies are already well-formed token trees, and parsing them again with `syn` was the single
/// most expensive step of rendering.
fn bound_lint_scope(
    tokens: proc_macro2::TokenStream,
    scope: &str,
    next_scope: &mut usize,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let policy = generated_lint_policy();
    let mut rendered = proc_macro2::TokenStream::new();
    let mut scoped_impls = proc_macro2::TokenStream::new();
    let mut scoped_privates = proc_macro2::TokenStream::new();
    let mut reexport_crate_wide = false;
    for item in split_generated_items(tokens)? {
        match item.kind {
            RawItemKind::Macro {
                capability_gate: true,
                group,
            } => {
                let mut tokens = item.tokens;
                let proc_macro2::TokenTree::Group(gate) = &tokens[group] else {
                    unreachable!("a macro invocation ends in its delimited body");
                };
                let bounded = bound_lint_scope(gate.stream(), scope, next_scope)?;
                let mut rebuilt = proc_macro2::Group::new(gate.delimiter(), bounded);
                rebuilt.set_span(gate.span());
                tokens[group] = proc_macro2::TokenTree::Group(rebuilt);
                rendered.extend(tokens);
            }
            RawItemKind::Macro { .. } => rendered.extend(item.tokens),
            RawItemKind::Impl | RawItemKind::AnonymousConst => {
                scoped_impls.extend(item.tokens);
            }
            RawItemKind::Fn | RawItemKind::Static | RawItemKind::NamedConst
                if item.visibility != RawVisibility::Other =>
            {
                reexport_crate_wide |= item.visibility == RawVisibility::Crate;
                scoped_privates.extend(item.into_scoped_tokens());
            }
            _ => {
                rendered.extend(policy.clone());
                rendered.extend(item.tokens);
            }
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
                #scoped_privates
            }
            #[allow(unused_imports)]
            #import
        });
    }
    if !scoped_impls.is_empty() {
        rendered.extend(quote! {
            #policy
            const _: () = {
                #scoped_impls
            };
        });
    }
    Ok(rendered)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Item visibility as far as lint scoping needs to distinguish it.
enum RawVisibility {
    /// No visibility keyword: the item moves into a scope module as `pub(super)`.
    Inherited,
    /// Exactly `pub(crate)`: the item moves and the scope module is re-exported crate-wide.
    Crate,
    /// Any other visibility: the item stays at its original module level.
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Item classification derived from the head tokens of one generated item.
enum RawItemKind {
    /// A macro invocation or `macro_rules!` definition; `group` indexes its delimited body.
    Macro {
        /// Whether the invocation is one of the known capability pass-through gates.
        capability_gate: bool,
        /// Index of the delimited body within the item's tokens.
        group: usize,
    },
    /// An `impl` block, with or without `unsafe`.
    Impl,
    /// An anonymous `const _` item.
    AnonymousConst,
    /// A named constant.
    NamedConst,
    /// A free function.
    Fn,
    /// A static item.
    Static,
    /// Any other item; it keeps an individual lint attribute.
    Other,
}

/// One top-level generated item split from a token stream without parsing its body.
struct RawItem {
    /// Every token of the item, attributes first.
    tokens: Vec<proc_macro2::TokenTree>,
    /// Number of leading tokens that form outer attributes.
    attributes: usize,
    /// Number of tokens that form the visibility after the attributes.
    visibility_tokens: usize,
    /// Visibility classification.
    visibility: RawVisibility,
    /// Item classification.
    kind: RawItemKind,
}

impl RawItem {
    /// Rewrites the item for its scope module: private items become `pub(super)`.
    fn into_scoped_tokens(self) -> Vec<proc_macro2::TokenTree> {
        if self.visibility != RawVisibility::Inherited {
            return self.tokens;
        }
        let mut tokens = Vec::with_capacity(self.tokens.len() + 2);
        let attributes_end = self.attributes;
        let body_start = attributes_end + self.visibility_tokens;
        tokens.extend_from_slice(&self.tokens[..attributes_end]);
        tokens.extend(quote!(pub(super)));
        tokens.extend_from_slice(&self.tokens[body_start..]);
        tokens
    }
}

/// Reports whether a token is the given punctuation character.
fn is_punct(token: &proc_macro2::TokenTree, character: char) -> bool {
    matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == character)
}

/// Reports whether a token is the given identifier or keyword.
fn is_ident(token: &proc_macro2::TokenTree, name: &str) -> bool {
    matches!(token, proc_macro2::TokenTree::Ident(ident) if ident == name)
}

/// Reports whether a token is a group with the given delimiter.
fn is_group(token: &proc_macro2::TokenTree, delimiter: proc_macro2::Delimiter) -> bool {
    matches!(token, proc_macro2::TokenTree::Group(group) if group.delimiter() == delimiter)
}

/// Splits a generated item list at the token level.
///
/// A top-level item ends at the first `;` outside any delimiter group, or, for brace-bodied
/// items, at the first `{ ... }` group outside any delimiter group and outside generic
/// arguments. Constants, statics, type aliases, and imports end only at `;`, because their
/// types and initializers may carry const-generic brace groups before the terminator.
fn split_generated_items(tokens: proc_macro2::TokenStream) -> Result<Vec<RawItem>, syn::Error> {
    use proc_macro2::{Delimiter, TokenTree};

    let trees = tokens.into_iter().collect::<Vec<_>>();
    let mut items = Vec::new();
    let mut start = 0;
    while start < trees.len() {
        let no_terminator = |at: usize| {
            syn::Error::new(
                trees[at.min(trees.len() - 1)].span(),
                "generated item has no terminator",
            )
        };
        let mut index = start;
        while index < trees.len() && is_punct(&trees[index], '#') {
            let mut next = index + 1;
            if next < trees.len() && is_punct(&trees[next], '!') {
                next += 1;
            }
            if next < trees.len() && is_group(&trees[next], Delimiter::Bracket) {
                index = next + 1;
            } else {
                break;
            }
        }
        let attributes = index - start;
        if index >= trees.len() {
            return Err(no_terminator(index));
        }
        let mut visibility = RawVisibility::Inherited;
        let mut visibility_tokens = 0;
        if is_ident(&trees[index], "pub") {
            visibility_tokens = 1;
            visibility = RawVisibility::Other;
            if let Some(TokenTree::Group(restriction)) = trees.get(index + 1)
                && restriction.delimiter() == Delimiter::Parenthesis
            {
                let restriction = restriction.stream().into_iter().collect::<Vec<_>>();
                if restriction.len() == 1 && is_ident(&restriction[0], "crate") {
                    visibility = RawVisibility::Crate;
                }
                visibility_tokens = 2;
            }
        }
        let mut keyword = index + visibility_tokens;
        loop {
            let Some(token) = trees.get(keyword) else {
                return Err(no_terminator(keyword));
            };
            if is_ident(token, "unsafe")
                || is_ident(token, "async")
                || is_ident(token, "default")
                || (is_ident(token, "const")
                    && trees
                        .get(keyword + 1)
                        .is_some_and(|next| is_ident(next, "fn")))
            {
                keyword += 1;
            } else if is_ident(token, "extern")
                && trees
                    .get(keyword + 1)
                    .is_some_and(|next| matches!(next, TokenTree::Literal(_)))
            {
                keyword += 2;
            } else if is_ident(token, "extern")
                && trees
                    .get(keyword + 1)
                    .is_some_and(|next| is_ident(next, "fn") || is_ident(next, "crate"))
            {
                keyword += 1;
            } else {
                break;
            }
        }
        let head = match &trees[keyword] {
            TokenTree::Ident(ident) => ident.to_string(),
            _ => String::new(),
        };
        let (kind, brace_terminated) = match head.as_str() {
            "fn" => (RawItemKind::Fn, true),
            "impl" => (RawItemKind::Impl, true),
            "static" => (RawItemKind::Static, false),
            "const" => {
                let anonymous = trees
                    .get(keyword + 1)
                    .is_some_and(|next| is_ident(next, "_"));
                (
                    if anonymous {
                        RawItemKind::AnonymousConst
                    } else {
                        RawItemKind::NamedConst
                    },
                    false,
                )
            }
            "struct" | "enum" | "union" | "trait" | "mod" => (RawItemKind::Other, true),
            "type" | "use" | "crate" => (RawItemKind::Other, false),
            _ => {
                // A macro invocation or definition: `path ! [name] <delimited body>`.
                let bang = (keyword..trees.len())
                    .find(|at| is_punct(&trees[*at], '!'))
                    .ok_or_else(|| no_terminator(keyword))?;
                let group = (bang + 1..trees.len())
                    .find(|at| matches!(trees[*at], TokenTree::Group(_)))
                    .ok_or_else(|| no_terminator(bang))?;
                let capability_gate = visibility_tokens == 0
                    && bang == keyword + 1
                    && (head == "__recursa_if_arbitrary"
                        || head == "__recursa_if_serde"
                        || head == "__recursa_if_explorer");
                let end = if is_group(&trees[group], Delimiter::Brace) {
                    group
                } else {
                    (group + 1..trees.len())
                        .find(|at| is_punct(&trees[*at], ';'))
                        .ok_or_else(|| no_terminator(group))?
                };
                items.push(RawItem {
                    tokens: trees[start..=end].to_vec(),
                    attributes,
                    visibility_tokens,
                    visibility,
                    kind: RawItemKind::Macro {
                        capability_gate,
                        group: group - start,
                    },
                });
                start = end + 1;
                continue;
            }
        };
        let mut angle = 0i32;
        let mut end = None;
        let mut at = keyword + 1;
        while at < trees.len() {
            match &trees[at] {
                TokenTree::Punct(punct) if punct.as_char() == ';' => {
                    end = Some(at);
                    break;
                }
                TokenTree::Punct(punct)
                    if punct.as_char() == '-'
                        && punct.spacing() == proc_macro2::Spacing::Joint
                        && trees.get(at + 1).is_some_and(|next| is_punct(next, '>')) =>
                {
                    at += 2;
                    continue;
                }
                TokenTree::Punct(punct) if punct.as_char() == '<' => angle += 1,
                TokenTree::Punct(punct) if punct.as_char() == '>' => angle -= 1,
                TokenTree::Group(group)
                    if brace_terminated && angle <= 0 && group.delimiter() == Delimiter::Brace =>
                {
                    end = Some(at);
                    break;
                }
                _ => {}
            }
            at += 1;
        }
        let end = end.ok_or_else(|| no_terminator(keyword))?;
        items.push(RawItem {
            tokens: trees[start..=end].to_vec(),
            attributes,
            visibility_tokens,
            visibility,
            kind,
        });
        start = end + 1;
    }
    Ok(items)
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
pub(super) fn render_root(
    plan: &EmissionPlan,
    parsers: &ParserSet,
) -> Result<proc_macro2::TokenStream, String> {
    let lr_items = parsers
        .default_parser()
        .tables()
        .map(|lr| render_root_items(plan, lr))
        .transpose()?;
    let parser_modules = render_parser_modules(plan, parsers)?;
    let input_newtype = render_input_newtype(
        plan,
        &quote! {
            /// A cursor bound to this generated grammar's token identities.
        },
    );
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let release_text = &plan.private_abi.release;
    let release = format!("release_{release_text}").replace(['.', '-'], "_");
    let release = format_ident!("{release}");
    let private_protocol = plan.private_abi.protocol;
    let protocol = format_ident!("protocol_{private_protocol}");
    let grammar_name = &plan.grammar_name;
    let view_trivia_gaps = (!plan.trivia_kinds.is_empty()).then(|| {
        quote! {
            /// Iterates immutable source gaps retained by the complete parsed document.
            pub fn trivia_gaps(self) -> TriviaGaps<'view> {
                TriviaGaps {
                    inner: self.cursor.source_gaps()
                        .expect("this generated grammar retains trivia"),
                }
            }
        }
    });
    #[cfg(feature = "diagrams")]
    let aggregate_diagram = plan.diagrams.as_ref().map(|_| {
        quote! {
            #[cfg_attr(doc, #facade::__recursa_diagram_doc(aggregate))]
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
    let lexical_machine = render_lexical_machine(plan, facade)
        .expect("the emission plan carries a renderable lexical machine");
    // The LR runtime computes error expectations from its tables and needs only
    // the facade's width-independent empty root FOLLOW, so generated output has
    // no use for a grammar-wide FOLLOW pool.
    let token_items = plan
        .tokens
        .iter()
        .enumerate()
        .map(|(index, token)| render_token(plan, token, index, parsers));
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
    let explorer_raw_lex = plan.explorer.as_ref().map(|_| {
        let raw_lex = explorer_raw_lex_function();
        quote! {
            __recursa_if_explorer! {
                pub(super) fn #raw_lex(
                    source: &str,
                ) -> #facade::__private::LexResult<'_> {
                    lex(source).inner
                }
            }
        }
    });
    let framing_root = plan
        .framing
        .as_ref()
        .filter(|_| cfg!(feature = "spans"))
        .map(|framing| {
            let framing_root_schema = explorer_schema_static(
                plan.nodes[framing.node_index].parse_applications[framing.application_index]
                    .parse_index,
                None,
            );
            let boundary_kind = framing.boundary_kind;
            let line_kinds = &framing.line_kinds;
            let delimited_kinds = &framing.delimited_kinds;
            let mut boundary_words = vec![0_u64; plan.kind_count.div_ceil(64)];
            boundary_words[usize::from(boundary_kind) / 64] |=
                1_u64 << (usize::from(boundary_kind) % 64);
            let node = &plan.nodes[framing.node_index];
            let application = &node.parse_applications[framing.application_index];
            let payload = framing.payload.as_ref().map_or_else(
                || quote!(None),
                |payload| {
                    let variant = explorer_schema_static(
                        application.parse_index,
                        Some(payload.variant_index),
                    );
                    let terminator = &payload.terminator;
                    quote!(Some(#facade::__private::framing_payload_plan(
                        &#variant,
                        #terminator,
                    )))
                },
            );
            let framing_exports = if plan.arena_ast {
                quote! {
                    pub use #facade::framing::{
                        ArenaCompleteFrame as CompleteFrame,
                        ArenaCompleteIsland as CompleteIsland,
                        FramePart as CompletePart,
                        FrameDiagnostic, FrameDiagnosticKind, FrameError, FrameFailure,
                        FrameRejection, PayloadRegion, SourceRegion,
                    };
                }
            } else {
                quote! {
                    pub use #facade::framing::{
                        CompleteFrame, CompleteIsland, CompletePart, FrameDiagnostic,
                        FrameDiagnosticKind, FrameError, FrameFailure, FrameRejection,
                        PayloadRegion, SourceRegion,
                    };
                }
            };
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
                        &#framing_root_schema,
                        #boundary_kind,
                        &[#(#line_kinds),*],
                        &[#(#delimited_kinds),*],
                        #payload,
                        &__RECURSA_FRAMING_BOUNDARY_FOLLOW,
                    );

                #framing_exports
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

    Ok(quote! {
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

        /// A provenance-bearing view of one parsed value of this grammar.
        ///
        /// A view pairs a borrowed value with its occurrence cursor. Node-specific
        /// accessors are inherent methods on `View<'view, ThatNode>`; the shared
        /// span, element, and Pretty behaviour is implemented once here.
        pub struct View<'view, T: ?Sized> {
            pub(crate) value: &'view T,
            pub(crate) cursor: #facade::__private::ViewCursor<'view>,
        }

        impl<T: ?Sized> Clone for View<'_, T> {
            fn clone(&self) -> Self { *self }
        }

        impl<T: ?Sized> Copy for View<'_, T> {}

        impl<'view, T: ?Sized + #facade::__private::NodeElements> View<'view, T> {
            /// Active grammar elements in authored source order.
            pub fn span_elements(self) -> #facade::Elements<'view> {
                self.value.__recursa_span_elements(self.cursor)
            }

            #view_trivia_gaps
        }

        impl<'view, T: ?Sized + #facade::__private::NodeElements> #facade::NodeView
            for View<'view, T>
        {
            type Value = T;
            fn value(&self) -> &T { self.value }
            fn span_elements(&self) -> #facade::Elements<'_> {
                self.value.__recursa_span_elements(self.cursor)
            }
            fn __recursa_cursor(&self) -> #facade::__private::ViewCursor<'_> { self.cursor }
        }

        impl<'view, T> #facade::Pretty for View<'view, T>
        where
            T: ?Sized + #facade::__private::NodeElements + #facade::Pretty,
        {
            fn pretty(&self, context: &mut #facade::PrettyContext) -> ::core::fmt::Result {
                #facade::__private::pretty_view(self, context)
            }
        }

        #input_newtype

        mod __recursa_lexical {
            use super::{Input, TokenKind};

            #lexical_machine

        fn __recursa_raw_kind(kind: TokenKind) -> u16 {
            match kind {
                #(#raw_kind_arms)*
                TokenKind::LexicalError => #lexical_error_kind,
            }
        }

        pub(crate) fn __recursa_typed_kind(kind: u16) -> TokenKind {
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
            let scanned = #facade::__private::lex::<__RecursaLexToken>(
                source,
                &__RECURSA_LEX_PLAN,
            );
            let (records, trivia) = #facade::__private::lex_into_parts(scanned);
            LexResult { inner: __recursa_finalize(source, records, trivia) }
        }

        #framing_lex
        #explorer_raw_lex

        }

        pub use __recursa_lexical::{
            GrammarInfo, LexBuildError, LexBuilder, LexError, LexErrorCode, LexErrors, LexResult,
            Token, Tokens, #trivia_exports grammar_info, lex,
        };

        #[doc(hidden)]
        pub(crate) mod __recursa_content_tokens {
            #(#token_items)*
        }

        #lr_items
        #parser_modules

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
    })
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
    parsers: &ParserSet,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let generation = format_ident!("{}", plan.identity);
    let node_ident = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let mut view_generics = generics.clone();
    view_generics.params.insert(0, syn::parse_quote!('view));
    let (view_impl_generics, _, view_where_clause) = view_generics.split_for_impl();
    let view_application = quote!(#module_anchor::View<'view, #node_ident #type_generics>);
    let input_lifetime = &node.input_lifetime;
    let node_has_lifetime = generics
        .params
        .iter()
        .any(|parameter| matches!(parameter, GenericParam::Lifetime(_)));
    let inherent_method_generics = if node_has_lifetime {
        quote!(<A>)
    } else {
        quote!(<#input_lifetime, A>)
    };
    let without_spans_entry_point = if plan.arena_ast {
        quote! {
            /// Parses this Node without retaining occurrence provenance.
            pub fn parse_without_spans #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<
                #facade::ArenaParsed<
                    #input_lifetime,
                    <Self as #facade::__private::ArenaGeneratedParse<#input_lifetime, A>>::Family,
                >,
                #facade::ParseError,
            >
            where
                Self: #facade::__private::ArenaGeneratedParse<#input_lifetime, A>,
            {
                #facade::__private::parse_arena_without_spans::<Self, A>(&mut input.inner)
            }
        }
    } else {
        quote! {
            /// Parses this Node without retaining occurrence provenance.
            pub fn parse_without_spans #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError>
            where
                Self: #facade::__private::Parse<#input_lifetime, A>,
            {
                #facade::__private::parse_without_spans(&mut input.inner)
            }
        }
    };
    let entry_points = if plan.arena_ast {
        quote! {
            pub fn parse #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<
                #facade::ArenaParsed<
                    #input_lifetime,
                    <Self as #facade::__private::ArenaGeneratedParse<#input_lifetime, A>>::Family,
                >,
                #facade::ParseError,
            >
            where
                Self: #facade::__private::ArenaGeneratedParse<#input_lifetime, A>,
            {
                #facade::__private::parse_arena::<Self, A>(&mut input.inner)
            }
            pub fn parse_spanned #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<
                #facade::ArenaParsed<
                    #input_lifetime,
                    <Self as #facade::__private::ArenaGeneratedParse<#input_lifetime, A>>::Family,
                >,
                #facade::ParseError,
            >
            where
                Self: #facade::__private::ArenaGeneratedParse<#input_lifetime, A>,
            {
                #facade::__private::parse_arena_spanned::<Self, A>(&mut input.inner)
            }
            #without_spans_entry_point
        }
    } else {
        quote! {
            pub fn parse #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError>
            where
                Self: #facade::__private::Parse<#input_lifetime, A>,
            {
                #facade::__private::parse(&mut input.inner)
            }
            pub fn parse_spanned #inherent_method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> Result<#facade::Parsed<#input_lifetime, Self>, #facade::ParseError>
            where
                Self: #facade::__private::Parse<#input_lifetime, A>,
            {
                #facade::__private::parse_spanned(&mut input.inner)
            }
            #without_spans_entry_point
        }
    };
    let metadata = render_metadata(plan, node, node_index);
    let accessors = render_accessors(plan, node);
    let span_elements = render_span_elements(node, node_index);
    let parse_implementations = node
        .parse_applications
        .iter()
        .flat_map(|application| parsers.iter().map(move |parser| (application, parser)))
        .map(|(application, parser)| {
            // A closed application can borrow the parser input through one of its
            // type arguments even when the node family itself only declares type
            // parameters.  For example, `Atom<T>` is materialized as
            // `Atom<IdentAtom<'__recursa_input>>`.  The LR reducer helper is an
            // inherent impl on that concrete application, so its impl must bind
            // the materialization lifetime.  Keep the authored node generics
            // unchanged for every other generated capability, and add the
            // synthetic lifetime only to the parse renderer's private view.
            let parse_node = parse_node_with_application_lifetime(node, &application.ty);
            render_node_application(
                plan,
                &parse_node,
                application,
                parser
                    .tables()
                    .expect("the sole parser always has LR tables"),
                parser,
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
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
    let diagram_attribute = plan
        .diagrams
        .as_ref()
        .map(|_| quote!(#[cfg_attr(doc, #facade::__recursa_diagram_doc(node = #node_index))]));
    #[cfg(not(feature = "diagrams"))]
    let diagram_attribute = proc_macro2::TokenStream::new();
    let view_accessors = (!accessors.is_empty()).then(|| {
        quote! {
            impl #view_impl_generics #view_application #view_where_clause {
                #accessors
            }
        }
    });

    Ok(quote! {
        __recursa_generation_handshake!(#generation);

        #(#token_bindings)*

        #metadata

        #view_accessors

        impl #impl_generics #facade::__private::NodeElements for #node_ident #type_generics
            #where_clause
        {
            fn __recursa_span_elements<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> #facade::Elements<'view> {
                #span_elements
            }
        }

        impl #impl_generics #facade::__private::NodeViewFactory for #node_ident #type_generics
            #where_clause
        {
            type View<'view> = #view_application where Self: 'view;
            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> #view_application {
                #module_anchor::View { value: self, cursor }
            }
        }

        #(#parse_implementations)*

        #configured_traits
        #explorer

        #diagram_attribute
        impl #impl_generics #node_ident #type_generics #where_clause {
            #entry_points
        }
    })
}

/// Gives the table-driven renderer a lifetime parameter when a closed application
/// mentions the generated input lifetime through a nested type argument.
///
/// This is deliberately a rendering-only adjustment: `NodeParsePlan::generics`
/// describes the authored declaration and must not acquire a parameter that the
/// source Node does not declare.  The adjustment is needed for inherent reducer
/// impls, whose self type is the *materialized application* rather than the open
/// node family.
fn parse_node_with_application_lifetime(
    node: &NodeParsePlan,
    application_ty: &Type,
) -> NodeParsePlan {
    if node.generics.lifetimes().next().is_some() {
        return node.clone();
    }

    struct InputLifetimeUse {
        found: bool,
    }

    impl<'ast> syn::visit::Visit<'ast> for InputLifetimeUse {
        fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
            if lifetime.ident == "__recursa_input" {
                self.found = true;
            }
        }
    }

    let mut input_lifetime = InputLifetimeUse { found: false };
    syn::visit::Visit::visit_type(&mut input_lifetime, application_ty);
    if !input_lifetime.found {
        return node.clone();
    }

    let mut parse_node = node.clone();
    parse_node
        .generics
        .params
        .insert(0, syn::parse_quote!('__recursa_input));
    parse_node
}
