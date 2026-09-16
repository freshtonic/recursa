//! Rendering for the Recursa parser: root tables, the reduce dispatcher, and per-node parse impls.
//!
//! Section 8 of `docs/table-driven-parsing.md` is the shape. The renderer runs the [`crate::lr`]
//! analysis over the lowered grammar at the renderer-selection point, refuses to generate on any
//! conflict, and then emits data plus reduce actions: the packed tables as byte-string literals,
//! one generic dispatcher with a range match over each contiguous block of a grammar type's
//! rules, and for every node application and content token a `GeneratedParse` impl whose `parse`
//! hands its start state and accept rule to the facade loop. A node application's dispatcher arm
//! calls a declaration-local free shim instead of naming the node type at the grammar root: that
//! keeps private node declarations private while allowing their own module to run the typed
//! reduction.
//!
//! A reduce action pops the values of its rule's value-carrying symbols in reverse right-hand
//! side order and pushes the constructed node. The pops are `unsafe` because the value stack is
//! type-erased; they are sound because the pop in a parent rule and the push in the child rule
//! are both derived from the same lowered symbol, and the popped type is inferred from the
//! constructed field, so a mismatch cannot be written by hand. Fixed tokens never reach the
//! value stack: a field whose type is a fixed token is constructed as a unit value.
//!
//! Sequences, enum alternatives, fixed tokens and content tokens are rendered.
//! An alternative's reduce pops its children and boxes them at
//! construction; the implicit left operand of an infix or postfix variant is the first pop.
//! Textually identical node-local reductions share one match arm so rustc optimizes their
//! construction once while the LR table retains every distinct rule number.
//!
//! Containers (#114) follow the lowering's two forms. An inlined choice is visible to the node's
//! own reduce action through the rule's [`SiteForm`]s: an absent site is `None`, `false` or an
//! empty `Vec`, a present one wraps the inner value. A helper nonterminal (`opt(...)`,
//! `presence(...)`, `list(...)`, `list1(...)`, `list1t(...)`) and a presence envelope are reduced
//! in the root dispatcher, where the value they build is spelled through the element's node
//! type or content-token path, so a shared helper is typed once and every user pops the same
//! type by inference.
//!
//! Deviations from the section 8 sketch, all deliberate: a type's start state and accept rule
//! are passed to `lr::run` as arguments instead of living as consts on `GeneratedParse`; there is
//! no `VALUE_SLOT`, because the
//! value stack is typed at each pop by the constructed field; the `start_states` table that
//! the analysis packs is not rendered, since every type carries its own start facts; the end
//! marker terminal travels with the tables; and the reduce dispatcher takes one `Reduction`
//! context (stack, popped frames, parse context) rather than separate stack, span and builder
//! parameters, which is where #118 adds the occurrence stack.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use super::parsing::{
    ElementParseKind, ElementParsePlan, ExpectedAtomIndex, FieldPlan, FieldWrapper,
    NodeApplicationParsePlan, NodeBodyPlan, NodeParsePlan, SemanticParseProjection,
    render_construction,
};
use super::*;
use crate::lr::tables::EncodedTable;
use crate::lr::{
    AutomatonOptions, ElementValue, EncodedLrTables, LrAnalysis, LrDiagnostic, NonterminalOrigin,
    OptionalContent, Rule, RuleAction, SiteForm, Symbol,
};
use crate::materialization::{LoweredGrammar, VariantAlternativeCorrelation};
use crate::{KindId, TypeId};

/// Binary table files published beside generated Rust.
pub(super) const LR_TABLE_ASSET_NAMES: [&str; 14] = [
    "lr-pact.bin",
    "lr-table.bin",
    "lr-check.bin",
    "lr-defact.bin",
    "lr-pgoto.bin",
    "lr-defgoto.bin",
    "lr-r1.bin",
    "lr-r2.bin",
    "lr-value-pops.bin",
    "lr-reduce-actions.bin",
    "lr-expected-atoms.bin",
    "lr-lookahead.bin",
    "lr-folds.bin",
    "lr-tail.bin",
];

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
    /// The application's rules, paired with their grammar-wide identities.
    pub(super) rules: Vec<(u32, Rule)>,
}

