//! Configured-trait planning adapters and generated implementations.
//!
//! Standard traits, Arbitrary, and Serde share one recipe-owned boundary. Pretty and
//! Visit remain capability modules and are invoked here only through their renderers.

use super::*;

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    #[test]
    fn arbitrary_identity_collision_is_preserved_and_rejected_without_replacement() {
        test_support::arbitrary_identity_collision_is_preserved_and_rejected_without_replacement();
    }
}

/// Renders render node configured traits from frozen inputs without changing canonical generation order.
pub(super) fn render_node_configured_traits(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    node_index: usize,
) -> proc_macro2::TokenStream {
    let mut implementations = Vec::new();
    for recipe in [
        ConfiguredTraitRecipe::Debug,
        ConfiguredTraitRecipe::Clone,
        ConfiguredTraitRecipe::Copy,
        ConfiguredTraitRecipe::PartialEq,
        ConfiguredTraitRecipe::Eq,
        ConfiguredTraitRecipe::PartialOrd,
        ConfiguredTraitRecipe::Ord,
        ConfiguredTraitRecipe::Hash,
    ] {
        if node.configured_traits.contains(&recipe) {
            implementations.push(render_node_standard_trait(plan, node, recipe));
        }
    }
    if node.configured_traits.iter().any(|recipe| {
        matches!(
            recipe,
            ConfiguredTraitRecipe::Serialize | ConfiguredTraitRecipe::Deserialize
        )
    }) {
        implementations.push(render_node_serde(plan, node));
    }
    if node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Arbitrary)
    {
        implementations.push(render_node_arbitrary(plan, node));
    }
    if plan.pretty.nodes[node_index].is_some() {
        implementations.push(render_node_pretty(plan, node, node_index));
    }
    if node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Visit)
    {
        implementations.push(render_node_visit(plan, node, false));
    }
    if node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::VisitMut)
    {
        implementations.push(render_node_visit(plan, node, true));
    }
    quote!(#(#implementations)*)
}

