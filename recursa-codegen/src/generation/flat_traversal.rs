//! Flat AST traversal generation: the grammar's `NodeRef`, its `children` function, the pool and
//! store accessors a traversal needs, and the post-order checker that is invariant I1's evidence.
//!
//! [`super::flat`] renders the flat types and the `FlatAst` owner, and
//! [`super::table_driven_flat`] fills them during a parse. This module renders the layer above
//! both: one traversal value per Node application, the direct children of one Node in source
//! order, and the bridge to `recursa-core`'s grammar-erased walk.
//!
//! What is emitted, all of it at the grammar root beside the owner:
//!
//! - `NodeRef`, an enum with one variant per publicly materialized Node application, each holding
//!   that application's Node handle. A non-public Node cannot be named here, so a child of one
//!   reaches `children` as `NodeRef::Opaque`, and that variant exists only when the grammar has
//!   such a child.
//! - `children(ast, node, f)`, which calls `f` once per direct child in source order. A text
//!   handle and a presence flag are data, not children, exactly as the design says, so neither
//!   reaches `f`.
//! - `FlatNodeRef`, which turns a handle of one application into a `NodeRef`, and the owner's
//!   `root_ref` built on it. The owner's root type is a type parameter, so only a trait can name
//!   the right variant for it.
//! - The owner's `pool`, `lists` and `texts` accessors, and one `FlatPool` implementation per
//!   pooled application.
//! - `check_post_order`, which walks `children` over every pooled Node and reports the first
//!   same-pool child that does not precede its parent. One comparison decides each pair, and it
//!   lives in `recursa-core` so a unit test can hand it a violation of its own.
//!
//! A wide match is split into bounded helpers, as [`super::visit`] splits a wide Visit match: one
//! helper per [`FLAT_CHILDREN_LEAF_WIDTH`] arms, each reporting whether it matched. A grammar with
//! a thousand Node applications, or one enum with a hundred variants, therefore typechecks as a
//! series of small functions rather than one enormous match.
//!
//! `docs/flat-ast-design.md` settles the traversal API, the owner API and invariant I1.

use std::collections::BTreeMap;

use super::*;

/// Maximum match arms one generated traversal helper holds.
///
/// The value is [`super::visit`]'s leaf width, because the two splits answer the same problem: a
/// pg-sql-scale grammar has enum declarations with more than a hundred variants and more than a
/// thousand Node applications in one dispatch.
const FLAT_CHILDREN_LEAF_WIDTH: usize = 32;

/// One `NodeRef` variant and the publicly materialized Node application it names.
struct NodeRefVariant<'plan> {
    /// The declaration the application specializes.
    node: &'plan NodeParsePlan,
    /// The materialized application this variant names.
    application: &'plan NodeApplicationParsePlan,
    /// The variant identifier, which is also what `NodeRef::name` returns.
    variant: Ident,
    /// The concrete flat type of the application.
    ty: Type,
}

/// Appends the readable name of one type, which is its head identifier and then its arguments.
///
/// A generic application takes its arguments into its name, so `Holder<Name>` becomes
/// `HolderName` and `Holder<Number>` becomes `HolderNumber`. Anything that is not a path type
/// contributes nothing, and the collision rule in [`node_ref_variants`] then separates the names.
fn append_type_name(ty: &Type, name: &mut String) {
    let Type::Path(path) = ty else { return };
    let Some(segment) = path.path.segments.last() else {
        return;
    };
    name.push_str(&segment.ident.unraw().to_string());
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return;
    };
    for argument in &arguments.args {
        if let syn::GenericArgument::Type(inner) = argument {
            append_type_name(inner, name);
        }
    }
}