/// Everything the renderers need from the LR analysis of one generation.
pub(super) struct TableDrivenRendering {
    /// The end-marker terminal.
    end_marker: u32,
    /// The encoded tables.
    tables: EncodedLrTables,
    /// Whether some rule folds an occurrence under a mandatory policy (an erased optional
    /// fixed-token group), which makes every parse fold provenance (#118).
    mandatory_provenance: bool,
    /// Per terminal, its canonical atom plus one or `0`: the vocabulary the error path uses to
    /// name expected terminals.
    expected_atoms: EncodedTable,
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
    /// Whether each grammar-wide rule must run generated semantic code.
    reduce_actions: EncodedTable,
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

/// Why the table-driven parser's LALR(1) construction failed to produce tables.
///
/// An unresolved conflict in an authored grammar (or a lowering-stage diagnostic to which no
/// rule or token binds) is an
/// ordinary authoring error, carried structured so the render-stage caller reports it as its
/// registered code (`RCA0400`, `RCA0401`, `RCA0402` or `RCA0403`) instead of the internal-
/// invariant `RCA9105`. Every other failure in this boundary is genuinely unexpected, so it
/// stays free text for that fallback.
pub(super) enum LrBuildError {
    /// Every unresolved conflict or lowering diagnostic the LR analysis reported, in the
    /// analysis's own deterministic `(state, lookahead)` order.
    Diagnostics(Vec<LrDiagnostic>),
    /// An unexpected internal invariant violation.
    Internal(String),
}

impl From<String> for LrBuildError {
    /// Classifies a plain-text failure from elsewhere in this boundary (table packing, plan
    /// rendering, ...) as an internal invariant violation, so the many helpers below that
    /// already return `Result<_, String>` keep working unchanged behind `?`.
    fn from(detail: String) -> Self {
        Self::Internal(detail)
    }
}

#[cfg(test)]
impl LrBuildError {
    /// Renders this failure as free text: joined conflict/lowering diagnostics, or the internal
    /// detail unchanged. Only test code, which cares solely about the message, collapses the
    /// structured `Diagnostics` case back to text this way; the render-stage boundary
    /// (`generation.rs`'s `render_lr_parser`) keeps it structured.
    pub(super) fn render(&self) -> String {
        match self {
            Self::Diagnostics(diagnostics) => {
                let mut text = format!(
                    "the Recursa parser cannot be generated: {} conflict(s)\n",
                    diagnostics.len()
                );
                for diagnostic in diagnostics {
                    text.push_str(&diagnostic.render());
                }
                text
            }
            Self::Internal(detail) => detail.clone(),
        }
    }
}

impl TableDrivenRendering {
    /// Runs the analysis and indexes its rules by node application and content token.
    pub(super) fn build(lowered: &LoweredGrammar) -> Result<Self, LrBuildError> {
        let analysis = LrAnalysis::from_lowered(lowered, AutomatonOptions::default())
            .map_err(LrBuildError::Diagnostics)?;
        if !analysis.diagnostics.is_empty() {
            return Err(LrBuildError::Diagnostics(analysis.diagnostics));
        }
        let expected_atoms = expected_atoms(&lowered.grammar)?;
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
        let mut reduce_actions = Vec::with_capacity(grammar.rules().len());
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
                        return Err(LrBuildError::Internal(format!(
                            "table-driven generation cannot reduce `{}`: a normalized type without \
                             a node application carries {} values",
                            grammar.render_rule(crate::lr::RuleId::new(index)),
                            rule.value_pops
                        )));
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
                reduce_actions.push(u32::from(rule_has_reduce_action(
                    rule,
                    &application_by_type,
                )));
                continue;
            };
            reduce_actions.push(1);
            if let DispatchTarget::Application(application) = target {
                let tables = applications
                    .get_mut(&application)
                    .expect("every application was indexed");
                tables.rules.push((index, rule.clone()));
            }
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
        let mut type_carries_value = HashMap::new();
        for nonterminal in grammar.nonterminals() {
            if let NonterminalOrigin::Type(type_id) = nonterminal.origin {
                type_carries_value.insert(type_id, nonterminal.carries_value);
            }
        }
        let mandatory_provenance = grammar.rules().iter().any(|rule| rule.fold.is_mandatory());
        Ok(Self {
            mandatory_provenance,
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
            reduce_actions: EncodedTable::unsigned(&reduce_actions),
            application_by_type,
            token_by_type,
            root_rules,
            variants: map.variants.clone(),
            dump: analysis.dump(),
            expected_atoms,
        })
    }

    /// Returns the exact encoded table bytes published outside generated Rust syntax.
    pub(super) fn assets(&self) -> Vec<(String, Vec<u8>)> {
        let tables = &self.tables;
        [
            &tables.pact,
            &tables.table,
            &tables.check,
            &tables.defact,
            &tables.pgoto,
            &tables.defgoto,
            &tables.r1,
            &tables.r2,
            &tables.value_pops,
            &self.reduce_actions,
            &self.expected_atoms,
            &tables.lookahead,
            &tables.folds,
            &tables.tail,
        ]
        .into_iter()
        .zip(LR_TABLE_ASSET_NAMES)
        .map(|(table, name)| (name.to_owned(), table.literal()))
        .collect()
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

/// One semantically distinct action of a node application and all grammar-wide rules using it.
struct ApplicationReduceAction<'a> {
    /// Grammar-wide rule identities routed to this action.
    rules: Vec<u32>,
    /// One representative rule from which the action body is rendered.
    rule: &'a Rule,
}

/// Coalesces context-cloned rules that execute the same node construction.
fn application_reduce_actions(tables: &ApplicationTables) -> Vec<ApplicationReduceAction<'_>> {
    let mut positions = HashMap::<&RuleAction, usize>::new();
    let mut actions = Vec::<ApplicationReduceAction<'_>>::new();
    for (global_rule, rule) in &tables.rules {
        if let Some(position) = positions.get(&rule.action).copied() {
            actions[position].rules.push(*global_rule);
        } else {
            positions.insert(&rule.action, actions.len());
            actions.push(ApplicationReduceAction {
                rules: vec![*global_rule],
                rule,
            });
        }
    }
    actions
}

