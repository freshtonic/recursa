//! Flat AST generation. This module renders one lifetime-free `Copy` flat type per Node
//! declaration, one `FlatAst` owner at the grammar root, and the layout assertions the design
//! requires.
//!
//! A nested Node owns its children. A flat Node holds each child as a four-byte typed handle into
//! one of the grammar's stores. This module renders the second representation from the same frozen
//! [`NodeParsePlan`] the nested renderers read. [`super::arbitrary_seed`] renders its mirror types
//! the same way. Field cardinality comes from the plan's [`FieldWrapper`] list. The terminal
//! identity comes from the plan's projection. The two representations therefore cannot drift
//! apart.
//!
//! The module emits these items beside the authored declaration. They go in the declaration's own
//! module, at the declaration's own visibility, because the grammar root cannot name a Node
//! declared without `pub`:
//!
//! - `Flat{Ident}` and its `FlatNode` implementation.
//! - One layout assertion per materialized application. A shared-pool Node asserts an alignment
//!   of at most four bytes, which is invariant I6. A pooled Node asserts its authored `max_size`
//!   bound, which is invariant I7.
//! - The recorded size of each application, but only for a Node the root cannot name. The root
//!   reads that constant for its own size table.
//! - The opaque traversal items of a Node the root cannot name. [`super::flat_traversal`] renders
//!   them.
//! - The requirement walk of a Node the root cannot name. [`super::flat_requires`] renders it.
//!
//! The module emits these items at the grammar root:
//!
//! - `FlatAst`, its builder, and the generic index implementations. The builder pre-sizes every
//!   store from the parse's remaining token count, by the measured multipliers this module
//!   declares below.
//! - `FlatBuffers`, the emptied stores of a finished result, with `FlatAst::recycle` that produces
//!   them and `FlatAstBuilder::from_buffers` that reuses them. The buffers borrow no input, so one
//!   value serves a whole batch of parses; `super::table_driven_flat` renders the entry points that
//!   take them.
//! - `FLAT_NODE_SIZES`, which records the size of every flat Node type in parse-index order.
//! - One `FlatStorage` implementation per application whose flat type is publicly reachable. Every
//!   other application gets that implementation in the declaration's own module. A non-public Node
//!   always lives in the shared pool. The shared pool never names its element type, so the
//!   module-local implementation needs only the `pub(crate)` shared-pool accessor the root emits.
//!
//! `docs/flat-ast-design.md` settles the storage tiering, the field mapping, and the owner API.
//! `docs/adr/0012-tiered-flat-storage.md` records the decision.

use super::*;

/// Shared-pool words reserved per remaining token when a flat parse creates its store.
///
/// Measured over pg-sql's 43,246 pinned corpus statements and its 16 stress fixtures: the shared
/// pool ends at a median 4.13 words per token, a 90th percentile of 7.17, and a maximum of 13.50.
/// The pool is a `Vec<u32>`, so a generous reservation is four bytes per token of slack; the
/// multiplier therefore covers the 90th percentile rather than the median.
const SHARED_WORDS_PER_TOKEN: usize = 8;

/// List-store slots reserved per remaining token.
///
/// Measured on the same inputs: a median 0.40 slots per token, a 90th percentile of 0.67, and a
/// maximum of 1.00. One slot per token is also a `u32` each, and it covers every statement
/// measured.
const LIST_SLOTS_PER_TOKEN: usize = 1;

/// Remaining tokens per reserved text-pool entry.
///
/// Measured on the same inputs: a median 0.33 texts per token, a 90th percentile of 0.50, and a
/// maximum of 0.82. One text per two tokens is the 90th percentile.
const TOKENS_PER_TEXT: usize = 2;

/// Remaining tokens per reserved Node of one typed pool.
///
/// Measured on the same inputs, per pool, as a fraction of the token count: `Expr` a median 0.14
/// and a 90th percentile of 0.43, `ColumnRef` 0.00 and 0.10, `FuncCall` 0.00 and 0.10,
/// `SelectStmt` 0.00 and 0.14, `Statement` 0.11 and 0.33. One reservation serves every pool, and a
/// flat Node is several words wide, so the multiplier sits near the widest pool's median rather
/// than its 90th percentile. One Node per four tokens cut the allocation count of pg-sql's
/// `select_list_10000` stress fixture from 31 to 10 and of `in_list_10000` from 19 to 9.
const TOKENS_PER_POOLED_NODE: usize = 4;

/// Smallest typed pool reservation. A smaller estimate reserves nothing at all.
///
/// A typed pool a statement never touches costs one wasted allocation, and pg-sql's median pinned
/// statement leaves three of its five pools empty. Reserving unconditionally therefore raised the
/// measured pool allocations per corpus statement from 2.71 to 4.90. Staying lazy below this floor
/// keeps the short-statement cost at 2.71 and keeps the win on the large inputs, where the
/// estimate clears the floor.
const MIN_POOLED_NODES: usize = 16;

/// Smallest reservation for the shared pool, the list store, the scratch stack and the text pool.
///
/// Every one of those already allocates on its first write, so a floor costs no allocation that a
/// parse would not make anyway. It keeps an empty or one-token input from reserving a capacity that
/// the first few writes immediately outgrow.
const MIN_STORE_ENTRIES: usize = 16;

/// Returns one declaration's generated flat type identifier.
pub(super) fn flat_type_ident(ident: &Ident) -> Ident {
    format_ident!("Flat{ident}")
}

/// Returns the grammar-root-relative path of one Node family's flat type.
///
/// The flat type is emitted beside its declaration, so its path is the declaration's own semantic
/// path with the last segment renamed.
fn flat_family_path(node: &NodeParsePlan) -> Result<syn::Path, String> {
    let mut path = emitted_semantic_path(&node.semantic_key)?;
    let segment = path
        .segments
        .last_mut()
        .ok_or_else(|| "a resolved semantic key has at least one segment".to_owned())?;
    segment.ident = flat_type_ident(&segment.ident);
    segment.arguments = syn::PathArguments::None;
    Ok(path)
}

/// Returns every authored type parameter of a Node family, in declaration order.
///
/// A flat Node is lifetime-free, because every child is a handle and every content token's text
/// lives in the text pool. Type parameters survive; their bounds do not, because a flat field
/// built from a parameter is only ever a `Handle<T>`.
fn flat_type_parameters(node: &NodeParsePlan) -> Vec<Ident> {
    node.generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            GenericParam::Type(parameter) => Some(parameter.ident.clone()),
            GenericParam::Lifetime(_) | GenericParam::Const(_) => None,
        })
        .collect()
}

