//! Configured-trait planning adapters and generated implementations.
//!
//! Standard traits, Arbitrary, and Serde share one recipe-owned boundary. Pretty and
//! Visit remain capability modules and are invoked here only through their renderers.

use super::*;

/// Structural value traits intrinsic to every generated content-token type.
///
/// These traits let ordinary Rust derives on authored AST Nodes compose through generated token
/// fields. They are emitted independently of optional configured-trait recipes, but remain part
/// of trait-family identity and debug metadata.
pub(super) const INTRINSIC_CONTENT_TOKEN_VALUE_TRAITS: [ConfiguredTraitRecipe; 7] = [
    ConfiguredTraitRecipe::Debug,
    ConfiguredTraitRecipe::Clone,
    ConfiguredTraitRecipe::PartialEq,
    ConfiguredTraitRecipe::Eq,
    ConfiguredTraitRecipe::PartialOrd,
    ConfiguredTraitRecipe::Ord,
    ConfiguredTraitRecipe::Hash,
];

/// Optional recipes supported privately when a locally configured Node owns a hidden token field.
pub(super) const PRIVATE_CONTENT_TOKEN_SUPPORT_TRAITS: [ConfiguredTraitRecipe; 3] = [
    ConfiguredTraitRecipe::Arbitrary,
    ConfiguredTraitRecipe::Serialize,
    ConfiguredTraitRecipe::Deserialize,
];

/// Reports whether a locally configured Node needs one hidden token adapter.
pub(super) fn content_token_support_required(
    plan: &EmissionPlan,
    token: ContentTokenId,
    recipe: ConfiguredTraitRecipe,
) -> bool {
    content_token_support_required_in(&plan.nodes, token, recipe)
}

/// Reports private hidden-token support directly from the frozen Node plans.
pub(super) fn content_token_support_required_in(
    nodes: &[NodeParsePlan],
    token: ContentTokenId,
    recipe: ConfiguredTraitRecipe,
) -> bool {
    nodes.iter().any(|node| {
        if !node.configured_traits.contains(&recipe) {
            return false;
        }
        let mut required = false;
        for_each_element(&node.body, &mut |element| {
            if let ElementParseKind::Semantic { projection, .. } = &element.kind
                && matches!(
                    projection.as_ref(),
                    SemanticParseProjection::Direct {
                        explorer_terminal: ExplorerTerminalPlan::Content(candidate),
                        ..
                    } if *candidate == token
                )
            {
                required = true;
            }
        });
        required
    })
}

