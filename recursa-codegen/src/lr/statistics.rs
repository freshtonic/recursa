#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Deterministic structural statistics over a completed LR analysis.
//!
//! The report is derived solely from the normalized grammar, finished LR grammar, automaton and
//! packed tables. It therefore observes the same immutable structures generation renders and
//! does not require phase-local logging or a second automaton build.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write;

use super::automaton::{Automaton, StateId};
use super::grammar::{LrGrammar, NonterminalOrigin, RuleAction};
use super::lowering::{TypeRoles, expand_atoms};
use super::tables::{EncodedLrTables, LrTables};
use crate::materialization::LoweredGrammar;
use crate::{DeclId, TypeId, TypeShape};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Stable categories used to attribute nonterminals, rules and state kernels.
pub enum LrOrigin {
    /// A normalized grammar type.
    Type,
    /// A synthetic parse-capability start.
    Start,
    /// A shared container helper.
    Helper,
    /// A conflict-driven per-site copy of a shared helper.
    HelperCopy,
    /// A restricted Pratt language or its attachment unit.
    Restriction,
    /// An authored restricted expression.
    DeclaredRestriction,
}

impl LrOrigin {
    /// Returns the stable report spelling.
    const fn name(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Start => "start",
            Self::Helper => "helper",
            Self::HelperCopy => "helper-copy",
            Self::DeclaredRestriction => "declared-restriction",
            Self::Restriction => "restriction",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Counts attributed to the five [`LrOrigin`] categories.
pub struct OriginStatistics {
    /// Normalized grammar types.
    pub r#type: usize,
    /// Synthetic parse-capability starts.
    pub start: usize,
    /// Shared container helpers.
    pub helper: usize,
    /// Conflict-driven helper copies.
    pub helper_copy: usize,
    /// Restricted Pratt languages and attachment units.
    pub restriction: usize,
    /// Authored restricted expressions.
    pub declared_restriction: usize,
}

impl OriginStatistics {
    /// Returns the sum of all origin categories.
    pub const fn total(&self) -> usize {
        self.r#type
            + self.start
            + self.helper
            + self.helper_copy
            + self.restriction
            + self.declared_restriction
    }

    /// Increments one origin category.
    fn increment(&mut self, origin: LrOrigin) {
        match origin {
            LrOrigin::Type => self.r#type += 1,
            LrOrigin::Start => self.start += 1,
            LrOrigin::Helper => self.helper += 1,
            LrOrigin::HelperCopy => self.helper_copy += 1,
            LrOrigin::Restriction => self.restriction += 1,
            LrOrigin::DeclaredRestriction => self.declared_restriction += 1,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Counts of normalized grammar type shapes.
pub struct NormalizedShapeStatistics {
    /// Token types.
    pub token: usize,
    /// Alias types.
    pub alias: usize,
    /// Sequence types.
    pub sequence: usize,
    /// Enum types.
    pub r#enum: usize,
    /// Pratt types.
    pub pratt: usize,
}

impl NormalizedShapeStatistics {
    /// Returns the number of normalized types.
    pub const fn total(&self) -> usize {
        self.token + self.alias + self.sequence + self.r#enum + self.pratt
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Counts of normalized types and sites by their materialization role.
pub struct TypeRoleStatistics {
    /// Content-bearing token types.
    pub content_tokens: usize,
    /// Fixed-token-only sequence types.
    pub fixed_sequences: usize,
    /// Fixed-token presence sites.
    pub presence_sites: usize,
    /// Types retaining parse capability.
    pub start_types: usize,
    /// Sites carrying authored Pratt exclusions.
    pub pratt_exclusions: usize,
    /// Materialized Node applications.
    pub applications: usize,
    /// Repetition attachment units.
    pub attachment_units: usize,
    /// Fixed-token presence envelopes.
    pub presence_envelopes: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Counts of rules by reduce-action family.
pub struct RuleActionStatistics {
    /// Parse-capability accept rules.
    pub accept: usize,
    /// Content-token rules.
    pub token: usize,
    /// Node, enum, sequence and Pratt construction rules.
    pub construct: usize,
    /// Fixed-token-only sequence rules.
    pub fixed_sequence: usize,
    /// Attachment pass-through rules.
    pub attachment: usize,
    /// Fixed-token presence-envelope rules.
    pub envelope: usize,
    /// Absent optional-helper rules.
    pub optional_absent: usize,
    /// Present optional-helper rules.
    pub optional_present: usize,
    /// Empty list-helper rules.
    pub list_empty: usize,
    /// First-element list-helper rules.
    pub list_first: usize,
    /// List-extension rules.
    pub list_push: usize,
    /// Non-empty to zero-or-more conversion rules.
    pub list_from_non_empty: usize,
    /// Optional-trailing-separator pass-through rules.
    pub list_trailing: usize,
}

impl RuleActionStatistics {
    /// Returns the number of rules across all action families.
    pub const fn total(&self) -> usize {
        self.accept
            + self.token
            + self.construct
            + self.fixed_sequence
            + self.attachment
            + self.envelope
            + self.optional_absent
            + self.optional_present
            + self.list_empty
            + self.list_first
            + self.list_push
            + self.list_from_non_empty
            + self.list_trailing
    }

    /// Records one reduce action.
    fn record(&mut self, action: &RuleAction) {
        match action {
            RuleAction::Accept { .. } => self.accept += 1,
            RuleAction::Token { .. } => self.token += 1,
            RuleAction::Construct { .. } => self.construct += 1,
            RuleAction::FixedSequence { .. } => self.fixed_sequence += 1,
            RuleAction::Attachment { .. } => self.attachment += 1,
            RuleAction::Envelope { .. } => self.envelope += 1,
            RuleAction::OptionalAbsent { .. } => self.optional_absent += 1,
            RuleAction::OptionalPresent { .. } => self.optional_present += 1,
            RuleAction::ListEmpty { .. } => self.list_empty += 1,
            RuleAction::ListFirst { .. } => self.list_first += 1,
            RuleAction::ListPush { .. } => self.list_push += 1,
            RuleAction::ListFromNonEmpty { .. } => self.list_from_non_empty += 1,
            RuleAction::ListTrailing => self.list_trailing += 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One combination of origins occurring in otherwise unattributed mixed state kernels.
pub struct KernelMixStatistics {
    /// Origins present, in stable category order.
    pub origins: Vec<LrOrigin>,
    /// States with exactly this origin combination.
    pub states: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// State counts attributed from their kernel rules.
pub struct StateStatistics {
    /// States whose kernel rules all have one origin, attributed by that origin.
    pub exclusive: OriginStatistics,
    /// Mixed-origin kernel combinations.
    pub mixed: Vec<KernelMixStatistics>,
}

impl StateStatistics {
    /// Returns the number of states, exclusive and mixed.
    pub fn total(&self) -> usize {
        self.exclusive.total() + self.mixed.iter().map(|mix| mix.states).sum::<usize>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Size and fixed-width encoding of one packed LR table.
pub struct TableStatistics {
    /// Stable generated-table name.
    pub name: &'static str,
    /// Decoded cell count.
    pub cells: usize,
    /// Bytes occupied by one decoded runtime cell.
    pub decoded_width: usize,
    /// Bytes occupied by all decoded runtime cells.
    pub decoded_bytes: usize,
    /// Base64 digits per encoded cell.
    pub encoded_width: usize,
    /// Encoded payload bytes, excluding the one-byte width prefix.
    pub encoded_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Membership size of one content-bearing token type.
pub struct ContentTokenStatistics {
    /// Dense normalized grammar type identity.
    pub type_id: TypeId,
    /// Normalized type name.
    pub name: String,
    /// Named atom selected by the type.
    pub atom: String,
    /// Concrete token kinds accepted by the atom.
    pub kinds: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Counts for one distinct restricted Pratt language.
pub struct PrattRestrictionStatistics {
    /// Restricted normalized type.
    pub type_id: TypeId,
    /// Normalized type name.
    pub name: String,
    /// Excluded declarations in normalized order.
    pub excluded: Vec<DeclId>,
    /// Minimum admitted binding power.
    pub min_bp: u8,
    /// Nonterminals carrying this restriction.
    pub nonterminals: usize,
    /// Rules owned by those nonterminals.
    pub rules: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Reachability statistics for one public parse-capability start.
pub struct StartStatistics {
    /// Dense normalized grammar type identity.
    pub type_id: TypeId,
    /// Normalized type name.
    pub name: String,
    /// Existing automaton start state.
    pub state: StateId,
    /// States reachable through finished automaton transitions.
    pub reachable_states: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete deterministic statistics for one normalized grammar and generated LR machine.
pub struct LrStatistics {
    /// Terminals including the synthetic end marker.
    pub terminals: usize,
    /// Normalized atom expressions.
    pub atoms: usize,
    /// Normalized parse sites.
    pub sites: usize,
    /// Normalized grammar type shapes.
    pub shapes: NormalizedShapeStatistics,
    /// Normalized materialization roles.
    pub roles: TypeRoleStatistics,
    /// LR nonterminals by semantic origin.
    pub nonterminals: OriginStatistics,
    /// LR rules by their left-hand-side origin.
    pub rules: OriginStatistics,
    /// LR rules by reduce-action family.
    pub rule_actions: RuleActionStatistics,
    /// Automaton states by exclusive or mixed kernel origin.
    pub states: StateStatistics,
    /// Packed tables in generated argument order.
    pub tables: Vec<TableStatistics>,
    /// Content-bearing token types in normalized type order.
    pub content_tokens: Vec<ContentTokenStatistics>,
    /// Distinct restricted Pratt languages in semantic order.
    pub pratt_restrictions: Vec<PrattRestrictionStatistics>,
    /// Public parse-capability starts in start-rule order.
    pub starts: Vec<StartStatistics>,
}

impl LrStatistics {
    /// Collects statistics from the completed structures of one analysis.
    pub(super) fn collect(
        lowered: &LoweredGrammar,
        roles: &TypeRoles,
        grammar: &LrGrammar,
        automaton: &Automaton,
        tables: &LrTables,
    ) -> Self {
        let shapes = shape_statistics(lowered);
        let role_statistics = role_statistics(roles);
        let mut nonterminals = OriginStatistics::default();
        for nonterminal in grammar.nonterminals() {
            nonterminals.increment(origin(&nonterminal.origin));
        }
        let mut rules = OriginStatistics::default();
        let mut rule_actions = RuleActionStatistics::default();
        for rule in grammar.rules() {
            rules.increment(origin(&grammar.nonterminal(rule.lhs).origin));
            rule_actions.record(&rule.action);
        }

        Self {
            terminals: grammar.terminal_count(),
            atoms: lowered.grammar.atoms.len(),
            sites: lowered.grammar.sites.len(),
            shapes,
            roles: role_statistics,
            nonterminals,
            rules,
            rule_actions,
            states: state_statistics(grammar, automaton),
            tables: table_statistics(tables),
            content_tokens: content_token_statistics(lowered, roles),
            pratt_restrictions: pratt_restriction_statistics(lowered, grammar),
            starts: start_statistics(lowered, automaton),
        }
    }

    /// Renders a deterministic line-oriented report suitable for snapshots and simple tools.
    pub fn render(&self) -> String {
        let mut out = String::from("lr-statistics-v1\n");
        writeln!(out, "terminals={}", self.terminals).expect("writing to a String cannot fail");
        writeln!(out, "normalized.atoms={}", self.atoms).expect("String write");
        writeln!(out, "normalized.sites={}", self.sites).expect("String write");
        writeln!(out, "normalized.types={}", self.shapes.total()).expect("String write");
        writeln!(out, "shape.token={}", self.shapes.token).expect("String write");
        writeln!(out, "shape.alias={}", self.shapes.alias).expect("String write");
        writeln!(out, "shape.sequence={}", self.shapes.sequence).expect("String write");
        writeln!(out, "shape.enum={}", self.shapes.r#enum).expect("String write");
        writeln!(out, "shape.pratt={}", self.shapes.pratt).expect("String write");
        render_roles(&mut out, &self.roles);
        writeln!(out, "nonterminal.total={}", self.nonterminals.total()).expect("String write");
        render_origins(&mut out, "nonterminal", &self.nonterminals);
        writeln!(out, "rule.total={}", self.rules.total()).expect("String write");
        render_origins(&mut out, "rule", &self.rules);
        render_rule_actions(&mut out, &self.rule_actions);
        writeln!(out, "state.total={}", self.states.total()).expect("String write");
        render_origins(&mut out, "state.exclusive", &self.states.exclusive);
        for mix in &self.states.mixed {
            let names = mix
                .origins
                .iter()
                .map(|origin| origin.name())
                .collect::<Vec<_>>();
            writeln!(out, "state.mixed.{}={}", names.join("+"), mix.states).expect("String write");
        }
        for table in &self.tables {
            writeln!(
                out,
                "table.{}=cells:{} decoded-width:{} decoded-bytes:{} encoded-width:{} encoded-bytes:{}",
                table.name,
                table.cells,
                table.decoded_width,
                table.decoded_bytes,
                table.encoded_width,
                table.encoded_bytes
            )
            .expect("String write");
        }
        for token in &self.content_tokens {
            writeln!(
                out,
                "content-token.{}=name:{} atom:{} kinds:{}",
                token.type_id.index(),
                token.name,
                token.atom,
                token.kinds
            )
            .expect("String write");
        }
        for (index, restriction) in self.pratt_restrictions.iter().enumerate() {
            writeln!(
                out,
                "pratt-restriction.{index}=type:{} name:{} excluded:{} min-bp:{} nonterminals:{} rules:{}",
                restriction.type_id.index(),
                restriction.name,
                restriction.excluded.len(),
                restriction.min_bp,
                restriction.nonterminals,
                restriction.rules
            )
            .expect("String write");
        }
        for start in &self.starts {
            writeln!(
                out,
                "start.{}=name:{} state:{} reachable:{}",
                start.type_id.index(),
                start.name,
                start.state.index(),
                start.reachable_states
            )
            .expect("String write");
        }
        out
    }
}

/// Classifies one LR origin.
fn origin(origin: &NonterminalOrigin) -> LrOrigin {
    match origin {
        NonterminalOrigin::Type(_) => LrOrigin::Type,
        NonterminalOrigin::Start(_) => LrOrigin::Start,
        NonterminalOrigin::Helper => LrOrigin::Helper,
        NonterminalOrigin::HelperCopy { .. } => LrOrigin::HelperCopy,
        NonterminalOrigin::Restriction { .. } => LrOrigin::Restriction,
        NonterminalOrigin::DeclaredRestriction { .. } => LrOrigin::DeclaredRestriction,
    }
}

/// Counts normalized type shapes.
fn shape_statistics(lowered: &LoweredGrammar) -> NormalizedShapeStatistics {
    let mut result = NormalizedShapeStatistics::default();
    for description in &lowered.grammar.types {
        match description.shape {
            TypeShape::Token { .. } => result.token += 1,
            TypeShape::Alias { .. } => result.alias += 1,
            TypeShape::Sequence { .. } => result.sequence += 1,
            TypeShape::Enum { .. } => result.r#enum += 1,
            TypeShape::Pratt { .. } => result.pratt += 1,
        }
    }
    result
}

/// Counts materialization roles.
fn role_statistics(roles: &TypeRoles) -> TypeRoleStatistics {
    TypeRoleStatistics {
        content_tokens: roles.content_tokens.len(),
        fixed_sequences: roles.fixed_sequences.len(),
        presence_sites: roles.presence_sites.len(),
        start_types: roles.start_types.len(),
        pratt_exclusions: roles.pratt_exclusions.len(),
        applications: roles.applications.len(),
        attachment_units: roles.attachment_units.len(),
        presence_envelopes: roles.presence_envelopes.len(),
    }
}

/// Attributes states from the origins of rules in their kernels.
fn state_statistics(grammar: &LrGrammar, automaton: &Automaton) -> StateStatistics {
    let mut exclusive = OriginStatistics::default();
    let mut mixed = BTreeMap::<Vec<LrOrigin>, usize>::new();
    for state in automaton.states() {
        let origins = state
            .kernel()
            .iter()
            .map(|item| origin(&grammar.nonterminal(grammar.rule(item.rule).lhs).origin))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if let [only] = origins.as_slice() {
            exclusive.increment(*only);
        } else {
            *mixed.entry(origins).or_default() += 1;
        }
    }
    StateStatistics {
        exclusive,
        mixed: mixed
            .into_iter()
            .map(|(origins, states)| KernelMixStatistics { origins, states })
            .collect(),
    }
}

/// Collects decoded and encoded table dimensions.
fn table_statistics(tables: &LrTables) -> Vec<TableStatistics> {
    let encoded = tables.encode();
    let EncodedLrTables {
        pact,
        table,
        check,
        defact,
        pgoto,
        defgoto,
        r1,
        r2,
        value_pops,
        start_states,
        lookahead,
        folds,
        tail,
    } = &encoded;
    [
        ("pact", tables.pact.len(), pact),
        ("table", tables.table.len(), table),
        ("check", tables.check.len(), check),
        ("defact", tables.defact.len(), defact),
        ("pgoto", tables.pgoto.len(), pgoto),
        ("defgoto", tables.defgoto.len(), defgoto),
        ("r1", tables.r1.len(), r1),
        ("r2", tables.r2.len(), r2),
        ("value-pops", tables.value_pops.len(), value_pops),
        ("start-states", tables.start_states.len(), start_states),
        ("lookahead", tables.lookahead.len(), lookahead),
        ("folds", tables.folds.len(), folds),
        ("tail", tables.tail.len(), tail),
    ]
    .into_iter()
    .map(|(name, cells, encoded)| TableStatistics {
        name,
        cells,
        decoded_width: std::mem::size_of::<u32>(),
        decoded_bytes: cells * std::mem::size_of::<u32>(),
        encoded_width: encoded.width,
        encoded_bytes: encoded.text.len(),
    })
    .collect()
}

/// Reports each content token's expanded atom membership.
fn content_token_statistics(
    lowered: &LoweredGrammar,
    roles: &TypeRoles,
) -> Vec<ContentTokenStatistics> {
    let atom_kinds = expand_atoms(&lowered.grammar);
    roles
        .content_tokens
        .iter()
        .map(|type_id| {
            let description = &lowered.grammar.types[type_id.index()];
            let TypeShape::Token { atom, .. } = description.shape else {
                unreachable!("content-token roles name token types")
            };
            ContentTokenStatistics {
                type_id: *type_id,
                name: description.name.clone(),
                atom: lowered.grammar.atoms[atom.index()].name.clone(),
                kinds: atom_kinds[atom.index()].iter().count(),
            }
        })
        .collect()
}

/// Groups restriction-origin nonterminals and their rules by distinct Pratt language.
fn pratt_restriction_statistics(
    lowered: &LoweredGrammar,
    grammar: &LrGrammar,
) -> Vec<PrattRestrictionStatistics> {
    let mut groups = BTreeMap::<(TypeId, Vec<DeclId>, u8), (usize, usize)>::new();
    for (index, nonterminal) in grammar.nonterminals().iter().enumerate() {
        let NonterminalOrigin::Restriction {
            type_id,
            excluded,
            min_bp,
        } = &nonterminal.origin
        else {
            continue;
        };
        let entry = groups
            .entry((*type_id, excluded.clone(), *min_bp))
            .or_default();
        entry.0 += 1;
        entry.1 += grammar
            .rules_of(super::grammar::NonterminalId::new(index as u32))
            .len();
    }
    groups
        .into_iter()
        .map(
            |((type_id, excluded, min_bp), (nonterminals, rules))| PrattRestrictionStatistics {
                type_id,
                name: lowered.grammar.types[type_id.index()].name.clone(),
                excluded,
                min_bp,
                nonterminals,
                rules,
            },
        )
        .collect()
}

/// Computes graph reachability for every existing parse-capability start.
fn start_statistics(lowered: &LoweredGrammar, automaton: &Automaton) -> Vec<StartStatistics> {
    automaton
        .start_states()
        .iter()
        .map(|(type_id, state)| StartStatistics {
            type_id: *type_id,
            name: lowered.grammar.types[type_id.index()].name.clone(),
            state: *state,
            reachable_states: reachable_states(automaton, *state),
        })
        .collect()
}

/// Counts states reachable from one existing state by finished transitions.
fn reachable_states(automaton: &Automaton, start: StateId) -> usize {
    let mut seen = vec![false; automaton.states().len()];
    let mut queue = VecDeque::from([start]);
    seen[start.index()] = true;
    while let Some(state) = queue.pop_front() {
        for (_, target) in automaton.states()[state.index()].transitions() {
            if !seen[target.index()] {
                seen[target.index()] = true;
                queue.push_back(*target);
            }
        }
    }
    seen.into_iter().filter(|visited| *visited).count()
}

/// Renders the five origin counts under one prefix.
fn render_origins(out: &mut String, prefix: &str, counts: &OriginStatistics) {
    writeln!(out, "{prefix}.type={}", counts.r#type).expect("String write");
    writeln!(out, "{prefix}.start={}", counts.start).expect("String write");
    writeln!(out, "{prefix}.helper={}", counts.helper).expect("String write");
    writeln!(out, "{prefix}.helper-copy={}", counts.helper_copy).expect("String write");
    writeln!(out, "{prefix}.restriction={}", counts.restriction).expect("String write");
}

/// Renders normalized materialization-role counts.
fn render_roles(out: &mut String, roles: &TypeRoleStatistics) {
    writeln!(out, "role.content-token={}", roles.content_tokens).expect("String write");
    writeln!(out, "role.fixed-sequence={}", roles.fixed_sequences).expect("String write");
    writeln!(out, "role.presence-site={}", roles.presence_sites).expect("String write");
    writeln!(out, "role.start-type={}", roles.start_types).expect("String write");
    writeln!(out, "role.pratt-exclusion={}", roles.pratt_exclusions).expect("String write");
    writeln!(out, "role.application={}", roles.applications).expect("String write");
    writeln!(out, "role.attachment-unit={}", roles.attachment_units).expect("String write");
    writeln!(out, "role.presence-envelope={}", roles.presence_envelopes).expect("String write");
}

/// Renders reduce-action-family counts.
fn render_rule_actions(out: &mut String, actions: &RuleActionStatistics) {
    for (name, count) in [
        ("accept", actions.accept),
        ("token", actions.token),
        ("construct", actions.construct),
        ("fixed-sequence", actions.fixed_sequence),
        ("attachment", actions.attachment),
        ("envelope", actions.envelope),
        ("optional-absent", actions.optional_absent),
        ("optional-present", actions.optional_present),
        ("list-empty", actions.list_empty),
        ("list-first", actions.list_first),
        ("list-push", actions.list_push),
        ("list-from-non-empty", actions.list_from_non_empty),
        ("list-trailing", actions.list_trailing),
    ] {
        writeln!(out, "rule-action.{name}={count}").expect("String write");
    }
}
