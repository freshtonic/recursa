//! Flat requirement generation: the walk over the flat pools that answers the same two questions
//! a value walk answers.
//!
//! [`super::requires`] owns the requirement plan, the per-Node ceiling, and the walk over a nested
//! value. This module renders the second walk, over the stores of one flat parse. The two are not
//! two answers: the fold, the ceiling test and the answer all live in `recursa-core`, and
//! `docs/configuration-requirements.md` states that every storage reaches its answer through them.
//! One thing differs here, and it is the projection of the extent:
//!
//! - **The value walk** carries a `ViewCursor` and takes each extent out of the occurrence
//!   sidecar: element `e` of the active shape is `cursor.child(e)`.
//! - **The flat walk** has no occurrence sidecar, because a flat parse keeps one span per Node and
//!   per text (`docs/adr/0008-keep-flat-asts-semantic-only.md`). It takes each extent out of the
//!   stores instead: a child's own span, a text's own span, or, for a repetition, the extent from
//!   the first item's start to the last item's end.
//!
//! The module emits this at the grammar root:
//!
//! - `FlatRequires`, with one implementation per publicly materialized Node application. A
//!   `FlatAst`'s root type is a type parameter, so only a trait can name the right walk and the
//!   right ceiling for it, exactly as `FlatNodeRef` names the right `NodeRef` variant.
//! - One walk function per publicly materialized application whose ceiling is above the baseline,
//!   with that application's static note table beside it.
//! - The owner's `minimum_requirement` and `requirement_above`.
//!
//! The module emits one walk function in each non-public Node's own module, through
//! [`render_node_flat_requires`], because the grammar root cannot name such a Node's flat type.
//! That function takes the raw handle, as the opaque children function does.
//!
//! An application whose ceiling is the baseline holds no gate anywhere below it, so the scan would
//! skip it on the first comparison. Generation therefore emits neither its walk nor the call to
//! it: the pruning the value walk does at run time, this module also does at generation time.

use super::*;

/// Names one application's generated flat walk function.
fn flat_walk_ident(application: &NodeApplicationParsePlan) -> Ident {
    format_ident!("__recursa_flat_requirements_{}", application.parse_index)
}

/// Names one application's generated flat note table.
fn flat_note_table_ident(application: &NodeApplicationParsePlan) -> Ident {
    format_ident!("__RECURSA_FLAT_REQUIREMENTS_{}", application.parse_index)
}