/// Resolves one Node identity to its frozen parse plan.
fn node_plan_of(plan: &EmissionPlan, node: NodeId) -> Result<&NodeParsePlan, String> {
    plan.nodes
        .iter()
        .find(|candidate| candidate.node == node)
        .ok_or_else(|| "a referenced Node family has no frozen parse plan".to_owned())
}

/// Returns the last path segment identifier of a type, when it has one.
fn type_head_ident(ty: &Type) -> Option<&Ident> {
    let Type::Path(path) = ty else { return None };
    path.path.segments.last().map(|segment| &segment.ident)
}

/// Returns the angle-bracketed type arguments of a type's last path segment, in authored order.
fn type_arguments(ty: &Type) -> Vec<Type> {
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

/// Maps one authored or materialized type argument to the flat type it names.
///
/// Three cases are admitted, in order: a type parameter of the enclosing declaration passes
/// through unchanged, because the flat type keeps the same parameters; a Node names that Node's
/// own flat type, with its arguments mapped in turn; anything else is rejected. A content token
/// bound to a type parameter is the rejected case that matters: a content token maps to `Text<K>`
/// data, never to a `Handle<T>` child, so a flat field built from the parameter would be wrong.
fn flat_type_argument(
    plan: &EmissionPlan,
    enclosing_parameters: &[Ident],
    argument: &Type,
) -> Result<Type, String> {
    let head = type_head_ident(argument)
        .ok_or_else(|| "a flat type argument must name a Node or a type parameter".to_owned())?;
    if type_arguments(argument).is_empty() && enclosing_parameters.contains(head) {
        return Ok(argument.clone());
    }
    let candidates = plan
        .nodes
        .iter()
        .filter(|candidate| candidate.ident == *head)
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return Err(format!(
            "type argument `{head}` is unsupported in flat mode: it names neither a type \
             parameter of the declaration nor exactly one generated Node",
        ));
    };
    flat_instantiation(
        plan,
        candidate,
        &type_arguments(argument),
        enclosing_parameters,
    )
}