/// Collects one variant per publicly materialized Node application, in canonical plan order.
///
/// The variant name is the declaration's identifier followed by the identifier of each of the
/// application's type arguments. Two applications that would share a name both take their own
/// canonical parse index as a suffix, which is unique across the grammar. A Node the grammar root
/// cannot name gets no variant at all, because the enum could not hold its handle.
fn node_ref_variants(plan: &EmissionPlan) -> Result<Vec<NodeRefVariant<'_>>, String> {
    let mut names = BTreeMap::new();
    let public = plan
        .nodes
        .iter()
        .filter(|node| node.publicly_materialized)
        .flat_map(|node| {
            node.parse_applications
                .iter()
                .map(move |application| (node, application))
        })
        .map(|(node, application)| {
            let mut name = node.ident.unraw().to_string();
            for argument in flat_type_argument_types(&application.static_ty) {
                append_type_name(&argument, &mut name);
            }
            *names.entry(name.clone()).or_insert(0_usize) += 1;
            (node, application, name)
        })
        .collect::<Vec<_>>();
    public
        .into_iter()
        .map(|(node, application, name)| {
            let shared = names.get(&name).copied().unwrap_or_default() > 1;
            let variant = if shared {
                format_ident!("{name}{}", application.parse_index)
            } else {
                format_ident!("{name}")
            };
            Ok(NodeRefVariant {
                node,
                application,
                variant,
                ty: flat_application_type(plan, node, application)?,
            })
        })
        .collect()
}

/// Returns the angle-bracketed type arguments of a type's last path segment, in authored order.
fn flat_type_argument_types(ty: &Type) -> Vec<Type> {
    let Type::Path(path) = ty else {
        return Vec::new();
    };
    let Some(syn::PathArguments::AngleBracketed(arguments)) =
        path.path.segments.last().map(|segment| &segment.arguments)
    else {
        return Vec::new();
    };
    arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            syn::GenericArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        })
        .collect()
}

/// Returns one application's flat type path without its type arguments.
///
/// A pattern names a variant through the enum's own path, and Rust infers the type arguments from
/// the matched value, so the pattern path drops them.
fn flat_pattern_path(ty: &Type) -> Result<syn::Path, String> {
    let Type::Path(path) = ty else {
        return Err("a flat Node type is always a path type".to_owned());
    };
    let mut path = path.path.clone();
    if let Some(segment) = path.segments.last_mut() {
        segment.arguments = syn::PathArguments::None;
    }
    Ok(path)
}

/// One direct child site of a flat Node: how to read it, and what it names.
struct ChildSite {
    /// Zero-based construction position of the field, which names its pattern binding.
    index: usize,
    /// Reads the field out of the copied flat Node.
    access: proc_macro2::TokenStream,
    /// The authored cardinality of the field, with `boxed!` removed.
    wrappers: Vec<FieldWrapper>,
    /// The `NodeRef` variant of the child application; `None` for a Node the root cannot name.
    variant: Option<Ident>,
}

/// Collects the direct child sites of one struct body or one enum variant, in source order.
///
/// Source order is the authored element order, which is the order the parse consumed. Only a
/// `Direct` semantic projection of a Node terminal is a child: a content token becomes a text
/// handle and a presence field becomes a `bool`, and the design calls both of them data.
fn child_sites(
    variants: &BTreeMap<MaterializedApplicationId, Ident>,
    elements: &[ElementParsePlan],
    access: &impl Fn(&FieldPlan) -> proc_macro2::TokenStream,
) -> Vec<ChildSite> {
    let mut sites = Vec::new();
    for element in elements {
        let ElementParseKind::Semantic { field, projection } = &element.kind else {
            continue;
        };
        let SemanticParseProjection::Direct {
            explorer_terminal,
            wrappers,
            ..
        } = projection.as_ref()
        else {
            continue;
        };
        let ExplorerTerminalPlan::Node(child) = explorer_terminal else {
            continue;
        };
        sites.push(ChildSite {
            index: field.index,
            access: access(field),
            wrappers: wrappers
                .iter()
                .copied()
                .filter(|wrapper| !matches!(wrapper, FieldWrapper::Boxed))
                .collect(),
            variant: variants.get(child).cloned(),
        });
    }
    sites
}

