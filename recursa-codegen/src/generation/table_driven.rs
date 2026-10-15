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
pub(super) enum DispatchTarget {
    /// A node application's reduce function.
    Application(MaterializedApplicationId),
    /// A content token's reduce function.
    Token(ContentTokenId),
}

/// One arm of the root dispatcher: an inclusive rule range and its target.
///
/// [`super::table_driven_flat`] renders the flat dispatcher from the same arms, so both
/// dispatchers cover exactly the same rules.
#[derive(Clone, Copy, Debug)]
pub(super) struct DispatchArm {
    /// First rule of the block.
    pub(super) first: u32,
    /// Last rule of the block.
    pub(super) last: u32,
    /// Where the block's rules are reduced.
    pub(super) target: DispatchTarget,
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
    pub(super) fn symbol_carries_value(&self, symbol: Symbol) -> bool {
        match symbol {
            Symbol::Terminal(_) => false,
            Symbol::Nonterminal(id) => self.carries_value[id.index()],
        }
    }

    /// Reports whether one grammar type's own symbol pushes a value; a type inlined as a
    /// terminal does not, so its parent names or allocates the value itself.
    pub(super) fn type_carries_value(&self, type_id: TypeId) -> bool {
        self.type_carries_value
            .get(&type_id)
            .copied()
            .unwrap_or(false)
    }

    /// Returns the node application one normalized grammar type stands for.
    pub(super) fn application_of_type(&self, type_id: TypeId) -> Option<MaterializedApplicationId> {
        self.application_by_type.get(&type_id).copied()
    }

    /// Returns the content token one normalized grammar type stands for.
    pub(super) fn token_of_type(&self, type_id: TypeId) -> Option<ContentTokenId> {
        self.token_by_type.get(&type_id).copied()
    }

    /// Returns the helper and envelope rules the root dispatcher reduces itself.
    pub(super) fn root_rules(&self) -> &[(u32, Rule)] {
        &self.root_rules
    }

    /// Returns the dispatcher arms in ascending rule order.
    pub(super) fn dispatch_arms(&self) -> &[DispatchArm] {
        &self.arms
    }
}

/// One semantically distinct action of a node application and all grammar-wide rules using it.
pub(super) struct ApplicationReduceAction<'a> {
    /// Grammar-wide rule identities routed to this action.
    pub(super) rules: Vec<u32>,
    /// One representative rule from which the action body is rendered.
    pub(super) rule: &'a Rule,
}

