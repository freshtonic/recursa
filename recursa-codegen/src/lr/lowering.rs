#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Lowering from the normalized grammar to LR rules.
//!
//! Terminals are the grammar's token kinds. Every grammar type becomes one nonterminal, except a
//! fixed token of one kind, which is inlined as its terminal so that keywords cost no reduce.
//! Containers become shared helper nonterminals in the simplest correct form: nullable
//! optionals and left-recursive lists (ticket #114 refines them). Pratt enums become plain
//! alternatives with the implicit left operand prepended to infix and postfix rules, and their
//! binding powers are recorded on the rules for the precedence resolver; a pair with no
//! precedence equivalent is `RCA0402`. A `#[parse(pratt(exclude(...)))]` site names a distinct
//! language, so the Pratt nonterminal is cloned once per distinct exclusion set (`Expr[excl
//! In]`) without the excluded rules, the way gram.y has `b_expr` beside `a_expr`. Every parse
//! entry type gets a synthetic start rule. A text predicate other than `Any` is `RCA0403`.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use super::grammar::{
    LrGrammar, NonterminalId, NonterminalOrigin, Rule, RuleAction, RulePrecedence, Symbol,
    TerminalId, UnrepresentableBindingPowers,
};
use super::report::{LrDiagnostic, NO_PRECEDENCE_EQUIVALENT, TEXT_PREDICATE};
use crate::materialization::{
    MaterializedElement, MaterializedFieldLowering, MaterializedNodeBody, MaterializedPrattVariant,
    MaterializedSemanticFieldProjection,
};
use crate::{
    ContainerShape, DeclId, GrammarDescription, KindSet, PrattPosition, PrattVariant,
    RepetitionCardinality, SiteId, TextPredicate, TypeId, TypeShape,
};

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// A distinct Pratt language a site selects: a Pratt type minus some of its LED variants. Two
/// sites with equal sets share one clone.
pub struct PrattExclusion {
    /// The Pratt type restricted.
    pub type_id: TypeId,
    /// The excluded LED variants, as normalized declarations in ascending order, deduplicated.
    pub excluded: Vec<DeclId>,
}

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
    /// Sites that restrict their Pratt target with `#[parse(pratt(exclude(...)))]`, keyed by the
    /// site the authored field lowers to (the outer site when fixed tokens are attached).
    pub pratt_exclusions: BTreeMap<SiteId, PrattExclusion>,
    /// Fixed-token attachment sequences around one semantic site; a Pratt operand behind one
    /// carries its exclusion set through it.
    pub attachment_units: BTreeSet<TypeId>,
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
        let attachment_units: BTreeSet<TypeId> =
            map.attachments.iter().map(|unit| unit.unit).collect();
        Self {
            pratt_exclusions: pratt_exclusions(lowered),
            content_tokens,
            fixed_sequences,
            presence_sites,
            start_types,
            attachment_units,
        }
    }
}

