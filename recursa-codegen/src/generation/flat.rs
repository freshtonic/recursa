//! Flat AST generation: one lifetime-free `Copy` flat type per Node declaration, one `FlatAst`
//! owner at the grammar root, and the layout assertions the design requires.
//!
//! The nested AST owns its children; a Flat AST stores every child as a four-byte typed handle
//! into one of the grammar's stores. This module renders the second representation from the same
//! frozen [`NodeParsePlan`] the nested renderers read, exactly as
//! [`super::arbitrary_seed`] renders its mirror types: field cardinality comes from the plan's
//! [`FieldWrapper`] list and the terminal identity comes from the plan's projection, so the two
//! representations cannot drift apart.
//!
//! What is emitted where:
//!
//! - `Flat{Ident}` and its `FlatNode` implementation go beside the authored declaration, in the
//!   declaration's own module and at the declaration's own visibility, because a Node declared
//!   without `pub` cannot be named from the grammar root.
//! - One layout assertion per materialized application goes in the same module: alignment at most
//!   four for a shared-pool Node (invariant I6) and the authored `max_size` bound for a pooled one
//!   (invariant I7).
//! - `FlatAst`, its builder, and the generic index implementations go at the grammar root.
//! - A `FlatStorage` implementation goes at the root when the flat type is publicly reachable and
//!   in the declaration's own module otherwise. A non-public Node always lives in the shared pool,
//!   which never names its element type, so the module-local implementation only needs the
//!   `pub(crate)` shared-pool accessor the root emits.
//!
//! `docs/flat-ast-design.md` settles the storage tiering, the field mapping, and the owner API;
//! `docs/adr/0012-tiered-flat-storage.md` records the decision.

use super::*;

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
/// A flat Node holds a `Text<K>` handle rather than the text itself, and `Text<K>` places no bound
/// on `K`, so the token type is named at `'static` to keep the flat Node lifetime-free.
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
/// The concrete declaration item is the one place that records what the author wrote after
/// `ast_node!` applied its `pub` default, so both the flat type and its fields read it from there.
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
/// Rationale: keeps the flat declaration, its tag, and the assertions that guard its layout in one
/// place, because all three are derived from the same body plan and would drift apart if split.
/// Invariants: the flat type preserves the authored struct style, enum variant shapes, field order
/// and field visibility; it carries the declaration's type parameters and none of its lifetimes.
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
    Ok(quote! {
        #declaration
        #tag_implementation
        #(#assertions)*
        #storage
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

/// Renders the shared-pool `FlatStorage` implementations a non-public Node needs in its own module.
///
/// The grammar root cannot name a Node declared without `pub`, so its owner implementations are
/// emitted here and reach the store through the root's `pub(crate)` shared-pool accessor. A
/// non-public Node always lives in the shared pool, because `#[flat(pool)]` on an unreachable
/// declaration is rejected during source modeling.
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
                name: flat_type_ident(&node.ident).to_string(),
                ty: flat_application_type(plan, node, application)?,
            });
        }
    }
    Ok(pooled)
}

/// Renders the grammar's `FlatAst` owner, its construction builder, and its index surface.
///
/// # Long-function contract
///
/// Rationale: the owner's fields, its builder, its index implementations, and the compile-time
/// checks over its handles are one contract; splitting them would let a pool exist without the
/// accessor that fills it or the index that reads it.
/// Invariants: one typed pool field per pooled application in canonical plan order, no public
/// mutable access to any store, and one shared root handle type parameter.
/// Phases: collects pooled applications, renders the owner and builder, then renders the index
/// surface, the per-application storage implementations, and the layout and thread-safety checks.
pub(super) fn render_flat_owner(plan: &EmissionPlan) -> Result<proc_macro2::TokenStream, String> {
    if !plan.flat {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let pooled = pooled_applications(plan)?;
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
        quote!(#field: #facade::__private::TypedPool::new(capture_spans))
    });
    let pool_moves = pooled.iter().map(|pool| {
        let field = &pool.field;
        quote!(#field: self.#field)
    });
    let pool_accessors = pooled.iter().map(|pool| {
        let (field, accessor, ty) = (&pool.field, &pool.accessor, &pool.ty);
        let doc = format!(" Borrows the `{}` pool for one reduce action.", pool.name);
        quote! {
            #[doc = #doc]
            #[doc(hidden)]
            pub(crate) fn #accessor(&mut self) -> &mut #facade::__private::TypedPool<#ty> {
                &mut self.#field
            }
        }
    });
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

            /// Borrows the shared pool for the Node modules' own storage implementations.
            ///
            /// A Node declared without `pub` cannot be named from this module, so its
            /// `FlatStorage` implementation is emitted beside the declaration and reaches the
            /// store through this accessor. It stays crate-private and exposes no mutation.
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
            /// Creates empty stores that capture one span per Node only when asked.
            #[doc(hidden)]
            pub(crate) fn new(capture_spans: bool) -> Self {
                Self {
                    #(#pool_initializers,)*
                    shared: #facade::__private::SharedPool::new(capture_spans),
                    lists: #facade::__private::ListStore::new(),
                    texts: #facade::__private::TextPool::new(capture_spans),
                    scratch: #facade::__private::ListScratch::new(),
                    has_spans: capture_spans,
                }
            }

            #(#pool_accessors)*

            /// Borrows the shared pool for one reduce action.
            #[doc(hidden)]
            pub(crate) fn shared_mut(&mut self) -> &mut #facade::__private::SharedPool {
                &mut self.shared
            }

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
                    root,
                    has_spans: self.has_spans,
                }
            }
        }

        #layout_checks

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
