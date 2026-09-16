//! Flat AST traversal generation. This module renders the grammar's `NodeRef`, its `children`
//! function, the store accessors a traversal needs, and the post-order checker.
//!
//! [`super::flat`] renders the flat types and the `FlatAst` owner. [`super::table_driven_flat`]
//! fills them during a parse. This module renders the layer above both. It reports one traversal
//! value per Node application, the direct children of one Node in source order, and the bridge to
//! `recursa-core`'s grammar-erased walk.
//!
//! The module emits this at the grammar root, beside the owner:
//!
//! - `NodeRef`, an enum with one variant per publicly materialized Node application. Each variant
//!   holds that application's Node handle.
//! - One `Opaque` variant, which holds an `OpaqueRef`. The root cannot name a Node whose
//!   declaration is not public, so it cannot give that Node a variant of its own. An opaque
//!   reference names such a Node instead. It reports the Node's name, and it continues the
//!   traversal into the Node's children. The variant exists only when the grammar has such a
//!   Node.
//! - `children(ast, node, f)`, which calls `f` once per direct child in source order. A text
//!   handle and a presence flag are data, not children. Neither one reaches `f`.
//! - `FlatNodeRef`, which turns a handle of one application into a `NodeRef`. The owner's
//!   `root_ref` is built on it. The owner's root type is a type parameter, so only a trait can
//!   name the right variant for it.
//! - The owner's `pool`, `lists` and `texts` accessors, and one `FlatPool` implementation per
//!   pooled application.
//! - `check_post_order`, which walks `children` over every pooled Node. It reports the first
//!   same-pool child that does not precede its parent. One comparison decides each pair. That
//!   comparison lives in `recursa-core`, so a unit test can give it a violation of its own.
//!
//! The module also emits items in each non-public Node's own module, through
//! [`render_node_flat_opaque_items`]. That module can name the Node, so it can also read the
//! Node. It holds the children function and the `OpaqueTable` static that make an opaque
//! reference work.
//!
//! A wide match becomes bounded helpers, as [`super::visit`] splits a wide Visit match. Each
//! helper holds at most [`VISIT_DISPATCH_LEAF_WIDTH`] arms, and each one reports whether it
//! matched. A grammar with a thousand Node applications therefore typechecks as a series of small
//! functions rather than as one enormous match.
//!
//! `docs/flat-ast-design.md` settles the traversal API, the owner API and invariant I1.

use std::collections::BTreeMap;

use super::*;

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

/// Returns the readable name of one materialized Node application.
///
/// The name is the declaration's identifier, followed by the identifier of each type argument of
/// a generic application. A `NodeRef` variant carries this name, and so does the opaque table of
/// a Node the grammar root cannot name.
pub(super) fn application_name(
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> String {
    let mut name = node.ident.unraw().to_string();
    for argument in flat_type_argument_types(&application.static_ty) {
        append_type_name(&argument, &mut name);
    }
    name
}

/// Collects the `NodeRef` variant identifier of every publicly materialized Node application.
///
/// The variant name is [`application_name`]. Two applications that would share a name both take
/// their own canonical parse index as a suffix, which is unique across the grammar. A Node the
/// grammar root cannot name gets no variant at all, because the enum could not hold its handle.
///
/// This map costs no type construction, so a caller that needs only the names does not pay for
/// [`node_ref_variants`].
fn node_ref_variant_idents(plan: &EmissionPlan) -> BTreeMap<MaterializedApplicationId, Ident> {
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
            let name = application_name(node, application);
            *names.entry(name.clone()).or_insert(0_usize) += 1;
            (application, name)
        })
        .collect::<Vec<_>>();
    public
        .into_iter()
        .map(|(application, name)| {
            let shared = names.get(&name).copied().unwrap_or_default() > 1;
            let variant = if shared {
                format_ident!("{name}{}", application.parse_index)
            } else {
                format_ident!("{name}")
            };
            (application.application, variant)
        })
        .collect()
}

/// Collects one variant per publicly materialized Node application, in canonical plan order.
///
/// [`node_ref_variant_idents`] decides the variant names. This function adds the concrete flat
/// type of each application, which the enum's own declaration needs.
fn node_ref_variants(plan: &EmissionPlan) -> Result<Vec<NodeRefVariant<'_>>, String> {
    let variants = node_ref_variant_idents(plan);
    plan.nodes
        .iter()
        .filter(|node| node.publicly_materialized)
        .flat_map(|node| {
            node.parse_applications
                .iter()
                .map(move |application| (node, application))
        })
        .map(|(node, application)| {
            let variant = variants
                .get(&application.application)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "publicly materialized Node `{}` has no `NodeRef` variant name",
                        node.ident,
                    )
                })?;
            Ok(NodeRefVariant {
                node,
                application,
                variant,
                ty: flat_application_type(plan, node, application)?,
            })
        })
        .collect()
}

