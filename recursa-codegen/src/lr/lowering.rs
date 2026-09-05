#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Lowering from the normalized grammar to LR rules.
//!
//! Terminals are the grammar's token kinds. Every grammar type becomes one nonterminal, except a
//! fixed token of one kind, which is inlined as its terminal so that keywords cost no reduce.
//! Containers become shared helper nonterminals in the simplest correct form: nullable
//! optionals and left-recursive lists (ticket #114 refines them). Pratt enums become plain
//! alternatives with the implicit left operand prepended to infix and postfix rules, and their
//! binding powers are recorded on the rules for ticket #115. Every parse entry type gets a
//! synthetic start rule. A text predicate other than `Any` is `RCA0403`.

use std::collections::{BTreeSet, HashMap};

use super::grammar::{
    LrGrammar, NonterminalId, NonterminalOrigin, Rule, RuleAction, RulePrecedence, Symbol,
    TerminalId, UnrepresentableBindingPowers,
};
use super::report::{LrDiagnostic, NO_PRECEDENCE_EQUIVALENT, TEXT_PREDICATE};
use crate::{
    ContainerShape, GrammarDescription, KindSet, PrattPosition, PrattVariant,
    RepetitionCardinality, SiteId, TextPredicate, TypeId, TypeShape,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// The facts about normalized types that the normalized grammar alone does not state: which
/// token types carry their token as a value, which sequences are fixed-token-only, which sites
/// select a boolean presence, and which types are parse entry points.
pub struct TypeRoles {
    /// Token types whose value is the matched token; every other token type is fixed.
    pub content_tokens: BTreeSet<TypeId>,
    /// Sequence types made only of fixed tokens; they carry no value.
    pub fixed_sequences: BTreeSet<TypeId>,
    /// Optional sites whose presence is the value of an authored `bool` field.
    pub presence_sites: BTreeSet<SiteId>,
    /// Types that get a start rule: node applications and content tokens.
    pub start_types: Vec<TypeId>,
}

impl TypeRoles {
    /// Reads the roles off a lowered grammar's correlations.
    pub(crate) fn from_lowered(lowered: &crate::materialization::LoweredGrammar) -> Self {
        let map = &lowered.lowering_map;
        let content_tokens: BTreeSet<TypeId> = map
            .content_targets
            .iter()
            .map(|correlation| correlation.lowered)
            .collect();
        let mut fixed_sequences = BTreeSet::new();
        let mut presence_sites = BTreeSet::new();
        for presence in &map.presences {
            fixed_sequences.insert(presence.sequence);
            presence_sites.insert(if presence.outer_is_optional {
                presence.outer
            } else {
                presence.envelope_sites[presence.presence_index]
            });
        }
        for group in &map.optional_fixed_groups {
            fixed_sequences.insert(group.sequence);
        }
        let mut start_types: Vec<TypeId> = map
            .applications
            .iter()
            .map(|correlation| correlation.lowered)
            .chain(content_tokens.iter().copied())
            .collect();
        start_types.sort_unstable();
        start_types.dedup();
        Self {
            content_tokens,
            fixed_sequences,
            presence_sites,
            start_types,
        }
    }
}

/// Lowers a normalized grammar to an LR grammar, or reports every text predicate (`RCA0403`)
/// and every Pratt binding power with no precedence equivalent (`RCA0402`) it carries.
pub fn lower(grammar: &GrammarDescription, roles: &TypeRoles) -> Result<LrGrammar, Vec<LrDiagnostic>> {
    let diagnostics = text_predicate_diagnostics(grammar);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut lowering = Lowering::new(grammar, roles);
    lowering.lower_types();
    if !lowering.diagnostics.is_empty() {
        return Err(lowering.diagnostics);
    }
    lowering.lower_starts();
    Ok(lowering.lr)
}

/// Reports `RCA0403` for every token type refined by a text predicate other than `Any`.
fn text_predicate_diagnostics(grammar: &GrammarDescription) -> Vec<LrDiagnostic> {
    let mut diagnostics = Vec::new();
    for description in &grammar.types {
        let TypeShape::Token {
            text_predicate: Some(predicate),
            ..
        } = &description.shape
        else {
            continue;
        };
        let predicate = &grammar.text_predicates[predicate.index()];
        let rendered = match predicate {
            TextPredicate::Any => continue,
            TextPredicate::Exact(text) => format!("exact text `{text}`"),
            TextPredicate::OneOf(texts) => format!("one of {texts:?}"),
            TextPredicate::NotIn(texts) => format!("none of {texts:?}"),
        };
        diagnostics.push(LrDiagnostic {
            code: TEXT_PREDICATE,
            message: format!(
                "token type `{}` matches {rendered}; text predicates are not supported by the \
                 table-driven parser style",
                description.name
            ),
            items: Vec::new(),
            actions: Vec::new(),
            notes: Vec::new(),
        });
    }
    diagnostics
}

/// Names every token kind for reports: its authored attribute spelling when it has one, else
/// the name of the one token type that matches exactly that kind, else its normalized name.
fn terminal_names(grammar: &GrammarDescription, atom_kinds: &[KindSet]) -> Vec<String> {
    let mut names: Vec<Option<String>> = grammar
        .kinds
        .iter()
        .map(|kind| kind.authored.clone())
        .collect();
    let mut claimed = vec![false; grammar.kinds.len()];
    for description in &grammar.types {
        let TypeShape::Token { atom, .. } = &description.shape else {
            continue;
        };
        let kinds: Vec<_> = atom_kinds[atom.index()].iter().collect();
        let [kind] = kinds.as_slice() else {
            continue;
        };
        let slot = &mut names[kind.index()];
        if slot.is_none() && !claimed[kind.index()] {
            *slot = Some(description.name.clone());
        } else if slot.as_deref() != Some(description.name.as_str()) && claimed[kind.index()] {
            *slot = None;
        }
        claimed[kind.index()] = true;
    }
    let mut seen = BTreeSet::new();
    names
        .into_iter()
        .zip(&grammar.kinds)
        .map(|(name, kind)| {
            let name = name.unwrap_or_else(|| kind.name.clone());
            if seen.insert(name.clone()) {
                name
            } else {
                kind.name.clone()
            }
        })
        .collect()
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Identity of a shared container helper nonterminal.
enum HelperKey {
    /// `opt(symbol)`, valued when the symbol is or when it is a presence selector.
    Optional {
        /// The wrapped symbol.
        inner: Symbol,
        /// Whether the helper carries a value.
        valued: bool,
    },
    /// A repetition of a symbol with an optional separator symbol.
    List {
        /// The repeated symbol.
        element: Symbol,
        /// The separator symbol, if any.
        separator: Option<Symbol>,
        /// Whether the list needs at least one element.
        one_or_more: bool,
        /// Whether one trailing separator is accepted.
        optional_trailing: bool,
    },
}

/// The lowering state: the grammar being built and the symbol chosen for every type.
struct Lowering<'a> {
    /// The normalized grammar.
    grammar: &'a GrammarDescription,
    /// The type roles.
    roles: &'a TypeRoles,
    /// The kind set of every atom.
    atom_kinds: Vec<KindSet>,
    /// The LR grammar under construction.
    lr: LrGrammar,
    /// The symbol each type lowers to: an inlined terminal or its nonterminal.
    type_symbols: Vec<Symbol>,
    /// Shared helper nonterminals.
    helpers: HashMap<HelperKey, NonterminalId>,
    /// Names already taken, to keep nonterminal names unique.
    names: BTreeSet<String>,
    /// `RCA0402` diagnostics collected while lowering Pratt variants.
    diagnostics: Vec<LrDiagnostic>,
}

impl<'a> Lowering<'a> {
    /// Creates the terminals and one nonterminal per type that needs one.
    fn new(grammar: &'a GrammarDescription, roles: &'a TypeRoles) -> Self {
        let atom_kinds = crate::analysis::expand_atoms(grammar);
        let mut lr = LrGrammar::new(terminal_names(grammar, &atom_kinds));
        let mut names = BTreeSet::new();
        let mut type_symbols = Vec::with_capacity(grammar.types.len());
        for (index, description) in grammar.types.iter().enumerate() {
            let type_id = TypeId::new(index as u32);
            let symbol = match &description.shape {
                TypeShape::Token { atom, .. } if !roles.content_tokens.contains(&type_id) => {
                    let kinds: Vec<_> = atom_kinds[atom.index()].iter().collect();
                    match kinds.as_slice() {
                        [kind] => Some(Symbol::Terminal(TerminalId::new(kind.get() as u32))),
                        _ => None,
                    }
                }
                _ => None,
            };
            let symbol = symbol.unwrap_or_else(|| {
                let carries_value = match &description.shape {
                    TypeShape::Token { .. } => roles.content_tokens.contains(&type_id),
                    TypeShape::Sequence { .. } => !roles.fixed_sequences.contains(&type_id),
                    _ => true,
                };
                let mut name = description.name.clone();
                if !names.insert(name.clone()) {
                    name = format!("{name}#{index}");
                    names.insert(name.clone());
                }
                Symbol::Nonterminal(lr.add_nonterminal(
                    name,
                    NonterminalOrigin::Type(type_id),
                    carries_value,
                ))
            });
            type_symbols.push(symbol);
        }
        Self {
            grammar,
            roles,
            atom_kinds,
            lr,
            type_symbols,
            helpers: HashMap::new(),
            names,
            diagnostics: Vec::new(),
        }
    }

    /// Emits the rules of every type that has a nonterminal.
    fn lower_types(&mut self) {
        for index in 0..self.grammar.types.len() {
            let type_id = TypeId::new(index as u32);
            let Symbol::Nonterminal(lhs) = self.type_symbols[index] else {
                continue;
            };
            let shape = self.grammar.types[index].shape.clone();
            match shape {
                TypeShape::Token { atom, .. } => {
                    let kinds: Vec<_> = self.atom_kinds[atom.index()].iter().collect();
                    let content = self.roles.content_tokens.contains(&type_id);
                    for kind in kinds {
                        let terminal = Symbol::Terminal(TerminalId::new(kind.get() as u32));
                        self.lr.add_rule(Rule {
                            lhs,
                            rhs: vec![terminal],
                            value_pops: 0,
                            precedence: None,
                            action: if content {
                                RuleAction::Token { type_id }
                            } else {
                                RuleAction::FixedSequence { type_id }
                            },
                        });
                    }
                }
                TypeShape::Alias { site } => {
                    let rhs = self.sites_symbols(&[site]);
                    self.add_construct(lhs, rhs, None, type_id, None);
                }
                TypeShape::Sequence { sites } => {
                    let rhs = self.sites_symbols(&sites);
                    if self.roles.fixed_sequences.contains(&type_id) {
                        self.lr.add_rule(Rule {
                            lhs,
                            rhs,
                            value_pops: 0,
                            precedence: None,
                            action: RuleAction::FixedSequence { type_id },
                        });
                    } else {
                        self.add_construct(lhs, rhs, None, type_id, None);
                    }
                }
                TypeShape::Enum { alternatives } => {
                    for alternative in alternatives {
                        let rhs = self.sites_symbols(&alternative.sites);
                        self.add_construct(lhs, rhs, Some(alternative.declaration), type_id, None);
                    }
                }
                TypeShape::Pratt { variants } => {
                    for variant in variants {
                        let mut rhs = Vec::new();
                        if matches!(
                            variant.position,
                            PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
                        ) {
                            rhs.push(Symbol::Nonterminal(lhs));
                        }
                        rhs.extend(self.sites_symbols(&variant.sites));
                        let precedence = self.variant_precedence(&variant);
                        self.add_construct(lhs, rhs, Some(variant.declaration), type_id, precedence);
                    }
                }
            }
        }
    }

    /// Derives a Pratt variant's rule precedence, reporting `RCA0402` naming the variant when
    /// its binding powers have no precedence equivalent; the rule then carries none.
    fn variant_precedence(&mut self, variant: &PrattVariant) -> Option<RulePrecedence> {
        match RulePrecedence::from_position(variant.position) {
            Ok(precedence) => precedence,
            Err(UnrepresentableBindingPowers { lbp, rbp }) => {
                self.diagnostics.push(LrDiagnostic {
                    code: NO_PRECEDENCE_EQUIVALENT,
                    message: format!(
                        "Pratt variant `{}` has lbp = {lbp} and rbp = {rbp}, which no rule \
                         precedence reproduces; the table-driven parser style needs rbp = lbp + 1 \
                         (left-associative), rbp = lbp or rbp = lbp - 1 (right-associative)",
                        self.grammar.declarations[variant.declaration.index()].name
                    ),
                    items: Vec::new(),
                    actions: Vec::new(),
                    notes: Vec::new(),
                });
                None
            }
        }
    }

    /// Adds one construction rule, counting its value-carrying symbols.
    fn add_construct(
        &mut self,
        lhs: NonterminalId,
        rhs: Vec<Symbol>,
        variant: Option<crate::DeclId>,
        type_id: TypeId,
        precedence: Option<RulePrecedence>,
    ) {
        let value_pops = self.value_pops(&rhs);
        self.lr.add_rule(Rule {
            lhs,
            rhs,
            value_pops,
            precedence,
            action: RuleAction::Construct { type_id, variant },
        });
    }

    /// Counts the value-carrying symbols of a right-hand side.
    fn value_pops(&self, rhs: &[Symbol]) -> u32 {
        rhs.iter()
            .filter(|symbol| self.carries_value(**symbol))
            .count() as u32
    }

    /// Reports whether a symbol pushes a value when reduced or shifted.
    fn carries_value(&self, symbol: Symbol) -> bool {
        match symbol {
            Symbol::Terminal(_) => false,
            Symbol::Nonterminal(id) => self.lr.nonterminal(id).carries_value,
        }
    }

    /// Lowers a site list to symbols.
    fn sites_symbols(&mut self, sites: &[SiteId]) -> Vec<Symbol> {
        sites.iter().map(|site| self.site_symbol(*site)).collect()
    }

    /// Lowers one site: its target symbol wrapped in helpers for its container shape.
    fn site_symbol(&mut self, site_id: SiteId) -> Symbol {
        let site = self.grammar.sites[site_id.index()].clone();
        let target = self.type_symbols[site.target.index()];
        let presence = self.roles.presence_sites.contains(&site_id);
        self.container_symbol(target, &site.container, presence)
    }

    /// Wraps a symbol in the helper nonterminals a container shape needs.
    fn container_symbol(&mut self, inner: Symbol, shape: &ContainerShape, presence: bool) -> Symbol {
        match shape {
            ContainerShape::Direct => inner,
            ContainerShape::Boxed(element) => self.container_symbol(inner, element, presence),
            ContainerShape::Optional(element) => {
                let inner = self.container_symbol(inner, element, false);
                let valued = presence || self.carries_value(inner);
                let key = HelperKey::Optional { inner, valued };
                if let Some(id) = self.helpers.get(&key) {
                    return Symbol::Nonterminal(*id);
                }
                let name = self.unique_name(format!(
                    "{}({})",
                    if presence { "presence" } else { "opt" },
                    self.lr.symbol_name(inner)
                ));
                let id = self
                    .lr
                    .add_nonterminal(name, NonterminalOrigin::Helper, valued);
                self.helpers.insert(key, id);
                self.lr.add_rule(Rule {
                    lhs: id,
                    rhs: Vec::new(),
                    value_pops: 0,
                    precedence: None,
                    action: RuleAction::OptionalAbsent,
                });
                let value_pops = self.value_pops(&[inner]);
                self.lr.add_rule(Rule {
                    lhs: id,
                    rhs: vec![inner],
                    value_pops,
                    precedence: None,
                    action: RuleAction::OptionalPresent,
                });
                Symbol::Nonterminal(id)
            }
            ContainerShape::Repeat {
                element,
                cardinality,
                separator,
            } => {
                let element = self.container_symbol(inner, element, false);
                let separator = separator.map(|separator| {
                    let symbol = self.type_symbols[separator.type_id.index()];
                    (symbol, separator.optional_trailing)
                });
                self.list_symbol(
                    element,
                    separator.map(|(symbol, _)| symbol),
                    *cardinality,
                    separator.is_some_and(|(_, trailing)| trailing),
                )
            }
        }
    }

    /// Returns the list helper for an element, creating its rules on first use.
    ///
    /// One-or-more: `L1 -> e | L1 sep e`, with `L1t -> L1 | L1 sep` when a trailing separator is
    /// allowed. Zero-or-more: `L -> %empty | L1` with a separator, `L -> %empty | L e` without.
    fn list_symbol(
        &mut self,
        element: Symbol,
        separator: Option<Symbol>,
        cardinality: RepetitionCardinality,
        optional_trailing: bool,
    ) -> Symbol {
        let key = HelperKey::List {
            element,
            separator,
            one_or_more: cardinality == RepetitionCardinality::OneOrMore,
            optional_trailing,
        };
        if let Some(id) = self.helpers.get(&key) {
            return Symbol::Nonterminal(*id);
        }
        let valued = self.carries_value(element);
        let element_pops = self.value_pops(&[element]);
        let separator_pops = separator.map_or(0, |separator| self.value_pops(&[separator]));
        let describe = |this: &Self, prefix: &str| {
            let mut name = format!("{prefix}({}", this.lr.symbol_name(element));
            if let Some(separator) = separator {
                name.push_str(", sep=");
                name.push_str(this.lr.symbol_name(separator));
            }
            name.push(')');
            name
        };

        let symbol = match cardinality {
            RepetitionCardinality::OneOrMore => {
                let core = self.one_or_more_core(element, separator, valued, describe(self, "list1"));
                if optional_trailing {
                    let name = self.unique_name(describe(self, "list1t"));
                    let id = self
                        .lr
                        .add_nonterminal(name, NonterminalOrigin::Helper, valued);
                    self.lr.add_rule(Rule {
                        lhs: id,
                        rhs: vec![Symbol::Nonterminal(core)],
                        value_pops: u32::from(valued),
                        precedence: None,
                        action: RuleAction::ListPass,
                    });
                    let separator = separator.expect("a trailing separator needs a separator");
                    self.lr.add_rule(Rule {
                        lhs: id,
                        rhs: vec![Symbol::Nonterminal(core), separator],
                        value_pops: u32::from(valued) + separator_pops,
                        precedence: None,
                        action: RuleAction::ListPass,
                    });
                    id
                } else {
                    core
                }
            }
            RepetitionCardinality::ZeroOrMore => {
                let name = self.unique_name(describe(self, "list"));
                let id = self
                    .lr
                    .add_nonterminal(name, NonterminalOrigin::Helper, valued);
                self.lr.add_rule(Rule {
                    lhs: id,
                    rhs: Vec::new(),
                    value_pops: 0,
                    precedence: None,
                    action: RuleAction::ListEmpty,
                });
                if separator.is_some() {
                    let core = self.one_or_more_core(element, separator, valued, describe(self, "list1"));
                    let core_id = if optional_trailing {
                        self.list_symbol(element, separator, RepetitionCardinality::OneOrMore, true)
                    } else {
                        Symbol::Nonterminal(core)
                    };
                    self.lr.add_rule(Rule {
                        lhs: id,
                        rhs: vec![core_id],
                        value_pops: u32::from(valued),
                        precedence: None,
                        action: RuleAction::ListPass,
                    });
                } else {
                    self.lr.add_rule(Rule {
                        lhs: id,
                        rhs: vec![Symbol::Nonterminal(id), element],
                        value_pops: u32::from(valued) + element_pops,
                        precedence: None,
                        action: RuleAction::ListPush,
                    });
                }
                id
            }
        };
        self.helpers.insert(key, symbol);
        Symbol::Nonterminal(symbol)
    }

    /// Returns the shared `L1 -> e | L1 sep e` helper, creating it on first use.
    fn one_or_more_core(
        &mut self,
        element: Symbol,
        separator: Option<Symbol>,
        valued: bool,
        name: String,
    ) -> NonterminalId {
        let key = HelperKey::List {
            element,
            separator,
            one_or_more: true,
            optional_trailing: false,
        };
        if let Some(id) = self.helpers.get(&key) {
            return *id;
        }
        let name = self.unique_name(name);
        let id = self
            .lr
            .add_nonterminal(name, NonterminalOrigin::Helper, valued);
        self.helpers.insert(key, id);
        let element_pops = self.value_pops(&[element]);
        self.lr.add_rule(Rule {
            lhs: id,
            rhs: vec![element],
            value_pops: element_pops,
            precedence: None,
            action: RuleAction::ListFirst,
        });
        let mut rhs = vec![Symbol::Nonterminal(id)];
        let mut value_pops = u32::from(valued);
        if let Some(separator) = separator {
            rhs.push(separator);
            value_pops += self.value_pops(&[separator]);
        }
        rhs.push(element);
        value_pops += element_pops;
        self.lr.add_rule(Rule {
            lhs: id,
            rhs,
            value_pops,
            precedence: None,
            action: RuleAction::ListPush,
        });
        id
    }

    /// Returns a nonterminal name not yet taken.
    fn unique_name(&mut self, name: String) -> String {
        if self.names.insert(name.clone()) {
            return name;
        }
        let mut counter = 2;
        loop {
            let candidate = format!("{name}#{counter}");
            if self.names.insert(candidate.clone()) {
                return candidate;
            }
            counter += 1;
        }
    }

    /// Adds a start rule for every entry type that has a nonterminal.
    fn lower_starts(&mut self) {
        for type_id in &self.roles.start_types {
            if let Symbol::Nonterminal(target) = self.type_symbols[type_id.index()] {
                self.lr.add_start(*type_id, target);
            }
        }
    }
}
