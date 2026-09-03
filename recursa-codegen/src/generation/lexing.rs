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
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let ident = &token.ident;
    let view = sibling_ident(ident, "View");
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

            impl<'input> #facade::Pretty for #view<'_, 'input> {
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

        #[doc(hidden)]
        #[derive(Clone, Copy)]
        pub struct #view<'view, 'input>
        where 'input: 'view,
        {
            value: &'view #ident<'input>,
            cursor: #facade::__private::ViewCursor<'view>,
        }

        impl<'input> #facade::__private::HasSpanSealed for #view<'_, 'input> {}
        impl<'input> #facade::HasSpan for #view<'_, 'input> {
            fn span(&self) -> Option<#facade::Span> { self.cursor.span() }
        }
        impl<'input> #facade::NodeView for #view<'_, 'input> {
            type Value = #ident<'input>;
            fn value(&self) -> &Self::Value { self.value }
        }
        impl<'input> #facade::__private::NodeViewFactory for #ident<'input> {
            type View<'view> = #view<'view, 'input> where Self: 'view;
            fn view<'view>(
                &'view self,
                cursor: #facade::__private::ViewCursor<'view>,
            ) -> <Self as #facade::__private::NodeViewFactory>::View<'view> {
                #view { value: self, cursor }
            }
        }
        impl<'input, A> #facade::__private::Parse<'input, A> for #ident<'input> {
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
        impl<'input, A> #facade::__private::ParseSealed<#module_anchor::Input<'input, A>>
            for #ident<'input>
        {}
        impl<'input, A> #facade::Parse<#module_anchor::Input<'input, A>> for #ident<'input> {
            type Output = #facade::Parsed<'input, Self>;
            fn parse(
                input: &mut #module_anchor::Input<'input, A>,
            ) -> Result<Self::Output, #facade::ParseError> {
                #facade::__private::parse(&mut input.inner)
            }
        }

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
    let view = sibling_ident(ident, "View");
    let canonical = &plan.tokens[binding.token].ident;
    let canonical_view = sibling_ident(canonical, "View");
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
        #[doc = "A provenance-aware view of this generated content-bearing token."]
        #visibility type #view<'view, 'input> =
            #module_anchor::__recursa_content_tokens::#canonical_view<'view, 'input>;
    }
}
