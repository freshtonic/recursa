//! Typed Visit event planning and traversal rendering.
//!
//! Variant naming, event-family emission, and child traversal stay together so generated
//! Visit implementations share one stable source-relative identity scheme.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    use super::{render_visit_match_dispatch, visit_dispatch_level_widths};
    #[test]
    fn nested_visit_variant_uses_the_path_relative_to_the_grammar_module() {
        test_support::nested_visit_variant_uses_the_path_relative_to_the_grammar_module();
    }
    #[test]
    fn root_visit_variants_are_encoded_away_from_reserved_and_nested_names() {
        test_support::root_visit_variants_are_encoded_away_from_reserved_and_nested_names();
    }
    #[test]
    fn recursive_concrete_visit_impls_do_not_publish_cyclic_child_bounds() {
        test_support::recursive_concrete_visit_impls_do_not_publish_cyclic_child_bounds();
    }
    #[test]
    fn visit_bounds_only_cover_node_type_parameters() {
        test_support::visit_bounds_only_cover_node_type_parameters();
    }

    #[test]
    fn large_visit_matches_are_split_into_bounded_helpers() {
        let arms = (0..170)
            .map(|index| {
                let variant = quote::format_ident!("Variant{index}");
                (quote::quote!(Node::#variant), quote::quote!())
            })
            .collect::<Vec<_>>();
        let layout = visit_dispatch_level_widths(arms.len());

        assert!(layout[0].iter().all(|width| *width <= 32));
        assert!(layout[1..].iter().flatten().all(|width| *width <= 16));

        for mutable in [false, true] {
            let rendered = render_visit_match_dispatch(
                &quote::format_ident!("recursa"),
                &syn::parse_quote!(crate::grammar),
                mutable,
                &quote::quote!('input),
                &arms,
            );
            let methods = rendered.methods.to_string();
            let call = rendered.body.to_string();
            let dispatcher = if mutable {
                "__recursa_visit_mut_children"
            } else {
                "__recursa_visit_children"
            };

            assert_eq!(
                methods.matches(&format!("fn {dispatcher}_leaf_")).count(),
                6
            );
            assert_eq!(methods.matches(&format!("fn {dispatcher} ")).count(), 1);
            assert_eq!(call, format!("Self :: {dispatcher} (self , visitor) ? ;"));
        }
    }
}

/// Reports whether authored visibility exposes an item outside its module.
pub(super) fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

/// Returns the event variant for a publicly materialized lifetime-only Node application.
pub(super) fn typed_visit_node_variant(plan: &EmissionPlan, node: &NodeParsePlan) -> Option<Ident> {
    (node.publicly_materialized
        && node
            .generics
            .params
            .iter()
            .all(|parameter| matches!(parameter, GenericParam::Lifetime(_))))
    .then(|| visit_variant_ident(plan, &node.semantic_key))
}

/// Finds the public generated binding for a content-token identity.
pub(super) fn typed_visit_token_binding(
    plan: &EmissionPlan,
    token: usize,
) -> Option<&TokenBindingPlan> {
    plan.token_bindings
        .iter()
        .find(|binding| binding.token == token && binding.publicly_materialized)
}

/// Derives a collision-resistant event identifier from a Node's semantic key.
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

/// Encodes a semantic key relative to the selected-root anchor for event dispatch.
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

/// Length-prefixes nested path segments so distinct semantic paths cannot collide.
pub(super) fn visit_nested_variant(relative: &str) -> String {
    let suffix = relative
        .split("::")
        .map(|segment| format!("{}_{}", segment.len(), segment))
        .collect::<Vec<_>>()
        .join("_");
    format!("__RecursaTarget_{suffix}")
}

/// Reports whether one generated traversal event family is required anywhere in the grammar.
pub(super) fn visit_family_enabled(plan: &EmissionPlan, mutable: bool) -> bool {
    let recipe = if mutable {
        ConfiguredTraitRecipe::VisitMut
    } else {
        ConfiguredTraitRecipe::Visit
    };
    plan.root_configured_traits.contains(&recipe)
        || plan
            .nodes
            .iter()
            .flat_map(|node| node.configured_traits.iter())
            .chain(
                plan.tokens
                    .iter()
                    .flat_map(|token| token.configured_traits.iter()),
            )
            .any(|candidate| *candidate == recipe)
}

/// Renders the closed immutable and mutable visit-event families for public grammar types.
///
/// # Long-function contract
///
/// Rationale: emits the closed event family and dispatch surface together.
/// Invariants: event variants are source-relative, collision-free, and borrow values for the visit duration.
/// Phases: declares immutable and mutable events, visitor traits, then event dispatch helpers.
pub(super) fn render_visit_events(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let visit = visit_family_enabled(plan, false);
    let visit_mut = visit_family_enabled(plan, true);
    let node_variants = plan.nodes.iter().filter_map(|node| {
        let variant = typed_visit_node_variant(plan, node)?;
        let mut path = emitted_semantic_path(&node.semantic_key)
            .expect("a resolved semantic key is an emitted Rust path");
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
        let mut path = emitted_semantic_path(&node.semantic_key)
            .expect("a resolved semantic key is an emitted Rust path");
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

            #[doc(hidden)]
            pub(crate) trait __RecursaVisitWith<'__recursa_input> {
                fn __recursa_visit_with<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<__RecursaVisitEvents<'__recursa_input>>;
            }

            impl<'__recursa_input, T> __RecursaVisitWith<'__recursa_input> for Box<T>
            where
                T: __RecursaVisitWith<'__recursa_input>,
            {
                fn __recursa_visit_with<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<__RecursaVisitEvents<'__recursa_input>>,
                {
                    (**self).__recursa_visit_with(visitor)
                }
            }

            impl<'__recursa_input, T> __RecursaVisitWith<'__recursa_input> for Option<T>
            where
                T: __RecursaVisitWith<'__recursa_input>,
            {
                fn __recursa_visit_with<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<__RecursaVisitEvents<'__recursa_input>>,
                {
                    if let Some(value) = self {
                        value.__recursa_visit_with(visitor)?;
                    }
                    ::core::ops::ControlFlow::Continue(())
                }
            }

            impl<'__recursa_input, T> __RecursaVisitWith<'__recursa_input> for Vec<T>
            where
                T: __RecursaVisitWith<'__recursa_input>,
            {
                fn __recursa_visit_with<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<__RecursaVisitEvents<'__recursa_input>>,
                {
                    for value in self {
                        value.__recursa_visit_with(visitor)?;
                    }
                    ::core::ops::ControlFlow::Continue(())
                }
            }

            impl<'__recursa_input, T> __RecursaVisitWith<'__recursa_input> for #facade::Vec1<T>
            where
                T: __RecursaVisitWith<'__recursa_input>,
            {
                fn __recursa_visit_with<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<__RecursaVisitEvents<'__recursa_input>>,
                {
                    for value in self.iter() {
                        value.__recursa_visit_with(visitor)?;
                    }
                    ::core::ops::ControlFlow::Continue(())
                }
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

            #[doc(hidden)]
            pub(crate) trait __RecursaVisitMutWith<'__recursa_input> {
                fn __recursa_visit_mut_with<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<__RecursaVisitEventsMut<'__recursa_input>>;
            }

            impl<'__recursa_input, T> __RecursaVisitMutWith<'__recursa_input> for Box<T>
            where
                T: __RecursaVisitMutWith<'__recursa_input>,
            {
                fn __recursa_visit_mut_with<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<__RecursaVisitEventsMut<'__recursa_input>>,
                {
                    (**self).__recursa_visit_mut_with(visitor)
                }
            }

            impl<'__recursa_input, T> __RecursaVisitMutWith<'__recursa_input> for Option<T>
            where
                T: __RecursaVisitMutWith<'__recursa_input>,
            {
                fn __recursa_visit_mut_with<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<__RecursaVisitEventsMut<'__recursa_input>>,
                {
                    if let Some(value) = self {
                        value.__recursa_visit_mut_with(visitor)?;
                    }
                    ::core::ops::ControlFlow::Continue(())
                }
            }

            impl<'__recursa_input, T> __RecursaVisitMutWith<'__recursa_input> for Vec<T>
            where
                T: __RecursaVisitMutWith<'__recursa_input>,
            {
                fn __recursa_visit_mut_with<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<__RecursaVisitEventsMut<'__recursa_input>>,
                {
                    for value in self {
                        value.__recursa_visit_mut_with(visitor)?;
                    }
                    ::core::ops::ControlFlow::Continue(())
                }
            }

            impl<'__recursa_input, T> __RecursaVisitMutWith<'__recursa_input> for #facade::Vec1<T>
            where
                T: __RecursaVisitMutWith<'__recursa_input>,
            {
                fn __recursa_visit_mut_with<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<__RecursaVisitEventsMut<'__recursa_input>>,
                {
                    for value in self.iter_mut() {
                        value.__recursa_visit_mut_with(visitor)?;
                    }
                    ::core::ops::ControlFlow::Continue(())
                }
            }
        }
    });
    quote!(#visit_event #visit_mut_event)
}

/// Maximum enum variants typechecked by one generated traversal helper.
const VISIT_DISPATCH_LEAF_WIDTH: usize = 32;

/// Maximum child helpers called by one generated traversal routing helper.
const VISIT_DISPATCH_ROUTE_WIDTH: usize = 16;

/// Generated traversal helpers plus the single expression used by the public traversal body.
struct RenderedVisitChildren {
    /// Private bounded helper methods emitted beside the generated traversal implementation.
    methods: proc_macro2::TokenStream,
    /// Single dispatcher invocation inserted into the public traversal method.
    body: proc_macro2::TokenStream,
}

/// Plans bounded leaf and routing levels for a large enum traversal match.
fn visit_dispatch_level_widths(variant_count: usize) -> Vec<Vec<usize>> {
    if variant_count == 0 {
        return Vec::new();
    }
    let mut levels = Vec::new();
    let mut remaining = variant_count;
    let mut leaves = Vec::new();
    while remaining != 0 {
        let width = remaining.min(VISIT_DISPATCH_LEAF_WIDTH);
        leaves.push(width);
        remaining -= width;
    }
    let mut child_count = leaves.len();
    levels.push(leaves);
    while child_count > 1 {
        let mut remaining = child_count;
        let mut routes = Vec::new();
        while remaining != 0 {
            let width = remaining.min(VISIT_DISPATCH_ROUTE_WIDTH);
            routes.push(width);
            remaining -= width;
        }
        child_count = routes.len();
        levels.push(routes);
    }
    levels
}

/// Splits a large variant traversal into independently typechecked associated helpers.
fn render_visit_match_dispatch(
    facade: &Ident,
    module_anchor: &syn::Path,
    mutable: bool,
    event_input: &proc_macro2::TokenStream,
    arms: &[(proc_macro2::TokenStream, proc_macro2::TokenStream)],
) -> RenderedVisitChildren {
    let layout = visit_dispatch_level_widths(arms.len());
    debug_assert!(!layout.is_empty());
    let dispatcher = if mutable {
        format_ident!("__recursa_visit_mut_children")
    } else {
        format_ident!("__recursa_visit_children")
    };
    let receiver = if mutable {
        quote!(&mut self)
    } else {
        quote!(&self)
    };
    let visitor_bound = if mutable {
        quote!(#facade::TotalVisitorMut<
            #module_anchor::__RecursaVisitEventsMut<#event_input>
        >)
    } else {
        quote!(#facade::TotalVisitor<
            #module_anchor::__RecursaVisitEvents<#event_input>
        >)
    };

    let mut definitions = Vec::new();
    let mut variant_start = 0usize;
    let mut children = Vec::with_capacity(layout[0].len());
    for (leaf_index, width) in layout[0].iter().copied().enumerate() {
        let leaf = format_ident!("{dispatcher}_leaf_{leaf_index}");
        let variant_end = variant_start + width;
        let leaf_arms = arms[variant_start..variant_end]
            .iter()
            .map(|(pattern, visits)| {
                quote! {
                    #pattern => {
                        #visits
                        ::core::ops::ControlFlow::Continue(true)
                    }
                }
            });
        definitions.push(quote! {
            fn #leaf<V>(
                #receiver,
                visitor: &mut V,
            ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>, bool>
            where
                V: #visitor_bound,
            {
                match self {
                    #(#leaf_arms,)*
                    _ => ::core::ops::ControlFlow::Continue(false),
                }
            }
        });
        children.push(leaf);
        variant_start = variant_end;
    }

    for (level_index, widths) in layout.iter().enumerate().skip(1) {
        let mut child_start = 0usize;
        let mut parents = Vec::with_capacity(widths.len());
        for (route_index, width) in widths.iter().copied().enumerate() {
            let route = format_ident!("{dispatcher}_route_{level_index}_{route_index}");
            let child_end = child_start + width;
            let calls = children[child_start..child_end].iter().map(|child| {
                quote! {
                    if Self::#child(self, visitor)? {
                        return ::core::ops::ControlFlow::Continue(true);
                    }
                }
            });
            definitions.push(quote! {
                fn #route<V>(
                    #receiver,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>, bool>
                where
                    V: #visitor_bound,
                {
                    #(#calls)*
                    ::core::ops::ControlFlow::Continue(false)
                }
            });
            parents.push(route);
            child_start = child_end;
        }
        children = parents;
    }

    let root = children
        .first()
        .expect("a non-empty visit dispatch has a root");
    definitions.push(quote! {
        fn #dispatcher<V>(
            #receiver,
            visitor: &mut V,
        ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
        where
            V: #visitor_bound,
        {
            let _ = Self::#root(self, visitor)?;
            ::core::ops::ControlFlow::Continue(())
        }
    });

    RenderedVisitChildren {
        methods: quote!(#(#definitions)*),
        body: quote!(Self::#dispatcher(self, visitor)?;),
    }
}

/// Renders one Node's configured immutable or mutable traversal implementation.
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
    let input_agnostic = node
        .generics
        .params
        .iter()
        .all(|parameter| !matches!(parameter, GenericParam::Lifetime(_)));
    add_visit_field_bounds(plan, node, mutable, &event_input, &mut generics);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let children = render_visit_children(plan, node, mutable, &event_input);
    let children_methods = &children.methods;
    let children_helper_impl = (!input_agnostic && !children_methods.is_empty()).then(|| {
        quote! {
            impl #impl_generics #node_ident #type_generics #where_clause {
                #children_methods
            }
        }
    });
    let variant = typed_visit_node_variant(plan, node);
    let input_agnostic_impl = if input_agnostic {
        let helper_input: syn::Lifetime = syn::parse_quote!('__recursa_event_input);
        let helper_event_input = quote!(#helper_input);
        let mut helper_generics = node.generics.clone();
        helper_generics
            .params
            .insert(0, syn::parse_quote!(#helper_input));
        add_visit_field_bounds(
            plan,
            node,
            mutable,
            &helper_event_input,
            &mut helper_generics,
        );
        let (helper_impl_generics, _, helper_where_clause) = helper_generics.split_for_impl();
        let helper_children = render_visit_children(plan, node, mutable, &helper_event_input);
        let helper_children_methods = &helper_children.methods;
        let helper_children_body = &helper_children.body;
        let helper_methods_impl = (!helper_children_methods.is_empty()).then(|| {
            quote! {
                impl #helper_impl_generics #node_ident #type_generics #helper_where_clause {
                    #helper_children_methods
                }
            }
        });
        if mutable {
            let event = quote!(#module_anchor::__RecursaVisitEventMut);
            let enter = variant.as_ref().map_or_else(
                || quote!(#event::Opaque(self, ::core::marker::PhantomData)),
                |variant| quote!(#event::#variant(self)),
            );
            let exit = enter.clone();
            Some(quote! {
                #helper_methods_impl

                impl #helper_impl_generics
                    #module_anchor::__RecursaVisitMutWith<#helper_input>
                    for #node_ident #type_generics #helper_where_clause
                {
                    fn __recursa_visit_mut_with<V>(
                        &mut self,
                        visitor: &mut V,
                    ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                    where
                        V: #facade::TotalVisitorMut<
                            #module_anchor::__RecursaVisitEventsMut<#helper_input>
                        >,
                    {
                        match visitor.total_enter_mut(#enter) {
                            ::core::ops::ControlFlow::Continue(()) => { #helper_children_body }
                            ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                            other => return other,
                        }
                        visitor.total_exit_mut(#exit)
                    }
                }
            })
        } else {
            let event = quote!(#module_anchor::__RecursaVisitEvent);
            let enter = variant.as_ref().map_or_else(
                || quote!(#event::Opaque(self, ::core::marker::PhantomData)),
                |variant| quote!(#event::#variant(self)),
            );
            let exit = enter.clone();
            Some(quote! {
                #helper_methods_impl

                impl #helper_impl_generics
                    #module_anchor::__RecursaVisitWith<#helper_input>
                    for #node_ident #type_generics #helper_where_clause
                {
                    fn __recursa_visit_with<V>(
                        &self,
                        visitor: &mut V,
                    ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                    where
                        V: #facade::TotalVisitor<
                            #module_anchor::__RecursaVisitEvents<#helper_input>
                        >,
                    {
                        match visitor.total_enter(#enter) {
                            ::core::ops::ControlFlow::Continue(()) => { #helper_children_body }
                            ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                            other => return other,
                        }
                        visitor.total_exit(#exit)
                    }
                }
            })
        }
    } else {
        None
    };
    if mutable {
        let event = quote!(#module_anchor::__RecursaVisitEventMut);
        let events = quote!(#module_anchor::__RecursaVisitEventsMut);
        let enter = variant.as_ref().map_or_else(
            || quote!(#event::Opaque(self, ::core::marker::PhantomData)),
            |variant| quote!(#event::#variant(self)),
        );
        let exit = enter.clone();
        let visit_body = if input_agnostic_impl.is_some() {
            quote! {
                <Self as #module_anchor::__RecursaVisitMutWith<'static>>
                    ::__recursa_visit_mut_with(self, visitor)
            }
        } else {
            let children_body = &children.body;
            quote! {
                match visitor.total_enter_mut(#enter) {
                    ::core::ops::ControlFlow::Continue(()) => { #children_body }
                    ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                    other => return other,
                }
                visitor.total_exit_mut(#exit)
            }
        };
        quote! {
            #children_helper_impl
            #input_agnostic_impl

            impl #impl_generics #facade::VisitMut for #node_ident #type_generics #where_clause {
                type EventsMut = #events<#event_input>;

                fn visit_mut<V>(
                    &mut self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitorMut<Self::EventsMut>,
                {
                    #visit_body
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
        let visit_body = if input_agnostic_impl.is_some() {
            quote! {
                <Self as #module_anchor::__RecursaVisitWith<'static>>
                    ::__recursa_visit_with(self, visitor)
            }
        } else {
            let children_body = &children.body;
            quote! {
                match visitor.total_enter(#enter) {
                    ::core::ops::ControlFlow::Continue(()) => { #children_body }
                    ::core::ops::ControlFlow::Break(#facade::VisitBreak::SkipChildren) => {}
                    other => return other,
                }
                visitor.total_exit(#exit)
            }
        };
        quote! {
            #children_helper_impl
            #input_agnostic_impl

            impl #impl_generics #facade::Visit for #node_ident #type_generics #where_clause {
                type Events = #events<#event_input>;

                fn visit<V>(
                    &self,
                    visitor: &mut V,
                ) -> ::core::ops::ControlFlow<#facade::VisitBreak<V::Error>>
                where
                    V: #facade::TotalVisitor<Self::Events>,
                {
                    #visit_body
                }
            }
        }
    }
}

/// Adds the field traversal requirements for one selected event-family lifetime.
fn add_visit_field_bounds(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    mutable: bool,
    event_input: &proc_macro2::TokenStream,
    generics: &mut Generics,
) {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let mut emitted = BTreeSet::new();
    for (field_type, uses_generated_helper, _requires_explicit_bound) in
        visit_field_types(plan, node, mutable)
            .into_iter()
            .filter(|(field_type, _, requires_explicit_bound)| {
                *requires_explicit_bound
                    || type_depends_on_node_type_parameters(field_type, node)
                    || type_mentions_non_event_node_lifetime(field_type, node)
            })
            .filter(|(field_type, uses_generated_helper, _)| {
                emitted.insert((quote!(#field_type).to_string(), *uses_generated_helper))
            })
    {
        let bound = if uses_generated_helper {
            if mutable {
                quote!(#module_anchor::__RecursaVisitMutWith<#event_input>)
            } else {
                quote!(#module_anchor::__RecursaVisitWith<#event_input>)
            }
        } else if mutable {
            quote!(#facade::VisitMut<
                EventsMut = #module_anchor::__RecursaVisitEventsMut<#event_input>
            >)
        } else {
            quote!(#facade::Visit<
                Events = #module_anchor::__RecursaVisitEvents<#event_input>
            >)
        };
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #bound));
    }
}

/// Returns non-recursive semantic fields whose traversal contract needs an explicit bound.
fn visit_field_types(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    mutable: bool,
) -> Vec<(Type, bool, bool)> {
    let mut fields = Vec::new();
    for_each_element(&node.body, &mut |element| {
        let ElementParseKind::Semantic {
            projection: semantic,
            ..
        } = &element.kind
        else {
            return;
        };
        let SemanticParseProjection::Direct {
            terminal,
            terminal_node,
            explorer_terminal,
            wrappers,
            ..
        } = semantic.as_ref()
        else {
            return;
        };
        if is_recursive_terminal(node, terminal) {
            return;
        }
        let cyclic = terminal_node
            .is_some_and(|child| node_reaches(plan, child, node.node, &mut BTreeSet::new()));
        if !cyclic {
            let content_token = matches!(explorer_terminal, ExplorerTerminalPlan::Content(_));
            let requires_explicit_bound = content_token
                || terminal_node.is_some_and(|child| {
                    let recipe = if mutable {
                        ConfiguredTraitRecipe::VisitMut
                    } else {
                        ConfiguredTraitRecipe::Visit
                    };
                    !plan.nodes.iter().any(|candidate| {
                        candidate.node == child && candidate.configured_traits.contains(&recipe)
                    })
                });
            fields.push((
                render_wrapped_type(&plan.facade, terminal, wrappers),
                content_token
                    || terminal_node
                        .is_some_and(|child| uses_input_agnostic_visit(plan, child, mutable)),
                requires_explicit_bound,
            ));
        }
    });
    fields
}

/// Reports whether a field refers to a Node lifetime other than its selected event lifetime.
fn type_mentions_non_event_node_lifetime(ty: &Type, node: &NodeParsePlan) -> bool {
    let candidates = node
        .generics
        .lifetimes()
        .map(|parameter| parameter.lifetime.ident.unraw().to_string())
        .filter(|candidate| candidate != &node.input_lifetime.ident.unraw().to_string())
        .collect::<BTreeSet<_>>();
    if candidates.is_empty() {
        return false;
    }

    /// Detects references to non-event lifetime parameters owned by one Node.
    struct LifetimeVisitor<'a> {
        /// Candidate lifetime parameter names.
        candidates: &'a BTreeSet<String>,
        /// Whether a candidate has been observed.
        found: bool,
    }

    impl<'ast> syn::visit::Visit<'ast> for LifetimeVisitor<'_> {
        fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
            if self
                .candidates
                .contains(&lifetime.ident.unraw().to_string())
            {
                self.found = true;
            }
        }
    }

    let mut visitor = LifetimeVisitor {
        candidates: &candidates,
        found: false,
    };
    syn::visit::Visit::visit_type(&mut visitor, ty);
    visitor.found
}

/// Reports whether a generated child can traverse under its caller's event-family lifetime.
fn uses_input_agnostic_visit(plan: &EmissionPlan, node: NodeId, mutable: bool) -> bool {
    let recipe = if mutable {
        ConfiguredTraitRecipe::VisitMut
    } else {
        ConfiguredTraitRecipe::Visit
    };
    plan.nodes
        .iter()
        .find(|candidate| candidate.node == node)
        .is_some_and(|child| {
            child
                .generics
                .params
                .iter()
                .all(|parameter| !matches!(parameter, GenericParam::Lifetime(_)))
                && child.configured_traits.contains(&recipe)
        })
}

/// Reports whether a direct semantic projection traverses through a generated helper trait.
fn projection_uses_generated_visit_helper(
    plan: &EmissionPlan,
    projection: &SemanticParseProjection,
    mutable: bool,
) -> bool {
    match projection {
        SemanticParseProjection::Direct {
            terminal_node,
            explorer_terminal,
            ..
        } => {
            matches!(explorer_terminal, ExplorerTerminalPlan::Content(_))
                || terminal_node
                    .is_some_and(|child| uses_input_agnostic_visit(plan, child, mutable))
        }
        SemanticParseProjection::FixedPresence { .. } => false,
    }
}

/// Reports whether the authored Node dependency graph reaches `target` from `current`.
fn node_reaches(
    plan: &EmissionPlan,
    current: NodeId,
    target: NodeId,
    visited: &mut BTreeSet<NodeId>,
) -> bool {
    if current == target {
        return true;
    }
    if !visited.insert(current) {
        return false;
    }
    let Some(body) = plan
        .nodes
        .iter()
        .find(|node| node.node == current)
        .map(|node| &node.body)
    else {
        return false;
    };
    let mut dependencies = Vec::new();
    for_each_element(body, &mut |element| {
        if let ElementParseKind::Semantic {
            projection: semantic,
            ..
        } = &element.kind
            && let SemanticParseProjection::Direct {
                terminal_node: Some(node),
                ..
            } = semantic.as_ref()
        {
            dependencies.push(*node);
        }
    });
    dependencies
        .into_iter()
        .any(|dependency| node_reaches(plan, dependency, target, visited))
}

/// Renders Visit calls for semantic children in authored source order.
fn render_visit_children(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    mutable: bool,
    event_input: &proc_macro2::TokenStream,
) -> RenderedVisitChildren {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let visit = |value: proc_macro2::TokenStream, uses_generated_helper: bool| {
        if uses_generated_helper && mutable {
            quote! {
                <_ as #module_anchor::__RecursaVisitMutWith<#event_input>>
                    ::__recursa_visit_mut_with(#value, visitor)?;
            }
        } else if uses_generated_helper {
            quote! {
                <_ as #module_anchor::__RecursaVisitWith<#event_input>>
                    ::__recursa_visit_with(#value, visitor)?;
            }
        } else if mutable {
            quote!(#facade::VisitMut::visit_mut(#value, visitor)?;)
        } else {
            quote!(#facade::Visit::visit(#value, visitor)?;)
        }
    };
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let fields = elements.iter().filter_map(|element| {
                let ElementParseKind::Semantic { field, projection } = &element.kind else {
                    return None;
                };
                if !matches!(projection.as_ref(), SemanticParseProjection::Direct { .. }) {
                    return None;
                }
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                Some(visit(
                    if mutable {
                        quote!(&mut #access)
                    } else {
                        quote!(&#access)
                    },
                    projection_uses_generated_visit_helper(plan, projection, mutable),
                ))
            });
            RenderedVisitChildren {
                methods: proc_macro2::TokenStream::new(),
                body: quote!(#(#fields)*),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let node_ident = &node.ident;
            let arms = variants
                .iter()
                .map(|variant| {
                    let semantic = variant
                        .elements
                        .iter()
                        .filter_map(|element| match &element.kind {
                            ElementParseKind::Semantic { field, projection } => {
                                let direct = matches!(
                                    projection.as_ref(),
                                    SemanticParseProjection::Direct { .. }
                                );
                                let uses_generated_helper = projection_uses_generated_visit_helper(
                                    plan, projection, mutable,
                                );
                                Some((field, direct, uses_generated_helper))
                            }
                            ElementParseKind::Fixed { .. }
                            | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
                        })
                        .collect::<Vec<_>>();
                    let bindings = semantic
                        .iter()
                        .map(|(field, _, _)| format_ident!("__recursa_visit_field_{}", field.index))
                        .collect::<Vec<_>>();
                    let variant_ident = &variant.ident;
                    let pattern =
                        match variant.style {
                            ResolvedFieldStyle::Named => {
                                let fields = semantic.iter().zip(&bindings).map(
                                    |((field, _, _), binding)| {
                                        let ident = field
                                            .ident
                                            .as_ref()
                                            .expect("a named variant field has an identifier");
                                        quote!(#ident: #binding)
                                    },
                                );
                                quote!(#node_ident::#variant_ident { #(#fields),* })
                            }
                            ResolvedFieldStyle::Unnamed => {
                                let field_count = semantic
                                    .iter()
                                    .map(|(field, _, _)| field.index + 1)
                                    .max()
                                    .unwrap_or(0);
                                let mut fields =
                                    (0..field_count).map(|_| quote!(_)).collect::<Vec<_>>();
                                for ((field, _, _), binding) in semantic.iter().zip(&bindings) {
                                    fields[field.index] = quote!(#binding);
                                }
                                quote!(#node_ident::#variant_ident(#(#fields),*))
                            }
                            ResolvedFieldStyle::Unit => quote!(#node_ident::#variant_ident),
                        };
                    let visits = semantic.iter().zip(bindings).filter_map(
                        |((_, direct, uses_generated_helper), binding)| {
                            direct.then(|| visit(quote!(#binding), *uses_generated_helper))
                        },
                    );
                    (pattern, quote!(#(#visits)*))
                })
                .collect::<Vec<_>>();
            if arms.len() > VISIT_DISPATCH_LEAF_WIDTH {
                render_visit_match_dispatch(facade, module_anchor, mutable, event_input, &arms)
            } else {
                let arms = arms
                    .iter()
                    .map(|(pattern, visits)| quote!(#pattern => { #visits }));
                RenderedVisitChildren {
                    methods: proc_macro2::TokenStream::new(),
                    body: quote!(match self { #(#arms),* }),
                }
            }
        }
    }
}