/// Returns the path of one non-public application's walk function, as any module names it.
fn flat_walk_path(
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> Result<syn::Path, String> {
    let mut path = emitted_semantic_path(&node.semantic_key)?;
    let segment = path
        .segments
        .last_mut()
        .ok_or_else(|| "a resolved semantic key has at least one segment".to_owned())?;
    segment.ident = flat_walk_ident(application);
    segment.arguments = syn::PathArguments::None;
    Ok(path)
}

/// How one application's walk is called, and what it can hold.
struct FlatWalkTarget {
    /// The highest requirement reachable through the application's declaration.
    ceiling: u16,
    /// The function that walks it.
    call: proc_macro2::TokenStream,
    /// Whether the function takes a raw handle, which a non-public application's does.
    raw: bool,
}

/// Collects the walk of every materialized application, keyed by its identity.
fn flat_walk_targets(
    plan: &EmissionPlan,
) -> Result<BTreeMap<MaterializedApplicationId, FlatWalkTarget>, String> {
    let mut targets = BTreeMap::new();
    for node in &plan.nodes {
        let ceiling = plan.requirements.ceiling(node.node);
        for application in &node.parse_applications {
            let (call, raw) = if node.publicly_materialized {
                let ident = flat_walk_ident(application);
                (quote!(#ident), false)
            } else {
                let path = flat_walk_path(node, application)?;
                (quote!(#path), true)
            };
            targets.insert(
                application.application,
                FlatWalkTarget { ceiling, call, raw },
            );
        }
    }
    Ok(targets)
}

/// Everything one rendered flat walk body needs beyond the application it walks.
struct FlatWalkContext<'plan> {
    /// The identifier generated code names the Recursa facade through.
    facade: &'plan Ident,
    /// The grammar root module, which declares the owner.
    anchor: &'plan syn::Path,
    /// The walk of every application a child site can name.
    targets: &'plan BTreeMap<MaterializedApplicationId, FlatWalkTarget>,
}

/// Renders the call that walks one child, guarded by the child's own ceiling.
///
/// The guard is the one the value walk emits, with one difference: a child whose ceiling is the
/// baseline can hold no requirement at all, so generation emits nothing rather than a comparison
/// that is always true.
fn render_child_walk(
    context: &FlatWalkContext<'_>,
    child: MaterializedApplicationId,
    handle: &proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = context.facade;
    let target = context.targets.get(&child).ok_or_else(|| {
        "a flat child site names a Node application absent from the plan".to_owned()
    })?;
    if target.ceiling == 0 {
        return Ok(quote!());
    }
    let call = &target.call;
    let argument = if target.raw {
        quote!((#handle).raw())
    } else {
        quote!(#handle)
    };
    let ceiling = target.ceiling;
    Ok(quote! {
        if !scan.skips(#facade::Configuration::from_ordinal(#ceiling)) {
            #call(ast, #argument, scan);
        }
    })
}

/// Renders the walk of one planned element: its own gate, then its child.
///
/// The shape follows [`super::requires::render_element`] exactly, because the two walks must
/// record the same notes in the same source order. Only the extent and the field access differ: a
/// flat field holds a handle, and the stores resolve it.
fn render_flat_element(
    context: &FlatWalkContext<'_>,
    builder: &mut NoteBuilder<'_>,
    record: Option<RequirementRecord>,
    element: &ElementParsePlan,
    access: &proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String> {
    let ElementParseKind::Semantic { projection, .. } = &element.kind else {
        return Ok(quote!());
    };
    let site = format!(".{}", element.metadata_name);
    let facade = context.facade;
    // A missing optional value has no extent of its own, so the record carries the extent of the
    // Node that holds the field. The nested walk records it at the same place: its own cursor.
    let owner_extent = quote!(__recursa_span);
    match projection.as_ref() {
        SemanticParseProjection::FixedPresence { .. } => {
            let Some(record) = record else {
                return Ok(quote!());
            };
            // A presence field is a `bool`, and no flat store holds the fixed syntax it stands
            // for, so this record has no extent at all. A parse without spans reports the same
            // thing everywhere.
            let call = builder.record_span(&record, &site, &quote!(::core::option::Option::None));
            Ok(if record.absent {
                quote!(if !#access { #call })
            } else {
                quote!(if #access { #call })
            })
        }
        SemanticParseProjection::Direct {
            explorer_terminal,
            wrappers,
            ..
        } => {
            // A content token is data in a flat Node, so its extent comes from the text pool and
            // the walk never descends into it.
            let text = matches!(explorer_terminal, ExplorerTerminalPlan::Content(_));
            let child = match explorer_terminal {
                ExplorerTerminalPlan::Node(child) => Some(*child),
                ExplorerTerminalPlan::Content(_) => None,
            };
            let extent = |handle: proc_macro2::TokenStream| {
                if text {
                    quote!(ast.text_span(#handle))
                } else {
                    quote!(ast.span(#handle))
                }
            };
            let descend = |handle: proc_macro2::TokenStream| match child {
                Some(child) => render_child_walk(context, child, &handle),
                None => Ok(quote!()),
            };
            let repetition = wrappers.iter().any(|wrapper| {
                matches!(wrapper, FieldWrapper::ZeroOrMore | FieldWrapper::OneOrMore)
            });
            let optional = wrappers
                .iter()
                .any(|wrapper| matches!(wrapper, FieldWrapper::Optional));
            if repetition {
                let run = if optional {
                    quote!(match #access {
                        ::core::option::Option::Some(__recursa_list) => &ast[__recursa_list],
                        ::core::option::Option::None => &[],
                    })
                } else {
                    quote!(&ast[#access])
                };
                let first = extent(quote!(*__recursa_first));
                let last = extent(quote!(*__recursa_last));
                let walk = descend(quote!(*__recursa_item))?;
                let note = record.as_ref().map(|record| {
                    let call = builder.record_span(record, &site, &quote!(__recursa_run));
                    if record.absent {
                        quote!(if __recursa_items.is_empty() { #call })
                    } else {
                        quote!(if !__recursa_items.is_empty() { #call })
                    }
                });
                let run_extent = note.is_some().then(|| {
                    quote! {
                        let __recursa_run = #facade::__private::flat_run_extent(
                            __recursa_items.first().and_then(
                                |__recursa_first| #first,
                            ),
                            __recursa_items.last().and_then(
                                |__recursa_last| #last,
                            ),
                        );
                    }
                });
                let item = (!walk.is_empty()).then(|| {
                    quote! {
                        for __recursa_item in __recursa_items {
                            #walk
                        }
                    }
                });
                if note.is_none() && item.is_none() {
                    return Ok(quote!());
                }
                return Ok(quote! {
                    {
                        let __recursa_items = #run;
                        #run_extent
                        #note
                        #item
                    }
                });
            }
            if optional {
                let present = record
                    .as_ref()
                    .filter(|record| !record.absent)
                    .map(|record| {
                        let extent = extent(quote!(__recursa_value));
                        builder.record_span(record, &site, &quote!(#extent))
                    });
                let missing = record
                    .as_ref()
                    .filter(|record| record.absent)
                    .map(|record| builder.record_span(record, &site, &owner_extent));
                let walk = descend(quote!(__recursa_value))?;
                if present.is_none() && missing.is_none() && walk.is_empty() {
                    return Ok(quote!());
                }
                return Ok(quote! {
                    match #access {
                        ::core::option::Option::Some(__recursa_value) => { #present #walk }
                        ::core::option::Option::None => { #missing }
                    }
                });
            }
            let note = record.as_ref().map(|record| {
                let extent = extent(quote!(#access));
                builder.record_span(record, &site, &quote!(#extent))
            });
            let walk = descend(quote!(#access))?;
            Ok(quote!(#note #walk))
        }
    }
}

/// Renders one application's flat walk body: its own note, then its elements in source order.
fn render_flat_walk_body(
    context: &FlatWalkContext<'_>,
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    flat_ty: &Type,
    builder: &mut NoteBuilder<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let requirements = &plan.requirements;
    let own = requirements
        .nodes
        .get(&node.node)
        .map(|record| builder.record_span(record, "", &quote!(__recursa_span)));
    let field = |field: &FieldPlan| {
        field.ident.as_ref().map_or_else(
            || {
                let index = syn::Index::from(field.index);
                quote!(__recursa_node.#index)
            },
            |ident| quote!(__recursa_node.#ident),
        )
    };
    match &application.body {
        NodeBodyPlan::Struct { elements, .. } => {
            let mut steps = Vec::new();
            for element in elements {
                let record = match &element.kind {
                    ElementParseKind::Semantic { field, .. } => requirements
                        .fields
                        .get(&(node.node, None, field.index))
                        .cloned(),
                    ElementParseKind::Fixed { .. }
                    | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
                };
                let access = match &element.kind {
                    ElementParseKind::Semantic { field: plan, .. } => field(plan),
                    ElementParseKind::Fixed { .. }
                    | ElementParseKind::OptionalFixedTokenGroup { .. } => quote!(),
                };
                steps.push(render_flat_element(
                    context, builder, record, element, &access,
                )?);
            }
            Ok(quote!(#own #(#steps)*))
        }
        NodeBodyPlan::Enum { variants } => {
            let path = flat_pattern_path(flat_ty)?;
            let mut arms = Vec::new();
            for variant in variants {
                let site = format!("::{}", variant.ident);
                let variant_note = requirements
                    .variants
                    .get(&variant.resolved_variant)
                    .map(|record| builder.record_span(record, &site, &quote!(__recursa_span)));
                let mut steps = Vec::new();
                let mut bound = Vec::new();
                for element in &variant.elements {
                    let ElementParseKind::Semantic {
                        field: field_plan, ..
                    } = &element.kind
                    else {
                        continue;
                    };
                    let record = requirements
                        .fields
                        .get(&(node.node, Some(variant.resolved_variant), field_plan.index))
                        .cloned();
                    let binding = format_ident!("__recursa_field_{}", field_plan.index);
                    let step =
                        render_flat_element(context, builder, record, element, &quote!(#binding))?;
                    if !step.is_empty() {
                        bound.push(field_plan.index);
                    }
                    steps.push(step);
                }
                let pattern = flat_variant_pattern(&path, variant, &bound);
                arms.push(quote!(#pattern => { #variant_note #(#steps)* }));
            }
            Ok(quote! {
                #own
                match __recursa_node { #(#arms)* }
            })
        }
    }
}

/// Renders one enum variant's flat pattern, binding only the fields the walk reads.
fn flat_variant_pattern(
    path: &syn::Path,
    variant: &VariantParsePlan,
    bound: &[usize],
) -> proc_macro2::TokenStream {
    let ident = &variant.ident;
    let fields = variant
        .elements
        .iter()
        .filter_map(|element| match &element.kind {
            ElementParseKind::Semantic { field, .. } => Some(field),
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .collect::<Vec<_>>();
    match variant.style {
        ResolvedFieldStyle::Named => {
            let entries = fields
                .iter()
                .filter(|field| bound.contains(&field.index))
                .map(|field| {
                    let name = field
                        .ident
                        .as_ref()
                        .expect("a named variant field has an identifier");
                    let binding = format_ident!("__recursa_field_{}", field.index);
                    quote!(#name: #binding)
                });
            quote!(#path::#ident { #(#entries,)* .. })
        }
        ResolvedFieldStyle::Unnamed => {
            let width = fields
                .iter()
                .map(|field| field.index + 1)
                .max()
                .unwrap_or_default();
            let mut positions = (0..width).map(|_| quote!(_)).collect::<Vec<_>>();
            for field in fields.iter().filter(|field| bound.contains(&field.index)) {
                let binding = format_ident!("__recursa_field_{}", field.index);
                positions[field.index] = quote!(#binding);
            }
            quote!(#path::#ident(#(#positions),*))
        }
        ResolvedFieldStyle::Unit => quote!(#path::#ident),
    }
}

/// One application's rendered flat walk, ready to emit beside its note table.
struct RenderedFlatWalk {
    /// The function and its note table; `None` when the application can hold no requirement.
    tokens: Option<proc_macro2::TokenStream>,
}

/// Renders one application's flat walk function, or nothing when its ceiling is the baseline.
fn render_flat_walk(
    context: &FlatWalkContext<'_>,
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
) -> Result<RenderedFlatWalk, String> {
    let facade = context.facade;
    let anchor = context.anchor;
    if plan.requirements.ceiling(node.node) == 0 {
        return Ok(RenderedFlatWalk { tokens: None });
    }
    let flat_ty = flat_application_type(plan, node, application)?;
    let table = flat_note_table_ident(application);
    let mut builder = NoteBuilder::new(
        facade,
        table.clone(),
        &plan.requirements.configurations,
        node.ident.unraw().to_string(),
    );
    let body = render_flat_walk_body(context, plan, node, application, &flat_ty, &mut builder)?;
    let notes = builder.into_notes();
    let count = notes.len();
    let declaration = (count != 0).then(|| {
        quote! {
            static #table: [#facade::RequirementNote; #count] = [#(#notes),*];
        }
    });
    let walk = flat_walk_ident(application);
    let (handle, read) = if node.publicly_materialized {
        (
            quote!(__recursa_handle: #facade::__private::Handle<#flat_ty>),
            quote!(),
        )
    } else {
        (
            quote!(__recursa_raw: ::core::num::NonZeroU32),
            quote! {
                let __recursa_handle =
                    #facade::__private::opaque_handle::<#flat_ty>(__recursa_raw);
            },
        )
    };
    let visibility = (!node.publicly_materialized).then(|| quote!(pub));
    let hidden = (!node.publicly_materialized).then(|| quote!(#[doc(hidden)]));
    Ok(RenderedFlatWalk {
        tokens: Some(quote! {
            #declaration

            #hidden
            #visibility fn #walk<__RecursaRoot: #facade::__private::FlatNode>(
                ast: &#anchor::FlatAst<'_, __RecursaRoot>,
                #handle,
                scan: &mut #facade::RequirementScan,
            ) {
                if scan.finished() {
                    return;
                }
                #read
                let __recursa_node = ast[__recursa_handle];
                let __recursa_span = ast.span(__recursa_handle);
                let _ = (&__recursa_node, &__recursa_span);
                #body
            }
        }),
    })
}

/// Renders the flat walk one Node's own module holds, for a Node the grammar root cannot name.
///
/// A non-public Node has no `FlatRequires` implementation, because the owner's root type is always
/// publicly reachable. Its parent calls this function by path instead, with the raw handle, which
/// is how [`super::flat_traversal`] reaches such a Node's children.
pub(super) fn render_node_flat_requires(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
) -> Result<proc_macro2::TokenStream, String> {
    if !flat_requires_declared(plan) || node.publicly_materialized {
        return Ok(proc_macro2::TokenStream::new());
    }
    let targets = flat_walk_targets(plan)?;
    let context = FlatWalkContext {
        facade: &plan.facade,
        anchor: &plan.module_anchor,
        targets: &targets,
    };
    let mut items = Vec::new();
    for application in &node.parse_applications {
        if let Some(tokens) = render_flat_walk(&context, plan, node, application)?.tokens {
            items.push(tokens);
        }
    }
    Ok(quote!(#(#items)*))
}

/// Reports whether this grammar generates a flat requirement walk at all.
///
/// Both halves are needed: the stores, and the gates the walk reports. A grammar that declares
/// `flat` without `configurations(...)`, or lists `Requires` without `flat`, emits nothing here.
fn flat_requires_declared(plan: &EmissionPlan) -> bool {
    plan.flat
        && plan.requirements.is_declared()
        && plan.nodes.iter().all(|node| {
            node.configured_traits
                .contains(&ConfiguredTraitRecipe::Requires)
        })
}

/// Renders the grammar's flat requirement surface at the grammar root.
///
/// # Long-function contract
///
/// Rationale: the trait, its implementations, the walk functions and the owner's two methods are
/// one contract. The set of publicly materialized applications decides which implementations exist,
/// and each implementation names the walk of its own application. The parts are therefore rendered
/// together or not at all.
/// Invariants: one implementation per publicly materialized application; one walk per application
/// whose ceiling is above the baseline; notes recorded in authored source order; every answer
/// reached through `recursa-core`'s own fold, ceiling test and answer.
/// Phases: collects the walk of every application, renders the public walks, then renders the
/// trait, its implementations and the owner's methods.
pub(super) fn render_flat_requires(
    plan: &EmissionPlan,
) -> Result<proc_macro2::TokenStream, String> {
    if !flat_requires_declared(plan) {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let targets = flat_walk_targets(plan)?;
    let context = FlatWalkContext {
        facade,
        anchor: &plan.module_anchor,
        targets: &targets,
    };
    let mut walks = Vec::new();
    let mut implementations = Vec::new();
    for node in plan.nodes.iter().filter(|node| node.publicly_materialized) {
        let ceiling = plan.requirements.ceiling(node.node);
        for application in &node.parse_applications {
            let flat_ty = flat_application_type(plan, node, application)?;
            if let Some(tokens) = render_flat_walk(&context, plan, node, application)?.tokens {
                walks.push(tokens);
            }
            let walk = flat_walk_ident(application);
            let body = (ceiling != 0).then(|| quote!(#walk(ast, handle, scan);));
            implementations.push(quote! {
                impl FlatRequires for #flat_ty {
                    const REQUIREMENT_CEILING: #facade::Configuration =
                        #facade::Configuration::from_ordinal(#ceiling);

                    fn __recursa_scan_flat_requirements<
                        __RecursaRoot: #facade::__private::FlatNode,
                    >(
                        ast: &FlatAst<'_, __RecursaRoot>,
                        handle: #facade::__private::Handle<Self>,
                        scan: &mut #facade::RequirementScan,
                    ) {
                        let _ = (ast, handle, &scan);
                        #body
                    }
                }
            });
        }
    }
    if implementations.is_empty() {
        return Ok(proc_macro2::TokenStream::new());
    }
    Ok(quote! {
        /// Names one flat Node type's own requirement walk and its ceiling.
        ///
        /// A `FlatAst`'s root type is a type parameter, so the owner cannot name the walk of its
        /// own root by itself. Generation implements this trait for every publicly materialized
        /// application, exactly as it implements `FlatNodeRef`.
        pub trait FlatRequires: #facade::__private::FlatNode {
            /// No construct inside a Node of this type requires a configuration above this one.
            ///
            /// It is the same generated literal `recursa::Requires::REQUIREMENT_CEILING` carries
            /// for the nested Node, because both come from one fixpoint over the type graph.
            const REQUIREMENT_CEILING: #facade::Configuration;

            /// Walks the Node `handle` names, in authored source order. Generated code only.
            #[doc(hidden)]
            fn __recursa_scan_flat_requirements<
                __RecursaRoot: #facade::__private::FlatNode,
            >(
                ast: &FlatAst<'_, __RecursaRoot>,
                handle: #facade::__private::Handle<Self>,
                scan: &mut #facade::RequirementScan,
            );
        }

        #(#implementations)*

        #(#walks)*

        impl<'__recursa_input, __RecursaRoot: FlatRequires>
            FlatAst<'__recursa_input, __RecursaRoot>
        {
            /// Returns the lowest configuration whose grammar accepts this result, with the extent
            /// of the first construct, in source order, that sets it.
            ///
            /// `None` means the grammar's baseline configuration accepts the whole result. This is
            /// the flat counterpart of `recursa::Parsed::minimum_requirement` and
            /// `recursa::ArenaParsed::minimum_requirement`, and it answers with the same
            /// configuration and the same extent: the fold, the ceiling test and the answer are
            /// the runtime's own, and only the walk reads the stores instead of a value.
            ///
            /// The extent is `None` for a parse that retained no span, which is
            /// `parse_flat_without_spans` or a build without the `spans` feature.
            pub fn minimum_requirement(&self) -> ::core::option::Option<#facade::Requirement> {
                self.__recursa_scan_requirements(
                    #facade::__private::requirement_minimum_scan(),
                )
            }

            /// Returns the first construct, in source order, whose requirement is above `floor`.
            ///
            /// This is the flat counterpart of `recursa::Parsed::requirement_above`.
            pub fn requirement_above(
                &self,
                floor: #facade::Configuration,
            ) -> ::core::option::Option<#facade::Requirement> {
                self.__recursa_scan_requirements(
                    #facade::__private::requirement_above_scan(floor),
                )
            }

            /// Runs one scan from this result's root, so both questions share one walk.
            fn __recursa_scan_requirements(
                &self,
                scan: #facade::RequirementScan,
            ) -> ::core::option::Option<#facade::Requirement> {
                #facade::__private::requirement_scan_root(
                    scan,
                    __RecursaRoot::REQUIREMENT_CEILING,
                    |scan| {
                        __RecursaRoot::__recursa_scan_flat_requirements(
                            self,
                            self.root(),
                            scan,
                        );
                    },
                )
            }
        }
    })
}