/// Renders feature-gated private adapters without changing public token trait selections.
pub(super) fn render_content_token_optional_trait_helpers(
    plan: &EmissionPlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let arbitrary = plan.tokens.iter().any(|token| {
        content_token_support_required(plan, token.semantic, ConfiguredTraitRecipe::Arbitrary)
    });
    let serialize = plan.tokens.iter().any(|token| {
        content_token_support_required(plan, token.semantic, ConfiguredTraitRecipe::Serialize)
    });
    let deserialize = plan.tokens.iter().any(|token| {
        content_token_support_required(plan, token.semantic, ConfiguredTraitRecipe::Deserialize)
    });
    let arbitrary = arbitrary.then(|| {
        quote! {
            __recursa_if_arbitrary! {
                #[doc(hidden)]
                pub(crate) trait __RecursaArbitraryWith<'arbitrary>: Sized {
                    fn __recursa_arbitrary_with(
                        input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    ) -> #facade::__private::arbitrary::Result<Self>;
                }

                impl<'arbitrary, T> __RecursaArbitraryWith<'arbitrary> for Box<T>
                where
                    T: __RecursaArbitraryWith<'arbitrary>,
                {
                    fn __recursa_arbitrary_with(
                        input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    ) -> #facade::__private::arbitrary::Result<Self> {
                        T::__recursa_arbitrary_with(input).map(Box::new)
                    }
                }

                impl<'arbitrary, T> __RecursaArbitraryWith<'arbitrary> for Option<T>
                where
                    T: __RecursaArbitraryWith<'arbitrary>,
                {
                    fn __recursa_arbitrary_with(
                        input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    ) -> #facade::__private::arbitrary::Result<Self> {
                        T::__recursa_arbitrary_with(input).map(Some)
                    }
                }

                impl<'arbitrary, T> __RecursaArbitraryWith<'arbitrary> for Vec<T>
                where
                    T: __RecursaArbitraryWith<'arbitrary>,
                {
                    fn __recursa_arbitrary_with(
                        input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    ) -> #facade::__private::arbitrary::Result<Self> {
                        T::__recursa_arbitrary_with(input).map(|value| vec![value])
                    }
                }

                impl<'arbitrary, T> __RecursaArbitraryWith<'arbitrary> for #facade::Vec1<T>
                where
                    T: __RecursaArbitraryWith<'arbitrary>,
                {
                    fn __recursa_arbitrary_with(
                        input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    ) -> #facade::__private::arbitrary::Result<Self> {
                        T::__recursa_arbitrary_with(input).map(#facade::Vec1::new)
                    }
                }
            }
        }
    });
    let serde = (serialize || deserialize).then(|| {
        let serialize = serialize.then(|| {
            quote! {
                #[doc(hidden)]
                pub(crate) trait __RecursaSerializeWith {
                    fn __recursa_serialize_with<S>(
                        &self,
                        serializer: S,
                    ) -> Result<S::Ok, S::Error>
                    where
                        S: #facade::__private::serde::Serializer;
                }

                impl<T> __RecursaSerializeWith for Box<T>
                where
                    T: __RecursaSerializeWith,
                {
                    fn __recursa_serialize_with<S>(
                        &self,
                        serializer: S,
                    ) -> Result<S::Ok, S::Error>
                    where
                        S: #facade::__private::serde::Serializer,
                    {
                        (**self).__recursa_serialize_with(serializer)
                    }
                }

                struct __RecursaSerializeRef<'value, T>(&'value T);

                impl<T> #facade::__private::serde::Serialize for __RecursaSerializeRef<'_, T>
                where
                    T: __RecursaSerializeWith,
                {
                    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                    where
                        S: #facade::__private::serde::Serializer,
                    {
                        self.0.__recursa_serialize_with(serializer)
                    }
                }

                impl<T> __RecursaSerializeWith for Option<T>
                where
                    T: __RecursaSerializeWith,
                {
                    fn __recursa_serialize_with<S>(
                        &self,
                        serializer: S,
                    ) -> Result<S::Ok, S::Error>
                    where
                        S: #facade::__private::serde::Serializer,
                    {
                        match self {
                            Some(value) => #facade::__private::serde::Serializer::serialize_some(
                                serializer,
                                &__RecursaSerializeRef(value),
                            ),
                            None => #facade::__private::serde::Serializer::serialize_none(serializer),
                        }
                    }
                }

                impl<T> __RecursaSerializeWith for Vec<T>
                where
                    T: __RecursaSerializeWith,
                {
                    fn __recursa_serialize_with<S>(
                        &self,
                        serializer: S,
                    ) -> Result<S::Ok, S::Error>
                    where
                        S: #facade::__private::serde::Serializer,
                    {
                        let mut sequence = #facade::__private::serde::Serializer::serialize_seq(
                            serializer,
                            Some(self.len()),
                        )?;
                        for value in self {
                            #facade::__private::serde::ser::SerializeSeq::serialize_element(
                                &mut sequence,
                                &__RecursaSerializeRef(value),
                            )?;
                        }
                        #facade::__private::serde::ser::SerializeSeq::end(sequence)
                    }
                }

                impl<T> __RecursaSerializeWith for #facade::Vec1<T>
                where
                    T: __RecursaSerializeWith,
                {
                    fn __recursa_serialize_with<S>(
                        &self,
                        serializer: S,
                    ) -> Result<S::Ok, S::Error>
                    where
                        S: #facade::__private::serde::Serializer,
                    {
                        let mut sequence = #facade::__private::serde::Serializer::serialize_seq(
                            serializer,
                            Some(self.len()),
                        )?;
                        for value in self.iter() {
                            #facade::__private::serde::ser::SerializeSeq::serialize_element(
                                &mut sequence,
                                &__RecursaSerializeRef(value),
                            )?;
                        }
                        #facade::__private::serde::ser::SerializeSeq::end(sequence)
                    }
                }

                pub(crate) fn __recursa_serialize_content<T, S>(
                    value: &T,
                    serializer: S,
                ) -> Result<S::Ok, S::Error>
                where
                    T: __RecursaSerializeWith,
                    S: #facade::__private::serde::Serializer,
                {
                    value.__recursa_serialize_with(serializer)
                }
            }
        });
        let deserialize = deserialize.then(|| {
            quote! {
                #[doc(hidden)]
                pub(crate) trait __RecursaDeserializeWith<'de>: Sized {
                    fn __recursa_deserialize_with<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>;
                }

                impl<'de, T> __RecursaDeserializeWith<'de> for Box<T>
                where
                    T: __RecursaDeserializeWith<'de>,
                {
                    fn __recursa_deserialize_with<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>,
                    {
                        T::__recursa_deserialize_with(deserializer).map(Box::new)
                    }
                }

                struct __RecursaDeserializeSeed<T>(::core::marker::PhantomData<fn() -> T>);

                impl<'de, T> #facade::__private::serde::de::DeserializeSeed<'de>
                    for __RecursaDeserializeSeed<T>
                where
                    T: __RecursaDeserializeWith<'de>,
                {
                    type Value = T;

                    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>,
                    {
                        T::__recursa_deserialize_with(deserializer)
                    }
                }

                struct __RecursaOptionVisitor<T>(::core::marker::PhantomData<fn() -> T>);

                impl<'de, T> #facade::__private::serde::de::Visitor<'de>
                    for __RecursaOptionVisitor<T>
                where
                    T: __RecursaDeserializeWith<'de>,
                {
                    type Value = Option<T>;

                    fn expecting(
                        &self,
                        formatter: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        formatter.write_str("an optional content token")
                    }

                    fn visit_none<E>(self) -> Result<Self::Value, E> {
                        Ok(None)
                    }

                    fn visit_unit<E>(self) -> Result<Self::Value, E> {
                        Ok(None)
                    }

                    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>,
                    {
                        T::__recursa_deserialize_with(deserializer).map(Some)
                    }
                }

                impl<'de, T> __RecursaDeserializeWith<'de> for Option<T>
                where
                    T: __RecursaDeserializeWith<'de>,
                {
                    fn __recursa_deserialize_with<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>,
                    {
                        #facade::__private::serde::Deserializer::deserialize_option(
                            deserializer,
                            __RecursaOptionVisitor(::core::marker::PhantomData),
                        )
                    }
                }

                struct __RecursaSequenceVisitor<T>(::core::marker::PhantomData<fn() -> T>);

                impl<'de, T> #facade::__private::serde::de::Visitor<'de>
                    for __RecursaSequenceVisitor<T>
                where
                    T: __RecursaDeserializeWith<'de>,
                {
                    type Value = Vec<T>;

                    fn expecting(
                        &self,
                        formatter: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        formatter.write_str("a sequence of content tokens")
                    }

                    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
                    where
                        A: #facade::__private::serde::de::SeqAccess<'de>,
                    {
                        let mut values = Vec::new();
                        while let Some(value) =
                            #facade::__private::serde::de::SeqAccess::next_element_seed(
                                &mut sequence,
                                __RecursaDeserializeSeed(::core::marker::PhantomData),
                            )?
                        {
                            values.push(value);
                        }
                        Ok(values)
                    }
                }

                impl<'de, T> __RecursaDeserializeWith<'de> for Vec<T>
                where
                    T: __RecursaDeserializeWith<'de>,
                {
                    fn __recursa_deserialize_with<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>,
                    {
                        #facade::__private::serde::Deserializer::deserialize_seq(
                            deserializer,
                            __RecursaSequenceVisitor(::core::marker::PhantomData),
                        )
                    }
                }

                impl<'de, T> __RecursaDeserializeWith<'de> for #facade::Vec1<T>
                where
                    T: __RecursaDeserializeWith<'de>,
                {
                    fn __recursa_deserialize_with<D>(deserializer: D) -> Result<Self, D::Error>
                    where
                        D: #facade::__private::serde::Deserializer<'de>,
                    {
                        let values = <Vec<T> as __RecursaDeserializeWith<'de>>
                            ::__recursa_deserialize_with(deserializer)?;
                        let mut values = values.into_iter();
                        let Some(first) = values.next() else {
                            return Err(#facade::__private::serde::de::Error::custom(
                                "expected at least one content token",
                            ));
                        };
                        let mut result = #facade::Vec1::new(first);
                        for value in values {
                            result.push(value);
                        }
                        Ok(result)
                    }
                }

                pub(crate) fn __recursa_deserialize_content<'de, T, D>(
                    deserializer: D,
                ) -> Result<T, D::Error>
                where
                    T: __RecursaDeserializeWith<'de>,
                    D: #facade::__private::serde::Deserializer<'de>,
                {
                    T::__recursa_deserialize_with(deserializer)
                }
            }
        });
        quote! {
            __recursa_if_serde! {
                #serialize
                #deserialize
            }
        }
    });
    quote!(#arbitrary #serde)
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::super::test_support;
    #[test]
    fn arbitrary_identity_collision_is_preserved_and_rejected_without_replacement() {
        test_support::arbitrary_identity_collision_is_preserved_and_rejected_without_replacement();
    }
}

