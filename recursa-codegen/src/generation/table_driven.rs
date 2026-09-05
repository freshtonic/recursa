//! Rendering for the table-driven parser style: the root tables and reduce dispatcher, and the
//! per-node `TableDriven` parse impls with their reduce actions.
//!
//! Section 8 of `docs/table-driven-parsing.md` is the shape. The renderer runs the [`crate::lr`]
//! analysis over the lowered grammar at the renderer-selection point, refuses to generate on any
//! conflict, and then emits data plus reduce actions: the packed tables as byte-string literals,
//! one generic dispatcher with a range match over each contiguous block of a grammar type's
//! rules, and for every node application and content token an `impl GeneratedParse<'i, A,
//! TableDriven>` whose `parse` hands its start state and accept rule to the facade loop.
//!
//! A reduce action pops the values of its rule's value-carrying symbols in reverse right-hand
//! side order and pushes the constructed node. The pops are `unsafe` because the value stack is
//! type-erased; they are sound because the pop in a parent rule and the push in the child rule
//! are both derived from the same lowered symbol, and the popped type is inferred from the
//! constructed field, so a mismatch cannot be written by hand. Fixed tokens never reach the
//! value stack: a field whose type is a fixed token is constructed as a unit value.
//!
//! Containers (#114) follow the lowering's two forms. An inlined choice is visible to the node's
//! own reduce action through the rule's [`SiteForm`]s: an absent site is `None`, `false` or an
//! empty `Vec`, a present one wraps the inner value. A helper nonterminal (`opt(...)`,
//! `presence(...)`, `list(...)`, `list1(...)`, `list1t(...)`) and a presence envelope are reduced
//! in the root dispatcher, where the value they build is spelled through the element's node
//! type or content-token path, so a shared helper is typed once and every user pops the same
//! type by inference. Pratt (#115) and named parsers (#116) still fail generation with a message
//! naming the ticket.
//!
//! Deviations from the section 8 sketch, all deliberate: a type's start state and accept rule
//! are passed to `lr::run` as arguments instead of living as consts on `GeneratedParse`, so the
//! trait shared with recursive descent stays untouched; there is no `VALUE_SLOT`, because the
//! value stack is typed at each pop by the constructed field; the `start_states` table that
//! the analysis packs is not rendered, since every type carries its own start facts; the end
//! marker terminal travels with the tables; and the reduce dispatcher takes one `Reduction`
//! context (stack, popped frames, parse context) rather than separate stack, span and builder
//! parameters, which is where #118 adds the occurrence stack.

use std::collections::{BTreeMap, HashMap};

use super::parsing::{
    ElementParseKind, ElementParsePlan, FieldPlan, FieldWrapper, NodeApplicationParsePlan,
    NodeBodyPlan, NodeParsePlan, SemanticParseProjection, render_construction,
};
use super::*;
use crate::TypeId;
use crate::lr::{
    AutomatonOptions, ElementValue, EncodedLrTables, LrAnalysis, LrDiagnostic, NoPrecedence,
    NonterminalOrigin, OptionalContent, Rule, RuleAction, SiteForm, Symbol,
};
use crate::materialization::{LoweredGrammar, VariantAlternativeCorrelation};

/// What a block of contiguous rules reduces to.
#[derive(Clone, Copy, Debug)]
enum DispatchTarget {
    /// A node application's reduce function.
    Application(MaterializedApplicationId),
    /// A content token's reduce function.
    Token(ContentTokenId),
}

/// One arm of the root dispatcher: an inclusive rule range and its target.
#[derive(Clone, Copy, Debug)]
struct DispatchArm {
    /// First rule of the block.
    first: u32,
    /// Last rule of the block.
    last: u32,
    /// Where the block's rules are reduced.
    target: DispatchTarget,
    /// The target's local index of the block's first rule; a type's rules form several blocks
    /// when the lowering creates a helper in the middle of them.
    local_base: u32,
}

