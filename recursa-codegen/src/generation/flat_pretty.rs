//! Flat Pretty generation: one `FlatPretty` implementation per flat Node type, the container
//! dispatch its fields need, and the rendering surface of the grammar's `FlatAst` owner.
//!
//! A flat render must produce exactly what a detached nested value of the same Node produces, so
//! it is not a second renderer. [`super::pretty`] renders both representations from one frozen
//! [`PrettyNodeBehavior`]; this module supplies the flat half of the two decisions that actually
//! differ:
//!
//! - **Value access.** A flat Node is `Copy` and holds its children as four-byte handles, so a
//!   field access is spelled exactly as the nested one (`self.field`), minus the `Box` layer the
//!   flat field mapping drops.
//! - **Child dispatch.** A nested child renders itself; a flat child is a handle, an optional
//!   handle, a list handle or a text handle, and only the result's stores can resolve one. Every
//!   such field renders through the generated `__RecursaFlatPrettyWith` container trait, which
//!   reaches the Node behind a handle and calls its `FlatPretty` implementation.
//!
//! What is emitted where:
//!
//! - One `FlatPretty` implementation per Node family goes beside the authored declaration, through
//!   the same per-node path [`super::flat`] uses, because a Node declared without `pub` cannot be
//!   named from the grammar root.
//! - The container trait, its handle, optional, list and text implementations, and the owner's
//!   `pretty`, `render`, `render_with` and `pretty_node` methods go at the grammar root.
//!
//! A flat result keeps no occurrence provenance, so a flat render emits the canonical spelling of
//! every fixed token and omits optional fixed syntax it cannot have captured. That is the same
//! document a detached nested AST renders. `docs/flat-ast-design.md` settles the rendering
//! contract and `docs/adr/0008-keep-flat-asts-semantic-only.md` records the decision.

use super::*;

/// The lifetime a generated flat rendering method binds for the store it reads.
fn flat_pretty_input_lifetime() -> syn::Lifetime {
    syn::parse_quote!('__recursa_flat_pretty_input)
}

/// The root Node type parameter a generated flat rendering method binds for the store it reads.
fn flat_pretty_root_parameter() -> Ident {
    format_ident!("__RecursaFlatPrettyRoot")
}

/// Returns every authored type parameter of one Node family, in declaration order.
///
/// A flat type is lifetime-free and keeps only the type parameters, exactly as
/// [`super::flat`] declares it, so a rendering implementation names the same list.
fn flat_pretty_parameters(node: &NodeParsePlan) -> Vec<Ident> {
    node.generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            GenericParam::Type(parameter) => Some(parameter.ident.clone()),
            GenericParam::Lifetime(_) | GenericParam::Const(_) => None,
        })
        .collect()
}

