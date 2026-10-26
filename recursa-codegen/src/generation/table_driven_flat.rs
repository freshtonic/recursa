//! Flat AST construction. This module renders the reduce actions that build a Flat AST on the LR
//! automaton, and the `parse_flat` entry points that run them.
//!
//! [`super::table_driven`] renders the nested reduce actions. This module renders a second set of
//! actions over the same tables, the same rules and the same value stack. Nothing about the
//! automaton changes. A flat action pops and pushes exactly as many values as the nested action
//! for the same rule. The packed `value_pops` table therefore stays valid for both, and the two
//! dispatchers cover exactly the same rules.
//!
//! What the value stack holds differs:
//!
//! - A Node's Construct action puts the Node in its pool and pushes the four-byte `Handle`.
//! - A content token's action appends its text to the text pool and pushes a text handle.
//! - A list step pushes the `OpenList` it builds on the parse's one scratch stack. The rule that
//!   consumes the list closes it into the list store.
//! - `boxed!` disappears, because every child is already a handle.
//! - A fixed-token unit Node carries no value on the stack at all, exactly as in the nested path.
//!   Its parent allocates it and keeps the handle.
//!
//! The runtime types are `recursa-core`'s `flat` module.
//!
//! Each application also gets `parse_flat_reusing` and `parse_flat_reusing_without_spans`. They
//! run the same automaton over the same stores; the stores come from the caller's buffer slot and
//! go back into it when the parse fails. [`super::flat`] renders `FlatBuffers` and the builder
//! constructor they use.
//!
//! A parent reduces after its children, so every typed pool comes out in post-order. Invariant I1
//! of `docs/flat-ast-design.md` therefore holds by construction, with no ordering work here.
//!
//! Spans come from the LR extent stack in `recursa-core/src/lr.rs`, which exists only while a
//! parse tracks extents. A Node's span is its own rule's extent. A text handle's span is its token
//! record's span. A Node the parent allocates takes the extent of the site it occupies. Flat
//! parsing never builds the occurrence tree, as
//! `docs/adr/0008-keep-flat-asts-semantic-only.md` requires.

use std::borrow::Cow;

use super::flat::{content_token_static_type, flat_application_type, flat_terminal_type};
use super::parsing::{
    ElementParseKind, ElementParsePlan, FieldPlan, FieldWrapper, NodeApplicationParsePlan,
    NodeBodyPlan, NodeParsePlan, SemanticParseProjection, render_construction,
};
use super::table_driven::{
    StartTables, TableDrivenRendering, application_reduce_actions, input_lifetime, plan_variant,
};
use super::*;
use crate::lr::{ElementValue, OptionalContent, Rule, RuleAction, SiteForm, Symbol};

/// The safety note every generated flat reduce action carries.
const SAFETY: &str = "Reduce actions of the flat construction backend. SAFETY: each `pop` names \
                      the type the child rule's flat action pushed for the same grammar symbol, \
                      and pops happen in reverse right-hand-side order. A flat action pops and \
                      pushes the same counts as the nested action of the same rule, so both share \
                      the packed tables.";

/// Names the declaration-local flat reducer shim of one materialized node application.
fn flat_reducer_ident(application: &NodeApplicationParsePlan, action: usize) -> Ident {
    format_ident!(
        "__recursa_reduce_flat_{}_{}",
        application.parse_index,
        action
    )
}

/// Names the flat reduce method emitted on the application's own flat type.
fn flat_method_ident(application: &NodeApplicationParsePlan, action: usize) -> Ident {
    format_ident!(
        "__recursa_reduce_flat_action_{}_{}",
        application.parse_index,
        action
    )
}

/// Resolves the declaration-local flat shim without naming the flat type at the grammar root.
///
/// The root can name neither a Node declared without `pub` nor that Node's flat type. The root
/// dispatcher therefore calls a free shim in the declaration's own module. The nested dispatcher
/// does exactly the same.
pub(super) fn flat_reducer_path(
    application: &NodeApplicationParsePlan,
    action: usize,
) -> Result<syn::Path, String> {
    let Type::Path(type_path) = &application.ty else {
        return Err("a materialized node application must have a path type".to_owned());
    };
    let mut path = type_path.path.clone();
    path.segments
        .pop()
        .ok_or_else(|| "a materialized node application path is empty".to_owned())?;
    path.segments
        .push(syn::PathSegment::from(flat_reducer_ident(
            application,
            action,
        )));
    Ok(path)
}