/// The table facts one grammar type's parse impl needs.
#[derive(Clone, Debug)]
pub(super) struct StartTables {
    /// The automaton state the type's parse starts in.
    pub(super) start_state: u32,
    /// The type's start rule, whose reduction accepts.
    pub(super) accept_rule: u32,
}

/// The rules of one node application, in rule order, with their grammar-wide identities.
#[derive(Clone, Debug)]
pub(super) struct ApplicationTables {
    /// The start facts, absent for a type without a nonterminal.
    pub(super) start: Option<StartTables>,
    /// The rules, local index order.
    pub(super) rules: Vec<Rule>,
}

/// Everything the renderers need from the table-driven analysis of one generation.
pub(super) struct TableDrivenRendering {
    /// The end-marker terminal.
    end_marker: u32,
    /// The encoded tables.
    tables: EncodedLrTables,
    /// The dispatcher arms in ascending rule order.
    arms: Vec<DispatchArm>,
    /// Per node application: its rules and start facts.
    applications: HashMap<MaterializedApplicationId, ApplicationTables>,
    /// Per content token: its start facts.
    tokens: HashMap<ContentTokenId, StartTables>,
    /// Whether each nonterminal carries a value, indexed by nonterminal.
    carries_value: Vec<bool>,
    /// Whether each grammar type's symbol carries a value; a type inlined as a terminal does not.
    type_carries_value: HashMap<TypeId, bool>,
    /// Node applications by normalized type, for spelling element types in helper actions.
    application_by_type: HashMap<TypeId, MaterializedApplicationId>,
    /// Content tokens by normalized type, for spelling element types in helper actions.
    token_by_type: HashMap<TypeId, ContentTokenId>,
    /// The helper and envelope rules the root dispatcher reduces itself, by rule index.
    root_rules: Vec<(u32, Rule)>,
    /// Authored variants correlated with lowered alternatives, for enum reduce actions.
    variants: Vec<VariantAlternativeCorrelation>,
    /// The bison-style automaton dump, for the debug bundle.
    pub(super) dump: String,
}

/// Joins conflict diagnostics into one generation failure message.
fn render_diagnostics(diagnostics: &[LrDiagnostic]) -> String {
    let mut text = format!(
        "the table-driven parser cannot be generated: {} conflict(s)\n",
        diagnostics.len()
    );
    for diagnostic in diagnostics {
        text.push_str(&diagnostic.render());
    }
    text
}