/// Renders the configured-trait implementations selected for one Node.
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
    // An arena_ast Node's own field types (`ArenaBox`/`ArenaVec`/`ArenaVec1`) cannot implement
    // `arbitrary::Arbitrary` directly (its `arbitrary(u)` has no channel for a `&Bump` handle) —
    // Arbitrary support for an arena_ast grammar is instead provided by a separate, always-heap
    // "seed" mirror type plus mechanical materialization into the arena (see `arbitrary_seed.rs`
    // and `render_arbitrary_seed_module`), not a direct impl on this Node itself.
    if !plan.arena_ast
        && node
            .configured_traits
            .contains(&ConfiguredTraitRecipe::Arbitrary)
    {
        implementations.push(render_node_arbitrary(plan, node));
        let name = |node: NodeId| super::table_driven::nested_node_path(plan, node);
        implementations.push(
            super::rejections::render_arbitrary_support(
                plan,
                node,
                &super::rejections::NestedNaming::Value(&name),
            )
            .unwrap_or_else(|error| quote!(::core::compile_error!(#error);)),
        );
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
    if node
        .configured_traits
        .contains(&ConfiguredTraitRecipe::Requires)
    {
        implementations.push(render_node_requires(plan, node, node_index));
    }
    quote!(#(#implementations)*)
}

/// Splits a Node's generics and augments its where-clause for one generated trait.
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

/// Prepares `Pretty` implementation generics without imposing recursive self bounds.
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
        NodeBodyPlan::Enum { .. } => trait_variants(&node.body)
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

/// Reports whether a semantic type references any generic type parameter owned by the Node.
pub(super) fn type_depends_on_node_type_parameters(ty: &Type, node: &NodeParsePlan) -> bool {
    let candidates = node
        .generics
        .type_params()
        .map(|parameter| parameter.ident.unraw().to_string())
        .chain(
            node.generics
                .const_params()
                .map(|parameter| parameter.ident.unraw().to_string()),
        )
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

        fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && let Some(segment) = path.path.segments.first()
                && self.candidates.contains(&segment.ident.unraw().to_string())
            {
                self.depends = true;
            }
            syn_visit::visit_expr_path(self, path);
        }
    }

    let mut visitor = DependencyVisitor {
        candidates: &candidates,
        depends: false,
    };
    visitor.visit_type(ty);
    visitor.depends
}

/// Resolves an attachment terminal to the fixed spelling emitted by `Pretty`.
pub(super) fn attachment_pretty_terminal(
    node: &NodeParsePlan,
    element: &ElementParsePlan,
) -> Option<Type> {
    match &element.kind {
        ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
            SemanticParseProjection::Direct {
                terminal,
                attachment: Some(_),
                opaque_provenance: true,
                ..
            } if !is_recursive_terminal(node, terminal) => Some(terminal.as_ref().clone()),
            SemanticParseProjection::Direct { .. }
            | SemanticParseProjection::FixedPresence { .. } => None,
        },
        ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
    }
}

/// Renders one catalogued standard-trait implementation over semantic fields only.
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

/// Iterates semantic fields in source order, excluding fixed-token elements.
pub(super) fn semantic_fields(elements: &[ElementParsePlan]) -> Vec<&FieldPlan> {
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { field, .. } => Some(field),
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .collect()
}

/// Borrows semantic fields together with the projections needed by capability renderers.
fn semantic_fields_with_projections(
    elements: &[ElementParsePlan],
) -> Vec<(&FieldPlan, &SemanticParseProjection)> {
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { field, projection } => Some((field, projection.as_ref())),
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .collect()
}

/// Returns the struct or enum variant shapes participating in structural trait behavior.
pub(super) fn trait_variants(body: &NodeBodyPlan) -> Vec<VariantParsePlan> {
    match body {
        NodeBodyPlan::Enum { variants, .. } => variants.clone(),
        NodeBodyPlan::Struct { .. } => unreachable!("struct Nodes do not have variants"),
    }
}

/// Renders `Debug` output that mirrors the authored semantic Node shape.
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
        NodeBodyPlan::Enum { .. } => {
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

/// Builds a variant pattern binding only semantic values required by a trait body.
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

/// Renders structural cloning of semantic fields while reconstructing fixed syntax implicitly.
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
        NodeBodyPlan::Enum { .. } => {
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

/// Renders semantic-field equality for each matching Node shape.
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
        NodeBodyPlan::Enum { .. } => {
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

/// Renders deterministic lexicographic comparison across variants and semantic fields.
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
        NodeBodyPlan::Enum { .. } => {
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

/// Renders variant-discriminant and semantic-field hashing for the authored Node shape.
pub(super) fn render_hash_body(node: &NodeParsePlan) -> proc_macro2::TokenStream {
    match &node.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let hashes = semantic_fields(elements).into_iter().map(|field| {
                let access = field_access(quote!(self), field.ident.as_ref(), field.index);
                quote!(::core::hash::Hash::hash(&#access, state);)
            });
            quote!(#(#hashes)*)
        }
        NodeBodyPlan::Enum { .. } => {
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

/// Reports whether a variant has any Node-typed element — a variant with none of these can
/// always be constructed without recursing into another generated `arbitrary()` call, so it is
/// safe to prefer once sampling depth is bounded (see `render_node_arbitrary`'s enum case).
fn variant_has_node_element(variant: &VariantParsePlan) -> bool {
    variant.elements.iter().any(|element| {
        let ElementParseKind::Semantic { projection, .. } = &element.kind else {
            return false;
        };
        matches!(
            projection.as_ref(),
            SemanticParseProjection::Direct {
                explorer_terminal: ExplorerTerminalPlan::Node(_),
                ..
            }
        )
    })
}

/// Renders bounded arbitrary construction for the Node's semantic shape.
pub(super) fn render_node_arbitrary(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let node_ident = &node.ident;
    let mut generics = node.generics.clone();
    generics.params.insert(0, syn::parse_quote!('arbitrary));
    // Only the node's own generic type parameters need a `where` bound here — a concrete field
    // type (the overwhelming common case) already has whatever `Arbitrary` impl it has, and
    // restating that as an explicit predicate creates a genuine infinite trait-solver cycle for
    // any mutually-recursive pair of Nodes (see `generic_type_dependencies`'s doc comment).
    for field_type in generic_type_dependencies(node) {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #field_type: #facade::__private::arbitrary::Arbitrary<'arbitrary>
            ));
    }
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, type_generics, _) = node.generics.split_for_impl();
    let arbitrary_value = |projection: &SemanticParseProjection| match projection {
        // A field that names a restricted expression samples only what that nonterminal
        // derives (`docs/declared-precedence.md`, "Restricted expressions"), so a sampled value
        // always has a rendering the site parses.
        SemanticParseProjection::Direct {
            terminal,
            terminal_node: Some(base),
            wrappers,
            restriction: Some(restriction),
            ..
        } if restriction_samples(plan, *base, *restriction) => {
            render_restricted_arbitrary_value(facade, terminal, wrappers, *restriction)
        }
        SemanticParseProjection::Direct {
            terminal,
            wrappers,
            explorer_terminal: ExplorerTerminalPlan::Content(_),
            ..
        } => {
            let field_type = render_wrapped_type(facade, terminal, wrappers);
            quote! {
                <#field_type as #module_anchor::__RecursaArbitraryWith<'arbitrary>>
                    ::__recursa_arbitrary_with(input)?
            }
        }
        SemanticParseProjection::Direct {
            terminal,
            terminal_node,
            wrappers,
            ..
        } => {
            // A sampled repetition of an element type with declared rejection rules keeps only
            // the items that parse back, so the sampler runs the parser's own walk over them and
            // drops every item that would complete a forbidden pair.
            let sampled = quote!(#facade::__private::arbitrary::Arbitrary::arbitrary(input)?);
            // A struct with text rules samples only values that parse back, or answers
            // `IncorrectFormat` when its retries run out. An optional field of such a struct is
            // then absent rather than unsampleable, so a rule that refuses most values never
            // starves the Node that holds it.
            if let (Some(node), [FieldWrapper::Optional, rest @ ..]) =
                (terminal_node, wrappers.as_slice())
                && plan.rejections.struct_rules(*node).is_some()
                && rest
                    .iter()
                    .all(|wrapper| matches!(wrapper, FieldWrapper::Boxed))
            {
                let inner = render_wrapped_type(facade, terminal, rest);
                return quote! {
                    if <bool as #facade::__private::arbitrary::Arbitrary>::arbitrary(input)? {
                        <#inner as #facade::__private::arbitrary::Arbitrary>::arbitrary(input).ok()
                    } else {
                        ::core::option::Option::None
                    }
                };
            }
            let rules = terminal_node
                .filter(|node| plan.rejections.list_rules(*node).is_some())
                .map(|_| quote!(<#terminal>::__recursa_rejection_rules()));
            rules
                .and_then(|rules| {
                    // The filter binds the sampled collection to a local, which the plain call
                    // in field position never did, so the field's own type is named here to keep
                    // the sampling call inferable.
                    let field_type = render_wrapped_type(facade, terminal, wrappers);
                    let typed = quote! {
                        {
                            let __recursa_field: #field_type = #sampled;
                            __recursa_field
                        }
                    };
                    super::rejections::render_arbitrary_filter(facade, wrappers, &rules, typed)
                })
                .unwrap_or(sampled)
        }
        SemanticParseProjection::FixedPresence { .. } => {
            quote!(#facade::__private::arbitrary::Arbitrary::arbitrary(input)?)
        }
    };
    // Renders the choice of one variant among `selected` (indices into the enum's variants) and
    // its construction. Inside a restricted sampler, `restricted` names the restriction, and an
    // operand that the restriction retypes samples that restriction again, as the lowering
    // parses it.
    let enum_body = |selected: &[usize], restricted: Option<(u16, &PlannedRestriction)>| {
        if selected.is_empty() {
            return quote!(return Err(#facade::__private::arbitrary::Error::IncorrectFormat));
        }
        let variants = trait_variants(&node.body);
        let arms = selected.iter().enumerate().map(|(choice, &index)| {
            let variant = &variants[index];
            let edges = restricted.map(|(_, restriction)| {
                let edges = restricted_edges(variant, node.node);
                (
                    edges.first,
                    edges.last.filter(|_| !restriction.leading_only),
                )
            });
            let values = variant
                .elements
                .iter()
                .enumerate()
                .filter_map(|(element_index, element)| {
                    let ElementParseKind::Semantic { field, projection } = &element.kind else {
                        return None;
                    };
                    let edge = edges.is_some_and(|(first, last)| {
                        first == Some(element_index) || last == Some(element_index)
                    });
                    let value = match (edge, restricted, projection.as_ref()) {
                        (
                            true,
                            Some((restriction, _)),
                            SemanticParseProjection::Direct {
                                terminal, wrappers, ..
                            },
                        ) => render_restricted_arbitrary_value(
                            facade,
                            terminal,
                            wrappers,
                            restriction,
                        ),
                        _ => arbitrary_value(projection),
                    };
                    Some((field, value))
                })
                .collect::<Vec<_>>();
            let variant_ident = &variant.ident;
            let value = match variant.style {
                ResolvedFieldStyle::Named => {
                    let entries = values.iter().map(|(field, value)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #value)
                    });
                    quote!(Self::#variant_ident { #(#entries),* })
                }
                ResolvedFieldStyle::Unnamed => {
                    let values = values.iter().map(|(_, value)| value);
                    quote!(Self::#variant_ident(#(#values),*))
                }
                ResolvedFieldStyle::Unit => quote!(Self::#variant_ident),
            };
            quote!(#choice => #value)
        });
        let last = selected.len().saturating_sub(1);
        // `int_in_range` deterministically returns the range's lower bound once the input is
        // exhausted (confirmed empirically, not merely a rare edge case), so a plain uniform
        // choice can deterministically re-select a recursive variant forever once a
        // mutually-recursive grammar (any real one) runs out of entropy. Past a bounded
        // sampling depth, prefer a variant with no Node-typed element at all — one that can
        // always be constructed without recursing further — falling back to the plain choice
        // when there is nothing to gain from restricting it (every variant already recurses,
        // or none does).
        let leaf_indices = selected
            .iter()
            .enumerate()
            .filter(|(_, index)| !variant_has_node_element(&variants[**index]))
            .map(|(choice, _)| choice)
            .collect::<Vec<_>>();
        let variant_index = if leaf_indices.len() == selected.len() {
            // Every variant is already a leaf (never recurses) — nothing to restrict.
            quote!(input.int_in_range(0usize..=#last)?)
        } else if leaf_indices.is_empty() {
            // No variant can be constructed without recursing into another Node at all (e.g.
            // even an "atom" case wraps a dedicated Node type rather than a bare content
            // token) — there is no variant to prefer, so the only way to guarantee
            // termination once already deep is to refuse to go deeper. This is not merely a
            // deeper search: `int_in_range` deterministically returns the same variant index
            // forever once entropy is exhausted, so without this the recursion genuinely never
            // ends rather than just becoming unlikely.
            quote! {
                {
                    if __recursa_arbitrary_depth >= #facade::__private::ARBITRARY_MAX_DEPTH {
                        return Err(#facade::__private::arbitrary::Error::NotEnoughData);
                    }
                    input.int_in_range(0usize..=#last)?
                }
            }
        } else {
            let leaf_count = leaf_indices.len();
            let leaf_upper = leaf_count - 1;
            quote! {
                if __recursa_arbitrary_depth >= #facade::__private::ARBITRARY_MAX_DEPTH {
                    const __RECURSA_LEAF_VARIANTS: [usize; #leaf_count] = [#(#leaf_indices),*];
                    __RECURSA_LEAF_VARIANTS[input.int_in_range(0usize..=#leaf_upper)?]
                } else {
                    input.int_in_range(0usize..=#last)?
                }
            }
        };
        quote! {
            match #variant_index {
                #(#arms,)*
                _ => unreachable!("the arbitrary variant index is in range"),
            }
        }
    };
    let body = match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = semantic_fields_with_projections(elements);
            let values = fields
                .iter()
                .map(|(_, projection)| arbitrary_value(projection));
            match style {
                ResolvedFieldStyle::Named => {
                    let entries = fields.iter().zip(values).map(|((field, _), value)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#ident: #value)
                    });
                    quote!(Self { #(#entries),* })
                }
                ResolvedFieldStyle::Unnamed => quote!(Self(#(#values),*)),
                ResolvedFieldStyle::Unit => quote!(Self),
            }
        }
        NodeBodyPlan::Enum { variants } => {
            // A variant that a declared rule refuses never parses back, so it is never sampled,
            // and a variant that `#[omit(Arbitrary)]` names is not sampled either.
            let all = (0..variants.len())
                .filter(|index| {
                    let variant = variants[*index].resolved_variant;
                    !plan.validations.refuses_variant(node.node, variant)
                        && !plan.unsampled.omits(node.node, variant)
                })
                .collect::<Vec<_>>();
            enum_body(&all, None)
        }
    };
    // One sampler per restricted expression of this base: the admitted variants only, with the
    // restriction applied again to every operand the lowering retypes.
    let restrictions = plan.restrictions.of(node.node);
    let restricted_sampler = (matches!(node.body, NodeBodyPlan::Enum { .. })
        && (0..restrictions.len()).any(|index| {
            restriction_samples(plan, node.node, u16::try_from(index).unwrap_or(u16::MAX))
        }))
    .then(|| {
        let arms = restrictions.iter().enumerate().map(|(index, restriction)| {
            let index = u16::try_from(index).expect("a restriction index fits u16");
            let selected = restriction
                .admitted
                .iter()
                .enumerate()
                .filter(|(variant, admitted)| {
                    let resolved_variant = trait_variants(&node.body)[*variant].resolved_variant;
                    **admitted
                        && !plan
                            .validations
                            .refuses_variant(node.node, resolved_variant)
                        && !plan.unsampled.omits(node.node, resolved_variant)
                })
                .map(|(variant, _)| variant)
                .collect::<Vec<_>>();
            if selected.is_empty() {
                return quote! {
                    #index => return Err(#facade::__private::arbitrary::Error::IncorrectFormat)
                };
            }
            let body = enum_body(&selected, Some((index, restriction)));
            quote!(#index => #body)
        });
        quote! {
            __recursa_if_arbitrary! {
            impl #impl_generics #node_ident #type_generics #where_clause {
                /// Samples a value of one restricted expression of this enum: an admitted
                /// variant, with the restriction applied again to every operand the lowering
                /// retypes.
                #[doc(hidden)]
                #[allow(dead_code)]
                pub(crate) fn __recursa_arbitrary_restricted(
                    input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
                    restriction: u16,
                ) -> #facade::__private::arbitrary::Result<Self> {
                    let (__recursa_arbitrary_depth_guard, __recursa_arbitrary_depth) =
                        #facade::__private::ArbitraryDepthGuard::enter();
                    let _ = &__recursa_arbitrary_depth_guard;
                    Ok(match restriction {
                        #(#arms,)*
                        _ => unreachable!("the restriction index is in range"),
                    })
                }
            }
            }
        }
    });
    // A struct with text rules samples again when a rule refuses the value, a bounded number of
    // times, so a sampled value always parses back (`docs/declared-rejections.md`, "Sampling").
    let checked_body = if plan.rejections.struct_rules(node.node).is_some() {
        quote! {
            {
                let mut __recursa_attempts = 0u32;
                loop {
                    let __recursa_sampled = #body;
                    if #facade::__private::RejectionFilter::new(Self::__recursa_rejection_rules())
                        .admits(__recursa_sampled.__recursa_rejection_key())
                    {
                        break __recursa_sampled;
                    }
                    __recursa_attempts += 1;
                    if __recursa_attempts >= #facade::__private::ARBITRARY_REJECTION_ATTEMPTS {
                        return Err(#facade::__private::arbitrary::Error::IncorrectFormat);
                    }
                }
            }
        }
    } else {
        body
    };
    quote! {
        __recursa_if_arbitrary! {
        impl #impl_generics #facade::__private::arbitrary::Arbitrary<'arbitrary>
            for #node_ident #type_generics #where_clause
        {
            fn arbitrary(
                input: &mut #facade::__private::arbitrary::Unstructured<'arbitrary>,
            ) -> #facade::__private::arbitrary::Result<Self> {
                // Tracks nested Node-construction depth across the whole `arbitrary()` call tree,
                // not just within this Node — every generated Node increments it on entry, so an
                // enum body several levels below can still see how deep its ancestors
                // already went. The guard's own binding keeps it alive for this whole function,
                // including every nested call `#body` makes before returning.
                let (__recursa_arbitrary_depth_guard, __recursa_arbitrary_depth) =
                    #facade::__private::ArbitraryDepthGuard::enter();
                let _ = &__recursa_arbitrary_depth_guard;
                Ok(#checked_body)
            }
        }
        }
        #restricted_sampler
    }
}

/// Reports whether a field that names restriction `restriction` of `base` samples through the
/// base's restricted sampler: the restriction is planned and admits a variant.
fn restriction_samples(plan: &EmissionPlan, base: NodeId, restriction: u16) -> bool {
    plan.restrictions
        .of(base)
        .get(usize::from(restriction))
        .is_some_and(|planned| planned.admitted.contains(&true))
}

/// Renders the sampling of one field that names a restricted expression: the base's restricted
/// sampler, under the field's storage wrappers.
fn render_restricted_arbitrary_value(
    facade: &Ident,
    terminal: &Type,
    wrappers: &[FieldWrapper],
    restriction: u16,
) -> proc_macro2::TokenStream {
    let base = quote!(<#terminal>::__recursa_arbitrary_restricted(input, #restriction)?);
    let more = quote!(<bool as #facade::__private::arbitrary::Arbitrary>::arbitrary(input)?);
    wrappers
        .iter()
        .rev()
        .fold(base, |value, wrapper| match wrapper {
            FieldWrapper::Boxed => quote!(::std::boxed::Box::new(#value)),
            FieldWrapper::Optional => quote! {
                if #more {
                    ::core::option::Option::Some(#value)
                } else {
                    ::core::option::Option::None
                }
            },
            FieldWrapper::ZeroOrMore => quote! {
                {
                    let mut __recursa_items = ::std::vec::Vec::new();
                    while #more {
                        __recursa_items.push(#value);
                    }
                    __recursa_items
                }
            },
            FieldWrapper::OneOrMore => quote! {
                {
                    let mut __recursa_items = #facade::Vec1::new(#value);
                    while #more {
                        __recursa_items.push(#value);
                    }
                    __recursa_items
                }
            },
        })
}

/// Renders configured serialization or deserialization through the authored semantic shape.
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
            let fields = elements.iter().filter_map(|element| match &element.kind {
                ElementParseKind::Semantic { field, projection } => Some((
                    field,
                    match projection.as_ref() {
                        SemanticParseProjection::Direct {
                            terminal, wrappers, ..
                        } => field_type(terminal, wrappers),
                        SemanticParseProjection::FixedPresence { .. } => syn::parse_quote!(bool),
                    },
                    serde_content_adapter(plan, projection, serialize, deserialize),
                )),
                ElementParseKind::Fixed { .. }
                | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
            });
            match style {
                ResolvedFieldStyle::Named => {
                    let fields = fields.map(|(field, ty, adapter)| {
                        let ident = field.ident.as_ref().expect("named field");
                        quote!(#adapter #ident: #ty)
                    });
                    quote!(struct #helper #generics { #(#fields),* })
                }
                ResolvedFieldStyle::Unnamed => {
                    let fields = fields.map(|(_, ty, adapter)| quote!(#adapter #ty));
                    quote!(struct #helper #generics (#(#fields),*);)
                }
                ResolvedFieldStyle::Unit => quote!(struct #helper #generics;),
            }
        }
        NodeBodyPlan::Enum { .. } => {
            let variants = trait_variants(&node.body);
            let variants = variants.iter().map(|variant| {
                let ident = &variant.ident;
                let fields = variant
                    .elements
                    .iter()
                    .filter_map(|element| match &element.kind {
                        ElementParseKind::Semantic { field, projection } => Some((
                            field,
                            match projection.as_ref() {
                                SemanticParseProjection::Direct {
                                    terminal, wrappers, ..
                                } => field_type(terminal, wrappers),
                                SemanticParseProjection::FixedPresence { .. } => {
                                    syn::parse_quote!(bool)
                                }
                            },
                            serde_content_adapter(plan, projection, serialize, deserialize),
                        )),
                        ElementParseKind::Fixed { .. }
                        | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
                    });
                match variant.style {
                    ResolvedFieldStyle::Named => {
                        let fields = fields.map(|(field, ty, adapter)| {
                            let field_ident = field.ident.as_ref().expect("named field");
                            quote!(#adapter #field_ident: #ty)
                        });
                        quote!(#ident { #(#fields),* })
                    }
                    ResolvedFieldStyle::Unnamed => {
                        let fields = fields.map(|(_, ty, adapter)| quote!(#adapter #ty));
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

/// Adds only the generic bounds required by the selected Serde direction.
pub(super) fn add_serde_bounds(
    facade: &Ident,
    node: &NodeParsePlan,
    generics: &mut Generics,
    bound: proc_macro2::TokenStream,
) {
    for field_type in non_content_semantic_field_types(facade, node)
        .into_iter()
        .chain(generic_type_dependencies(node))
    {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field_type: #bound));
    }
}

/// Renders direction-specific private text adapters for one hidden content-token field.
fn serde_content_adapter(
    plan: &EmissionPlan,
    projection: &SemanticParseProjection,
    serialize: bool,
    deserialize: bool,
) -> proc_macro2::TokenStream {
    if !matches!(
        projection,
        SemanticParseProjection::Direct {
            explorer_terminal: ExplorerTerminalPlan::Content(_),
            ..
        }
    ) {
        return proc_macro2::TokenStream::new();
    }
    let mut adapters = Vec::new();
    let module_anchor = &plan.module_anchor;
    if serialize {
        let path = syn::LitStr::new(
            &quote!(#module_anchor::__recursa_serialize_content).to_string(),
            proc_macro2::Span::call_site(),
        );
        adapters.push(quote!(serialize_with = #path));
    }
    if deserialize {
        let path = syn::LitStr::new(
            &quote!(#module_anchor::__recursa_deserialize_content).to_string(),
            proc_macro2::Span::call_site(),
        );
        adapters.push(quote!(deserialize_with = #path));
    }
    quote!(#[serde(#(#adapters),*)])
}

/// Collects the node's own declared generic type parameters (`T`, not a concrete field type)
/// actually reached by its semantic fields — the minimal set a generated impl's `where` clause
/// needs to bound. A concrete field type never needs restating here: the compiler already knows
/// whatever that type's own trait impls are without a redundant explicit predicate, and for a
/// mutually-recursive grammar (any real one), restating bounds on concrete types creates a
/// genuine infinite proof cycle in the trait solver (`A: Arbitrary` requires `B: Arbitrary`
/// requires `A: Arbitrary`...), not merely a deep one — confirmed via a minimal two-Node
/// reproduction; no `recursion_limit` bump fixes it, because it never terminates.
pub(super) fn generic_type_dependencies(node: &NodeParsePlan) -> Vec<Type> {
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
        let ElementParseKind::Semantic {
            projection: semantic,
            ..
        } = &element.kind
        else {
            return;
        };
        let SemanticParseProjection::Direct { terminal, .. } = semantic.as_ref() else {
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

/// Iterates the semantic field types that contribute generated-trait bounds.
pub(super) fn semantic_field_types(facade: &Ident, node: &NodeParsePlan) -> Vec<Type> {
    let elements = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => elements.clone(),
        NodeBodyPlan::Enum { .. } => trait_variants(&node.body)
            .into_iter()
            .flat_map(|variant| variant.elements)
            .collect(),
    };
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
                SemanticParseProjection::Direct {
                    terminal, wrappers, ..
                } if !is_recursive_terminal(node, terminal) => {
                    Some(render_wrapped_type(facade, terminal, wrappers))
                }
                SemanticParseProjection::Direct { .. } => None,
                SemanticParseProjection::FixedPresence { .. } => Some(syn::parse_quote!(bool)),
            },
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .collect()
}

/// Returns semantic field types whose public optional-trait bounds remain externally meaningful.
fn non_content_semantic_field_types(facade: &Ident, node: &NodeParsePlan) -> Vec<Type> {
    let elements = match &node.body {
        NodeBodyPlan::Struct { elements, .. } => elements.clone(),
        NodeBodyPlan::Enum { .. } => trait_variants(&node.body)
            .into_iter()
            .flat_map(|variant| variant.elements)
            .collect(),
    };
    elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
                SemanticParseProjection::Direct {
                    explorer_terminal: ExplorerTerminalPlan::Content(_),
                    ..
                } => None,
                SemanticParseProjection::Direct {
                    terminal, wrappers, ..
                } if !is_recursive_terminal(node, terminal) => {
                    Some(render_wrapped_type(facade, terminal, wrappers))
                }
                SemanticParseProjection::Direct { .. } => None,
                SemanticParseProjection::FixedPresence { .. } => Some(syn::parse_quote!(bool)),
            },
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .collect()
}

/// Reports whether a terminal application recursively targets the Node being implemented.
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
