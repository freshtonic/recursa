//! Rendering for the table-driven parser style: the root tables and reduce dispatcher, and the
//! per-node `TableDriven` parse impls with their reduce actions.
//!
//! Section 8 of `docs/table-driven-parsing.md` is the shape. The renderer runs the [`crate::lr`]
//! analysis over the lowered grammar at the renderer-selection point, refuses to generate on any
//! conflict, and then emits data plus reduce actions: the packed tables as byte-string literals,
//! one generic dispatcher with a range match over the contiguous rule block of each grammar
//! type, and for every node application and content token an `impl GeneratedParse<'i, A,
//! TableDriven>` whose `parse` hands its start state and accept rule to the facade loop.
//!
//! A reduce action pops the values of its rule's value-carrying symbols in reverse right-hand
//! side order and pushes the constructed node. The pops are `unsafe` because the value stack is
//! type-erased; they are sound because the pop in a parent rule and the push in the child rule
//! are both derived from the same lowered symbol, and the popped type is inferred from the
//! constructed field, so a mismatch cannot be written by hand. Fixed tokens never reach the
//! value stack: a field whose type is a fixed token is constructed as a unit value.
//!
//! Only what the tracer ticket (#113) needs is rendered: sequences, enum alternatives, fixed
//! tokens and content tokens. Containers (#114), Pratt (#115) and named parsers (#116) fail
//! generation with a message naming the ticket.
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

