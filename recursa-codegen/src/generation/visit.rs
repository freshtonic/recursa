//! Typed Visit event planning and traversal rendering.
//!
//! Variant naming, event-family emission, and child traversal stay together so generated
//! Visit implementations share one stable source-relative identity scheme.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    #[test]
    fn nested_visit_variant_uses_the_path_relative_to_the_grammar_module() {
        test_support::nested_visit_variant_uses_the_path_relative_to_the_grammar_module();
    }
    #[test]
    fn root_visit_variants_are_encoded_away_from_reserved_and_nested_names() {
        test_support::root_visit_variants_are_encoded_away_from_reserved_and_nested_names();
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn typed_visit_node_variant(plan: &EmissionPlan, node: &NodeParsePlan) -> Option<Ident> {
    (node.publicly_materialized
        && node
            .generics
            .params
            .iter()
            .all(|parameter| matches!(parameter, GenericParam::Lifetime(_))))
    .then(|| visit_variant_ident(plan, &node.semantic_key))
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn typed_visit_token_binding(
    plan: &EmissionPlan,
    token: usize,
) -> Option<&TokenBindingPlan> {
    plan.token_bindings
        .iter()
        .find(|binding| binding.token == token && binding.publicly_materialized)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn visit_variant_ident(plan: &EmissionPlan, semantic_key: &str) -> Ident {
    let anchor = plan
        .module_anchor
        .segments
        .last()
        .expect("a module anchor is nonempty")
        .ident
        .unraw()
        .to_string();
    format_ident!("{}", visit_variant_name(&anchor, semantic_key))
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn visit_variant_name(anchor: &str, semantic_key: &str) -> String {
    let segments = semantic_key.split("::").collect::<Vec<_>>();
    let anchor_index = segments
        .iter()
        .position(|segment| *segment == anchor)
        .map_or(0, |index| index + 1);
    let relative = semantic_key
        .split("::")
        .skip(anchor_index)
        .collect::<Vec<_>>()
        .join("::");
    visit_nested_variant(&relative)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn visit_nested_variant(relative: &str) -> String {
    let suffix = relative
        .split("::")
        .map(|segment| format!("{}_{}", segment.len(), segment))
        .collect::<Vec<_>>()
        .join("_");
    format!("__RecursaTarget_{suffix}")
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_visit_events(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let visit = plan
        .root_configured_traits
        .contains(&ConfiguredTraitRecipe::Visit)
        || plan
            .nodes
            .iter()
            .flat_map(|node| node.configured_traits.iter())
            .chain(
                plan.tokens
                    .iter()
                    .flat_map(|token| token.configured_traits.iter()),
            )
            .any(|recipe| *recipe == ConfiguredTraitRecipe::Visit);
    let visit_mut = plan
        .root_configured_traits
        .contains(&ConfiguredTraitRecipe::VisitMut)
        || plan
            .nodes
            .iter()
            .flat_map(|node| node.configured_traits.iter())
            .chain(
                plan.tokens
                    .iter()
                    .flat_map(|token| token.configured_traits.iter()),
            )
            .any(|recipe| *recipe == ConfiguredTraitRecipe::VisitMut);
    let node_variants = plan.nodes.iter().filter_map(|node| {
        let variant = typed_visit_node_variant(plan, node)?;
        let mut path = syn::parse_str::<syn::Path>(&format!("crate::{}", node.semantic_key))
            .expect("a resolved semantic key is a Rust path");
        let lifetimes = node
            .generics
            .params
            .iter()
            .filter(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
            .map(|_| quote!('__recursa_input))
            .collect::<Vec<_>>();
        if !lifetimes.is_empty() {
            path.segments
                .last_mut()
                .expect("nonempty semantic path")
                .arguments = PathArguments::AngleBracketed(syn::parse_quote!(<#(#lifetimes),*>));
        }
        let ty = quote!(#path);
        Some(quote!(#variant(&'__recursa_visit #ty),))
    });
    let token_variants = plan.tokens.iter().enumerate().filter_map(|(index, token)| {
        let binding = typed_visit_token_binding(plan, index)?;
        let variant = visit_variant_ident(plan, &binding.semantic_key);
        let canonical = &token.ident;
        Some(quote! {
            #variant(
                &'__recursa_visit
                    #module_anchor::__recursa_content_tokens::#canonical<'__recursa_input>,
            ),
        })
    });
    let node_variants_mut = plan.nodes.iter().filter_map(|node| {
        let variant = typed_visit_node_variant(plan, node)?;
        let mut path = syn::parse_str::<syn::Path>(&format!("crate::{}", node.semantic_key))
            .expect("a resolved semantic key is a Rust path");
        let lifetimes = node
            .generics
            .params
            .iter()
            .filter(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
            .map(|_| quote!('__recursa_input))
            .collect::<Vec<_>>();
        if !lifetimes.is_empty() {
            path.segments
                .last_mut()
                .expect("nonempty semantic path")
                .arguments = PathArguments::AngleBracketed(syn::parse_quote!(<#(#lifetimes),*>));
        }
        let ty = quote!(#path);
        Some(quote!(#variant(&'__recursa_visit mut #ty),))
    });
    let token_variants_mut = plan.tokens.iter().enumerate().filter_map(|(index, token)| {
        let binding = typed_visit_token_binding(plan, index)?;
        let variant = visit_variant_ident(plan, &binding.semantic_key);
        let canonical = &token.ident;
        Some(quote! {
            #variant(
                &'__recursa_visit mut
                    #module_anchor::__recursa_content_tokens::#canonical<'__recursa_input>,
            ),
        })
    });
    let visit_event = visit.then(|| {
        quote! {
            #[doc(hidden)]
            pub struct __RecursaVisitEvents<'__recursa_input>(
                ::core::marker::PhantomData<
                    fn(&'__recursa_input ()) -> &'__recursa_input (),
                >,
            );

            impl<'__recursa_input> #facade::__private::VisitEventFamily
                for __RecursaVisitEvents<'__recursa_input>
            {
                type Event<'__recursa_visit> =
                    __RecursaVisitEvent<'__recursa_visit, '__recursa_input>
                where
                    Self: '__recursa_visit;
            }

            #[doc(hidden)]
            #[allow(non_camel_case_types, private_interfaces)]
            pub enum __RecursaVisitEvent<'__recursa_visit, '__recursa_input>
            where
                '__recursa_input: '__recursa_visit,
            {
                #(#node_variants)*
                #(#token_variants)*
                Opaque(
                    &'__recursa_visit dyn #facade::__private::VisitNode,
                    ::core::marker::PhantomData<&'__recursa_input ()>,
                ),
            }
        }
    });
    let visit_mut_event = visit_mut.then(|| {
        quote! {
            #[doc(hidden)]
            pub struct __RecursaVisitEventsMut<'__recursa_input>(
                ::core::marker::PhantomData<
                    fn(&'__recursa_input ()) -> &'__recursa_input (),
                >,
            );

            impl<'__recursa_input> #facade::__private::VisitEventFamilyMut
                for __RecursaVisitEventsMut<'__recursa_input>
            {
                type EventMut<'__recursa_visit> =
                    __RecursaVisitEventMut<'__recursa_visit, '__recursa_input>
                where
                    Self: '__recursa_visit;
            }

            #[doc(hidden)]
            #[allow(non_camel_case_types, private_interfaces)]
            pub enum __RecursaVisitEventMut<'__recursa_visit, '__recursa_input>
            where
                '__recursa_input: '__recursa_visit,
            {
                #(#node_variants_mut)*
                #(#token_variants_mut)*
                Opaque(
                    &'__recursa_visit mut dyn #facade::__private::VisitNode,
                    ::core::marker::PhantomData<&'__recursa_input ()>,
                ),
            }
        }
    });
    quote!(#visit_event #visit_mut_event)
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_node_visit(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    mutable: bool,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let node_ident = &node.ident;
    let mut generics = node.generics.clone();
    let event_input = if node
        .generics
        .params
        .iter()
        .any(|parameter| matches!(parameter, GenericParam::Lifetime(_)))
    {
        let input_lifetime = &node.input_lifetime;
        quote!(#input_lifetime)
    } else {
        quote!('static)
    };
    let bound = if mutable {
        quote!(#facade::VisitMut<
            EventsMut = #module_anchor::__RecursaVisitEventsMut<#event_input>
        >)
    } else {
        quote!(#facade::Visit<
            Events = #module_anchor::__RecursaVisitEvents<#event_input>
        >)
    };
    for field_type in semantic_field_types(facade, node) {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #bound));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let children = render_visit_children(facade, node, mutable);
    let variant = typed_visit_node_variant(plan, node);
    if mutable {
        let event = quote!(#module_anchor::__RecursaVisitEventMut);
        let events = quote!(#module_anchor::__RecursaVisitEventsMut);
        let enter = variant.as_ref().map_or_else(
            || quote!(#event::Opaque(self, ::core::marker::PhantomData)),
            |variant| quote!(#event::#variant(self)),
        );
        let exit = enter.clone();
        quote! {
            impl #impl_generics #facade::VisitMut for #node_ident #type_generics #where_clause {
                type EventsMut = #events<#event_input>;

                fn visit_mut<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<Self::EventsMut>,
                {
                    match visitor.total_enter_mut(#enter) {
                        ::core::ops::ControlFlow::Continue(()) => { #children }
                        ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                        other => return other,
                    }
                    visitor.total_exit_mut(#exit)
                }
            }
        }
    } else {
        let event = quote!(#module_anchor::__RecursaVisitEvent);
        let events = quote!(#module_anchor::__RecursaVisitEvents);
        let enter = variant.as_ref().map_or_else(
            || quote!(#event::Opaque(self, ::core::marker::PhantomData)),
            |variant| quote!(#event::#variant(self)),
        );
        let exit = enter.clone();
        quote! {
            impl #impl_generics #facade::Visit for #node_ident #type_generics #where_clause {
                type Events = #events<#event_input>;

                fn visit<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<Self::Events>,
                {
                    match visitor.total_enter(#enter) {
                        ::core::ops::ControlFlow::Continue(()) => { #children }
                        ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                        other => return other,
                    }
                    visitor.total_exit(#exit)
                }
            }
        }
    }
}

/// Capability-owned plan or rendering step. Kept ordered where splitting would obscure shared binding scopes or alter deterministic plan, protocol, or generated-byte order.
pub(super) fn render_visit_children(
    facade: &Ident,
    node: &NodeParsePlan,
    mutable: bool,
) -> proc_macro2::TokenStream {
    let visit = |value: proc_macro2::TokenStream| {
        if mutable {
            quote!(#facade::VisitMut::visit_mut(#value, visitor)?;)
        } else {
            quote!(#facade::Visit::visit(#value, visitor)?;)
        }
    };
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let fields = elements.iter().filter_map(|element| {
                let ElementParseKind::Semantic { field, .. } = &element.kind else {
                    return None;
                };
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                Some(visit(if mutable {
                    quote!(&mut #access)
                } else {
                    quote!(&#access)
                }))
            });
            quote!(#(#fields)*)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let node_ident = &node.ident;
            let arms = variants.iter().map(|variant| {
                let semantic = variant
                    .elements
                    .iter()
                    .filter_map(|element| match &element.kind {
                        ElementParseKind::Semantic { field, .. } => Some(field),
                        ElementParseKind::Fixed { .. } => None,
                    })
                    .collect::<Vec<_>>();
                let bindings = semantic
                    .iter()
                    .map(|field| format_ident!("__recursa_visit_field_{}", field.index))
                    .collect::<Vec<_>>();
                let variant_ident = &variant.ident;
                let pattern = match variant.style {
                    ResolvedFieldStyle::Named => {
                        let fields = semantic.iter().zip(&bindings).map(|(field, binding)| {
                            let ident = field
                                .ident
                                .as_ref()
                                .expect("a named variant field has an identifier");
                            quote!(#ident: #binding)
                        });
                        quote!(#node_ident::#variant_ident { #(#fields),* })
                    }
                    ResolvedFieldStyle::Unnamed => {
                        let field_count = semantic
                            .iter()
                            .map(|field| field.index + 1)
                            .max()
                            .unwrap_or(0);
                        let mut fields = (0..field_count).map(|_| quote!(_)).collect::<Vec<_>>();
                        for (field, binding) in semantic.iter().zip(&bindings) {
                            fields[field.index] = quote!(#binding);
                        }
                        quote!(#node_ident::#variant_ident(#(#fields),*))
                    }
                    ResolvedFieldStyle::Unit => quote!(#node_ident::#variant_ident),
                };
                let visits = bindings.into_iter().map(|binding| visit(quote!(#binding)));
                quote!(#pattern => { #(#visits)* })
            });
            quote!(match self { #(#arms),* })
        }
    }
}