/// Returns the grammar's Flat AST store type at the rendering method's own parameters.
fn flat_pretty_store(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let module_anchor = &plan.module_anchor;
    let input = flat_pretty_input_lifetime();
    let root = flat_pretty_root_parameter();
    quote!(#module_anchor::FlatAst<#input, #root>)
}

/// Returns the rendering predicates one Node family's own type parameters owe the store.
///
/// A generic flat Node reaches a child of a parameter type through `Handle<T>`, so the parameter
/// must both render itself against this store and resolve inside it. A non-generic Node owes
/// nothing: every child type is closed, and its implementation covers every store.
fn flat_pretty_parameter_predicates(
    plan: &EmissionPlan,
    parameters: &[Ident],
) -> Vec<proc_macro2::TokenStream> {
    let facade = &plan.facade;
    let store = flat_pretty_store(plan);
    parameters
        .iter()
        .flat_map(|parameter| {
            [
                quote!(#parameter: #facade::FlatPretty<#store>),
                quote!(#store: #facade::__private::FlatStorage<#parameter>),
            ]
        })
        .collect()
}

/// Renders the child dispatch of one flat semantic value.
///
/// Every flat field — a handle, an optional handle, a list handle or a content token's text
/// handle — is dispatched through one generated container trait, so the rendering code above this
/// call is identical for both representations.
pub(super) fn flat_pretty_dispatch(
    plan: &EmissionPlan,
    value: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let module_anchor = &plan.module_anchor;
    quote! {
        #module_anchor::__RecursaFlatPrettyWith
            ::__recursa_flat_pretty_with(#value, __recursa_ast, context)
    }
}

/// Renders the item iteration of one repeated flat field.
///
/// A list handle names a contiguous run in the result's list store, so the run is borrowed from
/// the store rather than from the field.
pub(super) fn flat_pretty_iterate(
    collection: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote!(__recursa_ast[#collection].iter())
}

/// Renders the container of one repeated flat field, by value.
///
/// A flat container is an eight-byte list handle, or an optional one, and both are `Copy`. The
/// store resolves the handle into a run, so the handle is taken by value: borrowing it would only
/// be dereferenced again at the index.
pub(super) fn flat_pretty_collection(
    value: proc_macro2::TokenStream,
    value_is_reference: bool,
) -> proc_macro2::TokenStream {
    if value_is_reference {
        quote!(*(#value))
    } else {
        value
    }
}

/// Renders the recursive call one flat declared-precedence operand makes on its own family.
///
/// The operand is a handle, so the call resolves it in the store and re-enters the private
/// precedence-aware helper of the Node it names.
pub(super) fn flat_pretty_declared_recursion(
    operand: proc_macro2::TokenStream,
    minimum: &proc_macro2::TokenStream,
    next: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        __recursa_ast[*(#operand)].__recursa_pretty_declared(
            __recursa_ast,
            context,
            #minimum,
            #next,
        )
    }
}

/// Returns the frame one Node family's private flat Pretty helpers are assembled into.
///
/// The store's lifetime and root Node type are bound by each helper method rather than by the
/// inherent implementation block, because an inherent block cannot bind a parameter the flat type
/// itself does not mention.
pub(super) fn flat_pretty_frame(plan: &EmissionPlan, node: &NodeParsePlan) -> PrettyFrame {
    let facade = &plan.facade;
    let parameters = flat_pretty_parameters(node);
    let input = flat_pretty_input_lifetime();
    let root = flat_pretty_root_parameter();
    let store = flat_pretty_store(plan);
    let inherent_generics = (!parameters.is_empty()).then(|| {
        let bounded = parameters
            .iter()
            .map(|parameter| quote!(#parameter: #facade::__private::FlatNode));
        quote!(<#(#bounded),*>)
    });
    let type_generics = (!parameters.is_empty()).then(|| quote!(<#(#parameters),*>));
    let predicates = flat_pretty_parameter_predicates(plan, &parameters);
    let method_where = (!predicates.is_empty()).then(|| quote!(where #(#predicates),*));
    PrettyFrame {
        inherent_generics: quote!(#inherent_generics),
        type_generics: quote!(#type_generics),
        inherent_where: proc_macro2::TokenStream::new(),
        method_generics: quote!(<#input, #root: #facade::__private::FlatNode>),
        method_where: quote!(#method_where),
        method_parameters: quote!(__recursa_ast: &#store,),
        method_arguments: quote!(__recursa_ast,),
    }
}

/// Wraps one rendered flat Pretty body in its `FlatPretty` implementation.
pub(super) fn flat_pretty_trait_impl(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let ident = flat_type_ident(&node.ident);
    let parameters = flat_pretty_parameters(node);
    let input = flat_pretty_input_lifetime();
    let root = flat_pretty_root_parameter();
    let store = flat_pretty_store(plan);
    let max_width = plan.pretty_max_width;
    let indent = plan.pretty_indent;
    let bounded = parameters
        .iter()
        .map(|parameter| quote!(#parameter: #facade::__private::FlatNode));
    let type_generics = (!parameters.is_empty()).then(|| quote!(<#(#parameters),*>));
    let predicates = flat_pretty_parameter_predicates(plan, &parameters);
    let where_clause = (!predicates.is_empty()).then(|| quote!(where #(#predicates),*));
    quote! {
        impl<#input, #root: #facade::__private::FlatNode #(, #bounded)*>
            #facade::FlatPretty<#store> for #ident #type_generics
            #where_clause
        {
            fn pretty_in(
                &self,
                __recursa_ast: &#store,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.configure_defaults(#max_width, #indent);
                context.baseline_group(|context| { #body Ok(()) })
            }
        }
    }
}

/// Renders one Node family's flat `FlatPretty` implementation, beside its declaration.
///
/// The implementation is emitted only for a grammar that selects both `flat` and Pretty, and only
/// for a Node whose frozen plan carries Pretty behavior.
pub(super) fn render_node_flat_pretty(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> proc_macro2::TokenStream {
    if !plan.flat {
        return proc_macro2::TokenStream::new();
    }
    let Some(node_index) = plan
        .nodes
        .iter()
        .position(|candidate| candidate.node == node.node)
    else {
        return proc_macro2::TokenStream::new();
    };
    if plan.pretty.nodes[node_index].is_none() {
        return proc_macro2::TokenStream::new();
    }
    render_node_pretty_for(plan, node, node_index, PrettyAccess::Flat)
}

/// Reports whether this grammar renders Flat ASTs at all.
fn flat_pretty_enabled(plan: &EmissionPlan) -> bool {
    plan.flat && plan.pretty.nodes.iter().any(Option::is_some)
}

/// Renders the grammar root's flat rendering surface: container dispatch and the owner's methods.
///
/// # Long-function contract
///
/// Rationale: the container trait, one implementation per handle shape, and the owner methods
/// that enter them are one contract; a handle shape no implementation resolves would leave a
/// field unrenderable, and an owner method without the trait would have nothing to dispatch to.
/// Invariants: rendering starts at the root handle, reads only through the result's own stores,
/// and never mutates one; a flat render uses no provenance, so every fixed token is canonical.
/// Phases: renders the container trait, then one implementation per handle shape, then the owner's
/// `pretty`, `render`, `render_with` and `pretty_node`.
pub(super) fn render_flat_pretty_support(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    if !flat_pretty_enabled(plan) {
        return proc_macro2::TokenStream::new();
    }
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input = flat_pretty_input_lifetime();
    let root = flat_pretty_root_parameter();
    let store = flat_pretty_store(plan);
    quote! {
        /// Renders one flat field of any handle shape against the store that resolves it.
        ///
        /// A flat field is a Node handle, an optional handle, a list handle, or a content token's
        /// text handle. Each shape has one implementation here, so generated Node rendering
        /// dispatches every field through one call, exactly as nested rendering dispatches every
        /// field through `Pretty`.
        #[doc(hidden)]
        pub(crate) trait __RecursaFlatPrettyWith<__RecursaFlatPrettyStore: ?Sized> {
            /// Emits what this handle names into `context`, resolved through `store`.
            fn __recursa_flat_pretty_with(
                &self,
                store: &__RecursaFlatPrettyStore,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result;
        }

        impl<#input, #root, __RecursaFlatPrettyNode>
            __RecursaFlatPrettyWith<#store>
            for #facade::__private::Handle<__RecursaFlatPrettyNode>
        where
            #root: #facade::__private::FlatNode,
            __RecursaFlatPrettyNode:
                #facade::__private::FlatNode + #facade::FlatPretty<#store>,
            #store: #facade::__private::FlatStorage<__RecursaFlatPrettyNode>,
        {
            fn __recursa_flat_pretty_with(
                &self,
                store: &#store,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                #facade::FlatPretty::pretty_in(&store[*self], store, context)
            }
        }

        impl<#input, #root, __RecursaFlatPrettyChild> __RecursaFlatPrettyWith<#store>
            for Option<__RecursaFlatPrettyChild>
        where
            #root: #facade::__private::FlatNode,
            __RecursaFlatPrettyChild: __RecursaFlatPrettyWith<#store>,
        {
            fn __recursa_flat_pretty_with(
                &self,
                store: &#store,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                if let Some(child) = self {
                    child.__recursa_flat_pretty_with(store, context)?;
                }
                Ok(())
            }
        }

        impl<#input, #root, __RecursaFlatPrettyItem> __RecursaFlatPrettyWith<#store>
            for #facade::__private::List<__RecursaFlatPrettyItem>
        where
            #root: #facade::__private::FlatNode,
            __RecursaFlatPrettyItem:
                #facade::__private::ListItem + __RecursaFlatPrettyWith<#store>,
        {
            fn __recursa_flat_pretty_with(
                &self,
                store: &#store,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.render_repeated_by(store[*self].iter(), |context, item| {
                    item.__recursa_flat_pretty_with(store, context)
                })
            }
        }

        impl<#input, #root, __RecursaFlatPrettyItem> __RecursaFlatPrettyWith<#store>
            for #facade::__private::List1<__RecursaFlatPrettyItem>
        where
            #root: #facade::__private::FlatNode,
            __RecursaFlatPrettyItem:
                #facade::__private::ListItem + __RecursaFlatPrettyWith<#store>,
        {
            fn __recursa_flat_pretty_with(
                &self,
                store: &#store,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                context.render_repeated_by(store[*self].iter(), |context, item| {
                    item.__recursa_flat_pretty_with(store, context)
                })
            }
        }

        impl<#input, #root, __RecursaFlatPrettyToken> __RecursaFlatPrettyWith<#store>
            for #facade::__private::Text<__RecursaFlatPrettyToken>
        where
            #root: #facade::__private::FlatNode,
        {
            fn __recursa_flat_pretty_with(
                &self,
                store: &#store,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                // A flat result has no occurrence provenance, so the text pool is the only
                // source of a content token's spelling, exactly as the stored value is for a
                // detached nested AST.
                let matched = context.matched_text();
                context.token(matched.as_deref().unwrap_or_else(|| store[*self].as_str()))
            }
        }

        impl<#input, #root: #facade::__private::FlatNode> FlatAst<#input, #root> {
            /// Emits the Node `handle` names into `context`.
            ///
            /// Rendering reads only this result's stores, so the Node behind every handle below
            /// `handle` renders too. Panics when `handle` does not name a Node of this result.
            pub fn pretty_node<__RecursaFlatPrettyNode>(
                &self,
                handle: #facade::__private::Handle<__RecursaFlatPrettyNode>,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result
            where
                __RecursaFlatPrettyNode:
                    #facade::__private::FlatNode + #facade::FlatPretty<Self>,
                Self: #facade::__private::FlatStorage<__RecursaFlatPrettyNode>,
            {
                #facade::FlatPretty::pretty_in(&self[handle], self, context)
            }
        }

        impl<#input, #root> FlatAst<#input, #root>
        where
            #root: #facade::__private::FlatNode
                + #facade::FlatPretty<#module_anchor::FlatAst<#input, #root>>,
            #module_anchor::FlatAst<#input, #root>:
                #facade::__private::FlatStorage<#root>,
        {
            /// Emits this result's root Node into `context`.
            pub fn pretty(
                &self,
                context: &mut #facade::PrettyContext,
            ) -> ::core::fmt::Result {
                self.pretty_node(self.root(), context)
            }

            /// Renders this result with the grammar's selected layout.
            pub fn render(&self) -> String {
                let mut context = #facade::PrettyContext::new();
                self.pretty(&mut context)
                    .expect("rendering into a structured sink cannot fail");
                context.finish()
            }

            /// Renders this result with caller-selected width and indentation.
            pub fn render_with(&self, config: #facade::PrettyConfig) -> String {
                let mut context = #facade::PrettyContext::with_config(config);
                self.pretty(&mut context)
                    .expect("rendering into a structured sink cannot fail");
                context.finish()
            }
        }
    }
}