/// Builds `Flat{Ident}<..>` for one Node family at the given type arguments.
fn flat_instantiation(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    arguments: &[Type],
    enclosing_parameters: &[Ident],
) -> Result<Type, String> {
    let path = flat_family_path(node)?;
    let parameters = flat_type_parameters(node);
    if parameters.is_empty() {
        return Ok(Type::Path(syn::TypePath { qself: None, path }));
    }
    if arguments.len() != parameters.len() {
        return Err(format!(
            "Node `{}` is unsupported in flat mode: {} type parameters received {} arguments",
            node.ident,
            parameters.len(),
            arguments.len(),
        ));
    }
    let mapped = arguments
        .iter()
        .map(|argument| flat_type_argument(plan, enclosing_parameters, argument))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(syn::parse_quote!(#path<#(#mapped),*>))
}

/// Returns the concrete flat type of one materialized Node application.
///
/// A non-generic Node has exactly one application and its flat type carries no arguments. A
/// generic Node has one application per closed argument list, and each supplies the matching flat
/// arguments to the one flat type emitted beside the declaration.
pub(super) fn flat_application_type(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> Result<Type, String> {
    flat_instantiation(
        plan,
        node,
        &type_arguments(&application.static_ty),
        &Vec::new(),
    )
}

/// Returns the canonical generated type of one content token at `'static`.
///
/// A flat Node holds a `Text<K>` handle rather than the text itself. `Text<K>` places no bound on
/// `K`. The token type is therefore named at `'static`, which keeps the flat Node lifetime-free.
pub(super) fn content_token_static_type(
    plan: &EmissionPlan,
    token: ContentTokenId,
) -> Result<Type, String> {
    let index = plan
        .tokens
        .iter()
        .position(|candidate| candidate.semantic == token)
        .ok_or_else(|| "a content terminal names a token absent from the plan".to_owned())?;
    let mut path = plan.module_anchor.clone();
    path.segments.push(syn::PathSegment {
        ident: format_ident!("__recursa_content_tokens"),
        arguments: syn::PathArguments::None,
    });
    path.segments.push(syn::PathSegment {
        ident: format_ident!("__RecursaContentToken{index}"),
        arguments: syn::PathArguments::AngleBracketed(syn::parse_quote!(<'static>)),
    });
    Ok(Type::Path(syn::TypePath { qself: None, path }))
}

/// Maps one authored semantic field to its flat field type.
///
/// The terminal becomes a handle or a text handle, then the authored cardinality wraps it.
/// `boxed!` contributes nothing, because every child is already a four-byte handle: the design's
/// field-mapping table says `boxed!(T)` and `T` share one flat field type.
fn flat_field_type(
    plan: &EmissionPlan,
    facade: &Ident,
    parameters: &[Ident],
    projection: &SemanticParseProjection,
) -> Result<Type, String> {
    let SemanticParseProjection::Direct { wrappers, .. } = projection else {
        return Ok(syn::parse_quote!(bool));
    };
    let base = flat_terminal_type(plan, facade, parameters, projection)?
        .ok_or_else(|| "a direct field projection always has a flat terminal type".to_owned())?;
    Ok(wrappers
        .iter()
        .rev()
        .filter(|wrapper| !matches!(wrapper, FieldWrapper::Boxed))
        .fold(base, |ty, wrapper| match wrapper {
            FieldWrapper::Boxed => ty,
            FieldWrapper::Optional => syn::parse_quote!(Option<#ty>),
            FieldWrapper::ZeroOrMore => syn::parse_quote!(#facade::__private::List<#ty>),
            FieldWrapper::OneOrMore => syn::parse_quote!(#facade::__private::List1<#ty>),
        }))
}

/// Maps one authored semantic field's terminal to the flat type that stores it, before the
/// authored cardinality wraps it.
///
/// A Node child becomes a `Handle<Flat{Ident}>` and a content token a `Text<K>`; a presence field
/// has no terminal at all and returns `None`. Flat construction reads the same mapping, so a
/// reduce action spells the exact type the field holds instead of relying on inference.
pub(super) fn flat_terminal_type(
    plan: &EmissionPlan,
    facade: &Ident,
    parameters: &[Ident],
    projection: &SemanticParseProjection,
) -> Result<Option<Type>, String> {
    let SemanticParseProjection::Direct {
        terminal,
        terminal_node,
        explorer_terminal,
        ..
    } = projection
    else {
        return Ok(None);
    };
    let base: Type = match (terminal_node, explorer_terminal) {
        (_, ExplorerTerminalPlan::Content(token)) => {
            let token = content_token_static_type(plan, *token)?;
            syn::parse_quote!(#facade::__private::Text<#token>)
        }
        (Some(child), _) => {
            let child = node_plan_of(plan, *child)?;
            let arguments = type_arguments(terminal);
            let child = flat_instantiation(plan, child, &arguments, parameters)?;
            syn::parse_quote!(#facade::__private::Handle<#child>)
        }
        (None, _) => {
            let parameter = type_head_ident(terminal).filter(|head| parameters.contains(head));
            let Some(parameter) = parameter else {
                return Err(format!(
                    "field terminal `{}` is unsupported in flat mode: it is neither a generated \
                     Node, a content token, nor a type parameter of its declaration",
                    quote!(#terminal),
                ));
            };
            syn::parse_quote!(#facade::__private::Handle<#parameter>)
        }
    };
    Ok(Some(base))
}

/// One flat field: its construction position, optional name, visibility, and flat type.
struct FlatField {
    /// Named-field binding, absent for a tuple position.
    ident: Option<Ident>,
    /// Zero-based construction position in authored field order.
    index: usize,
    /// Authored field visibility, preserved from the concrete declaration.
    visibility: Visibility,
    /// Flat storage type after terminal and cardinality mapping.
    ty: Type,
}

/// Collects one struct or variant body's flat fields in authored field order.
///
/// `visibilities` supplies the authored per-position visibility of a struct body; an enum
/// variant's fields have no visibility of their own, so it is empty there.
fn flat_fields(
    plan: &EmissionPlan,
    facade: &Ident,
    parameters: &[Ident],
    elements: &[ElementParsePlan],
    visibilities: &[Visibility],
) -> Result<Vec<FlatField>, String> {
    let mut fields = Vec::new();
    for element in elements {
        let ElementParseKind::Semantic { field, projection } = &element.kind else {
            continue;
        };
        fields.push(FlatField {
            ident: field.ident.clone(),
            index: field.index,
            visibility: visibilities
                .get(field.index)
                .cloned()
                .unwrap_or(Visibility::Inherited),
            ty: flat_field_type(plan, facade, parameters, projection)?,
        });
    }
    fields.sort_by_key(|field| field.index);
    Ok(fields)
}

/// Renders one struct or variant field list, without a trailing terminator.
fn render_flat_fields(style: ResolvedFieldStyle, fields: &[FlatField]) -> proc_macro2::TokenStream {
    match style {
        ResolvedFieldStyle::Named => {
            let entries = fields.iter().map(|field| {
                let ident = field
                    .ident
                    .as_ref()
                    .expect("a named field has an identifier");
                let (visibility, ty) = (&field.visibility, &field.ty);
                quote!(#visibility #ident: #ty)
            });
            quote!({ #(#entries),* })
        }
        ResolvedFieldStyle::Unnamed => {
            let entries = fields.iter().map(|field| {
                let (visibility, ty) = (&field.visibility, &field.ty);
                quote!(#visibility #ty)
            });
            quote!((#(#entries),*))
        }
        ResolvedFieldStyle::Unit => quote!(),
    }
}

/// Authored declaration visibility and per-position struct-field visibility.
///
/// The concrete declaration item records what the author wrote after `ast_node!` applied its `pub`
/// default. It is the only place that records it. The flat type and its fields both read it there.
fn authored_visibility(node: &NodeParsePlan) -> (Visibility, Vec<Visibility>) {
    match &node.concrete_item {
        Some(Item::Struct(item)) => (
            item.vis.clone(),
            item.fields
                .iter()
                .map(|field| field.vis.clone())
                .collect::<Vec<_>>(),
        ),
        Some(Item::Enum(item)) => (item.vis.clone(), Vec::new()),
        _ => (syn::parse_quote!(pub), Vec::new()),
    }
}

/// Returns the shared-pool type tag derived from one Node's canonical key.
///
/// The key is unique per Node declaration, so two Node types of one grammar never share a tag.
/// Zero is replaced, because a zero tag would be indistinguishable from an uninitialized word.
fn flat_type_tag(semantic_key: &str) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for byte in semantic_key.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    if hash == 0 { 1 } else { hash }
}

/// Renders one Node family's flat type, its `FlatNode` implementation, and its layout assertions.
///
/// # Long-function contract
///
/// Rationale: the flat declaration, its tag, and the assertions that guard its layout stay in one
/// place. All three come from the same body plan. A split would let them drift apart.
/// Invariants: the flat type preserves the authored struct style, the enum variant shapes, the
/// field order and the field visibility. It carries the declaration's type parameters. It carries
/// none of its lifetimes.
/// Phases: reads authored visibility, renders the declaration body, then renders the tag
/// implementation and one assertion per materialized application.
pub(super) fn render_node_flat_items(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> Result<proc_macro2::TokenStream, String> {
    if !plan.flat {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let ident = flat_type_ident(&node.ident);
    let parameters = flat_type_parameters(node);
    let (visibility, field_visibilities) = authored_visibility(node);
    let generics = (!parameters.is_empty()).then(|| quote!(<#(#parameters),*>));
    let declaration = match &node.body {
        NodeBodyPlan::Struct { style, elements } => {
            let fields = flat_fields(plan, facade, &parameters, elements, &field_visibilities)?;
            let body = render_flat_fields(*style, &fields);
            let terminator = matches!(
                style,
                ResolvedFieldStyle::Unnamed | ResolvedFieldStyle::Unit
            )
            .then(|| quote!(;));
            let doc = format!(
                " The flat storage form of [`{}`], with one handle per child.",
                node.ident,
            );
            quote! {
                #[doc = #doc]
                #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
                #visibility struct #ident #generics #body #terminator
            }
        }
        NodeBodyPlan::Enum { variants } => {
            let rendered = variants
                .iter()
                .map(|variant| {
                    let fields = flat_fields(plan, facade, &parameters, &variant.elements, &[])?;
                    let body = render_flat_fields(variant.style, &fields);
                    let variant_ident = &variant.ident;
                    let doc = format!(" The flat storage form of `{}`.", variant.ident);
                    Ok(quote!(#[doc = #doc] #variant_ident #body))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let doc = format!(
                " The flat storage form of [`{}`], with one handle per child.",
                node.ident,
            );
            quote! {
                #[doc = #doc]
                #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
                #visibility enum #ident #generics {
                    #(#rendered),*
                }
            }
        }
    };
    let tag = flat_type_tag(&node.semantic_key);
    let bounds = (!parameters.is_empty()).then(|| {
        let bounded = parameters
            .iter()
            .map(|parameter| quote!(#parameter: #facade::__private::FlatNode));
        quote!(<#(#bounded),*>)
    });
    let tag_implementation = quote! {
        impl #bounds #facade::__private::FlatNode for #ident #generics {
            const TAG: u32 = #tag;
        }
    };
    let assertions = node
        .parse_applications
        .iter()
        .map(|application| render_application_assertions(plan, node, application))
        .collect::<Result<Vec<_>, String>>()?;
    let storage = (!node.publicly_materialized)
        .then(|| render_shared_storage_impls(plan, node))
        .transpose()?;
    let sizes = (!node.publicly_materialized)
        .then(|| render_size_constants(plan, node))
        .transpose()?;
    let opaque = render_node_flat_opaque_items(plan, node)?;
    let requires = render_node_flat_requires(plan, node)?;
    let pretty = render_node_flat_pretty(plan, node);
    Ok(quote! {
        #declaration
        #tag_implementation
        #(#assertions)*
        #storage
        #sizes
        #opaque
        #requires
        #pretty
    })
}

/// Renders the layout assertions one materialized application owes the design's invariants.
///
/// I6 applies to every shared-pool Node, whose words must be four-byte aligned. I7 applies to a
/// pooled Node whose declaration supplied `max_size`.
fn render_application_assertions(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> Result<proc_macro2::TokenStream, String> {
    let ty = flat_application_type(plan, node, application)?;
    if !node.flat.pool {
        let message = format!(
            "error[RCA9105]: flat Node `Flat{}` lives in the shared pool and must have an \
             alignment of at most four bytes",
            node.ident,
        );
        return Ok(quote! {
            const _: () = assert!(
                ::core::mem::align_of::<#ty>() <= 4,
                #message
            );
        });
    }
    let Some(max_size) = node.flat.max_size else {
        return Ok(proc_macro2::TokenStream::new());
    };
    let max_size = usize::try_from(max_size).map_err(|_| {
        format!(
            "`#[flat(pool, max_size = ...)]` on `{}` exceeds the addressable range",
            node.ident,
        )
    })?;
    let message = format!(
        "flat Node `Flat{}` exceeds its declared `#[flat(pool, max_size = {max_size})]` bound",
        node.ident,
    );
    Ok(quote! {
        const _: () = assert!(
            ::core::mem::size_of::<#ty>() <= #max_size,
            #message
        );
    })
}

/// Names the recorded size of one application's flat type.
fn flat_size_ident(application: &NodeApplicationParsePlan) -> Ident {
    format_ident!("__RECURSA_FLAT_SIZE_{}", application.parse_index)
}

/// Returns the path of one application's recorded size, as any module of the crate names it.
fn flat_size_path(
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> Result<syn::Path, String> {
    let mut path = emitted_semantic_path(&node.semantic_key)?;
    let segment = path
        .segments
        .last_mut()
        .ok_or_else(|| "a resolved semantic key has at least one segment".to_owned())?;
    segment.ident = flat_size_ident(application);
    segment.arguments = syn::PathArguments::None;
    Ok(path)
}

/// Renders the recorded size of every application of one non-public Node, in its own module.
///
/// The grammar's `FLAT_NODE_SIZES` records one size per flat Node application. The grammar root
/// cannot name a Node whose declaration is not public, so it cannot call `size_of` on it either.
/// This constant makes the measurement in the module that can name the Node, and the table at the
/// root reads it.
fn render_size_constants(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> Result<proc_macro2::TokenStream, String> {
    let constants = node
        .parse_applications
        .iter()
        .map(|application| -> Result<proc_macro2::TokenStream, String> {
            let ty = flat_application_type(plan, node, application)?;
            let ident = flat_size_ident(application);
            let doc = format!(
                " Size in bytes of `Flat{}`, which this grammar's `FLAT_NODE_SIZES` records.",
                application_name(node, application),
            );
            Ok(quote! {
                #[doc = #doc]
                #[doc(hidden)]
                pub const #ident: usize = ::core::mem::size_of::<#ty>();
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(quote!(#(#constants)*))
}

/// Renders the shared-pool `FlatStorage` implementations a non-public Node needs in its own module.
///
/// The grammar root cannot name a Node declared without `pub`. That Node's owner implementations
/// are therefore emitted here. They reach the store through the root's `pub(crate)` shared-pool
/// accessor. A non-public Node always lives in the shared pool, because source modeling rejects
/// `#[flat(pool)]` on a declaration that is not publicly reachable.
fn render_shared_storage_impls(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let implementations = node
        .parse_applications
        .iter()
        .map(|application| -> Result<proc_macro2::TokenStream, String> {
            let ty = flat_application_type(plan, node, application)?;
            Ok(quote! {
                impl<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode>
                    #facade::__private::FlatStorage<#ty>
                    for #module_anchor::FlatAst<'__recursa_input, __RecursaRoot>
                {
                    fn node(
                        &self,
                        handle: #facade::__private::Handle<#ty>,
                    ) -> Option<&#ty> {
                        self.__recursa_flat_shared().try_get(handle)
                    }

                    fn node_span(
                        &self,
                        handle: #facade::__private::Handle<#ty>,
                    ) -> Option<#facade::Span> {
                        self.__recursa_flat_shared().span(handle)
                    }
                }
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(quote!(#(#implementations)*))
}

/// One pooled application's generated pool field identity and element type.
struct PooledApplication {
    /// Pool field and accessor name, keyed by the application's canonical parse index.
    field: Ident,
    /// Accessor name step three uses to append a Node during a reduce.
    accessor: Ident,
    /// Read accessor name, emitted only for a Node a declared rejection rule reads: an enum with
    /// rules, or the payload or identifier wrapper of a classified variant.
    ///
    /// A pool nothing reads gets no read accessor, so generated code stays free of an item the
    /// crate never names.
    read_accessor: Option<Ident>,
    /// Authored display name of the pooled application, used only in documentation.
    name: String,
    /// Concrete flat element type stored by the pool.
    ty: Type,
}

/// Collects every pooled Node application in canonical plan order.
fn pooled_applications(plan: &EmissionPlan) -> Result<Vec<PooledApplication>, String> {
    let mut pooled = Vec::new();
    for node in plan.nodes.iter().filter(|node| node.flat.pool) {
        for application in &node.parse_applications {
            pooled.push(PooledApplication {
                field: format_ident!("pool_{}", application.parse_index),
                accessor: format_ident!("pool_{}_mut", application.parse_index),
                read_accessor: plan
                    .rejections
                    .reads_pool(node.node)
                    .then(|| format_ident!("pool_{}", application.parse_index)),
                name: flat_type_ident(&node.ident).to_string(),
                ty: flat_application_type(plan, node, application)?,
            });
        }
    }
    Ok(pooled)
}

/// One entry of the grammar's `FLAT_NODE_SIZES` table.
struct FlatNodeSize {
    /// Canonical parse index of the application, which orders the table.
    parse_index: usize,
    /// Display name of the flat type: `Flat` and then the application's own name.
    name: String,
    /// The expression that reports the size in bytes.
    size: proc_macro2::TokenStream,
}

/// Collects the recorded size of every flat Node application, in parse-index order.
///
/// The root measures a publicly reachable application itself. Every other application records its
/// own size beside its declaration, because the root cannot name the type, and the root reads that
/// constant. [`render_size_constants`] emits it.
fn flat_node_sizes(plan: &EmissionPlan) -> Result<Vec<FlatNodeSize>, String> {
    let mut sizes = Vec::new();
    for node in &plan.nodes {
        for application in &node.parse_applications {
            let size = if node.publicly_materialized {
                let ty = flat_application_type(plan, node, application)?;
                quote!(::core::mem::size_of::<#ty>())
            } else {
                let path = flat_size_path(node, application)?;
                quote!(#path)
            };
            sizes.push(FlatNodeSize {
                parse_index: application.parse_index,
                name: format!("Flat{}", application_name(node, application)),
                size,
            });
        }
    }
    sizes.sort_by_key(|entry| entry.parse_index);
    Ok(sizes)
}

/// Renders the grammar's `FlatAst` owner, its construction builder, and its index surface.
///
/// # Long-function contract
///
/// Rationale: the owner's fields, its builder, its index implementations, and the compile-time
/// checks over its handles are one contract; splitting them would let a pool exist without the
/// accessor that fills it or the index that reads it.
/// Invariants: one typed pool field per pooled application, in canonical plan order. No public
/// mutable access to any store. One shared root handle type parameter.
/// Phases: collects pooled applications, renders the owner and builder, then renders the index
/// surface, the per-application storage implementations, and the layout and thread-safety checks.
pub(super) fn render_flat_owner(plan: &EmissionPlan) -> Result<proc_macro2::TokenStream, String> {
    if !plan.flat {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let pooled = pooled_applications(plan)?;
    let shared_words_per_token = proc_macro2::Literal::usize_unsuffixed(SHARED_WORDS_PER_TOKEN);
    let list_slots_per_token = proc_macro2::Literal::usize_unsuffixed(LIST_SLOTS_PER_TOKEN);
    let tokens_per_text = proc_macro2::Literal::usize_unsuffixed(TOKENS_PER_TEXT);
    let tokens_per_pooled_node = proc_macro2::Literal::usize_unsuffixed(TOKENS_PER_POOLED_NODE);
    let min_pooled_nodes = proc_macro2::Literal::usize_unsuffixed(MIN_POOLED_NODES);
    let min_store_entries = proc_macro2::Literal::usize_unsuffixed(MIN_STORE_ENTRIES);
    // A grammar with no pooled application never reads the typed pool estimate, so the binding is
    // emitted only when at least one pool exists.
    let pooled_nodes_binding = if pooled.is_empty() {
        proc_macro2::TokenStream::new()
    } else {
        quote! {
            let pooled_nodes = match tokens / #tokens_per_pooled_node {
                nodes if nodes < #min_pooled_nodes => 0,
                nodes => nodes,
            };
        }
    };
    let pool_fields = pooled.iter().map(|pool| {
        let (field, ty) = (&pool.field, &pool.ty);
        let doc = format!(
            " Every `{}` of this result, contiguous and in post-order.",
            pool.name,
        );
        quote!(#[doc = #doc] #field: #facade::__private::TypedPool<#ty>)
    });
    let builder_pool_fields = pool_fields.clone();
    let pool_initializers = pooled.iter().map(|pool| {
        let field = &pool.field;
        quote!(#field: #facade::__private::TypedPool::with_capacity(capture_spans, pooled_nodes))
    });
    let pool_moves = pooled.iter().map(|pool| {
        let field = &pool.field;
        quote!(#field: self.#field)
    });
    // A grammar with no rejection rule reads neither store during a reduce, so neither reader is
    // emitted and generated code names no item it never uses.
    let rejection_readers = (!plan.rejections.is_empty()).then(|| {
        quote! {
            /// Reads the shared pool during one reduce action.
            ///
            /// A declared rejection rule reads the variant of an item a list already holds, which
            /// is a match on the pooled value and materializes nothing
            /// (`docs/declared-rejections.md`).
            #[doc(hidden)]
            pub(crate) fn shared(&self) -> &#facade::__private::SharedPool {
                &self.shared
            }

            /// Reads the list scratch stack during one reduce action.
            ///
            /// A declared rejection rule reads the handles of the items the open list has passed.
            #[doc(hidden)]
            pub(crate) fn scratch(&self) -> &#facade::__private::ListScratch {
                &self.scratch
            }
        }
    });
    // A classification reads the text of an item's identifier during a reduce, so only a grammar
    // that classifies a variant emits the text reader.
    let text_reader = plan.rejections.reads_texts().then(|| {
        quote! {
            /// Reads the text pool during one reduce action.
            ///
            /// A declared classification folds the text of an item's identifier
            /// (`docs/declared-rejections.md`).
            #[doc(hidden)]
            pub(crate) fn texts(&self) -> &#facade::__private::TextPool<'__recursa_input> {
                &self.texts
            }
        }
    });
    let pool_accessors = pooled.iter().map(|pool| {
        let (field, accessor, ty) = (&pool.field, &pool.accessor, &pool.ty);
        let doc = format!(" Borrows the `{}` pool for one reduce action.", pool.name);
        let read = pool.read_accessor.as_ref().map(|read| {
            let doc = format!(
                " Reads the `{}` pool for one reduce action's declared rejection rules.",
                pool.name,
            );
            quote! {
                #[doc = #doc]
                #[doc(hidden)]
                pub(crate) fn #read(&self) -> &#facade::__private::TypedPool<#ty> {
                    &self.#field
                }
            }
        });
        quote! {
            #[doc = #doc]
            #[doc(hidden)]
            pub(crate) fn #accessor(&mut self) -> &mut #facade::__private::TypedPool<#ty> {
                &mut self.#field
            }

            #read
        }
    });
    let pool_buffer_fields = pooled.iter().map(|pool| {
        let (field, ty) = (&pool.field, &pool.ty);
        let doc = format!(" The emptied `{}` pool.", pool.name);
        quote!(#[doc = #doc] #field: #facade::__private::TypedPool<#ty>)
    });
    let pool_buffer_moves = pooled.iter().map(|pool| {
        let field = &pool.field;
        quote!(#field: buffers.#field)
    });
    let pool_recycles = pooled.iter().map(|pool| {
        let field = &pool.field;
        quote! {
            #field: {
                let mut pool = self.#field;
                pool.clear(false);
                pool
            }
        }
    });
    let builder_pool_recycles = pool_recycles.clone();
    let pool_warm_ups = pooled.iter().map(|pool| {
        let field = &pool.field;
        quote! {
            builder.#field.clear(capture_spans);
            builder.#field.reserve(pooled_nodes);
        }
    });
    let empty_pool_fields = pooled.iter().map(|pool| {
        let field = &pool.field;
        quote!(#field: #facade::__private::TypedPool::new(false))
    });
    let capacities = quote! {
        let shared_words = tokens
            .saturating_mul(#shared_words_per_token)
            .max(#min_store_entries);
        let list_slots = tokens
            .saturating_mul(#list_slots_per_token)
            .max(#min_store_entries);
        let texts = (tokens / #tokens_per_text).max(#min_store_entries);
        #pooled_nodes_binding
    };
    let storage_impls = plan
        .nodes
        .iter()
        .filter(|node| node.publicly_materialized)
        .flat_map(|node| {
            node.parse_applications
                .iter()
                .map(move |application| (node, application))
        })
        .map(|(node, application)| render_public_storage_impl(plan, node, application))
        .collect::<Result<Vec<_>, String>>()?;
    let pretty_support = render_flat_pretty_support(plan);
    let node_sizes = flat_node_sizes(plan)?.into_iter().map(|entry| {
        let (name, size) = (entry.name, entry.size);
        quote!((#name, #size),)
    });
    let layout_checks = plan
        .nodes
        .iter()
        .find(|node| node.publicly_materialized)
        .and_then(|node| {
            let application = node.parse_applications.first()?;
            Some((node, application))
        })
        .map(
            |(node, application)| -> Result<proc_macro2::TokenStream, String> {
                let ty = flat_application_type(plan, node, application)?;
                Ok(quote! {
                    const _: () = assert!(
                        ::core::mem::size_of::<Option<#facade::__private::Handle<#ty>>>() == 4,
                        "an optional Node handle is four bytes"
                    );
                    const _: () = assert!(
                        ::core::mem::size_of::<
                            Option<#facade::__private::List<#facade::__private::Handle<#ty>>>,
                        >() == 8,
                        "an optional list handle is eight bytes"
                    );
                })
            },
        )
        .transpose()?;
    Ok(quote! {
        /// One parsed result's Flat AST: every Node in a pool, every child a four-byte handle.
        ///
        /// Every store is append-only and every handle stays valid for the life of the result, so
        /// the owner exposes no mutable store access. Index a handle to borrow what it names, or
        /// call `get` for the checked form.
        pub struct FlatAst<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode> {
            #(#pool_fields,)*
            /// Every Node that no typed pool holds, as whole four-byte words.
            shared: #facade::__private::SharedPool,
            /// Every list of handles, each run contiguous.
            lists: #facade::__private::ListStore,
            /// Every content token's text, borrowed from the input or owned.
            texts: #facade::__private::TextPool<'__recursa_input>,
            /// The scratch stack this parse built its lists on, kept empty.
            ///
            /// The result keeps it for one reason: `recycle` hands its capacity to the next parse,
            /// which is what lets a recycled parse open a list without allocating. No reader
            /// reaches it, because every list it held was copied into the list store.
            scratch: #facade::__private::ListScratch,
            /// The handle of the Node the parse reduced last.
            root: #facade::__private::Handle<__RecursaRoot>,
            /// Whether the parse captured one span per Node and per text.
            has_spans: bool,
        }

        impl<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode>
            FlatAst<'__recursa_input, __RecursaRoot>
        {
            /// Returns the root Node's handle.
            pub fn root(&self) -> #facade::__private::Handle<__RecursaRoot> {
                self.root
            }

            /// Reports whether this result captured spans.
            pub fn has_spans(&self) -> bool {
                self.has_spans
            }

            /// Borrows the Node `handle` names, or returns `None` when this result lacks it.
            pub fn get<__RecursaNode>(
                &self,
                handle: #facade::__private::Handle<__RecursaNode>,
            ) -> Option<&__RecursaNode>
            where
                __RecursaNode: #facade::__private::FlatNode,
                Self: #facade::__private::FlatStorage<__RecursaNode>,
            {
                #facade::__private::FlatStorage::node(self, handle)
            }

            /// Returns the span of the Node `handle` names, or `None` without capture.
            pub fn span<__RecursaNode>(
                &self,
                handle: #facade::__private::Handle<__RecursaNode>,
            ) -> Option<#facade::Span>
            where
                __RecursaNode: #facade::__private::FlatNode,
                Self: #facade::__private::FlatStorage<__RecursaNode>,
            {
                #facade::__private::FlatStorage::node_span(self, handle)
            }

            /// Returns the extent of the run `list` names, or `None` without capture.
            ///
            /// The extent runs from the first item's span to the last's; an empty run has none.
            pub fn list_span<__RecursaNode>(
                &self,
                list: #facade::__private::List<
                    #facade::__private::Handle<__RecursaNode>,
                >,
            ) -> Option<#facade::Span>
            where
                __RecursaNode: #facade::__private::FlatNode,
                Self: #facade::__private::FlatStorage<__RecursaNode>,
            {
                let items = self.lists.get(list);
                let first = self.span(*items.first()?)?;
                let last = self.span(*items.last()?)?;
                #facade::Span::new(first.start() as usize, last.end() as usize).ok()
            }

            /// Borrows the text `handle` names, or returns `None` when this result lacks it.
            pub fn text<__RecursaToken>(
                &self,
                handle: #facade::__private::Text<__RecursaToken>,
            ) -> Option<&#facade::__private::ContentText<'__recursa_input>> {
                self.texts.get(handle)
            }

            /// Returns the span of the text `handle` names, or `None` without capture.
            pub fn text_span<__RecursaToken>(
                &self,
                handle: #facade::__private::Text<__RecursaToken>,
            ) -> Option<#facade::Span> {
                self.texts.span(handle)
            }

            /// Ends this result and returns its stores as buffers for the next parse.
            ///
            /// A batch consumer hands the buffers of one finished result to the next parse, which
            /// then starts with warm capacity and allocates nothing. Pass them to
            /// `parse_flat_reusing` or `parse_flat_reusing_without_spans`.
            ///
            /// # Handle invalidation
            ///
            /// Recycling ends the result, so every handle of it is stale from that moment. Reading
            /// a stale handle against a later result is a contract violation, not a compile error:
            /// consuming `self` stops a handle from outliving the result through the owner, and
            /// nothing can stop a handle a caller copied out first. Such a read reports whatever
            /// the later result stored at that index, or panics when the index is out of range or
            /// its shared-pool tag disagrees. Read every handle of a result before you recycle it.
            pub fn recycle(self) -> FlatBuffers {
                FlatBuffers {
                    #(#pool_recycles,)*
                    shared: {
                        let mut shared = self.shared;
                        shared.clear(false);
                        shared
                    },
                    lists: {
                        let mut lists = self.lists;
                        lists.clear();
                        lists
                    },
                    texts: self.texts.into_recycled(),
                    scratch: {
                        let mut scratch = self.scratch;
                        scratch.clear();
                        scratch
                    },
                }
            }

            /// Borrows the shared pool for the Node modules' own storage implementations.
            ///
            /// This module cannot name a Node declared without `pub`. That Node's `FlatStorage`
            /// implementation is therefore emitted beside the declaration, and it reaches the
            /// store through this accessor. The accessor stays crate-private. It exposes no
            /// mutation.
            #[doc(hidden)]
            pub(crate) fn __recursa_flat_shared(&self) -> &#facade::__private::SharedPool {
                &self.shared
            }
        }

        impl<
            '__recursa_input,
            __RecursaRoot: #facade::__private::FlatNode,
            __RecursaNode: #facade::__private::FlatNode,
        > ::core::ops::Index<#facade::__private::Handle<__RecursaNode>>
            for FlatAst<'__recursa_input, __RecursaRoot>
        where
            Self: #facade::__private::FlatStorage<__RecursaNode>,
        {
            type Output = __RecursaNode;

            fn index(
                &self,
                handle: #facade::__private::Handle<__RecursaNode>,
            ) -> &__RecursaNode {
                self.get(handle)
                    .expect("a Flat AST handle names a Node of the result that made it")
            }
        }

        impl<
            '__recursa_input,
            __RecursaRoot: #facade::__private::FlatNode,
            __RecursaItem: #facade::__private::ListItem,
        > ::core::ops::Index<#facade::__private::List<__RecursaItem>>
            for FlatAst<'__recursa_input, __RecursaRoot>
        {
            type Output = [__RecursaItem];

            fn index(&self, list: #facade::__private::List<__RecursaItem>) -> &[__RecursaItem] {
                self.lists.get(list)
            }
        }

        impl<
            '__recursa_input,
            __RecursaRoot: #facade::__private::FlatNode,
            __RecursaItem: #facade::__private::ListItem,
        > ::core::ops::Index<#facade::__private::List1<__RecursaItem>>
            for FlatAst<'__recursa_input, __RecursaRoot>
        {
            type Output = [__RecursaItem];

            fn index(&self, list: #facade::__private::List1<__RecursaItem>) -> &[__RecursaItem] {
                self.lists.get1(list)
            }
        }

        impl<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode, __RecursaToken>
            ::core::ops::Index<#facade::__private::Text<__RecursaToken>>
            for FlatAst<'__recursa_input, __RecursaRoot>
        {
            type Output = #facade::__private::ContentText<'__recursa_input>;

            fn index(
                &self,
                handle: #facade::__private::Text<__RecursaToken>,
            ) -> &#facade::__private::ContentText<'__recursa_input> {
                &self.texts[handle]
            }
        }

        #(#storage_impls)*

        /// One Flat AST under construction, before the parse has reduced its root.
        ///
        /// Flat reduce actions append to the stores through this value and `finish` seals it into
        /// the immutable owner. It is crate-private generated scaffolding, not a public builder.
        #[doc(hidden)]
        pub struct FlatAstBuilder<'__recursa_input> {
            #(#builder_pool_fields,)*
            /// Every Node that no typed pool holds, as whole four-byte words.
            shared: #facade::__private::SharedPool,
            /// Every list of handles, each run contiguous.
            lists: #facade::__private::ListStore,
            /// Every content token's text, borrowed from the input or owned.
            texts: #facade::__private::TextPool<'__recursa_input>,
            /// The one scratch stack every list is built on.
            scratch: #facade::__private::ListScratch,
            /// Whether this parse captures one span per Node and per text.
            has_spans: bool,
        }

        impl<'__recursa_input> FlatAstBuilder<'__recursa_input> {
            /// Creates empty stores pre-sized for `tokens` remaining tokens, which capture one
            /// span per Node only when asked.
            ///
            /// `tokens` is `ParseContext::remaining_tokens`, an upper bound on the tokens this
            /// parse can consume. Every store is sized from it by a measured per-token multiplier,
            /// so a short statement makes one allocation per store instead of growing each from
            /// zero. The multipliers are documented in `recursa-codegen`'s flat generation module.
            /// They are hints: a store that outgrows its reservation grows as it always did, and a
            /// store the parse never writes to holds nothing.
            #[doc(hidden)]
            pub(crate) fn new(capture_spans: bool, tokens: usize) -> Self {
                #capacities
                Self {
                    #(#pool_initializers,)*
                    shared: #facade::__private::SharedPool::with_capacity(
                        capture_spans,
                        shared_words,
                    ),
                    lists: #facade::__private::ListStore::with_capacity(list_slots),
                    texts: #facade::__private::TextPool::with_capacity(capture_spans, texts),
                    scratch: #facade::__private::ListScratch::with_capacity(list_slots),
                    has_spans: capture_spans,
                }
            }

            /// Reuses `buffers` for a parse of `tokens` remaining tokens.
            ///
            /// The buffers come from `FlatAst::recycle` or from `FlatBuffers::new`. Every store is
            /// cleared, which applies the requested span-capture setting and ends the previous
            /// result, and then reserves the same pre-sized capacity `new` reserves. A reservation
            /// a warm store already covers allocates nothing, which is the whole point: a parse
            /// that reuses buffers big enough for it makes no allocation of its own.
            #[doc(hidden)]
            pub(crate) fn from_buffers(
                buffers: FlatBuffers,
                capture_spans: bool,
                tokens: usize,
            ) -> Self {
                #capacities
                let mut builder = Self {
                    #(#pool_buffer_moves,)*
                    shared: buffers.shared,
                    lists: buffers.lists,
                    texts: buffers.texts,
                    scratch: buffers.scratch,
                    has_spans: capture_spans,
                };
                #(#pool_warm_ups)*
                builder.shared.clear(capture_spans);
                builder.shared.reserve(shared_words);
                builder.lists.clear();
                builder.lists.reserve(list_slots);
                builder.texts.clear(capture_spans);
                builder.texts.reserve(texts);
                builder.scratch.clear();
                builder.scratch.reserve(list_slots);
                builder
            }

            /// Returns the stores of an abandoned parse as buffers for the next one.
            ///
            /// A failed parse calls this so that the caller keeps the capacity it lent. The
            /// partial result is discarded: every store is emptied, so every handle the abandoned
            /// parse made is stale.
            #[doc(hidden)]
            pub(crate) fn recycle(self) -> FlatBuffers {
                FlatBuffers {
                    #(#builder_pool_recycles,)*
                    shared: {
                        let mut shared = self.shared;
                        shared.clear(false);
                        shared
                    },
                    lists: {
                        let mut lists = self.lists;
                        lists.clear();
                        lists
                    },
                    texts: self.texts.into_recycled(),
                    scratch: {
                        let mut scratch = self.scratch;
                        scratch.clear();
                        scratch
                    },
                }
            }

            #(#pool_accessors)*

            /// Borrows the shared pool for one reduce action.
            #[doc(hidden)]
            pub(crate) fn shared_mut(&mut self) -> &mut #facade::__private::SharedPool {
                &mut self.shared
            }

            #rejection_readers

            #text_reader

            /// Borrows the list store for one reduce action.
            #[doc(hidden)]
            pub(crate) fn lists_mut(&mut self) -> &mut #facade::__private::ListStore {
                &mut self.lists
            }

            /// Borrows the text pool for one reduce action.
            #[doc(hidden)]
            pub(crate) fn texts_mut(
                &mut self,
            ) -> &mut #facade::__private::TextPool<'__recursa_input> {
                &mut self.texts
            }

            /// Borrows the list scratch stack and the list store together.
            #[doc(hidden)]
            pub(crate) fn lists_and_scratch_mut(
                &mut self,
            ) -> (
                &mut #facade::__private::ListScratch,
                &mut #facade::__private::ListStore,
            ) {
                (&mut self.scratch, &mut self.lists)
            }

            /// Reports whether this parse captures spans.
            #[doc(hidden)]
            pub(crate) fn has_spans(&self) -> bool {
                self.has_spans
            }

            /// Seals the stores into an immutable result rooted at `root`.
            #[doc(hidden)]
            pub(crate) fn finish<__RecursaRoot: #facade::__private::FlatNode>(
                self,
                root: #facade::__private::Handle<__RecursaRoot>,
            ) -> FlatAst<'__recursa_input, __RecursaRoot> {
                debug_assert!(
                    self.scratch.is_empty(),
                    "every list a flat parse opened was closed before its root reduced",
                );
                FlatAst {
                    #(#pool_moves,)*
                    shared: self.shared,
                    lists: self.lists,
                    texts: self.texts,
                    scratch: self.scratch,
                    root,
                    has_spans: self.has_spans,
                }
            }
        }

        /// The recycled stores of a finished Flat AST, ready for the next parse.
        ///
        /// One value holds one result's emptied buffers: every typed pool, the shared pool, the
        /// list store, the text pool and the list scratch stack, each with the capacity the
        /// previous parse grew it to and none of its contents. A batch consumer keeps one of these
        /// beside its loop, so every parse after the first allocates nothing.
        ///
        /// It borrows nothing. The text pool of a live result borrows the input it was lexed from;
        /// a recycled pool holds no text, so the buffers outlive that input and serve a parse of
        /// any later one.
        ///
        /// # Handle invalidation
        ///
        /// Recycling a result ends it. Every handle of that result is stale, as `FlatAst::recycle`
        /// documents, and the next parse hands the same handle values out for Nodes of its own.
        pub struct FlatBuffers {
            #(#pool_buffer_fields,)*
            /// The emptied shared pool.
            shared: #facade::__private::SharedPool,
            /// The emptied list store.
            lists: #facade::__private::ListStore,
            /// The emptied text pool, which borrows no input.
            texts: #facade::__private::TextPool<'static>,
            /// The emptied list scratch stack.
            scratch: #facade::__private::ListScratch,
        }

        impl FlatBuffers {
            /// Creates buffers that hold no capacity at all.
            ///
            /// The first parse that reuses them pre-sizes each store exactly as a parse without
            /// buffers does, so a consumer can hold one value and always call the reusing entry
            /// point.
            pub fn new() -> Self {
                Self {
                    #(#empty_pool_fields,)*
                    shared: #facade::__private::SharedPool::new(false),
                    lists: #facade::__private::ListStore::new(),
                    texts: #facade::__private::TextPool::new(false),
                    scratch: #facade::__private::ListScratch::new(),
                }
            }
        }

        impl ::core::default::Default for FlatBuffers {
            fn default() -> Self {
                Self::new()
            }
        }

        /// Proves the recycled buffers cross thread boundaries, as a parsed result does.
        const _: () = {
            const fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<FlatBuffers>();
        };

        #layout_checks

        /// The size in bytes of every flat Node type of this grammar, in parse-index order.
        ///
        /// One entry holds the flat type's name and the type's `size_of`. The table records the
        /// sizes; it asserts nothing about them. A declaration that wants a bound declares
        /// `#[flat(pool, max_size = N)]`, which generates the assertion.
        ///
        /// A size is a compile-time fact, so a test can pin the whole table. A Node whose
        /// declaration is not public appears here too, under its own name.
        pub const FLAT_NODE_SIZES: &[(&str, usize)] = &[
            #(#node_sizes)*
        ];

        #pretty_support

        /// Proves the Flat AST result crosses thread boundaries for every root Node type.
        ///
        /// The bound is checked where the function is defined, so no instantiation is needed.
        #[doc(hidden)]
        pub const fn __recursa_assert_flat_ast_send_sync<
            __RecursaRoot: #facade::__private::FlatNode,
        >() {
            const fn assert_send_sync<T: Send + Sync>() {}
            assert_send_sync::<FlatAst<'static, __RecursaRoot>>();
        }
    })
}

/// Renders the owner-side `FlatStorage` implementation of one publicly reachable application.
fn render_public_storage_impl(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let ty = flat_application_type(plan, node, application)?;
    let body = if node.flat.pool {
        let field = format_ident!("pool_{}", application.parse_index);
        quote! {
            fn node(&self, handle: #facade::__private::Handle<#ty>) -> Option<&#ty> {
                self.#field.get(handle)
            }

            fn node_span(
                &self,
                handle: #facade::__private::Handle<#ty>,
            ) -> Option<#facade::Span> {
                self.#field.span(handle)
            }
        }
    } else {
        quote! {
            fn node(&self, handle: #facade::__private::Handle<#ty>) -> Option<&#ty> {
                self.shared.try_get(handle)
            }

            fn node_span(
                &self,
                handle: #facade::__private::Handle<#ty>,
            ) -> Option<#facade::Span> {
                self.shared.span(handle)
            }
        }
    };
    Ok(quote! {
        impl<'__recursa_input, __RecursaRoot: #facade::__private::FlatNode>
            #facade::__private::FlatStorage<#ty>
            for FlatAst<'__recursa_input, __RecursaRoot>
        {
            #body
        }
    })
}