/// Encodes the canonical atom of every token kind, in terminal order, as the atom index plus
/// one or `0` for a kind without a direct atom.
fn expected_atoms(grammar: &crate::GrammarDescription) -> Result<EncodedTable, String> {
    let index = ExpectedAtomIndex::from_grammar(grammar)?;
    let atoms: Vec<u32> = (0..grammar.kind_count)
        .map(|kind| {
            let kind = u16::try_from(kind).expect("token kinds fit the runtime kind width");
            index.get(KindId::new(kind)).map_or(0, |atom| atom + 1)
        })
        .collect();
    Ok(EncodedTable::unsigned(&atoms))
}

/// Reports whether two dispatch targets are the same reduce function.
fn same_target(left: DispatchTarget, right: DispatchTarget) -> bool {
    match (left, right) {
        (DispatchTarget::Application(a), DispatchTarget::Application(b)) => a == b,
        (DispatchTarget::Token(a), DispatchTarget::Token(b)) => a == b,
        _ => false,
    }
}

/// Reports whether reducing `rule` must execute generated semantic code.
///
/// State, occurrence and goto bookkeeping remains in the LR loop for every rule. This flag only
/// skips the generated callback for rules whose semantic value is passed through unchanged or
/// which exist solely to recognize syntax.
fn rule_has_reduce_action(
    rule: &Rule,
    application_by_type: &HashMap<TypeId, MaterializedApplicationId>,
) -> bool {
    match &rule.action {
        RuleAction::Accept { .. }
        | RuleAction::FixedSequence { .. }
        | RuleAction::Attachment { .. }
        | RuleAction::ListTrailing => false,
        RuleAction::Token { .. }
        | RuleAction::Envelope { .. }
        | RuleAction::ListEmpty { .. }
        | RuleAction::ListFirst { .. }
        | RuleAction::ListPush { .. }
        | RuleAction::ListFromNonEmpty { .. } => true,
        RuleAction::Construct { type_id, .. } => application_by_type.contains_key(type_id),
        RuleAction::OptionalAbsent { content } | RuleAction::OptionalPresent { content } => {
            !matches!(content, OptionalContent::Erased)
        }
    }
}