use super::*;
use super::parsing::{
    ElementParseKind, ElementParsePlan, FieldPlan, FieldWrapper, NodeApplicationParsePlan,
    NodeBodyPlan, NodeParsePlan, PlannedContainer, SemanticParseProjection, render_construction,
};
use crate::TypeId;
use crate::lr::{
    AutomatonOptions, EncodedLrTables, LrAnalysis, LrDiagnostic, NoPrecedence, Rule, RuleAction,
    Symbol,
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
        let analysis = LrAnalysis::from_lowered(lowered, &NoPrecedence, AutomatonOptions::default())
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
        let mut applications: HashMap<MaterializedApplicationId, ApplicationTables> = HashMap::new();
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
        for (index, rule) in grammar.rules().iter().enumerate() {
            let index = index as u32;
            let target = match &rule.action {
                RuleAction::Accept { .. } | RuleAction::FixedSequence { .. } => None,
                RuleAction::Token { type_id } => {
                    let token = token_by_type[type_id];
                    tokens
                        .entry(token)
                        .or_insert_with(|| start_of(*type_id).expect("content tokens have a start rule"));
                    Some(DispatchTarget::Token(token))
                }
                RuleAction::Construct { type_id, .. } => match application_by_type.get(type_id) {
                    Some(application) => {
                        applications
                            .get_mut(application)
                            .expect("every application was indexed")
                            .rules
                            .push(rule.clone());
                        Some(DispatchTarget::Application(*application))
                    }
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
                RuleAction::OptionalAbsent
                | RuleAction::OptionalPresent
                | RuleAction::ListEmpty
                | RuleAction::ListFirst
                | RuleAction::ListPush
                | RuleAction::ListPass => {
                    return Err(format!(
                        "table-driven generation does not render containers yet (`{}`, #114)",
                        grammar.render_rule(crate::lr::RuleId::new(index))
                    ));
                }
            };
            let Some(target) = target else {
                continue;
            };
            match arms.last_mut() {
                Some(arm) if arm.last + 1 == index && same_target(arm.target, target) => {
                    arm.last = index;
                }
                _ => arms.push(DispatchArm {
                    first: index,
                    last: index,
                    target,
                }),
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

/// Renders `root.rs`'s `__recursa_lr` module: the tables and the reduce dispatcher.
pub(super) fn render_root_items(plan: &EmissionPlan, lr: &TableDrivenRendering) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime = input_lifetime();
    let end_marker = lr.end_marker;
    let literal = |table: &crate::lr::tables::EncodedTable| proc_macro2::Literal::byte_string(&table.literal());
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
        let (first, last) = (arm.first, arm.last);
        match arm.target {
            DispatchTarget::Application(application) => {
                let ty = application_types[&application];
                quote! {
                    #first..=#last => <#ty>::__recursa_reduce_lr(rule - #first, reduction),
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
    quote! {
        #[doc(hidden)]
        pub(crate) mod __recursa_lr {
            pub(crate) static TABLES: #facade::__private::LrTables = #facade::__private::LrTables::new(
                #end_marker, #pact, #table, #check, #defact, #pgoto, #defgoto, #r1, #r2, #value_pops,
            );

            pub(crate) fn __recursa_reduce<#input_lifetime, A>(
                rule: u32,
                reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, A>,
            ) -> Result<(), #facade::ParseError> {
                match rule {
                    #(#arms)*
                    _ => Ok(()),
                }
            }
        }
    }
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
        helper_generics.params.push(syn::parse_quote!(#input_lifetime));
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
fn render_reduce_arm(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    lr: &TableDrivenRendering,
    index: u32,
    rule: &Rule,
) -> Result<proc_macro2::TokenStream, String> {
    let RuleAction::Construct { variant, .. } = &rule.action else {
        return Ok(quote!(#index => {},));
    };
    let (variant_ident, style, elements): (Option<&Ident>, ResolvedFieldStyle, &[ElementParsePlan]) =
        match (&application.body, variant) {
            (NodeBodyPlan::Struct { style, elements }, None) => (None, *style, elements),
            (NodeBodyPlan::Enum { variants, .. }, Some(declaration)) => {
                let resolved = plan_variant(lr, application, *declaration)?;
                let variant = variants
                    .iter()
                    .find(|variant| variant.resolved_variant == resolved)
                    .ok_or_else(|| format!("`{}` has no plan for one of its alternatives", node.ident))?;
                (Some(&variant.ident), variant.style, &variant.elements)
            }
            _ => {
                return Err(format!(
                    "`{}`: the lowered rule and the node plan disagree on the node's shape",
                    node.ident
                ));
            }
        };
    if elements.len() != rule.rhs.len() {
        return Err(format!(
            "`{}`: rule has {} symbols but the node plan has {} elements",
            node.ident,
            rule.rhs.len(),
            elements.len()
        ));
    }
    let mut pops = Vec::new();
    let mut values: Vec<(FieldPlan, Ident)> = Vec::new();
    for (position, (element, symbol)) in elements.iter().zip(&rule.rhs).enumerate().rev() {
        let ElementParseKind::Semantic { field, projection } = &element.kind else {
            if let ElementParseKind::OptionalFixedTokenGroup { .. } = &element.kind {
                return Err(format!(
                    "`{}`: optional fixed-token groups are not rendered by the table-driven tracer (#114)",
                    node.ident
                ));
            }
            continue;
        };
        let SemanticParseProjection::Direct {
            terminal,
            terminal_node,
            wrappers,
            container,
            ..
        } = projection.as_ref()
        else {
            return Err(format!(
                "`{}`: boolean presence fields are not rendered by the table-driven tracer (#114)",
                node.ident
            ));
        };
        if !matches!(container.as_ref(), PlannedContainer::Direct { .. }) {
            return Err(format!(
                "`{}`: optional and repeated fields are not rendered by the table-driven tracer (#114)",
                node.ident
            ));
        }
        let boxes = wrappers
            .iter()
            .map(|wrapper| match wrapper {
                FieldWrapper::Boxed => Ok(()),
                _ => Err(format!(
                    "`{}`: optional and repeated fields are not rendered by the table-driven tracer (#114)",
                    node.ident
                )),
            })
            .collect::<Result<Vec<()>, String>>()?
            .len();
        let value = format_ident!("__recursa_value_{position}");
        let mut expression = if lr.symbol_carries_value(*symbol) {
            quote!(unsafe { reduction.pop() })
        } else {
            unit_value(plan, node, terminal, *terminal_node)?
        };
        for _ in 0..boxes {
            expression = quote!(Box::new(#expression));
        }
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
    if !is_unit || path.path.segments.iter().any(|segment| !segment.arguments.is_empty()) {
        return Err(format!(
            "`{}`: a field of a fixed-token type must be a unit struct without generics for the \
             table-driven tracer",
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
    let parse_impl = render_parse_impl(plan, &ty, token.capture.render(facade), lr.token(token.semantic));
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