/// Builds trait impl parts from frozen inputs without changing canonical generation order.
pub(super) fn trait_impl_parts(
    facade: &Ident,
    node: &NodeParsePlan,
    bound: proc_macro2::TokenStream,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let mut generics = node.generics.clone();
    for field_type in semantic_field_types(facade, node) {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #bound));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    (
        quote!(#impl_generics),
        quote!(#type_generics),
        quote!(#where_clause),
    )
}

/// Builds pretty impl parts from frozen inputs without changing canonical generation order.
pub(super) fn pretty_impl_parts(
    facade: &Ident,
    node: &NodeParsePlan,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let mut generics = node.generics.clone();
    for field_type in semantic_field_types(facade, node)
        .into_iter()
        .filter(|field_type| type_depends_on_node_type_parameters(field_type, node))
    {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #facade::Pretty));
    }
    let attachment_terminals = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => elements
            .iter()
            .filter_map(|element| attachment_pretty_terminal(node, element))
            .collect::<Vec<_>>(),
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .into_iter()
            .flat_map(|variant| variant.elements)
            .filter_map(|element| attachment_pretty_terminal(node, &element))
            .collect(),
    };
    for terminal in attachment_terminals
        .into_iter()
        .filter(|terminal| type_depends_on_node_type_parameters(terminal, node))
    {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#terminal: #facade::Pretty));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    (
        quote!(#impl_generics),
        quote!(#type_generics),
        quote!(#where_clause),
    )
}

/// Builds type depends on node type parameters from frozen inputs without changing canonical generation order.
pub(super) fn type_depends_on_node_type_parameters(ty: &Type, node: &NodeParsePlan) -> bool {
    let candidates = node
        .generics
        .type_params()
        .map(|parameter| parameter.ident.unraw().to_string())
        .collect::<BTreeSet<_>>();
    if candidates.is_empty() {
        return false;
    }

    /// Detects references to the Node's declared type parameters.
    struct DependencyVisitor<'a> {
        /// Candidate generic parameter names.
        candidates: &'a BTreeSet<String>,
        /// Whether a candidate has been observed.
        depends: bool,
    }

    impl<'ast> syn::visit::Visit<'ast> for DependencyVisitor<'_> {
        fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && let Some(segment) = path.path.segments.first()
                && self.candidates.contains(&segment.ident.unraw().to_string())
            {
                self.depends = true;
            }
            syn_visit::visit_type_path(self, path);
        }
    }

    let mut visitor = DependencyVisitor {
        candidates: &candidates,
        depends: false,
    };
    visitor.visit_type(ty);
    visitor.depends
}

/// Builds attachment pretty terminal from frozen inputs without changing canonical generation order.
pub(super) fn attachment_pretty_terminal(
    node: &NodeParsePlan,
    element: &ElementParsePlan,
) -> Option<Type> {
    match &element.kind {
        ElementParseKind::Semantic {
            terminal,
            attachment: Some(_),
            opaque_provenance: true,
            ..
        } if !is_recursive_terminal(node, terminal) => Some(terminal.as_ref().clone()),
        ElementParseKind::Fixed { .. } | ElementParseKind::Semantic { .. } => None,
    }
}

/// Renders render node standard trait from frozen inputs without changing canonical generation order.
pub(super) fn render_node_standard_trait(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    recipe: ConfiguredTraitRecipe,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let trait_path = match recipe {
        ConfiguredTraitRecipe::Debug => quote!(::core::fmt::Debug),
        ConfiguredTraitRecipe::Clone => quote!(::core::clone::Clone),
        ConfiguredTraitRecipe::Copy => quote!(::core::marker::Copy),
        ConfiguredTraitRecipe::PartialEq => quote!(::core::cmp::PartialEq),
        ConfiguredTraitRecipe::Eq => quote!(::core::cmp::Eq),
        ConfiguredTraitRecipe::PartialOrd => quote!(::core::cmp::PartialOrd),
        ConfiguredTraitRecipe::Ord => quote!(::core::cmp::Ord),
        ConfiguredTraitRecipe::Hash => quote!(::core::hash::Hash),
        _ => unreachable!("grammar-aware recipes are rendered separately"),
    };
    let (impl_generics, type_generics, where_clause) =
        trait_impl_parts(facade, node, trait_path.clone());
    match recipe {
        ConfiguredTraitRecipe::Debug => {
            let body = render_debug_body(node);
            quote! {
                impl #impl_generics ::core::fmt::Debug for #node_ident #type_generics #where_clause {
                    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        #body
                    }
                }
            }
        }
        ConfiguredTraitRecipe::Clone => {
            let body = render_clone_body(node);
            quote! {
                impl #impl_generics ::core::clone::Clone for #node_ident #type_generics #where_clause {
                    fn clone(&self) -> Self { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Copy => quote! {
            impl #impl_generics ::core::marker::Copy for #node_ident #type_generics #where_clause {}
        },
        ConfiguredTraitRecipe::PartialEq => {
            let body = render_partial_eq_body(node);
            quote! {
                impl #impl_generics ::core::cmp::PartialEq for #node_ident #type_generics #where_clause {
                    fn eq(&self, other: &Self) -> bool { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Eq => quote! {
            impl #impl_generics ::core::cmp::Eq for #node_ident #type_generics #where_clause {}
        },
        ConfiguredTraitRecipe::PartialOrd => {
            let body = render_order_body(node, true);
            quote! {
                impl #impl_generics ::core::cmp::PartialOrd for #node_ident #type_generics #where_clause {
                    fn partial_cmp(&self, other: &Self) -> Option<::core::cmp::Ordering> { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Ord => {
            let body = render_order_body(node, false);
            quote! {
                impl #impl_generics ::core::cmp::Ord for #node_ident #type_generics #where_clause {
                    fn cmp(&self, other: &Self) -> ::core::cmp::Ordering { #body }
                }
            }
        }
        ConfiguredTraitRecipe::Hash => {
            let body = render_hash_body(node);
            quote! {
                impl #impl_generics ::core::hash::Hash for #node_ident #type_generics #where_clause {
                    fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) { #body }
                }
            }
        }
        _ => unreachable!("grammar-aware recipes are rendered separately"),
    }
}

/// Builds semantic fields from frozen inputs without changing canonical generation order.
pub(super) fn semantic_fields(elements: &[ElementParsePlan]) -> Vec<&FieldPlan> {
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { field, .. } => Some(field),
            ElementParseKind::Fixed { .. } => None,
        })
        .collect()
}

/// Builds trait variants from frozen inputs without changing canonical generation order.
pub(super) fn trait_variants(body: &NodeBodyPlan) -> Vec<VariantParsePlan> {
    match body {
        NodeBodyPlan::Enum { variants, .. } => variants.clone(),
        NodeBodyPlan::Pratt(pratt) => {
            let mut variants = pratt
                .nud
                .iter()
                .map(|branch| (branch.source_index, branch.variant.clone()))
                .chain(pratt.led.iter().map(|branch| {
                    let mut variant = branch.variant.clone();
                    variant.elements.insert(0, branch.lhs.clone());
                    (branch.source_index, variant)
                }))
                .collect::<Vec<_>>();
            variants.sort_by_key(|(source_index, _)| *source_index);
            variants.into_iter().map(|(_, variant)| variant).collect()
        }
        NodeBodyPlan::Struct { .. } => unreachable!("struct Nodes do not have variants"),
    }
}

/// Renders render debug body from frozen inputs without changing canonical generation order.
pub(super) fn render_debug_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    let node_name = node.ident.unraw().to_string();
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = semantic_fields(elements);
            match style {
                ResolvedFieldStyle::Named => {
                    let entries = fields.iter().map(|field| {
                        let ident = field.ident.as_ref().expect("named field");
                        let name = ident.unraw().to_string();
                        let access = field_access(quote!(self), Some(ident), field.index);
                        quote!(debug.field(#name, &#access);)
                    });
                    quote! {
                        let mut debug = formatter.debug_struct(#node_name);
                        #(#entries)*
                        debug.finish()
                    }
                }
                ResolvedFieldStyle::Unnamed => {
                    let entries = fields.iter().map(|field| {
                        let access = field_access(quote!(self), None, field.index);
                        quote!(debug.field(&#access);)
                    });
                    quote! {
                        let mut debug = formatter.debug_tuple(#node_name);
                        #(#entries)*
                        debug.finish()
                    }
                }
                ResolvedFieldStyle::Unit => quote!(formatter.write_str(#node_name)),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().map(|variant| {
                let name = variant.ident.unraw().to_string();
                let fields = semantic_fields(&variant.elements);
                let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "debug");
                match variant.style {
                    ResolvedFieldStyle::Named => {
                        let entries = fields.iter().zip(&bindings).map(|(field, binding)| {
                            let field_name = field.ident.as_ref().expect("named field").unraw().to_string();
                            quote!(debug.field(#field_name, #binding);)
                        });
                        quote!(#pattern => { let mut debug = formatter.debug_struct(#name); #(#entries)* debug.finish() })
                    }
                    ResolvedFieldStyle::Unnamed => {
                        let entries = bindings.iter().map(|binding| quote!(debug.field(#binding);));
                        quote!(#pattern => { let mut debug = formatter.debug_tuple(#name); #(#entries)* debug.finish() })
                    }
                    ResolvedFieldStyle::Unit => quote!(#pattern => formatter.write_str(#name)),
                }
            });
            quote!(match self { #(#arms),* })
        }
    }
}

/// Builds variant bound pattern from frozen inputs without changing canonical generation order.
pub(super) fn variant_bound_pattern(
    node: &Ident,
    variant: &VariantParsePlan,
    prefix: &str,
) -> (proc_macro2::TokenStream, Vec<Ident>) {
    let fields = semantic_fields(&variant.elements);
    let bindings = fields
        .iter()
        .map(|field| format_ident!("__recursa_{prefix}_{}", field.index))
        .collect::<Vec<_>>();
    let variant_ident = &variant.ident;
    let pattern = match variant.style {
        ResolvedFieldStyle::Named => {
            let entries = fields.iter().zip(&bindings).map(|(field, binding)| {
                let ident = field.ident.as_ref().expect("named field");
                quote!(#ident: #binding)
            });
            quote!(#node::#variant_ident { #(#entries),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let field_count = fields
                .iter()
                .map(|field| field.index + 1)
                .max()
                .unwrap_or(0);
            let mut entries = (0..field_count).map(|_| quote!(_)).collect::<Vec<_>>();
            for (field, binding) in fields.iter().zip(&bindings) {
                entries[field.index] = quote!(#binding);
            }
            quote!(#node::#variant_ident(#(#entries),*))
        }
        ResolvedFieldStyle::Unit => quote!(#node::#variant_ident),
    };
    (pattern, bindings)
}

/// Renders render clone body from frozen inputs without changing canonical generation order.
pub(super) fn render_clone_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = semantic_fields(elements);
            let values = fields.iter().map(|field| {
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                quote!(::core::clone::Clone::clone(&#access))
            });
            match style {
                ResolvedFieldStyle::Named => {
                    let entries = fields.iter().zip(values).map(|(field, value)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #value)
                    });
                    quote!(Self { #(#entries),* })
                }
                ResolvedFieldStyle::Unnamed => quote!(Self(#(#values),*)),
                ResolvedFieldStyle::Unit => quote!(Self),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().map(|variant| {
                let fields = semantic_fields(&variant.elements);
                let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "clone");
                let values = bindings
                    .iter()
                    .map(|binding| quote!(::core::clone::Clone::clone(#binding)));
                let variant_ident = &variant.ident;
                let value = match variant.style {
                    ResolvedFieldStyle::Named => {
                        let entries = fields.iter().zip(values).map(|(field, value)| {
                            let ident = field.ident.as_ref().expect("named field");
                            quote!(#ident: #value)
                        });
                        quote!(Self::#variant_ident { #(#entries),* })
                    }
                    ResolvedFieldStyle::Unnamed => quote!(Self::#variant_ident(#(#values),*)),
                    ResolvedFieldStyle::Unit => quote!(Self::#variant_ident),
                };
                quote!(#pattern => #value)
            });
            quote!(match self { #(#arms),* })
        }
    }
}

/// Renders render partial eq body from frozen inputs without changing canonical generation order.
pub(super) fn render_partial_eq_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let comparisons = semantic_fields(elements).into_iter().map(|field| {
                let left = field_access(quote!(self), field.ident.as_ref(), field.index);
                let right = field_access(quote!(other), field.ident.as_ref(), field.index);
                quote!(::core::cmp::PartialEq::eq(&#left, &#right))
            });
            quote!(true #(&& #comparisons)*)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().map(|variant| {
                let (left_pattern, left) = variant_bound_pattern(&node.ident, variant, "eq_left");
                let (right_pattern, right) =
                    variant_bound_pattern(&node.ident, variant, "eq_right");
                let comparisons = left
                    .iter()
                    .zip(right)
                    .map(|(left, right)| quote!(::core::cmp::PartialEq::eq(#left, #right)));
                quote!((#left_pattern, #right_pattern) => true #(&& #comparisons)*)
            });
            quote!(match (self, other) { #(#arms,)* _ => false })
        }
    }
}

/// Renders render order body from frozen inputs without changing canonical generation order.
pub(super) fn render_order_body(node: &NodeParsePlan, partial: bool) -> proc_macro2::TokenStream {
    let compare_fields = |left: Vec<proc_macro2::TokenStream>,
                          right: Vec<proc_macro2::TokenStream>| {
        let comparisons = left.into_iter().zip(right).map(|(left, right)| {
            if partial {
                quote! {
                    match ::core::cmp::PartialOrd::partial_cmp(#left, #right) {
                        Some(::core::cmp::Ordering::Equal) => {}
                        ordering => return ordering,
                    }
                }
            } else {
                quote! {
                    match ::core::cmp::Ord::cmp(#left, #right) {
                        ::core::cmp::Ordering::Equal => {}
                        ordering => return ordering,
                    }
                }
            }
        });
        if partial {
            quote!({ #(#comparisons)* Some(::core::cmp::Ordering::Equal) })
        } else {
            quote!({ #(#comparisons)* ::core::cmp::Ordering::Equal })
        }
    };
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let fields = semantic_fields(elements);
            let left = fields
                .iter()
                .map(|field| {
                    let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                    quote!(&#access)
                })
                .collect();
            let right = fields
                .iter()
                .map(|field| {
                    let access = field_access(quote!(other), field.ident.as_ref(), field.index);
                    quote!(&#access)
                })
                .collect();
            compare_fields(left, right)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let same_variant = variants.iter().map(|variant| {
                let (left_pattern, left) = variant_bound_pattern(&node.ident, variant, "ord_left");
                let (right_pattern, right) =
                    variant_bound_pattern(&node.ident, variant, "ord_right");
                let left = left.iter().map(|binding| quote!(#binding)).collect();
                let right = right.iter().map(|binding| quote!(#binding)).collect();
                let body = compare_fields(left, right);
                quote!((#left_pattern, #right_pattern) => #body)
            });
            let left_indices = variants.iter().enumerate().map(|(index, variant)| {
                let (pattern, _) = variant_bound_pattern(&node.ident, variant, "ord_variant_left");
                quote!(#pattern => #index)
            });
            let right_indices = variants.iter().enumerate().map(|(index, variant)| {
                let (pattern, _) = variant_bound_pattern(&node.ident, variant, "ord_variant_right");
                quote!(#pattern => #index)
            });
            let differing = if partial {
                quote! {
                    let left = match self { #(#left_indices),* };
                    let right = match other { #(#right_indices),* };
                    Some(::core::cmp::Ord::cmp(&left, &right))
                }
            } else {
                quote! {
                    let left = match self { #(#left_indices),* };
                    let right = match other { #(#right_indices),* };
                    ::core::cmp::Ord::cmp(&left, &right)
                }
            };
            quote!(match (self, other) { #(#same_variant,)* _ => { #differing } })
        }
    }
}

/// Renders render hash body from frozen inputs without changing canonical generation order.
pub(super) fn render_hash_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let hashes = semantic_fields(elements).into_iter().map(|field| {
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                quote!(::core::hash::Hash::hash(&#access, state);)
            });
            quote!(#(#hashes)*)
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().enumerate().map(|(index, variant)| {
                let (pattern, bindings) = variant_bound_pattern(&node.ident, variant, "hash");
                let hashes = bindings
                    .iter()
                    .map(|binding| quote!(::core::hash::Hash::hash(#binding, state);));
                quote!(#pattern => { ::core::hash::Hash::hash(&#index, state); #(#hashes)* })
            });
            quote!(match self { #(#arms),* })
        }
    }
}

/// Renders render node arbitrary from frozen inputs without changing canonical generation order.
pub(super) fn render_node_arbitrary(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let mut generics = node.generics.clone();
    generics.params.insert(0, syn::parse_quote!('arbitrary));
    for field_type in semantic_field_types(facade, node) {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #field_type: #facade::__private::arbitrary::Arbitrary<'arbitrary>
            ));
    }
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, type_generics, _) = node.generics.split_for_impl();
    let arbitrary_value = || quote!(#facade::__private::arbitrary::Arbitrary::arbitrary(input)?);
    let body = match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = semantic_fields(elements);
            let values = fields.iter().map(|_| arbitrary_value());
            match style {
                ResolvedFieldStyle::Named => {
                    let entries = fields.iter().zip(values).map(|(field, value)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #value)
                    });
                    quote!(Self { #(#entries),* })
                }
                ResolvedFieldStyle::Unnamed => quote!(Self(#(#values),*)),
                ResolvedFieldStyle::Unit => quote!(Self),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let arms = variants.iter().enumerate().map(|(index, variant)| {
                let fields = semantic_fields(&variant.elements);
                let values = fields.iter().map(|_| arbitrary_value());
                let variant_ident = &variant.ident;
                let value = match variant.style {
                    ResolvedFieldStyle::Named => {
                        let entries = fields.iter().zip(values).map(|(field, value)| {
                            let ident = field.ident.as_ref().expect("named field");
                            quote!(#ident: #value)
                        });
                        quote!(Self::#variant_ident { #(#entries),* })
                    }
                    ResolvedFieldStyle::Unnamed => quote!(Self::#variant_ident(#(#values),*)),
                    ResolvedFieldStyle::Unit => quote!(Self::#variant_ident),
                };
                quote!(#index => #value)
            });
            let last = variants.len().saturating_sub(1);
            quote! {
                match input.int_in_range(0usize..=#last)? {
                    #(#arms,)*
                    _ => unreachable!("the arbitrary variant index is in range"),
                }
            }
        }
    };
    quote! {
        __recursa_if_arbitrary! {
        impl #impl_generics #facade::__private::arbitrary::Arbitrary<'arbitrary>
            for #node_ident #type_generics #where_clause
        {
            fn arbitrary(
                input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
            ) -> #facade::__private::arbitrary::Result<Self> {
                Ok(#body)
            }
        }
        }
    }
}

/// Renders render node serde from frozen inputs without changing canonical generation order.
///
/// # Long-function contract
///
/// Rationale: emits Serde as one recipe because bounds, helper shapes, and borrowed input must agree.
/// Invariants: authored generics and source-borrowing lifetimes are preserved without widening the public API.
/// Phases: derives helper bounds, renders serialization, then renders validated deserialization.
pub(super) fn render_node_serde(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let node_ident = &node.ident;
    let remote = node_ident.unraw().to_string();
    let helper = format_ident!(
        "__RecursaSerde_{}",
        stable_hash(node.semantic_key.as_bytes())
    );
    let serde_crate = format!("{}::__private::serde", facade.unraw());
    let mut derives = Vec::new();
    let serialize = node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Serialize);
    let deserialize = node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Deserialize);
    if serialize {
        derives.push(quote!(#facade::__private::serde::Serialize));
    }
    if deserialize {
        derives.push(quote!(#facade::__private::serde::Deserialize));
    }
    let generics = &node.generics;
    let (_, node_type_generics, _) = generics.split_for_impl();
    let recursive_type: Type = syn::parse_quote!(#node_ident #node_type_generics);
    let field_type = |terminal: &Type, wrappers: &[FieldWrapper]| {
        let terminal = if is_recursive_terminal(node, terminal) {
            &recursive_type
        } else {
            terminal
        };
        render_wrapped_type(facade, terminal, wrappers)
    };
    let helper_body = match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = elements.iter().filter_map(|element| {
                let ElementParseKind::Semantic {
                    field,
                    terminal,
                    wrappers,
                    ..
                } = &element.kind
                else {
                    return None;
                };
                Some((field, field_type(terminal, wrappers)))
            });
            match style {
                ResolvedFieldStyle::Named => {
                    let fields = fields.map(|(field, ty)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #ty)
                    });
                    quote!(struct #helper #generics { #(#fields),* })
                }
                ResolvedFieldStyle::Unnamed => {
                    let fields = fields.map(|(_, ty)| quote!(#ty));
                    quote!(struct #helper #generics (#(#fields),*);)
                }
                ResolvedFieldStyle::Unit => quote!(struct #helper #generics;),
            }
        }
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
            let variants = trait_variants(&node.body);
            let variants = variants.iter().map(|variant| {
                let ident = &variant.ident;
                let fields = variant.elements.iter().filter_map(|element| {
                    let ElementParseKind::Semantic {
                        field,
                        terminal,
                        wrappers,
                        ..
                    } = &element.kind
                    else {
                        return None;
                    };
                    Some((field, field_type(terminal, wrappers)))
                });
                match variant.style {
                    ResolvedFieldStyle::Named => {
                        let fields = fields.map(|(field, ty)| {
                            let field_ident = field.ident.as_ref().expect("named field");
                            quote!(#field_ident: #ty)
                        });
                        quote!(#ident { #(#fields),* })
                    }
                    ResolvedFieldStyle::Unnamed => {
                        let fields = fields.map(|(_, ty)| quote!(#ty));
                        quote!(#ident(#(#fields),*))
                    }
                    ResolvedFieldStyle::Unit => quote!(#ident),
                }
            });
            quote!(enum #helper #generics { #(#variants),* })
        }
    };
    let helper_definition = quote! {
        #[derive(#(#derives),*)]
        #[serde(remote = #remote, crate = #serde_crate)]
        #[allow(non_camel_case_types)]
        #helper_body
    };
    let serialize_impl = serialize.then(|| {
        let mut impl_generics = node.generics.clone();
        add_serde_bounds(
            facade,
            node,
            &mut impl_generics,
            quote!(#facade::__private::serde::Serialize),
        );
        let (impl_generics, _, where_clause) = impl_generics.split_for_impl();
        let (_, type_generics, _) = node.generics.split_for_impl();
        quote! {
            impl #impl_generics #facade::__private::serde::Serialize
                for #node_ident #type_generics #where_clause
            {
                fn serialize<S: #facade::__private::serde::Serializer>(
                    &self,
                    serializer: S,
                ) -> Result<S::Ok, S::Error> {
                    #helper::serialize(self, serializer)
                }
            }
        }
    });
    let deserialize_impl = deserialize.then(|| {
        let mut impl_generics = node.generics.clone();
        impl_generics
            .params
            .insert(0, syn::parse_quote!('__recursa_de));
        add_serde_bounds(
            facade,
            node,
            &mut impl_generics,
            quote!(#facade::__private::serde::Deserialize<'__recursa_de>),
        );
        let (impl_generics, _, where_clause) = impl_generics.split_for_impl();
        let (_, type_generics, _) = node.generics.split_for_impl();
        quote! {
            impl #impl_generics #facade::__private::serde::Deserialize<'__recursa_de>
                for #node_ident #type_generics #where_clause
            {
                fn deserialize<D: #facade::__private::serde::Deserializer<'__recursa_de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    #helper::deserialize(deserializer)
                }
            }
        }
    });
    quote! {
        __recursa_if_serde! {
        #helper_definition
        #serialize_impl
        #deserialize_impl
        }
    }
}

/// Builds add serde bounds from frozen inputs without changing canonical generation order.
pub(super) fn add_serde_bounds(
    facade: &Ident,
    node: &NodeParsePlan,
    generics: &mut Generics,
    bound: proc_macro2::TokenStream,
) {
    for field_type in semantic_field_types(facade, node)
        .into_iter()
        .chain(serde_generic_dependencies(node))
    {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #bound));
    }
}

/// Builds serde generic dependencies from frozen inputs without changing canonical generation order.
pub(super) fn serde_generic_dependencies(node: &NodeParsePlan) -> Vec<Type> {
    let candidates = node
        .generics
        .type_params()
        .map(|parameter| parameter.ident.unraw().to_string())
        .collect::<BTreeSet<_>>();
    let mut dependencies = BTreeSet::new();
    /// Collects declared generic parameters reached by a Serde field type.
    struct DependencyVisitor<'a> {
        /// Candidate generic parameter names.
        candidates: &'a BTreeSet<String>,
        /// Canonical set of referenced generic parameter names.
        dependencies: &'a mut BTreeSet<String>,
    }
    impl<'ast> syn::visit::Visit<'ast> for DependencyVisitor<'_> {
        fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && let Some(segment) = path.path.segments.first()
            {
                let name = segment.ident.unraw().to_string();
                if self.candidates.contains(&name) {
                    self.dependencies.insert(name);
                }
            }
            syn_visit::visit_type_path(self, path);
        }
    }
    for_each_element(&node.body, &mut |element| {
        let ElementParseKind::Semantic { terminal, .. } = &element.kind else {
            return;
        };
        if !is_recursive_terminal(node, terminal) {
            DependencyVisitor {
                candidates: &candidates,
                dependencies: &mut dependencies,
            }
            .visit_type(terminal);
        }
    });
    node.generics
        .type_params()
        .filter(|parameter| dependencies.contains(&parameter.ident.unraw().to_string()))
        .map(|parameter| {
            let ident = &parameter.ident;
            syn::parse_quote!(#ident)
        })
        .collect()
}

/// Builds semantic field types from frozen inputs without changing canonical generation order.
pub(super) fn semantic_field_types(facade: &Ident, node: &NodeParsePlan) -> Vec<Type> {
    let elements = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => elements.clone(),
        NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => trait_variants(&node.body)
            .into_iter()
            .flat_map(|variant| variant.elements)
            .collect(),
    };
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic {
                terminal, wrappers, ..
            } if !is_recursive_terminal(node, terminal) => {
                Some(render_wrapped_type(facade, terminal, wrappers))
            }
            ElementParseKind::Semantic { .. } => None,
            ElementParseKind::Fixed { .. } => None,
        })
        .collect()
}

/// Builds is recursive terminal from frozen inputs without changing canonical generation order.
pub(super) fn is_recursive_terminal(node: &NodeParsePlan, terminal: &Type) -> bool {
    let Type::Path(terminal) = terminal else {
        return false;
    };
    let segments = terminal
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect::<Vec<_>>();
    if segments.last().is_some_and(|segment| segment == "Self") {
        return true;
    }
    let segments = segments
        .strip_prefix(&["crate".to_owned()])
        .unwrap_or(&segments);
    segments.join("::") == node.semantic_key
}