/// The lifetime name every generated parse impl uses for the input.
fn input_lifetime() -> syn::Lifetime {
    syn::parse_quote!('__recursa_input)
}

/// The lifetime semantic AST containers borrow in generated reduce actions.
fn semantic_lifetime(plan: &EmissionPlan) -> syn::Lifetime {
    if plan.arena_ast {
        syn::parse_quote!('__recursa_arena)
    } else {
        input_lifetime()
    }
}

/// Wraps one generated semantic value in the grammar-selected box.
fn box_value(
    plan: &EmissionPlan,
    ty: proc_macro2::TokenStream,
    value: proc_macro2::TokenStream,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    if plan.arena_ast {
        let facade = &plan.facade;
        (
            quote!(#facade::ArenaBox<'__recursa_arena, #ty>),
            quote!(#facade::ArenaBox::new_in(#value, reduction.arena())),
        )
    } else {
        (quote!(Box<#ty>), quote!(Box::new(#value)))
    }
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
        let input_lifetime = semantic_lifetime(plan);
        let (mut ty, mut value) = if let Some(application) =
            self.application_by_type.get(&element.type_id)
        {
            let mut ty = plan
                .nodes
                .iter()
                .flat_map(|node| node.parse_applications.iter())
                .find(|candidate| candidate.application == *application)
                .map(|candidate| candidate.ty.clone())
                .ok_or_else(|| "a lowered element has no planned node application".to_owned())?;
            if plan.arena_ast {
                let authored_lifetime: syn::Lifetime = syn::parse_quote!('__recursa_input);
                replace_lifetime(&mut ty, &authored_lifetime, &semantic_lifetime(plan));
            }
            let carries_value = self
                .type_carries_value
                .get(&element.type_id)
                .copied()
                .unwrap_or(false);
            let value = if carries_value {
                quote!(unsafe { reduction.pop::<#ty>() })
            } else {
                let Type::Path(path) = &ty else {
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
            (ty, value) = box_value(plan, ty, value);
        }
        Ok(ElementRendering { ty, value })
    }

    /// Renders the reduce action of one helper or envelope rule.
    fn render_root_action(
        &self,
        plan: &EmissionPlan,
        index: u32,
        rule: &Rule,
    ) -> Result<proc_macro2::TokenStream, String> {
        let facade = &plan.facade;
        let boxed =
            |mut ty: proc_macro2::TokenStream, mut value: proc_macro2::TokenStream, boxes: u32| {
                for _ in 0..boxes {
                    (ty, value) = box_value(plan, ty, value);
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
                    let list_ty = if plan.arena_ast {
                        quote!(#facade::ArenaVec1<'__recursa_arena, #ty>)
                    } else {
                        quote!(#facade::Vec1<#ty>)
                    };
                    let (ty, _) = boxed(list_ty, quote!(), *boxes);
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
                    let list_ty = if plan.arena_ast {
                        quote!(#facade::ArenaVec1<'__recursa_arena, #ty>)
                    } else {
                        quote!(#facade::Vec1<#ty>)
                    };
                    let (_, value) = boxed(
                        quote!(),
                        quote!(unsafe { reduction.pop::<#list_ty>() }),
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
                if plan.arena_ast {
                    quote!(reduction.push(#facade::ArenaVec::<#ty>::new_in(reduction.arena()));)
                } else {
                    quote!(reduction.push(Vec::<#ty>::new());)
                }
            }
            RuleAction::ListFirst { element } => {
                let value = self.element(plan, *element)?.value;
                if plan.arena_ast {
                    quote! {
                        let element = #value;
                        reduction.push(#facade::ArenaVec1::new_in(element, reduction.arena()));
                    }
                } else {
                    quote! {
                        let element = #value;
                        reduction.push(#facade::Vec1::new(element));
                    }
                }
            }
            RuleAction::ListPush { element } => {
                let ElementRendering { ty, value } = self.element(plan, *element)?;
                let list_ty = if plan.arena_ast {
                    quote!(#facade::ArenaVec1<'__recursa_arena, #ty>)
                } else {
                    quote!(#facade::Vec1<#ty>)
                };
                quote! {
                    let element = #value;
                    let mut list = unsafe { reduction.pop::<#list_ty>() };
                    list.push(element);
                    reduction.push(list);
                }
            }
            RuleAction::ListFromNonEmpty { element } => {
                let ty = self.element(plan, *element)?.ty;
                let list_ty = if plan.arena_ast {
                    quote!(#facade::ArenaVec1<'__recursa_arena, #ty>)
                } else {
                    quote!(#facade::Vec1<#ty>)
                };
                quote! {
                    let list = unsafe { reduction.pop::<#list_ty>() };
                    reduction.push(list.into_vec());
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
}

/// Renders the sole parser's `__recursa_lr` module, tables, and reduce dispatcher at the
/// grammar module in `root.rs`.
pub(super) fn render_root_items(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime = input_lifetime();
    let semantic_lifetime = semantic_lifetime(plan);
    let reducer_generics = if plan.arena_ast {
        quote!(<#input_lifetime: #semantic_lifetime, #semantic_lifetime, A>)
    } else {
        quote!(<#input_lifetime, A>)
    };
    let reduction_arena = if plan.arena_ast {
        quote!(#semantic_lifetime)
    } else {
        quote!('_)
    };
    let end_marker = lr.end_marker;
    let asset = |name: &str| {
        let path = format!("/recursa-generations/{}/{name}", plan.identity);
        quote!(include_bytes!(concat!(env!("OUT_DIR"), #path)))
    };
    let [
        pact,
        table,
        check,
        defact,
        pgoto,
        defgoto,
        r1,
        r2,
        value_pops,
        reduce_actions,
        expected_atoms,
        lookahead,
        folds,
        tail,
    ] = LR_TABLE_ASSET_NAMES.map(asset);
    let mandatory = lr.mandatory_provenance;
    let token_idents: BTreeMap<ContentTokenId, &Ident> = plan
        .tokens
        .iter()
        .map(|token| (token.semantic, &token.ident))
        .collect();
    let arms = lr.arms.iter().filter_map(|arm| {
        let (first, last) = (arm.first, arm.last);
        match arm.target {
            DispatchTarget::Application(_) => None,
            DispatchTarget::Token(token) => {
                let ident = token_idents[&token];
                Some(quote! {
                    #first..=#last => <#module_anchor::__recursa_content_tokens::#ident<#semantic_lifetime>>::__recursa_reduce_lr(reduction),
                })
            }
        }
    });
    let mut application_arms = Vec::new();
    for application in plan
        .nodes
        .iter()
        .flat_map(|node| node.parse_applications.iter())
    {
        for (action_index, action) in
            application_reduce_actions(lr.application(application.application))
                .iter()
                .enumerate()
        {
            let reducer = application_action_reducer_path(application, action_index)?;
            let rules = &action.rules;
            application_arms.push(quote! {
                #(#rules)|* => #reducer(reduction),
            });
        }
    }
    let root_actions = lr
        .root_rules
        .iter()
        .map(|(index, rule)| {
            let body = lr.render_root_action(plan, *index, rule)?;
            let ident = format_ident!("__recursa_reduce_root_{index}");
            let arm = if body.is_empty() {
                quote!(#index => Ok(()),)
            } else {
                quote!(#index => #ident(reduction),)
            };
            let function = (!body.is_empty()).then(|| {
                quote! {
                    #[inline(never)]
                    fn #ident #reducer_generics(
                        reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, #reduction_arena, A>,
                    ) -> Result<(), #facade::ParseError> {
                        #body
                        Ok(())
                    }
                }
            });
            Ok::<_, String>((arm, function))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root_arms = root_actions.iter().map(|(arm, _)| arm);
    let root_functions = root_actions
        .iter()
        .filter_map(|(_, function)| function.as_ref());
    let safety = "Reduce actions of the helper nonterminals and presence envelopes. SAFETY: each \
                  `pop` names the type the child rule's action pushed for the same grammar \
                  symbol, spelled through the element's node type or content-token path.";
    Ok(quote! {
        #[doc(hidden)]
        pub(crate) mod __recursa_lr {
            pub(crate) static TABLES: #facade::__private::LrTables = #facade::__private::LrTables::new(
                #end_marker, #pact, #table, #check, #defact, #pgoto, #defgoto, #r1, #r2, #value_pops,
                #reduce_actions, #expected_atoms, #lookahead, #folds, #tail, #mandatory,
            );

            #[doc = #safety]
            #[allow(clippy::manual_range_patterns)]
            #[inline(always)]
            pub(crate) fn __recursa_reduce #reducer_generics(
                rule: u32,
                reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, #reduction_arena, A>,
            ) -> Result<(), #facade::ParseError> {
                match rule {
                    #(#arms)*
                    #(#application_arms)*
                    #(#root_arms)*
                    _ => Ok(()),
                }
            }

            #(#root_functions)*
        }
    })
}

/// Names the declaration-local reducer shim for one materialized node application.
fn application_action_reducer_ident(
    application: &NodeApplicationParsePlan,
    action: usize,
) -> Ident {
    format_ident!("__recursa_reduce_lr_{}_{}", application.parse_index, action)
}

/// Resolves the declaration-local reducer shim without exposing the node type to its ancestors.
///
/// Materialized application types are emitted as an absolute `crate::module::Node<...>` path.
/// Replacing the final type segment with the application-unique shim keeps the call at the same
/// module while removing both the private declaration and its generic arguments from the root
/// dispatcher's name resolution.
fn application_action_reducer_path(
    application: &NodeApplicationParsePlan,
    action: usize,
) -> Result<syn::Path, String> {
    let Type::Path(type_path) = &application.ty else {
        return Err("a materialized node application must have a path type".to_owned());
    };
    if type_path.qself.is_some() {
        return Err("a materialized node application cannot have a qualified self type".to_owned());
    }
    let mut path = type_path.path.clone();
    path.segments
        .pop()
        .ok_or_else(|| "a materialized node application path is empty".to_owned())?;
    path.segments
        .push(syn::PathSegment::from(application_action_reducer_ident(
            application,
            action,
        )));
    Ok(path)
}

/// Renders the `TableDriven` parse impl of one type: `parse` runs the facade loop from the
/// type's start state over the parser's tables.
fn render_parse_impl(
    plan: &EmissionPlan,
    ty: &Type,
    family_ident: &Ident,
    publicly_materialized: bool,
    capture: proc_macro2::TokenStream,
    start: &StartTables,
    parser: &RenderedParser,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let tables_module = parser.tables_module(plan);
    let input_lifetime = input_lifetime();
    let start_state = start.start_state;
    let accept_rule = start.accept_rule;
    if plan.arena_ast {
        let family_visibility = publicly_materialized.then(|| quote!(pub));
        let arena_lifetime = semantic_lifetime(plan);
        let mut marker_ty = ty.clone();
        replace_lifetime(&mut marker_ty, &arena_lifetime, &input_lifetime);
        return quote! {
            #[doc(hidden)]
            #family_visibility struct #family_ident;

            unsafe impl #facade::ArenaAstFamily for #family_ident {
                type Ast<#arena_lifetime> = #ty;
            }

            impl<#input_lifetime, A> #facade::__private::ArenaGeneratedParse<#input_lifetime, A>
                for #marker_ty
            {
                type Family = #family_ident;
                const CAPTURE_POLICY: #facade::__private::CapturePolicy = #capture;
                fn grammar() -> &'static #facade::__private::GrammarInfo {
                    &#module_anchor::__RECURSA_GRAMMAR
                }
                fn root_follow() -> &'static #facade::__private::FollowSet {
                    #facade::__private::empty_follow()
                }
                fn parse_arena<#arena_lifetime>(
                    context: &mut #facade::__private::ParseContext<'_, #input_lifetime, #arena_lifetime, A>,
                ) -> Result<
                    #facade::__private::ParsedNode<#arena_lifetime, #ty>,
                    #facade::ParseError,
                >
                where
                    #input_lifetime: #arena_lifetime,
                {
                    #facade::__private::lr_run::<#ty, A, _>(
                        context,
                        &#tables_module::TABLES,
                        #start_state,
                        #accept_rule,
                        #tables_module::__recursa_reduce,
                    )
                }
            }
        };
    }
    quote! {
        impl<#input_lifetime, A> #facade::__private::Parse<#input_lifetime, A> for #ty
        {
            const CAPTURE_POLICY: #facade::__private::CapturePolicy = #capture;
            fn grammar() -> &'static #facade::__private::GrammarInfo {
                &#module_anchor::__RECURSA_GRAMMAR
            }
            fn root_follow() -> &'static #facade::__private::FollowSet {
                #facade::__private::empty_follow()
            }
            fn parse<'arena>(
                context: &mut #facade::__private::ParseContext<'_, #input_lifetime, 'arena, A>,
            ) -> Result<#facade::__private::ParsedNode<'arena, Self>, #facade::ParseError> {
                #facade::__private::lr_run::<Self, A, _>(
                    context,
                    &#tables_module::TABLES,
                    #start_state,
                    #accept_rule,
                    #tables_module::__recursa_reduce,
                )
            }
        }
    }
}

/// Rebinds one generated lifetime throughout a materialized application type.
fn replace_lifetime(ty: &mut Type, from: &syn::Lifetime, to: &syn::Lifetime) {
    struct Replace<'a> {
        from: &'a syn::Lifetime,
        to: &'a syn::Lifetime,
    }
    impl syn::visit_mut::VisitMut for Replace<'_> {
        fn visit_lifetime_mut(&mut self, lifetime: &mut syn::Lifetime) {
            if lifetime.ident == self.from.ident {
                *lifetime = self.to.clone();
            }
        }
    }
    syn::visit_mut::VisitMut::visit_type_mut(&mut Replace { from, to }, ty);
}

/// Renders one node application's `TableDriven` parse impl and its reduce actions.
pub(super) fn render_node_application(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    lr: &TableDrivenRendering,
    parser: &RenderedParser,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let input_lifetime = input_lifetime();
    let semantic_lifetime = semantic_lifetime(plan);
    let mut ty = application.ty.clone();
    if plan.arena_ast {
        replace_lifetime(&mut ty, &input_lifetime, &semantic_lifetime);
    }
    let ty = &ty;
    let tables = lr.application(application.application);
    let Some(start) = &tables.start else {
        return Ok(quote!());
    };
    let family_ident = format_ident!("__RecursaArenaFamilyNode{}", application.parse_index);
    let parse_impl = render_parse_impl(
        plan,
        ty,
        &family_ident,
        node.publicly_materialized,
        node.capture.render(facade),
        start,
        parser,
    );
    let mut helper_generics = Generics::default();
    if node.generics.lifetimes().next().is_some() || plan.arena_ast {
        helper_generics
            .params
            .push(syn::parse_quote!(#semantic_lifetime));
    }
    let (helper_impl_generics, _, helper_where_clause) = helper_generics.split_for_impl();
    let helper_method_generics = if plan.arena_ast {
        quote!(<#input_lifetime: #semantic_lifetime, A>)
    } else if node.generics.lifetimes().next().is_some() {
        quote!(<A>)
    } else {
        quote!(<#input_lifetime, A>)
    };
    let safety = "Reduce actions of the table-driven parser. SAFETY: each `pop` names, through \
                  the constructed field's type, the type the child rule's action pushed for the \
                  same grammar symbol, and pops happen in reverse right-hand-side order.";
    let action_groups = application_reduce_actions(tables);
    let actions = action_groups
        .iter()
        .enumerate()
        .map(|(action_index, action)| {
            let reducer_ident = application_action_reducer_ident(application, action_index);
            let method_ident = format_ident!(
                "__recursa_reduce_lr_action_{}_{}",
                application.parse_index,
                action_index
            );
            let body = render_reduce_body(plan, node, application, lr, action.rule)?;
            let shim = if plan.arena_ast {
                quote! {
                    #[doc = #safety]
                    #[inline(always)]
                    pub(in #module_anchor) fn #reducer_ident<#input_lifetime: #semantic_lifetime, #semantic_lifetime, A>(
                        reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, #semantic_lifetime, A>,
                    ) -> Result<(), #facade::ParseError> {
                        <#ty>::#method_ident(reduction)
                    }
                }
            } else {
                quote! {
                    #[doc = #safety]
                    #[inline(always)]
                    pub(in #module_anchor) fn #reducer_ident<#input_lifetime, A>(
                        reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, '_, A>,
                    ) -> Result<(), #facade::ParseError> {
                        <#ty>::#method_ident(reduction)
                    }
                }
            };
            let reduction_arena = if plan.arena_ast {
                quote!(#semantic_lifetime)
            } else {
                quote!('_)
            };
            let method = quote! {
                #[doc = #safety]
                #[inline(never)]
                fn #method_ident #helper_method_generics(
                    reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, #reduction_arena, A>,
                ) -> Result<(), #facade::ParseError> {
                    #body
                    Ok(())
                }
            };
            Ok::<_, String>((shim, method))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let shims = actions.iter().map(|(shim, _)| shim);
    let methods = actions.iter().map(|(_, method)| method);
    Ok(quote! {
        #parse_impl

        #(#shims)*

        impl #helper_impl_generics #ty #helper_where_clause {
            #(#methods)*
        }
    })
}

/// Renders one reduce body: pops for the rule's value-carrying elements and the construction.
///
/// The rule's site forms and the node plan's elements are walked together, in reverse, so that
/// every element finds its symbols and every inlined choice its absent or present form.
fn render_reduce_body(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    application: &NodeApplicationParsePlan,
    lr: &TableDrivenRendering,
    rule: &Rule,
) -> Result<proc_macro2::TokenStream, String> {
    let RuleAction::Construct { variant, sites, .. } = &rule.action else {
        return Ok(quote!({}));
    };
    let (variant_ident, style, elements): (
        Option<&Ident>,
        ResolvedFieldStyle,
        Cow<'_, [ElementParsePlan]>,
    ) = match (&application.body, variant) {
        (NodeBodyPlan::Struct { style, elements }, None) => {
            (None, *style, Cow::Borrowed(elements.as_slice()))
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
    let forms: Cow<'_, [SiteForm]> = Cow::Borrowed(sites.as_slice());
    if elements.len() != forms.len() {
        return Err(format!(
            "`{}`: rule has {} sites but the node plan has {} elements",
            node.ident,
            forms.len(),
            elements.len()
        ));
    }
    let mut pops = Vec::new();
    let mut values: Vec<(FieldPlan, Ident)> = Vec::new();
    for (position, (element, form)) in elements.iter().zip(forms.iter()).enumerate().rev() {
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
                FieldSite {
                    terminal,
                    terminal_node: *terminal_node,
                    wrappers,
                    form: *form,
                    symbols,
                },
            )?,
        };
        pops.push(quote!(let #value = #expression;));
        values.push((field.clone(), value));
    }
    values.reverse();
    let construction = render_construction(variant_ident, style, &values);
    Ok(quote! {
        {
            #(#pops)*
            reduction.push(#construction);
        }
    })
}

/// Wraps a value expression in `boxes` layers of `Box::new`.
fn boxed(
    plan: &EmissionPlan,
    mut expression: proc_macro2::TokenStream,
    boxes: usize,
) -> proc_macro2::TokenStream {
    for _ in 0..boxes {
        expression = if plan.arena_ast {
            let facade = &plan.facade;
            quote!(#facade::ArenaBox::new_in(#expression, reduction.arena()))
        } else {
            quote!(Box::new(#expression))
        };
    }
    expression
}

/// Pops one stack value and applies `boxes` ownership layers, moving an arena AST's innermost
/// boxed value directly into its final allocation.
fn boxed_stack_value(plan: &EmissionPlan, boxes: usize) -> proc_macro2::TokenStream {
    if plan.arena_ast && boxes > 0 {
        boxed(plan, quote!(unsafe { reduction.pop_boxed() }), boxes - 1)
    } else {
        boxed(plan, quote!(unsafe { reduction.pop() }), boxes)
    }
}

/// One direct field as a reduce action sees it: the authored terminal type and wrappers from the
/// plan, and the site form and symbols from the lowered rule.
struct FieldSite<'a> {
    /// Authored terminal type before materialization.
    terminal: &'a Type,
    /// Resolved node family, when the terminal is generated.
    terminal_node: Option<NodeId>,
    /// Authored type wrappers, outermost first.
    wrappers: &'a [FieldWrapper],
    /// How the site appears on the rule's right-hand side.
    form: SiteForm,
    /// The right-hand-side symbols the site occupies.
    symbols: &'a [Symbol],
}

/// Renders the value of one direct field from its site form: a required or helper site pops
/// the whole value, an inlined choice builds `None`, an empty `Vec`, or `Some` around the
/// inner value, with the authored `Box` layers applied where the wrappers place them.
fn render_field_value(
    plan: &EmissionPlan,
    node: &NodeParsePlan,
    lr: &TableDrivenRendering,
    site: FieldSite<'_>,
) -> Result<proc_macro2::TokenStream, String> {
    let FieldSite {
        terminal,
        terminal_node,
        wrappers,
        form,
        symbols,
    } = site;
    let facade = &plan.facade;
    let is_choice = |wrapper: &FieldWrapper| !matches!(wrapper, FieldWrapper::Boxed);
    // One element popped or named through the field's value-carrying symbol, boxed `boxes`
    // times. Usually `symbols` is that one symbol, but an inlined cardinality-one attachment
    // unit (`docs/declared-precedence.md`, "Attachment units inline") widens the form to the
    // attachment's own fixed-token and operand symbols, with the fixed tokens never carrying a
    // value; the value stack holds only value-carrying entries regardless of their
    // right-hand-side position, so `pop` finds the operand's value correctly once any symbol in
    // the widened form is known to carry one.
    let element = |boxes: usize| -> Result<proc_macro2::TokenStream, String> {
        let carries = symbols
            .iter()
            .any(|symbol| lr.symbol_carries_value(*symbol));
        let value = if carries {
            return Ok(boxed_stack_value(plan, boxes));
        } else {
            unit_value(plan, node, terminal, terminal_node)?
        };
        Ok(boxed(plan, value, boxes))
    };
    let Some(depth) = wrappers.iter().position(is_choice) else {
        return element(wrappers.len());
    };
    if !form.inlined {
        // A required list or a helper: the symbol's value is the field value from `depth` on.
        return Ok(boxed_stack_value(plan, depth));
    }
    let inner = match (&wrappers[depth], form.present) {
        (FieldWrapper::Optional, false) => quote!(None),
        (FieldWrapper::ZeroOrMore, false) => {
            if plan.arena_ast {
                quote!(#facade::ArenaVec::new_in(reduction.arena()))
            } else {
                quote!(Vec::new())
            }
        }
        (FieldWrapper::ZeroOrMore, true) => {
            if plan.arena_ast {
                quote!(unsafe { reduction.pop::<#facade::ArenaVec1<'__recursa_arena, _>>() }.into_vec())
            } else {
                quote!(unsafe { reduction.pop::<#facade::Vec1<_>>() }.into_vec())
            }
        }
        (FieldWrapper::Optional, true) => {
            let rest = &wrappers[depth + 1..];
            let inner = match rest.iter().position(is_choice) {
                None => element(rest.len())?,
                Some(list_depth) => boxed_stack_value(plan, list_depth),
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
    Ok(boxed(plan, inner, depth))
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
    parser: &RenderedParser,
) -> proc_macro2::TokenStream {
    let facade = &plan.facade;
    let module_anchor = &plan.module_anchor;
    let ident = &token.ident;
    let input_lifetime = semantic_lifetime(plan);
    let ty: Type = syn::parse_quote!(#ident<#input_lifetime>);
    let family_ident = format_ident!("__RecursaArenaFamilyToken{}", token.semantic.index());
    let parse_impl = render_parse_impl(
        plan,
        &ty,
        &family_ident,
        true,
        token.capture.render(facade),
        lr.token(token.semantic),
        parser,
    );
    let parsed_kind = (token.accepted_kind_variants.len() > 1).then(
        || quote!(kind: #module_anchor::__recursa_lexical::__recursa_typed_kind(record.kind),),
    );
    let reducer = if plan.arena_ast {
        quote! {
            impl<'__recursa_arena> #ident<'__recursa_arena> {
                pub(crate) fn __recursa_reduce_lr<'__recursa_input: '__recursa_arena, A>(
                    reduction: &mut #facade::__private::LrReduction<'_, '_, '__recursa_input, '__recursa_arena, A>,
                ) -> Result<(), #facade::ParseError> {
                    let record = reduction.token();
                    let text = reduction.matched_text_in_arena(record)?;
                    reduction.push(Self { text, #parsed_kind });
                    Ok(())
                }
            }
        }
    } else {
        quote! {
            impl<#input_lifetime> #ident<#input_lifetime> {
                pub(crate) fn __recursa_reduce_lr<A>(
                    reduction: &mut #facade::__private::LrReduction<'_, '_, #input_lifetime, '_, A>,
                ) -> Result<(), #facade::ParseError> {
                    let record = reduction.token();
                    let text = reduction.matched_text(record)?;
                    reduction.push(Self { text, #parsed_kind });
                    Ok(())
                }
            }
        }
    };
    quote! {
        #parse_impl
        #reducer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_reduce_dispatch_does_not_name_private_nested_node_types() {
        let package = tempfile::tempdir().expect("temporary private Node fixture");
        fs::write(
            package.path().join("grammar.rs"),
            r#"recursa::grammar! { module = crate }

mod nested {
    recursa::ast_node! {

        #[tok("root")]
        struct Private;
    }
}
"#,
        )
        .expect("write private Node fixture");
        let success = analyze_source_tree(
            SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect("analyze private Node fixture");
        let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
            .expect("join private Node emission plan");
        let rendered = render(&plan).expect("render private Node generation");

        assert!(
            !rendered
                .root
                .contains("< crate :: nested :: Private > :: __recursa_reduce_lr"),
            "the grammar-root dispatcher must not name a child module's private type: {}",
            rendered.root,
        );
        assert!(
            rendered
                .root
                .contains("crate :: nested :: __recursa_reduce_lr_0_0"),
            "the grammar-root dispatcher must call the declaration-local action shim: {}",
            rendered.root,
        );
        assert!(
            rendered.nodes[0]
                .1
                .contains("pub (in crate) fn __recursa_reduce_lr_0_0"),
            "the Node output must expose its action shim to the grammar root: {}",
            rendered.nodes[0].1,
        );
    }

    #[test]
    fn arena_boxed_fields_move_stack_values_directly_into_the_arena() {
        let package = tempfile::tempdir().expect("temporary arena AST fixture");
        fs::write(
            package.path().join("grammar.rs"),
            r#"recursa::grammar! {
    module = crate,
    arena_ast,
}

recursa::ast_node! {

    #[tok("leaf")]
    struct Leaf;
}

recursa::ast_node! {

    struct Root(boxed!(Leaf));
}
"#,
        )
        .expect("write arena AST fixture");
        let success = analyze_source_tree(
            SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect("analyze arena AST fixture");
        let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
            .expect("join arena AST emission plan");
        let rendered = render(&plan).expect("render arena AST generation");
        let root = rendered
            .nodes
            .iter()
            .find_map(|(_, output)| {
                output
                    .contains("reduction . push (Self (")
                    .then_some(output)
            })
            .expect("rendered Root node");

        assert!(
            root.contains("unsafe { reduction . pop_boxed () }"),
            "an arena-boxed stack value must move directly into its allocation: {root}",
        );
        assert!(
            !root.contains(
                "ArenaBox :: new_in (unsafe { reduction . pop () } , reduction . arena ())"
            ),
            "the generated action must not materialize the value between pop and allocation: \
             {root}",
        );
    }
}