/// Names the `static` opaque table of one Node application.
pub(super) fn opaque_table_ident(application: &NodeApplicationParsePlan) -> Ident {
    format_ident!("__RECURSA_FLAT_OPAQUE_{}", application.parse_index)
}

/// Names the children function one opaque table holds.
fn opaque_children_ident(application: &NodeApplicationParsePlan) -> Ident {
    format_ident!("__recursa_flat_opaque_children_{}", application.parse_index)
}

/// Returns the path of one application's opaque table, as any module of the crate names it.
///
/// The table is emitted beside the declaration, so its path is the declaration's own semantic
/// path with the last segment replaced.
fn opaque_table_path(
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> Result<syn::Path, String> {
    let mut path = emitted_semantic_path(&node.semantic_key)?;
    let segment = path
        .segments
        .last_mut()
        .ok_or_else(|| "a resolved semantic key has at least one segment".to_owned())?;
    segment.ident = opaque_table_ident(application);
    segment.arguments = syn::PathArguments::None;
    Ok(path)
}

/// Collects the opaque table of every Node application the grammar root cannot name.
///
/// Such a Node is always in the shared pool, because `#[flat(pool)]` on a declaration that is not
/// publicly reachable is rejected during source modeling. Its table therefore needs the shared
/// pool and the list store, and nothing that names the owner's root type.
fn opaque_tables(
    plan: &EmissionPlan,
) -> Result<BTreeMap<MaterializedApplicationId, syn::Path>, String> {
    let mut tables = BTreeMap::new();
    for node in plan.nodes.iter().filter(|node| !node.publicly_materialized) {
        for application in &node.parse_applications {
            tables.insert(
                application.application,
                opaque_table_path(node, application)?,
            );
        }
    }
    Ok(tables)
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
    /// The opaque table of the child application; `Some` exactly when `variant` is `None`.
    table: Option<syn::Path>,
}

/// What a rendered child site calls each child application.
struct ChildNaming<'plan> {
    /// The `NodeRef` variant of each publicly materialized application.
    variants: &'plan BTreeMap<MaterializedApplicationId, Ident>,
    /// The opaque table of each application the grammar root cannot name.
    tables: &'plan BTreeMap<MaterializedApplicationId, syn::Path>,
}

/// Where one rendered children body lives, which decides how it reaches the stores.
enum ChildrenScope<'plan> {
    /// The grammar root, which declares `NodeRef` and indexes the owner directly.
    Root,
    /// One Node's own module, which names `NodeRef` through the grammar anchor.
    ///
    /// The body there takes the list store as a parameter, because it never sees the owner: the
    /// owner's type names its root Node, and a Node the root cannot name must not depend on it.
    Module {
        /// The grammar root module, which declares `NodeRef`.
        anchor: &'plan syn::Path,
    },
}