/// Coalesces context-cloned rules that execute the same node construction.
///
/// The flat renderer coalesces the same way, so a flat action index always names the same rules
/// as the nested action index beside it.
pub(super) fn application_reduce_actions(
    tables: &ApplicationTables,
) -> Vec<ApplicationReduceAction<'_>> {
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
///
/// A merged kind of a `lookahead` declaration takes the atom of the token it merges. The kind
/// itself is never lexed and its own atom carries the generator's `merged:` label, while an
/// expected set that names it must read as the spelling the author wrote: a state that can
/// continue on `WITH_LA` expects the word `with`. Sharing the source atom also keeps the source
/// terminal and its merged kind one entry of the set rather than two.
fn expected_atoms(grammar: &crate::GrammarDescription) -> Result<EncodedTable, String> {
    let index = ExpectedAtomIndex::from_grammar(grammar)?;
    let mut atoms: Vec<u32> = (0..grammar.kind_count)
        .map(|kind| {
            let kind = u16::try_from(kind).expect("token kinds fit the runtime kind width");
            index.get(KindId::new(kind)).map_or(0, |atom| atom + 1)
        })
        .collect();
    for filter in &grammar.lookahead_filters {
        let Some(source) = atoms.get(filter.token.index()).copied() else {
            return Err("a lookahead filter names a token beyond the grammar width".to_owned());
        };
        let Some(merged) = atoms.get_mut(filter.merged.index()) else {
            return Err(
                "a lookahead filter names a merged kind beyond the grammar width".to_owned(),
            );
        };
        *merged = source;
    }
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
pub(super) fn input_lifetime() -> syn::Lifetime {
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
                // A text rule refuses one item on its own, so it runs on the first item too.
                let rejection = render_nested_list_check(plan, self, *element, rule, true);
                if plan.arena_ast {
                    quote! {
                        let element = #value;
                        #rejection
                        reduction.push(#facade::ArenaVec1::new_in(element, reduction.arena()));
                    }
                } else {
                    quote! {
                        let element = #value;
                        #rejection
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
                // The declared rejection rules of the element type run here, where the items the
                // list has passed and the extent of the item this step adds are both in hand.
                let rejection = render_nested_list_check(plan, self, *element, rule, false);
                quote! {
                    let element = #value;
                    let mut list = unsafe { reduction.pop::<#list_ty>() };
                    #rejection
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

/// Returns the Node plan and application one lowered list element names, when the element is a
/// Node whose declaration carries rejection rules.
///
/// The returned index is the Node's position in the plan, which names its generated rule table
/// and its key function. Both list backends resolve the element the same way, so both reach the
/// same table.
pub(super) fn rejection_element<'plan>(
    plan: &'plan EmissionPlan,
    lr: &TableDrivenRendering,
    element: ElementValue,
) -> Option<(usize, &'plan NodeParsePlan, &'plan NodeApplicationParsePlan)> {
    if plan.rejections.is_empty() {
        return None;
    }
    let application = lr.application_of_type(element.type_id)?;
    let (index, node) = plan.nodes.iter().enumerate().find(|(_, node)| {
        node.parse_applications
            .iter()
            .any(|candidate| candidate.application == application)
    })?;
    plan.rejections.rules(node.node)?;
    let application = node
        .parse_applications
        .iter()
        .find(|candidate| candidate.application == application)?;
    Some((index, node, application))
}

/// Names one Node's generated rejection rule table.
pub(super) fn rejection_table_ident(index: usize) -> Ident {
    format_ident!("__RECURSA_REJECTIONS_{index}")
}

/// Names one Node's generated nested key function.
fn rejection_key_ident(index: usize) -> Ident {
    format_ident!("__recursa_reject_key_{index}")
}

/// Returns a path that names one application type without its generic arguments.
///
/// A match arm names a variant through a path, and a path in a pattern carries no generic
/// arguments, so the pattern drops them and lets inference recover them from the matched value.
pub(super) fn bare_type_path(ty: &Type) -> Option<proc_macro2::TokenStream> {
    let Type::Path(path) = ty else {
        return None;
    };
    let mut path = path.path.clone();
    if let Some(last) = path.segments.last_mut() {
        last.arguments = syn::PathArguments::None;
    }
    Some(quote!(#path))
}

/// Replaces every lifetime of a type with the anonymous lifetime.
///
/// A generated tag function is a plain free function in the root's parser module. It borrows the
/// matched value for the call only, so every lifetime the application type carries is elided
/// rather than named in a generic list the module would have to thread.
fn elided_type(ty: &Type) -> Type {
    struct Elide;
    impl syn::visit_mut::VisitMut for Elide {
        fn visit_lifetime_mut(&mut self, lifetime: &mut syn::Lifetime) {
            *lifetime = syn::parse_quote!('_);
        }
    }
    let mut ty = ty.clone();
    syn::visit_mut::VisitMut::visit_type_mut(&mut Elide, &mut ty);
    ty
}

/// Renders the check one nested list step runs over the element type's rejection rules.
///
/// The first step of a list holds no passed item, so only a rule that refuses one item on its own
/// can answer there; an element type with no such rule renders nothing at that step.
fn render_nested_list_check(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
    element: ElementValue,
    rule: &Rule,
    first: bool,
) -> proc_macro2::TokenStream {
    let Some((index, node, _)) = rejection_element(plan, lr, element) else {
        return quote!();
    };
    if first
        && !plan
            .rejections
            .rules(node.node)
            .is_some_and(|rules| rules.refuses_single_items())
    {
        return quote!();
    }
    let Some(last) = rule.rhs.len().checked_sub(1) else {
        return quote!();
    };
    let table = rejection_table_ident(index);
    let key = rejection_key_ident(index);
    // A list item is held behind the authored `boxed!` layers, so the item reference needs one
    // more dereference than the owned element value does.
    let item_derefs = (0..=element.boxes).map(|_| quote!(*)).collect::<Vec<_>>();
    let element_derefs = (0..element.boxes).map(|_| quote!(*)).collect::<Vec<_>>();
    let passed = if first {
        quote!(::core::iter::empty())
    } else {
        quote! {
            list.as_slice().iter().map(|__recursa_item| (
                #key(&#(#item_derefs)*__recursa_item),
                ::core::option::Option::None,
            ))
        }
    };
    let added = quote! {
        (
            #key(&#(#element_derefs)*element),
            reduction.symbol_extent(#last),
        )
    };
    super::rejections::render_check(plan, &table, &passed, &added)
}

/// Returns the nested type path a key function names one Node by in a match pattern.
///
/// A path in a pattern carries no generic arguments, and the arena form names the semantic
/// lifetime, which the pattern elides.
pub(super) fn nested_node_path(
    plan: &EmissionPlan,
    node: NodeId,
) -> Result<proc_macro2::TokenStream, String> {
    let planned = plan
        .nodes
        .iter()
        .find(|candidate| candidate.node == node)
        .ok_or_else(|| "a Node a rejection key names has no frozen parse plan".to_owned())?;
    let [application] = planned.parse_applications.as_slice() else {
        return Err(format!(
            "`{}` is read by a rejection key but has more than one application",
            planned.ident
        ));
    };
    let mut ty = application.ty.clone();
    if plan.arena_ast {
        replace_lifetime(&mut ty, &input_lifetime(), &semantic_lifetime(plan));
    }
    bare_type_path(&elided_type(&ty)).ok_or_else(|| {
        format!(
            "`{}` is read by a rejection key but its application is not a path type",
            planned.ident
        )
    })
}

/// Renders the rule tables and key functions every nested list check of this grammar names.
///
/// Both backends call into the same table, so the table is rendered once in the parser module
/// that holds both dispatchers. Only an element type a list actually uses gets one. The flat
/// backend computes its keys inline, because a flat key reads pools the reduce holds.
fn render_root_rejections(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
) -> Result<proc_macro2::TokenStream, String> {
    if plan.rejections.is_empty() {
        return Ok(quote!());
    }
    let mut rendered = BTreeMap::new();
    for (_, rule) in lr.root_rules() {
        let (RuleAction::ListPush { element } | RuleAction::ListFirst { element }) = &rule.action
        else {
            continue;
        };
        let Some((index, node, application)) = rejection_element(plan, lr, *element) else {
            continue;
        };
        if rendered.contains_key(&index) {
            continue;
        }
        let table = rejection_table_ident(index);
        let mut nested_ty = application.ty.clone();
        if plan.arena_ast {
            replace_lifetime(&mut nested_ty, &input_lifetime(), &semantic_lifetime(plan));
        }
        let nested_ty = elided_type(&nested_ty);
        let nested_path = bare_type_path(&nested_ty).ok_or_else(|| {
            format!(
                "`{}` declares rejection rules but its application is not a path type",
                node.ident
            )
        })?;
        let mut items = super::rejections::render_rule_table(plan, node, &table);
        let name = |node: NodeId| nested_node_path(plan, node);
        items.extend(super::rejections::render_key_function(
            plan,
            node,
            &rejection_key_ident(index),
            &quote!(#nested_ty),
            &nested_path,
            &super::rejections::NestedNaming::Value(&name),
        )?);
        rendered.insert(index, items);
    }
    let items = rendered.into_values();
    Ok(quote!(#(#items)*))
}

/// Renders the sole parser's `__recursa_lr` module, tables, and reduce dispatcher at the
/// grammar module in `root.rs`.
pub(super) fn render_root_items(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
) -> Result<proc_macro2::TokenStream, String> {
    let facade = &plan.facade;
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
    let dispatch = render_dispatch_arms(
        plan,
        lr,
        &ReduceDispatch::Nested {
            semantic_lifetime: &semantic_lifetime,
        },
    )?;
    let (arms, application_arms) = (dispatch.token, dispatch.application);
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
    let flat_dispatcher = super::table_driven_flat::render_flat_dispatcher(plan, lr)?;
    let rejections = render_root_rejections(plan, lr)?;
    Ok(quote! {
        #[doc(hidden)]
        pub(crate) mod __recursa_lr {
            #rejections

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

            #flat_dispatcher
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

/// Which reduce backend one dispatcher's arms call.
///
/// The nested and the flat dispatcher cover exactly the same rules, because a flat action pops
/// and pushes the same counts as the nested action of the same rule. The two therefore build the
/// same arms. They differ only in what each arm calls.
pub(super) enum ReduceDispatch<'plan> {
    /// The nested actions. Their content-token type carries the grammar's semantic lifetime.
    Nested {
        /// The semantic lifetime of the grammar's generated types.
        semantic_lifetime: &'plan syn::Lifetime,
    },
    /// The flat actions. Each one takes the Flat AST builder beside the reduction.
    Flat,
}

impl ReduceDispatch<'_> {
    /// Returns the lifetime argument of one content token's generated type.
    ///
    /// A flat action pushes a text handle, which borrows nothing, so it names the token type at
    /// `'static`.
    fn token_lifetime(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Nested { semantic_lifetime } => quote!(#semantic_lifetime),
            Self::Flat => quote!('static),
        }
    }

    /// Returns the reduce method one content token's generated type supplies.
    fn token_method(&self) -> Ident {
        match self {
            Self::Nested { .. } => format_ident!("__recursa_reduce_lr"),
            Self::Flat => format_ident!("__recursa_reduce_flat"),
        }
    }

    /// Returns the arguments every arm of this dispatcher passes.
    fn arguments(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Nested { .. } => quote!(reduction),
            Self::Flat => quote!(reduction, builder),
        }
    }

    /// Returns the path of one node application's reduce action of this backend.
    ///
    /// Both backends emit the action as a free shim in the declaration's own module, because the
    /// grammar root cannot name a Node declared without `pub`.
    fn reducer_path(
        &self,
        application: &NodeApplicationParsePlan,
        action: usize,
    ) -> Result<syn::Path, String> {
        match self {
            Self::Nested { .. } => application_action_reducer_path(application, action),
            Self::Flat => super::table_driven_flat::flat_reducer_path(application, action),
        }
    }
}

/// The arms one reduce dispatcher matches, before its own root-rule arms.
pub(super) struct DispatchArms {
    /// One arm per content-token dispatch block, in block order.
    pub(super) token: Vec<proc_macro2::TokenStream>,
    /// One arm per node application reduce action, in canonical plan order.
    pub(super) application: Vec<proc_macro2::TokenStream>,
}

/// Renders the content-token arms and the node application arms of one reduce dispatcher.
///
/// Both dispatchers cover the same rules, so this function builds both sets of arms and
/// `dispatch` decides only what each arm calls. A root-rule arm is not built here: its body
/// differs between the backends, not only its callee.
pub(super) fn render_dispatch_arms(
    plan: &EmissionPlan,
    lr: &TableDrivenRendering,
    dispatch: &ReduceDispatch<'_>,
) -> Result<DispatchArms, String> {
    let module_anchor = &plan.module_anchor;
    let lifetime = dispatch.token_lifetime();
    let method = dispatch.token_method();
    let arguments = dispatch.arguments();
    let token_idents: BTreeMap<ContentTokenId, &Ident> = plan
        .tokens
        .iter()
        .map(|token| (token.semantic, &token.ident))
        .collect();
    let token = lr
        .dispatch_arms()
        .iter()
        .filter_map(|arm| {
            let (first, last) = (arm.first, arm.last);
            match arm.target {
                DispatchTarget::Application(_) => None,
                DispatchTarget::Token(token) => {
                    let ident = token_idents[&token];
                    let path = quote!(#module_anchor::__recursa_content_tokens::#ident<#lifetime>);
                    Some(quote!(#first..=#last => <#path>::#method(#arguments),))
                }
            }
        })
        .collect::<Vec<_>>();
    let mut application = Vec::new();
    for node_application in plan
        .nodes
        .iter()
        .flat_map(|node| node.parse_applications.iter())
    {
        for (index, action) in
            application_reduce_actions(lr.application(node_application.application))
                .iter()
                .enumerate()
        {
            let reducer = dispatch.reducer_path(node_application, index)?;
            let rules = &action.rules;
            application.push(quote!(#(#rules)|* => #reducer(#arguments),));
        }
    }
    Ok(DispatchArms { token, application })
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
    // A grammar that declares a rejection rule positions it at one list item, and only the LR
    // extent stack holds the extent of a reduced nonterminal.
    let demand_extents = (!plan.rejections.is_empty())
        .then(|| quote!(#facade::__private::ParseContext::demand_extents(context);));
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
                    #demand_extents
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
                #demand_extents
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
    let flat_items = super::table_driven_flat::render_node_application_flat(
        plan,
        node,
        application,
        lr,
        parser,
    )?;
    Ok(quote! {
        #parse_impl

        #(#shims)*

        impl #helper_impl_generics #ty #helper_where_clause {
            #(#methods)*
        }

        #flat_items
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
pub(super) fn plan_variant(
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
    let flat_items = super::table_driven_flat::render_token_flat_items(plan, token);
    quote! {
        #parse_impl
        #reducer
        #flat_items
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
        let root = root.split_whitespace().collect::<Vec<_>>().join(" ");

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