/// Renders the traversal of one child site: one call per child, in source order.
///
/// The authored cardinality decides the shape: a required child is one call, an optional child is
/// none or one, and a list is one call per item of its run. `Option<zero_or_many!>` does not
/// exist, so the remaining shapes are an error rather than a silent omission.
fn render_child_site(
    node: &NodeParsePlan,
    site: &ChildSite,
) -> Result<proc_macro2::TokenStream, String> {
    let emit = |item: proc_macro2::TokenStream| match &site.variant {
        Some(variant) => quote!(f(NodeRef::#variant(#item));),
        None => quote!({ let _ = #item; f(NodeRef::Opaque); }),
    };
    let access = &site.access;
    Ok(match site.wrappers.as_slice() {
        [] => emit(quote!(#access)),
        [FieldWrapper::Optional] => {
            let visit = emit(quote!(__recursa_child));
            quote! {
                if let Some(__recursa_child) = #access {
                    #visit
                }
            }
        }
        [FieldWrapper::ZeroOrMore] | [FieldWrapper::OneOrMore] => {
            let visit = emit(quote!(*__recursa_child));
            quote! {
                for __recursa_child in &ast[#access] {
                    #visit
                }
            }
        }
        [FieldWrapper::Optional, FieldWrapper::OneOrMore] => {
            let visit = emit(quote!(*__recursa_child));
            quote! {
                if let Some(__recursa_list) = #access {
                    for __recursa_child in &ast[__recursa_list] {
                        #visit
                    }
                }
            }
        }
        _ => {
            return Err(format!(
                "`{}`: flat traversal supports a child, an optional child, a list, or an optional \
                 one-or-more list",
                node.ident,
            ));
        }
    })
}

/// One wide match, split into bounded helper functions plus the body that calls them.
struct SplitMatch {
    /// Helper functions emitted beside the caller.
    helpers: proc_macro2::TokenStream,
    /// The body the caller uses in place of the match.
    body: proc_macro2::TokenStream,
}

/// Splits a wide match into helpers that each hold at most [`FLAT_CHILDREN_LEAF_WIDTH`] arms.
///
/// A narrow match stays one match and needs no helper. A wide one becomes one helper per chunk of
/// arms; each helper reports whether it matched, and the caller calls them in order until one
/// does. Every arm therefore typechecks inside a function of bounded size, which is what a
/// grammar with a thousand applications needs. The caller must return `()`, because a helper that
/// matched ends the dispatch with `return`.
fn split_match(
    prefix: &str,
    generics: &proc_macro2::TokenStream,
    parameters: &proc_macro2::TokenStream,
    arguments: &proc_macro2::TokenStream,
    subject: &proc_macro2::TokenStream,
    arms: &[(proc_macro2::TokenStream, proc_macro2::TokenStream)],
) -> SplitMatch {
    if arms.len() <= FLAT_CHILDREN_LEAF_WIDTH {
        let arms = arms
            .iter()
            .map(|(pattern, body)| quote!(#pattern => { #body }));
        return SplitMatch {
            helpers: proc_macro2::TokenStream::new(),
            body: quote!(match #subject { #(#arms)* }),
        };
    }
    let mut helpers = Vec::new();
    let mut calls = Vec::new();
    for (index, chunk) in arms.chunks(FLAT_CHILDREN_LEAF_WIDTH).enumerate() {
        let leaf = format_ident!("{prefix}_leaf_{index}");
        let arms = chunk
            .iter()
            .map(|(pattern, body)| quote!(#pattern => { #body true }));
        helpers.push(quote! {
            fn #leaf #generics (#parameters) -> bool {
                match #subject {
                    #(#arms)*
                    _ => false,
                }
            }
        });
        calls.push(quote! {
            if #leaf(#arguments) {
                return;
            }
        });
    }
    SplitMatch {
        helpers: quote!(#(#helpers)*),
        body: quote!(#(#calls)*),
    }
}

/// The per-application children helper of one materialized application.
fn children_helper_ident(application: &NodeApplicationParsePlan) -> Ident {
    format_ident!("__recursa_flat_children_{}", application.parse_index)
}

/// One application's rendered children helper.
struct RenderedChildren {
    /// The helper and its own split helpers; `None` when the application has no child at all.
    tokens: Option<proc_macro2::TokenStream>,
    /// Whether one child of the application is a Node the grammar root cannot name.
    opaque: bool,
}

/// Renders one application's children helper, or nothing when the application has no child.
///
/// # Long-function contract
///
/// Rationale: the struct and the enum case build the same body out of the same child sites, and
/// the enum case needs the struct case's site rendering for every one of its variants, so
/// splitting them would separate a pattern from the body it binds.
/// Invariants: children are reported in authored element order; a variant pattern binds only the
/// fields that are children and ignores the rest; a wide variant match is split into bounded
/// helpers.
/// Phases: collects the child sites of the body, renders one visit per site, then wraps the
/// result in the helper the dispatch calls.
fn render_application_children(
    plan: &EmissionPlan,
    variants: &BTreeMap<MaterializedApplicationId, Ident>,
    entry: &NodeRefVariant<'_>,
) -> Result<RenderedChildren, String> {
    let facade = &plan.facade;
    let (node, application, ty) = (entry.node, entry.application, &entry.ty);
    let helper = children_helper_ident(application);
    let signature = quote! {
        ast: &FlatAst<'_, __RecursaRoot>,
        __recursa_node: #ty,
        f: &mut impl FnMut(NodeRef)
    };
    let generics = quote!(<__RecursaRoot: #facade::__private::FlatNode>);
    let mut opaque = false;
    let (helpers, body) = match &application.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let sites = child_sites(variants, elements, &|field| {
                field.ident.as_ref().map_or_else(
                    || {
                        let index = syn::Index::from(field.index);
                        quote!(__recursa_node.#index)
                    },
                    |ident| quote!(__recursa_node.#ident),
                )
            });
            if sites.is_empty() {
                return Ok(RenderedChildren {
                    tokens: None,
                    opaque: false,
                });
            }
            opaque = sites.iter().any(|site| site.variant.is_none());
            let visits = sites
                .iter()
                .map(|site| render_child_site(node, site))
                .collect::<Result<Vec<_>, String>>()?;
            (proc_macro2::TokenStream::new(), quote!(#(#visits)*))
        }
        NodeBodyPlan::Enum { variants: bodies } => {
            let path = flat_pattern_path(ty)?;
            let mut arms = Vec::new();
            let mut any = false;
            for body in bodies {
                let sites = child_sites(variants, &body.elements, &|field| {
                    let binding = format_ident!("__recursa_field_{}", field.index);
                    quote!(#binding)
                });
                any |= !sites.is_empty();
                opaque |= sites.iter().any(|site| site.variant.is_none());
                let visits = sites
                    .iter()
                    .map(|site| render_child_site(node, site))
                    .collect::<Result<Vec<_>, String>>()?;
                arms.push((
                    render_variant_pattern(&path, body, &sites),
                    quote!(#(#visits)*),
                ));
            }
            if !any {
                return Ok(RenderedChildren {
                    tokens: None,
                    opaque: false,
                });
            }
            let split = split_match(
                &helper.to_string(),
                &generics,
                &signature,
                &quote!(ast, __recursa_node, &mut *f),
                &quote!(__recursa_node),
                &arms,
            );
            (split.helpers, split.body)
        }
    };
    Ok(RenderedChildren {
        tokens: Some(quote! {
            #helpers

            fn #helper #generics (#signature) {
                #body
            }
        }),
        opaque,
    })
}

/// Renders one enum variant's pattern, binding the fields that are children.
///
/// A named variant binds what it needs and ends in `..`, so a text handle or a presence flag
/// needs no binding. A tuple variant has to fill every position, so the positions that are not
/// children take `_`. The bindings are the ones [`child_sites`] already named.
fn render_variant_pattern(
    path: &syn::Path,
    body: &VariantParsePlan,
    sites: &[ChildSite],
) -> proc_macro2::TokenStream {
    let variant = &body.ident;
    let fields =
        body.elements
            .iter()
            .filter_map(|element| match &element.kind {
                ElementParseKind::Semantic { field, .. } => Some(field),
                ElementParseKind::Fixed { .. }
                | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
            })
            .collect::<Vec<_>>();
    let bound = sites.iter().map(|site| site.index).collect::<Vec<_>>();
    let is_child = |field: &FieldPlan| bound.contains(&field.index);
    match body.style {
        ResolvedFieldStyle::Named => {
            let entries = fields.iter().filter(|field| is_child(field)).map(|field| {
                let ident = field
                    .ident
                    .as_ref()
                    .expect("a named variant field has an identifier");
                let binding = format_ident!("__recursa_field_{}", field.index);
                quote!(#ident: #binding)
            });
            quote!(#path::#variant { #(#entries,)* .. })
        }
        ResolvedFieldStyle::Unnamed => {
            let width = fields
                .iter()
                .map(|field| field.index + 1)
                .max()
                .unwrap_or_default();
            let mut positions = (0..width).map(|_| quote!(_)).collect::<Vec<_>>();
            for field in fields.iter().filter(|field| is_child(field)) {
                let binding = format_ident!("__recursa_field_{}", field.index);
                positions[field.index] = quote!(#binding);
            }
            quote!(#path::#variant(#(#positions),*))
        }
        ResolvedFieldStyle::Unit => quote!(#path::#variant),
    }
}

/// One pooled application's pool field, element type, and reported name.
struct PooledPool {
    /// The application the pool holds, which is also the key of its `NodeRef` variant.
    application: MaterializedApplicationId,
    /// The owner's pool field, keyed by the application's canonical parse index.
    field: Ident,
    /// The concrete flat element type the pool holds.
    ty: Type,
    /// The flat type name a post-order violation reports.
    name: String,
}

/// Collects every pooled Node application in canonical plan order.
///
/// A pooled declaration is always publicly reachable, because `#[flat(pool)]` on an unreachable
/// declaration is rejected during source modeling, so the grammar root can always name the
/// element type of a pool.
fn pooled_pools(plan: &EmissionPlan) -> Result<Vec<PooledPool>, String> {
    let mut pooled = Vec::new();
    for node in plan.nodes.iter().filter(|node| node.flat.pool) {
        for application in &node.parse_applications {
            pooled.push(PooledPool {
                application: application.application,
                field: format_ident!("pool_{}", application.parse_index),
                ty: flat_application_type(plan, node, application)?,
                name: flat_type_ident(&node.ident).to_string(),
            });
        }
    }
    Ok(pooled)
}

/// Renders the grammar's flat traversal surface at the grammar root.
///
/// # Long-function contract
///
/// Rationale: `NodeRef`, `children`, `root_ref` and the checker are one contract. The variant set
/// decides what `children` can report, `children` decides what the checker can see, and the
/// `Opaque` variant exists only when a rendered child needs it, so the parts are rendered
/// together or not at all.
/// Invariants: one variant per publicly materialized application; children in source order; no
/// public mutable access to any store; `Opaque` present only when a public Node has a child the
/// root cannot name.
/// Phases: collects the variants, renders each application's children helper, then renders the
/// enum, the dispatch, the traversal bridge, the store accessors, and the post-order checker.
pub(super) fn render_flat_traversal(
    plan: &EmissionPlan,
) -> Result<proc_macro2::TokenStream, String> {
    if !plan.flat {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let entries = node_ref_variants(plan)?;
    if entries.is_empty() {
        return Ok(proc_macro2::TokenStream::new());
    }
    let variants = entries
        .iter()
        .map(|entry| (entry.application.application, entry.variant.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut helpers = Vec::new();
    let mut arms = Vec::new();
    let mut opaque = false;
    for entry in &entries {
        let variant = &entry.variant;
        let rendered = render_application_children(plan, &variants, entry)?;
        opaque |= rendered.opaque;
        match rendered.tokens {
            None => arms.push((
                quote!(NodeRef::#variant(_)),
                proc_macro2::TokenStream::new(),
            )),
            Some(helper) => {
                helpers.push(helper);
                let call = children_helper_ident(entry.application);
                arms.push((
                    quote!(NodeRef::#variant(__recursa_handle)),
                    quote!(#call(ast, ast[__recursa_handle], &mut *f);),
                ));
            }
        }
    }
    let opaque_variant = opaque.then(|| {
        quote! {
            /// A child Node this module cannot name, because its declaration is not public.
            ///
            /// A non-public Node has no variant of its own, so `children` reports its site and
            /// stops there. The Node is still stored, and the declaration's own module can read
            /// it.
            Opaque,
        }
    });
    if opaque {
        arms.push((quote!(NodeRef::Opaque), proc_macro2::TokenStream::new()));
    }
    let opaque_name = opaque.then(|| quote!(NodeRef::Opaque => "Opaque",));

    let declared = entries.iter().map(|entry| {
        let (variant, ty) = (&entry.variant, &entry.ty);
        let doc = format!(" The `{}` this reference names.", entry.node.ident);
        quote!(#[doc = #doc] #variant(#facade::__private::Handle<#ty>),)
    });
    let names = entries.iter().map(|entry| {
        let variant = &entry.variant;
        let name = variant.to_string();
        quote!(NodeRef::#variant(_) => #name,)
    });
    let node_ref_impls = entries.iter().map(|entry| {
        let (variant, ty) = (&entry.variant, &entry.ty);
        quote! {
            impl FlatNodeRef for #ty {
                fn node_ref(handle: #facade::__private::Handle<#ty>) -> NodeRef {
                    NodeRef::#variant(handle)
                }
            }
        }
    });

    let dispatch = split_match(
        "__recursa_flat_children_dispatch",
        &quote!(<__RecursaRoot: #facade::__private::FlatNode>),
        &quote! {
            ast: &FlatAst<'_, __RecursaRoot>,
            node: NodeRef,
            f: &mut impl FnMut(NodeRef)
        },
        &quote!(ast, node, &mut *f),
        &quote!(node),
        &arms,
    );
    let (dispatch_helpers, dispatch_body) = (dispatch.helpers, dispatch.body);

    let pooled = pooled_pools(plan)?;
    let pool_impls = pooled.iter().map(|pool| {
        let (field, ty) = (&pool.field, &pool.ty);
        quote! {
            impl<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode>
                #facade::__private::FlatPool<#ty>
                for FlatAst<'__recursa_input, __RecursaRoot>
            {
                fn pool(&self) -> &#facade::__private::TypedPool<#ty> {
                    &self.#field
                }
            }
        }
    });
    let pool_checks = pooled
        .iter()
        .map(|pool| -> Result<proc_macro2::TokenStream, String> {
            let ty = &pool.ty;
            let name = &pool.name;
            let variant = variants.get(&pool.application).ok_or_else(|| {
                format!("pooled flat Node `{name}` has no `NodeRef` variant of its own")
            })?;
            Ok(quote! {
                for __recursa_parent in ast.pool::<#ty>().handles() {
                    let mut __recursa_found = None;
                    children(ast, NodeRef::#variant(__recursa_parent), &mut |__recursa_child| {
                        if let NodeRef::#variant(__recursa_child) = __recursa_child {
                            if __recursa_found.is_none() {
                                let __recursa_pair = #facade::check_post_order_pair(
                                    #name,
                                    __recursa_parent.raw().get(),
                                    __recursa_child.raw().get(),
                                );
                                if let Err(__recursa_violation) = __recursa_pair {
                                    __recursa_found = Some(__recursa_violation);
                                }
                            }
                        }
                    });
                    if let Some(__recursa_violation) = __recursa_found {
                        return Err(__recursa_violation);
                    }
                }
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(quote! {
        /// One Node of this grammar's Flat AST, as a traversal value.
        ///
        /// One variant names one Node application and holds its handle. The variant name is the
        /// declaration's name, followed by the name of each type argument of a generic
        /// application. A traversal never stores this value in a Node: it is what `children`,
        /// `root_ref` and the walk pass around.
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum NodeRef {
            #(#declared)*
            #opaque_variant
        }

        impl NodeRef {
            /// Returns the name of the Node application this reference names.
            ///
            /// The name is the variant's own identifier, so a walk can print a tree without
            /// naming one flat type at the call site.
            pub fn name(self) -> &'static str {
                match self {
                    #(#names)*
                    #opaque_name
                }
            }
        }

        /// Names one flat Node type's own `NodeRef` variant.
        ///
        /// A `FlatAst`'s root type is a type parameter, so `root_ref` cannot name the variant of
        /// its root by itself. Generation implements this trait for every publicly materialized
        /// application, which is exactly the set `NodeRef` covers.
        pub trait FlatNodeRef: #facade::__private::FlatNode {
            /// Returns `handle` as the `NodeRef` variant of this Node type.
            fn node_ref(handle: #facade::__private::Handle<Self>) -> NodeRef;
        }

        #(#node_ref_impls)*

        #dispatch_helpers

        #(#helpers)*

        /// Calls `f` once per direct child of `node`, in source order.
        ///
        /// Source order is the order the parse read the children. A text handle and a presence
        /// flag are data, not children, so neither reaches `f`. A child of a Node this module
        /// cannot name arrives as `NodeRef::Opaque`.
        ///
        /// The traversal reads the stores only, so it never panics for a handle this result
        /// produced.
        pub fn children<__RecursaRoot: #facade::__private::FlatNode>(
            ast: &FlatAst<'_, __RecursaRoot>,
            node: NodeRef,
            f: &mut impl FnMut(NodeRef),
        ) {
            #dispatch_body
        }

        impl<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode>
            #facade::FlatChildren for FlatAst<'__recursa_input, __RecursaRoot>
        {
            type NodeRef = NodeRef;

            fn children(&self, node: NodeRef, f: &mut dyn FnMut(NodeRef)) {
                children(self, node, &mut |child| f(child));
            }
        }

        impl<'__recursa_input, __RecursaRoot: FlatNodeRef>
            FlatAst<'__recursa_input, __RecursaRoot>
        {
            /// Returns the root Node as a traversal value.
            pub fn root_ref(&self) -> NodeRef {
                __RecursaRoot::node_ref(self.root())
            }
        }

        impl<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode>
            FlatAst<'__recursa_input, __RecursaRoot>
        {
            /// Borrows the typed pool of one pooled Node type.
            ///
            /// The pool iterates its handles in post-order and reports its length, which is what
            /// a consumer needs to visit every Node of the type or to size a side table. Only a
            /// pooled Node type has a pool; every other Node lives in the shared pool.
            pub fn pool<__RecursaNode>(
                &self,
            ) -> &#facade::__private::TypedPool<__RecursaNode>
            where
                __RecursaNode: #facade::__private::FlatNode,
                Self: #facade::__private::FlatPool<__RecursaNode>,
            {
                #facade::__private::FlatPool::pool(self)
            }

            /// Borrows the list store, which holds every run of handles of this result.
            pub fn lists(&self) -> &#facade::__private::ListStore {
                &self.lists
            }

            /// Borrows the text pool, which owns every content token's text.
            pub fn texts(&self) -> &#facade::__private::TextPool<'__recursa_input> {
                &self.texts
            }
        }

        /// Checks that every typed pool of this result is in post-order.
        ///
        /// Invariant I1 of the design promises that a parse builds a parent after its children,
        /// so every same-pool child of a pooled Node has a smaller handle. This function is that
        /// promise's evidence: it walks `children` over every Node of every typed pool and
        /// returns the first pair that breaks it. A fresh parse always passes, which makes the
        /// call cheap to add to a test or a debug build.
        pub fn check_post_order<__RecursaRoot: #facade::__private::FlatNode>(
            ast: &FlatAst<'_, __RecursaRoot>,
        ) -> Result<(), #facade::PostOrderViolation> {
            #(#pool_checks)*
            Ok(())
        }

        #(#pool_impls)*
    })
}