impl ChildrenScope<'_> {
    /// Returns the path of the grammar's `NodeRef` from this scope.
    fn node_ref(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Root => quote!(NodeRef),
            Self::Module { anchor } => quote!(#anchor::NodeRef),
        }
    }

    /// Returns the run of one list field, ready to iterate.
    ///
    /// The root indexes the owner, which resolves both list shapes through one trait. A Node's own
    /// module holds the list store itself, which resolves each shape by its own method.
    fn run(
        &self,
        access: &proc_macro2::TokenStream,
        one_or_more: bool,
    ) -> proc_macro2::TokenStream {
        match self {
            Self::Root => quote!(&ast[#access]),
            Self::Module { .. } if one_or_more => quote!(__recursa_lists.get1(#access)),
            Self::Module { .. } => quote!(__recursa_lists.get(#access)),
        }
    }
}

/// Everything one rendered children body needs beyond the application it reports.
struct ChildrenContext<'plan> {
    /// The identifier generated code names the Recursa facade through.
    facade: &'plan Ident,
    /// What each child application is called from this body.
    naming: ChildNaming<'plan>,
    /// Where the body lives.
    scope: ChildrenScope<'plan>,
}

/// Collects the direct child sites of one struct body or one enum variant, in source order.
///
/// Source order is the authored element order, which is the order the parse consumed. Only a
/// `Direct` semantic projection of a Node terminal is a child: a content token becomes a text
/// handle and a presence field becomes a `bool`, and the design calls both of them data.
fn child_sites(
    naming: &ChildNaming<'_>,
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
        let variant = naming.variants.get(child).cloned();
        let table = match &variant {
            Some(_) => None,
            None => naming.tables.get(child).cloned(),
        };
        sites.push(ChildSite {
            index: field.index,
            access: access(field),
            wrappers: wrappers
                .iter()
                .copied()
                .filter(|wrapper| !matches!(wrapper, FieldWrapper::Boxed))
                .collect(),
            variant,
            table,
        });
    }
    sites
}

/// Renders the traversal of one child site: one call per child, in source order.
///
/// The authored cardinality decides the shape: a required child is one call, an optional child is
/// none or one, and a list is one call per item of its run. `Option<zero_or_many!>` does not
/// exist, so the remaining shapes are an error rather than a silent omission.
///
/// A child the grammar root can name arrives as its own `NodeRef` variant. Every other child
/// arrives as an opaque reference, which carries the child's raw handle and the child's own
/// traversal behavior, so the walk continues into it.
fn render_child_site(
    context: &ChildrenContext<'_>,
    node: &NodeParsePlan,
    site: &ChildSite,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = context.facade;
    let node_ref = context.scope.node_ref();
    let emit = |item: proc_macro2::TokenStream| match (&site.variant, &site.table) {
        (Some(variant), _) => Ok(quote!(f(#node_ref::#variant(#item));)),
        (None, Some(table)) => Ok(quote! {
            f(#node_ref::Opaque(
                #facade::__private::OpaqueRef::new((#item).raw(), &#table),
            ));
        }),
        (None, None) => Err(format!(
            "`{}`: a child the grammar root cannot name has no opaque table of its own",
            node.ident,
        )),
    };
    let access = &site.access;
    Ok(match site.wrappers.as_slice() {
        [] => emit(quote!(#access))?,
        [FieldWrapper::Optional] => {
            let visit = emit(quote!(__recursa_child))?;
            quote! {
                if let Some(__recursa_child) = #access {
                    #visit
                }
            }
        }
        [wrapper @ (FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)] => {
            let visit = emit(quote!(*__recursa_child))?;
            let run = context
                .scope
                .run(access, matches!(wrapper, FieldWrapper::OneOrMore));
            quote! {
                for __recursa_child in #run {
                    #visit
                }
            }
        }
        [FieldWrapper::Optional, FieldWrapper::OneOrMore] => {
            let visit = emit(quote!(*__recursa_child))?;
            let run = context.scope.run(&quote!(__recursa_list), true);
            quote! {
                if let Some(__recursa_list) = #access {
                    for __recursa_child in #run {
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

/// Splits a wide match into helpers that each hold at most [`VISIT_DISPATCH_LEAF_WIDTH`] arms.
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
    if arms.len() <= VISIT_DISPATCH_LEAF_WIDTH {
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
    for (index, chunk) in arms.chunks(VISIT_DISPATCH_LEAF_WIDTH).enumerate() {
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

/// The signature every split helper of one children body repeats.
struct ChildrenFrame {
    /// Generics of each split helper, and of the body's own function.
    generics: proc_macro2::TokenStream,
    /// Parameters of each split helper, and of the body's own function.
    parameters: proc_macro2::TokenStream,
    /// Arguments each split helper call passes.
    arguments: proc_macro2::TokenStream,
}

/// One application's children body: the split helpers it needs, and the body that calls them.
struct ChildrenBody {
    /// Split helpers emitted beside the body's own function.
    helpers: proc_macro2::TokenStream,
    /// The body itself.
    body: proc_macro2::TokenStream,
    /// Whether the application has one child at all.
    any: bool,
    /// Whether one child is a Node the grammar root cannot name.
    opaque: bool,
}

/// Renders the body that reports one application's direct children.
///
/// # Long-function contract
///
/// Rationale: the struct case and the enum case build the same body out of the same child sites.
/// The enum case needs the struct case's site rendering for every one of its variants. A split
/// would separate a pattern from the body it binds.
/// Invariants: children are reported in authored element order. A variant pattern binds only the
/// fields that are children, and it ignores the rest. A wide variant match becomes bounded
/// helpers.
/// Phases: collects the child sites of the body, then renders one visit per site. It then reports
/// whether the application has a child, and whether one child is opaque.
fn render_children_body(
    context: &ChildrenContext<'_>,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    ty: &Type,
    frame: &ChildrenFrame,
    prefix: &str,
) -> Result<ChildrenBody, String> {
    let empty = ChildrenBody {
        helpers: proc_macro2::TokenStream::new(),
        body: proc_macro2::TokenStream::new(),
        any: false,
        opaque: false,
    };
    match &application.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let sites = child_sites(&context.naming, elements, &|field| {
                field.ident.as_ref().map_or_else(
                    || {
                        let index = syn::Index::from(field.index);
                        quote!(__recursa_node.#index)
                    },
                    |ident| quote!(__recursa_node.#ident),
                )
            });
            if sites.is_empty() {
                return Ok(empty);
            }
            let opaque = sites.iter().any(|site| site.variant.is_none());
            let visits = sites
                .iter()
                .map(|site| render_child_site(context, node, site))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(ChildrenBody {
                helpers: proc_macro2::TokenStream::new(),
                body: quote!(#(#visits)*),
                any: true,
                opaque,
            })
        }
        NodeBodyPlan::Enum { variants: bodies } => {
            let path = flat_pattern_path(ty)?;
            let mut arms = Vec::new();
            let mut any = false;
            let mut opaque = false;
            for body in bodies {
                let sites = child_sites(&context.naming, &body.elements, &|field| {
                    let binding = format_ident!("__recursa_field_{}", field.index);
                    quote!(#binding)
                });
                any |= !sites.is_empty();
                opaque |= sites.iter().any(|site| site.variant.is_none());
                let visits = sites
                    .iter()
                    .map(|site| render_child_site(context, node, site))
                    .collect::<Result<Vec<_>, String>>()?;
                arms.push((
                    render_variant_pattern(&path, body, &sites),
                    quote!(#(#visits)*),
                ));
            }
            if !any {
                return Ok(empty);
            }
            let split = split_match(
                prefix,
                &frame.generics,
                &frame.parameters,
                &frame.arguments,
                &quote!(__recursa_node),
                &arms,
            );
            Ok(ChildrenBody {
                helpers: split.helpers,
                body: split.body,
                any: true,
                opaque,
            })
        }
    }
}

/// Renders one application's children helper at the grammar root, or nothing when it has no child.
///
/// The helper takes the owner itself, which resolves every handle and every list run of the
/// application's own children.
fn render_application_children(
    context: &ChildrenContext<'_>,
    entry: &NodeRefVariant<'_>,
) -> Result<RenderedChildren, String> {
    let facade = context.facade;
    let (node, application, ty) = (entry.node, entry.application, &entry.ty);
    let helper = children_helper_ident(application);
    let frame = ChildrenFrame {
        generics: quote!(<__RecursaRoot: #facade::__private::FlatNode>),
        parameters: quote! {
            ast: &FlatAst<'_, __RecursaRoot>,
            __recursa_node: #ty,
            f: &mut impl FnMut(NodeRef)
        },
        arguments: quote!(ast, __recursa_node, &mut *f),
    };
    let rendered =
        render_children_body(context, node, application, ty, &frame, &helper.to_string())?;
    if !rendered.any {
        return Ok(RenderedChildren {
            tokens: None,
            opaque: false,
        });
    }
    let (helpers, body) = (rendered.helpers, rendered.body);
    let (generics, parameters) = (&frame.generics, &frame.parameters);
    Ok(RenderedChildren {
        tokens: Some(quote! {
            #helpers

            fn #helper #generics (#parameters) {
                #body
            }
        }),
        opaque: rendered.opaque,
    })
}

/// Renders the opaque-traversal items one non-public Node needs in its own module.
///
/// A Node whose declaration is not public has no `NodeRef` variant, because the grammar root
/// cannot name it. Its parent reports it as an opaque reference, which carries the raw handle and
/// one `OpaqueTable` this function emits. The table's children function lives here, beside the
/// declaration, because only this module can name the Node.
///
/// The function reads the Node out of the shared pool and takes the list store as a parameter, so
/// its signature names no type of the owner. That is what lets one function pointer serve every
/// `FlatAst` of the grammar, whatever its root Node type is. One signature serves every Node, so
/// a Node without a list leaves the list store unused; the generated-code lint policy allows
/// that, as it does for the root's own children helpers.
pub(super) fn render_node_flat_opaque_items(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> Result<proc_macro2::TokenStream, String> {
    if !plan.flat || node.publicly_materialized {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let anchor = &plan.module_anchor;
    let variants = node_ref_variant_idents(plan);
    if variants.is_empty() {
        return Ok(proc_macro2::TokenStream::new());
    }
    let tables = opaque_tables(plan)?;
    let context = ChildrenContext {
        facade,
        naming: ChildNaming {
            variants: &variants,
            tables: &tables,
        },
        scope: ChildrenScope::Module { anchor },
    };
    let mut items = Vec::new();
    for application in &node.parse_applications {
        let ty = flat_application_type(plan, node, application)?;
        let children = opaque_children_ident(application);
        let frame = ChildrenFrame {
            generics: proc_macro2::TokenStream::new(),
            parameters: quote! {
                __recursa_node: #ty,
                __recursa_lists: &#facade::__private::ListStore,
                f: &mut dyn FnMut(#anchor::NodeRef)
            },
            arguments: quote!(__recursa_node, __recursa_lists, &mut *f),
        };
        let rendered = render_children_body(
            &context,
            node,
            application,
            &ty,
            &frame,
            &children.to_string(),
        )?;
        let read = rendered.any.then(|| {
            quote! {
                let __recursa_node = *__recursa_shared
                    .get(#facade::__private::opaque_handle::<#ty>(__recursa_raw));
            }
        });
        let (helpers, body) = (rendered.helpers, rendered.body);
        let name = application_name(node, application);
        let table = opaque_table_ident(application);
        let doc = format!(
            " Traversal behavior of `{name}`, whose declaration the grammar root cannot name.",
        );
        items.push(quote! {
            #helpers

            fn #children(
                __recursa_shared: &#facade::__private::SharedPool,
                __recursa_lists: &#facade::__private::ListStore,
                __recursa_raw: ::core::num::NonZeroU32,
                f: &mut dyn FnMut(#anchor::NodeRef),
            ) {
                #read
                #body
            }

            #[doc = #doc]
            #[doc(hidden)]
            pub static #table: #facade::__private::OpaqueTable<#anchor::NodeRef> =
                #facade::__private::OpaqueTable::new(#name, #children);
        });
    }
    Ok(quote!(#(#items)*))
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
/// A pooled declaration is always publicly reachable: source modeling rejects `#[flat(pool)]` on
/// a declaration that is not reachable. The grammar root can therefore always name the element
/// type of a pool.
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
/// decides what `children` can report. `children` decides what the checker can see. The `Opaque`
/// variant exists only when a rendered child needs it. The parts are therefore rendered together
/// or not at all.
/// Invariants: one variant per publicly materialized application; children in source order; no
/// public mutable access to any store; `Opaque` present only when a public Node has a child the
/// root cannot name; every Node of the result is reachable from the root.
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
    let tables = opaque_tables(plan)?;
    let context = ChildrenContext {
        facade,
        naming: ChildNaming {
            variants: &variants,
            tables: &tables,
        },
        scope: ChildrenScope::Root,
    };

    let mut helpers = Vec::new();
    let mut arms = Vec::new();
    let mut opaque = false;
    for entry in &entries {
        let variant = &entry.variant;
        let rendered = render_application_children(&context, entry)?;
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
            /// A non-public Node has no variant of its own, because a public enum cannot hold a
            /// private type. The reference carries the Node's raw handle and the Node's own
            /// traversal behavior instead. `children` therefore reports the children of the Node
            /// too, and `name` reports the Node's name.
            Opaque(#facade::__private::OpaqueRef<NodeRef>),
        }
    });
    if opaque {
        arms.push((
            quote!(NodeRef::Opaque(__recursa_opaque)),
            quote! {
                __recursa_opaque.children(
                    ast.__recursa_flat_shared(),
                    ast.lists(),
                    &mut *f,
                );
            },
        ));
    }
    let opaque_name =
        opaque.then(|| quote!(NodeRef::Opaque(__recursa_opaque) => __recursa_opaque.name(),));

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
        /// flag are data, not children, so neither one reaches `f`. A child this module cannot
        /// name arrives as `NodeRef::Opaque`. The children of that Node reach `f` too, because the
        /// opaque reference carries the Node's own traversal behavior.
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
            /// The pool iterates its handles in post-order and reports its length. A consumer
            /// needs both to visit every Node of the type, or to size a side table. Only a pooled
            /// Node type has a pool. Every other Node lives in the shared pool.
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

            /// Returns the number of four-byte words the shared pool holds.
            ///
            /// The count includes the pool's reserved word zero, so it is one for a result whose
            /// every Node is pooled. The shared pool holds no element type a consumer could name,
            /// so its size is the only fact it can report. A measurement that sizes the store from
            /// the token count reads it.
            pub fn shared_words(&self) -> usize {
                self.shared.len_words()
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