/// The builder type every flat reduce action appends to.
fn builder_type(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let module_anchor = &plan.module_anchor;
    let input_lifetime = input_lifetime();
    quote!(#module_anchor::FlatAstBuilder<#input_lifetime>)
}

/// The reduction parameter of a flat reduce action.
///
/// Flat construction never touches the AST arena, so the arena lifetime stays elided and one
/// signature serves both the ordinary and the `arena_ast` grammar.
fn reduction_parameter(plan: &EmissionPlan) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let input_lifetime = input_lifetime();
    quote!(&mut #facade::__private::LrReduction<'_, '_, #input_lifetime, '_, A>)
}

/// Renders the expression that appends one flat Node to its store and yields its handle.
///
/// A pooled application appends to its own typed pool; every other application appends to the
/// shared pool. Both accessors are crate-private, so the declaration's own module can call them.
fn flat_alloc(
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    value: proc_macro2::TokenStream,
    span: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if node.flat.pool {
        let accessor = format_ident!("pool_{}_mut", application.parse_index);
        quote!(builder.#accessor().push(#value, #span))
    } else {
        quote!(builder.shared_mut().alloc(#value, #span))
    }
}

/// Finds the Node application one field terminal names, for a child the parent allocates itself.
fn terminal_application<'plan>(
    plan: &'plan EmissionPlan,
    terminal_node: Option<NodeId>,
    terminal: &Type,
) -> Option<(&'plan NodeParsePlan, &'plan NodeApplicationParsePlan)> {
    let terminal_node = terminal_node?;
    let node = plan
        .nodes
        .iter()
        .find(|candidate| candidate.node == terminal_node)?;
    let application = node
        .parse_applications
        .iter()
        .find(|candidate| candidate.ty == *terminal)
        .or(match node.parse_applications.as_slice() {
            [single] => Some(single),
            _ => None,
        })?;
    Some((node, application))
}

/// Renders the value of a field whose type carries nothing on the value stack: a fixed-token unit
/// Node, which the flat parent allocates and refers to by handle.
///
/// The nested action names the unit struct instead of popping (`unit_value` in
/// [`super::table_driven`]); a flat Node holds a handle, so the parent must put the child in a
/// store. Its span is the extent of the site it occupies, which is the only place the child's
/// tokens appear.
fn flat_unit_value(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    terminal: &Type,
    terminal_node: Option<NodeId>,
    span: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String> {
    let Some((target, application)) = terminal_application(plan, terminal_node, terminal) else {
        return Err(format!(
            "`{}`: a valueless field must name a generated unit fixed-token Node",
            node.ident
        ));
    };
    let is_unit = matches!(
        &target.body,
        NodeBodyPlan::Struct { style: ResolvedFieldStyle::Unit, elements }
            if elements.iter().all(|element| matches!(element.kind, ElementParseKind::Fixed { .. }))
    );
    if !is_unit {
        return Err(format!(
            "`{}`: a field of a fixed-token type must be a unit struct for flat construction",
            node.ident
        ));
    }
    let flat_ty = flat_application_type(plan, target, application)?;
    if !type_arguments(&flat_ty).is_empty() {
        return Err(format!(
            "`{}`: a field of a fixed-token type must be a unit struct without generics for flat \
             construction",
            node.ident
        ));
    }
    Ok(flat_alloc(target, application, quote!(#flat_ty), span))
}

/// Returns the angle-bracketed type arguments of a type's last path segment.
fn type_arguments(ty: &Type) -> Vec<&syn::GenericArgument> {
    let Type::Path(path) = ty else {
        return Vec::new();
    };
    let Some(syn::PathArguments::AngleBracketed(arguments)) =
        path.path.segments.last().map(|segment| &segment.arguments)
    else {
        return Vec::new();
    };
    arguments.args.iter().collect()
}

/// One direct field as a flat reduce action sees it.
struct FlatFieldSite<'a> {
    /// Materialized terminal type before the flat mapping.
    terminal: &'a Type,
    /// Resolved Node family, when the terminal is generated.
    terminal_node: Option<NodeId>,
    /// The flat storage type of one element of the field: a Node or a text handle.
    item: &'a Type,
    /// Authored type wrappers, outermost first.
    wrappers: &'a [FieldWrapper],
    /// How the site appears on the rule's right-hand side.
    form: SiteForm,
    /// The right-hand-side symbols the site occupies.
    symbols: &'a [Symbol],
}

/// Renders the expression that closes the scratch list one symbol pushed, into the list store.
///
/// The open list is popped from the value stack exactly where the nested action pops its `Vec1`.
/// A close at the consumer is correct: LR completes an inner list before the outer list receives
/// its next item. The runtime's `ListScratch::push` asserts that in debug builds.
fn close_list(plan: &EmissionPlan, item: &Type, non_empty: bool) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let close = if non_empty {
        quote!(close1)
    } else {
        quote!(close)
    };
    quote!({
        let __recursa_open = unsafe { reduction.pop::<#facade::__private::OpenList>() };
        let (__recursa_scratch, __recursa_lists) = builder.lists_and_scratch_mut();
        __recursa_scratch.#close::<#item>(__recursa_open, __recursa_lists)
    })
}

/// Renders the empty run a zero-or-more field holds when its list is absent.
///
/// An empty run is a valid list handle distinct from an absent list, and it consumes no slot.
fn empty_list(item: &Type) -> proc_macro2::TokenStream {
    quote!(builder.lists_mut().push(::core::iter::empty::<#item>()))
}

/// Renders the flat value of one direct field from its site form.
///
/// This mirrors `render_field_value` in [`super::table_driven`] one branch at a time, because the
/// two actions must pop the same stack entries in the same order. Only what each branch yields
/// differs: a handle instead of a value, a closed list handle instead of a `Vec`, and no `Box`
/// layer at all.
fn render_flat_field_value(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    lr: &TableDrivenRendering,
    site: FlatFieldSite<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let is_choice = |wrapper: &FieldWrapper| !matches!(wrapper, FieldWrapper::Boxed);
    // One element: popped through the site's value-carrying symbol, or allocated here when the
    // site's symbols carry no value at all.
    let element = || -> Result<proc_macro2::TokenStream, String> {
        let carries = site
            .symbols
            .iter()
            .any(|symbol| lr.symbol_carries_value(*symbol));
        if carries {
            return Ok(quote!(unsafe { reduction.pop() }));
        }
        let (start, len) = (site.form.start as usize, site.form.len as usize);
        let span = quote!(reduction.symbol_range_extent(#start, #len));
        flat_unit_value(plan, node, site.terminal, site.terminal_node, span)
    };
    let Some(depth) = site.wrappers.iter().position(is_choice) else {
        return element();
    };
    if !site.form.inlined {
        // A required site or a helper nonterminal: its one symbol carries the whole field value.
        // A one-or-more helper pushes the open list itself, so this action closes it.
        return Ok(match site.wrappers[depth] {
            FieldWrapper::OneOrMore => close_list(plan, site.item, true),
            _ => quote!(unsafe { reduction.pop() }),
        });
    }
    let inner = match (&site.wrappers[depth], site.form.present) {
        (FieldWrapper::Optional, false) => quote!(None),
        (FieldWrapper::ZeroOrMore, false) => empty_list(site.item),
        (FieldWrapper::ZeroOrMore, true) => close_list(plan, site.item, false),
        (FieldWrapper::Optional, true) => {
            let rest = &site.wrappers[depth + 1..];
            let inner = match rest.iter().position(is_choice) {
                None => element()?,
                Some(list_depth) => match rest[list_depth] {
                    FieldWrapper::OneOrMore => close_list(plan, site.item, true),
                    FieldWrapper::ZeroOrMore => close_list(plan, site.item, false),
                    FieldWrapper::Boxed | FieldWrapper::Optional => {
                        return Err(format!(
                            "`{}`: an optional field wraps a list or one element",
                            node.ident
                        ));
                    }
                },
            };
            quote!(Some(#inner))
        }
        (FieldWrapper::OneOrMore, _) | (FieldWrapper::Boxed, _) => {
            return Err(format!(
                "`{}`: a one-or-more list is never an inlined choice",
                node.ident
            ));
        }
    };
    Ok(inner)
}

/// Renders one flat Construct action: the field values, the flat Node, and its handle.
///
/// # Long-function contract
///
/// Rationale: the rule's site forms and the application's elements are walked together, in
/// reverse, exactly as the nested action walks them; splitting the walk would let the two
/// representations pop the value stack differently.
/// Invariants: one popped value per value-carrying site, in reverse right-hand-side order; the
/// Node is allocated after its children, which is what keeps every typed pool in post-order.
/// Phases: resolves the rule's variant and elements, renders one value per semantic element, then
/// allocates the Node and pushes its handle.
fn render_flat_reduce_body(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    lr: &TableDrivenRendering,
    rule: &Rule,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let RuleAction::Construct { variant, sites, .. } = &rule.action else {
        return Ok(quote!({}));
    };
    let (variant_ident, variant_id, style, elements): (
        Option<&Ident>,
        Option<ResolvedVariantId>,
        ResolvedFieldStyle,
        Cow<'_, [ElementParsePlan]>,
    ) = match (&application.body, variant) {
        (NodeBodyPlan::Struct { style, elements }, None) => {
            (None, None, *style, Cow::Borrowed(elements.as_slice()))
        }
        (NodeBodyPlan::Enum { variants, .. }, Some(declaration)) => {
            let resolved = plan_variant(lr, application, *declaration)?;
            let variant = variants
                .iter()
                .find(|variant| variant.resolved_variant == resolved)
                .ok_or_else(|| {
                    format!("`{}` has no plan for one of its alternatives", node.ident)
                })?;
            (
                Some(&variant.ident),
                Some(resolved),
                variant.style,
                Cow::Borrowed(variant.elements.as_slice()),
            )
        }
        _ => {
            return Err(format!(
                "`{}`: the lowered rule and the node plan disagree on the node's shape",
                node.ident
            ));
        }
    };
    if elements.len() != sites.len() {
        return Err(format!(
            "`{}`: rule has {} sites but the node plan has {} elements",
            node.ident,
            sites.len(),
            elements.len()
        ));
    }
    let mut pops = Vec::new();
    let mut values: Vec<(FieldPlan, Ident)> = Vec::new();
    for (position, (element, form)) in elements.iter().zip(sites.iter()).enumerate().rev() {
        let ElementParseKind::Semantic { field, projection } = &element.kind else {
            continue;
        };
        let symbols = &rule.rhs[form.start as usize..(form.start + form.len) as usize];
        let value = format_ident!("__recursa_value_{position}");
        let expression = match projection.as_ref() {
            SemanticParseProjection::FixedPresence { .. } => {
                if form.inlined {
                    let present = form.present;
                    quote!(#present)
                } else {
                    quote!(unsafe { reduction.pop::<bool>() })
                }
            }
            projection @ SemanticParseProjection::Direct {
                terminal,
                terminal_node,
                wrappers,
                ..
            } => {
                let item = flat_terminal_type(plan, facade, &[], projection)?.ok_or_else(|| {
                    format!("`{}`: a direct field has a flat terminal type", node.ident)
                })?;
                render_flat_field_value(
                    plan,
                    node,
                    lr,
                    FlatFieldSite {
                        terminal,
                        terminal_node: *terminal_node,
                        item: &item,
                        wrappers,
                        form: *form,
                        symbols,
                    },
                )?
            }
        };
        pops.push(quote!(let #value = #expression;));
        values.push((field.clone(), value));
    }
    values.reverse();
    // A struct with text rules checks the flat value it has just built, before it is allocated.
    let check = match super::table_driven::struct_check_site(plan, lr, rule, node, &elements, sites)
    {
        Some((start, len)) if variant_ident.is_none() => super::rejections::render_struct_check(
            plan,
            node,
            super::rejections::StructFieldText::Flat {
                value: &format_ident!("__recursa_node"),
                read_pool: &|target, handle| flat_pool_read(plan, target, handle),
                flat_path_of: &|target| flat_pattern_path(plan, target),
            },
            quote!(reduction.symbol_range_extent(#start, #len)),
        )?,
        _ => quote!(),
    };
    // A refused variant or a validation hook reads the flat value it has just built, before it is
    // allocated, exactly where the nested reduce reads its value.
    let validation = render_reduce_checks(
        plan,
        node,
        variant_id,
        &field_extents(lr, rule, &elements, sites),
        BuiltNode::Flat(&format_ident!("__recursa_node"), &syn::parse_quote!(Self)),
    )?;
    let construction = render_construction(variant_ident, style, &values);
    let allocation = flat_alloc(
        node,
        application,
        quote!(__recursa_node),
        quote!(__recursa_span),
    );
    Ok(quote! {
        {
            #(#pops)*
            let __recursa_span = reduction.extent();
            let __recursa_node: Self = #construction;
            #check
            #validation
            let __recursa_handle = #allocation;
            reduction.push(__recursa_handle);
        }
    })
}

/// The flat storage type and value of one element a helper rule carries.
struct FlatElement {
    /// The element's flat storage type: a Node handle or a text handle.
    ty: Type,
    /// The expression yielding one element, popped or allocated.
    value: proc_macro2::TokenStream,
}

/// Spells one helper rule's element from the grammar root.
///
/// A helper nonterminal is shared. Its action therefore names the element through the flat type
/// of the Node application, or through the content token it stands for. The nested helper names
/// the nested type the same way. `span` is the extent an allocated unit Node takes.
fn flat_element(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
    element: ElementValue,
    span: proc_macro2::TokenStream,
) -> Result<FlatElement, String> {
    let facade = &plan.facade;
    if let Some(application) = lr.application_of_type(element.type_id) {
        let (node, application) = plan
            .nodes
            .iter()
            .flat_map(|node| {
                node.parse_applications
                    .iter()
                    .map(move |candidate| (node, candidate))
            })
            .find(|(_, candidate)| candidate.application == application)
            .ok_or_else(|| "a lowered element has no planned node application".to_owned())?;
        let flat_ty = flat_application_type(plan, node, application)?;
        let ty: Type = syn::parse_quote!(#facade::__private::Handle<#flat_ty>);
        let value = if lr.type_carries_value(element.type_id) {
            quote!(unsafe { reduction.pop::<#ty>() })
        } else {
            flat_alloc(node, application, quote!(#flat_ty), span)
        };
        return Ok(FlatElement { ty, value });
    }
    let Some(token) = lr.token_of_type(element.type_id) else {
        return Err(
            "a lowered element is neither a node application nor a content token".to_owned(),
        );
    };
    let token = content_token_static_type(plan, token)?;
    let ty: Type = syn::parse_quote!(#facade::__private::Text<#token>);
    Ok(FlatElement {
        value: quote!(unsafe { reduction.pop::<#ty>() }),
        ty,
    })
}

/// Renders the flat reduce action of one helper or envelope rule.
///
/// # Long-function contract
///
/// Rationale: one match over [`RuleAction`] keeps every helper's flat value beside the nested
/// value it replaces. That is the only way to see that both pop the same stack entries.
/// Invariants: a presence pushes a `bool`. An optional pushes an `Option`. A list step pushes the
/// open scratch list. The rule that consumes a list closes it into the list store.
/// Phases: one arm per helper action; an erased optional group renders nothing at all.
fn render_flat_root_action(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
    index: u32,
    rule: &Rule,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    // A helper rule's element is its last symbol, so that is the extent of a unit Node the action
    // allocates. Every earlier symbol is the helper's own fixed token or the list it extends.
    let element_span = match rule.rhs.len().checked_sub(1) {
        Some(last) => quote!(reduction.symbol_extent(#last)),
        None => quote!(None),
    };
    let body = match &rule.action {
        RuleAction::Envelope { present, .. } => {
            let present = *present;
            quote!(reduction.push(#present);)
        }
        RuleAction::OptionalAbsent { content } => match content {
            OptionalContent::Erased => quote!(),
            OptionalContent::Presence => quote!(reduction.push(false);),
            OptionalContent::Element(element) => {
                let ty = flat_element(plan, lr, *element, element_span)?.ty;
                quote!(reduction.push::<Option<#ty>>(None);)
            }
            OptionalContent::NonEmptyList { element, .. } => {
                let ty = flat_element(plan, lr, *element, element_span)?.ty;
                quote!(reduction.push::<Option<#facade::__private::List1<#ty>>>(None);)
            }
        },
        RuleAction::OptionalPresent { content } => match content {
            OptionalContent::Erased => quote!(),
            OptionalContent::Presence => quote!(reduction.push(true);),
            OptionalContent::Element(element) => {
                let value = flat_element(plan, lr, *element, element_span)?.value;
                quote! {
                    let __recursa_value = #value;
                    reduction.push(Some(__recursa_value));
                }
            }
            OptionalContent::NonEmptyList { element, .. } => {
                let ty = flat_element(plan, lr, *element, element_span)?.ty;
                let list = close_list(plan, &ty, true);
                quote! {
                    let __recursa_list = #list;
                    reduction.push(Some(__recursa_list));
                }
            }
        },
        RuleAction::ListEmpty { element } => {
            let ty = flat_element(plan, lr, *element, element_span)?.ty;
            let empty = empty_list(&ty);
            quote! {
                let __recursa_list = #empty;
                reduction.push(__recursa_list);
            }
        }
        RuleAction::ListFirst { element } => {
            let value = flat_element(plan, lr, *element, element_span)?.value;
            // A text rule refuses one item on its own, so it runs on the first item too.
            let rejection = render_flat_list_check(plan, lr, *element, rule, true)?;
            quote! {
                let __recursa_element = #value;
                #rejection
                let (__recursa_scratch, _) = builder.lists_and_scratch_mut();
                let mut __recursa_open = __recursa_scratch.open();
                __recursa_scratch.push(&mut __recursa_open, __recursa_element);
                reduction.push(__recursa_open);
            }
        }
        RuleAction::ListPush { element } => {
            let value = flat_element(plan, lr, *element, element_span)?.value;
            // The flat check reads the same rule table the nested check reads, and reports the
            // same extent: the extent of the element symbol this step consumed.
            let rejection = render_flat_list_check(plan, lr, *element, rule, false)?;
            quote! {
                let __recursa_element = #value;
                let mut __recursa_open =
                    unsafe { reduction.pop::<#facade::__private::OpenList>() };
                #rejection
                let (__recursa_scratch, _) = builder.lists_and_scratch_mut();
                __recursa_scratch.push(&mut __recursa_open, __recursa_element);
                reduction.push(__recursa_open);
            }
        }
        RuleAction::ListFromNonEmpty { element } => {
            let ty = flat_element(plan, lr, *element, element_span)?.ty;
            let list = close_list(plan, &ty, false);
            quote! {
                let __recursa_list = #list;
                reduction.push(__recursa_list);
            }
        }
        other => {
            return Err(format!(
                "rule {index} with action {other:?} is not a root-reduced rule"
            ));
        }
    };
    Ok(body)
}

/// Renders the check one flat list step runs over the element type's rejection rules.
///
/// A flat list holds handles, so the key of an item is a match on the pooled value, and a
/// classified item reads its identifier text out of the pools and the text pool; nothing is
/// materialized. The items the list has passed are still on the scratch stack, and the item this
/// step adds is already in its pool. The first step of a list holds no passed item, so only an
/// element type with a rule that refuses one item on its own renders a check there.
fn render_flat_list_check(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
    element: ElementValue,
    rule: &Rule,
    first: bool,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let Some((index, node, application)) =
        super::table_driven::rejection_element(plan, lr, element)
    else {
        return Ok(proc_macro2::TokenStream::new());
    };
    if first
        && !plan
            .rejections
            .rules(node.node)
            .is_some_and(|rules| rules.refuses_single_items())
    {
        return Ok(proc_macro2::TokenStream::new());
    }
    let Some(last) = rule.rhs.len().checked_sub(1) else {
        return Ok(proc_macro2::TokenStream::new());
    };
    let table = super::table_driven::rejection_table_ident(index);
    let flat_ty = flat_application_type(plan, node, application)?;
    let flat_path = super::table_driven::bare_type_path(&flat_ty).ok_or_else(|| {
        format!(
            "`{}` declares rejection rules but its flat application is not a path type",
            node.ident
        )
    })?;
    let read_pool =
        |target: NodeId, handle: proc_macro2::TokenStream| flat_pool_read(plan, target, handle);
    let flat_path_of = |target: NodeId| flat_pattern_path(plan, target);
    let key = super::rejections::render_flat_key_closure(
        plan,
        node,
        &flat_ty,
        &flat_path,
        &read_pool,
        &flat_path_of,
    )?;
    let read = read_pool(node.node, quote!(__recursa_handle))?;
    let passed = if first {
        quote!(::core::iter::empty())
    } else {
        quote! {
            builder
                .scratch()
                .items::<#facade::__private::Handle<#flat_ty>>(__recursa_open)
                .map(|__recursa_handle| (__recursa_key(#read), ::core::option::Option::None))
        }
    };
    let added = quote! {
        {
            let __recursa_handle = __recursa_element;
            (__recursa_key(#read), reduction.symbol_extent(#last))
        }
    };
    let check = super::rejections::render_check(plan, &table, &passed, &added);
    Ok(quote! {
        {
            let __recursa_key = #key;
            #check
        }
    })
}

/// Reads one pooled flat value of `target` through `handle`, during a reduce that holds
/// `builder`.
fn flat_pool_read(
    plan: &EmissionPlan,
    target: NodeId,
    handle: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, String> {
    let (planned, application, ty) = flat_node(plan, target)?;
    Ok(if planned.flat.pool {
        let accessor = format_ident!("pool_{}", application.parse_index);
        quote! {
            builder
                .#accessor()
                .get(#handle)
                .expect("a handle of this parse always addresses its own pool")
        }
    } else {
        quote!(builder.shared().get::<#ty>(#handle))
    })
}

/// Names one Node's flat type in a match pattern.
fn flat_pattern_path(
    plan: &EmissionPlan,
    target: NodeId,
) -> Result<proc_macro2::TokenStream, String> {
    let (planned, _, ty) = flat_node(plan, target)?;
    super::table_driven::bare_type_path(&ty).ok_or_else(|| {
        format!(
            "`{}` is read by a rejection key but its flat application is not a path type",
            planned.ident
        )
    })
}

/// Returns the plan, the single application and the flat type of one Node a flat key reads.
fn flat_node(
    plan: &EmissionPlan,
    target: NodeId,
) -> Result<(&NodeParsePlan, &NodeApplicationParsePlan, Type), String> {
    let planned = plan
        .nodes
        .iter()
        .find(|candidate| candidate.node == target)
        .ok_or_else(|| "a Node a rejection key reads has no frozen parse plan".to_owned())?;
    let [application] = planned.parse_applications.as_slice() else {
        return Err(format!(
            "`{}` is read by a rejection key but has more than one application",
            planned.ident
        ));
    };
    let ty = flat_application_type(plan, planned, application)?;
    Ok((planned, application, ty))
}

/// Renders one node application's flat reduce shims, flat reduce methods, and `parse_flat` entry
/// points.
pub(super) fn render_node_application_flat(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    lr: &TableDrivenRendering,
    parser: &RenderedParser,
) -> Result<proc_macro2::TokenStream, String> {
    if !plan.flat {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime = input_lifetime();
    let tables = lr.application(application.application);
    let Some(start) = &tables.start else {
        return Ok(proc_macro2::TokenStream::new());
    };
    let flat_ty = flat_application_type(plan, node, application)?;
    let builder_ty = builder_type(plan);
    let reduction = reduction_parameter(plan);
    let actions = application_reduce_actions(tables)
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let shim_ident = flat_reducer_ident(application, index);
            let method_ident = flat_method_ident(application, index);
            let body = render_flat_reduce_body(plan, node, application, lr, action.rule)?;
            let shim = quote! {
                #[doc = #SAFETY]
                #[inline(always)]
                pub(in #module_anchor) fn #shim_ident<#input_lifetime, A>(
                    reduction: #reduction,
                    builder: &mut #builder_ty,
                ) -> Result<(), #facade::ParseError> {
                    <#flat_ty>::#method_ident(reduction, builder)
                }
            };
            let method = quote! {
                #[doc = #SAFETY]
                #[inline(never)]
                fn #method_ident<#input_lifetime, A>(
                    reduction: #reduction,
                    builder: &mut #builder_ty,
                ) -> Result<(), #facade::ParseError> {
                    #body
                    Ok(())
                }
            };
            Ok::<_, String>((shim, method))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let shims = actions.iter().map(|(shim, _)| shim);
    let methods = actions.iter().map(|(_, method)| method);
    let entry_points = render_flat_entry_points(plan, node, application, &flat_ty, start, parser);
    Ok(quote! {
        #(#shims)*

        impl #flat_ty {
            #(#methods)*
        }

        #entry_points
    })
}

/// Renders `parse_flat` and `parse_flat_without_spans` beside one application's `parse`.
///
/// The entry point lives on the nested application type, because a caller names that type. It
/// returns the grammar's own `FlatAst`, rooted at this application's flat type. The runtime owns
/// the grammar check, the cursor checkpoint and the restore on failure. This closure owns the
/// stores, the automaton call and the sealed result.
fn render_flat_entry_points(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    flat_ty: &Type,
    start: &StartTables,
    parser: &RenderedParser,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let tables_module = parser.tables_module(plan);
    let input_lifetime = input_lifetime();
    let nested_ty = &application.ty;
    let (impl_generics, method_generics) = if node.generics.lifetimes().next().is_some() {
        (quote!(<#input_lifetime>), quote!(<A>))
    } else {
        (quote!(), quote!(<#input_lifetime, A>))
    };
    let start_state = start.start_state;
    let accept_rule = start.accept_rule;
    // The bodies live once per crate in the tables module (`render_flat_dispatcher`): one
    // generic entry per form, keyed by this Node's `__RecursaFlatEntry` impl. A closure body
    // on every Node cost the type checker and borrow checker about four seconds of a
    // fifty-second compile for a grammar of 1,500 Nodes, with nothing but the type path and
    // two table indices varying between the copies.
    let spanned = quote!(#tables_module::__recursa_parse_flat::<Self, A>(input, true));
    let without_spans = quote!(#tables_module::__recursa_parse_flat::<Self, A>(input, false));
    let spanned_reusing =
        quote!(#tables_module::__recursa_parse_flat_reusing::<Self, A>(input, buffers, true));
    let without_spans_reusing =
        quote!(#tables_module::__recursa_parse_flat_reusing::<Self, A>(input, buffers, false));
    let entry = quote! {
        impl<#input_lifetime> #tables_module::__RecursaFlatEntry<#input_lifetime> for #nested_ty {
            type Flat = #flat_ty;
            const START_STATE: u32 = #start_state;
            const ACCEPT_RULE: u32 = #accept_rule;
        }
    };
    let result =
        quote!(Result<#module_anchor::FlatAst<#input_lifetime, #flat_ty>, #facade::ParseError>);
    quote! {
        #entry

        impl #impl_generics #nested_ty {
            /// Parses this Node into a Flat AST, with one span per Node and per text.
            ///
            /// The Flat AST owns every Node in a pool and every child relationship as a
            /// four-byte handle. It is built directly by the parse: no nested AST is
            /// constructed. Every typed pool comes out in post-order, so each Node's
            /// same-pool children have smaller handles than the Node itself.
            ///
            /// Diagnostics, prefix acceptance and consumed input are the ones `parse`
            /// produces, because both run the same automaton over the same tables. Spans are
            /// retained only when the runtime has the `spans` feature; occurrence provenance
            /// is never retained.
            pub fn parse_flat #method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> #result {
                #spanned
            }

            /// Parses this Node into a Flat AST without retaining any span.
            ///
            /// The result's `has_spans` is false and every `span` is `None`. Everything else
            /// matches `parse_flat`.
            pub fn parse_flat_without_spans #method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
            ) -> #result {
                #without_spans
            }

            /// Parses this Node into a Flat AST built on `buffers`, with one span per Node and
            /// per text.
            ///
            /// The result is the one `parse_flat` returns. The difference is where its stores
            /// come from: a slot holding the buffers of an earlier result starts this parse with
            /// warm capacity, so a batch consumer that recycles every result allocates nothing
            /// after its first parse. An empty slot pre-sizes fresh stores exactly as
            /// `parse_flat` does, so one call site serves the first parse and every later one.
            ///
            /// The slot is emptied on success, because the result owns the stores; call
            /// `FlatAst::recycle` to fill it again. On failure the buffers go back into the slot
            /// emptied, so a failed parse loses neither the capacity nor the caller's ability to
            /// use `?` on a `ParseError`.
            ///
            /// Recycling invalidates every handle of the recycled result, which
            /// `FlatAst::recycle` documents.
            pub fn parse_flat_reusing #method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
                buffers: &mut Option<#module_anchor::FlatBuffers>,
            ) -> #result {
                #spanned_reusing
            }

            /// Parses this Node into a Flat AST built on `buffers`, without retaining any span.
            ///
            /// This is `parse_flat_reusing` with the span policy of
            /// `parse_flat_without_spans`.
            pub fn parse_flat_reusing_without_spans #method_generics(
                input: &mut #module_anchor::Input<#input_lifetime, A>,
                buffers: &mut Option<#module_anchor::FlatBuffers>,
            ) -> #result {
                #without_spans_reusing
            }
        }
    }
}

/// Renders one content token's flat reduce action.
///
/// The text goes into the result's text pool, which owns it. The Node holds only the returned
/// four-byte handle. The pool keeps the record's lexical kind beside the text, so a token that
/// admits several kinds stays distinguishable. The record's own span is the text's span.
pub(super) fn render_token_flat_items(
    plan: &EmissionPlan,
    token: &super::lexing::TokenParsePlan,
) -> proc_macro2::TokenStream {
    if !plan.flat {
        return proc_macro2::TokenStream::new();
    }
    let facade = &plan.facade;
    let ident = &token.ident;
    let input_lifetime = input_lifetime();
    let builder_ty = builder_type(plan);
    let reduction = reduction_parameter(plan);
    quote! {
        impl #ident<'static> {
            #[doc = #SAFETY]
            #[inline(never)]
            pub(crate) fn __recursa_reduce_flat<#input_lifetime, A>(
                reduction: #reduction,
                builder: &mut #builder_ty,
            ) -> Result<(), #facade::ParseError> {
                let record = reduction.token();
                let text = reduction.matched_text(record)?;
                let handle = builder
                    .texts_mut()
                    .push::<Self>(text, record.kind, Some(record.span));
                reduction.push(handle);
                Ok(())
            }
        }
    }
}

/// Renders the grammar root's flat reduce dispatcher and the helper actions it runs itself.
///
/// The arms are the nested dispatcher's arms, so the flat dispatcher covers exactly the rules the
/// nested one covers: one arm per content token block, one per node application action, and one
/// per helper or envelope rule.
pub(super) fn render_flat_dispatcher(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
) -> Result<proc_macro2::TokenStream, String> {
    if !plan.flat {
        return Ok(proc_macro2::TokenStream::new());
    }
    let facade = &plan.facade;
    let input_lifetime = input_lifetime();
    let builder_ty = builder_type(plan);
    let reduction = reduction_parameter(plan);
    let dispatch = render_dispatch_arms(plan, lr, &ReduceDispatch::Flat)?;
    let (token_arms, application_arms) = (dispatch.token, dispatch.application);
    let root_actions = lr
        .root_rules()
        .iter()
        .map(|(index, rule)| {
            let body = render_flat_root_action(plan, lr, *index, rule)?;
            let ident = format_ident!("__recursa_reduce_flat_root_{index}");
            let arm = if body.is_empty() {
                quote!(#index => Ok(()),)
            } else {
                quote!(#index => #ident(reduction, builder),)
            };
            let function = (!body.is_empty()).then(|| {
                quote! {
                    #[doc = #SAFETY]
                    #[inline(never)]
                    fn #ident<#input_lifetime, A>(
                        reduction: #reduction,
                        builder: &mut #builder_ty,
                    ) -> Result<(), #facade::ParseError> {
                        #body
                        Ok(())
                    }
                }
            });
            Ok::<_, String>((arm, function))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let root_arms = root_actions.iter().map(|(arm, _)| arm);
    let root_functions = root_actions
        .iter()
        .filter_map(|(_, function)| function.as_ref());
    let module_anchor = &plan.module_anchor;
    let demand_extents = (!plan.rejections.is_empty() || !plan.validations.is_empty())
        .then(|| quote!(#facade::__private::ParseContext::demand_extents(context);));
    let entries = quote! {
        /// The flat parse entry of one root Node: its flat type and its start tables. Every
        /// root Node implements it, and the four `parse_flat*` methods of each Node call the
        /// two generic entries below with it, so the parse bodies exist once per crate.
        pub(crate) trait __RecursaFlatEntry<#input_lifetime> {
            type Flat: #facade::__private::FlatNode;
            const START_STATE: u32;
            const ACCEPT_RULE: u32;
        }

        #[doc(hidden)]
        pub(crate) fn __recursa_parse_flat<#input_lifetime, N, A>(
            input: &mut #module_anchor::Input<#input_lifetime, A>,
            spans: bool,
        ) -> Result<#module_anchor::FlatAst<#input_lifetime, N::Flat>, #facade::ParseError>
        where
            N: __RecursaFlatEntry<#input_lifetime>,
        {
            #facade::__private::parse_flat_with(
                &mut input.inner,
                &#module_anchor::__RECURSA_GRAMMAR,
                spans,
                |context| {
                    #demand_extents
                    let mut builder = #module_anchor::FlatAstBuilder::new(
                        context.tracks_extents(),
                        context.remaining_tokens(),
                    );
                    let root = #facade::__private::lr_run_flat::<
                        #facade::__private::Handle<N::Flat>,
                        A,
                        _,
                        _,
                    >(
                        context,
                        &TABLES,
                        N::START_STATE,
                        N::ACCEPT_RULE,
                        &mut builder,
                        __recursa_reduce_flat,
                    )?;
                    Ok(builder.finish(root))
                },
            )
        }

        /// The reusing form differs from `__recursa_parse_flat` in two places only: it takes
        /// the caller's buffers when it builds the stores, and it gives them back when the
        /// automaton fails.
        #[doc(hidden)]
        pub(crate) fn __recursa_parse_flat_reusing<#input_lifetime, N, A>(
            input: &mut #module_anchor::Input<#input_lifetime, A>,
            buffers: &mut Option<#module_anchor::FlatBuffers>,
            spans: bool,
        ) -> Result<#module_anchor::FlatAst<#input_lifetime, N::Flat>, #facade::ParseError>
        where
            N: __RecursaFlatEntry<#input_lifetime>,
        {
            #facade::__private::parse_flat_with(
                &mut input.inner,
                &#module_anchor::__RECURSA_GRAMMAR,
                spans,
                |context| {
                    #demand_extents
                    let capture_spans = context.tracks_extents();
                    let tokens = context.remaining_tokens();
                    let mut builder = match buffers.take() {
                        Some(recycled) => #module_anchor::FlatAstBuilder::from_buffers(
                            recycled,
                            capture_spans,
                            tokens,
                        ),
                        None => #module_anchor::FlatAstBuilder::new(capture_spans, tokens),
                    };
                    match #facade::__private::lr_run_flat::<
                        #facade::__private::Handle<N::Flat>,
                        A,
                        _,
                        _,
                    >(
                        context,
                        &TABLES,
                        N::START_STATE,
                        N::ACCEPT_RULE,
                        &mut builder,
                        __recursa_reduce_flat,
                    ) {
                        Ok(root) => Ok(builder.finish(root)),
                        Err(error) => {
                            *buffers = Some(builder.recycle());
                            Err(error)
                        }
                    }
                },
            )
        }
    };
    Ok(quote! {
        #entries

        #[doc = #SAFETY]
        #[allow(clippy::manual_range_patterns)]
        #[inline(always)]
        pub(crate) fn __recursa_reduce_flat<#input_lifetime, A>(
            rule: u32,
            reduction: #reduction,
            builder: &mut #builder_ty,
        ) -> Result<(), #facade::ParseError> {
            match rule {
                #(#token_arms)*
                #(#application_arms)*
                #(#root_arms)*
                _ => Ok(()),
            }
        }

        #(#root_functions)*
    })
}