impl TableDrivenRendering {
    /// Runs the analysis and indexes its rules by node application and content token.
    pub(super) fn build(plan: &EmissionPlan, lowered: &LoweredGrammar) -> Result<Self, String> {
        if plan.explorer.is_some() || plan.framing.is_some() {
            return Err(
                "table-driven generation does not support the explorer or framing capability yet; \
                 those seams drive the recursive-descent impl until named parsers land (#116)"
                    .to_owned(),
            );
        }
        for description in &lowered.grammar.types {
            if matches!(description.shape, TypeShape::Pratt { .. }) {
                return Err(format!(
                    "table-driven generation does not render Pratt nodes yet (`{}` needs \
                     binding-power precedence, #115)",
                    description.name
                ));
            }
        }
        let analysis =
            LrAnalysis::from_lowered(lowered, &NoPrecedence, AutomatonOptions::default())
                .map_err(|diagnostics| render_diagnostics(&diagnostics))?;
        if !analysis.diagnostics.is_empty() {
            return Err(render_diagnostics(&analysis.diagnostics));
        }
        let map = &lowered.lowering_map;
        let application_by_type: HashMap<TypeId, MaterializedApplicationId> = map
            .applications
            .iter()
            .map(|correlation| (correlation.lowered, correlation.application))
            .collect();
        let token_by_type: HashMap<TypeId, ContentTokenId> = map
            .content_targets
            .iter()
            .map(|correlation| (correlation.lowered, correlation.semantic))
            .collect();

        let grammar = &analysis.grammar;
        let mut arms: Vec<DispatchArm> = Vec::new();
        let mut applications: HashMap<MaterializedApplicationId, ApplicationTables> =
            HashMap::new();
        let mut tokens: HashMap<ContentTokenId, StartTables> = HashMap::new();
        let start_of = |type_id: TypeId| -> Option<StartTables> {
            let start_state = analysis.automaton.start_state(type_id)?;
            let accept_rule = grammar
                .starts()
                .iter()
                .find(|start| start.type_id == type_id)?
                .rule;
            Some(StartTables {
                start_state: start_state.index() as u32,
                accept_rule: accept_rule.index() as u32,
            })
        };
        for correlation in &map.applications {
            applications.insert(
                correlation.application,
                ApplicationTables {
                    start: start_of(correlation.lowered),
                    rules: Vec::new(),
                },
            );
        }
        let mut root_rules = Vec::new();
        for (index, rule) in grammar.rules().iter().enumerate() {
            let index = index as u32;
            let target = match &rule.action {
                RuleAction::Accept { .. }
                | RuleAction::FixedSequence { .. }
                | RuleAction::Attachment { .. }
                | RuleAction::ListTrailing => None,
                RuleAction::Token { type_id } => {
                    let token = token_by_type[type_id];
                    tokens.entry(token).or_insert_with(|| {
                        start_of(*type_id).expect("content tokens have a start rule")
                    });
                    Some(DispatchTarget::Token(token))
                }
                RuleAction::Construct { type_id, .. } => match application_by_type.get(type_id) {
                    Some(application) => Some(DispatchTarget::Application(*application)),
                    None if rule.value_pops <= 1 => None,
                    None => {
                        return Err(format!(
                            "table-driven generation cannot reduce `{}`: a normalized type without \
                             a node application carries {} values",
                            grammar.render_rule(crate::lr::RuleId::new(index)),
                            rule.value_pops
                        ));
                    }
                },
                RuleAction::Envelope { .. }
                | RuleAction::OptionalAbsent { .. }
                | RuleAction::OptionalPresent { .. }
                | RuleAction::ListEmpty { .. }
                | RuleAction::ListFirst { .. }
                | RuleAction::ListPush { .. }
                | RuleAction::ListFromNonEmpty { .. } => {
                    root_rules.push((index, rule.clone()));
                    None
                }
            };
            let Some(target) = target else {
                continue;
            };
            let local_base = match target {
                DispatchTarget::Application(application) => {
                    let tables = applications
                        .get_mut(&application)
                        .expect("every application was indexed");
                    tables.rules.push(rule.clone());
                    tables.rules.len() as u32 - 1
                }
                DispatchTarget::Token(_) => 0,
            };
            match arms.last_mut() {
                Some(arm) if arm.last + 1 == index && same_target(arm.target, target) => {
                    arm.last = index;
                }
                _ => arms.push(DispatchArm {
                    first: index,
                    last: index,
                    target,
                    local_base,
                }),
            }
        }
        let mut type_carries_value = HashMap::new();
        for nonterminal in grammar.nonterminals() {
            if let NonterminalOrigin::Type(type_id) = nonterminal.origin {
                type_carries_value.insert(type_id, nonterminal.carries_value);
            }
        }
        Ok(Self {
            end_marker: grammar.end_marker().index() as u32,
            tables: analysis.tables.encode(),
            arms,
            applications,
            tokens,
            carries_value: grammar
                .nonterminals()
                .iter()
                .map(|nonterminal| nonterminal.carries_value)
                .collect(),
            type_carries_value,
            application_by_type,
            token_by_type,
            root_rules,
            variants: map.variants.clone(),
            dump: analysis.dump(),
        })
    }

    /// Returns a node application's rules and start facts.
    pub(super) fn application(&self, application: MaterializedApplicationId) -> &ApplicationTables {
        &self.applications[&application]
    }

