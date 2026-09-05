//! Generated lexing plans, token types, bindings, and lexical rendering.
//!
//! Token and rule data remain adjacent to emission so grammar-local token kinds,
//! classifiers and exact matched text share one review boundary.

use super::*;

#[derive(Clone)]
/// Runtime token type, capture, and classifier contract.
pub(super) struct TokenParsePlan {
    /// Semantic content-token identity represented by the generated type.
    pub(super) semantic: ContentTokenId,
    /// Generated Rust identifier visible at the binding site.
    pub(super) ident: Ident,
    /// Grammar-local numeric kind emitted for lexical matches.
    pub(super) kind: u16,
    /// Typed token-kind variant naming the numeric kind.
    pub(super) kind_variant: Ident,
    /// Reclaimable fixed kinds accepted as this content token.
    pub(super) accepted_kind_variants: Vec<Ident>,
    /// Diagnostic atom describing this token at parse failures.
    pub(super) expected: ExpectedSet,
    /// Whether values retain exact matched source text.
    pub(super) capture: PlannedCapturePolicy,
    /// Deterministic synthesized spelling used by generated `Arbitrary`.
    pub(super) arbitrary_spelling: String,
    /// Effective optional recipes in addition to intrinsic structural value traits.
    pub(super) configured_traits: Vec<ConfiguredTraitRecipe>,
}

#[derive(Clone)]
/// Generated binding and identity for one authored token declaration.
pub(super) struct TokenBindingPlan {
    /// Source module receiving the generated binding.
    pub(super) module: ModuleId,
    /// Stable source identity used to correlate the binding.
    pub(super) semantic_key: String,
    /// Rust identifier emitted into the source module.
    pub(super) ident: Ident,
    /// Authored visibility preserved by the generated binding.
    pub(super) visibility: Visibility,
    /// Whether the binding and every containing module are publicly reachable.
    pub(super) publicly_materialized: bool,
    /// Index of the semantic content-token plan this binding exposes.
    pub(super) token: usize,
}
#[derive(Clone)]
/// Ordered runtime matching rule for one token pattern.
pub(super) struct LexRulePlan {
    /// Backend-independent lexical pattern.
    pub(super) pattern: String,
    /// Exact authored spelling when this rule recognizes fixed syntax.
    pub(super) spelling: Option<String>,
    /// Whether a fixed spelling matches under ASCII-case equivalence.
    pub(super) ascii_insensitive: bool,
    /// Closed scanning mechanism extending or rejecting the starter pattern.
    pub(super) closed_matcher: Option<ClosedMatcher>,
    /// Numeric token kind produced by a successful match.
    pub(super) kind: u16,
    /// Whether matching text is consumed without a token record.
    pub(super) skip: bool,
    /// Grammar-local trivia classification retained outside significant parsing.
    pub(super) trivia_kind: Option<u16>,
    /// Whether this rule recognizes exact fixed syntax.
    pub(super) fixed: bool,
    /// Stable declaration-derived tie-break priority.
    pub(super) priority: u32,
}

#[derive(Clone)]
/// Exact keyword spelling mapped to its runtime kind.
pub(super) struct KeywordClassifierPlan {
    /// Normalized word-shaped fixed spelling used for classification.
    pub(super) canonical_spelling: String,
    /// Fixed-token kind assigned after carrier recognition.
    pub(super) kind: u16,
    /// Content-token kinds proven able to carry this word.
    pub(super) carriers: Vec<u16>,
    /// Whether classification uses ASCII-case-insensitive equivalence.
    pub(super) ascii_insensitive: bool,
}

#[derive(Clone)]
/// Word-like token kinds and their keyword refinement policy.
pub(super) struct WordClassifierPlan {
    /// Normalized fixed spelling tested by the generated classifier.
    pub(super) canonical_spelling: String,
    /// Fixed-token kind substituted on an equivalent match.
    pub(super) kind: u16,
    /// Whether the classifier folds ASCII case before comparison.
    pub(super) ascii_insensitive: bool,
}

/// Escapes one fixed spelling into an anchored lexer pattern.
pub(super) fn fixed_lexical_pattern(spelling: &str, equivalence: TextEquivalence) -> String {
    let word_shaped = is_word_shaped(spelling);
    if equivalence != TextEquivalence::AsciiInsensitive || !word_shaped {
        return regex_syntax::escape(spelling);
    }
    spelling
        .chars()
        .map(|character| {
            if character.is_ascii_alphabetic() {
                format!(
                    "[{}{}]",
                    character.to_ascii_lowercase(),
                    character.to_ascii_uppercase()
                )
            } else {
                regex_syntax::escape(&character.to_string())
            }
        })
        .collect()
}

