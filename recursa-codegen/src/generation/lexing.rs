//! Generated lexing plans, token types, bindings, and lexical rendering.
//!
//! Token and rule data remain adjacent to emission so grammar-local token kinds,
//! callbacks, classifiers, and exact matched text share one review boundary.

use super::*;

#[derive(Clone)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct TokenParsePlan {
    pub(super) semantic: ContentTokenId,
    pub(super) ident: Ident,
    pub(super) kind: u16,
    pub(super) kind_variant: Ident,
    pub(super) accepted_kind_variants: Vec<Ident>,
    pub(super) expected: ExpectedSet,
    pub(super) capture: PlannedCapturePolicy,
    pub(super) arbitrary_spelling: String,
    pub(super) configured_traits: Vec<ConfiguredTraitRecipe>,
}

#[derive(Clone)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct TokenBindingPlan {
    pub(super) module: ModuleId,
    pub(super) semantic_key: String,
    pub(super) ident: Ident,
    pub(super) visibility: Visibility,
    pub(super) publicly_materialized: bool,
    pub(super) token: usize,
}
#[derive(Clone)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct LexRulePlan {
    pub(super) pattern: String,
    pub(super) kind: u16,
    pub(super) skip: bool,
    pub(super) callback: Option<syn::Path>,
    pub(super) fixed: bool,
    pub(super) priority: u32,
}

#[derive(Clone)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct KeywordClassifierPlan {
    pub(super) canonical_spelling: String,
    pub(super) kind: u16,
    pub(super) carriers: Vec<u16>,
    pub(super) ascii_insensitive: bool,
}

#[derive(Clone)]
/// Capability-owned private plan data retained in deterministic field and variant order.
pub(super) struct WordClassifierPlan {
    pub(super) canonical_spelling: String,
    pub(super) kind: u16,
    pub(super) ascii_insensitive: bool,
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
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

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn is_word_shaped(spelling: &str) -> bool {
    let mut characters = spelling.chars();
    characters.next().is_some_and(|first| {
        (first == '_' || unicode_ident::is_xid_start(first))
            && characters.all(unicode_ident::is_xid_continue)
    })
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn fixed_pretty_spelling(spelling: &str, equivalence: TextEquivalence) -> String {
    if equivalence == TextEquivalence::AsciiInsensitive && is_word_shaped(spelling) {
        spelling.to_ascii_uppercase()
    } else {
        spelling.to_owned()
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
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
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
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
    let parsed_kind = stores_kind.then(|| {
        let typed_kind_arms = accepted_kind_variants.iter().map(|variant| {
            let index = plan
                .kind_variants
                .iter()
                .position(|candidate| candidate == variant)
                .expect("an accepted token kind must have an emitted variant")
                as u16;
            quote!(#index => #module_anchor::TokenKind::#variant,)
        });
        quote! {
            kind: match record.kind {
                #(#typed_kind_arms)*
                _ => unreachable!("the parser admitted an unexpected token kind"),
            },
        }
    });
    let expected = format_ident!("__RECURSA_TOKEN_START_{index}");
    let capture = token.capture.render(facade);
    let standard_derives = token
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
    let standard_derive =
        (!standard_derives.is_empty()).then(|| quote!(#[derive(#(#standard_derives),*)]));
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
    let pretty_impl = plan.pretty.tokens[index].map(|behavior| {
        let max_width = plan.pretty_max_width;
        let indent = plan.pretty_indent;
        let value = match behavior {
            PrettyTokenBehavior::MatchedOrStoredText => quote! {
                let matched = context.matched_text();
                context.token(matched.as_deref().unwrap_or_else(|| self.text()))
            },
        };
        quote! {
            impl<'input> #facade::Pretty for #ident<'input> {
                fn pretty(
                    &self,
                    context: &mut #facade::PrettyContext,
                ) -> ::core::fmt::Result {
                    context.configure_defaults(#max_width, #indent);
                    #value
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
            ) -> Self::View<'view> { #view { value: self, cursor } }
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
                    return Err(context.unexpected(&#module_anchor::#expected));
                };
                if !#module_anchor::#expected.contains(record.kind) {
                    return Err(context.unexpected(&#module_anchor::#expected));
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

        #visit_impl
        #visit_mut_impl
        #pretty_impl
        #arbitrary_impl
        #serialize_impl
        #deserialize_impl
        #explorer_impl
    }
}
/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
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
    quote! {
        #[doc = #token_documentation]
        #visibility type #ident<'input> =
            #module_anchor::__recursa_content_tokens::#canonical<'input>;
        #[doc = "A provenance-aware view of this generated content-bearing token."]
        #visibility type #view<'view, 'input> =
            #module_anchor::__recursa_content_tokens::#canonical_view<'view, 'input>;
    }
}