    /// Returns a content token's start facts.
    pub(super) fn token(&self, token: ContentTokenId) -> &StartTables {
        &self.tokens[&token]
    }

    /// Reports whether a right-hand-side symbol pushes a value.
    fn symbol_carries_value(&self, symbol: Symbol) -> bool {
        match symbol {
            Symbol::Terminal(_) => false,
            Symbol::Nonterminal(id) => self.carries_value[id.index()],
        }
    }
}

/// Reports whether two dispatch targets are the same reduce function.
fn same_target(left: DispatchTarget, right: DispatchTarget) -> bool {
    match (left, right) {
        (DispatchTarget::Application(a), DispatchTarget::Application(b)) => a == b,
        (DispatchTarget::Token(a), DispatchTarget::Token(b)) => a == b,
        _ => false,
    }
}

/// The lifetime name every table-driven impl uses for the input, matching the recursive-descent
/// renderer so that `NodeApplicationParsePlan::ty` reads verbatim.
fn input_lifetime() -> syn::Lifetime {
    syn::parse_quote!('__recursa_input)
}

/// The Rust spelling of one element value: its type and the expression that takes it off the
/// value stack (or names it, for a fixed-token unit node), boxed as the container shape says.
struct ElementRendering {
    /// The element's type.
    ty: proc_macro2::TokenStream,
    /// The expression yielding one element.
    value: proc_macro2::TokenStream,
}

impl TableDrivenRendering {
    /// Spells one element value from the root: a node application through its plan type, a
    /// content token through its generated path.
    fn element(
        &self,
        plan: &EmissionPlan,
        element: ElementValue,
    ) -> Result<ElementRendering, String> {
        let module_anchor = &plan.module_anchor;
        let input_lifetime = input_lifetime();
        let (mut ty, mut value) = if let Some(application) =
            self.application_by_type.get(&element.type_id)
        {
            let ty = plan
                .nodes
                .iter()
                .flat_map(|node| node.parse_applications.iter())
                .find(|candidate| candidate.application == *application)
                .map(|candidate| &candidate.ty)
                .ok_or_else(|| "a lowered element has no planned node application".to_owned())?;
            let carries_value = self
                .type_carries_value
                .get(&element.type_id)
                .copied()
                .unwrap_or(false);
            let value = if carries_value {
                quote!(unsafe { reduction.pop::<#ty>() })
            } else {
                let Type::Path(path) = ty else {
                    return Err(
                        "a valueless element must name a unit fixed-token struct".to_owned()
                    );
                };
                let path = &path.path;
                quote!(#path)
            };
            (quote!(#ty), value)
        } else if let Some(token) = self.token_by_type.get(&element.type_id) {
            let ident = plan
                .tokens
                .iter()
                .find(|candidate| candidate.semantic == *token)
                .map(|candidate| &candidate.ident)
                .ok_or_else(|| "a lowered element has no planned content token".to_owned())?;
            let ty = quote!(#module_anchor::__recursa_content_tokens::#ident<#input_lifetime>);
            (ty.clone(), quote!(unsafe { reduction.pop::<#ty>() }))
        } else {
            return Err(
                "a lowered element is neither a node application nor a content token".to_owned(),
            );
        };
        for _ in 0..element.boxes {
            ty = quote!(Box<#ty>);
            value = quote!(Box::new(#value));
        }
        Ok(ElementRendering { ty, value })
    }

    /// Renders the reduce action of one helper or envelope rule in the root dispatcher.
    fn render_root_arm(
        &self,
        plan: &EmissionPlan,
        index: u32,
        rule: &Rule,
    ) -> Result<proc_macro2::TokenStream, String> {
        let facade = &plan.facade;
        let boxed =
            |mut ty: proc_macro2::TokenStream, mut value: proc_macro2::TokenStream, boxes: u32| {
                for _ in 0..boxes {
                    ty = quote!(Box<#ty>);
                    value = quote!(Box::new(#value));
                }
                (ty, value)
            };
        let body = match &rule.action {
            RuleAction::Envelope { present, .. } => quote!(reduction.push(#present);),
            RuleAction::OptionalAbsent { content } => match content {
                OptionalContent::Erased => quote!(),
                OptionalContent::Presence => quote!(reduction.push(false);),
                OptionalContent::Element(element) => {
                    let ty = self.element(plan, *element)?.ty;
                    quote!(reduction.push::<Option<#ty>>(None);)
                }
                OptionalContent::NonEmptyList { element, boxes } => {
                    let ty = self.element(plan, *element)?.ty;
                    let (ty, _) = boxed(quote!(#facade::Vec1<#ty>), quote!(), *boxes);
                    quote!(reduction.push::<Option<#ty>>(None);)
                }
            },
            RuleAction::OptionalPresent { content } => match content {
                OptionalContent::Erased => quote!(),
                OptionalContent::Presence => quote!(reduction.push(true);),
                OptionalContent::Element(element) => {
                    let value = self.element(plan, *element)?.value;
                    quote! {
                        let value = #value;
                        reduction.push(Some(value));
                    }
                }
                OptionalContent::NonEmptyList { element, boxes } => {
                    let ty = self.element(plan, *element)?.ty;
                    let (_, value) = boxed(
                        quote!(),
                        quote!(unsafe { reduction.pop::<#facade::Vec1<#ty>>() }),
                        *boxes,
                    );
                    quote! {
                        let list = #value;
                        reduction.push(Some(list));
                    }
                }
            },
            RuleAction::ListEmpty { element } => {
                let ty = self.element(plan, *element)?.ty;
                quote!(reduction.push(Vec::<#ty>::new());)
            }
            RuleAction::ListFirst { element } => {
                let value = self.element(plan, *element)?.value;
                quote! {
                    let element = #value;
                    reduction.push(#facade::Vec1::new(element));
                }
            }
            RuleAction::ListPush { element } => {
                let ElementRendering { ty, value } = self.element(plan, *element)?;
                quote! {
                    let element = #value;
                    let mut list = unsafe { reduction.pop::<#facade::Vec1<#ty>>() };
                    list.push(element);
                    reduction.push(list);
                }
            }
            RuleAction::ListFromNonEmpty { element } => {
                let ty = self.element(plan, *element)?.ty;
                quote! {
                    let list = unsafe { reduction.pop::<#facade::Vec1<#ty>>() };
                    reduction.push(list.into_vec());
                }
            }
            other => {
                return Err(format!(
                    "rule {index} with action {other:?} is not a root-reduced rule"
                ));
            }
        };
        Ok(quote!(#index => { #body Ok(()) }))
    }
}

/// Renders `root.rs`'s `__recursa_lr` module: the tables and the reduce dispatcher.
pub(super) fn render_root_items(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime = input_lifetime();
    let end_marker = lr.end_marker;
    let literal = |table: &crate::lr::tables::EncodedTable| {
        proc_macro2::Literal::byte_string(&table.literal())
    };
    let pact = literal(&lr.tables.pact);
    let table = literal(&lr.tables.table);
    let check = literal(&lr.tables.check);
    let defact = literal(&lr.tables.defact);
    let pgoto = literal(&lr.tables.pgoto);
    let defgoto = literal(&lr.tables.defgoto);
    let r1 = literal(&lr.tables.r1);
    let r2 = literal(&lr.tables.r2);
    let value_pops = literal(&lr.tables.value_pops);
    let token_idents: BTreeMap<ContentTokenId, &Ident> = plan
        .tokens
        .iter()
        .map(|token| (token.semantic, &token.ident))
        .collect();
    let application_types: HashMap<MaterializedApplicationId, &Type> = plan
        .nodes
        .iter()
        .flat_map(|node| node.parse_applications.iter())
        .map(|application| (application.application, &application.ty))
        .collect();
    let arms = lr.arms.iter().map(|arm| {
        let (first, last, base) = (arm.first, arm.last, arm.local_base);
        match arm.target {
            DispatchTarget::Application(application) => {
                let ty = application_types[&application];
                quote! {
                    #first..=#last => <#ty>::__recursa_reduce_lr(rule - #first + #base, reduction),
                }
            }
            DispatchTarget::Token(token) => {
                let ident = token_idents[&token];
                quote! {
                    #first..=#last => <#module_anchor::__recursa_content_tokens::#ident<#input_lifetime>>::__recursa_reduce_lr(reduction),
                }
            }
        }
    });
    let root_arms = lr
        .root_rules
        .iter()
        .map(|(index, rule)| lr.render_root_arm(plan, *index, rule))
        .collect::<Result<Vec<_>, _>>()?;
    let safety = "Reduce actions of the helper nonterminals and presence envelopes. SAFETY: each \
                  `pop` names the type the child rule's action pushed for the same grammar \
                  symbol, spelled through the element's node type or content-token path.";
    Ok(quote! {
        #[doc(hidden)]
        pub(crate) mod __recursa_lr {
            pub(crate) static TABLES: #facade::__private::LrTables = #facade::__private::LrTables::new(
                #end_marker, #pact, #table, #check, #defact, #pgoto, #defgoto, #r1, #r2, #value_pops,
            );

            #[doc = #safety]
            pub(crate) fn __recursa_reduce<#input_lifetime, A>(
                rule: u32,
                reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, A>,
            ) -> Result<(), #facade::ParseError> {
                match rule {
                    #(#arms)*
                    #(#root_arms)*
                    _ => Ok(()),
                }
            }
        }
    })
}

/// Renders the `TableDriven` parse impl of one type: `parse` runs the facade loop from the
/// type's start state.
fn render_parse_impl(
    plan: &EmissionPlan,
    ty: &Type,
    capture: proc_macro2::TokenStream,
    start: &StartTables,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime = input_lifetime();
    let start_state = start.start_state;
    let accept_rule = start.accept_rule;
    quote! {
        impl<#input_lifetime, A>
            #facade::__private::Parse<#input_lifetime, A, #facade::__private::TableDriven>
            for #ty
        {
            const CAPTURE_POLICY: #facade::__private::CapturePolicy = #capture;
            fn grammar() -> &'static #facade::__private::GrammarInfo {
                &#module_anchor::__RECURSA_GRAMMAR
            }
            fn root_follow() -> &'static #facade::__private::FollowSet {
                &#module_anchor::__RECURSA_EMPTY_FOLLOW
            }
            fn parse(
                context: &mut #facade::__private::ParseContext<'_, #input_lifetime, A>,
            ) -> Result<#facade::__private::ParsedNode<Self>, #facade::ParseError> {
                #facade::__private::lr_run::<Self, A>(
                    context,
                    &#module_anchor::__recursa_lr::TABLES,
                    #start_state,
                    #accept_rule,
                    #module_anchor::__recursa_lr::__recursa_reduce,
                )
            }
        }
    }
}

/// Renders one node application's `TableDriven` parse impl and its reduce actions.
pub(super) fn render_node_application(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    lr: &TableDrivenRendering,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let input_lifetime = input_lifetime();
    let ty = &application.ty;
    let tables = lr.application(application.application);
    let Some(start) = &tables.start else {
        return Ok(quote!());
    };
    let parse_impl = render_parse_impl(plan, ty, node.capture.render(facade), start);
    let mut helper_generics = Generics::default();
    if node.generics.lifetimes().next().is_some() {
        helper_generics
            .params
            .push(syn::parse_quote!(#input_lifetime));
    }
    let (helper_impl_generics, _, helper_where_clause) = helper_generics.split_for_impl();
    let helper_method_generics = if node.generics.lifetimes().next().is_some() {
        quote!(<A>)
    } else {
        quote!(<#input_lifetime, A>)
    };
    let arms = tables
        .rules
        .iter()
        .enumerate()
        .map(|(index, rule)| render_reduce_arm(plan, node, application, lr, index as u32, rule))
        .collect::<Result<Vec<_>, _>>()?;
    let safety = "Reduce actions of the table-driven parser. SAFETY: each `pop` names, through \
                  the constructed field's type, the type the child rule's action pushed for the \
                  same grammar symbol, and pops happen in reverse right-hand-side order.";
    Ok(quote! {
        #parse_impl

        impl #helper_impl_generics #ty #helper_where_clause {
            #[doc = #safety]
            pub(crate) fn __recursa_reduce_lr #helper_method_generics(
                rule: u32,
                reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, A>,
            ) -> Result<(), #facade::ParseError> {
                match rule {
                    #(#arms)*
                    _ => {}
                }
                Ok(())
            }
        }
    })
}

/// Renders one reduce arm: pops for the rule's value-carrying elements and the construction.
///
/// The rule's site forms and the node plan's elements are walked together, in reverse, so that
/// every element finds its symbols and every inlined choice its absent or present form.
fn render_reduce_arm(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    lr: &TableDrivenRendering,
    index: u32,
    rule: &Rule,
) -> Result<proc_macro2::TokenStream, String> {
    let RuleAction::Construct { variant, sites, .. } = &rule.action else {
        return Ok(quote!(#index => {},));
    };
    let (variant_ident, style, elements): (
        Option<&Ident>,
        ResolvedFieldStyle,
        &[ElementParsePlan],
    ) = match (&application.body, variant) {
        (NodeBodyPlan::Struct { style, elements }, None) => (None, *style, elements),
        (NodeBodyPlan::Enum { variants, .. }, Some(declaration)) => {
            let resolved = plan_variant(lr, application, *declaration)?;
            let variant = variants
                .iter()
                .find(|variant| variant.resolved_variant == resolved)
                .ok_or_else(|| {
                    format!("`{}` has no plan for one of its alternatives", node.ident)
                })?;
            (Some(&variant.ident), variant.style, &variant.elements)
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
    for (position, (element, form)) in elements.iter().zip(sites).enumerate().rev() {
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
                    quote!(unsafe { reduction.pop() })
                }
            }
            SemanticParseProjection::Direct {
                terminal,
                terminal_node,
                wrappers,
                ..
            } => render_field_value(
                plan,
                node,
                lr,
                terminal,
                *terminal_node,
                wrappers,
                *form,
                symbols,
            )?,
        };
        pops.push(quote!(let #value = #expression;));
        values.push((field.clone(), value));
    }
    values.reverse();
    let construction = render_construction(variant_ident, style, &values);
    Ok(quote! {
        #index => {
            #(#pops)*
            reduction.push(#construction);
        }
    })
}

/// Wraps a value expression in `boxes` layers of `Box::new`.
fn boxed(mut expression: proc_macro2::TokenStream, boxes: usize) -> proc_macro2::TokenStream {
    for _ in 0..boxes {
        expression = quote!(Box::new(#expression));
    }
    expression
}

/// Renders the value of one direct field from its site form: a required or helper site pops
/// the whole value, an inlined choice builds `None`, an empty `Vec`, or `Some` around the
/// inner value, with the authored `Box` layers applied where the wrappers place them.
#[allow(clippy::too_many_arguments)]
fn render_field_value(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    lr: &TableDrivenRendering,
    terminal: &Type,
    terminal_node: Option<NodeId>,
    wrappers: &[FieldWrapper],
    form: SiteForm,
    symbols: &[Symbol],
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let is_choice = |wrapper: &FieldWrapper| !matches!(wrapper, FieldWrapper::Boxed);
    // One element popped or named through the symbol at `symbols[0]`, boxed `boxes` times.
    let element = |boxes: usize| -> Result<proc_macro2::TokenStream, String> {
        let carries = symbols
            .first()
            .is_some_and(|symbol| lr.symbol_carries_value(*symbol));
        let value = if carries {
            quote!(unsafe { reduction.pop() })
        } else {
            unit_value(plan, node, terminal, terminal_node)?
        };
        Ok(boxed(value, boxes))
    };
    let Some(depth) = wrappers.iter().position(is_choice) else {
        return element(wrappers.len());
    };
    if !form.inlined {
        // A required list or a helper: the symbol's value is the field value from `depth` on.
        return Ok(boxed(quote!(unsafe { reduction.pop() }), depth));
    }
    let inner = match (&wrappers[depth], form.present) {
        (FieldWrapper::Optional, false) => quote!(None),
        (FieldWrapper::ZeroOrMore, false) => quote!(Vec::new()),
        (FieldWrapper::ZeroOrMore, true) => {
            quote!(unsafe { reduction.pop::<#facade::Vec1<_>>() }.into_vec())
        }
        (FieldWrapper::Optional, true) => {
            let rest = &wrappers[depth + 1..];
            let inner = match rest.iter().position(is_choice) {
                None => element(rest.len())?,
                Some(list_depth) => boxed(quote!(unsafe { reduction.pop() }), list_depth),
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
    Ok(boxed(inner, depth))
}

/// Finds the resolved variant a lowered alternative declaration stands for.
fn plan_variant(
    lr: &TableDrivenRendering,
    application: &NodeApplicationParsePlan,
    declaration: crate::DeclId,
) -> Result<ResolvedVariantId, String> {
    lr.variants
        .iter()
        .find(|correlation| {
            correlation.application == application.application && correlation.lowered == declaration
        })
        .map(|correlation| correlation.variant)
        .ok_or_else(|| "a lowered alternative has no authored variant".to_owned())
}

/// Renders the value of a field whose type carries nothing on the value stack: a fixed token
/// declared as a unit struct.
fn unit_value(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    terminal: &Type,
    terminal_node: Option<NodeId>,
) -> Result<proc_macro2::TokenStream, String> {
    let is_unit = terminal_node
        .and_then(|id| plan.nodes.iter().find(|candidate| candidate.node == id))
        .is_some_and(|target| {
            matches!(
                &target.body,
                NodeBodyPlan::Struct { style: ResolvedFieldStyle::Unit, elements }
                    if elements.iter().all(|element| matches!(element.kind, ElementParseKind::Fixed { .. }))
            )
        });
    let Type::Path(path) = terminal else {
        return Err(format!(
            "`{}`: a valueless field must name a unit fixed-token struct",
            node.ident
        ));
    };
    if !is_unit
        || path
            .path
            .segments
            .iter()
            .any(|segment| !segment.arguments.is_empty())
    {
        return Err(format!(
            "`{}`: a field of a fixed-token type must be a unit struct without generics for the \
             table-driven parser",
            node.ident
        ));
    }
    let expression = &path.path;
    Ok(quote!(#expression))
}

/// Renders a content token's `TableDriven` parse impl and its reduce action.
pub(super) fn render_token_items(
    plan: &EmissionPlan,
    token: &super::lexing::TokenParsePlan,
    lr: &TableDrivenRendering,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let ident = &token.ident;
    let input_lifetime = input_lifetime();
    let ty: Type = syn::parse_quote!(#ident<#input_lifetime>);
    let parse_impl = render_parse_impl(
        plan,
        &ty,
        token.capture.render(facade),
        lr.token(token.semantic),
    );
    let parsed_kind = (token.accepted_kind_variants.len() > 1).then(
        || quote!(kind: #module_anchor::__recursa_lexical::__recursa_typed_kind(record.kind),),
    );
    quote! {
        #parse_impl

        impl<#input_lifetime> #ident<#input_lifetime> {
            pub(crate) fn __recursa_reduce_lr<A>(
                reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, A>,
            ) -> Result<(), #facade::ParseError> {
                let record = reduction.token();
                let text = reduction.matched_text(record)?;
                reduction.push(Self { text, #parsed_kind });
                Ok(())
            }
        }
    }
}