/// Reports whether a fixed spelling has the grammar's Unicode identifier shape.
pub(super) fn is_word_shaped(spelling: &str) -> bool {
    let mut characters = spelling.chars();
    characters.next().is_some_and(|first| {
        (first == '_' || unicode_ident::is_xid_start(first))
            && characters.all(unicode_ident::is_xid_continue)
    })
}

/// Selects the canonical spelling emitted for synthesized fixed syntax.
pub(super) fn fixed_pretty_spelling(spelling: &str, equivalence: TextEquivalence) -> String {
    if equivalence == TextEquivalence::AsciiInsensitive && is_word_shaped(spelling) {
        spelling.to_ascii_uppercase()
    } else {
        spelling.to_owned()
    }
}

/// Produces a deterministic witness proving a lexical pattern can match.
pub(super) fn lexical_pattern_witness(pattern: &str) -> Result<String, String> {
    use regex_syntax::hir::{Class, Hir, HirKind};

    fn render(hir: &Hir) -> Option<String> {
        match hir.kind() {
            HirKind::Empty | HirKind::Look(_) => Some(String::new()),
            HirKind::Literal(literal) => String::from_utf8(literal.0.to_vec()).ok(),
            HirKind::Class(Class::Unicode(class)) => {
                class.iter().next().map(|range| range.start().to_string())
            }
            HirKind::Class(Class::Bytes(class)) => class.iter().find_map(|range| {
                (range.start()..=range.end())
                    .find(|byte| byte.is_ascii())
                    .map(|byte| char::from(byte).to_string())
            }),
            HirKind::Repetition(repetition) => {
                let item = render(&repetition.sub)?;
                Some(item.repeat(repetition.min.try_into().ok()?))
            }
            HirKind::Capture(capture) => render(&capture.sub),
            HirKind::Concat(parts) => {
                let rendered = parts.iter().map(render).collect::<Option<Vec<_>>>()?;
                Some(rendered.concat())
            }
            HirKind::Alternation(parts) => parts
                .iter()
                .filter_map(render)
                .min_by_key(|candidate| candidate.len()),
        }
    }

    let hir = regex_syntax::Parser::new()
        .parse(pattern)
        .map_err(|error| {
            format!("validated lexical pattern `{pattern}` no longer parses: {error}")
        })?;
    let witness = render(&hir)
        .ok_or_else(|| format!("lexical pattern `{pattern}` has no UTF-8 synthetic witness"))?;
    if witness.is_empty() {
        return Err(format!(
            "nonempty lexical pattern `{pattern}` produced an empty synthetic witness"
        ));
    }
    Ok(witness)
}
/// Renders the grammar-local lexer and typed records.
///
/// # Long-function contract
///
/// Rationale: emits the grammar-bound lexer and token API from one frozen lexical plan.
/// Invariants: priority, longest-match behavior, classification, and exact matched text remain plan-driven.
/// Phases: declares kinds and records, finalizes records, then exposes lexing.
pub(super) fn render_token(
    plan: &EmissionPlan,
    token: &TokenParsePlan,
    index: usize,
    lr: Option<&TableDrivenRendering>,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let ident = &token.ident;
    let kind_variant = &token.kind_variant;
    let stores_kind = token.accepted_kind_variants.len() > 1;
    let accepted_kind_variants = &token.accepted_kind_variants;
    let kind_field = stores_kind.then(|| quote!(kind: #module_anchor::TokenKind,));
    let new_kind = stores_kind.then(|| quote!(kind: #module_anchor::TokenKind::#kind_variant,));
    let with_kind = stores_kind.then(|| {
        quote! {
            pub fn with_kind(
                kind: #module_anchor::TokenKind,
                text: impl Into<Box<str>>,
            ) -> Result<Self, #module_anchor::TokenKind> {
                match kind {
                    #(#module_anchor::TokenKind::#accepted_kind_variants)|* => Ok(Self {
                        text: #facade::__private::ContentText::owned(text.into()),
                        kind,
                    }),
                    rejected => Err(rejected),
                }
            }
        }
    });
    let kind_method = if stores_kind {
        quote! {
            pub const fn kind(&self) -> #module_anchor::TokenKind { self.kind }
        }
    } else {
        quote! {
            pub const fn kind(&self) -> #module_anchor::TokenKind {
                #module_anchor::TokenKind::#kind_variant
            }
        }
    };
    // The expectation guard in the generated parse method admits only the accepted kinds,
    // so the grammar-wide `__recursa_typed_kind` table yields exactly the accepted variant.
    // Sharing that table avoids one kind-to-variant match per content-token type.
    let parsed_kind = stores_kind.then(
        || quote!(kind: #module_anchor::__recursa_lexical::__recursa_typed_kind(record.kind),),
    );
    let expected = render_expected_set(plan, &token.expected);
    let capture = token.capture.render(facade);
    let parse_impl = match lr {
        Some(lr) => render_token_items(plan, token, lr),
        None => quote! {
            impl<'input, A> #facade::__private::Parse<'input, A, #facade::__private::RecursiveDescent>
                for #ident<'input>
            {
                const CAPTURE_POLICY: #facade::__private::CapturePolicy = #capture;
                fn grammar() -> &'static #facade::__private::GrammarInfo {
                    &#module_anchor::__RECURSA_GRAMMAR
                }
                fn root_follow() -> &'static #facade::__private::FollowSet {
                    &#module_anchor::__RECURSA_EMPTY_FOLLOW
                }
                fn parse(
                    context: &mut #facade::__private::ParseContext<'_, 'input, A>,
                ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                    let Some(record) = context.peek(0) else {
                        return Err(context.unexpected(&#expected));
                    };
                    if !#expected.contains(record.kind) {
                        return Err(context.unexpected(&#expected));
                    }
                    context.advance();
                    let text = context.matched_text(record)?;
                    context.finish(Self { text, #parsed_kind }, record.span)
                }
            }
        },
    };
    let mut standard_derives = token
        .configured_traits
        .iter()
        .filter_map(|recipe| match recipe {
            ConfiguredTraitRecipe::Debug => Some(quote!(Debug)),
            ConfiguredTraitRecipe::Clone => Some(quote!(Clone)),
            ConfiguredTraitRecipe::Copy => Some(quote!(Copy)),
            ConfiguredTraitRecipe::PartialEq => Some(quote!(PartialEq)),
            ConfiguredTraitRecipe::Eq => Some(quote!(Eq)),
            ConfiguredTraitRecipe::PartialOrd => Some(quote!(PartialOrd)),
            ConfiguredTraitRecipe::Ord => Some(quote!(Ord)),
            ConfiguredTraitRecipe::Hash => Some(quote!(Hash)),
            _ => None,
        })
        .collect::<Vec<_>>();
    // Authored Nodes may use ordinary Rust derives independently of Recursa's configured-trait
    // plan. Generated token types are string-like values, so keep their structurally valid value
    // traits available to those downstream derives without requiring a grammar-wide recipe.
    for recipe in INTRINSIC_CONTENT_TOKEN_VALUE_TRAITS {
        if !token.configured_traits.contains(&recipe) {
            standard_derives.push(match recipe {
                ConfiguredTraitRecipe::Debug => quote!(Debug),
                ConfiguredTraitRecipe::Clone => quote!(Clone),
                ConfiguredTraitRecipe::PartialEq => quote!(PartialEq),
                ConfiguredTraitRecipe::Eq => quote!(Eq),
                ConfiguredTraitRecipe::PartialOrd => quote!(PartialOrd),
                ConfiguredTraitRecipe::Ord => quote!(Ord),
                ConfiguredTraitRecipe::Hash => quote!(Hash),
                _ => unreachable!("the intrinsic content-token trait catalog is structural"),
            });
        }
    }
    let standard_derive = quote!(#[derive(#(#standard_derives),*)]);
    let visit_event = quote!(#module_anchor::__RecursaVisitEvent);
    let visit_event_mut = quote!(#module_anchor::__RecursaVisitEventMut);
    let visit_events = quote!(#module_anchor::__RecursaVisitEvents);
    let visit_events_mut = quote!(#module_anchor::__RecursaVisitEventsMut);
    let visit_variant = typed_visit_token_binding(plan, index)
        .map(|binding| visit_variant_ident(plan, &binding.semantic_key));
    let enter_event = visit_variant.as_ref().map_or_else(
        || quote!(#visit_event::Opaque(self, ::core::marker::PhantomData)),
        |variant| quote!(#visit_event::#variant(self)),
    );
    let exit_event = enter_event.clone();
    let enter_event_mut = visit_variant.as_ref().map_or_else(
        || quote!(#visit_event_mut::Opaque(self, ::core::marker::PhantomData)),
        |variant| quote!(#visit_event_mut::#variant(self)),
    );
    let exit_event_mut = enter_event_mut.clone();
    let visit_with_impl = visit_family_enabled(plan, false).then(|| {
        quote! {
            impl<'input> #module_anchor::__RecursaVisitWith<'input> for #ident<'input> {
                fn __recursa_visit_with<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<#visit_events<'input>>,
                {
                    match visitor.total_enter(#enter_event) {
                        ::core::ops::ControlFlow::Continue(())
                        | ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                        other => return other,
                    }
                    visitor.total_exit(#exit_event)
                }
            }
        }
    });
    let visit_mut_with_impl = visit_family_enabled(plan, true).then(|| {
        quote! {
            impl<'input> #module_anchor::__RecursaVisitMutWith<'input> for #ident<'input> {
                fn __recursa_visit_mut_with<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<#visit_events_mut<'input>>,
                {
                    match visitor.total_enter_mut(#enter_event_mut) {
                        ::core::ops::ControlFlow::Continue(())
                        | ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                        other => return other,
                    }
                    visitor.total_exit_mut(#exit_event_mut)
                }
            }
        }
    });
    let visit_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Visit)
        .then(|| {
            quote! {
                impl<'input> #facade::Visit for #ident<'input> {
                    type Events = #visit_events<'input>;

                    fn visit<V>(
                        &self,
                        visitor: &mut V,
                    ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                    where
                        V: #facade::TotalVisitor<Self::Events>,
                    {
                        match visitor.total_enter(#enter_event) {
                            ::core::ops::ControlFlow::Continue(())
                            | ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                            other => return other,
                        }
                        visitor.total_exit(#exit_event)
                    }
                }
            }
        });
    let visit_mut_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::VisitMut)
        .then(|| {
            quote! {
                impl<'input> #facade::VisitMut for #ident<'input> {
                    type EventsMut = #visit_events_mut<'input>;

                    fn visit_mut<V>(
                        &mut self,
                        visitor: &mut V,
                    ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                    where
                        V: #facade::TotalVisitorMut<Self::EventsMut>,
                    {
                        match visitor.total_enter_mut(#enter_event_mut) {
                            ::core::ops::ControlFlow::Continue(())
                            | ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                            other => return other,
                        }
                        visitor.total_exit_mut(#exit_event_mut)
                    }
                }
            }
        });
    let pretty_body = |behavior| match behavior {
        PrettyTokenBehavior::MatchedOrStoredText => quote! {
            let matched = context.matched_text();
            context.token(matched.as_deref().unwrap_or_else(|| self.text()))
        },
    };
    let pretty_with_impl = content_token_pretty_helper_enabled(plan).then(|| {
        let body = pretty_body(PrettyTokenBehavior::MatchedOrStoredText);
        quote! {
            impl<'input> #module_anchor::__RecursaPrettyWith for #ident<'input> {
                fn __recursa_pretty_with(
                    &self,
                    context: &mut #facade::PrettyContext,
                ) -> ::core::fmt::Result {
                    #body
                }
            }
        }
    });
    let pretty_impl = plan.pretty.tokens[index].map(|behavior| {
        let max_width = plan.pretty_max_width;
        let indent = plan.pretty_indent;
        let body = pretty_body(behavior);
        quote! {
            impl<'input> #facade::Pretty for #ident<'input> {
                fn pretty(
                    &self,
                    context: &mut #facade::PrettyContext,
                ) -> ::core::fmt::Result {
                    context.configure_defaults(#max_width, #indent);
                    #body
                }
            }

        }
    });
    let arbitrary_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Arbitrary)
        .then(|| {
            let spelling = &token.arbitrary_spelling;
            quote! {
                __recursa_if_arbitrary! {
                impl<'arbitrary, 'input> #facade::__private::arbitrary::Arbitrary<'arbitrary>
                    for #ident<'input>
                {
                    fn arbitrary(
                        input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    ) -> #facade::__private::arbitrary::Result<Self> {
                        let _ = input;
                        Ok(Self::new(#spelling))
                    }
                }
                }
            }
        });
    let arbitrary_with_impl =
        content_token_support_required(plan, token.semantic, ConfiguredTraitRecipe::Arbitrary)
            .then(|| {
                let spelling = &token.arbitrary_spelling;
                quote! {
                    __recursa_if_arbitrary! {
                        impl<'arbitrary, 'input> #module_anchor::__RecursaArbitraryWith<'arbitrary>
                            for #ident<'input>
                        {
                            fn __recursa_arbitrary_with(
                                input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                            ) -> #facade::__private::arbitrary::Result<Self> {
                                let _ = input;
                                Ok(Self::new(#spelling))
                            }
                        }
                    }
                }
            });
    let serialize_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Serialize)
        .then(|| {
            quote! {
                __recursa_if_serde! {
                    impl<'input> #facade::__private::serde::Serialize for #ident<'input> {
                        fn serialize<S: #facade::__private::serde::Serializer>(
                            &self,
                            serializer: S,
                        ) -> Result<S::Ok, S::Error> {
                            serializer.serialize_str(self.text())
                        }
                    }
                }
            }
        });
    let deserialize_impl = token
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Deserialize)
        .then(|| {
            quote! {
                __recursa_if_serde! {
                    impl<'de, 'input> #facade::__private::serde::Deserialize<'de> for #ident<'input> {
                        fn deserialize<D: #facade::__private::serde::Deserializer<'de>>(
                            deserializer: D,
                        ) -> Result<Self, D::Error> {
                            let text = <String as #facade::__private::serde::Deserialize>::deserialize(deserializer)?;
                            Ok(Self::new(text))
                        }
                    }
                }
            }
        });
    let serialize_with_impl =
        content_token_support_required(plan, token.semantic, ConfiguredTraitRecipe::Serialize)
            .then(|| {
                quote! {
                    __recursa_if_serde! {
                        impl<'input> #module_anchor::__RecursaSerializeWith for #ident<'input> {
                            fn __recursa_serialize_with<S>(
                                &self,
                                serializer: S,
                            ) -> Result<S::Ok, S::Error>
                            where
                                S: #facade::__private::serde::Serializer,
                            {
                                serializer.serialize_str(self.text())
                            }
                        }
                    }
                }
            });
    let deserialize_with_impl = content_token_support_required(
        plan,
        token.semantic,
        ConfiguredTraitRecipe::Deserialize,
    )
    .then(|| {
        quote! {
            __recursa_if_serde! {
                impl<'de, 'input> #module_anchor::__RecursaDeserializeWith<'de> for #ident<'input> {
                    fn __recursa_deserialize_with<D>(
                        deserializer: D,
                    ) -> Result<Self, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>,
                    {
                        let text = <String as #facade::__private::serde::Deserialize>::deserialize(deserializer)?;
                        Ok(Self::new(text))
                    }
                }
            }
        }
    });
    let explorer_impl = plan
        .explorer
        .as_ref()
        .map(|_| render_token_explorer(plan, token));
    quote! {
        #[doc(hidden)]
        #standard_derive
        pub struct #ident<'input> {
            text: #facade::__private::ContentText<'input>,
            #kind_field
        }

        impl<'input> #ident<'input> {
            pub fn new(text: impl Into<Box<str>>) -> Self {
                Self {
                    text: #facade::__private::ContentText::owned(text.into()),
                    #new_kind
                }
            }

            #with_kind
            pub fn text(&self) -> &str { self.text.as_str() }
            #kind_method
        }

        impl<'input> #facade::__private::NodeElements for #ident<'input> {}
        impl<'input> #facade::__private::NodeViewFactory for #ident<'input> {
            type View<'view> = #module_anchor::View<'view, #ident<'input>> where Self: 'view;
            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> #module_anchor::View<'view, #ident<'input>> {
                #module_anchor::View { value: self, cursor }
            }
        }
        #parse_impl

        #visit_with_impl
        #visit_mut_with_impl
        #visit_impl
        #visit_mut_impl
        #pretty_with_impl
        #pretty_impl
        #arbitrary_impl
        #arbitrary_with_impl
        #serialize_impl
        #deserialize_impl
        #serialize_with_impl
        #deserialize_with_impl
        #explorer_impl
    }
}
/// Renders one source-visible content-token type binding with authored visibility.
pub(super) fn render_token_binding(
    plan: &EmissionPlan,
    binding: &TokenBindingPlan,
) -> proc_macro2::TokenStream {
    let module_anchor = &plan.module_anchor;
    let visibility = &binding.visibility;
    let ident = &binding.ident;
    let canonical = &plan.tokens[binding.token].ident;
    let token_documentation = syn::LitStr::new(
        "A content-bearing token. Parsed values retain their exact matched input text; synthesized values own their text. Fixed tokens are syntax-only grammar elements: they shape parsing but store no value and have no generated token type.",
        proc_macro2::Span::call_site(),
    );
    #[cfg(feature = "diagrams")]
    let token_diagram_attribute = plan.diagrams.as_ref().map(|_| {
        let facade = &plan.facade;
        let token_index = binding.token;
        quote!(#[cfg_attr(doc, #facade::__recursa_diagram_doc(token = #token_index))])
    });
    #[cfg(not(feature = "diagrams"))]
    let token_diagram_attribute = proc_macro2::TokenStream::new();
    quote! {
        #[doc = #token_documentation]
        #token_diagram_attribute
        #visibility type #ident<'input> =
            #module_anchor::__recursa_content_tokens::#canonical<'input>;
    }
}

/// Packs one byte alphabet into the runtime's 256-bit membership words.
fn render_byte_set(bytes: impl IntoIterator<Item = u8>, facade: &Ident) -> proc_macro2::TokenStream {
    let mut words = [0_u64; 4];
    for byte in bytes {
        words[usize::from(byte >> 6)] |= 1_u64 << (byte & 63);
    }
    let words = words.iter().copied();
    quote!(#facade::__private::ByteSet([#(#words),*]))
}

/// Projects one next-exclusion predicate into the scalar ranges it accepts.
///
/// The runtime rejects a candidate whose following scalar is matched in full by
/// the authored predicate. Only single-scalar predicates express that, so the
/// projection is exact and generation refuses anything else.
fn exclusion_scalar_ranges(pattern: &str) -> Result<Vec<(char, char)>, String> {
    use regex_syntax::hir::{Class, Hir, HirKind};

    fn collect(hir: &Hir, ranges: &mut Vec<(char, char)>) -> Result<(), String> {
        match hir.kind() {
            HirKind::Class(Class::Unicode(class)) => {
                ranges.extend(class.iter().map(|range| (range.start(), range.end())));
                Ok(())
            }
            HirKind::Class(Class::Bytes(class)) => class.iter().try_for_each(|range| {
                let (start, end) = (range.start(), range.end());
                if end > 0x7f {
                    return Err(
                        "a next-exclusion predicate must match complete scalars".to_owned()
                    );
                }
                ranges.push((char::from(start), char::from(end)));
                Ok(())
            }),
            HirKind::Literal(literal) => {
                let text = std::str::from_utf8(&literal.0)
                    .map_err(|_| "a next-exclusion predicate must be valid UTF-8".to_owned())?;
                let mut scalars = text.chars();
                let (Some(scalar), None) = (scalars.next(), scalars.next()) else {
                    return Err(
                        "a next-exclusion predicate must match exactly one scalar".to_owned()
                    );
                };
                ranges.push((scalar, scalar));
                Ok(())
            }
            HirKind::Capture(capture) => collect(&capture.sub, ranges),
            HirKind::Concat(parts) if parts.len() == 1 => collect(&parts[0], ranges),
            HirKind::Alternation(parts) => {
                parts.iter().try_for_each(|part| collect(part, ranges))
            }
            _ => Err("a next-exclusion predicate must match exactly one scalar".to_owned()),
        }
    }

    let hir = regex_syntax::Parser::new()
        .parse(pattern)
        .map_err(|_| "a next-exclusion predicate must be a valid pattern".to_owned())?;
    let mut ranges = Vec::new();
    collect(&hir, &mut ranges)?;
    Ok(ranges)
}

/// Renders one rule's runtime outcome record.
fn render_lex_rule(rule: &LexRulePlan, rank: u32, facade: &Ident) -> proc_macro2::TokenStream {
    let kind = rule.kind;
    let skip = rule.skip;
    let trivia_kind = match rule.trivia_kind {
        Some(kind) => quote!(Some(#kind)),
        None => quote!(None),
    };
    let fixed = rule.fixed;
    quote!(#facade::__private::LexRule {
        kind: #kind,
        skip: #skip,
        trivia_kind: #trivia_kind,
        fixed: #fixed,
        rank: #rank,
    })
}

/// Reports whether a matcher's final extent can exceed its opening pattern.
///
/// A closed region arbitrates on the extent its callback reaches, which the
/// opening pattern alone cannot express, so those rules run in their own
/// machine.
fn is_closed_region(matcher: Option<&ClosedMatcher>) -> bool {
    matches!(
        matcher,
        Some(
            ClosedMatcher::SameDelimiter { .. }
                | ClosedMatcher::Nested { .. }
                | ClosedMatcher::PhysicalLine { .. }
        )
    )
}

/// Collects every byte that can begin a match of `pattern`.
fn pattern_first_bytes(pattern: &str) -> Result<Vec<u8>, String> {
    use regex_automata::dfa::Automaton as _;
    use regex_automata::dfa::{StartKind, dense};
    use regex_automata::{Anchored, Input};

    let dfa = dense::Builder::new()
        .configure(dense::DFA::config().start_kind(StartKind::Anchored))
        .build(pattern)
        .map_err(|_| "a lexical opener must be a valid pattern".to_owned())?;
    let start = dfa
        .start_state_forward(&Input::new(b"").anchored(Anchored::Yes))
        .map_err(|_| "a lexical opener must have an anchored start".to_owned())?;
    Ok((0..=u8::MAX)
        .filter(|&byte| !dfa.is_dead_state(dfa.next_state(start, byte)))
        .collect())
}

/// Renders one state-machine variant's pattern attribute.
///
/// Every emitted regular pattern carries `allow_greedy = true`. Source analysis
/// is the authority on which patterns a grammar may declare, and the runtime it
/// replaces tried every pattern at every offset, so the state machine must not
/// reject a pattern that the grammar already accepted.
fn render_machine_attribute(
    rule: &LexRulePlan,
    rank: u32,
    facade: &Ident,
) -> Result<proc_macro2::TokenStream, String> {
    let priority = proc_macro2::Literal::usize_unsuffixed(rank as usize);
    Ok(match rule.closed_matcher.as_ref() {
        // A case-insensitive fixed spelling keeps the ASCII-folded pattern source
        // analysis already produced. The state machine's own case flag folds
        // Unicode, which is a different equivalence.
        None => match rule.spelling.as_deref() {
            Some(spelling) if rule.fixed && !rule.ascii_insensitive => {
                quote!(#[token(#spelling, priority = #priority)])
            }
            _ => {
                let pattern = &rule.pattern;
                quote!(#[regex(#pattern, allow_greedy = true, priority = #priority)])
            }
        },
        Some(ClosedMatcher::SameDelimiter { opener }) => quote!(
            #[regex(
                #opener,
                callback = #facade::__private::lex_same_delimiter,
                allow_greedy = true,
                priority = #priority
            )]
        ),
        Some(ClosedMatcher::Nested { opener, closer }) => quote!(
            #[token(
                #opener,
                callback = |__recursa_lexer| #facade::__private::lex_nested(
                    __recursa_lexer,
                    #opener,
                    #closer,
                ),
                priority = #priority
            )]
        ),
        Some(ClosedMatcher::NextExclusion { pattern, excluded }) => {
            let ranges = exclusion_scalar_ranges(excluded)?;
            let starts = ranges.iter().map(|&(start, _)| start);
            let ends = ranges.iter().map(|&(_, end)| end);
            quote!(
                #[regex(
                    #pattern,
                    callback = |__recursa_lexer| #facade::__private::lex_next_exclusion(
                        __recursa_lexer,
                        &[#((#starts, #ends)),*],
                    ),
                    allow_greedy = true,
                    priority = #priority
                )]
            )
        }
        Some(ClosedMatcher::PhysicalLine { prefix }) => quote!(
            #[regex(
                #prefix,
                callback = #facade::__private::lex_physical_line,
                allow_greedy = true,
                priority = #priority
            )]
        ),
        Some(ClosedMatcher::OperatorRun { .. }) => {
            return Err("an operator run is arbitrated outside the state machine".to_owned());
        }
    })
}

/// Renders the private lexical state machine, its rule tables, and its plan.
///
/// # Long-function contract
///
/// Rationale: the state machine, its rule table, and its plan are one unit whose
/// indexes must agree exactly.
/// Invariants: variant order equals rule-table order; every rank is unique and
/// reproduces the runtime's fixed-then-priority-then-declaration ordering.
/// Phases: ranks each rule, partitions runs from machine rules, then renders the
/// enum, the index projection, and the three static tables.
pub(super) fn render_lexical_machine(
    plan: &EmissionPlan,
    facade: &Ident,
) -> Result<proc_macro2::TokenStream, String> {
    let rules = &plan.lexical_rules;
    let mut order = (0..rules.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| (rules[index].fixed, rules[index].priority, index));
    let mut ranks = vec![0_u32; rules.len()];
    for (position, &index) in order.iter().enumerate() {
        ranks[index] = u32::try_from(position + 1)
            .map_err(|_| "generated lexical rank exceeds runtime width".to_owned())?;
    }

    let machine_indexes = (0..rules.len())
        .filter(|&index| {
            !matches!(
                rules[index].closed_matcher,
                Some(ClosedMatcher::OperatorRun { .. })
            ) && !is_closed_region(rules[index].closed_matcher.as_ref())
        })
        .collect::<Vec<_>>();
    let run_indexes = (0..rules.len())
        .filter(|&index| {
            matches!(
                rules[index].closed_matcher,
                Some(ClosedMatcher::OperatorRun { .. })
            )
        })
        .collect::<Vec<_>>();
    let closed_indexes = (0..rules.len())
        .filter(|&index| is_closed_region(rules[index].closed_matcher.as_ref()))
        .collect::<Vec<_>>();
    if machine_indexes.is_empty() {
        return Err("a generated grammar needs one state-machine lexical rule".to_owned());
    }

    let render_machine = |indexes: &[usize], prefix: &str| {
        let variants = indexes
            .iter()
            .enumerate()
            .map(|(position, &index)| {
                let ident = format_ident!("{prefix}{position}");
                let attribute = render_machine_attribute(&rules[index], ranks[index], facade)?;
                Ok(quote! {
                    #attribute
                    #ident,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let arms = (0..indexes.len()).map(|position| {
            let ident = format_ident!("{prefix}{position}");
            quote!(Self::#ident => #position,)
        });
        let records = indexes
            .iter()
            .map(|&index| render_lex_rule(&rules[index], ranks[index], facade));
        Ok::<_, String>((
            variants,
            arms.collect::<Vec<_>>(),
            records.collect::<Vec<_>>(),
        ))
    };

    let (variants, index_arms, machine_rules) = render_machine(&machine_indexes, "__RecursaLex")?;
    let machine_rule_count = machine_indexes.len();
    let (closed_variants, closed_index_arms, closed_rules) =
        render_machine(&closed_indexes, "__RecursaClosedLex")?;
    let closed_rule_count = closed_indexes.len();
    let mut closed_first_bytes = Vec::new();
    for &index in &closed_indexes {
        let opener = match rules[index].closed_matcher.as_ref() {
            Some(ClosedMatcher::SameDelimiter { opener }) => opener.clone(),
            Some(ClosedMatcher::Nested { opener, .. }) => regex_syntax::escape(opener),
            Some(ClosedMatcher::PhysicalLine { prefix }) => prefix.clone(),
            _ => unreachable!("closed indexes were partitioned by their closed matcher"),
        };
        closed_first_bytes.extend(pattern_first_bytes(&opener)?);
    }
    let closed_first_bytes = render_byte_set(closed_first_bytes, facade);
    let closed_machine = (!closed_indexes.is_empty()).then(|| {
        quote! {
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            #[derive(#facade::__private::logos::Logos)]
            #[logos(crate = #facade::__private::logos)]
            #[logos(extras = #facade::__private::LexExtras)]
            enum __RecursaClosedLexToken {
                #(#closed_variants)*
            }

            impl #facade::__private::LexTokenIndex for __RecursaClosedLexToken {
                fn rule_index(&self) -> usize {
                    match self {
                        #(#closed_index_arms)*
                    }
                }
            }

            static __RECURSA_CLOSED_LEX_RULES: [#facade::__private::LexRule; #closed_rule_count] =
                [#(#closed_rules),*];

            fn __recursa_scan_closed(
                source: &str,
            ) -> Option<#facade::__private::LexClosedMatch> {
                #facade::__private::scan_closed::<__RecursaClosedLexToken>(
                    source,
                    &__RECURSA_CLOSED_LEX_RULES,
                )
            }
        }
    });
    let closed_scan = if closed_indexes.is_empty() {
        quote!(None)
    } else {
        quote!(Some(__recursa_scan_closed))
    };

    let operator_runs = run_indexes
        .iter()
        .map(|&index| {
            let Some(ClosedMatcher::OperatorRun {
                characters,
                fences,
                trailing,
                qualifying,
            }) = rules[index].closed_matcher.as_ref()
            else {
                unreachable!("run indexes were partitioned by their closed matcher");
            };
            let admitted = render_byte_set(characters.bytes(), facade);
            let fence_starts = render_byte_set(
                fences.iter().filter_map(|fence| fence.bytes().next()),
                facade,
            );
            let trailing = render_byte_set(trailing.bytes(), facade);
            let qualifying = render_byte_set(qualifying.bytes(), facade);
            let rule = render_lex_rule(&rules[index], ranks[index], facade);
            quote!(#facade::__private::OperatorRunRule {
                characters: #admitted,
                fence_starts: #fence_starts,
                fences: &[#(#fences),*],
                trailing: #trailing,
                qualifying: #qualifying,
                rule: #rule,
            })
        })
        .collect::<Vec<_>>();
    let operator_run_count = operator_runs.len();

    // Operator-fence suppression is the only mechanism that rejects a winning
    // candidate, and it rejects only fixed spellings. The recovery table is
    // therefore emitted only for grammars that declare an operator run.
    let fixed_literals = if operator_runs.is_empty() {
        Vec::new()
    } else {
        rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.fixed && rule.closed_matcher.is_none())
            .filter_map(|(index, rule)| {
                let spelling = rule.spelling.as_deref()?;
                let ascii_insensitive = rule.ascii_insensitive;
                let record = render_lex_rule(rule, ranks[index], facade);
                Some(quote!(#facade::__private::FixedLexLiteral {
                    spelling: #spelling,
                    ascii_insensitive: #ascii_insensitive,
                    rule: #record,
                }))
            })
            .collect::<Vec<_>>()
    };
    let fixed_literal_count = fixed_literals.len();

    let lexical_error_kind = plan.lexical_error_kind;
    let retains_trivia = rules.iter().any(|rule| rule.trivia_kind.is_some());

    Ok(quote! {
        #closed_machine

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[derive(#facade::__private::logos::Logos)]
        #[logos(crate = #facade::__private::logos)]
        #[logos(extras = #facade::__private::LexExtras)]
        enum __RecursaLexToken {
            #(#variants)*
        }

        impl #facade::__private::LexTokenIndex for __RecursaLexToken {
            fn rule_index(&self) -> usize {
                match self {
                    #(#index_arms)*
                }
            }
        }

        static __RECURSA_LEX_RULES: [#facade::__private::LexRule; #machine_rule_count] =
            [#(#machine_rules),*];

        static __RECURSA_FIXED_LEX_LITERALS:
            [#facade::__private::FixedLexLiteral; #fixed_literal_count] =
            [#(#fixed_literals),*];

        static __RECURSA_OPERATOR_RUNS:
            [#facade::__private::OperatorRunRule; #operator_run_count] =
            [#(#operator_runs),*];

        static __RECURSA_LEX_PLAN: #facade::__private::LexPlan = #facade::__private::LexPlan {
            rules: &__RECURSA_LEX_RULES,
            fixed_literals: &__RECURSA_FIXED_LEX_LITERALS,
            operator_runs: &__RECURSA_OPERATOR_RUNS,
            closed_first_bytes: #closed_first_bytes,
            closed_scan: #closed_scan,
            lexical_error_kind: #lexical_error_kind,
            retains_trivia: #retains_trivia,
        };
    })
}