/// Reads every `#[parse(pratt(exclude(...)))]` filter off the materialized fields and resolves
/// it to the site it lowers to and the normalized declarations of the excluded LED variants.
/// A name that is not a LED variant of the target is dropped here; the recursive-descent
/// planner reports it.
fn pratt_exclusions(
    lowered: &crate::materialization::LoweredGrammar,
) -> BTreeMap<SiteId, PrattExclusion> {
    let map = &lowered.lowering_map;
    let type_of_application: HashMap<_, _> = map
        .applications
        .iter()
        .map(|correlation| (correlation.application, correlation.lowered))
        .collect();
    let application_of_type: HashMap<_, _> = map
        .applications
        .iter()
        .map(|correlation| (correlation.lowered, correlation.application))
        .collect();
    let unit_semantic_site: HashMap<TypeId, SiteId> = map
        .attachments
        .iter()
        .map(|unit| (unit.unit, unit.sites[unit.semantic_index]))
        .collect();
    let mut exclusions = BTreeMap::new();
    for application in &lowered.applications {
        for group in application.body.element_groups() {
            for element in group.elements {
                let MaterializedElement::SemanticField {
                    field,
                    projection:
                        MaterializedSemanticFieldProjection::Direct {
                            pratt_extensions: Some(filter),
                            ..
                        },
                    ..
                } = element
                else {
                    continue;
                };
                let Some(site) = map.fields.iter().find_map(|correlation| {
                    match correlation.lowered {
                        MaterializedFieldLowering::ParseSite(site)
                            if correlation.application == application.id
                                && correlation.field == *field =>
                        {
                            Some(site)
                        }
                        _ => None,
                    }
                }) else {
                    continue;
                };
                let mut target = lowered.grammar.sites[site.index()].target;
                if let Some(semantic) = unit_semantic_site.get(&target) {
                    target = lowered.grammar.sites[semantic.index()].target;
                }
                let Some(target_application) = application_of_type.get(&target) else {
                    continue;
                };
                let Some(MaterializedNodeBody::Pratt(variants)) = lowered
                    .applications
                    .iter()
                    .find(|candidate| candidate.id == *target_application)
                    .map(|candidate| &candidate.body)
                else {
                    continue;
                };
                debug_assert_eq!(type_of_application.get(target_application), Some(&target));
                let mut excluded: Vec<DeclId> = filter
                    .excluded
                    .iter()
                    .filter_map(|name| {
                        variants.iter().find_map(|variant| match variant {
                            MaterializedPrattVariant::Led(led) if led.common.name == *name => {
                                Some(led.common.resolved_variant)
                            }
                            _ => None,
                        })
                    })
                    .filter_map(|resolved| {
                        map.variants
                            .iter()
                            .find(|correlation| {
                                correlation.application == *target_application
                                    && correlation.variant == resolved
                            })
                            .map(|correlation| correlation.lowered)
                    })
                    .collect();
                excluded.sort_unstable();
                excluded.dedup();
                if !excluded.is_empty() {
                    exclusions.insert(
                        site,
                        PrattExclusion {
                            type_id: target,
                            excluded,
                        },
                    );
                }
            }
        }
    }
    exclusions
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
    lowering.lower_clones();
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
    /// The clone of each distinct Pratt exclusion, created on first reference.
    clones: BTreeMap<PrattExclusion, NonterminalId>,
    /// The clone of each attachment sequence around a Pratt operand under an exclusion.
    attachment_clones: BTreeMap<(TypeId, PrattExclusion), NonterminalId>,
    /// Clones whose rules are still to be emitted, in creation order.
    pending_clones: VecDeque<(PrattExclusion, NonterminalId)>,
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
            clones: BTreeMap::new(),
            attachment_clones: BTreeMap::new(),
            pending_clones: VecDeque::new(),
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
                        let precedence = self.variant_precedence(&variant);
                        self.add_pratt_rule(lhs, type_id, &variant, precedence, None);
                    }
                }
            }
        }
    }

    /// Adds the rule of one Pratt variant to `lhs`: the implicit left operand for an infix or
    /// postfix variant, then its sites. Under an exclusion the operands of a prefix, infix or
    /// postfix variant refer to the clone; an atom's operands keep the full language, the way
    /// gram.y's `b_expr` has `'(' a_expr ')'`.
    fn add_pratt_rule(
        &mut self,
        lhs: NonterminalId,
        type_id: TypeId,
        variant: &PrattVariant,
        precedence: Option<RulePrecedence>,
        exclusion: Option<&PrattExclusion>,
    ) {
        let mut rhs = Vec::new();
        if matches!(
            variant.position,
            PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
        ) {
            rhs.push(Symbol::Nonterminal(lhs));
        }
        let operand_exclusion = match variant.position {
            PrattPosition::Atom => None,
            _ => exclusion,
        };
        for site in &variant.sites {
            rhs.push(self.site_symbol_under(*site, operand_exclusion));
        }
        self.add_construct(lhs, rhs, Some(variant.declaration), type_id, precedence);
    }

    /// Emits the rules of every clone created so far, including the clones those rules create.
    fn lower_clones(&mut self) {
        while let Some((exclusion, lhs)) = self.pending_clones.pop_front() {
            let TypeShape::Pratt { variants } =
                self.grammar.types[exclusion.type_id.index()].shape.clone()
            else {
                unreachable!("only Pratt types are cloned");
            };
            for variant in variants {
                if exclusion.excluded.contains(&variant.declaration) {
                    continue;
                }
                // The base lowering already reported an unrepresentable pair as `RCA0402`.
                let precedence = RulePrecedence::from_position(variant.position)
                    .ok()
                    .flatten();
                self.add_pratt_rule(lhs, exclusion.type_id, &variant, precedence, Some(&exclusion));
            }
        }
    }

    /// Returns the clone of `exclusion`, creating it on first reference; its rules are emitted
    /// by [`Self::lower_clones`].
    fn pratt_clone(&mut self, exclusion: &PrattExclusion) -> NonterminalId {
        if let Some(clone) = self.clones.get(exclusion) {
            return *clone;
        }
        let name = self.unique_name(self.exclusion_name(
            &self.grammar.types[exclusion.type_id.index()].name,
            exclusion,
        ));
        let clone = self.lr.add_nonterminal(
            name,
            NonterminalOrigin::Exclusion {
                type_id: exclusion.type_id,
                excluded: exclusion.excluded.clone(),
            },
            true,
        );
        self.clones.insert(exclusion.clone(), clone);
        self.pending_clones.push_back((exclusion.clone(), clone));
        clone
    }

    /// Returns the clone of the attachment sequence `unit` whose semantic site carries
    /// `exclusion`, creating it and its rule on first reference.
    fn attachment_clone(&mut self, unit: TypeId, exclusion: &PrattExclusion) -> NonterminalId {
        if let Some(clone) = self.attachment_clones.get(&(unit, exclusion.clone())) {
            return *clone;
        }
        let description = &self.grammar.types[unit.index()];
        let TypeShape::Sequence { sites } = description.shape.clone() else {
            unreachable!("attachment units are sequences");
        };
        let name = self.unique_name(self.exclusion_name(&description.name, exclusion));
        let clone = self.lr.add_nonterminal(
            name,
            NonterminalOrigin::Exclusion {
                type_id: unit,
                excluded: exclusion.excluded.clone(),
            },
            true,
        );
        self.attachment_clones
            .insert((unit, exclusion.clone()), clone);
        let rhs: Vec<Symbol> = sites
            .iter()
            .map(|site| self.site_symbol_under(*site, Some(exclusion)))
            .collect();
        self.add_construct(clone, rhs, None, unit, None);
        clone
    }

    /// Names a clone `Type[excl A,B]` after the excluded variants' authored names.
    fn exclusion_name(&self, base: &str, exclusion: &PrattExclusion) -> String {
        let owner = format!("{}::", self.grammar.types[exclusion.type_id.index()].name);
        let excluded: Vec<&str> = exclusion
            .excluded
            .iter()
            .map(|declaration| {
                let name = self.grammar.declarations[declaration.index()].name.as_str();
                name.strip_prefix(&owner).unwrap_or(name)
            })
            .collect();
        format!("{base}[excl {}]", excluded.join(","))
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
        self.site_symbol_under(site_id, None)
    }

    /// Lowers one site under an exclusion context: the site's own exclusion set wins, else the
    /// context applies when the site is a Pratt operand of the restricted type, directly or
    /// through an attachment sequence; any other target lowers as usual.
    fn site_symbol_under(&mut self, site_id: SiteId, context: Option<&PrattExclusion>) -> Symbol {
        let site = self.grammar.sites[site_id.index()].clone();
        let exclusion = self.roles.pratt_exclusions.get(&site_id).cloned();
        let exclusion = exclusion.as_ref().or(context);
        let target = match exclusion {
            Some(exclusion) if site.target == exclusion.type_id => {
                Symbol::Nonterminal(self.pratt_clone(exclusion))
            }
            Some(exclusion) if self.wraps_operand_of(site.target, exclusion.type_id) => {
                Symbol::Nonterminal(self.attachment_clone(site.target, exclusion))
            }
            _ => self.type_symbols[site.target.index()],
        };
        let presence = self.roles.presence_sites.contains(&site_id);
        self.container_symbol(target, &site.container, presence)
    }

    /// Reports whether `unit` is an attachment sequence whose semantic site targets `pratt`.
    fn wraps_operand_of(&self, unit: TypeId, pratt: TypeId) -> bool {
        if !self.roles.attachment_units.contains(&unit) {
            return false;
        }
        match &self.grammar.types[unit.index()].shape {
            TypeShape::Sequence { sites } => sites
                .iter()
                .any(|site| self.grammar.sites[site.index()].target == pratt),
            _ => false,
        }
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
