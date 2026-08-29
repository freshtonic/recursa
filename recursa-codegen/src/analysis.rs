#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Source-independent semantic analysis over a normalized grammar.
//!
//! The pipeline validates dense identities before computing nullable, FIRST,
//! FOLLOW, Pratt extender, conflict, and provenance facts. Successful facts
//! are frozen into decision products; failed prerequisites suppress every
//! dependent phase so diagnostics never describe invented semantics.

#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::rc::Rc;

use crate::ir::{ContainerCardinality, ContainerSemantics};
use crate::text_partition::{
    TextPartition, overlay_by_kind_residual, partition_by_kind_residual, partition_by_kind_summary,
};
use crate::{
    AtomExpression, AtomId, ConflictEvidence, ConflictRemedy, ConflictWitness, Diagnostic,
    DiagnosticCode, GrammarDescription, KindId, KindSet, Location, PrattPosition, ProofArena,
    ProofId, ProvenanceStep, ProvenancedKindSet, SiteId, TextEquivalence, TextPredicate,
    TextPredicateId, TypeId, TypeShape,
};

#[derive(Clone, Debug, Eq, PartialEq)]
/// Complete result of validating and analyzing one normalized grammar.
///
/// [`output`](Self::output) is present only when no error-severity diagnostic
/// prevents publishing a coherent set of parser decisions. Warnings may
/// accompany a successful output.
pub struct AnalysisReport {
    /// Diagnostics in deterministic source order.
    pub diagnostics: Vec<Diagnostic>,
    /// Frozen semantic facts and decisions, absent when analysis failed.
    pub output: Option<AnalysisOutput>,
    /// Source locations copied from the input for diagnostic rendering.
    locations: Vec<Location>,
    /// Deduplicated provenance proofs referenced by facts and diagnostics.
    proofs: ProofArena,
    /// Stable names and predicates needed to render normalized identities.
    diagnostic_context: DiagnosticContext,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// Stable display vocabulary retained separately from compact semantic identities.
struct DiagnosticContext {
    /// Token-kind names indexed by grammar-local identity.
    kind_names: Vec<String>,
    /// Atom names indexed by grammar-local identity.
    atom_names: Vec<String>,
    /// Declaration names indexed by grammar-local identity.
    declaration_names: Vec<String>,
    /// Grammar-type names indexed by grammar-local identity.
    type_names: Vec<String>,
    /// Exact-text predicates indexed for conflict evidence rendering.
    text_predicates: Vec<TextPredicate>,
}

impl DiagnosticContext {
    /// Indexes display names and text predicates for deterministic diagnostics.
    fn from_grammar(grammar: &GrammarDescription) -> Self {
        Self {
            kind_names: grammar.kinds.iter().map(|kind| kind.name.clone()).collect(),
            atom_names: grammar.atoms.iter().map(|atom| atom.name.clone()).collect(),
            declaration_names: grammar
                .declarations
                .iter()
                .map(|declaration| declaration.name.clone())
                .collect(),
            type_names: grammar.types.iter().map(|ty| ty.name.clone()).collect(),
            text_predicates: grammar.text_predicates.clone(),
        }
    }
}

impl AnalysisReport {
    /// Returns the location table used by diagnostic labels.
    pub fn locations(&self) -> &[Location] {
        &self.locations
    }

    /// Returns the provenance arena used by published facts and diagnostics.
    pub fn proofs(&self) -> &ProofArena {
        &self.proofs
    }

    /// Renders diagnostics in the stable, location-aware acceptance format.
    pub fn canonical_diagnostics(&self) -> String {
        use std::fmt::Write as _;

        let mut rendered = String::new();
        for diagnostic in &self.diagnostics {
            let severity = match diagnostic.severity {
                crate::Severity::Error => "error",
                crate::Severity::Warning => "warning",
            };
            writeln!(
                rendered,
                "{severity}[{}]: {}",
                diagnostic.code.as_str(),
                diagnostic.message,
            )
            .expect("writing to a String cannot fail");
            render_label(
                &mut rendered,
                "primary",
                &diagnostic.primary,
                &self.locations,
            );
            for label in &diagnostic.secondary {
                render_label(&mut rendered, "secondary", label, &self.locations);
            }
            if let Some(conflict) = &diagnostic.conflict {
                render_conflict(
                    &mut rendered,
                    conflict,
                    &self.proofs,
                    &self.diagnostic_context,
                );
            }
            if let Some(lookahead) = &diagnostic.lookahead {
                render_shared_prefix(&mut rendered, lookahead, &self.diagnostic_context);
            }
            if let Some(remedy) = &diagnostic.remedy {
                writeln!(rendered, "  remedy: {}", remedy.canonical_text())
                    .expect("writing to a String cannot fail");
            }
            if let Some(remedy) = &diagnostic.conflict_remedy {
                writeln!(rendered, "  remedy: {}", remedy.canonical_text())
                    .expect("writing to a String cannot fail");
            }
        }
        rendered
    }
}

/// Renders the bounded-lookahead witness attached to a dispatch conflict.
fn render_shared_prefix(
    rendered: &mut String,
    evidence: &crate::SharedPrefixEvidence,
    context: &DiagnosticContext,
) {
    use std::fmt::Write as _;

    let mut witness = evidence
        .witness
        .iter()
        .map(|position| context.kind_name(position.kind).to_owned())
        .collect::<Vec<_>>();
    if evidence.end_of_alternative {
        witness.push("<end>".to_owned());
    }
    let witness = witness.join(" ");
    writeln!(rendered, "  shared prefix: {witness}").expect("writing to a String cannot fail");
    if let Some(depth) = evidence.sufficient_depth {
        writeln!(
            rendered,
            "  lookahead: configured depth {} is exhausted; depth {depth} disambiguates",
            evidence.configured_depth,
        )
        .expect("writing to a String cannot fail");
    } else {
        writeln!(
            rendered,
            "  lookahead: no finite depth separates this shared prefix",
        )
        .expect("writing to a String cannot fail");
    }
}

/// Renders overlapping token kinds and both canonical provenance witnesses.
fn render_conflict(
    rendered: &mut String,
    conflict: &ConflictEvidence,
    proofs: &ProofArena,
    context: &DiagnosticContext,
) {
    use std::fmt::Write as _;

    let names = conflict
        .overlap
        .iter()
        .map(|kind| context.kind_name(kind))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(rendered, "  overlap: {{{names}}}").expect("writing to a String cannot fail");
    writeln!(
        rendered,
        "  witness: {}",
        context.kind_name(conflict.witness.kind),
    )
    .expect("writing to a String cannot fail");
    render_proof(
        rendered,
        "left proof",
        conflict.witness.left,
        proofs,
        context,
    );
    render_proof(
        rendered,
        "right proof",
        conflict.witness.right,
        proofs,
        context,
    );
}

/// Renders one proof chain from its direct origin through every propagation step.
fn render_proof(
    rendered: &mut String,
    label: &str,
    proof: ProofId,
    proofs: &ProofArena,
    context: &DiagnosticContext,
) {
    use std::fmt::Write as _;

    writeln!(rendered, "  {label}:").expect("writing to a String cannot fail");
    for node in proofs.chain(proof) {
        writeln!(rendered, "    - {}", context.render_step(&node.step))
            .expect("writing to a String cannot fail");
    }
}

impl DiagnosticContext {
    /// Resolves a token-kind identity for diagnostics, tolerating invalid input grammars.
    fn kind_name(&self, kind: KindId) -> &str {
        self.kind_names
            .get(kind.index())
            .map_or("<missing kind>", String::as_str)
    }

    /// Resolves an atom identity for diagnostics, tolerating invalid input grammars.
    fn atom_name(&self, atom: AtomId) -> &str {
        self.atom_names
            .get(atom.index())
            .map_or("<missing atom>", String::as_str)
    }

    /// Resolves a declaration identity for diagnostics, tolerating invalid input grammars.
    fn declaration_name(&self, declaration: crate::DeclId) -> &str {
        self.declaration_names
            .get(declaration.index())
            .map_or("<missing declaration>", String::as_str)
    }

    /// Resolves a grammar-type identity for diagnostics, tolerating invalid input grammars.
    fn type_name(&self, type_id: TypeId) -> &str {
        self.type_names
            .get(type_id.index())
            .map_or("<missing type>", String::as_str)
    }

    /// Describes one provenance edge with names from the retained diagnostic context.
    fn render_step(&self, step: &ProvenanceStep) -> String {
        match step {
            ProvenanceStep::DirectDeclaration { atom, kind, .. } => format!(
                "kind `{}` is declared by atom `{}`",
                self.kind_name(*kind),
                self.atom_name(*atom),
            ),
            ProvenanceStep::CategoryMembership { category, .. } => {
                format!("category `{}` includes it", self.atom_name(*category))
            }
            ProvenanceStep::ClassMembership { class, .. } => {
                format!("class `{}` includes it", self.atom_name(*class))
            }
            ProvenanceStep::TargetAdmission { target, .. } => {
                format!("target `{}` admits it", self.atom_name(*target))
            }
            ProvenanceStep::Reference {
                owner,
                target,
                declaration,
                ..
            } => format!(
                "`{}` references `{}` at `{}`",
                self.type_name(*owner),
                self.type_name(*target),
                self.declaration_name(*declaration),
            ),
            ProvenanceStep::FirstPropagation { type_id, .. } => {
                format!("FIRST propagates through `{}`", self.type_name(*type_id))
            }
            ProvenanceStep::NullablePrefix {
                owner, declaration, ..
            } => format!(
                "nullable prefix of `{}` passes `{}`",
                self.type_name(*owner),
                self.declaration_name(*declaration),
            ),
            ProvenanceStep::PrattExtender {
                pratt,
                variant,
                position,
                ..
            } => match position {
                PrattPosition::Infix { lbp, rbp } => format!(
                    "Pratt `{}` admits infix extender `{}` with lbp {lbp} and rbp {rbp}",
                    self.type_name(*pratt),
                    self.declaration_name(*variant),
                ),
                PrattPosition::Postfix { bp } => format!(
                    "Pratt `{}` admits postfix extender `{}` with bp {bp}",
                    self.type_name(*pratt),
                    self.declaration_name(*variant),
                ),
                PrattPosition::Atom | PrattPosition::Prefix { .. } => {
                    unreachable!("only Pratt led variants are extenders")
                }
            },
            ProvenanceStep::TextPredicate { predicate, .. } => {
                let predicate = self.text_predicates.get(predicate.index()).map_or_else(
                    || "<missing predicate>".to_owned(),
                    |value| format!("{value:?}"),
                );
                format!("text is refined by {predicate}")
            }
            ProvenanceStep::EntryEof { type_id, .. } => {
                format!("entry `{}` is followed by EOF", self.type_name(*type_id))
            }
            ProvenanceStep::FollowSuffix {
                owner, declaration, ..
            } => format!(
                "suffix of `{}` follows `{}`",
                self.type_name(*owner),
                self.declaration_name(*declaration),
            ),
            ProvenanceStep::FollowPropagation {
                owner,
                target,
                declaration,
                ..
            } => format!(
                "FOLLOW of `{}` propagates to `{}` at `{}`",
                self.type_name(*owner),
                self.type_name(*target),
                self.declaration_name(*declaration),
            ),
            ProvenanceStep::RepeatContinuation {
                owner, declaration, ..
            } => format!(
                "repetition in `{}` continues after `{}`",
                self.type_name(*owner),
                self.declaration_name(*declaration),
            ),
        }
    }
}

/// Renders one diagnostic label with its validated source range.
fn render_label(
    rendered: &mut String,
    style: &str,
    label: &crate::DiagnosticLabel,
    locations: &[Location],
) {
    use std::fmt::Write as _;

    if let Some(location) = label.location.and_then(|id| locations.get(id.index())) {
        writeln!(
            rendered,
            "  {style} source[{}] {}..{}: {}",
            location.source.get(),
            location.range.start,
            location.range.end,
            label.message,
        )
        .expect("writing to a String cannot fail");
    } else {
        writeln!(rendered, "  {style} <normalized input>: {}", label.message)
            .expect("writing to a String cannot fail");
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Successful source-independent facts and executable parser decisions.
///
/// All per-type and per-site slices use the dense IDs from the analyzed
/// [`GrammarDescription`]. Callers must therefore keep the output paired with
/// that grammar rather than combining facts from separate analyses.
pub struct AnalysisOutput {
    /// Supplies the first by type consumed by semantic analysis.
    first_by_type: Vec<FirstFacts>,
    /// Supplies the extenders by type consumed by semantic analysis.
    extenders_by_type: Vec<Option<ExtenderFacts>>,
    /// Supplies the first provenance by type consumed by semantic analysis.
    first_provenance_by_type: Vec<Vec<ProvenancedKindSet>>,
    /// Supplies the extender provenance by type consumed by semantic analysis.
    extender_provenance_by_type: Vec<Option<Vec<ProvenancedKindSet>>>,
    /// Supplies the follow by type consumed by semantic analysis.
    follow_by_type: Vec<FollowFacts>,
    /// Supplies the follow by site consumed by semantic analysis.
    follow_by_site: Vec<FollowFacts>,
    /// Supplies the decisions consumed by semantic analysis.
    decisions: crate::DecisionProducts,
}

impl AnalysisOutput {
    /// Returns FIRST facts for `type_id`, or `None` for an out-of-range ID.
    pub fn first(&self, type_id: TypeId) -> Option<&FirstFacts> {
        self.first_by_type.get(type_id.index())
    }

    /// Returns FIRST facts indexed by [`TypeId::index`](crate::TypeId::index).
    pub fn first_by_type(&self) -> &[FirstFacts] {
        &self.first_by_type
    }

    /// Returns Pratt extender facts for `type_id` when it names a Pratt type.
    pub fn extenders(&self, type_id: TypeId) -> Option<&ExtenderFacts> {
        self.extenders_by_type
            .get(type_id.index())
            .and_then(Option::as_ref)
    }

    /// Returns optional Pratt extender facts indexed by type ID.
    pub fn extenders_by_type(&self) -> &[Option<ExtenderFacts>] {
        &self.extenders_by_type
    }

    /// Returns provenance for each FIRST position of every type.
    pub fn first_provenance_by_type(&self) -> &[Vec<ProvenancedKindSet>] {
        &self.first_provenance_by_type
    }

    /// Returns provenance for the Pratt extender positions of `type_id`.
    pub fn extender_provenance(&self, type_id: TypeId) -> Option<&[ProvenancedKindSet]> {
        self.extender_provenance_by_type
            .get(type_id.index())
            .and_then(Option::as_deref)
    }

    /// Returns optional Pratt extender provenance indexed by type ID.
    pub fn extender_provenance_by_type(&self) -> &[Option<Vec<ProvenancedKindSet>>] {
        &self.extender_provenance_by_type
    }

    /// Returns FOLLOW facts for `type_id`, or `None` for an out-of-range ID.
    pub fn follow(&self, type_id: TypeId) -> Option<&FollowFacts> {
        self.follow_by_type.get(type_id.index())
    }

    /// Returns caller FOLLOW facts for a particular parse site.
    pub fn follow_site(&self, site_id: SiteId) -> Option<&FollowFacts> {
        self.follow_by_site.get(site_id.index())
    }

    /// Returns FOLLOW facts indexed by type ID.
    pub fn follow_by_type(&self) -> &[FollowFacts] {
        &self.follow_by_type
    }

    /// Returns caller FOLLOW facts indexed by parse-site ID.
    pub fn follow_by_site(&self) -> &[FollowFacts] {
        &self.follow_by_site
    }

    /// Returns the validated decisions consumed by generation.
    pub const fn decisions(&self) -> &crate::DecisionProducts {
        &self.decisions
    }

    /// Renders the stable summary of FIRST, nullability, and Pratt extenders.
    pub fn canonical_facts(&self) -> String {
        use std::fmt::Write as _;

        let mut rendered = String::new();
        for (index, first) in self.first_by_type.iter().enumerate() {
            let extenders = self.extenders_by_type[index]
                .as_ref()
                .map_or_else(|| "-".to_owned(), |facts| render_kind_set(&facts.kinds));
            writeln!(
                rendered,
                "type[{index}] FIRST={} NULLABLE={} EXTENDERS={extenders}",
                render_kind_set(&first.kinds),
                first.nullable,
            )
            .expect("writing to a String cannot fail");
        }
        rendered
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// FIRST positions, bounded prefixes, and nullability for one grammar type.
pub struct FirstFacts {
    /// Union of token kinds admitted at the first position.
    kinds: KindSet,
    /// First-position alternatives, retaining optional text predicates.
    pub positions: Vec<FirstPosition>,
    /// Bounded lookahead sequences for this grammar type.
    pub sequences: Vec<FirstSequence>,
    /// Whether the type can consume no tokens.
    pub nullable: bool,
}

impl FirstFacts {
    /// Returns the union of possible first token kinds.
    pub fn kinds(&self) -> &KindSet {
        &self.kinds
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Token kinds and positions that can extend a parsed Pratt left-hand side.
pub struct ExtenderFacts {
    /// Supplies the kinds consumed by semantic analysis.
    kinds: KindSet,
    /// Supplies the positions consumed by semantic analysis.
    pub positions: Vec<FirstPosition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Provenanced token kinds and EOF membership that may follow an entity.
pub struct FollowFacts {
    /// Supplies the kinds consumed by semantic analysis.
    kinds: ProvenancedKindSet,
    /// Supplies the eof proof consumed by semantic analysis.
    eof_proof: Option<ProofId>,
}

impl FollowFacts {
    /// Returns the token-kind portion of FOLLOW.
    pub fn kinds(&self) -> &KindSet {
        self.kinds.kinds()
    }

    /// Returns whether end-of-input belongs to FOLLOW.
    pub const fn eof(&self) -> bool {
        self.eof_proof.is_some()
    }

    /// Returns the provenance proof for EOF membership, when present.
    pub const fn eof_proof(&self) -> Option<ProofId> {
        self.eof_proof
    }

    /// Returns the provenance proof for `kind` membership, when present.
    pub fn proof(&self, kind: KindId) -> Option<ProofId> {
        self.kinds.proof(kind)
    }
}

impl ExtenderFacts {
    /// Returns the union of Pratt extender token kinds.
    pub fn kinds(&self) -> &KindSet {
        &self.kinds
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One token position in a bounded FIRST sequence.
pub struct FirstPosition {
    /// Token kinds accepted at this position.
    pub kinds: KindSet,
    /// Optional exact-text refinement within those token kinds.
    pub text_predicate: Option<TextPredicateId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One bounded lookahead prefix retained for conflict analysis.
pub struct FirstSequence {
    /// Ordered positions in the prefix.
    pub positions: Vec<FirstPosition>,
    /// Whether the described production can end after these positions.
    ends: bool,
}

impl FirstSequence {
    /// Whether this prefix can exhaust the production and must consult the
    /// dynamic caller FOLLOW set rather than assuming end-of-input.
    pub const fn ends(&self) -> bool {
        self.ends
    }
}

/// Validates `grammar` and computes its complete semantic analysis.
///
/// Validation and left-recursion failures stop dependent phases. Once the
/// grammar is structurally sound, FIRST, Pratt extenders, provenance, FOLLOW,
/// conflicts, and frozen decisions are computed in dependency order. This
/// ordering is part of the diagnostic contract: downstream phases never
/// manufacture findings from invalid upstream facts.
pub fn analyze(grammar: &GrammarDescription) -> AnalysisReport {
    // Phase boundary: stop after invalid prerequisites so later facts are never derived from an incoherent grammar.
    let locations = grammar.locations.clone();
    let diagnostic_context = DiagnosticContext::from_grammar(grammar);
    let diagnostics = validate(grammar);
    if !diagnostics.is_empty() {
        return AnalysisReport {
            diagnostics,
            output: None,
            locations,
            proofs: ProofArena::default(),
            diagnostic_context,
        };
    }

    let atom_kinds = expand_atoms(grammar);
    let mut first_by_type = compute_first(grammar, &atom_kinds);
    let diagnostics = detect_left_recursion(grammar, &first_by_type);
    if !diagnostics.is_empty() {
        return AnalysisReport {
            diagnostics,
            output: None,
            locations,
            proofs: ProofArena::default(),
            diagnostic_context,
        };
    }
    let first_sequence_analysis = FirstSequenceAnalysis::new(grammar, &atom_kinds);
    let extenders_by_type = compute_extenders(grammar, &first_by_type);
    let (mut proofs, first_provenance) = compute_first_provenance(grammar, &first_by_type);
    let extender_provenance =
        compute_extender_provenance(grammar, &first_by_type, &first_provenance, &mut proofs);
    let mut static_conflict_kinds_by_declaration =
        vec![KindSet::empty(grammar.kind_count); grammar.declarations.len()];
    let mut diagnostics = detect_conflicts(
        grammar,
        &first_by_type,
        &first_sequence_analysis,
        &first_provenance,
        &mut proofs,
        &mut static_conflict_kinds_by_declaration,
    );
    let (follow_by_type, follow_by_site, follow_positions_by_site) =
        compute_follow(grammar, &first_by_type, &first_provenance, &mut proofs);
    let warnings = detect_follow_conflicts(
        grammar,
        &first_by_type,
        &first_provenance,
        &follow_positions_by_site,
        &static_conflict_kinds_by_declaration,
        &mut proofs,
    );
    diagnostics.extend(warnings);
    diagnostics.sort_by_key(|diagnostic| diagnostic_source_key(diagnostic, grammar));
    let has_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::Severity::Error);
    let decisions = if has_errors {
        None
    } else {
        let planned =
            crate::decision_plan::plan_decisions(grammar, &first_by_type, &first_sequence_analysis);
        match publish_decisions(grammar, &first_by_type, &first_sequence_analysis, planned) {
            Ok(decisions) => Some(decisions),
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                None
            }
        }
    };
    if decisions.is_some() {
        populate_first_sequences(grammar, &first_sequence_analysis, &mut first_by_type);
    }
    let first_provenance_by_type = first_provenance
        .into_iter()
        .map(|positions| {
            positions
                .into_iter()
                .map(|position| position.kinds)
                .collect()
        })
        .collect();
    let extender_provenance_by_type = extender_provenance
        .into_iter()
        .map(|positions| {
            positions.map(|positions| {
                positions
                    .into_iter()
                    .map(|position| position.kinds)
                    .collect()
            })
        })
        .collect();
    let mut report = AnalysisReport {
        diagnostics,
        output: decisions.map(|decisions| AnalysisOutput {
            first_by_type,
            extenders_by_type,
            first_provenance_by_type,
            extender_provenance_by_type,
            follow_by_type,
            follow_by_site,
            decisions,
        }),
        locations,
        proofs,
        diagnostic_context,
    };
    compact_report_proofs(&mut report);
    report
}

/// Derives the publish decisions from normalized grammar structure and previously established semantic facts.
fn publish_decisions(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_sequences: &FirstSequenceAnalysis<'_>,
    decisions: crate::DecisionProducts,
) -> Result<crate::DecisionProducts, Box<Diagnostic>> {
    decisions
        .validate_with_first_sequences(grammar, semantic, first_sequences)
        .map_err(|invariant| {
            Box::new(Diagnostic::internal_analysis_invariant(format!(
                "invalid frozen decision product: {invariant}"
            )))
        })?;
    Ok(decisions)
}

/// Identifies one canonical paired residual set and remaining witness depth.
type DecisionWitnessCacheKey = (Vec<PairedDecisionState>, usize);
/// Stores exact paired residual witnesses shared across conflict sites.
type DecisionWitnessCache = BTreeMap<DecisionWitnessCacheKey, Option<Vec<DecisionWitness>>>;

/// Derives the first sequences for decision sites from normalized grammar structure and previously established semantic facts.
/// Reuses immutable grammar-wide FIRST support while keeping each root analysis isolated.
pub(crate) struct FirstSequenceAnalysis<'a> {
    /// Grammar analyzed by each root-local arena.
    grammar: &'a GrammarDescription,
    /// Expanded token kinds shared by each root-local arena.
    atom_kinds: Rc<[KindSet]>,
    /// Fixed-point end/continuation facts shared by root-local arenas.
    decision_summary_facts: Rc<DecisionSummaryFacts>,
    /// Fixed-point one-position languages shared by root-local arenas.
    decision_cutoff_facts: Rc<DecisionCutoffFacts>,
    /// Canonical grammar sites shared by root-local arenas.
    canonical_sites: Rc<[SiteId]>,
    /// Exact paired residual witnesses shared across conflict sites in this analysis run.
    decision_witnesses: RefCell<DecisionWitnessCache>,
    /// Normalized single-side decision frontiers shared across paired witness searches.
    decision_frontiers: RefCell<HashMap<DecisionState, Rc<DecisionFrontier>>>,
}

impl<'a> FirstSequenceAnalysis<'a> {
    /// Creates analysis-run FIRST support from one normalized grammar.
    pub(crate) fn for_grammar(grammar: &'a GrammarDescription) -> Self {
        let atom_kinds = expand_atoms(grammar);
        Self::new(grammar, &atom_kinds)
    }

    /// Creates one analysis-run FIRST support context.
    pub(crate) fn new(grammar: &'a GrammarDescription, atom_kinds: &[KindSet]) -> Self {
        let decision_summary_facts =
            Rc::new(DecisionSummaryFacts::from_grammar(grammar, atom_kinds));
        let decision_cutoff_facts = Rc::new(DecisionCutoffFacts::from_grammar(
            grammar,
            atom_kinds,
            &decision_summary_facts,
        ));
        let mut sites_by_semantics = HashMap::new();
        let canonical_sites = grammar
            .sites
            .iter()
            .enumerate()
            .map(|(index, site)| {
                let site_id = SiteId::new(index.try_into().expect("grammar site identity width"));
                *sites_by_semantics
                    .entry(decision_site_semantics(site))
                    .or_insert(site_id)
            })
            .collect::<Vec<_>>()
            .into();
        Self {
            grammar,
            atom_kinds: atom_kinds.to_vec().into(),
            decision_summary_facts,
            decision_cutoff_facts,
            canonical_sites,
            decision_witnesses: RefCell::new(BTreeMap::new()),
            decision_frontiers: RefCell::new(HashMap::new()),
        }
    }

    /// Derives one decision-site root with a fresh root-local arena.
    pub(crate) fn sequences(&self, sites: &[SiteId]) -> Vec<FirstSequence> {
        self.sequences_for_state(DecisionState::from_sites(sites))
    }

    /// Derives one arbitrary semantic decision-state root with a fresh root-local arena.
    fn sequences_for_state(&self, state: DecisionState) -> Vec<FirstSequence> {
        let mut language = self.language();
        let root = language.build(self.grammar.max_lookahead, vec![state]);
        language.materialize(root)
    }

    /// Canonicalizes authored site identities before using a decision state as a phase cache key.
    fn canonical_state(&self, state: DecisionState) -> DecisionState {
        canonicalize_decision_state(state, &self.canonical_sites)
    }

    /// Derives one exact configured-depth witness through the analysis-run residual cache.
    fn decision_witness_at_depth(
        &self,
        start: &PairedDecisionState,
        depth: usize,
    ) -> Option<Vec<DecisionWitness>> {
        let mut frontier = |state: &DecisionState| self.decision_frontier(state);
        canonical_decision_witness_suffix(
            start,
            self.grammar,
            depth,
            &mut self.decision_witnesses.borrow_mut(),
            &mut frontier,
        )
    }

    /// Derives and caches one canonical single-side decision frontier.
    fn decision_frontier(&self, state: &DecisionState) -> Rc<DecisionFrontier> {
        let state = self.canonical_state(state.clone());
        if let Some(frontier) = self.decision_frontiers.borrow().get(&state) {
            return Rc::clone(frontier);
        }
        #[cfg(test)]
        FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.set(
            FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS
                .get()
                .saturating_add(1),
        );
        let mut frontier = decision_frontier(&state, self.grammar, &self.atom_kinds);
        for transition in &mut frontier.transitions {
            transition.next = self.canonical_state(transition.next.clone());
        }
        frontier.transitions.sort_by(|left, right| {
            compare_first_positions(&left.position, &right.position)
                .then_with(|| left.next.cmp(&right.next))
                .then_with(|| right.minimum_stack_depth.cmp(&left.minimum_stack_depth))
        });
        frontier
            .transitions
            .dedup_by(|left, right| left.position == right.position && left.next == right.next);
        let frontier = Rc::new(frontier);
        self.decision_frontiers
            .borrow_mut()
            .insert(state, Rc::clone(&frontier));
        frontier
    }

    /// Creates one fresh mutable language arena over shared immutable grammar facts.
    fn language(&self) -> FirstLanguageArena<'a> {
        FirstLanguageArena::from_analysis(self)
    }
}

/// Compacts reachable proof nodes and remaps every report reference.
fn compact_report_proofs(report: &mut AnalysisReport) {
    let mut roots = Vec::new();
    if let Some(output) = &report.output {
        for positions in &output.first_provenance_by_type {
            for position in positions {
                roots.extend(position.proof_roots());
            }
        }
        for positions in output.extender_provenance_by_type.iter().flatten() {
            for position in positions {
                roots.extend(position.proof_roots());
            }
        }
        for follow in output.follow_by_type.iter().chain(&output.follow_by_site) {
            roots.extend(follow.kinds.proof_roots());
            roots.extend(follow.eof_proof);
        }
    }
    for diagnostic in &report.diagnostics {
        if let Some(conflict) = &diagnostic.conflict {
            roots.extend([conflict.witness.left, conflict.witness.right]);
        }
    }

    let remap = report.proofs.retain_reachable(roots);
    if let Some(output) = &mut report.output {
        for positions in &mut output.first_provenance_by_type {
            for position in positions {
                position.remap_proofs(&remap);
            }
        }
        for positions in output.extender_provenance_by_type.iter_mut().flatten() {
            for position in positions {
                position.remap_proofs(&remap);
            }
        }
        for follow in output
            .follow_by_type
            .iter_mut()
            .chain(&mut output.follow_by_site)
        {
            follow.kinds.remap_proofs(&remap);
            follow.eof_proof = follow.eof_proof.map(|proof| remap.get(proof));
        }
    }
    for diagnostic in &mut report.diagnostics {
        if let Some(conflict) = &mut diagnostic.conflict {
            conflict.witness.left = remap.get(conflict.witness.left);
            conflict.witness.right = remap.get(conflict.witness.right);
        }
    }
}

/// Rejects malformed grammar structure before any fixed-point computation begins.
fn validate(grammar: &GrammarDescription) -> Vec<Diagnostic> {
    // Validation deliberately accumulates independent structural failures before any fixed-point computation begins.
    let mut diagnostics = Vec::new();
    if grammar.max_lookahead == 0 {
        diagnostics.push(Diagnostic::invalid_grammar(
            "maximum lookahead must be at least one",
        ));
    }
    if grammar.kind_count > usize::from(u16::MAX) + 1 {
        diagnostics.push(Diagnostic::invalid_grammar(
            "grammar has more kinds than KindId can represent",
        ));
    }
    if grammar.kinds.len() != grammar.kind_count {
        diagnostics.push(Diagnostic::invalid_grammar(format!(
            "grammar width is {}, but {} kinds were declared",
            grammar.kind_count,
            grammar.kinds.len(),
        )));
    }

    for (index, location) in grammar.locations.iter().enumerate() {
        if location.source.get() >= grammar.source_count {
            diagnostics.push(Diagnostic::invalid_reference(format!(
                "location {index} references missing source {}",
                location.source.get(),
            )));
        }
        if location.range.start > location.range.end {
            diagnostics.push(Diagnostic::invalid_grammar(format!(
                "location {index} has a reversed byte range",
            )));
        }
    }
    for (index, declaration) in grammar.declarations.iter().enumerate() {
        if declaration.location.index() >= grammar.locations.len() {
            diagnostics.push(Diagnostic::invalid_reference(format!(
                "declaration {index} references missing location {}",
                declaration.location.get(),
            )));
        }
    }
    for (index, kind) in grammar.kinds.iter().enumerate() {
        validate_decl(
            kind.declaration,
            grammar,
            format_args!("kind {index}"),
            &mut diagnostics,
        );
    }
    for (index, atom) in grammar.atoms.iter().enumerate() {
        validate_decl(
            atom.declaration,
            grammar,
            format_args!("atom {index}"),
            &mut diagnostics,
        );
        match &atom.expression {
            AtomExpression::Kind(kind) => {
                if kind.index() >= grammar.kinds.len() {
                    diagnostics.push(Diagnostic::invalid_reference(format!(
                        "atom {index} references missing kind {}",
                        kind.get(),
                    )));
                }
            }
            _ => {
                for reference in atom.expression.references() {
                    if reference.index() >= grammar.atoms.len() {
                        diagnostics.push(Diagnostic::invalid_reference(format!(
                            "atom {index} references missing atom {}",
                            reference.get(),
                        )));
                    }
                }
            }
        }
    }
    validate_atom_cycles(grammar, &mut diagnostics);

    for (index, site) in grammar.sites.iter().enumerate() {
        if site.owner.index() >= grammar.types.len() {
            diagnostics.push(Diagnostic::invalid_reference(format!(
                "site {index} references missing owner type {}",
                site.owner.get(),
            )));
        }
        if site.target.index() >= grammar.types.len() {
            diagnostics.push(Diagnostic::invalid_reference(format!(
                "site {index} references missing target type {}",
                site.target.get(),
            )));
        }
        validate_decl(
            site.declaration,
            grammar,
            format_args!("site {index}"),
            &mut diagnostics,
        );
        if site.container.semantics().is_none() {
            let message = format!(
                "site {index} has unsupported container shape {}; expected T, Option<T>, Vec<T>, Vec1<T>, or Option<Vec1<T>> after ignoring Box",
                site.container,
            );
            let mut diagnostic = Diagnostic::invalid_grammar(message.clone());
            if let Some(location) = grammar
                .declarations
                .get(site.declaration.index())
                .map(|declaration| declaration.location)
                .filter(|location| location.index() < grammar.locations.len())
            {
                diagnostic.primary = crate::DiagnosticLabel::located(location, message);
            }
            diagnostics.push(diagnostic);
        }
        site.container.visit_separators(&mut |separator| {
            if separator.index() >= grammar.types.len() {
                diagnostics.push(Diagnostic::invalid_reference(format!(
                    "site {index} references missing separator type {}",
                    separator.get(),
                )));
            }
        });
    }

    for (index, description) in grammar.types.iter().enumerate() {
        validate_decl(
            description.declaration,
            grammar,
            format_args!("type {index}"),
            &mut diagnostics,
        );
        match &description.shape {
            TypeShape::Token {
                atom,
                text_predicate,
            } => {
                if atom.index() >= grammar.atoms.len() {
                    diagnostics.push(Diagnostic::invalid_reference(format!(
                        "type {index} references missing atom {}",
                        atom.get(),
                    )));
                }
                if text_predicate
                    .is_some_and(|predicate| predicate.index() >= grammar.text_predicates.len())
                {
                    diagnostics.push(Diagnostic::invalid_reference(format!(
                        "type {index} references a missing text predicate",
                    )));
                }
            }
            shape => {
                match shape {
                    TypeShape::Enum { alternatives } => {
                        for alternative in alternatives {
                            validate_decl(
                                alternative.declaration,
                                grammar,
                                format_args!("alternative of type {index}"),
                                &mut diagnostics,
                            );
                        }
                    }
                    TypeShape::Pratt { variants } => {
                        for variant in variants {
                            validate_decl(
                                variant.declaration,
                                grammar,
                                format_args!("Pratt variant of type {index}"),
                                &mut diagnostics,
                            );
                        }
                    }
                    _ => {}
                }
                for site in sites_of(shape) {
                    if site.index() >= grammar.sites.len() {
                        diagnostics.push(Diagnostic::invalid_reference(format!(
                            "type {index} references missing site {}",
                            site.get(),
                        )));
                    } else if grammar.sites[site.index()].owner != TypeId::new(index as u32) {
                        diagnostics.push(Diagnostic::invalid_reference(format!(
                            "type {index} references site {} owned by another type",
                            site.get(),
                        )));
                    }
                }
            }
        }
    }

    for entry in &grammar.entry_types {
        if entry.index() >= grammar.types.len() {
            diagnostics.push(Diagnostic::invalid_reference(format!(
                "entry list references missing type {}",
                entry.get(),
            )));
        }
    }
    diagnostics
}

/// Diagnoses a declaration identity that falls outside the normalized grammar table.
fn validate_decl(
    declaration: crate::DeclId,
    grammar: &GrammarDescription,
    owner: impl std::fmt::Display,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if declaration.index() >= grammar.declarations.len() {
        diagnostics.push(Diagnostic::invalid_reference(format!(
            "{owner} references missing declaration {}",
            declaration.get(),
        )));
    }
}

/// Diagnoses atom cycles before nullable and FIRST propagation.
fn validate_atom_cycles(grammar: &GrammarDescription, diagnostics: &mut Vec<Diagnostic>) {
    let adjacency = grammar
        .atoms
        .iter()
        .map(|atom| {
            atom.expression
                .references()
                .into_iter()
                .filter(|reference| reference.index() < grammar.atoms.len())
                .map(AtomId::index)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut cycle_diagnostics = Vec::new();
    for mut component in strongly_connected_indices(&adjacency) {
        component.sort_unstable();
        let recursive = component.len() > 1
            || adjacency[component[0]]
                .iter()
                .any(|target| *target == component[0]);
        if !recursive {
            continue;
        }

        let names = component
            .iter()
            .map(|index| format!("`{}`", grammar.atoms[*index].name))
            .collect::<Vec<_>>();
        let message = if names.len() == 1 {
            format!("atom {} forms a membership cycle", names[0])
        } else {
            format!("atoms {} form a membership cycle", names.join(", "))
        };
        let first = &grammar.atoms[component[0]];
        let mut diagnostic = Diagnostic::invalid_grammar(message.clone());
        diagnostic.primary = grammar
            .declarations
            .get(first.declaration.index())
            .map_or_else(
                || crate::DiagnosticLabel::unlocated(message.clone()),
                |declaration| {
                    crate::DiagnosticLabel::located(declaration.location, message.clone())
                },
            );
        diagnostic.secondary = component[1..]
            .iter()
            .map(|index| {
                let atom = &grammar.atoms[*index];
                let message = format!("`{}` participates in this membership cycle", atom.name);
                grammar
                    .declarations
                    .get(atom.declaration.index())
                    .map_or_else(
                        || crate::DiagnosticLabel::unlocated(message.clone()),
                        |declaration| {
                            crate::DiagnosticLabel::located(declaration.location, message.clone())
                        },
                    )
            })
            .collect();
        cycle_diagnostics.push(diagnostic);
    }
    cycle_diagnostics.sort_by_key(|diagnostic| diagnostic_source_key(diagnostic, grammar));
    diagnostics.extend(cycle_diagnostics);
}

/// Derives the sites of from normalized grammar structure and previously established semantic facts.
fn sites_of(shape: &TypeShape) -> Vec<SiteId> {
    match shape {
        TypeShape::Token { .. } => Vec::new(),
        TypeShape::Alias { site } => vec![*site],
        TypeShape::Sequence { sites } => sites.clone(),
        TypeShape::Enum { alternatives } => alternatives
            .iter()
            .flat_map(|alternative| alternative.sites.iter().copied())
            .collect(),
        TypeShape::Pratt { variants } => variants
            .iter()
            .flat_map(|variant| variant.sites.iter().copied())
            .collect(),
    }
}

/// Expands atom membership into concrete token-kind sets.
fn expand_atoms(grammar: &GrammarDescription) -> Vec<KindSet> {
    fn expand(id: AtomId, grammar: &GrammarDescription, cache: &mut [Option<KindSet>]) -> KindSet {
        if let Some(cached) = &cache[id.index()] {
            return cached.clone();
        }
        let mut result = KindSet::empty(grammar.kind_count);
        match &grammar.atoms[id.index()].expression {
            AtomExpression::Kind(kind) => {
                result.insert(*kind).expect("validated kind ID");
            }
            AtomExpression::Category(members) | AtomExpression::Class(members) => {
                for member in members {
                    result.union_assign(&expand(*member, grammar, cache));
                }
            }
            AtomExpression::Target { base, admits } => {
                result.union_assign(&expand(*base, grammar, cache));
                for admitted in admits {
                    result.union_assign(&expand(*admitted, grammar, cache));
                }
            }
        }
        cache[id.index()] = Some(result.clone());
        result
    }

    let mut cache = vec![None; grammar.atoms.len()];
    (0..grammar.atoms.len())
        .map(|index| expand(AtomId::new(index as u32), grammar, &mut cache))
        .collect()
}

/// Computes nullable and FIRST facts to a fixed point.
fn compute_first(grammar: &GrammarDescription, atom_kinds: &[KindSet]) -> Vec<FirstFacts> {
    let empty = || first_facts(Vec::new(), false, grammar.kind_count);
    let mut facts = vec![empty(); grammar.types.len()];
    loop {
        let previous = facts.clone();
        for (index, description) in grammar.types.iter().enumerate() {
            facts[index] = first_for_shape(&description.shape, grammar, atom_kinds, &previous);
        }
        if facts == previous {
            break;
        }
    }

    facts
}

/// Derives the first for shape from normalized grammar structure and previously established semantic facts.
fn first_for_shape(
    shape: &TypeShape,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
    facts: &[FirstFacts],
) -> FirstFacts {
    match shape {
        TypeShape::Token {
            atom,
            text_predicate,
        } => FirstFacts {
            kinds: atom_kinds[atom.index()].clone(),
            positions: vec![FirstPosition {
                kinds: atom_kinds[atom.index()].clone(),
                text_predicate: *text_predicate,
            }],
            sequences: Vec::new(),
            nullable: false,
        },
        TypeShape::Alias { site } => first_for_sites(&[*site], grammar, facts),
        TypeShape::Sequence { sites } => first_for_sites(sites, grammar, facts),
        TypeShape::Enum { alternatives } => {
            let mut positions = Vec::new();
            let mut nullable = false;
            for alternative in alternatives {
                let alternative = first_for_sites(&alternative.sites, grammar, facts);
                merge_positions(&mut positions, &alternative.positions);
                nullable |= alternative.nullable;
            }
            first_facts(positions, nullable, grammar.kind_count)
        }
        TypeShape::Pratt { variants } => {
            let mut positions = Vec::new();
            for variant in variants {
                if matches!(
                    variant.position,
                    PrattPosition::Atom | PrattPosition::Prefix { .. }
                ) {
                    let variant = first_for_sites(&variant.sites, grammar, facts);
                    merge_positions(&mut positions, &variant.positions);
                }
            }
            first_facts(positions, false, grammar.kind_count)
        }
    }
}

/// Derives the first for sites from normalized grammar structure and previously established semantic facts.
fn first_for_sites(
    sites: &[SiteId],
    grammar: &GrammarDescription,
    facts: &[FirstFacts],
) -> FirstFacts {
    let mut positions = Vec::new();
    let mut nullable = true;
    for id in sites {
        let site = &grammar.sites[id.index()];
        let target = &facts[site.target.index()];
        merge_positions(&mut positions, &target.positions);
        if !site_nullable(site, target) {
            nullable = false;
            break;
        }
    }
    first_facts(positions, nullable, grammar.kind_count)
}

/// Derives the site nullable from normalized grammar structure and previously established semantic facts.
fn site_nullable(site: &crate::ParseSite, target: &FirstFacts) -> bool {
    match container_semantics(site).cardinality {
        ContainerCardinality::One | ContainerCardinality::OneOrMore => target.nullable,
        ContainerCardinality::Optional
        | ContainerCardinality::ZeroOrMore
        | ContainerCardinality::OptionalOneOrMore => true,
    }
}

/// Derives the container semantics from normalized grammar structure and previously established semantic facts.
fn container_semantics(site: &crate::ParseSite) -> ContainerSemantics {
    site.container
        .semantics()
        .expect("container shapes are validated before analysis")
}

/// Unions FIRST positions while retaining canonical token predicates.
fn merge_positions(into: &mut Vec<FirstPosition>, from: &[FirstPosition]) {
    for position in from {
        if let Some(existing) = into
            .iter_mut()
            .find(|existing| existing.text_predicate == position.text_predicate)
        {
            existing.kinds.union_assign(&position.kinds);
        } else {
            into.push(position.clone());
        }
    }
    into.sort_by_key(|position| {
        position
            .text_predicate
            .map_or(0, |id| u64::from(id.get()) + 1)
    });
}

/// Derives the first facts from normalized grammar structure and previously established semantic facts.
fn first_facts(positions: Vec<FirstPosition>, nullable: bool, kind_count: usize) -> FirstFacts {
    let mut kinds = KindSet::empty(kind_count);
    for position in &positions {
        kinds.union_assign(&position.kinds);
    }
    FirstFacts {
        kinds,
        positions,
        sequences: Vec::new(),
        nullable,
    }
}

/// Populates bounded FIRST sequences after left recursion has been rejected.
fn populate_first_sequences(
    grammar: &GrammarDescription,
    analysis: &FirstSequenceAnalysis<'_>,
    semantic: &mut [FirstFacts],
) {
    let mut language = analysis.language();
    for (index, facts) in semantic.iter_mut().enumerate() {
        #[cfg(test)]
        FIRST_LANGUAGE_AGGREGATE_MATERIALIZATIONS.set(
            FIRST_LANGUAGE_AGGREGATE_MATERIALIZATIONS
                .get()
                .saturating_add(1),
        );
        let root = language.build(
            grammar.max_lookahead,
            vec![DecisionState(vec![DecisionItem::Type(TypeId::new(
                index.try_into().expect("validated type width"),
            ))])],
        );
        facts.sequences = language.materialize(root);
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Dense identity of one canonical bounded FIRST-language node.
struct FirstLanguageNodeId(u32);

impl FirstLanguageNodeId {
    /// Returns the arena index for this node.
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense identity of one canonical semantic decision state.
struct FirstLanguageDecisionStateId(u32);

impl FirstLanguageDecisionStateId {
    /// Returns the arena index for this decision state.
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense identity of one canonical consuming token position.
struct FirstLanguagePositionId(u32);

impl FirstLanguagePositionId {
    /// Returns the arena index for this token position.
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Hash-cons key for one grammar-local consuming token position.
struct FirstLanguagePositionKey {
    /// Canonical packed token-kind membership.
    kinds: Vec<u64>,
    /// Optional exact-text refinement authored for the position.
    text_predicate: Option<TextPredicateId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Memoization key for one bounded residual subset.
struct FirstLanguageStateKey {
    /// Remaining token positions available in the bounded prefix.
    remaining: usize,
    /// Canonical residual precision required at this depth.
    residuals: FirstLanguageStateResiduals,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Canonical residual identity at full or final-position precision.
enum FirstLanguageStateResiduals {
    /// Full semantic states required above the recursively projected tail.
    Full(Vec<FirstLanguageDecisionStateId>),
    /// Recursively projected bounded languages sufficient above the specialized tail.
    Projected(Vec<FirstLanguageProjectedComponentId>),
    /// Two-position projected languages sufficient when two positions remain.
    DepthTwo(Vec<FirstLanguageDepthTwoFrontierId>),
    /// Projected frontier languages sufficient at the final bounded position.
    Cutoff(Vec<FirstLanguageCutoffFrontierId>),
}

impl FirstLanguageStateResiduals {
    #[cfg(test)]
    /// Counts compact residual identities retained by this state key.
    fn len(&self) -> usize {
        match self {
            Self::Full(residuals) => residuals.len(),
            Self::Projected(components) => components.len(),
            Self::DepthTwo(frontiers) => frontiers.len(),
            Self::Cutoff(frontiers) => frontiers.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// One consuming transition whose semantic residual is already interned.
struct FirstLanguageTransition {
    /// Interned token position consumed by this transition.
    position: FirstLanguagePositionId,
    /// Canonical successor decision-state identity.
    next: FirstLanguageDecisionStateId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One normalized consuming frontier cached by interned decision-state identity.
struct FirstLanguageFrontier {
    /// Whether the decision state can terminate without consuming another token.
    can_end: bool,
    /// Canonical consuming transitions whose successor states are already interned.
    transitions: Vec<FirstLanguageTransition>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One cutoff transition projected to the only observable successor facts.
struct FirstLanguageCutoffTransition {
    /// Interned token position consumed by this transition.
    position: FirstLanguagePositionId,
    /// Exact-end and continuation facts after the position is consumed.
    summary: u8,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One frontier projected at the final bounded lookahead position.
struct FirstLanguageCutoffFrontier {
    /// Whether the decision state can terminate without consuming another token.
    can_end: bool,
    /// Canonical consuming transitions with factored successor summaries.
    transitions: Vec<FirstLanguageCutoffTransition>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense identity of one canonical projected cutoff frontier.
struct FirstLanguageCutoffFrontierId(u32);

impl FirstLanguageCutoffFrontierId {
    /// Returns the arena index for this projected frontier.
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One two-position transition projected to its canonical cutoff suffix language.
struct FirstLanguageDepthTwoTransition {
    /// Interned token position consumed by this transition.
    position: FirstLanguagePositionId,
    /// Canonical one-position language after this position is consumed.
    next: FirstLanguageCutoffFrontierId,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One frontier projected to exactly two observable token positions.
struct FirstLanguageDepthTwoFrontier {
    /// Whether the decision state can terminate without consuming another token.
    can_end: bool,
    /// Canonical transitions with projected one-position suffixes.
    transitions: Vec<FirstLanguageDepthTwoTransition>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense identity of one canonical projected two-position frontier.
struct FirstLanguageDepthTwoFrontierId(u32);

impl FirstLanguageDepthTwoFrontierId {
    /// Returns the arena index for this projected frontier.
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One recursive bounded-language transition projected to its canonical suffix component.
struct FirstLanguageProjectedTransition {
    /// Interned token position consumed by this transition.
    position: FirstLanguagePositionId,
    /// Canonical bounded suffix component after this position is consumed.
    next: FirstLanguageProjectedComponentId,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// One exact bounded-language component projected from a single decision state.
struct FirstLanguageProjectedComponent {
    /// Whether the component can end exactly at this prefix.
    exact_end: bool,
    /// Whether the component can continue beyond the bounded prefix.
    cutoff: bool,
    /// Canonical consuming transitions to shorter projected components.
    transitions: Vec<FirstLanguageProjectedTransition>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Dense identity of one canonical recursively projected bounded-language component.
struct FirstLanguageProjectedComponentId(u32);

impl FirstLanguageProjectedComponentId {
    /// Returns the arena index for this projected component.
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One existing frontier cache entry at either its projected or full precision.
enum FirstLanguageCachedFrontier {
    /// Cutoff-only projection that retains no successor decision states.
    Cutoff(FirstLanguageCutoffFrontierId),
    /// Full within-bound frontier with semantic successor identities.
    Full(FirstLanguageFrontier),
}

/// Bit recording that a bounded FIRST residual can end at the cutoff.
const FIRST_LANGUAGE_CUTOFF_END: u8 = 1;
/// Bit recording that a bounded FIRST residual can consume beyond the cutoff.
const FIRST_LANGUAGE_CUTOFF_CONTINUES: u8 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
/// One deterministic edge in the bounded symbolic FIRST language.
struct FirstLanguageCell {
    /// Pairwise-disjoint token kinds belonging to this cell.
    kinds: KindSet,
    /// Pairwise-disjoint text partition within those token kinds.
    text: TextPartition,
    /// Canonical successor language.
    next: FirstLanguageNodeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One authored-predicate cover edge used only for public sequence projection.
struct FirstLanguageCover {
    /// Authored position retained for diagnostics and semantic projections.
    position: FirstPosition,
    /// Canonical successor language represented by this cover edge.
    next: FirstLanguageNodeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One canonical bounded FIRST-language node.
struct FirstLanguageNode {
    /// Whether a production can end exactly at this prefix.
    exact_end: bool,
    /// Whether a production continues beyond the configured prefix depth.
    cutoff: bool,
    /// Exact deterministic symbolic language partition.
    cells: Vec<FirstLanguageCell>,
    /// Compact authored-predicate cover used to publish legacy FIRST facts.
    covers: Vec<FirstLanguageCover>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Structural hash-cons key for one bounded FIRST-language node.
struct FirstLanguageNodeKey {
    /// Whether a production can end exactly at this prefix.
    exact_end: bool,
    /// Whether a production continues beyond the configured prefix depth.
    cutoff: bool,
    /// Canonical deterministic cells encoded without width-bearing set values.
    cells: Vec<(Vec<u64>, TextPartition, FirstLanguageNodeId)>,
    /// Canonical authored cover encoded without width-bearing set values.
    covers: Vec<(Vec<u64>, Option<TextPredicateId>, FirstLanguageNodeId)>,
}

impl From<&FirstLanguageNode> for FirstLanguageNodeKey {
    /// Derives the structural interning key for a completed node.
    fn from(node: &FirstLanguageNode) -> Self {
        Self {
            exact_end: node.exact_end,
            cutoff: node.cutoff,
            cells: node
                .cells
                .iter()
                .map(|edge| (edge.kinds.words().to_vec(), edge.text.clone(), edge.next))
                .collect(),
            covers: node
                .covers
                .iter()
                .map(|edge| {
                    (
                        edge.position.kinds.words().to_vec(),
                        edge.position.text_predicate,
                        edge.next,
                    )
                })
                .collect(),
        }
    }
}

/// Hash-consed bounded residual language used while computing FIRST-k.
struct FirstLanguageArena<'a> {
    /// Normalized grammar whose residuals are explored.
    grammar: &'a GrammarDescription,
    /// Expanded token kinds indexed by atom identity.
    atom_kinds: Rc<[KindSet]>,
    /// Fixed-point end/continuation facts used only at the bounded cutoff.
    decision_summary_facts: Rc<DecisionSummaryFacts>,
    /// Fixed-point one-position languages used by final-depth projection.
    decision_cutoff_facts: Rc<DecisionCutoffFacts>,
    /// Canonical grammar sites grouped by their decision-language semantics.
    canonical_sites: Rc<[SiteId]>,
    /// Whether this arena materializes deterministic cells in addition to authored covers.
    build_deterministic_cells: bool,
    /// Canonical semantic decision states indexed by dense identity.
    decision_states: Vec<DecisionState>,
    /// Dense identities interned by semantic decision-state value.
    decision_state_ids: HashMap<DecisionState, FirstLanguageDecisionStateId>,
    /// Canonical consuming token positions indexed by dense identity.
    positions: Vec<FirstPosition>,
    /// Dense position identities interned by grammar-local semantic value.
    position_ids: HashMap<FirstLanguagePositionKey, FirstLanguagePositionId>,
    /// Normalized consuming frontiers indexed by interned decision-state identity.
    decision_frontiers: Vec<Option<FirstLanguageCachedFrontier>>,
    /// Canonical projected cutoff frontiers indexed by dense identity.
    cutoff_frontiers: Vec<FirstLanguageCutoffFrontier>,
    /// Dense identity candidates indexed by compact semantic frontier hash.
    cutoff_frontier_ids: HashMap<u64, Vec<FirstLanguageCutoffFrontierId>>,
    /// Canonical projected two-position frontiers indexed by dense identity.
    depth_two_frontiers: Vec<FirstLanguageDepthTwoFrontier>,
    /// Dense two-position identities indexed by compact semantic frontier hash.
    depth_two_frontier_ids: HashMap<u64, Vec<FirstLanguageDepthTwoFrontierId>>,
    /// Two-position projection memoized by semantic decision-state identity.
    depth_two_by_decision: Vec<Option<FirstLanguageDepthTwoFrontierId>>,
    /// Canonical recursively projected bounded-language components.
    projected_components: Vec<FirstLanguageProjectedComponent>,
    /// Dense projected component identities indexed by compact semantic hash.
    projected_component_ids: HashMap<u64, Vec<FirstLanguageProjectedComponentId>>,
    /// Bounded component projection memoized by depth and decision-state identity.
    projected_by_decision:
        HashMap<(usize, FirstLanguageDecisionStateId), FirstLanguageProjectedComponentId>,
    /// Canonical completed nodes indexed by dense identity.
    nodes: Vec<FirstLanguageNode>,
    /// Residual subset memoization keyed by remaining lookahead.
    states: BTreeMap<FirstLanguageStateKey, FirstLanguageNodeId>,
    /// Exact-end and continuation summaries indexed by decision-state identity.
    cutoff_summaries: Vec<Option<u8>>,
    /// Structural node interner that merges semantically identical suffixes.
    interned: BTreeMap<FirstLanguageNodeKey, FirstLanguageNodeId>,
    /// Exact commutative unions of deterministic language nodes.
    unioned: BTreeMap<(FirstLanguageNodeId, FirstLanguageNodeId), FirstLanguageNodeId>,
    /// Lazily projected public sequences indexed by canonical node.
    materialized: Vec<Option<Vec<FirstSequence>>>,
}

impl<'a> FirstLanguageArena<'a> {
    #[cfg(test)]
    /// Creates an empty symbolic FIRST arena for one grammar analysis.
    fn new(grammar: &'a GrammarDescription, atom_kinds: &'a [KindSet]) -> Self {
        let mut language = FirstSequenceAnalysis::new(grammar, atom_kinds).language();
        language.build_deterministic_cells = true;
        language
    }

    /// Creates a root-local arena from one analysis-run immutable support context.
    fn from_analysis(analysis: &FirstSequenceAnalysis<'a>) -> Self {
        Self {
            grammar: analysis.grammar,
            atom_kinds: Rc::clone(&analysis.atom_kinds),
            decision_summary_facts: Rc::clone(&analysis.decision_summary_facts),
            decision_cutoff_facts: Rc::clone(&analysis.decision_cutoff_facts),
            canonical_sites: Rc::clone(&analysis.canonical_sites),
            build_deterministic_cells: false,
            decision_states: Vec::new(),
            decision_state_ids: HashMap::new(),
            positions: Vec::new(),
            position_ids: HashMap::new(),
            decision_frontiers: Vec::new(),
            cutoff_frontiers: Vec::new(),
            cutoff_frontier_ids: HashMap::new(),
            depth_two_frontiers: Vec::new(),
            depth_two_frontier_ids: HashMap::new(),
            depth_two_by_decision: Vec::new(),
            projected_components: Vec::new(),
            projected_component_ids: HashMap::new(),
            projected_by_decision: HashMap::new(),
            nodes: Vec::new(),
            states: BTreeMap::new(),
            cutoff_summaries: Vec::new(),
            interned: BTreeMap::new(),
            unioned: BTreeMap::new(),
            materialized: Vec::new(),
        }
    }

    #[cfg(test)]
    /// Counts distinct semantic positions retained by cached decision frontiers.
    fn cached_frontier_position_count(&self) -> usize {
        self.positions.len()
    }

    #[cfg(test)]
    /// Counts token-kind words retained directly by cached decision frontier positions.
    fn cached_frontier_kind_words(&self) -> usize {
        self.positions
            .iter()
            .map(|position| position.kinds.words().len())
            .sum()
    }

    #[cfg(test)]
    /// Counts compact consuming arcs retained by cached decision frontiers.
    fn cached_frontier_arc_count(&self) -> usize {
        self.decision_frontiers
            .iter()
            .flatten()
            .filter_map(|frontier| match frontier {
                FirstLanguageCachedFrontier::Cutoff(_) => None,
                FirstLanguageCachedFrontier::Full(frontier) => Some(frontier.transitions.len()),
            })
            .sum::<usize>()
            + self
                .cutoff_frontiers
                .iter()
                .map(|frontier| frontier.transitions.len())
                .sum::<usize>()
    }

    #[cfg(test)]
    /// Counts owned projected transition copies retained by the frontier store and its index.
    fn retained_cutoff_frontier_component_copies(&self) -> usize {
        self.cutoff_frontiers
            .iter()
            .map(|frontier| frontier.transitions.len())
            .sum::<usize>()
    }

    /// Builds or reuses the canonical node for one residual subset and depth.
    fn build(&mut self, remaining: usize, residuals: Vec<DecisionState>) -> FirstLanguageNodeId {
        let residuals = residuals
            .into_iter()
            .map(|residual| self.intern_decision_state(residual))
            .collect();
        self.build_interned(remaining, residuals)
    }

    /// Builds or reuses one node from already-interned residual identities.
    fn build_interned(
        &mut self,
        remaining: usize,
        mut residuals: Vec<FirstLanguageDecisionStateId>,
    ) -> FirstLanguageNodeId {
        residuals.sort();
        residuals.dedup();
        if remaining == 0 {
            return self.build_cutoff(&residuals);
        }
        if !self.build_deterministic_cells {
            return self.build_cover_state(remaining, residuals);
        }
        if remaining == 1 {
            let mut frontiers = residuals
                .into_iter()
                .map(|residual| self.projected_decision_frontier(residual))
                .collect::<Vec<_>>();
            frontiers.sort();
            frontiers.dedup();
            return self.build_cutoff_state(frontiers);
        }
        if remaining == 2 {
            let mut frontiers = residuals
                .into_iter()
                .map(|residual| self.projected_depth_two_frontier(residual))
                .collect::<Vec<_>>();
            frontiers.sort();
            frontiers.dedup();
            return self.build_depth_two_state(frontiers);
        }
        if remaining <= 4 {
            let mut components = residuals
                .into_iter()
                .map(|residual| self.projected_component(remaining, residual))
                .collect::<Vec<_>>();
            components.sort();
            components.dedup();
            return self.build_projected_state(remaining, components);
        }
        let key = FirstLanguageStateKey {
            remaining,
            residuals: FirstLanguageStateResiduals::Full(residuals),
        };
        if let Some(node) = self.states.get(&key) {
            return *node;
        }
        self.record_state_build(remaining);
        let FirstLanguageStateResiduals::Full(residuals) = &key.residuals else {
            unreachable!("outer bounded state retains full residual precision")
        };
        let (exact_end, transitions) = self.frontier(residuals);
        let cells = if self.build_deterministic_cells {
            self.build_cells(&transitions, remaining - 1)
        } else {
            Vec::new()
        };
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells,
            covers: self.build_covers(&transitions, remaining - 1),
        };
        debug_assert!(first_language_cells_are_disjoint(&node.cells, self.grammar));
        let node_id = self.intern_node(node);
        self.states.insert(key, node_id);
        node_id
    }

    /// Builds the authored cover used by public sequence materialization without a duplicate deterministic graph.
    fn build_cover_state(
        &mut self,
        remaining: usize,
        residuals: Vec<FirstLanguageDecisionStateId>,
    ) -> FirstLanguageNodeId {
        debug_assert!(remaining > 0);
        let key = FirstLanguageStateKey {
            remaining,
            residuals: FirstLanguageStateResiduals::Full(residuals),
        };
        if let Some(node) = self.states.get(&key) {
            return *node;
        }
        self.record_state_build(remaining);
        let FirstLanguageStateResiduals::Full(residuals) = &key.residuals else {
            unreachable!("cover-only state retains full residual precision")
        };
        let (exact_end, transitions) = self.frontier(residuals);
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells: Vec::new(),
            covers: self.build_covers(&transitions, remaining - 1),
        };
        let node_id = self.intern_node(node);
        self.states.insert(key, node_id);
        node_id
    }

    /// Builds or reuses one recursive state from canonical projected components.
    fn build_projected_state(
        &mut self,
        remaining: usize,
        mut components: Vec<FirstLanguageProjectedComponentId>,
    ) -> FirstLanguageNodeId {
        components.sort();
        components.dedup();
        if remaining == 0 {
            let mut exact_end = false;
            let mut cutoff = false;
            for component in components {
                let component = &self.projected_components[component.index()];
                exact_end |= component.exact_end;
                cutoff |= component.cutoff;
                debug_assert!(component.transitions.is_empty());
            }
            return self.intern_node(FirstLanguageNode {
                exact_end,
                cutoff,
                cells: Vec::new(),
                covers: Vec::new(),
            });
        }
        let key = FirstLanguageStateKey {
            remaining,
            residuals: FirstLanguageStateResiduals::Projected(components),
        };
        if let Some(node) = self.states.get(&key) {
            return *node;
        }
        self.record_state_build(remaining);
        let FirstLanguageStateResiduals::Projected(components) = &key.residuals else {
            unreachable!("recursive bounded state retains projected components")
        };
        if self.build_deterministic_cells && components.len() == 2 {
            let components = components.clone();
            let mut level = components
                .iter()
                .copied()
                .map(|component| self.build_projected_state(remaining, vec![component]))
                .collect::<Vec<_>>();
            while level.len() > 1 {
                let mut next = Vec::with_capacity(level.len().div_ceil(2));
                for pair in level.chunks(2) {
                    next.push(if let [left, right] = pair {
                        self.union_nodes(*left, *right)
                    } else {
                        pair[0]
                    });
                }
                level = next;
            }
            let node = level[0];
            self.states.insert(key, node);
            return node;
        }
        let FirstLanguageStateResiduals::Projected(components) = &key.residuals else {
            unreachable!("recursive bounded state retains projected components")
        };
        let mut exact_end = false;
        let mut transitions = Vec::new();
        for component in components {
            let component = &self.projected_components[component.index()];
            exact_end |= component.exact_end;
            debug_assert!(!component.cutoff);
            transitions.extend_from_slice(&component.transitions);
        }
        self.sort_dedup_projected_transitions(&mut transitions);
        let cells = if self.build_deterministic_cells {
            self.build_projected_cells(&transitions, remaining - 1)
        } else {
            Vec::new()
        };
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells,
            covers: self.build_projected_covers(&transitions, remaining - 1),
        };
        debug_assert!(first_language_cells_are_disjoint(&node.cells, self.grammar));
        let node_id = self.intern_node(node);
        self.states.insert(key, node_id);
        node_id
    }

    /// Builds or reuses a two-position node from canonical projected frontiers.
    fn build_depth_two_state(
        &mut self,
        frontiers: Vec<FirstLanguageDepthTwoFrontierId>,
    ) -> FirstLanguageNodeId {
        let key = FirstLanguageStateKey {
            remaining: 2,
            residuals: FirstLanguageStateResiduals::DepthTwo(frontiers),
        };
        if let Some(node) = self.states.get(&key) {
            return *node;
        }
        self.record_state_build(2);

        let FirstLanguageStateResiduals::DepthTwo(frontiers) = &key.residuals else {
            unreachable!("two-position state retains projected frontier precision")
        };
        let (exact_end, transitions) = self.depth_two_frontier(frontiers);
        let cells = if self.build_deterministic_cells {
            self.build_depth_two_cells(&transitions)
        } else {
            Vec::new()
        };
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells,
            covers: self.build_depth_two_covers(&transitions),
        };
        debug_assert!(first_language_cells_are_disjoint(&node.cells, self.grammar));
        let node_id = self.intern_node(node);
        self.states.insert(key, node_id);
        node_id
    }

    /// Builds or reuses a final-position node from canonical projected frontiers.
    fn build_cutoff_state(
        &mut self,
        frontiers: Vec<FirstLanguageCutoffFrontierId>,
    ) -> FirstLanguageNodeId {
        let key = FirstLanguageStateKey {
            remaining: 1,
            residuals: FirstLanguageStateResiduals::Cutoff(frontiers),
        };
        if let Some(node) = self.states.get(&key) {
            return *node;
        }
        self.record_state_build(1);

        let FirstLanguageStateResiduals::Cutoff(frontiers) = &key.residuals else {
            unreachable!("final-position state retains projected frontier precision")
        };
        let (exact_end, transitions) = self.cutoff_frontier(frontiers);
        let cells = if self.build_deterministic_cells {
            self.build_cutoff_cells(&transitions)
        } else {
            Vec::new()
        };
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells,
            covers: self.build_cutoff_covers(&transitions),
        };
        debug_assert!(first_language_cells_are_disjoint(&node.cells, self.grammar));
        let node_id = self.intern_node(node);
        self.states.insert(key, node_id);
        node_id
    }

    /// Records one unique bounded-state construction for structural tests.
    fn record_state_build(&mut self, _remaining: usize) {
        #[cfg(test)]
        FIRST_LANGUAGE_STATE_COUNT.set(FIRST_LANGUAGE_STATE_COUNT.get().saturating_add(1));
    }

    /// Interns one semantic decision state and returns its dense arena identity.
    fn intern_decision_state(&mut self, state: DecisionState) -> FirstLanguageDecisionStateId {
        let state = canonicalize_decision_state(state, &self.canonical_sites);
        if let Some(id) = self.decision_state_ids.get(&state) {
            return *id;
        }
        let id = FirstLanguageDecisionStateId(
            self.decision_states
                .len()
                .try_into()
                .expect("bounded FIRST decision-state identity overflow"),
        );
        self.decision_states.push(state.clone());
        self.decision_frontiers.push(None);
        self.depth_two_by_decision.push(None);
        self.cutoff_summaries.push(None);
        self.decision_state_ids.insert(state, id);
        id
    }

    /// Interns one consuming position and returns its compact arena identity.
    fn intern_position(&mut self, position: FirstPosition) -> FirstLanguagePositionId {
        let key = FirstLanguagePositionKey {
            kinds: position.kinds.words().to_vec(),
            text_predicate: position.text_predicate,
        };
        if let Some(id) = self.position_ids.get(&key) {
            return *id;
        }
        let id = FirstLanguagePositionId(
            self.positions
                .len()
                .try_into()
                .expect("bounded FIRST position identity overflow"),
        );
        self.positions.push(position);
        self.position_ids.insert(key, id);
        id
    }

    /// Interns one complete projected cutoff frontier by semantic value.
    fn intern_cutoff_frontier(
        &mut self,
        frontier: FirstLanguageCutoffFrontier,
    ) -> FirstLanguageCutoffFrontierId {
        let mut hasher = DefaultHasher::new();
        frontier.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(id) = self.cutoff_frontier_ids.get(&hash).and_then(|candidates| {
            candidates
                .iter()
                .copied()
                .find(|candidate| self.cutoff_frontiers[candidate.index()] == frontier)
        }) {
            return id;
        }
        let id = FirstLanguageCutoffFrontierId(
            self.cutoff_frontiers
                .len()
                .try_into()
                .expect("bounded FIRST cutoff-frontier identity overflow"),
        );
        self.cutoff_frontiers.push(frontier);
        self.cutoff_frontier_ids.entry(hash).or_default().push(id);
        id
    }

    /// Interns one complete projected two-position frontier by semantic value.
    fn intern_depth_two_frontier(
        &mut self,
        frontier: FirstLanguageDepthTwoFrontier,
    ) -> FirstLanguageDepthTwoFrontierId {
        let mut hasher = DefaultHasher::new();
        frontier.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(id) = self
            .depth_two_frontier_ids
            .get(&hash)
            .and_then(|candidates| {
                candidates
                    .iter()
                    .copied()
                    .find(|candidate| self.depth_two_frontiers[candidate.index()] == frontier)
            })
        {
            return id;
        }
        let id = FirstLanguageDepthTwoFrontierId(
            self.depth_two_frontiers
                .len()
                .try_into()
                .expect("bounded FIRST depth-two frontier identity overflow"),
        );
        self.depth_two_frontiers.push(frontier);
        self.depth_two_frontier_ids
            .entry(hash)
            .or_default()
            .push(id);
        id
    }

    /// Interns one recursively projected bounded-language component by semantic value.
    fn intern_projected_component(
        &mut self,
        component: FirstLanguageProjectedComponent,
    ) -> FirstLanguageProjectedComponentId {
        let mut hasher = DefaultHasher::new();
        component.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(id) = self
            .projected_component_ids
            .get(&hash)
            .and_then(|candidates| {
                candidates
                    .iter()
                    .copied()
                    .find(|candidate| self.projected_components[candidate.index()] == component)
            })
        {
            return id;
        }
        let id = FirstLanguageProjectedComponentId(
            self.projected_components
                .len()
                .try_into()
                .expect("bounded FIRST projected-component identity overflow"),
        );
        self.projected_components.push(component);
        self.projected_component_ids
            .entry(hash)
            .or_default()
            .push(id);
        id
    }

    /// Collapses cutoff subsets through cached individual end/continuation summaries.
    fn build_cutoff(&mut self, residuals: &[FirstLanguageDecisionStateId]) -> FirstLanguageNodeId {
        let mut summary = 0_u8;
        for residual in residuals {
            #[cfg(test)]
            FIRST_LANGUAGE_CUTOFF_RESIDUAL_VISITS.set(
                FIRST_LANGUAGE_CUTOFF_RESIDUAL_VISITS
                    .get()
                    .saturating_add(1),
            );
            summary |= self.cutoff_summary(*residual);
        }
        self.intern_cutoff_node(summary)
    }

    /// Computes or reuses the two-bit cutoff summary for one interned residual.
    fn cutoff_summary(&mut self, residual: FirstLanguageDecisionStateId) -> u8 {
        if let Some(summary) = self.cutoff_summaries[residual.index()] {
            return summary;
        }
        let summary = if let Some(frontier) = &self.decision_frontiers[residual.index()] {
            let (can_end, continues) = match frontier {
                FirstLanguageCachedFrontier::Cutoff(frontier) => {
                    let frontier = &self.cutoff_frontiers[frontier.index()];
                    (frontier.can_end, !frontier.transitions.is_empty())
                }
                FirstLanguageCachedFrontier::Full(frontier) => {
                    (frontier.can_end, !frontier.transitions.is_empty())
                }
            };
            (u8::from(can_end) * FIRST_LANGUAGE_CUTOFF_END)
                | (u8::from(continues) * FIRST_LANGUAGE_CUTOFF_CONTINUES)
        } else {
            #[cfg(test)]
            {
                FIRST_LANGUAGE_FRONTIER_EXPANSIONS
                    .set(FIRST_LANGUAGE_FRONTIER_EXPANSIONS.get().saturating_add(1));
                FIRST_LANGUAGE_CUTOFF_SUMMARY_COMPUTATIONS.set(
                    FIRST_LANGUAGE_CUTOFF_SUMMARY_COMPUTATIONS
                        .get()
                        .saturating_add(1),
                );
            }
            decision_cutoff_summary(
                &self.decision_states[residual.index()],
                &self.decision_summary_facts,
            )
        };
        self.cutoff_summaries[residual.index()] = Some(summary);
        summary
    }

    /// Derives and interns one reachable decision state's normalized frontier once.
    fn ensure_decision_frontier(&mut self, residual: FirstLanguageDecisionStateId) {
        if matches!(
            self.decision_frontiers[residual.index()],
            Some(FirstLanguageCachedFrontier::Full(_))
        ) {
            return;
        }
        #[cfg(test)]
        FIRST_LANGUAGE_FRONTIER_EXPANSIONS
            .set(FIRST_LANGUAGE_FRONTIER_EXPANSIONS.get().saturating_add(1));
        #[cfg(test)]
        FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.set(
            FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS
                .get()
                .saturating_add(1),
        );
        let frontier = decision_frontier(
            &self.decision_states[residual.index()],
            self.grammar,
            &self.atom_kinds,
        );
        let can_end = frontier.can_end;
        let mut transitions = Vec::with_capacity(frontier.transitions.len());
        for transition in frontier.transitions {
            let next = self.intern_decision_state(transition.next);
            let position = self.intern_position(transition.position);
            transitions.push(FirstLanguageTransition { position, next });
        }
        self.decision_frontiers[residual.index()] =
            Some(FirstLanguageCachedFrontier::Full(FirstLanguageFrontier {
                can_end,
                transitions,
            }));
    }

    /// Projects or derives one decision frontier at cutoff precision without interning successors.
    fn projected_decision_frontier(
        &mut self,
        residual: FirstLanguageDecisionStateId,
    ) -> FirstLanguageCutoffFrontierId {
        match self.decision_frontiers[residual.index()].clone() {
            Some(FirstLanguageCachedFrontier::Cutoff(frontier)) => return frontier,
            Some(FirstLanguageCachedFrontier::Full(frontier)) => {
                let transitions = self.project_full_cutoff_transitions(&frontier.transitions);
                return self.intern_cutoff_frontier(FirstLanguageCutoffFrontier {
                    can_end: frontier.can_end,
                    transitions,
                });
            }
            None => {}
        }

        #[cfg(test)]
        FIRST_LANGUAGE_FRONTIER_EXPANSIONS
            .set(FIRST_LANGUAGE_FRONTIER_EXPANSIONS.get().saturating_add(1));
        let state = &self.decision_states[residual.index()];
        let (can_end, projected) = if self.decision_cutoff_facts.supports_state(state) {
            let frontier = self
                .decision_cutoff_facts
                .for_state(state, &self.decision_summary_facts);
            (
                frontier.can_end,
                frontier
                    .transitions
                    .into_iter()
                    .map(|transition| {
                        (
                            FirstPosition {
                                kinds: self.atom_kinds[transition.atom.index()].clone(),
                                text_predicate: transition.text_predicate,
                            },
                            transition.summary,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            let frontier = decision_frontier(state, self.grammar, &self.atom_kinds);
            (
                frontier.can_end,
                frontier
                    .transitions
                    .into_iter()
                    .map(|transition| {
                        let summary =
                            decision_cutoff_summary(&transition.next, &self.decision_summary_facts);
                        (transition.position, summary)
                    })
                    .collect(),
            )
        };
        let mut transitions = Vec::with_capacity(projected.len());
        for (position, summary) in projected {
            #[cfg(test)]
            FIRST_LANGUAGE_CUTOFF_SUMMARY_READS
                .set(FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.get().saturating_add(1));
            let position = self.intern_position(position);
            transitions.push(FirstLanguageCutoffTransition { position, summary });
        }
        self.sort_dedup_cutoff_transitions(&mut transitions);
        let frontier = FirstLanguageCutoffFrontier {
            can_end,
            transitions,
        };
        let frontier = self.intern_cutoff_frontier(frontier);
        self.decision_frontiers[residual.index()] =
            Some(FirstLanguageCachedFrontier::Cutoff(frontier));
        frontier
    }

    /// Projects one decision state to its exact two-position bounded frontier.
    fn projected_depth_two_frontier(
        &mut self,
        residual: FirstLanguageDecisionStateId,
    ) -> FirstLanguageDepthTwoFrontierId {
        if let Some(frontier) = self.depth_two_by_decision[residual.index()] {
            return frontier;
        }
        self.ensure_decision_frontier(residual);
        let frontier = match self.decision_frontiers[residual.index()]
            .as_ref()
            .expect("decision frontier is cached after derivation")
        {
            FirstLanguageCachedFrontier::Full(frontier) => frontier.clone(),
            FirstLanguageCachedFrontier::Cutoff(_) => {
                unreachable!("two-position projection upgrades a cutoff frontier")
            }
        };
        let mut transitions = Vec::with_capacity(frontier.transitions.len());
        for transition in frontier.transitions {
            transitions.push(FirstLanguageDepthTwoTransition {
                position: transition.position,
                next: self.projected_decision_frontier(transition.next),
            });
        }
        self.sort_dedup_depth_two_transitions(&mut transitions);
        let frontier = self.intern_depth_two_frontier(FirstLanguageDepthTwoFrontier {
            can_end: frontier.can_end,
            transitions,
        });
        self.depth_two_by_decision[residual.index()] = Some(frontier);
        frontier
    }

    /// Canonicalizes two-position transitions by semantic position and suffix language.
    fn sort_dedup_depth_two_transitions(
        &self,
        transitions: &mut Vec<FirstLanguageDepthTwoTransition>,
    ) {
        transitions.sort_by(|left, right| {
            compare_first_positions(
                &self.positions[left.position.index()],
                &self.positions[right.position.index()],
            )
            .then_with(|| left.next.cmp(&right.next))
        });
        transitions.dedup();
    }

    /// Unions projected two-position frontiers before deterministic partitioning.
    fn depth_two_frontier(
        &self,
        frontier_ids: &[FirstLanguageDepthTwoFrontierId],
    ) -> (bool, Vec<FirstLanguageDepthTwoTransition>) {
        let mut exact_end = false;
        let mut transitions = Vec::new();
        for frontier in frontier_ids {
            let frontier = &self.depth_two_frontiers[frontier.index()];
            exact_end |= frontier.can_end;
            transitions.extend_from_slice(&frontier.transitions);
        }
        self.sort_dedup_depth_two_transitions(&mut transitions);
        (exact_end, transitions)
    }

    /// Projects one decision state to an exact recursively factored bounded language.
    fn projected_component(
        &mut self,
        remaining: usize,
        residual: FirstLanguageDecisionStateId,
    ) -> FirstLanguageProjectedComponentId {
        if let Some(component) = self
            .projected_by_decision
            .get(&(remaining, residual))
            .copied()
        {
            return component;
        }
        #[cfg(test)]
        FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.set(
            FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS
                .get()
                .saturating_add(1),
        );
        let component = if remaining == 0 {
            let summary = self.cutoff_summary(residual);
            FirstLanguageProjectedComponent {
                exact_end: summary & FIRST_LANGUAGE_CUTOFF_END != 0,
                cutoff: summary & FIRST_LANGUAGE_CUTOFF_CONTINUES != 0,
                transitions: Vec::new(),
            }
        } else if remaining == 1 {
            let frontier = self.projected_decision_frontier(residual);
            let frontier = self.cutoff_frontiers[frontier.index()].clone();
            let mut transitions = Vec::with_capacity(frontier.transitions.len());
            for transition in frontier.transitions {
                let next = self.intern_projected_component(FirstLanguageProjectedComponent {
                    exact_end: transition.summary & FIRST_LANGUAGE_CUTOFF_END != 0,
                    cutoff: transition.summary & FIRST_LANGUAGE_CUTOFF_CONTINUES != 0,
                    transitions: Vec::new(),
                });
                transitions.push(FirstLanguageProjectedTransition {
                    position: transition.position,
                    next,
                });
            }
            self.sort_dedup_projected_transitions(&mut transitions);
            FirstLanguageProjectedComponent {
                exact_end: frontier.can_end,
                cutoff: false,
                transitions,
            }
        } else {
            self.ensure_decision_frontier(residual);
            let frontier = match self.decision_frontiers[residual.index()]
                .as_ref()
                .expect("decision frontier is cached after derivation")
            {
                FirstLanguageCachedFrontier::Full(frontier) => frontier.clone(),
                FirstLanguageCachedFrontier::Cutoff(_) => {
                    unreachable!("recursive projection upgrades a cutoff frontier")
                }
            };
            let mut transitions = Vec::with_capacity(frontier.transitions.len());
            for transition in frontier.transitions {
                transitions.push(FirstLanguageProjectedTransition {
                    position: transition.position,
                    next: self.projected_component(remaining - 1, transition.next),
                });
            }
            self.sort_dedup_projected_transitions(&mut transitions);
            FirstLanguageProjectedComponent {
                exact_end: frontier.can_end,
                cutoff: false,
                transitions,
            }
        };
        let component = self.intern_projected_component(component);
        self.projected_by_decision
            .insert((remaining, residual), component);
        component
    }

    /// Canonicalizes recursive transitions by semantic position and suffix component.
    fn sort_dedup_projected_transitions(
        &self,
        transitions: &mut Vec<FirstLanguageProjectedTransition>,
    ) {
        transitions.sort_by(|left, right| {
            compare_first_positions(
                &self.positions[left.position.index()],
                &self.positions[right.position.index()],
            )
            .then_with(|| left.next.cmp(&right.next))
        });
        transitions.dedup();
    }

    /// Projects cached full successor identities to their observable cutoff facts.
    fn project_full_cutoff_transitions(
        &mut self,
        transitions: &[FirstLanguageTransition],
    ) -> Vec<FirstLanguageCutoffTransition> {
        let mut projected = Vec::with_capacity(transitions.len());
        for transition in transitions {
            #[cfg(test)]
            FIRST_LANGUAGE_CUTOFF_SUMMARY_READS
                .set(FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.get().saturating_add(1));
            projected.push(FirstLanguageCutoffTransition {
                position: transition.position,
                summary: self.cutoff_summary(transition.next),
            });
        }
        self.sort_dedup_cutoff_transitions(&mut projected);
        projected
    }

    /// Canonicalizes projected transitions by semantic position and two-bit successor facts.
    fn sort_dedup_cutoff_transitions(&self, transitions: &mut Vec<FirstLanguageCutoffTransition>) {
        transitions.sort_by(|left, right| {
            compare_first_positions(
                &self.positions[left.position.index()],
                &self.positions[right.position.index()],
            )
            .then_with(|| left.summary.cmp(&right.summary))
        });
        transitions.dedup();
    }

    /// Unions projected decision frontiers without retaining unobservable successor identities.
    fn cutoff_frontier(
        &mut self,
        frontier_ids: &[FirstLanguageCutoffFrontierId],
    ) -> (bool, Vec<FirstLanguageCutoffTransition>) {
        let mut exact_end = false;
        let mut transitions = Vec::new();
        for frontier in frontier_ids {
            let frontier = &self.cutoff_frontiers[frontier.index()];
            exact_end |= frontier.can_end;
            #[cfg(test)]
            FIRST_LANGUAGE_CUTOFF_COMPONENT_MEMBERSHIPS.set(
                FIRST_LANGUAGE_CUTOFF_COMPONENT_MEMBERSHIPS
                    .get()
                    .saturating_add(frontier.transitions.len()),
            );
            transitions.extend_from_slice(&frontier.transitions);
        }
        self.sort_dedup_cutoff_transitions(&mut transitions);
        #[cfg(test)]
        FIRST_LANGUAGE_TRANSITION_OPERATIONS.set(
            FIRST_LANGUAGE_TRANSITION_OPERATIONS
                .get()
                .saturating_add(transitions.len()),
        );
        (exact_end, transitions)
    }

    /// Interns one of the four structural cutoff outcomes.
    fn intern_cutoff_node(&mut self, summary: u8) -> FirstLanguageNodeId {
        self.intern_node(FirstLanguageNode {
            exact_end: summary & FIRST_LANGUAGE_CUTOFF_END != 0,
            cutoff: summary & FIRST_LANGUAGE_CUTOFF_CONTINUES != 0,
            cells: Vec::new(),
            covers: Vec::new(),
        })
    }

    /// Computes the consuming frontier for a canonical residual subset.
    fn frontier(
        &mut self,
        residuals: &[FirstLanguageDecisionStateId],
    ) -> (bool, Vec<FirstLanguageTransition>) {
        let mut exact_end = false;
        let mut transitions = Vec::new();
        for residual in residuals {
            self.ensure_decision_frontier(*residual);
            let frontier = self.decision_frontiers[residual.index()]
                .as_ref()
                .expect("decision frontier is cached after derivation");
            let FirstLanguageCachedFrontier::Full(frontier) = frontier else {
                unreachable!("full frontier request upgrades a cutoff projection")
            };
            exact_end |= frontier.can_end;
            transitions.extend(frontier.transitions.iter().copied());
        }
        transitions.sort_by(|left, right| {
            compare_first_positions(
                &self.positions[left.position.index()],
                &self.positions[right.position.index()],
            )
            .then_with(|| left.next.cmp(&right.next))
        });
        transitions
            .dedup_by(|left, right| left.position == right.position && left.next == right.next);
        #[cfg(test)]
        FIRST_LANGUAGE_TRANSITION_OPERATIONS.set(
            FIRST_LANGUAGE_TRANSITION_OPERATIONS
                .get()
                .saturating_add(transitions.len()),
        );
        (exact_end, transitions)
    }

    /// Builds deterministic disjoint token/text cells and coalesces equal suffixes.
    fn build_cells(
        &mut self,
        transitions: &[FirstLanguageTransition],
        remaining: usize,
    ) -> Vec<FirstLanguageCell> {
        debug_assert!(remaining > 0);
        let mut coalesced = BTreeMap::<(TextPartition, FirstLanguageNodeId), KindSet>::new();
        for (text, residuals, kinds) in partition_by_kind_residual(
            transitions,
            self.grammar,
            |candidate| self.positions[candidate.position.index()].text_predicate,
            |candidate| &self.positions[candidate.position.index()].kinds,
            |candidate| candidate.next,
        ) {
            let next = self.build_interned(remaining, residuals);
            coalesced
                .entry((text, next))
                .and_modify(|merged| merged.union_assign(&kinds))
                .or_insert(kinds);
        }
        coalesced
            .into_iter()
            .map(|((text, next), kinds)| FirstLanguageCell { kinds, text, next })
            .collect()
    }

    /// Builds deterministic cells from exact projected two-position transitions.
    fn build_depth_two_cells(
        &mut self,
        transitions: &[FirstLanguageDepthTwoTransition],
    ) -> Vec<FirstLanguageCell> {
        let mut coalesced = BTreeMap::<(TextPartition, FirstLanguageNodeId), KindSet>::new();
        for (text, frontiers, kinds) in partition_by_kind_residual(
            transitions,
            self.grammar,
            |candidate| self.positions[candidate.position.index()].text_predicate,
            |candidate| &self.positions[candidate.position.index()].kinds,
            |candidate| candidate.next,
        ) {
            let next = self.build_cutoff_state(frontiers);
            coalesced
                .entry((text, next))
                .and_modify(|merged| merged.union_assign(&kinds))
                .or_insert(kinds);
        }
        coalesced
            .into_iter()
            .map(|((text, next), kinds)| FirstLanguageCell { kinds, text, next })
            .collect()
    }

    /// Builds deterministic cells from recursively projected transitions.
    fn build_projected_cells(
        &mut self,
        transitions: &[FirstLanguageProjectedTransition],
        remaining: usize,
    ) -> Vec<FirstLanguageCell> {
        #[cfg(test)]
        FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.set(
            FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS
                .get()
                .saturating_add(transitions.len()),
        );

        let mut coalesced = BTreeMap::<(TextPartition, FirstLanguageNodeId), KindSet>::new();
        for (text, components, kinds) in partition_by_kind_residual(
            transitions,
            self.grammar,
            |candidate| self.positions[candidate.position.index()].text_predicate,
            |candidate| &self.positions[candidate.position.index()].kinds,
            |candidate| candidate.next,
        ) {
            let next = self.build_projected_state(remaining, components);
            coalesced
                .entry((text, next))
                .and_modify(|merged| merged.union_assign(&kinds))
                .or_insert(kinds);
        }
        coalesced
            .into_iter()
            .map(|((text, next), kinds)| FirstLanguageCell { kinds, text, next })
            .collect()
    }

    /// Builds cutoff cells by folding two-bit summaries during text partitioning.
    fn build_cutoff_cells(
        &mut self,
        transitions: &[FirstLanguageCutoffTransition],
    ) -> Vec<FirstLanguageCell> {
        let mut factored = BTreeMap::<(Option<TextPredicateId>, u8), KindSet>::new();
        for transition in transitions {
            #[cfg(test)]
            FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS.set(
                FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS
                    .get()
                    .saturating_add(1),
            );
            let position = &self.positions[transition.position.index()];
            factored
                .entry((position.text_predicate, transition.summary))
                .and_modify(|kinds| kinds.union_assign(&position.kinds))
                .or_insert_with(|| position.kinds.clone());
        }
        let factored = factored
            .into_iter()
            .map(|((text_predicate, summary), kinds)| (text_predicate, summary, kinds))
            .collect::<Vec<_>>();
        partition_by_kind_summary(
            &factored,
            self.grammar,
            |candidate| candidate.0,
            |candidate| &candidate.2,
            |candidate| candidate.1,
        )
        .into_iter()
        .map(|(text, summary, kinds)| FirstLanguageCell {
            kinds,
            text,
            next: self.intern_cutoff_node(summary),
        })
        .collect()
    }

    /// Builds a compact exact cover using only authored predicate identities.
    fn build_covers(
        &mut self,
        transitions: &[FirstLanguageTransition],
        remaining: usize,
    ) -> Vec<FirstLanguageCover> {
        let mut coalesced =
            BTreeMap::<(Option<TextPredicateId>, FirstLanguageNodeId), KindSet>::new();
        for transition in transitions {
            let position = self.positions[transition.position.index()].clone();
            let next = self.build_interned(remaining, vec![transition.next]);
            coalesced
                .entry((position.text_predicate, next))
                .and_modify(|merged| merged.union_assign(&position.kinds))
                .or_insert(position.kinds);
        }
        coalesced
            .into_iter()
            .map(|((text_predicate, next), kinds)| FirstLanguageCover {
                position: FirstPosition {
                    kinds,
                    text_predicate,
                },
                next,
            })
            .collect()
    }

    /// Builds the authored-predicate cover for projected two-position transitions.
    fn build_depth_two_covers(
        &mut self,
        transitions: &[FirstLanguageDepthTwoTransition],
    ) -> Vec<FirstLanguageCover> {
        let mut coalesced =
            BTreeMap::<(Option<TextPredicateId>, FirstLanguageNodeId), KindSet>::new();
        for transition in transitions {
            let position = self.positions[transition.position.index()].clone();
            let next = self.build_cutoff_state(vec![transition.next]);
            coalesced
                .entry((position.text_predicate, next))
                .and_modify(|merged| merged.union_assign(&position.kinds))
                .or_insert(position.kinds);
        }
        coalesced
            .into_iter()
            .map(|((text_predicate, next), kinds)| FirstLanguageCover {
                position: FirstPosition {
                    kinds,
                    text_predicate,
                },
                next,
            })
            .collect()
    }

    /// Builds the authored-predicate cover for recursively projected transitions.
    fn build_projected_covers(
        &mut self,
        transitions: &[FirstLanguageProjectedTransition],
        remaining: usize,
    ) -> Vec<FirstLanguageCover> {
        let mut coalesced =
            BTreeMap::<(Option<TextPredicateId>, FirstLanguageNodeId), KindSet>::new();
        for transition in transitions {
            let position = self.positions[transition.position.index()].clone();
            let next = self.build_projected_state(remaining, vec![transition.next]);
            coalesced
                .entry((position.text_predicate, next))
                .and_modify(|merged| merged.union_assign(&position.kinds))
                .or_insert(position.kinds);
        }
        coalesced
            .into_iter()
            .map(|((text_predicate, next), kinds)| FirstLanguageCover {
                position: FirstPosition {
                    kinds,
                    text_predicate,
                },
                next,
            })
            .collect()
    }

    /// Builds the final-position cover from factored successor summaries.
    fn build_cutoff_covers(
        &mut self,
        transitions: &[FirstLanguageCutoffTransition],
    ) -> Vec<FirstLanguageCover> {
        let mut coalesced =
            BTreeMap::<(Option<TextPredicateId>, FirstLanguageNodeId), KindSet>::new();
        for transition in transitions {
            let position = self.positions[transition.position.index()].clone();
            let next = self.intern_cutoff_node(transition.summary);
            coalesced
                .entry((position.text_predicate, next))
                .and_modify(|merged| merged.union_assign(&position.kinds))
                .or_insert(position.kinds);
        }
        coalesced
            .into_iter()
            .map(|((text_predicate, next), kinds)| FirstLanguageCover {
                position: FirstPosition {
                    kinds,
                    text_predicate,
                },
                next,
            })
            .collect()
    }

    /// Computes or reuses the exact commutative union of two deterministic
    /// bounded-language nodes.
    fn union_nodes(
        &mut self,
        left: FirstLanguageNodeId,
        right: FirstLanguageNodeId,
    ) -> FirstLanguageNodeId {
        if left == right {
            return left;
        }
        let key = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        if let Some(union) = self.unioned.get(&key) {
            return *union;
        }
        #[cfg(test)]
        FIRST_LANGUAGE_NODE_UNION_MISSES
            .set(FIRST_LANGUAGE_NODE_UNION_MISSES.get().saturating_add(1));

        let left = self.nodes[key.0.index()].clone();
        let right = self.nodes[key.1.index()].clone();
        let mut candidates = left.cells.clone();
        candidates.extend(right.cells.iter().cloned());
        let mut cells = Vec::new();
        for (text, residuals, kinds) in overlay_by_kind_residual(
            &candidates,
            |candidate| &candidate.text,
            |candidate| &candidate.kinds,
            |candidate| candidate.next,
        ) {
            let mut residuals = residuals.into_iter();
            let Some(mut next) = residuals.next() else {
                continue;
            };
            for residual in residuals {
                next = self.union_nodes(next, residual);
            }
            cells.push(FirstLanguageCell { kinds, text, next });
        }
        cells.sort_by(|left, right| {
            left.text
                .cmp(&right.text)
                .then_with(|| left.next.cmp(&right.next))
                .then_with(|| left.kinds.words().cmp(right.kinds.words()))
        });

        let mut covers = BTreeMap::<(Option<TextPredicateId>, FirstLanguageNodeId), KindSet>::new();
        for cover in left.covers.into_iter().chain(right.covers) {
            covers
                .entry((cover.position.text_predicate, cover.next))
                .and_modify(|known| known.union_assign(&cover.position.kinds))
                .or_insert(cover.position.kinds);
        }
        let covers = covers
            .into_iter()
            .map(|((text_predicate, next), kinds)| FirstLanguageCover {
                position: FirstPosition {
                    kinds,
                    text_predicate,
                },
                next,
            })
            .collect();
        let node = self.intern_node(FirstLanguageNode {
            exact_end: left.exact_end || right.exact_end,
            cutoff: left.cutoff || right.cutoff,
            cells,
            covers,
        });
        debug_assert!(first_language_cells_are_disjoint(
            &self.nodes[node.index()].cells,
            self.grammar,
        ));
        self.unioned.insert(key, node);
        node
    }

    /// Interns one completed structural node and returns its canonical identity.
    fn intern_node(&mut self, node: FirstLanguageNode) -> FirstLanguageNodeId {
        let key = FirstLanguageNodeKey::from(&node);
        if let Some(node) = self.interned.get(&key) {
            return *node;
        }
        let id = FirstLanguageNodeId(
            self.nodes
                .len()
                .try_into()
                .expect("bounded FIRST node identity overflow"),
        );
        self.nodes.push(node);
        self.materialized.push(None);
        self.interned.insert(key, id);
        id
    }

    /// Lazily projects one symbolic node into the public rectangular sequence cover.
    fn materialize(&mut self, node: FirstLanguageNodeId) -> Vec<FirstSequence> {
        if let Some(sequences) = &self.materialized[node.index()] {
            return sequences.clone();
        }
        let current = self.nodes[node.index()].clone();
        let mut sequences = Vec::new();
        if current.exact_end {
            sequences.push(FirstSequence {
                positions: Vec::new(),
                ends: true,
            });
        }
        if current.cutoff {
            sequences.push(FirstSequence {
                positions: Vec::new(),
                ends: false,
            });
        }
        for cover in current.covers {
            for suffix in self.materialize(cover.next) {
                let mut positions = Vec::with_capacity(suffix.positions.len() + 1);
                positions.push(cover.position.clone());
                positions.extend(suffix.positions);
                sequences.push(FirstSequence {
                    positions,
                    ends: suffix.ends,
                });
            }
        }
        #[cfg(test)]
        FIRST_SEQUENCE_PEAK_ROWS.set(FIRST_SEQUENCE_PEAK_ROWS.get().max(sequences.len()));
        sort_dedup_first_sequences(&mut sequences);
        self.materialized[node.index()] = Some(sequences.clone());
        sequences
    }
}

/// Reports whether every deterministic outgoing symbolic cell is pairwise disjoint.
fn first_language_cells_are_disjoint(
    cells: &[FirstLanguageCell],
    grammar: &GrammarDescription,
) -> bool {
    cells.iter().enumerate().all(|(index, left)| {
        cells[index + 1..].iter().all(|right| {
            left.kinds
                .intersection(&right.kinds)
                .expect("FIRST cells have the grammar width")
                .is_empty()
                || !left.text.overlaps(&right.text, grammar.text_equivalence)
        })
    })
}

/// Sorts dedup first sequences.
fn sort_dedup_first_sequences(sequences: &mut Vec<FirstSequence>) {
    #[cfg(test)]
    FIRST_SEQUENCE_PEAK_ROWS.set(FIRST_SEQUENCE_PEAK_ROWS.get().max(sequences.len()));
    sequences.sort_by(compare_first_sequences);
    sequences.dedup();
    let maximum_length = sequences
        .iter()
        .map(|sequence| sequence.positions.len())
        .max()
        .unwrap_or(0);
    for coordinate in 0..maximum_length {
        let position_rows = intern_first_position_rows(sequences);
        let mut groups = BTreeMap::<RectangleMergeKey, FirstSequence>::new();
        let mut shorter = Vec::new();
        for (sequence, position_ids) in std::mem::take(sequences).into_iter().zip(position_rows) {
            if sequence.positions.len() <= coordinate {
                shorter.push(sequence);
                continue;
            }
            let key = RectangleMergeKey::new(&sequence, &position_ids, coordinate);
            groups
                .entry(key)
                .and_modify(|merged| {
                    merged.positions[coordinate]
                        .kinds
                        .union_assign(&sequence.positions[coordinate].kinds);
                })
                .or_insert(sequence);
        }
        sequences.extend(shorter);
        sequences.extend(groups.into_values());
    }
    sequences.sort_by(compare_first_sequences);
    sequences.dedup();
}

#[cfg(test)]
thread_local! {
    /// Counts paired decision products built while proving static conflicts.
    static CONFLICT_DECISION_PRODUCT_BUILDS: Cell<usize> = const { Cell::new(0) };
    /// Counts paired residual states expanded while building one decision product.
    static DECISION_PRODUCT_STATE_EXPANSIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts parent states inspected while checking one product successor for cycles.
    static DECISION_PRODUCT_ANCESTOR_STATE_VISITS: Cell<usize> = const { Cell::new(0) };
    /// Counts raw left/right consuming-transition pairs considered by a decision product.
    static DECISION_PRODUCT_TRANSITION_PAIRS: Cell<usize> = const { Cell::new(0) };
    /// Counts explicit paired residual states expanded while constructing a bounded witness.
    static DECISION_WITNESS_STATE_EXPANSIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts exact symbolic edge pairs inspected while deriving a frozen bounded witness.
    static BOUNDED_WITNESS_EDGE_PAIR_CHECKS: Cell<usize> = const { Cell::new(0) };
    /// Counts distinct interned position states visited by rectangle grouping.
    static RECTANGLE_POSITION_STATES: Cell<usize> = const { Cell::new(0) };
    /// Tracks the largest eager rectangle row set presented for normalization.
    static FIRST_SEQUENCE_PEAK_ROWS: Cell<usize> = const { Cell::new(0) };
    /// Counts unique bounded residual subset states explored by symbolic FIRST.
    static FIRST_LANGUAGE_STATE_COUNT: Cell<usize> = const { Cell::new(0) };
    /// Counts consuming grammar transitions visited by symbolic FIRST states.
    static FIRST_LANGUAGE_TRANSITION_OPERATIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts raw individual grammar residual frontier expansions.
    static FIRST_LANGUAGE_FRONTIER_EXPANSIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts full semantic decision-state clones during bounded FIRST construction.
    static FIRST_LANGUAGE_DECISION_STATE_CLONES: Cell<usize> = const { Cell::new(0) };
    /// Counts full semantic decision-state ordering comparisons during bounded FIRST construction.
    static FIRST_LANGUAGE_DECISION_STATE_COMPARISONS: Cell<usize> = const { Cell::new(0) };
    /// Counts residual identities rescanned after a cutoff cell has already been partitioned.
    static FIRST_LANGUAGE_CUTOFF_RESIDUAL_VISITS: Cell<usize> = const { Cell::new(0) };
    /// Counts candidate summaries folded while constructing cutoff cells.
    static FIRST_LANGUAGE_CUTOFF_SUMMARY_READS: Cell<usize> = const { Cell::new(0) };
    /// Counts cutoff transition-to-kind candidates expanded before exact text partitioning.
    static FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS: Cell<usize> = const { Cell::new(0) };
    /// Counts projected cutoff components scanned while unioning residual frontiers.
    static FIRST_LANGUAGE_CUTOFF_COMPONENT_MEMBERSHIPS: Cell<usize> = const { Cell::new(0) };
    /// Counts full semantic frontier derivations requested by bounded FIRST construction.
    static FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts summary-only frontier derivations for residuals at the lookahead boundary.
    static FIRST_LANGUAGE_CUTOFF_SUMMARY_COMPUTATIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts normalized consuming transitions materialized by full frontier derivation.
    static FIRST_LANGUAGE_DECISION_TRANSITION_MATERIALIZATIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts full non-consuming closure states visited by decision-frontier traversal.
    static FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS: Cell<usize> = const { Cell::new(0) };
    /// Counts raw decision-state projections into recursively factored tail components.
    static FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS: Cell<usize> = const { Cell::new(0) };
    /// Counts projected transitions expanded into token-kind partition candidates.
    static FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS: Cell<usize> = const { Cell::new(0) };
    /// Counts complete grammar-wide one-position fact constructions.
    static FIRST_LANGUAGE_CUTOFF_FACT_CONSTRUCTIONS: Cell<usize> = const { Cell::new(0) };
    /// Counts distinct commutative deterministic node overlays.
    static FIRST_LANGUAGE_NODE_UNION_MISSES: Cell<usize> = const { Cell::new(0) };
    /// Counts grammar-wide per-type FIRST-k publications after successful analysis.
    static FIRST_LANGUAGE_AGGREGATE_MATERIALIZATIONS: Cell<usize> = const { Cell::new(0) };
}

/// Interns each wide FIRST position once and returns compact row identities.
fn intern_first_position_rows(sequences: &[FirstSequence]) -> Vec<Vec<u32>> {
    let mut positions = sequences
        .iter()
        .flat_map(|sequence| sequence.positions.iter())
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| compare_first_positions(left, right));
    positions.dedup_by(|left, right| left == right);
    #[cfg(test)]
    RECTANGLE_POSITION_STATES.set(RECTANGLE_POSITION_STATES.get() + positions.len());

    sequences
        .iter()
        .map(|sequence| {
            sequence
                .positions
                .iter()
                .map(|position| {
                    positions
                        .binary_search_by(|candidate| compare_first_positions(candidate, position))
                        .expect("the position came from the interned rows")
                        .try_into()
                        .expect("FIRST position identity overflow")
                })
                .collect()
        })
        .collect()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Canonical all-but-one-coordinate key for exact rectangle union.
struct RectangleMergeKey {
    /// Whether the represented sequence can end at this depth.
    ends: bool,
    /// Text predicate retained at the coordinate whose kind set is merged.
    merged_text_predicate: Option<TextPredicateId>,
    /// Interned identities of every non-merged position.
    positions: Vec<u32>,
}

impl RectangleMergeKey {
    /// Indexes one rectangle for a single linear grouping pass.
    fn new(sequence: &FirstSequence, position_ids: &[u32], coordinate: usize) -> Self {
        Self {
            ends: sequence.ends,
            merged_text_predicate: sequence.positions[coordinate].text_predicate,
            positions: position_ids
                .iter()
                .enumerate()
                .filter_map(|(index, position)| (index != coordinate).then_some(*position))
                .collect(),
        }
    }
}

/// Orders FIRST sequences canonically for deterministic decisions.
fn compare_first_sequences(left: &FirstSequence, right: &FirstSequence) -> std::cmp::Ordering {
    left.positions
        .iter()
        .zip(&right.positions)
        .find_map(|(left, right)| {
            let ordering = left
                .kinds
                .kind_count()
                .cmp(&right.kinds.kind_count())
                .then_with(|| left.kinds.words().cmp(right.kinds.words()))
                .then_with(|| left.text_predicate.cmp(&right.text_predicate));
            (!ordering.is_eq()).then_some(ordering)
        })
        .unwrap_or_else(|| {
            left.positions
                .len()
                .cmp(&right.positions.len())
                .then_with(|| right.ends.cmp(&left.ends))
        })
}

#[derive(Clone, Copy, Debug)]
/// Carries the first corner edge consumed while deriving source-independent grammar facts.
struct FirstCornerEdge {
    /// Type from which the left-corner relation originates.
    from: TypeId,
    /// Supplies the to consumed by semantic analysis.
    to: TypeId,
    /// Supplies the declaration consumed by semantic analysis.
    declaration: crate::DeclId,
}

/// Diagnoses left-corner cycles with canonical source witnesses.
fn detect_left_recursion(grammar: &GrammarDescription, first: &[FirstFacts]) -> Vec<Diagnostic> {
    let edges: Vec<Vec<FirstCornerEdge>> = grammar
        .types
        .iter()
        .enumerate()
        .map(|(index, description)| {
            first_corner_edges(
                TypeId::new(index as u32),
                &description.shape,
                grammar,
                first,
            )
        })
        .collect();
    let mut diagnostics = Vec::new();
    for mut component in strongly_connected_components(&edges) {
        component.sort_unstable();
        let recursive = component.len() > 1
            || edges[component[0].index()]
                .iter()
                .any(|edge| edge.to == component[0]);
        if !recursive {
            continue;
        }

        let mut in_component = vec![false; grammar.types.len()];
        for type_id in &component {
            in_component[type_id.index()] = true;
        }
        let mut internal_edges = component
            .iter()
            .flat_map(|type_id| edges[type_id.index()].iter().copied())
            .filter(|edge| in_component[edge.to.index()])
            .collect::<Vec<_>>();
        internal_edges.sort_by_key(|edge| edge_source_key(*edge, grammar));

        let names = component
            .iter()
            .map(|type_id| format!("`{}`", grammar.types[type_id.index()].name))
            .collect::<Vec<_>>();
        let message = if names.len() == 1 {
            format!("type {} forms a left-recursive cycle", names[0])
        } else {
            format!("types {} form a left-recursive cycle", names.join(", "))
        };
        let labels = internal_edges
            .iter()
            .map(|edge| {
                let location = grammar.declarations[edge.declaration.index()].location;
                let from = &grammar.types[edge.from.index()].name;
                let to = &grammar.types[edge.to.index()].name;
                crate::DiagnosticLabel::located(
                    location,
                    format!("`{from}` reaches `{to}` before consuming a token"),
                )
            })
            .collect::<Vec<_>>();
        diagnostics.push(Diagnostic::left_recursion(message, component, labels));
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic_source_key(diagnostic, grammar));
    diagnostics
}

/// Derives the first corner edges from normalized grammar structure and previously established semantic facts.
fn first_corner_edges(
    owner: TypeId,
    shape: &TypeShape,
    grammar: &GrammarDescription,
    first: &[FirstFacts],
) -> Vec<FirstCornerEdge> {
    match shape {
        TypeShape::Token { .. } => Vec::new(),
        TypeShape::Alias { site } => first_corner_edges_for_sites(owner, &[*site], grammar, first),
        TypeShape::Sequence { sites } => first_corner_edges_for_sites(owner, sites, grammar, first),
        TypeShape::Enum { alternatives } => alternatives
            .iter()
            .flat_map(|alternative| {
                first_corner_edges_for_sites(owner, &alternative.sites, grammar, first)
            })
            .collect(),
        TypeShape::Pratt { variants } => variants
            .iter()
            .filter(|variant| {
                matches!(
                    variant.position,
                    PrattPosition::Atom | PrattPosition::Prefix { .. }
                )
            })
            .flat_map(|variant| first_corner_edges_for_sites(owner, &variant.sites, grammar, first))
            .collect(),
    }
}

/// Derives the first corner edges for sites from normalized grammar structure and previously established semantic facts.
fn first_corner_edges_for_sites(
    owner: TypeId,
    sites: &[SiteId],
    grammar: &GrammarDescription,
    first: &[FirstFacts],
) -> Vec<FirstCornerEdge> {
    let mut edges = Vec::new();
    for site_id in sites {
        let site = &grammar.sites[site_id.index()];
        edges.push(FirstCornerEdge {
            from: owner,
            to: site.target,
            declaration: site.declaration,
        });
        if !site_nullable(site, &first[site.target.index()]) {
            break;
        }
    }
    edges
}

/// Derives the edge source key from normalized grammar structure and previously established semantic facts.
fn edge_source_key(
    edge: FirstCornerEdge,
    grammar: &GrammarDescription,
) -> (u32, u32, u32, u32, u32) {
    let location_id = grammar.declarations[edge.declaration.index()].location;
    let location = &grammar.locations[location_id.index()];
    (
        location.source.get(),
        location.range.start,
        location.range.end,
        edge.from.get(),
        edge.to.get(),
    )
}

/// Derives the declaration source key from normalized grammar structure and previously established semantic facts.
fn declaration_source_key(
    declaration: crate::DeclId,
    grammar: &GrammarDescription,
) -> (u32, u32, u32, crate::DeclId) {
    let location_id = grammar.declarations[declaration.index()].location;
    let location = &grammar.locations[location_id.index()];
    (
        location.source.get(),
        location.range.start,
        location.range.end,
        declaration,
    )
}

/// Derives the diagnostic source key from normalized grammar structure and previously established semantic facts.
fn diagnostic_source_key(
    diagnostic: &Diagnostic,
    grammar: &GrammarDescription,
) -> (u32, u32, u32, crate::DiagnosticCode, u16, String) {
    let (source, start, end) = diagnostic
        .primary
        .location
        .and_then(|id| grammar.locations.get(id.index()))
        .map_or((u32::MAX, u32::MAX, u32::MAX), |location| {
            (
                location.source.get(),
                location.range.start,
                location.range.end,
            )
        });
    (
        source,
        start,
        end,
        diagnostic.code,
        diagnostic
            .conflict
            .as_ref()
            .map_or(u16::MAX, |conflict| conflict.witness.kind.get()),
        diagnostic.message.clone(),
    )
}

/// Derives the strongly connected components from normalized grammar structure and previously established semantic facts.
fn strongly_connected_components(edges: &[Vec<FirstCornerEdge>]) -> Vec<Vec<TypeId>> {
    let adjacency = edges
        .iter()
        .map(|outgoing| outgoing.iter().map(|edge| edge.to.index()).collect())
        .collect::<Vec<Vec<_>>>();
    strongly_connected_indices(&adjacency)
        .into_iter()
        .map(|component| {
            component
                .into_iter()
                .map(|index| TypeId::new(index as u32))
                .collect()
        })
        .collect()
}

/// Derives the strongly connected indices from normalized grammar structure and previously established semantic facts.
fn strongly_connected_indices(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    /// Mutable depth-first state for Tarjan's strongly connected component algorithm.
    struct Tarjan<'a> {
        /// Canonically ordered outgoing edges for every vertex.
        edges: &'a [Vec<usize>],
        /// Next depth-first discovery index.
        next_index: usize,
        /// Discovery index assigned to each visited vertex.
        indices: Vec<Option<usize>>,
        /// Lowest reachable discovery index for each vertex.
        lowlinks: Vec<usize>,
        /// Active depth-first vertices awaiting component assignment.
        stack: Vec<usize>,
        /// Whether each vertex remains on the active stack.
        on_stack: Vec<bool>,
        /// Completed strongly connected components in discovery order.
        components: Vec<Vec<usize>>,
    }

    impl Tarjan<'_> {
        /// Visits one vertex, propagates its lowlink, and closes a component at each root.
        fn connect(&mut self, vertex: usize) {
            let index = self.next_index;
            self.next_index += 1;
            self.indices[vertex] = Some(index);
            self.lowlinks[vertex] = index;
            self.stack.push(vertex);
            self.on_stack[vertex] = true;

            let targets = self.edges[vertex].clone();
            for target in targets {
                if self.indices[target].is_none() {
                    self.connect(target);
                    self.lowlinks[vertex] = self.lowlinks[vertex].min(self.lowlinks[target]);
                } else if self.on_stack[target] {
                    self.lowlinks[vertex] = self.lowlinks[vertex]
                        .min(self.indices[target].expect("visited vertex has an index"));
                }
            }

            if self.lowlinks[vertex] == self.indices[vertex].expect("vertex has an index") {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().expect("root remains on Tarjan stack");
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == vertex {
                        break;
                    }
                }
                self.components.push(component);
            }
        }
    }

    let mut tarjan = Tarjan {
        edges,
        next_index: 0,
        indices: vec![None; edges.len()],
        lowlinks: vec![0; edges.len()],
        stack: Vec::new(),
        on_stack: vec![false; edges.len()],
        components: Vec::new(),
    };
    for vertex in 0..edges.len() {
        if tarjan.indices[vertex].is_none() {
            tarjan.connect(vertex);
        }
    }
    tarjan.components
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the provenanced position consumed while deriving source-independent grammar facts.
struct ProvenancedPosition {
    /// Supplies the kinds consumed by semantic analysis.
    kinds: ProvenancedKindSet,
    /// Supplies the text predicate consumed by semantic analysis.
    text_predicate: Option<TextPredicateId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the provenanced first consumed while deriving source-independent grammar facts.
struct ProvenancedFirst {
    /// Supplies the positions consumed by semantic analysis.
    positions: Vec<ProvenancedPosition>,
    /// Supplies the nullable consumed by semantic analysis.
    nullable: bool,
}

/// Replays FIRST propagation while interning provenance proofs.
fn compute_first_provenance(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
) -> (ProofArena, Vec<Vec<ProvenancedPosition>>) {
    let mut proofs = ProofArena::with_source_order(declaration_source_order(grammar));
    let mut atom_cache = vec![None; grammar.atoms.len()];
    let atom_provenance = (0..grammar.atoms.len())
        .map(|index| {
            expand_atom_provenance(
                AtomId::new(index as u32),
                grammar,
                &mut atom_cache,
                &mut proofs,
            )
        })
        .collect::<Vec<_>>();

    let mut positions = vec![Vec::new(); grammar.types.len()];
    loop {
        let previous = positions.clone();
        for (index, description) in grammar.types.iter().enumerate() {
            positions[index] = first_for_shape_provenance(
                TypeId::new(index as u32),
                description,
                grammar,
                semantic,
                &atom_provenance,
                &previous,
                &mut proofs,
            )
            .positions;
        }
        if positions == previous {
            break;
        }
    }

    for (type_id, (semantic, provenanced)) in semantic.iter().zip(&positions).enumerate() {
        assert_eq!(
            semantic.positions.len(),
            provenanced.len(),
            "semantic and provenanced FIRST positions disagree for type {type_id}",
        );
        for (semantic, provenanced) in semantic.positions.iter().zip(provenanced) {
            assert_eq!(semantic.text_predicate, provenanced.text_predicate);
            assert_eq!(&semantic.kinds, provenanced.kinds.kinds());
            for kind in semantic.kinds.iter() {
                assert!(provenanced.kinds.proof(kind).is_some());
            }
        }
    }
    (proofs, positions)
}

/// Derives the declaration source order from normalized grammar structure and previously established semantic facts.
fn declaration_source_order(grammar: &GrammarDescription) -> Vec<u32> {
    let mut declarations = (0..grammar.declarations.len()).collect::<Vec<_>>();
    declarations.sort_by_key(|index| {
        let declaration = &grammar.declarations[*index];
        let location = &grammar.locations[declaration.location.index()];
        (
            location.source,
            location.range.start,
            location.range.end,
            crate::DeclId::new(*index as u32),
        )
    });
    let mut order = vec![0; grammar.declarations.len()];
    let mut previous_location = None;
    let mut source_rank = 0;
    for declaration in declarations {
        let location_id = grammar.declarations[declaration].location;
        let location = &grammar.locations[location_id.index()];
        let location_key = (location.source, location.range.start, location.range.end);
        if previous_location.is_some_and(|previous| previous != location_key) {
            source_rank += 1;
        }
        order[declaration] = source_rank;
        previous_location = Some(location_key);
    }
    order
}

/// Expands atom membership and records the derivation of each token kind.
fn expand_atom_provenance(
    atom_id: AtomId,
    grammar: &GrammarDescription,
    cache: &mut [Option<ProvenancedKindSet>],
    proofs: &mut ProofArena,
) -> ProvenancedKindSet {
    if let Some(cached) = &cache[atom_id.index()] {
        return cached.clone();
    }
    let atom = &grammar.atoms[atom_id.index()];
    let result = match &atom.expression {
        AtomExpression::Kind(kind) => {
            let proof = proofs.intern(
                None,
                ProvenanceStep::DirectDeclaration {
                    atom: atom_id,
                    kind: *kind,
                    declaration: atom.declaration,
                },
            );
            let mut result = ProvenancedKindSet::empty(grammar.kind_count);
            result.insert(*kind, proof, proofs);
            result
        }
        AtomExpression::Category(members) => {
            let mut members = members.clone();
            members.sort_unstable();
            let mut result = ProvenancedKindSet::empty(grammar.kind_count);
            for member in members {
                let member = expand_atom_provenance(member, grammar, cache, proofs);
                let wrapped = wrap_provenance(
                    &member,
                    |_, _| ProvenanceStep::CategoryMembership {
                        category: atom_id,
                        declaration: atom.declaration,
                    },
                    proofs,
                );
                result.merge(&wrapped, proofs);
            }
            result
        }
        AtomExpression::Class(members) => {
            let mut members = members.clone();
            members.sort_unstable();
            let mut result = ProvenancedKindSet::empty(grammar.kind_count);
            for member in members {
                let member = expand_atom_provenance(member, grammar, cache, proofs);
                let wrapped = wrap_provenance(
                    &member,
                    |_, _| ProvenanceStep::ClassMembership {
                        class: atom_id,
                        declaration: atom.declaration,
                    },
                    proofs,
                );
                result.merge(&wrapped, proofs);
            }
            result
        }
        AtomExpression::Target { base, admits } => {
            let mut members = vec![*base];
            members.extend(admits);
            members.sort_unstable();
            let mut result = ProvenancedKindSet::empty(grammar.kind_count);
            for member in members {
                let member = expand_atom_provenance(member, grammar, cache, proofs);
                let wrapped = wrap_provenance(
                    &member,
                    |_, _| ProvenanceStep::TargetAdmission {
                        target: atom_id,
                        declaration: atom.declaration,
                    },
                    proofs,
                );
                result.merge(&wrapped, proofs);
            }
            result
        }
    };
    cache[atom_id.index()] = Some(result.clone());
    result
}

/// Adds one propagation step to every proof in a provenanced FIRST set.
fn wrap_provenance(
    source: &ProvenancedKindSet,
    step: impl Fn(KindId, ProofId) -> ProvenanceStep,
    proofs: &mut ProofArena,
) -> ProvenancedKindSet {
    let mut wrapped = ProvenancedKindSet::empty(source.kinds().kind_count());
    for kind in source.kinds().iter() {
        let parent = source.proof(kind).expect("FIRST bit has a proof");
        let proof = proofs.intern(Some(parent), step(kind, parent));
        wrapped.insert(kind, proof, proofs);
    }
    wrapped
}

#[allow(clippy::too_many_arguments)]
/// Derives the first for shape provenance from normalized grammar structure and previously established semantic facts.
fn first_for_shape_provenance(
    owner: TypeId,
    description: &crate::TypeDescription,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    atom_provenance: &[ProvenancedKindSet],
    previous: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> ProvenancedFirst {
    // Keep proof construction in lockstep with FIRST semantics so every admitted kind has one reproducible witness.
    let raw = match &description.shape {
        TypeShape::Token {
            atom,
            text_predicate,
        } => {
            let kinds = if let Some(predicate) = text_predicate {
                wrap_provenance(
                    &atom_provenance[atom.index()],
                    |_, _| ProvenanceStep::TextPredicate {
                        predicate: *predicate,
                        declaration: description.declaration,
                    },
                    proofs,
                )
            } else {
                atom_provenance[atom.index()].clone()
            };
            ProvenancedFirst {
                positions: vec![ProvenancedPosition {
                    kinds,
                    text_predicate: *text_predicate,
                }],
                nullable: false,
            }
        }
        TypeShape::Alias { site } => {
            first_for_sites_provenance(owner, &[*site], grammar, semantic, previous, proofs)
        }
        TypeShape::Sequence { sites } => {
            first_for_sites_provenance(owner, sites, grammar, semantic, previous, proofs)
        }
        TypeShape::Enum { alternatives } => {
            let mut alternatives = alternatives.iter().collect::<Vec<_>>();
            alternatives.sort_by_key(|alternative| alternative.declaration);
            let mut combined = ProvenancedFirst {
                positions: Vec::new(),
                nullable: false,
            };
            for alternative in alternatives {
                let first = first_for_sites_provenance(
                    owner,
                    &alternative.sites,
                    grammar,
                    semantic,
                    previous,
                    proofs,
                );
                merge_provenanced_positions(&mut combined.positions, &first.positions, proofs);
                combined.nullable |= first.nullable;
            }
            combined
        }
        TypeShape::Pratt { variants } => {
            let mut variants = variants
                .iter()
                .filter(|variant| {
                    matches!(
                        variant.position,
                        PrattPosition::Atom | PrattPosition::Prefix { .. }
                    )
                })
                .collect::<Vec<_>>();
            variants.sort_by_key(|variant| declaration_source_key(variant.declaration, grammar));
            let mut positions = Vec::new();
            for variant in variants {
                let first = first_for_sites_provenance(
                    owner,
                    &variant.sites,
                    grammar,
                    semantic,
                    previous,
                    proofs,
                );
                merge_provenanced_positions(&mut positions, &first.positions, proofs);
            }
            ProvenancedFirst {
                positions,
                nullable: false,
            }
        }
    };

    let positions = raw
        .positions
        .into_iter()
        .map(|position| ProvenancedPosition {
            kinds: wrap_provenance(
                &position.kinds,
                |_, _| ProvenanceStep::FirstPropagation {
                    type_id: owner,
                    declaration: description.declaration,
                },
                proofs,
            ),
            text_predicate: position.text_predicate,
        })
        .collect();
    ProvenancedFirst {
        positions,
        nullable: raw.nullable,
    }
}

/// Derives the first for sites provenance from normalized grammar structure and previously established semantic facts.
fn first_for_sites_provenance(
    owner: TypeId,
    sites: &[SiteId],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> ProvenancedFirst {
    let mut positions = Vec::new();
    let mut nullable = true;
    let mut nullable_prefix: Vec<SiteId> = Vec::new();
    for site_id in sites {
        let site = &grammar.sites[site_id.index()];
        for position in &provenanced[site.target.index()] {
            let mut kinds = wrap_provenance(
                &position.kinds,
                |_, _| ProvenanceStep::Reference {
                    owner,
                    target: site.target,
                    site: *site_id,
                    declaration: site.declaration,
                },
                proofs,
            );
            for prefix_id in nullable_prefix.iter().rev() {
                let prefix = &grammar.sites[prefix_id.index()];
                kinds = wrap_provenance(
                    &kinds,
                    |_, _| ProvenanceStep::NullablePrefix {
                        owner,
                        site: *prefix_id,
                        declaration: prefix.declaration,
                    },
                    proofs,
                );
            }
            merge_provenanced_position(
                &mut positions,
                ProvenancedPosition {
                    kinds,
                    text_predicate: position.text_predicate,
                },
                proofs,
            );
        }
        if !site_nullable(site, &semantic[site.target.index()]) {
            nullable = false;
            break;
        }
        nullable_prefix.push(*site_id);
    }
    ProvenancedFirst {
        positions,
        nullable,
    }
}

/// Unions provenanced positions while retaining canonical proofs.
fn merge_provenanced_positions(
    into: &mut Vec<ProvenancedPosition>,
    from: &[ProvenancedPosition],
    proofs: &ProofArena,
) {
    for position in from {
        merge_provenanced_position(into, position.clone(), proofs);
    }
}

/// Inserts one position or replaces its proof with a canonical predecessor.
fn merge_provenanced_position(
    into: &mut Vec<ProvenancedPosition>,
    position: ProvenancedPosition,
    proofs: &ProofArena,
) {
    if let Some(existing) = into
        .iter_mut()
        .find(|existing| existing.text_predicate == position.text_predicate)
    {
        existing.kinds.merge(&position.kinds, proofs);
    } else {
        into.push(position);
    }
    into.sort_by_key(|position| {
        position
            .text_predicate
            .map_or(0, |id| u64::from(id.get()) + 1)
    });
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the semantic follow consumed while deriving source-independent grammar facts.
struct SemanticFollow {
    /// Supplies the kinds consumed by semantic analysis.
    kinds: KindSet,
    /// Supplies the eof consumed by semantic analysis.
    eof: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the provenanced follow consumed while deriving source-independent grammar facts.
struct ProvenancedFollow {
    /// Supplies the positions consumed by semantic analysis.
    positions: Vec<ProvenancedPosition>,
    /// Supplies the eof proof consumed by semantic analysis.
    eof_proof: Option<ProofId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Carries the follow edge consumed while deriving source-independent grammar facts.
struct FollowEdge {
    /// Grammar type whose FOLLOW facts flow along this edge.
    from: TypeId,
    /// Supplies the to consumed by semantic analysis.
    to: TypeId,
    /// Supplies the site consumed by semantic analysis.
    site: Option<SiteId>,
    /// Supplies the declaration consumed by semantic analysis.
    declaration: crate::DeclId,
}

/// Derives the site sequences from normalized grammar structure and previously established semantic facts.
fn site_sequences<'a>(shape: &'a TypeShape, grammar: &GrammarDescription) -> Vec<&'a [SiteId]> {
    let mut sequences = match shape {
        TypeShape::Token { .. } => Vec::new(),
        TypeShape::Alias { site } => vec![std::slice::from_ref(site)],
        TypeShape::Sequence { sites } => vec![sites.as_slice()],
        TypeShape::Enum { alternatives } => alternatives
            .iter()
            .map(|alternative| alternative.sites.as_slice())
            .collect(),
        TypeShape::Pratt { variants } => variants
            .iter()
            .map(|variant| variant.sites.as_slice())
            .collect(),
    };
    sequences.sort_by_key(|sites| {
        sites
            .iter()
            .map(|site| declaration_source_key(grammar.sites[site.index()].declaration, grammar))
            .collect::<Vec<_>>()
    });
    sequences
}

/// Derives the empty semantic follow from normalized grammar structure and previously established semantic facts.
fn empty_semantic_follow(grammar: &GrammarDescription) -> SemanticFollow {
    SemanticFollow {
        kinds: KindSet::empty(grammar.kind_count),
        eof: false,
    }
}

/// Derives the empty provenanced follow from normalized grammar structure and previously established semantic facts.
fn empty_provenanced_follow() -> ProvenancedFollow {
    ProvenancedFollow {
        positions: Vec::new(),
        eof_proof: None,
    }
}

/// Computes grammar-type and parse-site FOLLOW facts to a fixed point.
fn compute_follow(
    grammar: &GrammarDescription,
    first: &[FirstFacts],
    first_provenance: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> (Vec<FollowFacts>, Vec<FollowFacts>, Vec<ProvenancedFollow>) {
    // Seed entry EOF facts first, then iterate monotone FOLLOW propagation to a fixed point before publishing site views.
    let reachable = reachable_types(grammar);
    let mut semantic_by_type = vec![empty_semantic_follow(grammar); grammar.types.len()];
    let mut semantic_by_site = vec![empty_semantic_follow(grammar); grammar.sites.len()];
    let mut edges = Vec::new();

    let mut entries = grammar.entry_types.clone();
    entries.sort_unstable();
    entries.dedup();
    for entry in &entries {
        semantic_by_type[entry.index()].eof = true;
    }

    for (owner_index, description) in grammar.types.iter().enumerate() {
        if !reachable[owner_index] {
            continue;
        }
        let owner = TypeId::new(owner_index as u32);
        for sites in site_sequences(&description.shape, grammar) {
            collect_semantic_follow_for_sites(
                owner,
                sites,
                grammar,
                first,
                &mut semantic_by_type,
                &mut semantic_by_site,
                &mut edges,
            );
        }
    }
    edges.sort_unstable();
    edges.dedup();
    propagate_semantic_follow(&edges, &mut semantic_by_type, &mut semantic_by_site);

    let mut provenanced_by_type = vec![empty_provenanced_follow(); grammar.types.len()];
    let mut provenanced_by_site = vec![empty_provenanced_follow(); grammar.sites.len()];
    for entry in entries {
        let declaration = grammar.types[entry.index()].declaration;
        provenanced_by_type[entry.index()].eof_proof = Some(proofs.intern(
            None,
            ProvenanceStep::EntryEof {
                type_id: entry,
                declaration,
            },
        ));
    }
    for (owner_index, description) in grammar.types.iter().enumerate() {
        if !reachable[owner_index] {
            continue;
        }
        let owner = TypeId::new(owner_index as u32);
        for sites in site_sequences(&description.shape, grammar) {
            collect_provenanced_follow_for_sites(
                owner,
                sites,
                grammar,
                first,
                first_provenance,
                &mut provenanced_by_type,
                &mut provenanced_by_site,
                proofs,
            );
        }
    }
    propagate_provenanced_follow(
        &edges,
        &mut provenanced_by_type,
        &mut provenanced_by_site,
        proofs,
    );

    for (index, (semantic, provenanced)) in semantic_by_type
        .iter()
        .zip(&provenanced_by_type)
        .enumerate()
    {
        let provenanced_kinds = kinds_for_positions(&provenanced.positions, grammar, proofs);
        assert_eq!(
            &semantic.kinds,
            provenanced_kinds.kinds(),
            "semantic and provenanced FOLLOW disagree for type {index}",
        );
        assert_eq!(
            semantic.eof,
            provenanced.eof_proof.is_some(),
            "semantic and provenanced EOF disagree for type {index}",
        );
    }
    for (index, (semantic, provenanced)) in semantic_by_site
        .iter()
        .zip(&provenanced_by_site)
        .enumerate()
    {
        let provenanced_kinds = kinds_for_positions(&provenanced.positions, grammar, proofs);
        assert_eq!(
            &semantic.kinds,
            provenanced_kinds.kinds(),
            "semantic and provenanced FOLLOW disagree for site {index}",
        );
        assert_eq!(
            semantic.eof,
            provenanced.eof_proof.is_some(),
            "semantic and provenanced EOF disagree for site {index}",
        );
    }

    let by_type = provenanced_by_type
        .iter()
        .map(|follow| FollowFacts {
            kinds: kinds_for_positions(&follow.positions, grammar, proofs),
            eof_proof: follow.eof_proof,
        })
        .collect();
    let by_site = provenanced_by_site
        .iter()
        .map(|follow| FollowFacts {
            kinds: kinds_for_positions(&follow.positions, grammar, proofs),
            eof_proof: follow.eof_proof,
        })
        .collect();
    (by_type, by_site, provenanced_by_site)
}

/// Derives the reachable types from normalized grammar structure and previously established semantic facts.
fn reachable_types(grammar: &GrammarDescription) -> Vec<bool> {
    let mut reachable = vec![false; grammar.types.len()];
    let mut pending = grammar.entry_types.clone();
    pending.sort_unstable_by(|left, right| right.cmp(left));
    pending.dedup();
    while let Some(type_id) = pending.pop() {
        if std::mem::replace(&mut reachable[type_id.index()], true) {
            continue;
        }
        for site_id in sites_of(&grammar.types[type_id.index()].shape) {
            let site = &grammar.sites[site_id.index()];
            pending.push(site.target);
            if let Some(separator) = container_semantics(site).separator_type() {
                pending.push(separator);
            }
        }
        pending.sort_unstable_by(|left, right| right.cmp(left));
        pending.dedup();
    }
    reachable
}

#[allow(clippy::too_many_arguments)]
/// Collects semantic follow for sites.
fn collect_semantic_follow_for_sites(
    owner: TypeId,
    sites: &[SiteId],
    grammar: &GrammarDescription,
    first: &[FirstFacts],
    by_type: &mut [SemanticFollow],
    by_site: &mut [SemanticFollow],
    edges: &mut Vec<FollowEdge>,
) {
    for (index, site_id) in sites.iter().copied().enumerate() {
        let site = &grammar.sites[site_id.index()];
        let suffix = first_for_sites(&sites[index + 1..], grammar, first);
        by_site[site_id.index()].kinds.union_assign(&suffix.kinds);
        by_type[site.target.index()]
            .kinds
            .union_assign(&suffix.kinds);
        if suffix.nullable {
            edges.push(FollowEdge {
                from: owner,
                to: site.target,
                site: Some(site_id),
                declaration: site.declaration,
            });
        }

        let semantics = container_semantics(site);
        if !semantics.cardinality.is_repeated() {
            continue;
        }
        let separator = semantics.separator_type();
        if let Some(separator) = separator {
            by_type[site.target.index()]
                .kinds
                .union_assign(first[separator.index()].kinds());
            by_site[site_id.index()]
                .kinds
                .union_assign(first[separator.index()].kinds());
            if first[separator.index()].nullable {
                by_type[site.target.index()]
                    .kinds
                    .union_assign(first[site.target.index()].kinds());
                by_site[site_id.index()]
                    .kinds
                    .union_assign(first[site.target.index()].kinds());
            }
            by_type[separator.index()]
                .kinds
                .union_assign(first[site.target.index()].kinds());
            if first[site.target.index()].nullable {
                by_type[separator.index()]
                    .kinds
                    .union_assign(first[separator.index()].kinds());
                by_type[separator.index()].kinds.union_assign(&suffix.kinds);
                if suffix.nullable {
                    edges.push(FollowEdge {
                        from: owner,
                        to: separator,
                        site: None,
                        declaration: site.declaration,
                    });
                }
            }
        } else {
            let repeated_first = first[site.target.index()].kinds().clone();
            by_type[site.target.index()]
                .kinds
                .union_assign(&repeated_first);
            by_site[site_id.index()].kinds.union_assign(&repeated_first);
        }
    }
}

/// Propagates token kinds and EOF membership across one FOLLOW edge.
fn propagate_semantic_follow(
    edges: &[FollowEdge],
    by_type: &mut [SemanticFollow],
    by_site: &mut [SemanticFollow],
) {
    let (components, component_of) = follow_component_order(by_type.len(), edges);
    for component in components {
        let component_id = component_of[component[0]];
        loop {
            let previous = by_type.to_vec();
            for edge in edges.iter().filter(|edge| {
                component_of[edge.from.index()] == component_id
                    && component_of[edge.to.index()] == component_id
            }) {
                apply_semantic_follow_edge(*edge, &previous, by_type, by_site);
            }
            if component
                .iter()
                .all(|member| by_type[*member] == previous[*member])
            {
                break;
            }
        }
        let previous = by_type.to_vec();
        for edge in edges.iter().filter(|edge| {
            component_of[edge.from.index()] == component_id
                && component_of[edge.to.index()] != component_id
        }) {
            apply_semantic_follow_edge(*edge, &previous, by_type, by_site);
        }
    }
}

/// Applies semantic follow edge.
fn apply_semantic_follow_edge(
    edge: FollowEdge,
    source_facts: &[SemanticFollow],
    by_type: &mut [SemanticFollow],
    by_site: &mut [SemanticFollow],
) {
    let source = &source_facts[edge.from.index()];
    by_type[edge.to.index()].kinds.union_assign(&source.kinds);
    by_type[edge.to.index()].eof |= source.eof;
    if let Some(site) = edge.site {
        by_site[site.index()].kinds.union_assign(&source.kinds);
        by_site[site.index()].eof |= source.eof;
    }
}

/// Derives the follow component order from normalized grammar structure and previously established semantic facts.
fn follow_component_order(
    node_count: usize,
    edges: &[FollowEdge],
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut adjacency = vec![Vec::new(); node_count];
    for edge in edges {
        adjacency[edge.from.index()].push(edge.to.index());
    }
    for targets in &mut adjacency {
        targets.sort_unstable();
        targets.dedup();
    }
    let mut components = strongly_connected_indices(&adjacency);
    for component in &mut components {
        component.sort_unstable();
    }
    components.sort_by_key(|component| component[0]);
    let mut component_of = vec![0; node_count];
    for (component_id, component) in components.iter().enumerate() {
        for member in component {
            component_of[*member] = component_id;
        }
    }
    let mut outgoing = vec![Vec::new(); components.len()];
    let mut indegree = vec![0; components.len()];
    for edge in edges {
        let from = component_of[edge.from.index()];
        let to = component_of[edge.to.index()];
        if from != to {
            outgoing[from].push(to);
        }
    }
    for targets in &mut outgoing {
        targets.sort_unstable();
        targets.dedup();
        for target in targets.iter().copied() {
            indegree[target] += 1;
        }
    }
    let mut ready = (0..components.len())
        .filter(|component| indegree[*component] == 0)
        .collect::<Vec<_>>();
    ready.sort_by_key(|component| components[*component][0]);
    let mut ordered = Vec::with_capacity(components.len());
    while !ready.is_empty() {
        let component = ready.remove(0);
        ordered.push(components[component].clone());
        for target in outgoing[component].iter().copied() {
            indegree[target] -= 1;
            if indegree[target] == 0 {
                ready.push(target);
                ready.sort_by_key(|candidate| components[*candidate][0]);
            }
        }
    }
    debug_assert_eq!(ordered.len(), components.len());
    (ordered, component_of)
}

#[allow(clippy::too_many_arguments)]
/// Collects provenanced follow for sites.
fn collect_provenanced_follow_for_sites(
    owner: TypeId,
    sites: &[SiteId],
    grammar: &GrammarDescription,
    first: &[FirstFacts],
    first_provenance: &[Vec<ProvenancedPosition>],
    by_type: &mut [ProvenancedFollow],
    by_site: &mut [ProvenancedFollow],
    proofs: &mut ProofArena,
) {
    // Harvest site FOLLOW only after convergence; mixing collection into propagation would make proofs order-dependent.
    for (index, site_id) in sites.iter().copied().enumerate() {
        let site = &grammar.sites[site_id.index()];
        let suffix = first_for_sites_provenance(
            owner,
            &sites[index + 1..],
            grammar,
            first,
            first_provenance,
            proofs,
        );
        let suffix_positions = wrap_follow_positions(
            &suffix.positions,
            |_, _| ProvenanceStep::FollowSuffix {
                owner,
                site: site_id,
                declaration: site.declaration,
            },
            proofs,
        );
        merge_provenanced_positions(
            &mut by_site[site_id.index()].positions,
            &suffix_positions,
            proofs,
        );
        merge_provenanced_positions(
            &mut by_type[site.target.index()].positions,
            &suffix_positions,
            proofs,
        );

        let semantics = container_semantics(site);
        if !semantics.cardinality.is_repeated() {
            continue;
        }
        let separator = semantics.separator_type();
        if let Some(separator) = separator {
            let separator_positions = wrap_follow_positions(
                &first_provenance[separator.index()],
                |_, _| ProvenanceStep::RepeatContinuation {
                    owner,
                    site: site_id,
                    declaration: site.declaration,
                },
                proofs,
            );
            merge_provenanced_positions(
                &mut by_type[site.target.index()].positions,
                &separator_positions,
                proofs,
            );
            merge_provenanced_positions(
                &mut by_site[site_id.index()].positions,
                &separator_positions,
                proofs,
            );
            if first[separator.index()].nullable {
                let repeated_target_positions = wrap_follow_positions(
                    &first_provenance[site.target.index()],
                    |_, _| ProvenanceStep::RepeatContinuation {
                        owner,
                        site: site_id,
                        declaration: site.declaration,
                    },
                    proofs,
                );
                merge_provenanced_positions(
                    &mut by_type[site.target.index()].positions,
                    &repeated_target_positions,
                    proofs,
                );
                merge_provenanced_positions(
                    &mut by_site[site_id.index()].positions,
                    &repeated_target_positions,
                    proofs,
                );
            }
            let target_positions = wrap_follow_positions(
                &first_provenance[site.target.index()],
                |_, _| ProvenanceStep::RepeatContinuation {
                    owner,
                    site: site_id,
                    declaration: site.declaration,
                },
                proofs,
            );
            merge_provenanced_positions(
                &mut by_type[separator.index()].positions,
                &target_positions,
                proofs,
            );
            if first[site.target.index()].nullable {
                merge_provenanced_positions(
                    &mut by_type[separator.index()].positions,
                    &separator_positions,
                    proofs,
                );
                merge_provenanced_positions(
                    &mut by_type[separator.index()].positions,
                    &suffix_positions,
                    proofs,
                );
            }
        } else {
            let target_positions = wrap_follow_positions(
                &first_provenance[site.target.index()],
                |_, _| ProvenanceStep::RepeatContinuation {
                    owner,
                    site: site_id,
                    declaration: site.declaration,
                },
                proofs,
            );
            merge_provenanced_positions(
                &mut by_type[site.target.index()].positions,
                &target_positions,
                proofs,
            );
            merge_provenanced_positions(
                &mut by_site[site_id.index()].positions,
                &target_positions,
                proofs,
            );
        }
    }
}

/// Adds one FOLLOW propagation step to every incoming proof.
fn wrap_follow_positions(
    source: &[ProvenancedPosition],
    step: impl Fn(KindId, ProofId) -> ProvenanceStep + Copy,
    proofs: &mut ProofArena,
) -> Vec<ProvenancedPosition> {
    source
        .iter()
        .map(|position| ProvenancedPosition {
            kinds: wrap_provenance(&position.kinds, step, proofs),
            text_predicate: position.text_predicate,
        })
        .collect()
}

/// Propagates FOLLOW membership while retaining canonical derivations.
fn propagate_provenanced_follow(
    edges: &[FollowEdge],
    by_type: &mut [ProvenancedFollow],
    by_site: &mut [ProvenancedFollow],
    proofs: &mut ProofArena,
) {
    let (components, component_of) = follow_component_order(by_type.len(), edges);
    for component in components {
        let component_id = component_of[component[0]];
        loop {
            let previous = by_type.to_vec();
            for edge in edges.iter().filter(|edge| {
                component_of[edge.from.index()] == component_id
                    && component_of[edge.to.index()] == component_id
            }) {
                apply_provenanced_follow_edge(*edge, &previous, by_type, by_site, proofs);
            }
            if component
                .iter()
                .all(|member| by_type[*member] == previous[*member])
            {
                break;
            }
        }
        let previous = by_type.to_vec();
        for edge in edges.iter().filter(|edge| {
            component_of[edge.from.index()] == component_id
                && component_of[edge.to.index()] != component_id
        }) {
            apply_provenanced_follow_edge(*edge, &previous, by_type, by_site, proofs);
        }
    }
}

/// Applies provenanced follow edge.
fn apply_provenanced_follow_edge(
    edge: FollowEdge,
    source_facts: &[ProvenancedFollow],
    by_type: &mut [ProvenancedFollow],
    by_site: &mut [ProvenancedFollow],
    proofs: &mut ProofArena,
) {
    let source = &source_facts[edge.from.index()];
    let propagated = wrap_follow_positions(
        &source.positions,
        |_, _| ProvenanceStep::FollowPropagation {
            owner: edge.from,
            target: edge.to,
            site: edge.site,
            declaration: edge.declaration,
        },
        proofs,
    );
    merge_provenanced_positions(&mut by_type[edge.to.index()].positions, &propagated, proofs);
    if let Some(site) = edge.site {
        merge_provenanced_positions(&mut by_site[site.index()].positions, &propagated, proofs);
    }
    if let Some(parent) = source.eof_proof {
        let proof = proofs.intern(
            Some(parent),
            ProvenanceStep::FollowPropagation {
                owner: edge.from,
                target: edge.to,
                site: edge.site,
                declaration: edge.declaration,
            },
        );
        merge_eof_proof(&mut by_type[edge.to.index()].eof_proof, proof, proofs);
        if let Some(site) = edge.site {
            merge_eof_proof(&mut by_site[site.index()].eof_proof, proof, proofs);
        }
    }
}

/// Retains the canonically preferred proof of EOF membership.
fn merge_eof_proof(slot: &mut Option<ProofId>, proof: ProofId, proofs: &ProofArena) {
    if slot.is_none_or(|current| proofs.compare(proof, current).is_lt()) {
        *slot = Some(proof);
    }
}

/// Derives the kinds for positions from normalized grammar structure and previously established semantic facts.
fn kinds_for_positions(
    positions: &[ProvenancedPosition],
    grammar: &GrammarDescription,
    proofs: &ProofArena,
) -> ProvenancedKindSet {
    let mut kinds = ProvenancedKindSet::empty(grammar.kind_count);
    for position in positions {
        kinds.merge(&position.kinds, proofs);
    }
    kinds
}

/// Records why each Pratt extender token kind is reachable.
fn compute_extender_provenance(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_provenance: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> Vec<Option<Vec<ProvenancedPosition>>> {
    grammar
        .types
        .iter()
        .enumerate()
        .map(|(index, description)| {
            let TypeShape::Pratt { variants } = &description.shape else {
                return None;
            };
            let owner = TypeId::new(index as u32);
            let mut led = variants
                .iter()
                .filter(|variant| is_pratt_led(variant.position))
                .collect::<Vec<_>>();
            led.sort_by_key(|variant| declaration_source_key(variant.declaration, grammar));
            let mut positions = Vec::new();
            for variant in led {
                let extender = pratt_extender_provenance(
                    owner,
                    variant,
                    grammar,
                    semantic,
                    first_provenance,
                    proofs,
                );
                merge_provenanced_positions(&mut positions, &extender.positions, proofs);
            }
            Some(positions)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
/// Derives the pratt extender provenance from normalized grammar structure and previously established semantic facts.
fn pratt_extender_provenance(
    owner: TypeId,
    variant: &crate::PrattVariant,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_provenance: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> ProvenancedFirst {
    let first = first_for_sites_provenance(
        owner,
        &variant.sites,
        grammar,
        semantic,
        first_provenance,
        proofs,
    );
    debug_assert!(is_pratt_led(variant.position));
    let positions = first
        .positions
        .into_iter()
        .map(|position| ProvenancedPosition {
            kinds: wrap_provenance(
                &position.kinds,
                |_, _| ProvenanceStep::PrattExtender {
                    pratt: owner,
                    pratt_declaration: grammar.types[owner.index()].declaration,
                    variant: variant.declaration,
                    position: variant.position,
                },
                proofs,
            ),
            text_predicate: position.text_predicate,
        })
        .collect();
    ProvenancedFirst {
        positions,
        nullable: first.nullable,
    }
}

/// Reports whether a Pratt position can begin an expression without a left operand.
const fn is_pratt_nud(position: PrattPosition) -> bool {
    matches!(position, PrattPosition::Atom | PrattPosition::Prefix { .. })
}

/// Reports whether a Pratt position extends an already parsed left operand.
const fn is_pratt_led(position: PrattPosition) -> bool {
    matches!(
        position,
        PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
    )
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Classifies the decision item alternatives whose semantics affect the derived facts.
enum DecisionItem {
    /// Selects the type alternative for semantic analysis.
    Type(TypeId),
    /// Selects the site alternative for semantic analysis.
    Site(SiteId),
    /// Selects the repeat tail alternative for semantic analysis.
    RepeatTail {
        /// Supplies the target consumed by semantic analysis.
        target: TypeId,
        /// Supplies the separator consumed by semantic analysis.
        separator: Option<TypeId>,
        /// Whether the separator may terminate the repetition without another element.
        optional_trailing: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// Source-independent parse-site behavior observable to bounded decisions.
struct DecisionSiteSemantics {
    /// Referenced grammar type.
    target: TypeId,
    /// Normalized cardinality encoded without source container ownership.
    cardinality: u8,
    /// Optional separator type for repeated sites.
    separator: Option<TypeId>,
    /// Whether a repeated site accepts one final separator without another element.
    optional_trailing: bool,
    /// Minimum Pratt binding power required at this site.
    min_bp: u8,
}

/// Projects one authored site to its source-independent decision behavior.
fn decision_site_semantics(site: &crate::ParseSite) -> DecisionSiteSemantics {
    let semantics = container_semantics(site);
    let cardinality = match semantics.cardinality {
        ContainerCardinality::One => 0,
        ContainerCardinality::Optional => 1,
        ContainerCardinality::ZeroOrMore => 2,
        ContainerCardinality::OneOrMore => 3,
        ContainerCardinality::OptionalOneOrMore => 4,
    };
    DecisionSiteSemantics {
        target: site.target,
        cardinality,
        separator: semantics.separator_type(),
        optional_trailing: semantics
            .separator
            .is_some_and(|separator| separator.optional_trailing),
        min_bp: site.min_bp,
    }
}

#[derive(Debug, Eq, PartialEq)]
/// Carries the decision state consumed while deriving source-independent grammar facts.
struct DecisionState(Vec<DecisionItem>);

impl Clone for DecisionState {
    fn clone(&self) -> Self {
        #[cfg(test)]
        FIRST_LANGUAGE_DECISION_STATE_CLONES
            .set(FIRST_LANGUAGE_DECISION_STATE_CLONES.get().saturating_add(1));
        Self(self.0.clone())
    }
}

impl Ord for DecisionState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(test)]
        FIRST_LANGUAGE_DECISION_STATE_COMPARISONS.set(
            FIRST_LANGUAGE_DECISION_STATE_COMPARISONS
                .get()
                .saturating_add(1),
        );
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for DecisionState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for DecisionState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.0, state);
    }
}

impl DecisionState {
    /// Seeds a decision state from grammar positions in source order.
    fn from_sites(sites: &[SiteId]) -> Self {
        Self(sites.iter().copied().map(DecisionItem::Site).collect())
    }

    /// Derives the consuming site and tail from normalized grammar structure and previously established semantic facts.
    fn consuming_site_and_tail(site: &crate::ParseSite, tail: &[SiteId]) -> Self {
        let semantics = container_semantics(site);
        let mut items = if semantics.cardinality.is_repeated() {
            vec![
                DecisionItem::Type(site.target),
                DecisionItem::RepeatTail {
                    target: site.target,
                    separator: semantics.separator_type(),
                    optional_trailing: semantics
                        .separator
                        .is_some_and(|separator| separator.optional_trailing),
                },
            ]
        } else {
            vec![DecisionItem::Type(site.target)]
        };
        items.extend(tail.iter().copied().map(DecisionItem::Site));
        Self(items)
    }
}

/// Rewrites authored site identities to the canonical representative of their semantics.
fn canonicalize_decision_state(
    mut state: DecisionState,
    canonical_sites: &[SiteId],
) -> DecisionState {
    for item in &mut state.0 {
        if let DecisionItem::Site(site) = item {
            *site = canonical_sites[site.index()];
        }
    }
    state
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar-wide least-fixed-point facts needed to summarize cutoff residuals compositionally.
struct DecisionSummaryFacts {
    /// Exact-end and continuation facts indexed by type identity.
    by_type: Vec<u8>,
    /// Exact-end and continuation facts indexed by parse-site identity.
    by_site: Vec<u8>,
    /// Whether each type derives at least one finite token sequence.
    productive_by_type: Vec<bool>,
    /// Whether each parse site derives at least one finite token sequence.
    productive_by_site: Vec<bool>,
    /// Whether each type has a finite consuming path that reaches a dead residual.
    dead_prefix_by_type: Vec<bool>,
    /// Whether each parse site has a finite consuming path that reaches a dead residual.
    dead_prefix_by_site: Vec<bool>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// One consuming edge retained by the one-position cutoff fixed point.
struct DecisionCutoffFactTransition {
    /// Token atom consumed by this edge.
    atom: AtomId,
    /// Optional token-text restriction.
    text_predicate: Option<TextPredicateId>,
    /// Exact-end and continuation facts after consuming this edge.
    summary: u8,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
/// One-position decision language for a grammar item or type.
struct DecisionCutoffFact {
    /// Whether this language can consume no token.
    can_end: bool,
    /// Canonical consuming edges and their residual summaries.
    transitions: Vec<DecisionCutoffFactTransition>,
}

impl DecisionCutoffFact {
    /// Unions another alternative and canonicalizes equal token predicates.
    fn union_assign(&mut self, other: &Self) {
        self.can_end |= other.can_end;
        self.transitions.extend_from_slice(&other.transitions);
        self.normalize();
    }

    /// Coalesces equal token predicates by existentially unioning successor facts.
    fn normalize(&mut self) {
        let mut summaries = BTreeMap::<(AtomId, Option<TextPredicateId>), u8>::new();
        for transition in self.transitions.drain(..) {
            *summaries
                .entry((transition.atom, transition.text_predicate))
                .or_default() |= transition.summary;
        }
        self.transitions = summaries
            .into_iter()
            .map(
                |((atom, text_predicate), summary)| DecisionCutoffFactTransition {
                    atom,
                    text_predicate,
                    summary,
                },
            )
            .collect();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar-wide one-position decision facts solved as a monotone fixed point.
struct DecisionCutoffFacts {
    /// Facts indexed by type identity.
    by_type: Vec<DecisionCutoffFact>,
    /// Facts indexed by parse-site identity.
    by_site: Vec<DecisionCutoffFact>,
    /// Facts for repetition continuations indexed by target and separator behavior.
    by_repeat: BTreeMap<(TypeId, Option<TypeId>, bool), DecisionCutoffFact>,
}

impl DecisionCutoffFacts {
    /// Solves the finite one-position language equations for all grammar items.
    fn from_grammar(
        grammar: &GrammarDescription,
        atom_kinds: &[KindSet],
        summaries: &DecisionSummaryFacts,
    ) -> Self {
        #[cfg(test)]
        FIRST_LANGUAGE_CUTOFF_FACT_CONSTRUCTIONS.set(
            FIRST_LANGUAGE_CUTOFF_FACT_CONSTRUCTIONS
                .get()
                .saturating_add(1),
        );
        let mut facts = Self {
            by_type: vec![DecisionCutoffFact::default(); grammar.types.len()],
            by_site: vec![DecisionCutoffFact::default(); grammar.sites.len()],
            by_repeat: grammar
                .sites
                .iter()
                .filter_map(|site| {
                    let semantics = container_semantics(site);
                    semantics.cardinality.is_repeated().then_some((
                        (
                            site.target,
                            semantics.separator_type(),
                            semantics
                                .separator
                                .is_some_and(|separator| separator.optional_trailing),
                        ),
                        Default::default(),
                    ))
                })
                .collect(),
        };
        loop {
            let by_repeat = facts
                .by_repeat
                .keys()
                .map(|&(target, separator, optional_trailing)| {
                    let mut items = Vec::new();
                    if let Some(separator) = separator {
                        items.push((
                            &facts.by_type[separator.index()],
                            summaries.by_type[separator.index()],
                        ));
                    }
                    items.push((
                        &facts.by_type[target.index()],
                        summaries.by_type[target.index()],
                    ));
                    items.push((
                        &facts.by_repeat[&(target, separator, optional_trailing)],
                        decision_repeat_tail_summary(
                            target,
                            separator,
                            optional_trailing,
                            &summaries.by_type,
                        ),
                    ));
                    let mut fact = compose_decision_cutoff_facts(&items);
                    if optional_trailing {
                        let separator = separator
                            .expect("optional trailing behavior requires a separator type");
                        fact.union_assign(&facts.by_type[separator.index()]);
                    }
                    fact.can_end = true;
                    ((target, separator, optional_trailing), fact)
                })
                .collect::<BTreeMap<_, _>>();
            let by_site = grammar
                .sites
                .iter()
                .map(|site| {
                    let semantics = container_semantics(site);
                    let target = &facts.by_type[site.target.index()];
                    match semantics.cardinality {
                        ContainerCardinality::One => target.clone(),
                        ContainerCardinality::Optional => {
                            let mut fact = target.clone();
                            fact.can_end = true;
                            fact
                        }
                        ContainerCardinality::ZeroOrMore
                        | ContainerCardinality::OneOrMore
                        | ContainerCardinality::OptionalOneOrMore => {
                            let optional_trailing = semantics
                                .separator
                                .is_some_and(|separator| separator.optional_trailing);
                            let repeat = &facts.by_repeat
                                [&(site.target, semantics.separator_type(), optional_trailing)];
                            let mut fact = compose_decision_cutoff_facts(&[
                                (target, summaries.by_type[site.target.index()]),
                                (
                                    repeat,
                                    decision_repeat_tail_summary(
                                        site.target,
                                        semantics.separator_type(),
                                        optional_trailing,
                                        &summaries.by_type,
                                    ),
                                ),
                            ]);
                            if matches!(
                                semantics.cardinality,
                                ContainerCardinality::ZeroOrMore
                                    | ContainerCardinality::OptionalOneOrMore
                            ) {
                                fact.can_end = true;
                            }
                            fact
                        }
                    }
                })
                .collect::<Vec<_>>();
            let by_type = grammar
                .types
                .iter()
                .map(|description| match &description.shape {
                    TypeShape::Token {
                        atom,
                        text_predicate,
                    } => {
                        if decision_token_is_viable(*atom, *text_predicate, grammar, atom_kinds) {
                            DecisionCutoffFact {
                                can_end: false,
                                transitions: vec![DecisionCutoffFactTransition {
                                    atom: *atom,
                                    text_predicate: *text_predicate,
                                    summary: FIRST_LANGUAGE_CUTOFF_END,
                                }],
                            }
                        } else {
                            DecisionCutoffFact::default()
                        }
                    }
                    TypeShape::Alias { site } => by_site[site.index()].clone(),
                    TypeShape::Sequence { sites } => compose_decision_cutoff_facts(
                        &sites
                            .iter()
                            .map(|site| (&by_site[site.index()], summaries.by_site[site.index()]))
                            .collect::<Vec<_>>(),
                    ),
                    TypeShape::Enum { alternatives } => {
                        let mut fact = DecisionCutoffFact::default();
                        for alternative in alternatives {
                            fact.union_assign(&compose_decision_cutoff_facts(
                                &alternative
                                    .sites
                                    .iter()
                                    .map(|site| {
                                        (&by_site[site.index()], summaries.by_site[site.index()])
                                    })
                                    .collect::<Vec<_>>(),
                            ));
                        }
                        fact
                    }
                    TypeShape::Pratt { variants } => {
                        let mut fact = DecisionCutoffFact::default();
                        for variant in variants
                            .iter()
                            .filter(|variant| is_pratt_nud(variant.position))
                        {
                            fact.union_assign(&compose_decision_cutoff_facts(
                                &variant
                                    .sites
                                    .iter()
                                    .map(|site| {
                                        (&by_site[site.index()], summaries.by_site[site.index()])
                                    })
                                    .collect::<Vec<_>>(),
                            ));
                        }
                        fact
                    }
                })
                .collect::<Vec<_>>();
            let next = Self {
                by_type,
                by_site,
                by_repeat,
            };
            if next == facts {
                return facts;
            }
            facts = next;
        }
    }

    /// Composes the one-position fact for one semantic decision stack.
    fn for_state(
        &self,
        state: &DecisionState,
        summaries: &DecisionSummaryFacts,
    ) -> DecisionCutoffFact {
        compose_decision_cutoff_facts(
            &state
                .0
                .iter()
                .map(|item| match item {
                    DecisionItem::Type(type_id) => (
                        &self.by_type[type_id.index()],
                        summaries.by_type[type_id.index()],
                    ),
                    DecisionItem::Site(site_id) => (
                        &self.by_site[site_id.index()],
                        summaries.by_site[site_id.index()],
                    ),
                    DecisionItem::RepeatTail {
                        target,
                        separator,
                        optional_trailing,
                    } => (
                        &self.by_repeat[&(*target, *separator, *optional_trailing)],
                        decision_repeat_tail_summary(
                            *target,
                            *separator,
                            *optional_trailing,
                            &summaries.by_type,
                        ),
                    ),
                })
                .collect::<Vec<_>>(),
        )
    }

    /// Reports whether every repetition continuation has a grammar-derived fact.
    fn supports_state(&self, state: &DecisionState) -> bool {
        state.0.iter().all(|item| match item {
            DecisionItem::RepeatTail {
                target,
                separator,
                optional_trailing,
            } => self
                .by_repeat
                .contains_key(&(*target, *separator, *optional_trailing)),
            DecisionItem::Type(_) | DecisionItem::Site(_) => true,
        })
    }
}

/// Composes one-position languages for an ordered semantic sequence.
fn compose_decision_cutoff_facts(items: &[(&DecisionCutoffFact, u8)]) -> DecisionCutoffFact {
    let mut fact = DecisionCutoffFact::default();
    let mut prefix_can_end = true;
    for (index, (item, summary)) in items.iter().enumerate() {
        if !prefix_can_end {
            break;
        }
        for transition in &item.transitions {
            let successor = decision_sequence_summary(
                std::iter::once(transition.summary)
                    .chain(items[index + 1..].iter().map(|(_, summary)| *summary)),
            );
            fact.transitions.push(DecisionCutoffFactTransition {
                atom: transition.atom,
                text_predicate: transition.text_predicate,
                summary: successor,
            });
        }
        prefix_can_end &= summary & FIRST_LANGUAGE_CUTOFF_END != 0;
    }
    fact.can_end = prefix_can_end;
    fact.normalize();
    fact
}

impl DecisionSummaryFacts {
    /// Solves the monotone two-bit grammar equations without optimistic recursion defaults.
    fn from_grammar(grammar: &GrammarDescription, atom_kinds: &[KindSet]) -> Self {
        let mut by_type = vec![0_u8; grammar.types.len()];
        let (by_type, by_site) = loop {
            let by_site = grammar
                .sites
                .iter()
                .map(|site| decision_site_summary(site, &by_type))
                .collect::<Vec<_>>();
            let next = grammar
                .types
                .iter()
                .map(|description| match &description.shape {
                    TypeShape::Token {
                        atom,
                        text_predicate,
                    } => {
                        if decision_token_is_viable(*atom, *text_predicate, grammar, atom_kinds) {
                            FIRST_LANGUAGE_CUTOFF_CONTINUES
                        } else {
                            0
                        }
                    }
                    TypeShape::Alias { site } => by_site[site.index()],
                    TypeShape::Sequence { sites } => {
                        decision_sequence_summary(sites.iter().map(|site| by_site[site.index()]))
                    }
                    TypeShape::Enum { alternatives } => alternatives
                        .iter()
                        .map(|alternative| {
                            decision_sequence_summary(
                                alternative.sites.iter().map(|site| by_site[site.index()]),
                            )
                        })
                        .fold(0_u8, std::ops::BitOr::bitor),
                    TypeShape::Pratt { variants } => variants
                        .iter()
                        .filter(|variant| is_pratt_nud(variant.position))
                        .map(|variant| {
                            decision_sequence_summary(
                                variant.sites.iter().map(|site| by_site[site.index()]),
                            )
                        })
                        .fold(0_u8, std::ops::BitOr::bitor),
                })
                .collect::<Vec<_>>();
            debug_assert!(
                next.iter()
                    .zip(&by_type)
                    .all(|(next, previous)| next | previous == *next)
            );
            if next == by_type {
                break (by_type, by_site);
            }
            by_type = next;
        };
        let mut productive_by_type = vec![false; grammar.types.len()];
        let productive_by_site = loop {
            let productive_by_site = grammar
                .sites
                .iter()
                .map(|site| decision_site_productive(site, &productive_by_type))
                .collect::<Vec<_>>();
            let next = grammar
                .types
                .iter()
                .map(|description| match &description.shape {
                    TypeShape::Token {
                        atom,
                        text_predicate,
                    } => decision_token_is_viable(*atom, *text_predicate, grammar, atom_kinds),
                    TypeShape::Alias { site } => productive_by_site[site.index()],
                    TypeShape::Sequence { sites } => {
                        sites.iter().all(|site| productive_by_site[site.index()])
                    }
                    TypeShape::Enum { alternatives } => alternatives.iter().any(|alternative| {
                        alternative
                            .sites
                            .iter()
                            .all(|site| productive_by_site[site.index()])
                    }),
                    TypeShape::Pratt { variants } => variants
                        .iter()
                        .filter(|variant| is_pratt_nud(variant.position))
                        .any(|variant| {
                            variant
                                .sites
                                .iter()
                                .all(|site| productive_by_site[site.index()])
                        }),
                })
                .collect::<Vec<_>>();
            debug_assert!(
                next.iter()
                    .zip(&productive_by_type)
                    .all(|(next, previous)| *next || !*previous)
            );
            if next == productive_by_type {
                break productive_by_site;
            }
            productive_by_type = next;
        };
        let mut dead_prefix_by_type = vec![false; grammar.types.len()];
        let dead_prefix_by_site = loop {
            let dead_prefix_by_site = grammar
                .sites
                .iter()
                .map(|site| {
                    decision_site_has_dead_prefix(site, &productive_by_type, &dead_prefix_by_type)
                })
                .collect::<Vec<_>>();
            let next = grammar
                .types
                .iter()
                .enumerate()
                .map(|(index, description)| {
                    if by_type[index] == 0 {
                        return true;
                    }
                    match &description.shape {
                        TypeShape::Token {
                            atom,
                            text_predicate,
                        } => !decision_token_is_viable(*atom, *text_predicate, grammar, atom_kinds),
                        TypeShape::Alias { site } => dead_prefix_by_site[site.index()],
                        TypeShape::Sequence { sites } => {
                            decision_sequence_has_dead_prefix(sites.iter().map(|site| {
                                (
                                    productive_by_site[site.index()],
                                    dead_prefix_by_site[site.index()],
                                )
                            }))
                        }
                        TypeShape::Enum { alternatives } => {
                            alternatives.is_empty()
                                || alternatives.iter().any(|alternative| {
                                    decision_sequence_has_dead_prefix(alternative.sites.iter().map(
                                        |site| {
                                            (
                                                productive_by_site[site.index()],
                                                dead_prefix_by_site[site.index()],
                                            )
                                        },
                                    ))
                                })
                        }
                        TypeShape::Pratt { variants } => {
                            let nud = variants
                                .iter()
                                .filter(|variant| is_pratt_nud(variant.position))
                                .collect::<Vec<_>>();
                            nud.is_empty()
                                || nud.iter().any(|variant| {
                                    decision_sequence_has_dead_prefix(variant.sites.iter().map(
                                        |site| {
                                            (
                                                productive_by_site[site.index()],
                                                dead_prefix_by_site[site.index()],
                                            )
                                        },
                                    ))
                                })
                        }
                    }
                })
                .collect::<Vec<_>>();
            debug_assert!(
                next.iter()
                    .zip(&dead_prefix_by_type)
                    .all(|(next, previous)| *next || !*previous)
            );
            if next == dead_prefix_by_type {
                break dead_prefix_by_site;
            }
            dead_prefix_by_type = next;
        };
        Self {
            by_type,
            by_site,
            productive_by_type,
            productive_by_site,
            dead_prefix_by_type,
            dead_prefix_by_site,
        }
    }

    /// Composes one semantic decision state from already-solved item facts.
    fn summarize_state(&self, state: &DecisionState) -> u8 {
        decision_sequence_summary(state.0.iter().map(|item| self.summarize_item(item)))
    }

    /// Returns the solved facts for one semantic stack item.
    fn summarize_item(&self, item: &DecisionItem) -> u8 {
        match item {
            DecisionItem::Type(type_id) => self.by_type[type_id.index()],
            DecisionItem::Site(site_id) => self.by_site[site_id.index()],
            DecisionItem::RepeatTail {
                target,
                separator,
                optional_trailing,
            } => {
                decision_repeat_tail_summary(*target, *separator, *optional_trailing, &self.by_type)
            }
        }
    }

    /// Reports whether a semantic state can derive the empty token sequence.
    fn state_is_nullable(&self, state: &DecisionState) -> bool {
        self.summarize_state(state) & FIRST_LANGUAGE_CUTOFF_END != 0
    }

    /// Reports whether a semantic state derives at least one finite token sequence.
    fn state_is_productive(&self, state: &DecisionState) -> bool {
        state.0.iter().all(|item| match item {
            DecisionItem::Type(type_id) => self.productive_by_type[type_id.index()],
            DecisionItem::Site(site_id) => self.productive_by_site[site_id.index()],
            DecisionItem::RepeatTail { .. } => true,
        })
    }

    /// Reports whether a finite consuming prefix can reach a residual with no end or transition.
    fn state_has_dead_prefix(&self, state: &DecisionState) -> bool {
        decision_sequence_has_dead_prefix(state.0.iter().map(|item| match item {
            DecisionItem::Type(type_id) => (
                self.productive_by_type[type_id.index()],
                self.dead_prefix_by_type[type_id.index()],
            ),
            DecisionItem::Site(site_id) => (
                self.productive_by_site[site_id.index()],
                self.dead_prefix_by_site[site_id.index()],
            ),
            DecisionItem::RepeatTail {
                target, separator, ..
            } => (
                true,
                decision_repeat_tail_has_dead_prefix(
                    *target,
                    *separator,
                    &self.productive_by_type,
                    &self.dead_prefix_by_type,
                ),
            ),
        }))
    }
}

/// Reports whether a token can consume at least one normalized lexical value.
fn decision_token_is_viable(
    atom: AtomId,
    text_predicate: Option<TextPredicateId>,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
) -> bool {
    !atom_kinds[atom.index()].is_empty()
        && text_predicate.is_none_or(|predicate| {
            !matches!(
                &grammar.text_predicates[predicate.index()],
                TextPredicate::OneOf(options) if options.is_empty()
            )
        })
}

/// Reports whether one parse-site container derives at least one finite token sequence.
fn decision_site_productive(site: &crate::ParseSite, productive_by_type: &[bool]) -> bool {
    match container_semantics(site).cardinality {
        ContainerCardinality::One | ContainerCardinality::OneOrMore => {
            productive_by_type[site.target.index()]
        }
        ContainerCardinality::Optional
        | ContainerCardinality::ZeroOrMore
        | ContainerCardinality::OptionalOneOrMore => true,
    }
}

/// Reports whether an ordered semantic sequence can consume into a dead residual.
fn decision_sequence_has_dead_prefix(items: impl IntoIterator<Item = (bool, bool)>) -> bool {
    let mut prefix_productive = true;
    for (productive, has_dead_prefix) in items {
        if prefix_productive && has_dead_prefix {
            return true;
        }
        prefix_productive &= productive;
        if !prefix_productive {
            return false;
        }
    }
    false
}

/// Reports whether one parse-site container can consume into a dead residual.
fn decision_site_has_dead_prefix(
    site: &crate::ParseSite,
    productive_by_type: &[bool],
    dead_prefix_by_type: &[bool],
) -> bool {
    let semantics = container_semantics(site);
    let target = site.target;
    let target_dead = dead_prefix_by_type[target.index()];
    if !semantics.cardinality.is_repeated() {
        return target_dead;
    }
    target_dead
        || (productive_by_type[target.index()]
            && decision_repeat_tail_has_dead_prefix(
                target,
                semantics.separator_type(),
                productive_by_type,
                dead_prefix_by_type,
            ))
}

/// Reports whether choosing another repetition element can consume into a dead residual.
fn decision_repeat_tail_has_dead_prefix(
    target: TypeId,
    separator: Option<TypeId>,
    productive_by_type: &[bool],
    dead_prefix_by_type: &[bool],
) -> bool {
    separator.map_or(dead_prefix_by_type[target.index()], |separator| {
        dead_prefix_by_type[separator.index()]
            || (productive_by_type[separator.index()] && dead_prefix_by_type[target.index()])
    })
}

/// Composes exact-end and continuation facts for an ordered semantic sequence.
fn decision_sequence_summary(summaries: impl IntoIterator<Item = u8>) -> u8 {
    let mut can_end = true;
    let mut continues = false;
    for summary in summaries {
        continues |= can_end && summary & FIRST_LANGUAGE_CUTOFF_CONTINUES != 0;
        can_end &= summary & FIRST_LANGUAGE_CUTOFF_END != 0;
        if continues && !can_end {
            break;
        }
    }
    (u8::from(can_end) * FIRST_LANGUAGE_CUTOFF_END)
        | (u8::from(continues) * FIRST_LANGUAGE_CUTOFF_CONTINUES)
}

/// Summarizes one parse-site container from the current type fixed-point facts.
fn decision_site_summary(site: &crate::ParseSite, by_type: &[u8]) -> u8 {
    let target = by_type[site.target.index()];
    let semantics = container_semantics(site);
    match semantics.cardinality {
        ContainerCardinality::One => target,
        ContainerCardinality::Optional => target | FIRST_LANGUAGE_CUTOFF_END,
        ContainerCardinality::ZeroOrMore | ContainerCardinality::OptionalOneOrMore => {
            decision_sequence_summary([
                target,
                decision_repeat_tail_summary(
                    site.target,
                    semantics.separator_type(),
                    semantics
                        .separator
                        .is_some_and(|separator| separator.optional_trailing),
                    by_type,
                ),
            ]) | FIRST_LANGUAGE_CUTOFF_END
        }
        ContainerCardinality::OneOrMore => decision_sequence_summary([
            target,
            decision_repeat_tail_summary(
                site.target,
                semantics.separator_type(),
                semantics
                    .separator
                    .is_some_and(|separator| separator.optional_trailing),
                by_type,
            ),
        ]),
    }
}

/// Solves the least fixed point for one repeat-tail item's epsilon-or-continuation language.
fn decision_repeat_tail_summary(
    target: TypeId,
    separator: Option<TypeId>,
    optional_trailing: bool,
    by_type: &[u8],
) -> u8 {
    let continuation = if let Some(separator) = separator {
        decision_sequence_summary([by_type[separator.index()], by_type[target.index()]])
    } else {
        by_type[target.index()]
    };
    let trailing = if optional_trailing {
        let separator = separator.expect("optional trailing behavior requires a separator type");
        by_type[separator.index()]
    } else {
        0
    };
    FIRST_LANGUAGE_CUTOFF_END | ((continuation | trailing) & FIRST_LANGUAGE_CUTOFF_CONTINUES)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the paired decision state alternatives whose semantics affect the derived facts.
enum PairedDecisionState {
    /// Selects the residual alternative for semantic analysis.
    Residual(DecisionState, DecisionState),
    /// Selects the end alternative for semantic analysis.
    End,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Classifies the decision witness alternatives whose semantics affect the derived facts.
enum DecisionWitness {
    /// Selects the token alternative for semantic analysis.
    Token(crate::LookaheadWitnessPosition),
    /// Selects the end alternative for semantic analysis.
    End,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Carries the decision transition consumed while deriving source-independent grammar facts.
struct DecisionTransition {
    /// Supplies the position consumed by semantic analysis.
    position: FirstPosition,
    /// Supplies the next consumed by semantic analysis.
    next: DecisionState,
    /// Supplies the minimum stack depth consumed by semantic analysis.
    minimum_stack_depth: usize,
}

#[derive(Clone, Default)]
/// Carries the decision frontier consumed while deriving source-independent grammar facts.
struct DecisionFrontier {
    /// Supplies the can end consumed by semantic analysis.
    can_end: bool,
    /// Supplies the transitions consumed by semantic analysis.
    transitions: Vec<DecisionTransition>,
}

#[derive(Clone, Copy)]
/// One semantic event emitted while traversing a decision state's non-consuming closure.
enum DecisionFrontierEvent<'a> {
    /// The decision state can terminate without consuming another token.
    End,
    /// The decision state can consume one token and leave `rest` as its residual.
    Transition {
        /// Token atom consumed by this edge.
        atom: AtomId,
        /// Optional exact-text restriction on the consumed token.
        text_predicate: Option<TextPredicateId>,
        /// Semantic residual after the token is consumed.
        rest: &'a [DecisionItem],
        /// Minimum semantic stack depth seen before this token is consumed.
        minimum_stack_depth: usize,
    },
}

/// Carries the decision outcome consumed while deriving source-independent grammar facts.
struct DecisionOutcome {
    /// Supplies the sufficient depth consumed by semantic analysis.
    sufficient_depth: Option<usize>,
    /// Supplies the witness consumed by semantic analysis.
    witness: Vec<crate::LookaheadWitnessPosition>,
    /// Supplies the end of alternative consumed by semantic analysis.
    end_of_alternative: bool,
}

/// Distinguishes a proven unbounded shared prefix from a finite decision product.
enum DecisionProduct {
    /// A canonical back-edge proves that no finite lookahead separates the languages.
    Cyclic,
    /// The complete acyclic product supports the exact sufficient-depth calculation.
    Acyclic(BTreeMap<PairedDecisionState, Vec<(PairedDecisionState, DecisionWitness)>>),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Carries the decision parent consumed while deriving source-independent grammar facts.
struct DecisionParent {
    /// Supplies the dense parent-state identity consumed by semantic analysis.
    state: usize,
    /// Supplies the left minimum stack depth consumed by semantic analysis.
    left_minimum_stack_depth: usize,
    /// Supplies the right minimum stack depth consumed by semantic analysis.
    right_minimum_stack_depth: usize,
}

/// Hash-conses paired residuals and their incoming derivations for product traversal.
///
/// `states`, `incoming`, and `successors` remain index-aligned for every dense identity.
/// Each `incoming` row remains sorted because product insertion uses binary search to deduplicate
/// derivations before scheduling a parent-driven revisit.
struct DecisionProductParents {
    /// Canonical paired residual value for each dense identity.
    states: Vec<PairedDecisionState>,
    /// Dense identity assigned to each canonical paired residual.
    ids: BTreeMap<PairedDecisionState, usize>,
    /// Incoming derivations indexed by child-state identity.
    incoming: Vec<Vec<DecisionParent>>,
    /// Complete consuming successors already generated for each state identity.
    successors: Vec<Option<Vec<PairedDecisionSuccessor>>>,
}

impl DecisionProductParents {
    /// Creates a parent arena rooted at one paired residual.
    fn new(start: PairedDecisionState) -> Self {
        Self {
            states: vec![start.clone()],
            ids: BTreeMap::from([(start, 0)]),
            incoming: vec![Vec::new()],
            successors: vec![None],
        }
    }

    /// Returns the dense identity for one paired residual, interning it when absent.
    fn intern(&mut self, state: PairedDecisionState) -> usize {
        if let Some(id) = self.ids.get(&state) {
            return *id;
        }
        let id = self.states.len();
        self.states.push(state.clone());
        self.ids.insert(state, id);
        self.incoming.push(Vec::new());
        self.successors.push(None);
        id
    }
}

#[derive(Default)]
/// Reuses generation-stamped dense visitation storage across ancestor traversals.
struct DecisionVisitScratch {
    /// Generation in which each dense state identity was last visited.
    marks: Vec<u32>,
    /// Current nonzero traversal generation.
    generation: u32,
    /// Pending dense state identities for the current traversal.
    queue: VecDeque<usize>,
}

impl DecisionVisitScratch {
    /// Starts a fresh traversal from one state identity.
    fn start(&mut self, start: usize, state_count: usize) {
        self.marks.resize(state_count, 0);
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.marks.fill(0);
            self.generation = 1;
        }
        self.queue.clear();
        self.queue.push_back(start);
    }

    /// Marks a state for the current generation and reports whether it was new.
    fn visit(&mut self, state: usize) -> bool {
        if self.marks[state] == self.generation {
            return false;
        }
        self.marks[state] = self.generation;
        true
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Carries the paired decision successor consumed while deriving source-independent grammar facts.
struct PairedDecisionSuccessor {
    /// Supplies the state consumed by semantic analysis.
    state: PairedDecisionState,
    /// Supplies the witness consumed by semantic analysis.
    witness: DecisionWitness,
    /// Supplies the left minimum stack depth consumed by semantic analysis.
    left_minimum_stack_depth: usize,
    /// Supplies the right minimum stack depth consumed by semantic analysis.
    right_minimum_stack_depth: usize,
}

/// Distinguishes unavailable frozen analysis from an exact bounded witness result.
enum BoundedDecisionWitness {
    /// The decision shape is outside the sound frozen-language subset and needs explicit traversal.
    Unsupported,
    /// Exact bounded languages were compared and either produced or disproved a witness.
    Exact(Option<Vec<DecisionWitness>>),
}

impl BoundedDecisionWitness {
    /// Reports that exact configured-depth languages have no shared prefix.
    fn is_disjoint(&self) -> bool {
        matches!(self, Self::Exact(None))
    }
}

/// Derives the analyze decision from normalized grammar structure and previously established semantic facts.
fn analyze_decision(
    left: DecisionState,
    right: DecisionState,
    first_sequences: &FirstSequenceAnalysis<'_>,
    bounded_witness: BoundedDecisionWitness,
) -> DecisionOutcome {
    let left = first_sequences.canonical_state(left);
    let right = first_sequences.canonical_state(right);
    let start = paired_residual(left, right, first_sequences.grammar);
    let bounded_proves_complete_overlap = matches!(
        &bounded_witness,
        BoundedDecisionWitness::Exact(Some(witness))
            if witness
                .iter()
                .any(|position| matches!(position, DecisionWitness::End))
    );
    let sufficient_depth = if bounded_proves_complete_overlap {
        None
    } else {
        #[cfg(test)]
        CONFLICT_DECISION_PRODUCT_BUILDS.set(CONFLICT_DECISION_PRODUCT_BUILDS.get() + 1);
        match build_decision_product(start.clone(), first_sequences) {
            DecisionProduct::Cyclic => None,
            DecisionProduct::Acyclic(graph) => {
                Some(longest_decision_path(&start, &graph, &mut BTreeMap::new()) + 1)
            }
        }
    };
    let witness_path = match bounded_witness {
        BoundedDecisionWitness::Unsupported => {
            first_sequences.decision_witness_at_depth(&start, first_sequences.grammar.max_lookahead)
        }
        BoundedDecisionWitness::Exact(witness) => witness,
    }
    .unwrap_or_default();
    let end_of_alternative = witness_path
        .iter()
        .any(|witness| matches!(witness, DecisionWitness::End));
    let witness = witness_path
        .into_iter()
        .filter_map(|witness| match witness {
            DecisionWitness::Token(position) => Some(position),
            DecisionWitness::End => None,
        })
        .collect();
    DecisionOutcome {
        sufficient_depth,
        witness,
        end_of_alternative,
    }
}

/// Freezes validated dispatch, container, and failure decisions for generation.
fn build_decision_product(
    start: PairedDecisionState,
    first_sequences: &FirstSequenceAnalysis<'_>,
) -> DecisionProduct {
    let grammar = first_sequences.grammar;
    let summary_facts = &first_sequences.decision_summary_facts;
    if paired_state_has_complete_overlap(&start, grammar, summary_facts) {
        return DecisionProduct::Cyclic;
    }
    let mut graph = BTreeMap::new();
    let mut parents = DecisionProductParents::new(start);
    let mut queue = VecDeque::from([0]);
    let mut ancestor_scratch = DecisionVisitScratch::default();
    let mut path_scratch = DecisionVisitScratch::default();
    let mut depth_first_burst = 64usize;
    while let Some(state_id) = if depth_first_burst == 0 {
        queue.pop_front()
    } else {
        depth_first_burst -= 1;
        queue.pop_back()
    } {
        #[cfg(test)]
        DECISION_PRODUCT_STATE_EXPANSIONS.set(DECISION_PRODUCT_STATE_EXPANSIONS.get() + 1);
        let state = parents.states[state_id].clone();
        let paired_successors = if let Some(cached) = &parents.successors[state_id] {
            if cached.iter().any(|successor| {
                paired_state_has_complete_overlap(&successor.state, grammar, summary_facts)
                    || decision_successor_closes_cycle(
                        state_id,
                        successor,
                        &parents,
                        &mut ancestor_scratch,
                        &mut path_scratch,
                    )
            }) {
                return DecisionProduct::Cyclic;
            }
            cached.clone()
        } else {
            let mut frontier = |state: &DecisionState| first_sequences.decision_frontier(state);
            let Some(generated) =
                paired_decision_successors_until(&state, grammar, &mut frontier, |successor| {
                    paired_state_has_complete_overlap(&successor.state, grammar, summary_facts)
                        || decision_successor_closes_cycle(
                            state_id,
                            successor,
                            &parents,
                            &mut ancestor_scratch,
                            &mut path_scratch,
                        )
                })
            else {
                return DecisionProduct::Cyclic;
            };
            parents.successors[state_id] = Some(generated.clone());
            generated
        };
        let mut successors = Vec::new();
        for successor in paired_successors {
            let successor_id = parents.intern(successor.state.clone());
            let parent = DecisionParent {
                state: state_id,
                left_minimum_stack_depth: successor.left_minimum_stack_depth,
                right_minimum_stack_depth: successor.right_minimum_stack_depth,
            };
            let incoming = &mut parents.incoming[successor_id];
            if incoming.binary_search(&parent).is_err() {
                incoming.push(parent);
                incoming.sort();
                // A new derivation to an existing residual can establish a
                // boundary-safe pumping proof that its first derivation did
                // not. Revisit that residual with the expanded parent graph.
                queue.push_back(successor_id);
            }
            successors.push((successor.state, successor.witness));
        }
        graph.insert(state, successors);
    }
    DecisionProduct::Acyclic(graph)
}

/// Proves that a paired residual accepts at least one identical complete token sequence.
fn paired_state_has_complete_overlap(
    state: &PairedDecisionState,
    grammar: &GrammarDescription,
    summary_facts: &DecisionSummaryFacts,
) -> bool {
    let PairedDecisionState::Residual(left, right) = state else {
        return true;
    };
    states_have_productive_common_prefix_with_nullable_suffixes(left, right, grammar, summary_facts)
}

/// Proves both states accept one shared productive prefix followed by nullable suffixes.
fn states_have_productive_common_prefix_with_nullable_suffixes(
    left: &DecisionState,
    right: &DecisionState,
    grammar: &GrammarDescription,
    summary_facts: &DecisionSummaryFacts,
) -> bool {
    let common_length = left
        .0
        .iter()
        .zip(&right.0)
        .take_while(|(left, right)| decision_items_equivalent(left, right, grammar))
        .count();
    let common = DecisionState(left.0[..common_length].to_vec());
    let left_suffix = DecisionState(left.0[common_length..].to_vec());
    let right_suffix = DecisionState(right.0[common_length..].to_vec());
    summary_facts.state_is_productive(&common)
        && summary_facts.state_is_nullable(&left_suffix)
        && summary_facts.state_is_nullable(&right_suffix)
}

/// Proves `shorter` is productive and `longer` adds only a nullable semantic suffix.
#[cfg(test)]
fn state_is_productive_prefix_with_nullable_extension(
    shorter: &DecisionState,
    longer: &DecisionState,
    grammar: &GrammarDescription,
    summary_facts: &DecisionSummaryFacts,
) -> bool {
    shorter.0.len() <= longer.0.len()
        && states_have_productive_common_prefix_with_nullable_suffixes(
            shorter,
            longer,
            grammar,
            summary_facts,
        )
        && shorter
            .0
            .iter()
            .zip(&longer.0)
            .all(|(left, right)| decision_items_equivalent(left, right, grammar))
}

/// Reports whether a successor repeats or safely self-embeds one product ancestor.
fn decision_successor_closes_cycle(
    current: usize,
    successor: &PairedDecisionSuccessor,
    parents: &DecisionProductParents,
    ancestor_scratch: &mut DecisionVisitScratch,
    path_scratch: &mut DecisionVisitScratch,
) -> bool {
    ancestor_scratch.start(current, parents.states.len());
    while let Some(candidate) = ancestor_scratch.queue.pop_front() {
        #[cfg(test)]
        DECISION_PRODUCT_ANCESTOR_STATE_VISITS.set(
            DECISION_PRODUCT_ANCESTOR_STATE_VISITS
                .get()
                .saturating_add(1),
        );
        if !ancestor_scratch.visit(candidate) {
            continue;
        }
        let candidate_state = &parents.states[candidate];
        if candidate_state == &successor.state
            || (paired_residual_self_embeds(candidate_state, &successor.state)
                && path_preserves_continuations(
                    candidate,
                    current,
                    successor,
                    parents,
                    path_scratch,
                ))
        {
            return true;
        }
        ancestor_scratch.queue.extend(
            parents.incoming[candidate]
                .iter()
                .map(|parent| parent.state),
        );
    }
    false
}

/// Derives the pumping ancestor from normalized grammar structure and previously established semantic facts.
#[cfg(test)]
fn pumping_ancestor(
    current: usize,
    successor: &PairedDecisionSuccessor,
    parents: &DecisionProductParents,
) -> Option<usize> {
    let mut ancestor_scratch = DecisionVisitScratch::default();
    let mut path_scratch = DecisionVisitScratch::default();
    ancestor_scratch.start(current, parents.states.len());
    while let Some(candidate) = ancestor_scratch.queue.pop_front() {
        #[cfg(test)]
        DECISION_PRODUCT_ANCESTOR_STATE_VISITS.set(
            DECISION_PRODUCT_ANCESTOR_STATE_VISITS
                .get()
                .saturating_add(1),
        );
        if !ancestor_scratch.visit(candidate) {
            continue;
        }
        if paired_residual_self_embeds(&parents.states[candidate], &successor.state)
            && path_preserves_continuations(
                candidate,
                current,
                successor,
                parents,
                &mut path_scratch,
            )
        {
            return Some(candidate);
        }
        ancestor_scratch.queue.extend(
            parents.incoming[candidate]
                .iter()
                .map(|parent| parent.state),
        );
    }
    None
}

/// Derives the paired residual self embeds from normalized grammar structure and previously established semantic facts.
fn paired_residual_self_embeds(
    ancestor: &PairedDecisionState,
    successor: &PairedDecisionState,
) -> bool {
    let (
        PairedDecisionState::Residual(ancestor_left, ancestor_right),
        PairedDecisionState::Residual(successor_left, successor_right),
    ) = (ancestor, successor)
    else {
        return false;
    };
    residual_self_embeds(ancestor_left, successor_left)
        && residual_self_embeds(ancestor_right, successor_right)
        && (ancestor_left.0.len() < successor_left.0.len()
            || ancestor_right.0.len() < successor_right.0.len())
}

/// Derives the residual self embeds from normalized grammar structure and previously established semantic facts.
fn residual_self_embeds(ancestor: &DecisionState, successor: &DecisionState) -> bool {
    let Some((active, continuation)) = ancestor.0.split_first() else {
        return false;
    };
    successor.0.first() == Some(active)
        && successor.0.len() >= ancestor.0.len()
        && successor.0.ends_with(continuation)
}

/// Derives the path preserves continuations from normalized grammar structure and previously established semantic facts.
fn path_preserves_continuations(
    ancestor: usize,
    current: usize,
    successor: &PairedDecisionSuccessor,
    parents: &DecisionProductParents,
    scratch: &mut DecisionVisitScratch,
) -> bool {
    let PairedDecisionState::Residual(ancestor_left, ancestor_right) = &parents.states[ancestor]
    else {
        return false;
    };
    let left_continuation = &ancestor_left.0[1..];
    let right_continuation = &ancestor_right.0[1..];
    if successor.left_minimum_stack_depth <= left_continuation.len()
        || successor.right_minimum_stack_depth <= right_continuation.len()
    {
        return false;
    }
    scratch.start(current, parents.states.len());
    while let Some(state) = scratch.queue.pop_front() {
        if !scratch.visit(state) {
            continue;
        }
        if state == ancestor {
            return true;
        }
        let PairedDecisionState::Residual(left, right) = &parents.states[state] else {
            continue;
        };
        if !left.0.ends_with(left_continuation) || !right.0.ends_with(right_continuation) {
            continue;
        }
        for parent in &parents.incoming[state] {
            if parent.left_minimum_stack_depth > left_continuation.len()
                && parent.right_minimum_stack_depth > right_continuation.len()
            {
                scratch.queue.push_back(parent.state);
            }
        }
    }
    false
}

/// Derives paired successors from a caller-provided single-side frontier cache.
fn paired_decision_successors_with(
    state: &PairedDecisionState,
    grammar: &GrammarDescription,
    frontier: &mut impl FnMut(&DecisionState) -> Rc<DecisionFrontier>,
) -> Vec<PairedDecisionSuccessor> {
    paired_decision_successors_until(state, grammar, frontier, |_| false)
        .expect("an unconditional paired-successor traversal cannot stop")
}

/// Derives paired successors while allowing an exact semantic proof to stop a wide symbolic cell early.
fn paired_decision_successors_until(
    state: &PairedDecisionState,
    grammar: &GrammarDescription,
    frontier: &mut impl FnMut(&DecisionState) -> Rc<DecisionFrontier>,
    mut stop: impl FnMut(&PairedDecisionSuccessor) -> bool,
) -> Option<Vec<PairedDecisionSuccessor>> {
    if matches!(state, PairedDecisionState::End) {
        let successor = PairedDecisionSuccessor {
            state: PairedDecisionState::End,
            witness: DecisionWitness::End,
            left_minimum_stack_depth: 0,
            right_minimum_stack_depth: 0,
        };
        if stop(&successor) {
            return None;
        }
        return Some(vec![successor]);
    }
    let PairedDecisionState::Residual(left, right) = state else {
        unreachable!()
    };
    let left_frontier = frontier(left);
    let right_frontier = frontier(right);
    let mut successors = Vec::new();
    if left_frontier.can_end && right_frontier.can_end {
        let successor = PairedDecisionSuccessor {
            state: PairedDecisionState::End,
            witness: DecisionWitness::End,
            left_minimum_stack_depth: 0,
            right_minimum_stack_depth: 0,
        };
        if stop(&successor) {
            return None;
        }
        successors.push(successor);
    }

    let candidates = left_frontier
        .transitions
        .iter()
        .enumerate()
        .map(|(index, _)| (false, index))
        .chain(
            right_frontier
                .transitions
                .iter()
                .enumerate()
                .map(|(index, _)| (true, index)),
        )
        .collect::<Vec<_>>();
    let transition = |candidate: &(bool, usize)| {
        if candidate.0 {
            &right_frontier.transitions[candidate.1]
        } else {
            &left_frontier.transitions[candidate.1]
        }
    };
    let cells = partition_by_kind_residual(
        &candidates,
        grammar,
        |candidate| transition(candidate).position.text_predicate,
        |candidate| &transition(candidate).position.kinds,
        |candidate| *candidate,
    );
    let mut overlapping = BTreeMap::<(usize, usize), KindId>::new();
    for (_, members, kinds) in cells {
        let left_members = members
            .iter()
            .filter_map(|(right, index)| (!right).then_some(*index))
            .collect::<Vec<_>>();
        let right_members = members
            .iter()
            .filter_map(|(right, index)| right.then_some(*index))
            .collect::<Vec<_>>();
        let Some(kind) = kinds.iter().next() else {
            continue;
        };
        for left in &left_members {
            for right in &right_members {
                #[cfg(test)]
                DECISION_PRODUCT_TRANSITION_PAIRS.set(DECISION_PRODUCT_TRANSITION_PAIRS.get() + 1);
                let left_transition = &left_frontier.transitions[*left];
                let right_transition = &right_frontier.transitions[*right];
                let successor = PairedDecisionSuccessor {
                    state: paired_residual(
                        left_transition.next.clone(),
                        right_transition.next.clone(),
                        grammar,
                    ),
                    witness: DecisionWitness::Token(crate::LookaheadWitnessPosition {
                        kind,
                        left_text_predicate: left_transition.position.text_predicate,
                        right_text_predicate: right_transition.position.text_predicate,
                    }),
                    left_minimum_stack_depth: left_transition.minimum_stack_depth,
                    right_minimum_stack_depth: right_transition.minimum_stack_depth,
                };
                if stop(&successor) {
                    return None;
                }
                overlapping
                    .entry((*left, *right))
                    .and_modify(|known| *known = (*known).min(kind))
                    .or_insert(kind);
            }
        }
    }
    for ((left, right), kind) in overlapping {
        let left = &left_frontier.transitions[left];
        let right = &right_frontier.transitions[right];
        successors.push(PairedDecisionSuccessor {
            state: paired_residual(left.next.clone(), right.next.clone(), grammar),
            witness: DecisionWitness::Token(crate::LookaheadWitnessPosition {
                kind,
                left_text_predicate: left.position.text_predicate,
                right_text_predicate: right.position.text_predicate,
            }),
            left_minimum_stack_depth: left.minimum_stack_depth,
            right_minimum_stack_depth: right.minimum_stack_depth,
        });
    }
    successors.sort();
    successors.dedup();
    Some(successors)
}

/// Derives the paired residual from normalized grammar structure and previously established semantic facts.
fn paired_residual(
    mut left: DecisionState,
    mut right: DecisionState,
    grammar: &GrammarDescription,
) -> PairedDecisionState {
    while left.0.len() == right.0.len()
        && left.0.len() > 1
        && right.0.len() > 1
        && left
            .0
            .last()
            .zip(right.0.last())
            .is_some_and(|(left, right)| decision_items_equivalent(left, right, grammar))
    {
        left.0.pop();
        right.0.pop();
    }
    PairedDecisionState::Residual(left, right)
}

/// Derives the decision items equivalent from normalized grammar structure and previously established semantic facts.
fn decision_items_equivalent(
    left: &DecisionItem,
    right: &DecisionItem,
    grammar: &GrammarDescription,
) -> bool {
    match (left, right) {
        (DecisionItem::Type(left), DecisionItem::Type(right)) => left == right,
        (DecisionItem::Site(left), DecisionItem::Site(right)) => {
            let left = &grammar.sites[left.index()];
            let right = &grammar.sites[right.index()];
            decision_site_semantics(left) == decision_site_semantics(right)
        }
        (
            DecisionItem::RepeatTail {
                target: left_target,
                separator: left_separator,
                optional_trailing: left_optional_trailing,
            },
            DecisionItem::RepeatTail {
                target: right_target,
                separator: right_separator,
                optional_trailing: right_optional_trailing,
            },
        ) => {
            left_target == right_target
                && left_separator == right_separator
                && left_optional_trailing == right_optional_trailing
        }
        _ => false,
    }
}

/// Derives the decision frontier from normalized grammar structure and previously established semantic facts.
fn walk_decision_frontier(
    start: &DecisionState,
    grammar: &GrammarDescription,
    mut visit: impl FnMut(DecisionFrontierEvent<'_>) -> bool,
) {
    // Explore the bounded lookahead product while deduplicating frontier states to terminate recursive shared prefixes.
    let mut best_minimum_depth = BTreeMap::from([(start.clone(), start.0.len())]);
    let mut queue = VecDeque::from([(start.clone(), start.0.len())]);
    while let Some((state, minimum_stack_depth)) = queue.pop_front() {
        #[cfg(test)]
        FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.set(
            FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS
                .get()
                .saturating_add(1),
        );
        if best_minimum_depth.get(&state) != Some(&minimum_stack_depth) {
            continue;
        }
        let Some((first, rest)) = state.0.split_first() else {
            if !visit(DecisionFrontierEvent::End) {
                return;
            }
            continue;
        };
        match first {
            DecisionItem::Type(type_id) => {
                let description = &grammar.types[type_id.index()];
                match &description.shape {
                    TypeShape::Token {
                        atom,
                        text_predicate,
                    } => {
                        if !visit(DecisionFrontierEvent::Transition {
                            atom: *atom,
                            text_predicate: *text_predicate,
                            rest,
                            minimum_stack_depth,
                        }) {
                            return;
                        }
                    }
                    TypeShape::Alias { site } => queue_decision_expansion(
                        vec![DecisionItem::Site(*site)],
                        rest,
                        minimum_stack_depth,
                        &mut best_minimum_depth,
                        &mut queue,
                    ),
                    TypeShape::Sequence { sites } => queue_decision_expansion(
                        sites.iter().copied().map(DecisionItem::Site).collect(),
                        rest,
                        minimum_stack_depth,
                        &mut best_minimum_depth,
                        &mut queue,
                    ),
                    TypeShape::Enum { alternatives } => {
                        for alternative in alternatives {
                            queue_decision_expansion(
                                alternative
                                    .sites
                                    .iter()
                                    .copied()
                                    .map(DecisionItem::Site)
                                    .collect(),
                                rest,
                                minimum_stack_depth,
                                &mut best_minimum_depth,
                                &mut queue,
                            );
                        }
                    }
                    TypeShape::Pratt { variants } => {
                        for variant in variants.iter().filter(|variant| {
                            matches!(
                                variant.position,
                                PrattPosition::Atom | PrattPosition::Prefix { .. }
                            )
                        }) {
                            queue_decision_expansion(
                                variant
                                    .sites
                                    .iter()
                                    .copied()
                                    .map(DecisionItem::Site)
                                    .collect(),
                                rest,
                                minimum_stack_depth,
                                &mut best_minimum_depth,
                                &mut queue,
                            );
                        }
                    }
                }
            }
            DecisionItem::Site(site_id) => {
                let site = &grammar.sites[site_id.index()];
                let semantics = container_semantics(site);
                match semantics.cardinality {
                    ContainerCardinality::One => queue_decision_expansion(
                        vec![DecisionItem::Type(site.target)],
                        rest,
                        minimum_stack_depth,
                        &mut best_minimum_depth,
                        &mut queue,
                    ),
                    ContainerCardinality::Optional => {
                        queue_decision_expansion(
                            Vec::new(),
                            rest,
                            minimum_stack_depth,
                            &mut best_minimum_depth,
                            &mut queue,
                        );
                        queue_decision_expansion(
                            vec![DecisionItem::Type(site.target)],
                            rest,
                            minimum_stack_depth,
                            &mut best_minimum_depth,
                            &mut queue,
                        );
                    }
                    ContainerCardinality::ZeroOrMore
                    | ContainerCardinality::OneOrMore
                    | ContainerCardinality::OptionalOneOrMore => {
                        if matches!(
                            semantics.cardinality,
                            ContainerCardinality::ZeroOrMore
                                | ContainerCardinality::OptionalOneOrMore
                        ) {
                            queue_decision_expansion(
                                Vec::new(),
                                rest,
                                minimum_stack_depth,
                                &mut best_minimum_depth,
                                &mut queue,
                            );
                        }
                        queue_decision_expansion(
                            vec![
                                DecisionItem::Type(site.target),
                                DecisionItem::RepeatTail {
                                    target: site.target,
                                    separator: semantics.separator_type(),
                                    optional_trailing: semantics
                                        .separator
                                        .is_some_and(|separator| separator.optional_trailing),
                                },
                            ],
                            rest,
                            minimum_stack_depth,
                            &mut best_minimum_depth,
                            &mut queue,
                        );
                    }
                }
            }
            DecisionItem::RepeatTail {
                target,
                separator,
                optional_trailing,
            } => {
                queue_decision_expansion(
                    Vec::new(),
                    rest,
                    minimum_stack_depth,
                    &mut best_minimum_depth,
                    &mut queue,
                );
                let mut expansion = Vec::new();
                if let Some(separator) = separator {
                    expansion.push(DecisionItem::Type(*separator));
                }
                expansion.extend([
                    DecisionItem::Type(*target),
                    DecisionItem::RepeatTail {
                        target: *target,
                        separator: *separator,
                        optional_trailing: *optional_trailing,
                    },
                ]);
                queue_decision_expansion(
                    expansion,
                    rest,
                    minimum_stack_depth,
                    &mut best_minimum_depth,
                    &mut queue,
                );
                if *optional_trailing {
                    let separator =
                        separator.expect("optional trailing behavior requires a separator type");
                    queue_decision_expansion(
                        vec![DecisionItem::Type(separator)],
                        rest,
                        minimum_stack_depth,
                        &mut best_minimum_depth,
                        &mut queue,
                    );
                }
            }
        }
    }
}

/// Derives the decision frontier from normalized grammar structure and previously established semantic facts.
fn decision_frontier(
    start: &DecisionState,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
) -> DecisionFrontier {
    let mut frontier = DecisionFrontier::default();
    walk_decision_frontier(start, grammar, |event| {
        match event {
            DecisionFrontierEvent::End => frontier.can_end = true,
            DecisionFrontierEvent::Transition {
                atom,
                text_predicate,
                rest,
                minimum_stack_depth,
            } => {
                #[cfg(test)]
                FIRST_LANGUAGE_DECISION_TRANSITION_MATERIALIZATIONS.set(
                    FIRST_LANGUAGE_DECISION_TRANSITION_MATERIALIZATIONS
                        .get()
                        .saturating_add(1),
                );
                frontier.transitions.push(DecisionTransition {
                    position: FirstPosition {
                        kinds: atom_kinds[atom.index()].clone(),
                        text_predicate,
                    },
                    next: DecisionState(rest.to_vec()),
                    minimum_stack_depth,
                });
            }
        }
        true
    });
    frontier.transitions.sort_by(|left, right| {
        compare_first_positions(&left.position, &right.position)
            .then_with(|| left.next.cmp(&right.next))
            .then_with(|| right.minimum_stack_depth.cmp(&left.minimum_stack_depth))
    });
    frontier
        .transitions
        .dedup_by(|left, right| left.position == right.position && left.next == right.next);
    frontier
}

/// Derives only the exact-end and continuation facts required at the lookahead cutoff.
fn decision_cutoff_summary(start: &DecisionState, facts: &DecisionSummaryFacts) -> u8 {
    facts.summarize_state(start)
}

/// Derives the queue decision expansion from normalized grammar structure and previously established semantic facts.
fn queue_decision_expansion(
    mut expansion: Vec<DecisionItem>,
    rest: &[DecisionItem],
    minimum_stack_depth: usize,
    best_minimum_depth: &mut BTreeMap<DecisionState, usize>,
    queue: &mut VecDeque<(DecisionState, usize)>,
) {
    expansion.extend_from_slice(rest);
    let state = DecisionState(expansion);
    let minimum_stack_depth = minimum_stack_depth.min(state.0.len());
    if best_minimum_depth
        .get(&state)
        .is_none_or(|best| *best < minimum_stack_depth)
    {
        best_minimum_depth.insert(state.clone(), minimum_stack_depth);
        queue.push_back((state, minimum_stack_depth));
    }
}

/// Orders FIRST positions by token kind and exact-text predicate.
fn compare_first_positions(left: &FirstPosition, right: &FirstPosition) -> std::cmp::Ordering {
    left.kinds
        .kind_count()
        .cmp(&right.kinds.kind_count())
        .then_with(|| left.kinds.words().cmp(right.kinds.words()))
        .then_with(|| left.text_predicate.cmp(&right.text_predicate))
}

/// Derives the longest decision path from normalized grammar structure and previously established semantic facts.
fn longest_decision_path(
    state: &PairedDecisionState,
    graph: &BTreeMap<PairedDecisionState, Vec<(PairedDecisionState, DecisionWitness)>>,
    memo: &mut BTreeMap<PairedDecisionState, usize>,
) -> usize {
    if let Some(length) = memo.get(state) {
        return *length;
    }
    let length = graph[state]
        .iter()
        .map(|(successor, _)| 1 + longest_decision_path(successor, graph, memo))
        .max()
        .unwrap_or(0);
    memo.insert(state.clone(), length);
    length
}

/// Derives the decision witness at depth from normalized grammar structure and previously established semantic facts.
#[cfg(test)]
fn decision_witness_at_depth(
    start: &PairedDecisionState,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
    depth: usize,
) -> Option<Vec<DecisionWitness>> {
    let mut frontier =
        |state: &DecisionState| Rc::new(decision_frontier(state, grammar, atom_kinds));
    canonical_decision_witness_suffix(start, grammar, depth, &mut BTreeMap::new(), &mut frontier)
}

/// Derives the lexicographically first fixed-depth witness without enumerating unrelated paths.
fn canonical_decision_witness_suffix(
    state: &PairedDecisionState,
    grammar: &GrammarDescription,
    remaining: usize,
    memo: &mut BTreeMap<(Vec<PairedDecisionState>, usize), Option<Vec<DecisionWitness>>>,
    frontier: &mut impl FnMut(&DecisionState) -> Rc<DecisionFrontier>,
) -> Option<Vec<DecisionWitness>> {
    canonical_decision_witness_subset(
        std::slice::from_ref(state),
        grammar,
        remaining,
        memo,
        frontier,
    )
}

/// Derives one canonical witness while merging residuals reached by the same observed prefix.
fn canonical_decision_witness_subset(
    states: &[PairedDecisionState],
    grammar: &GrammarDescription,
    remaining: usize,
    memo: &mut BTreeMap<(Vec<PairedDecisionState>, usize), Option<Vec<DecisionWitness>>>,
    frontier: &mut impl FnMut(&DecisionState) -> Rc<DecisionFrontier>,
) -> Option<Vec<DecisionWitness>> {
    if remaining == 0 {
        return Some(Vec::new());
    }
    let key = (states.to_vec(), remaining);
    if let Some(witness) = memo.get(&key) {
        return witness.clone();
    }
    if let [PairedDecisionState::Residual(left, right)] = states
        && left == right
    {
        let result = canonical_single_decision_witness_suffix(
            left,
            remaining,
            &mut BTreeMap::new(),
            frontier,
        );
        memo.insert(key, result.clone());
        return result;
    }
    let mut successors_by_witness = BTreeMap::<DecisionWitness, BTreeSet<_>>::new();
    for state in states {
        #[cfg(test)]
        DECISION_WITNESS_STATE_EXPANSIONS
            .set(DECISION_WITNESS_STATE_EXPANSIONS.get().saturating_add(1));
        for successor in paired_decision_successors_with(state, grammar, frontier) {
            successors_by_witness
                .entry(successor.witness)
                .or_default()
                .insert(successor.state);
        }
    }
    for (witness, successors) in successors_by_witness {
        if matches!(witness, DecisionWitness::End) {
            let result = Some(vec![DecisionWitness::End]);
            memo.insert(key, result.clone());
            return result;
        }
        let successors = successors.into_iter().collect::<Vec<_>>();
        if let Some(suffix) =
            canonical_decision_witness_subset(&successors, grammar, remaining - 1, memo, frontier)
        {
            let mut result = Vec::with_capacity(1 + suffix.len());
            result.push(witness);
            result.extend(suffix);
            memo.insert(key, Some(result.clone()));
            return Some(result);
        }
    }
    memo.insert(key, None);
    None
}

/// Derives the canonical prefix of one language without constructing its Cartesian self-product.
fn canonical_single_decision_witness_suffix(
    state: &DecisionState,
    remaining: usize,
    memo: &mut BTreeMap<(DecisionState, usize), Option<Vec<DecisionWitness>>>,
    frontier: &mut impl FnMut(&DecisionState) -> Rc<DecisionFrontier>,
) -> Option<Vec<DecisionWitness>> {
    if remaining == 0 {
        return Some(Vec::new());
    }
    let key = (state.clone(), remaining);
    if let Some(witness) = memo.get(&key) {
        return witness.clone();
    }
    let state_frontier = frontier(state);
    let mut successors_by_witness = BTreeMap::<DecisionWitness, BTreeSet<DecisionState>>::new();
    for transition in &state_frontier.transitions {
        successors_by_witness
            .entry(DecisionWitness::Token(crate::LookaheadWitnessPosition {
                kind: transition
                    .position
                    .kinds
                    .iter()
                    .next()
                    .expect("a decision transition consumes at least one kind"),
                left_text_predicate: transition.position.text_predicate,
                right_text_predicate: transition.position.text_predicate,
            }))
            .or_default()
            .insert(transition.next.clone());
    }
    for (witness, successors) in successors_by_witness {
        let suffix = successors
            .into_iter()
            .filter_map(|successor| {
                canonical_single_decision_witness_suffix(&successor, remaining - 1, memo, frontier)
            })
            .min();
        if let Some(suffix) = suffix {
            let mut result = Vec::with_capacity(1 + suffix.len());
            result.push(witness);
            result.extend(suffix);
            memo.insert(key, Some(result.clone()));
            return Some(result);
        }
    }
    let result = state_frontier.can_end.then_some(vec![DecisionWitness::End]);
    memo.insert(key, result.clone());
    result
}

/// Produces canonical ambiguity diagnostics from overlapping semantic decisions.
fn detect_conflicts(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_sequences: &FirstSequenceAnalysis<'_>,
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
    static_conflict_kinds_by_declaration: &mut [KindSet],
) -> Vec<Diagnostic> {
    // Collect static conflicts before caller-FOLLOW checks so diagnostics preserve their semantic phase and remedy.
    let mut diagnostics = Vec::new();
    for (index, description) in grammar.types.iter().enumerate() {
        let owner = TypeId::new(index as u32);
        match &description.shape {
            TypeShape::Enum { alternatives } => {
                let mut alternatives = alternatives.iter().collect::<Vec<_>>();
                alternatives.sort_by_key(|alternative| alternative.declaration);
                for left_index in 0..alternatives.len() {
                    for right_index in left_index + 1..alternatives.len() {
                        let left = alternatives[left_index];
                        let right = alternatives[right_index];
                        let left_first = first_for_sites_provenance(
                            owner,
                            &left.sites,
                            grammar,
                            semantic,
                            provenanced,
                            proofs,
                        );
                        let right_first = first_for_sites_provenance(
                            owner,
                            &right.sites,
                            grammar,
                            semantic,
                            provenanced,
                            proofs,
                        );
                        let evidence = overlap_evidence(
                            &left_first.positions,
                            &right_first.positions,
                            grammar,
                            proofs,
                        );
                        if evidence.is_none() && !(left_first.nullable && right_first.nullable) {
                            continue;
                        }
                        let bounded_witness = bounded_site_language_witness(
                            &left.sites,
                            &right.sites,
                            grammar,
                            first_sequences,
                        );
                        if bounded_witness.is_disjoint() {
                            continue;
                        }
                        let decision = analyze_decision(
                            DecisionState::from_sites(&left.sites),
                            DecisionState::from_sites(&right.sites),
                            first_sequences,
                            bounded_witness,
                        );
                        if decision
                            .sufficient_depth
                            .is_some_and(|depth| depth <= grammar.max_lookahead)
                        {
                            continue;
                        }
                        let sufficient_depth = decision.sufficient_depth;
                        let remedy = sufficient_depth.map_or(
                            ConflictRemedy::RestructureSharedPrefix {
                                left: left.declaration,
                                right: right.declaration,
                            },
                            |sufficient| ConflictRemedy::RaiseMaxLookahead {
                                configured: grammar.max_lookahead,
                                sufficient,
                            },
                        );
                        let mut diagnostic = Diagnostic::shared_prefix_conflict(
                            DiagnosticCode::EnumFirstOverlap,
                            format!(
                                "enum `{}` has alternatives with an unresolved shared prefix",
                                description.name,
                            ),
                            declaration_label(
                                left.declaration,
                                grammar,
                                "this alternative can start with the witness kind",
                            ),
                            declaration_label(
                                right.declaration,
                                grammar,
                                "this alternative can start with the same kind",
                            ),
                            evidence,
                            remedy,
                        );
                        diagnostic.lookahead = Some(crate::SharedPrefixEvidence {
                            configured_depth: grammar.max_lookahead,
                            sufficient_depth,
                            witness: decision.witness,
                            end_of_alternative: decision.end_of_alternative,
                        });
                        diagnostics.push(diagnostic);
                    }
                }
            }
            TypeShape::Sequence { sites } => detect_static_conflicts_for_sites(
                owner,
                sites,
                grammar,
                semantic,
                first_sequences,
                provenanced,
                proofs,
                &mut diagnostics,
                static_conflict_kinds_by_declaration,
            ),
            TypeShape::Pratt { variants } => {
                detect_pratt_conflicts(
                    owner,
                    &description.name,
                    variants,
                    grammar,
                    semantic,
                    first_sequences,
                    provenanced,
                    proofs,
                    &mut diagnostics,
                );
                let mut variants = variants.iter().collect::<Vec<_>>();
                variants
                    .sort_by_key(|variant| declaration_source_key(variant.declaration, grammar));
                for variant in variants {
                    detect_static_conflicts_for_sites(
                        owner,
                        &variant.sites,
                        grammar,
                        semantic,
                        first_sequences,
                        provenanced,
                        proofs,
                        &mut diagnostics,
                        static_conflict_kinds_by_declaration,
                    );
                }
            }
            TypeShape::Alias { .. } | TypeShape::Token { .. } => {}
        }
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic_source_key(diagnostic, grammar));
    diagnostics
}

/// Derives a configured-depth witness from exact paired decision residuals.
fn bounded_site_language_witness(
    left: &[SiteId],
    right: &[SiteId],
    grammar: &GrammarDescription,
    first_sequences: &FirstSequenceAnalysis<'_>,
) -> BoundedDecisionWitness {
    if left.iter().chain(right).any(|site| {
        let site = &grammar.sites[site.index()];
        site.min_bp != 0 || site.container.semantics().is_none()
    }) {
        return BoundedDecisionWitness::Unsupported;
    }
    let left_state = first_sequences.canonical_state(DecisionState::from_sites(left));
    let right_state = first_sequences.canonical_state(DecisionState::from_sites(right));
    if !first_sequences
        .decision_summary_facts
        .state_is_productive(&left_state)
        || !first_sequences
            .decision_summary_facts
            .state_is_productive(&right_state)
        || first_sequences
            .decision_summary_facts
            .state_has_dead_prefix(&left_state)
        || first_sequences
            .decision_summary_facts
            .state_has_dead_prefix(&right_state)
    {
        return BoundedDecisionWitness::Unsupported;
    }
    let paired = paired_residual(left_state.clone(), right_state.clone(), grammar);
    let witness = first_sequences.decision_witness_at_depth(&paired, grammar.max_lookahead);
    BoundedDecisionWitness::Exact(witness)
}

#[allow(clippy::too_many_arguments)]
/// Diagnoses overlapping Pratt roles and extender selections.
fn detect_pratt_conflicts(
    owner: TypeId,
    owner_name: &str,
    variants: &[crate::PrattVariant],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_sequences: &FirstSequenceAnalysis<'_>,
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut nud = variants
        .iter()
        .filter(|variant| is_pratt_nud(variant.position))
        .collect::<Vec<_>>();
    nud.sort_by_key(|variant| declaration_source_key(variant.declaration, grammar));
    detect_pratt_group_conflicts(
        owner,
        owner_name,
        &nud,
        DiagnosticCode::PrattAtomOverlap,
        grammar,
        semantic,
        first_sequences,
        provenanced,
        proofs,
        diagnostics,
    );

    let mut led = variants
        .iter()
        .filter(|variant| is_pratt_led(variant.position))
        .collect::<Vec<_>>();
    led.sort_by_key(|variant| declaration_source_key(variant.declaration, grammar));
    detect_pratt_group_conflicts(
        owner,
        owner_name,
        &led,
        DiagnosticCode::PrattLedOverlap,
        grammar,
        semantic,
        first_sequences,
        provenanced,
        proofs,
        diagnostics,
    );
}

#[allow(clippy::too_many_arguments)]
/// Detects pratt group conflicts.
fn detect_pratt_group_conflicts(
    owner: TypeId,
    owner_name: &str,
    variants: &[&crate::PrattVariant],
    code: DiagnosticCode,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_sequences: &FirstSequenceAnalysis<'_>,
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Compare Pratt groups as units because member-wise conflicts would lose the binding-power context.
    for left_index in 0..variants.len() {
        for right_index in left_index + 1..variants.len() {
            let left = variants[left_index];
            let right = variants[right_index];
            let left_first = if is_pratt_led(left.position) {
                pratt_extender_provenance(owner, left, grammar, semantic, provenanced, proofs)
            } else {
                first_for_sites_provenance(
                    owner,
                    &left.sites,
                    grammar,
                    semantic,
                    provenanced,
                    proofs,
                )
            };
            let right_first = if is_pratt_led(right.position) {
                pratt_extender_provenance(owner, right, grammar, semantic, provenanced, proofs)
            } else {
                first_for_sites_provenance(
                    owner,
                    &right.sites,
                    grammar,
                    semantic,
                    provenanced,
                    proofs,
                )
            };
            let evidence = overlap_evidence(
                &left_first.positions,
                &right_first.positions,
                grammar,
                proofs,
            );
            if evidence.is_none() && !(left_first.nullable && right_first.nullable) {
                continue;
            }
            let bounded_witness =
                bounded_site_language_witness(&left.sites, &right.sites, grammar, first_sequences);
            if bounded_witness.is_disjoint() {
                continue;
            }
            let decision = analyze_decision(
                DecisionState::from_sites(&left.sites),
                DecisionState::from_sites(&right.sites),
                first_sequences,
                bounded_witness,
            );
            if decision
                .sufficient_depth
                .is_some_and(|depth| depth <= grammar.max_lookahead)
            {
                continue;
            }
            let left_position = pratt_position_name(left.position);
            let right_position = pratt_position_name(right.position);
            let remedy = decision.sufficient_depth.map_or(
                ConflictRemedy::RestructurePrattSharedPrefix {
                    pratt: owner,
                    left: left.declaration,
                    right: right.declaration,
                },
                |sufficient| ConflictRemedy::RaiseMaxLookahead {
                    configured: grammar.max_lookahead,
                    sufficient,
                },
            );
            let (left_label, right_label) = if evidence.is_some() {
                (
                    format!("this {left_position} variant can start with the witness kind"),
                    format!("this {right_position} variant can start with the same kind"),
                )
            } else {
                (
                    format!("this {left_position} variant can end at this decision point"),
                    format!("this {right_position} variant can also end here"),
                )
            };
            let mut diagnostic = Diagnostic::shared_prefix_conflict(
                code,
                format!(
                    "Pratt `{owner_name}` has overlapping {}",
                    pratt_overlap_name(left.position, right.position),
                ),
                declaration_label(left.declaration, grammar, left_label),
                declaration_label(right.declaration, grammar, right_label),
                evidence,
                remedy,
            );
            diagnostic.lookahead = Some(crate::SharedPrefixEvidence {
                configured_depth: grammar.max_lookahead,
                sufficient_depth: decision.sufficient_depth,
                witness: decision.witness,
                end_of_alternative: decision.end_of_alternative,
            });
            diagnostics.push(diagnostic);
        }
    }
}

/// Derives the pratt position name from normalized grammar structure and previously established semantic facts.
const fn pratt_position_name(position: PrattPosition) -> &'static str {
    match position {
        PrattPosition::Atom => "atom",
        PrattPosition::Prefix { .. } => "prefix",
        PrattPosition::Infix { .. } => "infix",
        PrattPosition::Postfix { .. } => "postfix",
    }
}

/// Derives the pratt overlap name from normalized grammar structure and previously established semantic facts.
const fn pratt_overlap_name(left: PrattPosition, right: PrattPosition) -> &'static str {
    match (left, right) {
        (PrattPosition::Atom, PrattPosition::Atom) => "atom variants",
        (PrattPosition::Prefix { .. }, PrattPosition::Prefix { .. }) => "prefix variants",
        (PrattPosition::Atom, PrattPosition::Prefix { .. })
        | (PrattPosition::Prefix { .. }, PrattPosition::Atom) => "atom and prefix variants",
        (PrattPosition::Infix { .. }, PrattPosition::Infix { .. }) => "led variants",
        (PrattPosition::Postfix { .. }, PrattPosition::Postfix { .. }) => "postfix variants",
        (PrattPosition::Infix { .. }, PrattPosition::Postfix { .. })
        | (PrattPosition::Postfix { .. }, PrattPosition::Infix { .. }) => {
            "infix and postfix variants"
        }
        _ => "Pratt variants",
    }
}

#[allow(clippy::too_many_arguments)]
/// Detects static conflicts for sites.
fn detect_static_conflicts_for_sites(
    owner: TypeId,
    sites: &[SiteId],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_sequences: &FirstSequenceAnalysis<'_>,
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
    diagnostics: &mut Vec<Diagnostic>,
    static_conflict_kinds_by_declaration: &mut [KindSet],
) {
    for index in 0..sites.len().saturating_sub(1) {
        let site_id = sites[index];
        let site = &grammar.sites[site_id.index()];
        if !site_nullable(site, &semantic[site.target.index()]) {
            continue;
        }
        let tail = first_for_sites_provenance(
            owner,
            &sites[index + 1..],
            grammar,
            semantic,
            provenanced,
            proofs,
        );
        if tail.nullable {
            continue;
        }
        let current =
            first_for_sites_provenance(owner, &[site_id], grammar, semantic, provenanced, proofs);
        let Some(evidence) = overlap_evidence(&current.positions, &tail.positions, grammar, proofs)
        else {
            continue;
        };
        let tail_sites = &sites[index + 1..];
        let left = DecisionState::consuming_site_and_tail(site, tail_sites);
        let right = DecisionState::from_sites(tail_sites);
        let bounded_witness = bounded_nullable_tail_witness(
            site,
            tail_sites,
            &left,
            &right,
            grammar,
            first_sequences,
        );
        if bounded_witness.is_disjoint() {
            continue;
        }
        let decision = analyze_decision(left, right, first_sequences, bounded_witness);
        if decision
            .sufficient_depth
            .is_some_and(|depth| depth <= grammar.max_lookahead)
        {
            continue;
        }
        let sufficient_depth = decision.sufficient_depth;
        static_conflict_kinds_by_declaration[site.declaration.index()]
            .union_assign(&evidence.overlap);
        let tail_site = sites[index + 1];
        let remedy = sufficient_depth.map_or(
            ConflictRemedy::MakeSiteRequired { site: site_id },
            |sufficient| ConflictRemedy::RaiseMaxLookahead {
                configured: grammar.max_lookahead,
                sufficient,
            },
        );
        let mut diagnostic = Diagnostic::conflict(
            DiagnosticCode::StaticUseSiteOverlap,
            format!(
                "nullable use site in `{}` overlaps its required tail",
                grammar.types[owner.index()].name,
            ),
            declaration_label(
                site.declaration,
                grammar,
                "this use site is nullable and can consume the witness kind",
            ),
            declaration_label(
                grammar.sites[tail_site.index()].declaration,
                grammar,
                "the following required tail can start with the same kind",
            ),
            evidence,
            remedy,
        );
        if let Some(sufficient_depth) = sufficient_depth {
            diagnostic.lookahead = Some(crate::SharedPrefixEvidence {
                configured_depth: grammar.max_lookahead,
                sufficient_depth: Some(sufficient_depth),
                witness: decision.witness,
                end_of_alternative: decision.end_of_alternative,
            });
        }
        diagnostics.push(diagnostic);
    }
}

#[allow(clippy::too_many_arguments)]
/// Derives the exact configured-depth witness for a nullable consuming site against its required tail.
fn bounded_nullable_tail_witness(
    site: &crate::ParseSite,
    tail_sites: &[SiteId],
    left: &DecisionState,
    right: &DecisionState,
    grammar: &GrammarDescription,
    first_sequences: &FirstSequenceAnalysis<'_>,
) -> BoundedDecisionWitness {
    if site.min_bp != 0
        || site.container.semantics().is_none()
        || tail_sites
            .iter()
            .any(|site| grammar.sites[site.index()].min_bp != 0)
    {
        return BoundedDecisionWitness::Unsupported;
    }
    if !first_sequences
        .decision_summary_facts
        .state_is_productive(left)
        || !first_sequences
            .decision_summary_facts
            .state_is_productive(right)
        || first_sequences
            .decision_summary_facts
            .state_has_dead_prefix(left)
        || first_sequences
            .decision_summary_facts
            .state_has_dead_prefix(right)
    {
        return BoundedDecisionWitness::Unsupported;
    }
    let left = first_sequences.canonical_state(left.clone());
    let right = first_sequences.canonical_state(right.clone());
    let paired = paired_residual(left, right, grammar);
    BoundedDecisionWitness::Exact(
        first_sequences.decision_witness_at_depth(&paired, grammar.max_lookahead),
    )
}

/// Derives the declaration label from normalized grammar structure and previously established semantic facts.
fn declaration_label(
    declaration: crate::DeclId,
    grammar: &GrammarDescription,
    message: impl Into<String>,
) -> crate::DiagnosticLabel {
    crate::DiagnosticLabel::located(grammar.declarations[declaration.index()].location, message)
}

/// Diagnoses nullable alternatives whose FIRST sets overlap caller FOLLOW.
fn detect_follow_conflicts(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_provenance: &[Vec<ProvenancedPosition>],
    follow_by_site: &[ProvenancedFollow],
    static_conflict_kinds_by_declaration: &[KindSet],
    proofs: &mut ProofArena,
) -> Vec<Diagnostic> {
    let mut sites = (0..grammar.sites.len())
        .map(|index| SiteId::new(index as u32))
        .collect::<Vec<_>>();
    sites.sort_by_key(|site| {
        declaration_source_key(grammar.sites[site.index()].declaration, grammar)
    });
    let mut diagnostics = Vec::new();
    for site_id in sites {
        let site = &grammar.sites[site_id.index()];
        let follow = &follow_by_site[site_id.index()].positions;
        if follow.is_empty() {
            continue;
        }

        if site_nullable(site, &semantic[site.target.index()])
            && let Some(evidence) = overlap_evidence_excluding(
                &first_provenance[site.target.index()],
                follow,
                &static_conflict_kinds_by_declaration[site.declaration.index()],
                grammar,
                proofs,
            )
        {
            let secondary_declaration =
                follow_source_declaration(evidence.witness.right, site.declaration, proofs);
            diagnostics.push(Diagnostic::conflict_warning(
                DiagnosticCode::NullableFollowOverlap,
                format!(
                    "nullable use site in `{}` may consume a caller FOLLOW kind",
                    grammar.types[site.owner.index()].name,
                ),
                declaration_label(
                    site.declaration,
                    grammar,
                    "this nullable use site can consume the witness kind",
                ),
                declaration_label(
                    secondary_declaration,
                    grammar,
                    "the same kind can follow this use site",
                ),
                evidence,
                ConflictRemedy::ReviewNullableFollow { site: site_id },
            ));
        }

        let active = active_extender_provenance(
            site.target,
            site.min_bp,
            grammar,
            semantic,
            first_provenance,
            proofs,
        );
        if !active.is_empty()
            && let Some(evidence) = overlap_evidence_excluding(
                &active,
                follow,
                &static_conflict_kinds_by_declaration[site.declaration.index()],
                grammar,
                proofs,
            )
        {
            let secondary_declaration =
                follow_source_declaration(evidence.witness.right, site.declaration, proofs);
            diagnostics.push(Diagnostic::conflict_warning(
                DiagnosticCode::PrattFollowOverlap,
                format!(
                    "Pratt use site in `{}` may consume an active extender from caller FOLLOW",
                    grammar.types[site.owner.index()].name,
                ),
                declaration_label(
                    site.declaration,
                    grammar,
                    "this Pratt use site admits the witness extender",
                ),
                declaration_label(
                    secondary_declaration,
                    grammar,
                    "the same kind can follow this Pratt use site",
                ),
                evidence,
                ConflictRemedy::ReviewPrattFollow {
                    site: site_id,
                    min_bp: site.min_bp,
                },
            ));
        }
    }
    aggregate_follow_diagnostics(diagnostics, grammar, proofs)
}

/// Deduplicates FOLLOW conflicts and chooses their canonical witnesses.
fn aggregate_follow_diagnostics(
    diagnostics: Vec<Diagnostic>,
    grammar: &GrammarDescription,
    proofs: &ProofArena,
) -> Vec<Diagnostic> {
    let mut aggregated: Vec<Diagnostic> = Vec::new();
    for diagnostic in diagnostics {
        let site = match &diagnostic.conflict_remedy {
            Some(ConflictRemedy::ReviewNullableFollow { site })
            | Some(ConflictRemedy::ReviewPrattFollow { site, .. }) => *site,
            _ => unreachable!("FOLLOW diagnostics have a typed FOLLOW remedy"),
        };
        let declaration = grammar.sites[site.index()].declaration;
        if let Some(existing) = aggregated.iter_mut().find(|existing| {
            if existing.code != diagnostic.code {
                return false;
            }
            let existing_site = match &existing.conflict_remedy {
                Some(ConflictRemedy::ReviewNullableFollow { site })
                | Some(ConflictRemedy::ReviewPrattFollow { site, .. }) => *site,
                _ => return false,
            };
            grammar.sites[existing_site.index()].declaration == declaration
        }) {
            let incoming = diagnostic.conflict.expect("FOLLOW warning has evidence");
            let existing_conflict = existing
                .conflict
                .as_mut()
                .expect("FOLLOW warning has evidence");
            existing_conflict.overlap.union_assign(&incoming.overlap);
            let incoming_pair = (incoming.witness.left, incoming.witness.right);
            let existing_pair = (
                existing_conflict.witness.left,
                existing_conflict.witness.right,
            );
            if compare_proof_pairs(incoming_pair, existing_pair, proofs)
                .then_with(|| incoming.witness.kind.cmp(&existing_conflict.witness.kind))
                .is_lt()
            {
                existing_conflict.witness = incoming.witness;
            }
        } else {
            aggregated.push(diagnostic);
        }
    }
    aggregated.sort_by_key(|diagnostic| diagnostic_source_key(diagnostic, grammar));
    aggregated
}

/// Derives the follow source declaration from normalized grammar structure and previously established semantic facts.
fn follow_source_declaration(
    proof: ProofId,
    site_declaration: crate::DeclId,
    proofs: &ProofArena,
) -> crate::DeclId {
    proofs
        .chain(proof)
        .map(|node| node.step.declaration())
        .find(|declaration| *declaration != site_declaration)
        .unwrap_or(site_declaration)
}

/// Derives the active extender provenance from normalized grammar structure and previously established semantic facts.
fn active_extender_provenance(
    type_id: TypeId,
    min_bp: u8,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    first_provenance: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> Vec<ProvenancedPosition> {
    let TypeShape::Pratt { variants } = &grammar.types[type_id.index()].shape else {
        return Vec::new();
    };
    let mut variants = variants
        .iter()
        .filter(|variant| match variant.position {
            PrattPosition::Infix { lbp, .. } => lbp >= min_bp,
            PrattPosition::Postfix { bp } => bp >= min_bp,
            PrattPosition::Atom | PrattPosition::Prefix { .. } => false,
        })
        .collect::<Vec<_>>();
    variants.sort_by_key(|variant| declaration_source_key(variant.declaration, grammar));
    let mut positions = Vec::new();
    for variant in variants {
        let first = pratt_extender_provenance(
            type_id,
            variant,
            grammar,
            semantic,
            first_provenance,
            proofs,
        );
        merge_provenanced_positions(&mut positions, &first.positions, proofs);
    }
    positions
}

/// Derives the overlap evidence from normalized grammar structure and previously established semantic facts.
fn overlap_evidence(
    left: &[ProvenancedPosition],
    right: &[ProvenancedPosition],
    grammar: &GrammarDescription,
    proofs: &ProofArena,
) -> Option<ConflictEvidence> {
    overlap_evidence_excluding(
        left,
        right,
        &KindSet::empty(grammar.kind_count),
        grammar,
        proofs,
    )
}

/// Derives the overlap evidence excluding from normalized grammar structure and previously established semantic facts.
fn overlap_evidence_excluding(
    left: &[ProvenancedPosition],
    right: &[ProvenancedPosition],
    excluded: &KindSet,
    grammar: &GrammarDescription,
    proofs: &ProofArena,
) -> Option<ConflictEvidence> {
    let mut overlap = KindSet::empty(grammar.kind_count);
    let mut witnesses = vec![None; grammar.kind_count];
    for left_position in left {
        for right_position in right {
            if !predicates_overlap(
                left_position.text_predicate,
                right_position.text_predicate,
                &grammar.text_predicates,
                grammar.text_equivalence,
            ) {
                continue;
            }
            let intersection = left_position
                .kinds
                .kinds()
                .intersection(right_position.kinds.kinds())
                .expect("positions have the grammar width")
                .difference(excluded)
                .expect("excluded kinds have the grammar width");
            overlap.union_assign(&intersection);
            for kind in intersection.iter() {
                let pair = (
                    left_position.kinds.proof(kind).expect("bit has proof"),
                    right_position.kinds.proof(kind).expect("bit has proof"),
                );
                let slot = &mut witnesses[kind.index()];
                if slot.is_none_or(|current| proof_pair_is_better(pair, current, proofs)) {
                    *slot = Some(pair);
                }
            }
        }
    }
    let kind = overlap.iter().min_by(|left_kind, right_kind| {
        let left_pair = witnesses[left_kind.index()].expect("overlap kind has proof pair");
        let right_pair = witnesses[right_kind.index()].expect("overlap kind has proof pair");
        compare_proof_pairs(left_pair, right_pair, proofs).then_with(|| left_kind.cmp(right_kind))
    })?;
    let (left, right) = witnesses[kind.index()].expect("overlap kind has proof pair");
    Some(ConflictEvidence {
        overlap,
        witness: ConflictWitness { kind, left, right },
    })
}

/// Derives the proof pair is better from normalized grammar structure and previously established semantic facts.
fn proof_pair_is_better(
    candidate: (ProofId, ProofId),
    current: (ProofId, ProofId),
    proofs: &ProofArena,
) -> bool {
    compare_proof_pairs(candidate, current, proofs).is_lt()
}

/// Orders competing witness pairs by their canonical provenance chains.
fn compare_proof_pairs(
    left: (ProofId, ProofId),
    right: (ProofId, ProofId),
    proofs: &ProofArena,
) -> std::cmp::Ordering {
    let left_hops = proofs.get(left.0).expect("valid proof").rank.hops
        + proofs.get(left.1).expect("valid proof").rank.hops;
    let right_hops = proofs.get(right.0).expect("valid proof").rank.hops
        + proofs.get(right.1).expect("valid proof").rank.hops;
    left_hops
        .cmp(&right_hops)
        .then_with(|| proofs.compare(left.0, right.0))
        .then_with(|| proofs.compare(left.1, right.1))
}

/// Derives the predicates overlap from normalized grammar structure and previously established semantic facts.
pub(crate) fn predicates_overlap(
    left: Option<TextPredicateId>,
    right: Option<TextPredicateId>,
    predicates: &[TextPredicate],
    equivalence: TextEquivalence,
) -> bool {
    let Some(left) = left else {
        return true;
    };
    let Some(right) = right else {
        return true;
    };
    let left = &predicates[left.index()];
    let right = &predicates[right.index()];
    match (left, right) {
        (TextPredicate::Any, _) | (_, TextPredicate::Any) => true,
        (TextPredicate::Exact(left), TextPredicate::Exact(right)) => {
            equivalence.equivalent(left, right)
        }
        (TextPredicate::Exact(exact), TextPredicate::OneOf(options))
        | (TextPredicate::OneOf(options), TextPredicate::Exact(exact)) => options
            .iter()
            .any(|option| equivalence.equivalent(exact, option)),
        (TextPredicate::Exact(exact), TextPredicate::NotIn(excluded))
        | (TextPredicate::NotIn(excluded), TextPredicate::Exact(exact)) => !excluded
            .iter()
            .any(|value| equivalence.equivalent(exact, value)),
        (TextPredicate::OneOf(left), TextPredicate::OneOf(right)) => left.iter().any(|left| {
            right
                .iter()
                .any(|right| equivalence.equivalent(left, right))
        }),
        (TextPredicate::OneOf(options), TextPredicate::NotIn(excluded))
        | (TextPredicate::NotIn(excluded), TextPredicate::OneOf(options)) => {
            options.iter().any(|option| {
                !excluded
                    .iter()
                    .any(|value| equivalence.equivalent(option, value))
            })
        }
        (TextPredicate::NotIn(_), TextPredicate::NotIn(_)) => true,
    }
}

/// Finds the smallest lookahead depth that separates two FIRST-sequence sets.
///
/// Returns `None` when the sets still overlap at every depth through
/// `max_depth`; it does not claim that no larger finite depth could separate
/// them.
pub fn disambiguation_depth(
    left: &[FirstSequence],
    right: &[FirstSequence],
    predicates: &[TextPredicate],
    equivalence: TextEquivalence,
    max_depth: usize,
) -> Option<usize> {
    (1..=max_depth).find(|depth| {
        !left.iter().any(|left| {
            right.iter().any(|right| {
                first_sequences_overlap_within(left, right, predicates, equivalence, *depth)
            })
        })
    })
}

/// Derives the first sequences overlap within from normalized grammar structure and previously established semantic facts.
fn first_sequences_overlap_within(
    left: &FirstSequence,
    right: &FirstSequence,
    predicates: &[TextPredicate],
    equivalence: TextEquivalence,
    depth: usize,
) -> bool {
    let left_length = left.positions.len().min(depth);
    let right_length = right.positions.len().min(depth);
    if left_length != right_length {
        return false;
    }
    if left_length < depth && left.ends != right.ends {
        return false;
    }
    left.positions
        .iter()
        .zip(&right.positions)
        .take(left_length)
        .all(|(left, right)| {
            !left
                .kinds
                .intersection(&right.kinds)
                .expect("FIRST positions have the grammar width")
                .is_empty()
                && predicates_overlap(
                    left.text_predicate,
                    right.text_predicate,
                    predicates,
                    equivalence,
                )
        })
}

/// Derives the canonical configured-depth ambiguity witness from exact symbolic FIRST languages.
///
/// Work includes input-trie materialization and packed-kind posting candidates
/// across reachable overlapping node pairs. Shared prefixes avoid repeating
/// suffix work, but a fully overlapping edge product can still be Cartesian.
#[cfg(test)]
fn bounded_first_sequence_witness(
    left: &[FirstSequence],
    right: &[FirstSequence],
    grammar: &GrammarDescription,
) -> Option<Vec<DecisionWitness>> {
    let left = FirstSequenceWitnessTrie::from_sequences(left, grammar.max_lookahead);
    let right = FirstSequenceWitnessTrie::from_sequences(right, grammar.max_lookahead);
    minimum_bounded_witness(
        &left,
        &right,
        FirstSequenceWitnessNodeId(0),
        FirstSequenceWitnessNodeId(0),
        grammar,
        &mut BTreeMap::new(),
    )
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Dense identity of one bounded-witness prefix-trie node.
struct FirstSequenceWitnessNodeId(usize);

#[cfg(test)]
impl FirstSequenceWitnessNodeId {
    /// Returns the vector index owned by this identity.
    const fn index(self) -> usize {
        self.0
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
/// Ordered key for one exact symbolic position in a bounded-witness trie.
struct FirstSequenceWitnessPosition(FirstPosition);

#[cfg(test)]
impl Ord for FirstSequenceWitnessPosition {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        compare_first_positions(&self.0, &other.0)
    }
}

#[cfg(test)]
impl PartialOrd for FirstSequenceWitnessPosition {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Marks a bounded sequence that reaches a non-ending lookahead cutoff.
#[cfg(test)]
const FIRST_SEQUENCE_CUTOFF_TERMINAL: u8 = 1;
/// Marks a bounded sequence that ends exactly at its terminal trie node.
#[cfg(test)]
const FIRST_SEQUENCE_END_TERMINAL: u8 = 2;

#[cfg(test)]
#[derive(Default)]
/// Mutable prefix-trie node used only while interning exact input sequences.
struct FirstSequenceWitnessBuildNode {
    /// Bit zero records an `ends=false` terminal and bit one an `ends=true` terminal.
    terminals: u8,
    /// Exact symbolic outgoing positions and their interned children.
    edges: BTreeMap<FirstSequenceWitnessPosition, FirstSequenceWitnessNodeId>,
}

/// One exact symbolic outgoing edge in a bounded-witness prefix trie.
#[cfg(test)]
struct FirstSequenceWitnessEdge {
    /// Token kinds and text predicate consumed by the edge.
    position: FirstPosition,
    /// Prefix-trie state after consuming the position.
    child: FirstSequenceWitnessNodeId,
}

/// One immutable prefix-trie node with symbolic kind-word postings.
#[cfg(test)]
struct FirstSequenceWitnessNode {
    /// Depth of this prefix from the root.
    depth: usize,
    /// Bit zero records an `ends=false` terminal and bit one an `ends=true` terminal.
    terminals: u8,
    /// Canonically ordered exact symbolic outgoing edges.
    edges: Vec<FirstSequenceWitnessEdge>,
    /// Edge indices mentioning each non-empty packed token-kind word.
    edges_by_kind_word: BTreeMap<usize, Vec<usize>>,
}

/// Exact prefix trie over one materialized bounded FIRST language.
#[cfg(test)]
struct FirstSequenceWitnessTrie {
    /// Dense canonical nodes, rooted at index zero.
    nodes: Vec<FirstSequenceWitnessNode>,
}

#[cfg(test)]
impl FirstSequenceWitnessTrie {
    /// Interns exact symbolic sequences and indexes outgoing overlap candidates by packed kind word.
    fn from_sequences(sequences: &[FirstSequence], max_lookahead: usize) -> Self {
        let mut build = vec![FirstSequenceWitnessBuildNode::default()];
        let mut depths = vec![0];
        for sequence in sequences {
            debug_assert!(sequence.positions.len() <= max_lookahead);
            let mut node = FirstSequenceWitnessNodeId(0);
            for position in &sequence.positions {
                let key = FirstSequenceWitnessPosition(position.clone());
                let child = if let Some(child) = build[node.index()].edges.get(&key) {
                    *child
                } else {
                    let child = FirstSequenceWitnessNodeId(build.len());
                    let depth = depths[node.index()] + 1;
                    build.push(FirstSequenceWitnessBuildNode::default());
                    depths.push(depth);
                    build[node.index()].edges.insert(key, child);
                    child
                };
                node = child;
            }
            build[node.index()].terminals |= if sequence.ends {
                FIRST_SEQUENCE_END_TERMINAL
            } else {
                FIRST_SEQUENCE_CUTOFF_TERMINAL
            };
        }
        let nodes = build
            .into_iter()
            .zip(depths)
            .map(|(node, depth)| {
                let edges = node
                    .edges
                    .into_iter()
                    .map(|(position, child)| FirstSequenceWitnessEdge {
                        position: position.0,
                        child,
                    })
                    .collect::<Vec<_>>();
                let mut edges_by_kind_word = BTreeMap::<usize, Vec<usize>>::new();
                for (edge, transition) in edges.iter().enumerate() {
                    for (word, kinds) in transition.position.kinds.words().iter().enumerate() {
                        if *kinds != 0 {
                            edges_by_kind_word.entry(word).or_default().push(edge);
                        }
                    }
                }
                FirstSequenceWitnessNode {
                    depth,
                    terminals: node.terminals,
                    edges,
                    edges_by_kind_word,
                }
            })
            .collect();
        Self { nodes }
    }
}

/// Returns the canonical witness suffix shared by two exact prefix-trie nodes.
#[cfg(test)]
fn minimum_bounded_witness(
    left: &FirstSequenceWitnessTrie,
    right: &FirstSequenceWitnessTrie,
    left_node: FirstSequenceWitnessNodeId,
    right_node: FirstSequenceWitnessNodeId,
    grammar: &GrammarDescription,
    memo: &mut BTreeMap<
        (FirstSequenceWitnessNodeId, FirstSequenceWitnessNodeId),
        Option<Vec<DecisionWitness>>,
    >,
) -> Option<Vec<DecisionWitness>> {
    let key = (left_node, right_node);
    if let Some(witness) = memo.get(&key) {
        return witness.clone();
    }
    let left_node = &left.nodes[left_node.index()];
    let right_node = &right.nodes[right_node.index()];
    debug_assert_eq!(left_node.depth, right_node.depth);
    let terminal = if left_node.depth == grammar.max_lookahead {
        (left_node.terminals != 0 && right_node.terminals != 0).then(Vec::new)
    } else if left_node.terminals & right_node.terminals & FIRST_SEQUENCE_CUTOFF_TERMINAL != 0 {
        Some(Vec::new())
    } else if left_node.terminals & right_node.terminals & FIRST_SEQUENCE_END_TERMINAL != 0 {
        Some(vec![DecisionWitness::End])
    } else {
        None
    };
    if terminal.as_ref().is_some_and(Vec::is_empty) {
        memo.insert(key, terminal.clone());
        return terminal;
    }

    let mut pairs_by_witness = BTreeMap::<
        crate::LookaheadWitnessPosition,
        Vec<(FirstSequenceWitnessNodeId, FirstSequenceWitnessNodeId)>,
    >::new();
    for left_edge in &left_node.edges {
        let mut right_candidates = BTreeSet::new();
        for (word, kinds) in left_edge.position.kinds.words().iter().enumerate() {
            if *kinds == 0 {
                continue;
            }
            right_candidates.extend(
                right_node
                    .edges_by_kind_word
                    .get(&word)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
        }
        for right_edge in right_candidates {
            #[cfg(test)]
            BOUNDED_WITNESS_EDGE_PAIR_CHECKS
                .set(BOUNDED_WITNESS_EDGE_PAIR_CHECKS.get().saturating_add(1));
            let right_edge = &right_node.edges[right_edge];
            let Some(kind) =
                first_common_kind(&left_edge.position.kinds, &right_edge.position.kinds)
            else {
                continue;
            };
            if !predicates_overlap(
                left_edge.position.text_predicate,
                right_edge.position.text_predicate,
                &grammar.text_predicates,
                grammar.text_equivalence,
            ) {
                continue;
            }
            pairs_by_witness
                .entry(crate::LookaheadWitnessPosition {
                    kind,
                    left_text_predicate: left_edge.position.text_predicate,
                    right_text_predicate: right_edge.position.text_predicate,
                })
                .or_default()
                .push((left_edge.child, right_edge.child));
        }
    }
    for (witness, children) in pairs_by_witness {
        let suffix = children
            .into_iter()
            .filter_map(|(left_child, right_child)| {
                minimum_bounded_witness(left, right, left_child, right_child, grammar, memo)
            })
            .min();
        if let Some(suffix) = suffix {
            let mut candidate = Vec::with_capacity(suffix.len() + 1);
            candidate.push(DecisionWitness::Token(witness));
            candidate.extend(suffix);
            memo.insert(key, Some(candidate.clone()));
            return Some(candidate);
        }
    }
    memo.insert(key, terminal.clone());
    terminal
}

/// Returns the smallest concrete kind shared by two equally wide symbolic sets without allocating.
#[cfg(test)]
fn first_common_kind(left: &KindSet, right: &KindSet) -> Option<KindId> {
    debug_assert_eq!(left.kind_count(), right.kind_count());
    left.words()
        .iter()
        .zip(right.words())
        .enumerate()
        .find_map(|(word, (left, right))| {
            let shared = left & right;
            (shared != 0).then(|| {
                KindId::new(
                    (word * u64::BITS as usize + shared.trailing_zeros() as usize)
                        .try_into()
                        .expect("shared grammar kind identity width"),
                )
            })
        })
}

#[cfg(test)]
/// Retains the original Cartesian implementation as an exhaustive semantic oracle.
fn naive_bounded_first_sequence_witness(
    left: &[FirstSequence],
    right: &[FirstSequence],
    grammar: &GrammarDescription,
) -> Option<Vec<DecisionWitness>> {
    left.iter()
        .flat_map(|left| right.iter().map(move |right| (left, right)))
        .filter(|(left, right)| {
            first_sequences_overlap_within(
                left,
                right,
                &grammar.text_predicates,
                grammar.text_equivalence,
                grammar.max_lookahead,
            )
        })
        .filter_map(|(left, right)| {
            let mut witness = left
                .positions
                .iter()
                .zip(&right.positions)
                .map(|(left, right)| {
                    let kind = left
                        .kinds
                        .intersection(&right.kinds)
                        .expect("FIRST positions have the grammar width")
                        .iter()
                        .next()?;
                    Some(DecisionWitness::Token(crate::LookaheadWitnessPosition {
                        kind,
                        left_text_predicate: left.text_predicate,
                        right_text_predicate: right.text_predicate,
                    }))
                })
                .collect::<Option<Vec<_>>>()?;
            if witness.len() < grammar.max_lookahead && left.ends && right.ends {
                witness.push(DecisionWitness::End);
            }
            Some(witness)
        })
        .min()
}

/// Computes the token kinds that can extend each Pratt expression.
fn compute_extenders(
    grammar: &GrammarDescription,
    first: &[FirstFacts],
) -> Vec<Option<ExtenderFacts>> {
    grammar
        .types
        .iter()
        .map(|description| {
            let TypeShape::Pratt { variants } = &description.shape else {
                return None;
            };
            let mut positions = Vec::new();
            for variant in variants {
                if matches!(
                    variant.position,
                    PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
                ) {
                    let variant = first_for_sites(&variant.sites, grammar, first);
                    merge_positions(&mut positions, &variant.positions);
                }
            }
            let facts = first_facts(positions, false, grammar.kind_count);
            Some(ExtenderFacts {
                kinds: facts.kinds,
                positions: facts.positions,
            })
        })
        .collect()
}

/// Renders token-kind membership in stable grammar order.
fn render_kind_set(set: &KindSet) -> String {
    let members = set
        .iter()
        .map(|kind| kind.get().to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{members}}}")
}

#[cfg(test)]
mod decision_product_tests {
    use super::*;

    /// Adds one deterministic test declaration and source span.
    fn test_declaration(
        grammar: &mut GrammarDescription,
        next_byte: &mut u32,
        name: &str,
    ) -> crate::DeclId {
        let start = *next_byte;
        *next_byte += 2;
        let location =
            grammar.push_location(Location::new(crate::SourceId::new(0), start..start + 1));
        grammar.push_declaration(crate::Declaration::new(name, location))
    }

    /// Builds one small grammar covering every decision-closure branch.
    fn cutoff_summary_equivalence_grammar() -> (GrammarDescription, Vec<DecisionItem>) {
        fn push_token(grammar: &mut GrammarDescription, next_byte: &mut u32, name: &str) -> TypeId {
            let declaration = test_declaration(grammar, next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        }

        let mut grammar = GrammarDescription::new(
            "cutoff-summary-equivalence",
            2,
            2,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let token_a = push_token(&mut grammar, &mut next_byte, "A");
        let token_b = push_token(&mut grammar, &mut next_byte, "B");

        let empty_declaration = test_declaration(&mut grammar, &mut next_byte, "Empty");
        let empty = grammar.push_type(crate::TypeDescription::new(
            "Empty",
            empty_declaration,
            TypeShape::Sequence { sites: Vec::new() },
        ));

        let alias = grammar.next_type_id();
        let alias_declaration = test_declaration(&mut grammar, &mut next_byte, "OptionalAlias");
        let alias_site_declaration =
            test_declaration(&mut grammar, &mut next_byte, "OptionalAlias::value");
        let optional_alias_site = grammar.push_site(crate::ParseSite::new(
            alias,
            token_a,
            alias_site_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "OptionalAlias",
                alias_declaration,
                TypeShape::Alias {
                    site: optional_alias_site,
                },
            )),
            alias,
        );

        let sequence = grammar.next_type_id();
        let sequence_declaration = test_declaration(&mut grammar, &mut next_byte, "Containers");
        let container_shapes = [
            crate::ContainerShape::Direct,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
            crate::ContainerShape::zero_or_more(crate::ContainerShape::Direct, Some(token_b)),
            crate::ContainerShape::one_or_more(crate::ContainerShape::Direct, None),
            crate::ContainerShape::optional(crate::ContainerShape::one_or_more(
                crate::ContainerShape::Direct,
                Some(token_b),
            )),
        ];
        let container_sites = container_shapes
            .into_iter()
            .enumerate()
            .map(|(index, shape)| {
                let declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Containers::{index}"),
                );
                grammar.push_site(crate::ParseSite::new(
                    sequence,
                    if index == 1 { empty } else { token_a },
                    declaration,
                    shape,
                ))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Containers",
                sequence_declaration,
                TypeShape::Sequence {
                    sites: container_sites,
                },
            )),
            sequence,
        );

        let choice = grammar.next_type_id();
        let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
        let choice_alternatives = [("Token", token_a), ("Empty", empty)]
            .into_iter()
            .map(|(name, target)| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Choice::{name}"));
                let site = grammar.push_site(crate::ParseSite::new(
                    choice,
                    target,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum {
                    alternatives: choice_alternatives,
                },
            )),
            choice,
        );

        let pratt = grammar.next_type_id();
        let pratt_declaration = test_declaration(&mut grammar, &mut next_byte, "Expr");
        let pratt_variants = [
            ("Atom", PrattPosition::Atom, vec![token_a]),
            (
                "Prefix",
                PrattPosition::Prefix { bp: 20 },
                vec![token_b, token_a],
            ),
            (
                "Infix",
                PrattPosition::Infix { lbp: 10, rbp: 11 },
                vec![token_b, token_a],
            ),
        ]
        .into_iter()
        .map(|(name, position, targets)| {
            let declaration =
                test_declaration(&mut grammar, &mut next_byte, &format!("Expr::{name}"));
            let sites = targets
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let site_declaration = test_declaration(
                        &mut grammar,
                        &mut next_byte,
                        &format!("Expr::{name}::{index}"),
                    );
                    grammar.push_site(crate::ParseSite::new(
                        pratt,
                        target,
                        site_declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect();
            crate::PrattVariant {
                declaration,
                position,
                sites,
            }
        })
        .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Expr",
                pratt_declaration,
                TypeShape::Pratt {
                    variants: pratt_variants,
                },
            )),
            pratt,
        );

        let cycle_a = grammar.next_type_id();
        let cycle_b = TypeId::new(cycle_a.get() + 1);
        let cycle_a_declaration = test_declaration(&mut grammar, &mut next_byte, "CycleA");
        let cycle_a_site_declaration = test_declaration(&mut grammar, &mut next_byte, "CycleA::b");
        let cycle_a_site = grammar.push_site(crate::ParseSite::new(
            cycle_a,
            cycle_b,
            cycle_a_site_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "CycleA",
                cycle_a_declaration,
                TypeShape::Alias { site: cycle_a_site },
            )),
            cycle_a,
        );
        let cycle_b_declaration = test_declaration(&mut grammar, &mut next_byte, "CycleB");
        let cycle_b_alternatives = [("Cycle", cycle_a), ("Token", token_b)]
            .into_iter()
            .map(|(name, target)| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("CycleB::{name}"));
                let site = grammar.push_site(crate::ParseSite::new(
                    cycle_b,
                    target,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "CycleB",
                cycle_b_declaration,
                TypeShape::Enum {
                    alternatives: cycle_b_alternatives,
                },
            )),
            cycle_b,
        );

        let mut items = (0..grammar.types.len())
            .map(|index| {
                DecisionItem::Type(TypeId::new(
                    index.try_into().expect("test type identity width"),
                ))
            })
            .collect::<Vec<_>>();
        items.extend((0..grammar.sites.len()).map(|index| {
            DecisionItem::Site(crate::SiteId::new(
                index.try_into().expect("test site identity width"),
            ))
        }));
        for target in [token_a, empty] {
            for separator in [None, Some(token_b)] {
                items.push(DecisionItem::RepeatTail {
                    target,
                    separator,
                    optional_trailing: false,
                });
            }
        }
        (grammar, items)
    }

    /// Builds a diagonal pair choice repeated three times at bounded depth five.
    fn correlated_pair_grammar(branch_count: usize) -> (GrammarDescription, TypeId) {
        let mut grammar = GrammarDescription::new(
            "correlated-first",
            branch_count,
            5,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let mut tokens = Vec::new();
        for index in 0..branch_count {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            let kind = grammar.push_kind(crate::KindDescription::new(&name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                &name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            let token_name = format!("Token{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &token_name);
            tokens.push(grammar.push_type(crate::TypeDescription::new(
                token_name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            )));
        }

        let mut pairs = Vec::new();
        for (index, token) in tokens.into_iter().enumerate() {
            let name = format!("Pair{index}");
            let owner = grammar.next_type_id();
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            let sites = (0..2)
                .map(|site_index| {
                    let site_name = format!("{name}::{site_index}");
                    let site_declaration =
                        test_declaration(&mut grammar, &mut next_byte, &site_name);
                    grammar.push_site(crate::ParseSite::new(
                        owner,
                        token,
                        site_declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Sequence { sites },
                )),
                owner,
            );
            pairs.push(owner);
        }

        let choice = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "CorrelatedPair");
        let alternatives = pairs
            .into_iter()
            .enumerate()
            .map(|(index, pair)| {
                let name = format!("CorrelatedPair::{index}");
                let alternative = test_declaration(&mut grammar, &mut next_byte, &name);
                let site = grammar.push_site(crate::ParseSite::new(
                    choice,
                    pair,
                    alternative,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration: alternative,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "CorrelatedPair",
                declaration,
                TypeShape::Enum { alternatives },
            )),
            choice,
        );

        let root = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "Root");
        let sites = (0..3)
            .map(|index| {
                let name = format!("Root::{index}");
                let site_declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                grammar.push_site(crate::ParseSite::new(
                    root,
                    choice,
                    site_declaration,
                    crate::ContainerShape::Direct,
                ))
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Root",
                declaration,
                TypeShape::Sequence { sites },
            )),
            root,
        );
        (grammar, root)
    }

    /// Builds repeated correlated choices whose text predicates overlap on one spelling.
    fn overlapping_text_pair_grammar(branch_count: usize) -> (GrammarDescription, TypeId) {
        let mut grammar = GrammarDescription::new(
            "overlapping-text-first",
            1,
            5,
            TextEquivalence::AsciiInsensitive,
        );
        let mut next_byte = 0;
        let kind_declaration = test_declaration(&mut grammar, &mut next_byte, "Word");
        let kind = grammar.push_kind(crate::KindDescription::new("Word", kind_declaration));
        let atom_declaration = test_declaration(&mut grammar, &mut next_byte, "WordAtom");
        let atom = grammar.push_atom(crate::AtomDescription::new(
            "WordAtom",
            atom_declaration,
            AtomExpression::Kind(kind),
        ));
        let mut tokens = Vec::new();
        for index in 0..branch_count {
            let name = format!("Token{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            let text_predicate = grammar.push_text_predicate(TextPredicate::OneOf(vec![
                "common".to_owned(),
                format!("branch{index}"),
            ]));
            tokens.push(grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: Some(text_predicate),
                },
            )));
        }

        let mut pairs = Vec::new();
        for (index, token) in tokens.into_iter().enumerate() {
            let name = format!("Pair{index}");
            let owner = grammar.next_type_id();
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            let sites = (0..2)
                .map(|site_index| {
                    let site_name = format!("{name}::{site_index}");
                    let site_declaration =
                        test_declaration(&mut grammar, &mut next_byte, &site_name);
                    grammar.push_site(crate::ParseSite::new(
                        owner,
                        token,
                        site_declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Sequence { sites },
                )),
                owner,
            );
            pairs.push(owner);
        }

        let choice = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "OverlappingPair");
        let alternatives = pairs
            .into_iter()
            .enumerate()
            .map(|(index, pair)| {
                let name = format!("OverlappingPair::{index}");
                let alternative = test_declaration(&mut grammar, &mut next_byte, &name);
                let site = grammar.push_site(crate::ParseSite::new(
                    choice,
                    pair,
                    alternative,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration: alternative,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "OverlappingPair",
                declaration,
                TypeShape::Enum { alternatives },
            )),
            choice,
        );

        let root = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "OverlappingRoot");
        let sites = (0..3)
            .map(|index| {
                let name = format!("OverlappingRoot::{index}");
                let site_declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                grammar.push_site(crate::ParseSite::new(
                    root,
                    choice,
                    site_declaration,
                    crate::ContainerShape::Direct,
                ))
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "OverlappingRoot",
                declaration,
                TypeShape::Sequence { sites },
            )),
            root,
        );
        (grammar, root)
    }

    #[test]
    fn recursive_language_inclusion_bounds_paired_product_states() {
        const BASE_BRANCHES: usize = 32;
        fn push_token(grammar: &mut GrammarDescription, next_byte: &mut u32, name: &str) -> TypeId {
            let declaration = test_declaration(grammar, next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        }

        fn push_sequence(
            grammar: &mut GrammarDescription,
            next_byte: &mut u32,
            expected: TypeId,
            name: &str,
            targets: &[TypeId],
        ) {
            let declaration = test_declaration(grammar, next_byte, name);
            let sites = targets
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    let declaration =
                        test_declaration(grammar, next_byte, &format!("{name}::{index}"));
                    grammar.push_site(crate::ParseSite::new(
                        expected,
                        *target,
                        declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Sequence { sites },
                )),
                expected,
            );
        }

        fn push_choice(
            grammar: &mut GrammarDescription,
            next_byte: &mut u32,
            expected: TypeId,
            name: &str,
            targets: &[TypeId],
        ) {
            let declaration = test_declaration(grammar, next_byte, name);
            let alternatives = targets
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    let declaration =
                        test_declaration(grammar, next_byte, &format!("{name}::{index}"));
                    let site = grammar.push_site(crate::ParseSite::new(
                        expected,
                        *target,
                        declaration,
                        crate::ContainerShape::Direct,
                    ));
                    crate::Alternative {
                        declaration,
                        sites: vec![site],
                    }
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Enum { alternatives },
                )),
                expected,
            );
        }

        let mut grammar = GrammarDescription::new(
            "recursive-language-inclusion",
            3 + BASE_BRANCHES,
            5,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let with = push_token(&mut grammar, &mut next_byte, "WITH");
        let name = push_token(&mut grammar, &mut next_byte, "NAME");
        let values = push_token(&mut grammar, &mut next_byte, "VALUES");
        let base_statements = (0..BASE_BRANCHES)
            .map(|index| push_token(&mut grammar, &mut next_byte, &format!("BASE{index}")))
            .collect::<Vec<_>>();

        let cte = grammar.next_type_id();
        let with_clause = TypeId::new(cte.get() + 1);
        let with_statement = TypeId::new(cte.get() + 2);
        let select_body = TypeId::new(cte.get() + 3);
        let compound_body = TypeId::new(cte.get() + 4);
        let subquery = TypeId::new(cte.get() + 5);
        let statement = TypeId::new(cte.get() + 6);
        push_sequence(&mut grammar, &mut next_byte, cte, "Cte", &[name, statement]);
        push_sequence(
            &mut grammar,
            &mut next_byte,
            with_clause,
            "WithClause",
            &[with, cte],
        );
        push_sequence(
            &mut grammar,
            &mut next_byte,
            with_statement,
            "WithStatement",
            &[with_clause, statement],
        );
        push_choice(
            &mut grammar,
            &mut next_byte,
            select_body,
            "SelectBody",
            &[with_statement, values],
        );
        push_sequence(
            &mut grammar,
            &mut next_byte,
            compound_body,
            "CompoundBody",
            &[select_body],
        );
        push_choice(
            &mut grammar,
            &mut next_byte,
            subquery,
            "Subquery",
            &[compound_body],
        );
        let mut statement_alternatives = vec![with_statement, subquery];
        statement_alternatives.extend(base_statements);
        push_choice(
            &mut grammar,
            &mut next_byte,
            statement,
            "Statement",
            &statement_alternatives,
        );

        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        DECISION_PRODUCT_STATE_EXPANSIONS.set(0);
        DECISION_PRODUCT_TRANSITION_PAIRS.set(0);
        FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.set(0);
        let product = build_decision_product(
            paired_residual(
                DecisionState(vec![DecisionItem::Type(with_statement)]),
                DecisionState(vec![DecisionItem::Type(subquery)]),
                &grammar,
            ),
            &first_sequences,
        );
        let state_expansions = DECISION_PRODUCT_STATE_EXPANSIONS.get();
        let transition_pairs = DECISION_PRODUCT_TRANSITION_PAIRS.get();
        let closure_visits = FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.get();
        assert!(matches!(product, DecisionProduct::Cyclic));
        assert!(
            state_expansions <= 1 && transition_pairs <= 1 && closure_visits <= BASE_BRANCHES * 8,
            "a productive common language with a nullable extension must prove complete overlap from its first shared successor; states={state_expansions} transition_pairs={transition_pairs} closure_visits={closure_visits}",
        );

        let outcome = analyze_decision(
            DecisionState(vec![DecisionItem::Type(with_statement)]),
            DecisionState(vec![DecisionItem::Type(subquery)]),
            &first_sequences,
            BoundedDecisionWitness::Unsupported,
        );
        assert_eq!(outcome.sufficient_depth, None);
    }

    #[test]
    fn complete_overlap_proof_requires_productivity_and_a_nullable_extension() {
        let mut grammar =
            GrammarDescription::new("complete-overlap-proof", 2, 2, TextEquivalence::Sensitive);
        let mut next_byte = 0;
        let mut push_token = |grammar: &mut GrammarDescription, name: &str| {
            let declaration = test_declaration(grammar, &mut next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        };
        let common = push_token(&mut grammar, "COMMON");
        let tail = push_token(&mut grammar, "TAIL");
        let never = grammar.next_type_id();
        let never_site_declaration = test_declaration(&mut grammar, &mut next_byte, "Never::self");
        let never_site = grammar.push_site(crate::ParseSite::new(
            never,
            never,
            never_site_declaration,
            crate::ContainerShape::Direct,
        ));
        let never_declaration = test_declaration(&mut grammar, &mut next_byte, "Never");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Never",
                never_declaration,
                TypeShape::Sequence {
                    sites: vec![never_site],
                },
            )),
            never,
        );
        let optional_tail_declaration =
            test_declaration(&mut grammar, &mut next_byte, "OptionalTail");
        let optional_tail = grammar.push_site(crate::ParseSite::new(
            common,
            tail,
            optional_tail_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        let other_optional_tail_declaration =
            test_declaration(&mut grammar, &mut next_byte, "OtherOptionalTail");
        let other_optional_tail = grammar.push_site(crate::ParseSite::new(
            common,
            common,
            other_optional_tail_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        let atom_kinds = expand_atoms(&grammar);
        let facts = DecisionSummaryFacts::from_grammar(&grammar, &atom_kinds);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);

        let productive = DecisionState(vec![DecisionItem::Type(common)]);
        assert!(state_is_productive_prefix_with_nullable_extension(
            &productive,
            &DecisionState(vec![
                DecisionItem::Type(common),
                DecisionItem::Site(optional_tail),
            ]),
            &grammar,
            &facts,
        ));
        assert!(!state_is_productive_prefix_with_nullable_extension(
            &productive,
            &DecisionState(vec![DecisionItem::Type(common), DecisionItem::Type(tail),]),
            &grammar,
            &facts,
        ));
        assert!(!state_is_productive_prefix_with_nullable_extension(
            &DecisionState(vec![DecisionItem::Type(never)]),
            &DecisionState(vec![
                DecisionItem::Type(never),
                DecisionItem::Site(optional_tail),
            ]),
            &grammar,
            &facts,
        ));
        assert!(facts.state_has_dead_prefix(&DecisionState(vec![DecisionItem::Type(never),])));

        let left = DecisionState(vec![
            DecisionItem::Type(common),
            DecisionItem::Site(optional_tail),
        ]);
        let right = DecisionState(vec![
            DecisionItem::Type(common),
            DecisionItem::Site(other_optional_tail),
        ]);
        assert!(states_have_productive_common_prefix_with_nullable_suffixes(
            &left, &right, &grammar, &facts,
        ));
        DECISION_PRODUCT_STATE_EXPANSIONS.set(0);
        assert!(matches!(
            build_decision_product(PairedDecisionState::Residual(left, right), &first_sequences,),
            DecisionProduct::Cyclic,
        ));
        assert_eq!(
            DECISION_PRODUCT_STATE_EXPANSIONS.get(),
            0,
            "a productive common prefix with independently nullable tails must stop before product expansion",
        );
    }

    #[test]
    fn complete_overlap_proof_rejects_lexically_empty_token_suffixes() {
        let mut grammar =
            GrammarDescription::new("empty-token-productivity", 2, 1, TextEquivalence::Sensitive);
        let mut next_byte = 0;
        let a_declaration = test_declaration(&mut grammar, &mut next_byte, "A");
        let a_kind = grammar.push_kind(crate::KindDescription::new("A", a_declaration));
        let a_atom = grammar.push_atom(crate::AtomDescription::new(
            "A",
            a_declaration,
            AtomExpression::Kind(a_kind),
        ));
        let a = grammar.push_type(crate::TypeDescription::new(
            "A",
            a_declaration,
            TypeShape::Token {
                atom: a_atom,
                text_predicate: None,
            },
        ));
        let b_declaration = test_declaration(&mut grammar, &mut next_byte, "B");
        let b_kind = grammar.push_kind(crate::KindDescription::new("B", b_declaration));
        let b_atom = grammar.push_atom(crate::AtomDescription::new(
            "B",
            b_declaration,
            AtomExpression::Kind(b_kind),
        ));
        let b = grammar.push_type(crate::TypeDescription::new(
            "B",
            b_declaration,
            TypeShape::Token {
                atom: b_atom,
                text_predicate: None,
            },
        ));

        let empty_atom_declaration = test_declaration(&mut grammar, &mut next_byte, "EmptyAtom");
        let empty_atom = grammar.push_atom(crate::AtomDescription::new(
            "EmptyAtom",
            empty_atom_declaration,
            AtomExpression::Category(Vec::new()),
        ));
        let empty_atom_token = grammar.push_type(crate::TypeDescription::new(
            "EmptyAtomToken",
            empty_atom_declaration,
            TypeShape::Token {
                atom: empty_atom,
                text_predicate: None,
            },
        ));
        let empty_text_declaration = test_declaration(&mut grammar, &mut next_byte, "EmptyText");
        let empty_text = grammar.push_text_predicate(TextPredicate::OneOf(Vec::new()));
        let empty_text_token = grammar.push_type(crate::TypeDescription::new(
            "EmptyTextToken",
            empty_text_declaration,
            TypeShape::Token {
                atom: a_atom,
                text_predicate: Some(empty_text),
            },
        ));
        let optional_tail_declaration =
            test_declaration(&mut grammar, &mut next_byte, "OptionalTail");
        let optional_tail = grammar.push_site(crate::ParseSite::new(
            a,
            b,
            optional_tail_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        let atom_kinds = expand_atoms(&grammar);
        let facts = DecisionSummaryFacts::from_grammar(&grammar, &atom_kinds);
        let cutoff = DecisionCutoffFacts::from_grammar(&grammar, &atom_kinds, &facts);

        for empty in [empty_atom_token, empty_text_token] {
            assert_eq!(
                facts.summarize_state(&DecisionState(vec![DecisionItem::Type(empty)])),
                0,
                "a lexically empty token has no exact end or consuming continuation",
            );
            assert!(
                cutoff.by_type[empty.index()].transitions.is_empty(),
                "cutoff facts must not synthesize a transition for a lexically empty token",
            );
            let shorter = DecisionState(vec![DecisionItem::Type(a), DecisionItem::Type(empty)]);
            let longer = DecisionState(vec![
                DecisionItem::Type(a),
                DecisionItem::Type(empty),
                DecisionItem::Site(optional_tail),
            ]);
            assert!(!state_is_productive_prefix_with_nullable_extension(
                &shorter, &longer, &grammar, &facts,
            ));
        }
    }

    #[test]
    fn decision_product_follows_a_deep_overlap_before_wide_finite_siblings() {
        const BRANCH_COUNT: usize = 64;
        let mut grammar = GrammarDescription::new(
            "deep-overlap-before-wide-siblings",
            BRANCH_COUNT * 3 - 1,
            3,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let mut push_token = |grammar: &mut GrammarDescription, name: &str| {
            let declaration = test_declaration(grammar, &mut next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        };
        let starters = (0..BRANCH_COUNT)
            .map(|index| push_token(&mut grammar, &format!("START_{index}")))
            .collect::<Vec<_>>();
        let mut left_tails = (0..BRANCH_COUNT - 1)
            .map(|index| push_token(&mut grammar, &format!("LEFT_{index}")))
            .collect::<Vec<_>>();
        let mut right_tails = (0..BRANCH_COUNT - 1)
            .map(|index| push_token(&mut grammar, &format!("RIGHT_{index}")))
            .collect::<Vec<_>>();
        let common = push_token(&mut grammar, "COMMON");
        let mut push_wrapper = |grammar: &mut GrammarDescription, name: &str| {
            let owner = grammar.next_type_id();
            let declaration = test_declaration(grammar, &mut next_byte, name);
            let site_declaration =
                test_declaration(grammar, &mut next_byte, &format!("{name}::value"));
            let site = grammar.push_site(crate::ParseSite::new(
                owner,
                common,
                site_declaration,
                crate::ContainerShape::Direct,
            ));
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Sequence { sites: vec![site] },
                )),
                owner,
            );
            owner
        };
        left_tails.push(push_wrapper(&mut grammar, "LeftCommon"));
        right_tails.push(push_wrapper(&mut grammar, "RightCommon"));

        let mut push_choice = |grammar: &mut GrammarDescription, name: &str, tails: &[TypeId]| {
            let owner = grammar.next_type_id();
            let declaration = test_declaration(grammar, &mut next_byte, name);
            let alternatives = starters
                .iter()
                .copied()
                .zip(tails.iter().copied())
                .enumerate()
                .map(|(index, (starter, tail))| {
                    let alternative =
                        test_declaration(grammar, &mut next_byte, &format!("{name}::{index}"));
                    let sites = [starter, tail]
                        .into_iter()
                        .enumerate()
                        .map(|(site_index, target)| {
                            let site_declaration = test_declaration(
                                grammar,
                                &mut next_byte,
                                &format!("{name}::{index}::{site_index}"),
                            );
                            grammar.push_site(crate::ParseSite::new(
                                owner,
                                target,
                                site_declaration,
                                crate::ContainerShape::Direct,
                            ))
                        })
                        .collect();
                    crate::Alternative {
                        declaration: alternative,
                        sites,
                    }
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Enum { alternatives },
                )),
                owner,
            );
            owner
        };
        let left = push_choice(&mut grammar, "Left", &left_tails);
        let right = push_choice(&mut grammar, "Right", &right_tails);
        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        DECISION_PRODUCT_STATE_EXPANSIONS.set(0);

        let left_state = DecisionState(vec![DecisionItem::Type(left)]);
        let right_state = DecisionState(vec![DecisionItem::Type(right)]);
        let start = PairedDecisionState::Residual(left_state.clone(), right_state.clone());
        let product = build_decision_product(start.clone(), &first_sequences);

        assert!(matches!(product, DecisionProduct::Cyclic));
        assert!(
            DECISION_PRODUCT_STATE_EXPANSIONS.get() <= 2,
            "the product must follow the deepest scheduled branch before expanding unrelated finite siblings; expansions={}",
            DECISION_PRODUCT_STATE_EXPANSIONS.get(),
        );
        let witness =
            decision_witness_at_depth(&start, &grammar, &atom_kinds, grammar.max_lookahead)
                .expect("the final branch has one complete shared sequence");
        assert!(
            witness
                .iter()
                .any(|position| matches!(position, DecisionWitness::End)),
        );
        DECISION_PRODUCT_STATE_EXPANSIONS.set(0);
        let outcome = analyze_decision(
            left_state,
            right_state,
            &first_sequences,
            BoundedDecisionWitness::Exact(Some(witness)),
        );
        assert_eq!(outcome.sufficient_depth, None);
        assert_eq!(
            DECISION_PRODUCT_STATE_EXPANSIONS.get(),
            0,
            "a bounded complete witness already proves that no finite lookahead separates the languages",
        );
    }

    #[test]
    fn shallow_recursive_overlap_stops_before_unrelated_finite_product_branches() {
        const FINITE_BRANCHES: usize = 16;

        fn push_token(grammar: &mut GrammarDescription, next_byte: &mut u32, name: &str) -> TypeId {
            let declaration = test_declaration(grammar, next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        }

        let mut grammar = GrammarDescription::new(
            "shallow-recursive-overlap",
            1 + FINITE_BRANCHES * 2,
            5,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let recurse = push_token(&mut grammar, &mut next_byte, "RECURSE");
        let finite = (0..FINITE_BRANCHES)
            .map(|index| {
                let first = push_token(
                    &mut grammar,
                    &mut next_byte,
                    &format!("FINITE_FIRST_{index}"),
                );
                let second = push_token(
                    &mut grammar,
                    &mut next_byte,
                    &format!("FINITE_SECOND_{index}"),
                );
                (first, second)
            })
            .collect::<Vec<_>>();

        let recursive = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "Recursive");
        let mut alternatives = Vec::with_capacity(1 + FINITE_BRANCHES);
        let recursive_declaration =
            test_declaration(&mut grammar, &mut next_byte, "Recursive::Recursive");
        let recursive_sites = [recurse, recursive]
            .into_iter()
            .enumerate()
            .map(|(index, target)| {
                let declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Recursive::Recursive::{index}"),
                );
                grammar.push_site(crate::ParseSite::new(
                    recursive,
                    target,
                    declaration,
                    crate::ContainerShape::Direct,
                ))
            })
            .collect();
        alternatives.push(crate::Alternative {
            declaration: recursive_declaration,
            sites: recursive_sites,
        });
        alternatives.extend(finite.into_iter().enumerate().map(|(branch, targets)| {
            let alternative = test_declaration(
                &mut grammar,
                &mut next_byte,
                &format!("Recursive::{branch}"),
            );
            let sites = [targets.0, targets.1]
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let declaration = test_declaration(
                        &mut grammar,
                        &mut next_byte,
                        &format!("Recursive::{branch}::{index}"),
                    );
                    grammar.push_site(crate::ParseSite::new(
                        recursive,
                        target,
                        declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect();
            crate::Alternative {
                declaration: alternative,
                sites,
            }
        }));
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Recursive",
                declaration,
                TypeShape::Enum { alternatives },
            )),
            recursive,
        );

        let choice = grammar.next_type_id();
        let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
        let choice_alternatives = (0..2)
            .map(|index| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Choice::{index}"));
                let site = grammar.push_site(crate::ParseSite::new(
                    choice,
                    recursive,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum {
                    alternatives: choice_alternatives,
                },
            )),
            choice,
        );

        let repeated_choice = grammar.next_type_id();
        let repeated_choice_declaration =
            test_declaration(&mut grammar, &mut next_byte, "RepeatedChoice");
        let repeated_declaration =
            test_declaration(&mut grammar, &mut next_byte, "RepeatedChoice::Repeated");
        let repeated_site = grammar.push_site(crate::ParseSite::new(
            repeated_choice,
            recursive,
            repeated_declaration,
            crate::ContainerShape::zero_or_more(crate::ContainerShape::Direct, Some(recurse)),
        ));
        let direct_declaration =
            test_declaration(&mut grammar, &mut next_byte, "RepeatedChoice::Direct");
        let direct_site = grammar.push_site(crate::ParseSite::new(
            repeated_choice,
            recursive,
            direct_declaration,
            crate::ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "RepeatedChoice",
                repeated_choice_declaration,
                TypeShape::Enum {
                    alternatives: vec![
                        crate::Alternative {
                            declaration: repeated_declaration,
                            sites: vec![repeated_site],
                        },
                        crate::Alternative {
                            declaration: direct_declaration,
                            sites: vec![direct_site],
                        },
                    ],
                },
            )),
            repeated_choice,
        );

        let atom_kinds = expand_atoms(&grammar);
        let recursive_state = DecisionState(vec![DecisionItem::Type(recursive)]);
        let paired_start =
            paired_residual(recursive_state.clone(), recursive_state.clone(), &grammar);
        let direct_witness =
            decision_witness_at_depth(&paired_start, &grammar, &atom_kinds, grammar.max_lookahead)
                .expect("recursive language overlaps itself");
        let first_sequence_analysis = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        let mut language = first_sequence_analysis.language();
        let root = language.build(grammar.max_lookahead, vec![recursive_state.clone()]);
        let sequences = language.materialize(root);
        let frozen_witness = bounded_first_sequence_witness(&sequences, &sequences, &grammar)
            .expect("frozen recursive language overlaps itself");
        assert_eq!(frozen_witness, direct_witness);
        let repeated_start = paired_residual(
            DecisionState::from_sites(&[repeated_site]),
            DecisionState::from_sites(&[direct_site]),
            &grammar,
        );
        let direct_repeated_witness = decision_witness_at_depth(
            &repeated_start,
            &grammar,
            &atom_kinds,
            grammar.max_lookahead,
        )
        .expect("the repeated and direct languages overlap");
        let repeated_root = language.build(
            grammar.max_lookahead,
            vec![DecisionState::from_sites(&[repeated_site])],
        );
        let repeated_sequences = language.materialize(repeated_root);
        let direct_root = language.build(
            grammar.max_lookahead,
            vec![DecisionState::from_sites(&[direct_site])],
        );
        let direct_sequences = language.materialize(direct_root);
        let frozen_repeated_witness =
            bounded_first_sequence_witness(&repeated_sequences, &direct_sequences, &grammar)
                .expect("the frozen repeated and direct languages overlap");
        assert_eq!(frozen_repeated_witness, direct_repeated_witness);

        DECISION_PRODUCT_STATE_EXPANSIONS.set(0);
        let outcome = analyze_decision(
            recursive_state.clone(),
            recursive_state,
            &first_sequence_analysis,
            BoundedDecisionWitness::Exact(Some(frozen_witness)),
        );
        assert_eq!(outcome.sufficient_depth, None);
        assert_eq!(outcome.witness.len(), grammar.max_lookahead);
        assert!(
            DECISION_PRODUCT_STATE_EXPANSIONS.get() <= 4,
            "a proven shallow cycle must stop the unbounded product before unrelated finite branches; expansions={}",
            DECISION_PRODUCT_STATE_EXPANSIONS.get(),
        );

        DECISION_WITNESS_STATE_EXPANSIONS.set(0);
        let report = analyze(&grammar);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::EnumFirstOverlap),
        );
        assert!(
            DECISION_WITNESS_STATE_EXPANSIONS.get() <= grammar.max_lookahead,
            "required and repeated enum sites must reuse the analysis-run direct witness cache; expansions={}",
            DECISION_WITNESS_STATE_EXPANSIONS.get(),
        );
    }

    #[test]
    fn recursive_product_stops_inside_a_wide_overlap_cell() {
        const FINITE_BRANCHES: usize = 128;

        let mut grammar = GrammarDescription::new(
            "recursive-wide-overlap-cell",
            FINITE_BRANCHES + 1,
            5,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let shared_declaration = test_declaration(&mut grammar, &mut next_byte, "SHARED");
        let shared_kind =
            grammar.push_kind(crate::KindDescription::new("SHARED", shared_declaration));
        let shared_atom = grammar.push_atom(crate::AtomDescription::new(
            "SHARED",
            shared_declaration,
            AtomExpression::Kind(shared_kind),
        ));
        let shared = grammar.push_type(crate::TypeDescription::new(
            "Shared",
            shared_declaration,
            TypeShape::Token {
                atom: shared_atom,
                text_predicate: None,
            },
        ));

        // Put the recursive residual before every finite residual in typed-ID
        // order. A cycle proof is therefore the first exact member of the
        // one wide symbolic SHARED cell.
        let recursive = grammar.next_type_id();
        let recursive_declaration = test_declaration(&mut grammar, &mut next_byte, "Recursive");
        let mut alternatives = Vec::with_capacity(FINITE_BRANCHES + 1);
        let mut recursive_tail_site = None;
        for (branch, target) in std::iter::once(recursive)
            .chain((0..FINITE_BRANCHES).map(|index| TypeId::new(2 + index as u32)))
            .enumerate()
        {
            let declaration = test_declaration(
                &mut grammar,
                &mut next_byte,
                &format!("Recursive::{branch}"),
            );
            let sites = [shared, target]
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let site_declaration = test_declaration(
                        &mut grammar,
                        &mut next_byte,
                        &format!("Recursive::{branch}::{index}"),
                    );
                    grammar.push_site(crate::ParseSite::new(
                        recursive,
                        target,
                        site_declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect::<Vec<_>>();
            if branch == 0 {
                recursive_tail_site = Some(sites[1]);
            }
            alternatives.push(crate::Alternative { declaration, sites });
        }
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Recursive",
                recursive_declaration,
                TypeShape::Enum { alternatives },
            )),
            recursive,
        );

        for branch in 0..FINITE_BRANCHES {
            let name = format!("FINITE_{branch}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            let kind = grammar.push_kind(crate::KindDescription::new(&name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                &name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    &name,
                    declaration,
                    TypeShape::Token {
                        atom,
                        text_predicate: None,
                    },
                )),
                TypeId::new(2 + branch as u32),
            );
        }

        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        DECISION_PRODUCT_TRANSITION_PAIRS.set(0);
        let state = DecisionState(vec![DecisionItem::Site(
            recursive_tail_site.expect("recursive tail site"),
        )]);
        let product = build_decision_product(
            paired_residual(state.clone(), state, &grammar),
            &first_sequences,
        );
        assert!(matches!(product, DecisionProduct::Cyclic));
        assert!(
            DECISION_PRODUCT_TRANSITION_PAIRS.get() <= 1,
            "a proven first recursive successor must stop before the unrelated Cartesian tail of its symbolic cell; transition_pairs={}",
            DECISION_PRODUCT_TRANSITION_PAIRS.get(),
        );
    }

    #[test]
    fn optional_trailing_separator_is_part_of_the_frozen_single_site_language() {
        let mut grammar = GrammarDescription::new(
            "optional-trailing-separator",
            2,
            3,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let mut push_token = |grammar: &mut GrammarDescription, name: &str| {
            let declaration = test_declaration(grammar, &mut next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            let target = grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ));
            (target, kind)
        };
        let (item, item_kind) = push_token(&mut grammar, "ITEM");
        let (separator, separator_kind) = push_token(&mut grammar, "COMMA");

        let exact = grammar.next_type_id();
        let exact_declaration = test_declaration(&mut grammar, &mut next_byte, "Exact");
        let exact_sites = [item, separator]
            .into_iter()
            .enumerate()
            .map(|(index, target)| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Exact::{index}"));
                grammar.push_site(crate::ParseSite::new(
                    exact,
                    target,
                    declaration,
                    crate::ContainerShape::Direct,
                ))
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Exact",
                exact_declaration,
                TypeShape::Sequence { sites: exact_sites },
            )),
            exact,
        );

        let choice = grammar.next_type_id();
        let repeated_declaration = test_declaration(&mut grammar, &mut next_byte, "Repeated");
        let repeated_site = grammar.push_site(crate::ParseSite::new(
            choice,
            item,
            repeated_declaration,
            crate::ContainerShape::repeated_with_separator(
                crate::ContainerShape::Direct,
                crate::RepetitionCardinality::OneOrMore,
                Some(crate::RepeatSeparator::new(separator, true)),
            ),
        ));
        let exact_alternative = test_declaration(&mut grammar, &mut next_byte, "ExactAlternative");
        let exact_site = grammar.push_site(crate::ParseSite::new(
            choice,
            exact,
            exact_alternative,
            crate::ContainerShape::Direct,
        ));
        let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum {
                    alternatives: vec![
                        crate::Alternative {
                            declaration: repeated_declaration,
                            sites: vec![repeated_site],
                        },
                        crate::Alternative {
                            declaration: exact_alternative,
                            sites: vec![exact_site],
                        },
                    ],
                },
            )),
            choice,
        );

        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        let repeated = first_sequences.sequences(&[repeated_site]);
        let exact = first_sequences.sequences(&[exact_site]);
        let frozen = bounded_first_sequence_witness(&repeated, &exact, &grammar)
            .expect("a trailing separator is an exact shared prefix");
        assert_eq!(
            frozen,
            vec![
                DecisionWitness::Token(crate::LookaheadWitnessPosition {
                    kind: item_kind,
                    left_text_predicate: None,
                    right_text_predicate: None,
                }),
                DecisionWitness::Token(crate::LookaheadWitnessPosition {
                    kind: separator_kind,
                    left_text_predicate: None,
                    right_text_predicate: None,
                }),
                DecisionWitness::End,
            ],
        );
        let direct = decision_witness_at_depth(
            &paired_residual(
                DecisionState::from_sites(&[repeated_site]),
                DecisionState::from_sites(&[exact_site]),
                &grammar,
            ),
            &grammar,
            &atom_kinds,
            grammar.max_lookahead,
        )
        .expect("direct decision traversal must retain the trailing separator");
        assert_eq!(frozen, direct);
    }

    #[test]
    fn nonzero_pratt_binding_power_keeps_authored_identity_outside_the_site_language_cache() {
        let mut grammar = GrammarDescription::new(
            "binding-power-sensitive-site",
            2,
            3,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let mut push_token = |grammar: &mut GrammarDescription, name: &str| {
            let declaration = test_declaration(grammar, &mut next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        };
        let number = push_token(&mut grammar, "NUMBER");
        let bang = push_token(&mut grammar, "BANG");

        let expression = grammar.next_type_id();
        let expression_declaration = test_declaration(&mut grammar, &mut next_byte, "Expression");
        let atom_declaration = test_declaration(&mut grammar, &mut next_byte, "Expression::Atom");
        let atom_site = grammar.push_site(crate::ParseSite::new(
            expression,
            number,
            atom_declaration,
            crate::ContainerShape::Direct,
        ));
        let postfix_declaration =
            test_declaration(&mut grammar, &mut next_byte, "Expression::Postfix");
        let postfix_site = grammar.push_site(crate::ParseSite::new(
            expression,
            bang,
            postfix_declaration,
            crate::ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Expression",
                expression_declaration,
                TypeShape::Pratt {
                    variants: vec![
                        crate::PrattVariant {
                            declaration: atom_declaration,
                            position: PrattPosition::Atom,
                            sites: vec![atom_site],
                        },
                        crate::PrattVariant {
                            declaration: postfix_declaration,
                            position: PrattPosition::Postfix { bp: 5 },
                            sites: vec![postfix_site],
                        },
                    ],
                },
            )),
            expression,
        );

        let choice = grammar.next_type_id();
        let low_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice::Low");
        let low = grammar.push_site(crate::ParseSite::new(
            choice,
            expression,
            low_declaration,
            crate::ContainerShape::Direct,
        ));
        let high_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice::High");
        let high = grammar.push_site(
            crate::ParseSite::new(
                choice,
                expression,
                high_declaration,
                crate::ContainerShape::Direct,
            )
            .with_min_bp(10),
        );
        let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum {
                    alternatives: vec![
                        crate::Alternative {
                            declaration: low_declaration,
                            sites: vec![low],
                        },
                        crate::Alternative {
                            declaration: high_declaration,
                            sites: vec![high],
                        },
                    ],
                },
            )),
            choice,
        );

        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        assert_ne!(
            decision_site_semantics(&grammar.sites[low.index()]),
            decision_site_semantics(&grammar.sites[high.index()]),
        );
        assert_ne!(
            first_sequences.canonical_sites[low.index()],
            first_sequences.canonical_sites[high.index()],
        );
        assert!(matches!(
            bounded_site_language_witness(&[low], &[high], &grammar, &first_sequences,),
            BoundedDecisionWitness::Unsupported,
        ));
        let tail_sites = [low];
        let left =
            DecisionState::consuming_site_and_tail(&grammar.sites[high.index()], &tail_sites);
        let right = DecisionState::from_sites(&tail_sites);
        let bounded = bounded_nullable_tail_witness(
            &grammar.sites[high.index()],
            &tail_sites,
            &left,
            &right,
            &grammar,
            &first_sequences,
        );
        assert!(matches!(&bounded, BoundedDecisionWitness::Unsupported));
        DECISION_WITNESS_STATE_EXPANSIONS.set(0);
        FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.set(0);
        let _ = analyze_decision(left.clone(), right.clone(), &first_sequences, bounded);
        assert!(
            DECISION_WITNESS_STATE_EXPANSIONS.get() > 0,
            "unsupported binding-power states retain explicit witness traversal",
        );
        let frontier_computations = FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.get();
        assert!(
            frontier_computations > 0,
            "the unbounded decision product and fallback must use the analysis-run frontier cache",
        );
        let _ = analyze_decision(
            left,
            right,
            &first_sequences,
            BoundedDecisionWitness::Unsupported,
        );
        assert_eq!(
            FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.get(),
            frontier_computations,
            "repeated unsupported decisions must reuse the analysis-run frontier cache",
        );
    }

    #[test]
    fn single_site_cardinalities_match_direct_decision_witnesses() {
        let mut grammar = GrammarDescription::new(
            "single-site-cardinalities",
            1,
            3,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let item_declaration = test_declaration(&mut grammar, &mut next_byte, "ITEM");
        let item_kind = grammar.push_kind(crate::KindDescription::new("ITEM", item_declaration));
        let item_atom = grammar.push_atom(crate::AtomDescription::new(
            "ITEM",
            item_declaration,
            AtomExpression::Kind(item_kind),
        ));
        let item = grammar.push_type(crate::TypeDescription::new(
            "Item",
            item_declaration,
            TypeShape::Token {
                atom: item_atom,
                text_predicate: None,
            },
        ));

        let owner = grammar.next_type_id();
        let direct_declaration = test_declaration(&mut grammar, &mut next_byte, "Owner::Direct");
        let direct = grammar.push_site(crate::ParseSite::new(
            owner,
            item,
            direct_declaration,
            crate::ContainerShape::Direct,
        ));
        let shapes = [
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
            crate::ContainerShape::zero_or_more(crate::ContainerShape::Direct, None),
            crate::ContainerShape::one_or_more(crate::ContainerShape::Direct, None),
            crate::ContainerShape::optional(crate::ContainerShape::one_or_more(
                crate::ContainerShape::Direct,
                None,
            )),
        ];
        let shaped = shapes
            .into_iter()
            .enumerate()
            .map(|(index, shape)| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Owner::{index}"));
                grammar.push_site(crate::ParseSite::new(owner, item, declaration, shape))
            })
            .collect::<Vec<_>>();
        let owner_declaration = test_declaration(&mut grammar, &mut next_byte, "Owner");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Owner",
                owner_declaration,
                TypeShape::Sequence {
                    sites: std::iter::once(direct)
                        .chain(shaped.iter().copied())
                        .collect(),
                },
            )),
            owner,
        );

        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        for &site in &shaped {
            let BoundedDecisionWitness::Exact(Some(frozen)) =
                bounded_site_language_witness(&[site], &[direct], &grammar, &first_sequences)
            else {
                panic!("each cardinality overlaps one direct item");
            };
            let direct_witness = decision_witness_at_depth(
                &paired_residual(
                    DecisionState::from_sites(&[site]),
                    DecisionState::from_sites(&[direct]),
                    &grammar,
                ),
                &grammar,
                &atom_kinds,
                grammar.max_lookahead,
            )
            .expect("direct decision traversal sees the same overlap");
            assert_eq!(frozen, direct_witness);
        }
        for (current, tail) in [
            (shaped[0], direct),
            (shaped[0], shaped[2]),
            (shaped[1], direct),
            (shaped[1], shaped[2]),
        ] {
            let tail_sites = [tail];
            let left = DecisionState::consuming_site_and_tail(
                &grammar.sites[current.index()],
                &tail_sites,
            );
            let right = DecisionState::from_sites(&tail_sites);
            let BoundedDecisionWitness::Exact(frozen) = bounded_nullable_tail_witness(
                &grammar.sites[current.index()],
                &tail_sites,
                &left,
                &right,
                &grammar,
                &first_sequences,
            ) else {
                panic!("zero-binding-power nullable current and tail states are supported");
            };
            let direct_witness = decision_witness_at_depth(
                &paired_residual(left, right, &grammar),
                &grammar,
                &atom_kinds,
                grammar.max_lookahead,
            );
            assert_eq!(frozen, direct_witness);
        }
    }

    #[test]
    fn text_predicates_match_direct_ascii_insensitive_witnesses() {
        let mut grammar = GrammarDescription::new(
            "single-site-text-predicates",
            1,
            2,
            TextEquivalence::AsciiInsensitive,
        );
        let mut next_byte = 0;
        let word_declaration = test_declaration(&mut grammar, &mut next_byte, "WORD");
        let word_kind = grammar.push_kind(crate::KindDescription::new("WORD", word_declaration));
        let word_atom = grammar.push_atom(crate::AtomDescription::new(
            "WORD",
            word_declaration,
            AtomExpression::Kind(word_kind),
        ));
        let predicates = [
            TextPredicate::Any,
            TextPredicate::Exact("A".to_owned()),
            TextPredicate::OneOf(vec!["a".to_owned(), "B".to_owned()]),
            TextPredicate::NotIn(vec!["b".to_owned()]),
        ]
        .into_iter()
        .map(|predicate| grammar.push_text_predicate(predicate))
        .collect::<Vec<_>>();
        let targets = predicates
            .iter()
            .enumerate()
            .map(|(index, predicate)| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Word{index}"));
                grammar.push_type(crate::TypeDescription::new(
                    format!("Word{index}"),
                    declaration,
                    TypeShape::Token {
                        atom: word_atom,
                        text_predicate: Some(*predicate),
                    },
                ))
            })
            .collect::<Vec<_>>();
        let owner = grammar.next_type_id();
        let sites = targets
            .iter()
            .enumerate()
            .flat_map(|(index, target)| {
                let repeated_declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Owner::Repeated{index}"),
                );
                let repeated = grammar.push_site(crate::ParseSite::new(
                    owner,
                    *target,
                    repeated_declaration,
                    crate::ContainerShape::one_or_more(crate::ContainerShape::Direct, None),
                ));
                let direct_declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Owner::Direct{index}"),
                );
                let direct = grammar.push_site(crate::ParseSite::new(
                    owner,
                    *target,
                    direct_declaration,
                    crate::ContainerShape::Direct,
                ));
                [repeated, direct]
            })
            .collect::<Vec<_>>();
        let owner_declaration = test_declaration(&mut grammar, &mut next_byte, "Owner");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Owner",
                owner_declaration,
                TypeShape::Sequence {
                    sites: sites.clone(),
                },
            )),
            owner,
        );

        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        for (left_index, right_index) in [(0, 1), (1, 2), (1, 3), (2, 3)] {
            let left = sites[left_index * 2];
            let right = sites[right_index * 2 + 1];
            let BoundedDecisionWitness::Exact(Some(frozen)) =
                bounded_site_language_witness(&[left], &[right], &grammar, &first_sequences)
            else {
                panic!("ASCII-insensitive predicates overlap on a concrete spelling");
            };
            let direct = decision_witness_at_depth(
                &paired_residual(
                    DecisionState::from_sites(&[left]),
                    DecisionState::from_sites(&[right]),
                    &grammar,
                ),
                &grammar,
                &atom_kinds,
                grammar.max_lookahead,
            )
            .expect("direct predicate traversal sees the same overlap");
            assert_eq!(frozen, direct);
            assert!(matches!(frozen.first(), Some(DecisionWitness::Token(_))));
        }
    }

    #[test]
    fn bounded_witness_indexes_correlated_mostly_disjoint_prefixes() {
        const BRANCHES: usize = 256;
        let kind_count = BRANCHES * 2 + 1;
        let grammar = GrammarDescription::new(
            "indexed-bounded-witness",
            kind_count,
            3,
            TextEquivalence::Sensitive,
        );
        let position = |kind| FirstPosition {
            kinds: KindSet::singleton(kind_count, KindId::new(kind))
                .expect("test kind is within the grammar width"),
            text_predicate: None,
        };
        let shared = position(0);
        let left = (0..BRANCHES)
            .map(|branch| FirstSequence {
                positions: vec![shared.clone(), position(branch as u16 + 1)],
                ends: true,
            })
            .collect::<Vec<_>>();
        let right = (0..BRANCHES)
            .map(|branch| FirstSequence {
                positions: vec![
                    shared.clone(),
                    position(if branch == 0 {
                        1
                    } else {
                        (BRANCHES + branch) as u16
                    }),
                ],
                ends: true,
            })
            .collect::<Vec<_>>();

        BOUNDED_WITNESS_EDGE_PAIR_CHECKS.set(0);
        assert_eq!(
            bounded_first_sequence_witness(&left, &right, &grammar),
            Some(vec![
                DecisionWitness::Token(crate::LookaheadWitnessPosition {
                    kind: KindId::new(0),
                    left_text_predicate: None,
                    right_text_predicate: None,
                }),
                DecisionWitness::Token(crate::LookaheadWitnessPosition {
                    kind: KindId::new(1),
                    left_text_predicate: None,
                    right_text_predicate: None,
                }),
                DecisionWitness::End,
            ]),
        );
        assert!(
            BOUNDED_WITNESS_EDGE_PAIR_CHECKS.get() <= BRANCHES * 4,
            "shared prefixes must be compared once and mostly-disjoint suffixes indexed; checks={}",
            BOUNDED_WITNESS_EDGE_PAIR_CHECKS.get(),
        );
    }

    #[test]
    fn indexed_bounded_witness_matches_the_cartesian_oracle_exhaustively() {
        for equivalence in [
            TextEquivalence::Sensitive,
            TextEquivalence::AsciiInsensitive,
        ] {
            let mut grammar = GrammarDescription::new("bounded-witness-oracle", 3, 2, equivalence);
            let any = grammar.push_text_predicate(TextPredicate::Any);
            let exact_a = grammar.push_text_predicate(TextPredicate::Exact("A".to_owned()));
            let exact_a_alias = grammar.push_text_predicate(TextPredicate::Exact("A".to_owned()));
            let one_of = grammar
                .push_text_predicate(TextPredicate::OneOf(vec!["a".to_owned(), "B".to_owned()]));
            let not_in_b = grammar.push_text_predicate(TextPredicate::NotIn(vec!["b".to_owned()]));
            let exact_z = grammar.push_text_predicate(TextPredicate::Exact("Z".to_owned()));
            let kinds = |members: &[u16]| FirstPosition {
                kinds: KindSet::from_kinds(3, members.iter().copied().map(KindId::new))
                    .expect("oracle kinds are within the grammar width"),
                text_predicate: None,
            };
            let mut kind_zero = kinds(&[0]);
            kind_zero.text_predicate = Some(any);
            let mut wide = kinds(&[0, 1]);
            wide.text_predicate = Some(exact_a);
            let mut kind_one = kinds(&[1]);
            kind_one.text_predicate = Some(one_of);
            let mut kind_two_not_in = kinds(&[2]);
            kind_two_not_in.text_predicate = Some(not_in_b);
            let mut wide_not_in = kinds(&[0, 1]);
            wide_not_in.text_predicate = Some(not_in_b);
            let mut kind_two_exact = kinds(&[2]);
            kind_two_exact.text_predicate = Some(exact_z);
            let universe = [
                FirstSequence {
                    positions: Vec::new(),
                    ends: false,
                },
                FirstSequence {
                    positions: Vec::new(),
                    ends: true,
                },
                FirstSequence {
                    positions: vec![kind_zero],
                    ends: false,
                },
                FirstSequence {
                    positions: vec![wide],
                    ends: true,
                },
                FirstSequence {
                    positions: vec![kind_one, kind_two_not_in],
                    ends: true,
                },
                FirstSequence {
                    positions: vec![wide_not_in, kind_two_exact],
                    ends: false,
                },
            ];
            let languages = (0..1_usize << universe.len())
                .map(|members| {
                    universe
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| members & (1 << index) != 0)
                        .map(|(_, sequence)| sequence.clone())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            for (left_index, left) in languages.iter().enumerate() {
                for (right_index, right) in languages.iter().enumerate() {
                    assert_eq!(
                        bounded_first_sequence_witness(left, right, &grammar),
                        naive_bounded_first_sequence_witness(left, right, &grammar),
                        "indexed witness diverged for {equivalence:?} language masks {left_index:#08b} and {right_index:#08b}",
                    );
                }
            }

            let unrefined = vec![FirstSequence {
                positions: vec![kinds(&[0])],
                ends: true,
            }];
            let mut explicit_any_position = kinds(&[0]);
            explicit_any_position.text_predicate = Some(any);
            let explicit_any = vec![FirstSequence {
                positions: vec![explicit_any_position],
                ends: true,
            }];
            assert_eq!(
                bounded_first_sequence_witness(&unrefined, &explicit_any, &grammar),
                naive_bounded_first_sequence_witness(&unrefined, &explicit_any, &grammar),
                "a nonempty unrefined position is semantically Any",
            );

            let mut original_position = kinds(&[0]);
            original_position.text_predicate = Some(exact_a);
            let mut alias_position = kinds(&[0]);
            alias_position.text_predicate = Some(exact_a_alias);
            let equivalent_left = vec![
                FirstSequence {
                    positions: vec![alias_position],
                    ends: true,
                },
                FirstSequence {
                    positions: vec![original_position],
                    ends: true,
                },
            ];
            let canonical =
                bounded_first_sequence_witness(&equivalent_left, &explicit_any, &grammar);
            assert_eq!(
                canonical,
                naive_bounded_first_sequence_witness(&equivalent_left, &explicit_any, &grammar,),
            );
            assert_eq!(
                canonical,
                Some(vec![
                    DecisionWitness::Token(crate::LookaheadWitnessPosition {
                        kind: KindId::new(0),
                        left_text_predicate: Some(exact_a),
                        right_text_predicate: Some(any),
                    }),
                    DecisionWitness::End,
                ]),
                "semantically equal predicate IDs retain canonical witness ordering",
            );
        }
    }

    #[test]
    fn nullable_current_vs_tail_uses_the_direct_bounded_witness_cache() {
        const BRANCHES: usize = 16;
        let mut grammar = GrammarDescription::new(
            "nullable-current-vs-tail",
            BRANCHES,
            3,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let tokens = (0..BRANCHES)
            .map(|branch| {
                let name = format!("TOKEN_{branch}");
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let kind = grammar.push_kind(crate::KindDescription::new(&name, declaration));
                let atom = grammar.push_atom(crate::AtomDescription::new(
                    &name,
                    declaration,
                    AtomExpression::Kind(kind),
                ));
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Token {
                        atom,
                        text_predicate: None,
                    },
                ))
            })
            .collect::<Vec<_>>();
        let choice = grammar.next_type_id();
        let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
        let alternatives = tokens
            .into_iter()
            .enumerate()
            .map(|(branch, target)| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Choice::{branch}"));
                let site = grammar.push_site(crate::ParseSite::new(
                    choice,
                    target,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum { alternatives },
            )),
            choice,
        );
        let root = grammar.next_type_id();
        let current_declaration = test_declaration(&mut grammar, &mut next_byte, "Root::current");
        let current = grammar.push_site(crate::ParseSite::new(
            root,
            choice,
            current_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        let tail_declaration = test_declaration(&mut grammar, &mut next_byte, "Root::tail");
        let tail = grammar.push_site(crate::ParseSite::new(
            root,
            choice,
            tail_declaration,
            crate::ContainerShape::one_or_more(crate::ContainerShape::Direct, None),
        ));
        let root_declaration = test_declaration(&mut grammar, &mut next_byte, "Root");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Root",
                root_declaration,
                TypeShape::Sequence {
                    sites: vec![current, tail],
                },
            )),
            root,
        );

        DECISION_WITNESS_STATE_EXPANSIONS.set(0);
        FIRST_LANGUAGE_STATE_COUNT.set(0);
        let report = analyze(&grammar);
        assert_eq!(
            FIRST_LANGUAGE_STATE_COUNT.get(),
            0,
            "nullable current-vs-tail witnesses must not materialize complete FIRST-k languages",
        );
        assert!(
            DECISION_WITNESS_STATE_EXPANSIONS.get() <= BRANCHES * grammar.max_lookahead,
            "direct nullable-tail witness work must remain bounded by reachable paired residuals; expansions={}",
            DECISION_WITNESS_STATE_EXPANSIONS.get(),
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::StaticUseSiteOverlap),
            "frozen witness reuse must not suppress the unresolved static conflict",
        );
    }

    #[test]
    fn unproductive_required_sites_do_not_bypass_enum_or_pratt_conflicts() {
        fn grammar_with_unproductive_choice(pratt: bool) -> GrammarDescription {
            let mut grammar = GrammarDescription::new(
                "unproductive-required-site",
                1,
                1,
                TextEquivalence::Sensitive,
            );
            let mut next_byte = 0;
            let a_declaration = test_declaration(&mut grammar, &mut next_byte, "A");
            let a_kind = grammar.push_kind(crate::KindDescription::new("A", a_declaration));
            let a_atom = grammar.push_atom(crate::AtomDescription::new(
                "A",
                a_declaration,
                AtomExpression::Kind(a_kind),
            ));
            let a = grammar.push_type(crate::TypeDescription::new(
                "A",
                a_declaration,
                TypeShape::Token {
                    atom: a_atom,
                    text_predicate: None,
                },
            ));

            let tail = grammar.next_type_id();
            let tail_a_declaration = test_declaration(&mut grammar, &mut next_byte, "Tail::a");
            let tail_a = grammar.push_site(crate::ParseSite::new(
                tail,
                a,
                tail_a_declaration,
                crate::ContainerShape::Direct,
            ));
            let tail_self_declaration =
                test_declaration(&mut grammar, &mut next_byte, "Tail::self");
            let tail_self = grammar.push_site(crate::ParseSite::new(
                tail,
                tail,
                tail_self_declaration,
                crate::ContainerShape::Direct,
            ));
            let tail_declaration = test_declaration(&mut grammar, &mut next_byte, "Tail");
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    "Tail",
                    tail_declaration,
                    TypeShape::Sequence {
                        sites: vec![tail_a, tail_self],
                    },
                )),
                tail,
            );

            let choice = grammar.next_type_id();
            let tail_choice_declaration =
                test_declaration(&mut grammar, &mut next_byte, "Choice::Tail");
            let tail_choice = grammar.push_site(crate::ParseSite::new(
                choice,
                tail,
                tail_choice_declaration,
                crate::ContainerShape::Direct,
            ));
            let a_choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice::A");
            let a_choice = grammar.push_site(crate::ParseSite::new(
                choice,
                a,
                a_choice_declaration,
                crate::ContainerShape::Direct,
            ));
            let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
            let shape = if pratt {
                TypeShape::Pratt {
                    variants: vec![
                        crate::PrattVariant {
                            declaration: tail_choice_declaration,
                            position: PrattPosition::Atom,
                            sites: vec![tail_choice],
                        },
                        crate::PrattVariant {
                            declaration: a_choice_declaration,
                            position: PrattPosition::Atom,
                            sites: vec![a_choice],
                        },
                    ],
                }
            } else {
                TypeShape::Enum {
                    alternatives: vec![
                        crate::Alternative {
                            declaration: tail_choice_declaration,
                            sites: vec![tail_choice],
                        },
                        crate::Alternative {
                            declaration: a_choice_declaration,
                            sites: vec![a_choice],
                        },
                    ],
                }
            };
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    "Choice",
                    choice_declaration,
                    shape,
                )),
                choice,
            );
            grammar
        }

        for (pratt, code) in [
            (false, DiagnosticCode::EnumFirstOverlap),
            (true, DiagnosticCode::PrattAtomOverlap),
        ] {
            DECISION_WITNESS_STATE_EXPANSIONS.set(0);
            let report = analyze(&grammar_with_unproductive_choice(pratt));
            assert!(
                DECISION_WITNESS_STATE_EXPANSIONS.get() > 0,
                "unproductive required sites must retain explicit traversal",
            );
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "unproductive required sites must not suppress {code:?}",
            );
        }
    }

    #[test]
    fn productive_choices_with_dead_shared_prefixes_use_explicit_conflict_analysis() {
        fn push_token(grammar: &mut GrammarDescription, next_byte: &mut u32, name: &str) -> TypeId {
            let declaration = test_declaration(grammar, next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        }

        fn push_choice(
            grammar: &mut GrammarDescription,
            next_byte: &mut u32,
            name: &str,
            alternatives: &[&[TypeId]],
        ) -> TypeId {
            let owner = grammar.next_type_id();
            let declaration = test_declaration(grammar, next_byte, name);
            let alternatives = alternatives
                .iter()
                .enumerate()
                .map(|(alternative_index, targets)| {
                    let alternative = test_declaration(
                        grammar,
                        next_byte,
                        &format!("{name}::{alternative_index}"),
                    );
                    let sites = targets
                        .iter()
                        .enumerate()
                        .map(|(site_index, target)| {
                            let site = test_declaration(
                                grammar,
                                next_byte,
                                &format!("{name}::{alternative_index}::{site_index}"),
                            );
                            grammar.push_site(crate::ParseSite::new(
                                owner,
                                *target,
                                site,
                                crate::ContainerShape::Direct,
                            ))
                        })
                        .collect();
                    crate::Alternative {
                        declaration: alternative,
                        sites,
                    }
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Enum { alternatives },
                )),
                owner,
            );
            owner
        }

        let mut grammar = GrammarDescription::new(
            "productive-choice-with-dead-prefix",
            5,
            1,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let a = push_token(&mut grammar, &mut next_byte, "A");
        let b = push_token(&mut grammar, &mut next_byte, "B");
        let c = push_token(&mut grammar, &mut next_byte, "C");
        let d = push_token(&mut grammar, &mut next_byte, "D");
        let e = push_token(&mut grammar, &mut next_byte, "E");
        let empty_declaration = test_declaration(&mut grammar, &mut next_byte, "Empty");
        let empty = grammar.push_type(crate::TypeDescription::new(
            "Empty",
            empty_declaration,
            TypeShape::Enum {
                alternatives: Vec::new(),
            },
        ));
        let left = push_choice(&mut grammar, &mut next_byte, "Left", &[&[a, empty], &[b]]);
        let right = push_choice(&mut grammar, &mut next_byte, "Right", &[&[a, empty], &[c]]);
        let _root = push_choice(
            &mut grammar,
            &mut next_byte,
            "Root",
            &[&[left, d], &[right, e]],
        );

        DECISION_WITNESS_STATE_EXPANSIONS.set(0);
        let report = analyze(&grammar);
        assert!(
            DECISION_WITNESS_STATE_EXPANSIONS.get() > 0,
            "a reachable dead prefix must retain explicit configured-depth traversal",
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::EnumFirstOverlap),
            "a dead shared A prefix must not be erased by productive B/C alternatives",
        );
    }

    #[test]
    fn unproductive_nullable_tail_falls_back_to_explicit_conflict_analysis() {
        let mut grammar = GrammarDescription::new(
            "unproductive-nullable-tail",
            1,
            1,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let a_declaration = test_declaration(&mut grammar, &mut next_byte, "A");
        let a_kind = grammar.push_kind(crate::KindDescription::new("A", a_declaration));
        let a_atom = grammar.push_atom(crate::AtomDescription::new(
            "A",
            a_declaration,
            AtomExpression::Kind(a_kind),
        ));
        let a = grammar.push_type(crate::TypeDescription::new(
            "A",
            a_declaration,
            TypeShape::Token {
                atom: a_atom,
                text_predicate: None,
            },
        ));

        let tail = grammar.next_type_id();
        let tail_a_declaration = test_declaration(&mut grammar, &mut next_byte, "Tail::a");
        let tail_a = grammar.push_site(crate::ParseSite::new(
            tail,
            a,
            tail_a_declaration,
            crate::ContainerShape::Direct,
        ));
        let tail_self_declaration = test_declaration(&mut grammar, &mut next_byte, "Tail::self");
        let tail_self = grammar.push_site(crate::ParseSite::new(
            tail,
            tail,
            tail_self_declaration,
            crate::ContainerShape::Direct,
        ));
        let tail_declaration = test_declaration(&mut grammar, &mut next_byte, "Tail");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Tail",
                tail_declaration,
                TypeShape::Sequence {
                    sites: vec![tail_a, tail_self],
                },
            )),
            tail,
        );

        let root = grammar.next_type_id();
        let current_declaration = test_declaration(&mut grammar, &mut next_byte, "Root::current");
        let current = grammar.push_site(crate::ParseSite::new(
            root,
            a,
            current_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        let tail_site_declaration = test_declaration(&mut grammar, &mut next_byte, "Root::tail");
        let tail_site = grammar.push_site(crate::ParseSite::new(
            root,
            tail,
            tail_site_declaration,
            crate::ContainerShape::Direct,
        ));
        let root_declaration = test_declaration(&mut grammar, &mut next_byte, "Root");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Root",
                root_declaration,
                TypeShape::Sequence {
                    sites: vec![current, tail_site],
                },
            )),
            root,
        );

        DECISION_WITNESS_STATE_EXPANSIONS.set(0);
        let report = analyze(&grammar);
        assert!(
            DECISION_WITNESS_STATE_EXPANSIONS.get() > 0,
            "an empty bounded language for an unproductive state must retain explicit traversal",
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::StaticUseSiteOverlap),
            "unproductive bounded languages must not suppress the overlapping nullable tail",
        );
    }

    #[test]
    fn separable_nullable_current_vs_tail_skips_an_irrelevant_explicit_witness() {
        let mut grammar = GrammarDescription::new(
            "separable-nullable-current-vs-tail",
            1,
            3,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let item_declaration = test_declaration(&mut grammar, &mut next_byte, "ITEM");
        let item_kind = grammar.push_kind(crate::KindDescription::new("ITEM", item_declaration));
        let item_atom = grammar.push_atom(crate::AtomDescription::new(
            "ITEM",
            item_declaration,
            AtomExpression::Kind(item_kind),
        ));
        let item = grammar.push_type(crate::TypeDescription::new(
            "Item",
            item_declaration,
            TypeShape::Token {
                atom: item_atom,
                text_predicate: None,
            },
        ));
        let root = grammar.next_type_id();
        let current_declaration = test_declaration(&mut grammar, &mut next_byte, "Root::current");
        let current = grammar.push_site(crate::ParseSite::new(
            root,
            item,
            current_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        let tail_declaration = test_declaration(&mut grammar, &mut next_byte, "Root::tail");
        let tail = grammar.push_site(crate::ParseSite::new(
            root,
            item,
            tail_declaration,
            crate::ContainerShape::Direct,
        ));
        let root_declaration = test_declaration(&mut grammar, &mut next_byte, "Root");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Root",
                root_declaration,
                TypeShape::Sequence {
                    sites: vec![current, tail],
                },
            )),
            root,
        );

        CONFLICT_DECISION_PRODUCT_BUILDS.set(0);
        DECISION_WITNESS_STATE_EXPANSIONS.set(0);
        let _ = analyze(&grammar);
        assert_eq!(
            CONFLICT_DECISION_PRODUCT_BUILDS.get(),
            0,
            "an exact absent bounded witness must skip the irrelevant unbounded product",
        );
        assert!(
            DECISION_WITNESS_STATE_EXPANSIONS.get() <= grammar.max_lookahead,
            "the direct exact witness must prove separation with bounded residual work; expansions={}",
            DECISION_WITNESS_STATE_EXPANSIONS.get(),
        );
    }

    #[test]
    fn disjoint_statement_like_alternatives_skip_paired_decision_products() {
        const BRANCH_COUNT: usize = 64;
        const WRAPPER_DEPTH: usize = 4;
        let mut grammar = GrammarDescription::new(
            "disjoint-statement-like-alternatives",
            BRANCH_COUNT,
            5,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let mut alternatives = Vec::with_capacity(BRANCH_COUNT);
        for branch in 0..BRANCH_COUNT {
            let token_name = format!("Token{branch}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &token_name);
            let kind = grammar.push_kind(crate::KindDescription::new(&token_name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                &token_name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            let mut target = grammar.push_type(crate::TypeDescription::new(
                &token_name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ));
            for depth in 0..WRAPPER_DEPTH {
                let name = format!("Branch{branch}Wrapper{depth}");
                let owner = grammar.next_type_id();
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let site_declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("{name}::value"));
                let site = grammar.push_site(crate::ParseSite::new(
                    owner,
                    target,
                    site_declaration,
                    crate::ContainerShape::Direct,
                ));
                assert_eq!(
                    grammar.push_type(crate::TypeDescription::new(
                        name,
                        declaration,
                        TypeShape::Sequence { sites: vec![site] },
                    )),
                    owner,
                );
                target = owner;
            }
            alternatives.push(target);
        }

        let optional_sequence = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "OptionalThenRequired");
        let optional_declaration = test_declaration(
            &mut grammar,
            &mut next_byte,
            "OptionalThenRequired::optional",
        );
        let optional = grammar.push_site(crate::ParseSite::new(
            optional_sequence,
            alternatives[0],
            optional_declaration,
            crate::ContainerShape::optional(crate::ContainerShape::Direct),
        ));
        let required_declaration = test_declaration(
            &mut grammar,
            &mut next_byte,
            "OptionalThenRequired::required",
        );
        let required = grammar.push_site(crate::ParseSite::new(
            optional_sequence,
            alternatives[1],
            required_declaration,
            crate::ContainerShape::Direct,
        ));
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "OptionalThenRequired",
                declaration,
                TypeShape::Sequence {
                    sites: vec![optional, required],
                },
            )),
            optional_sequence,
        );

        let pratt = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "DisjointPratt");
        let variants = alternatives[2..4]
            .iter()
            .enumerate()
            .map(|(branch, target)| {
                let declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("DisjointPratt::Branch{branch}"),
                );
                let site = grammar.push_site(crate::ParseSite::new(
                    pratt,
                    *target,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::PrattVariant {
                    declaration,
                    position: PrattPosition::Atom,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "DisjointPratt",
                declaration,
                TypeShape::Pratt { variants },
            )),
            pratt,
        );

        let statement = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "Statement");
        let alternatives = alternatives
            .into_iter()
            .enumerate()
            .map(|(branch, target)| {
                let declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Statement::Branch{branch}"),
                );
                let site = grammar.push_site(crate::ParseSite::new(
                    statement,
                    target,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Statement",
                declaration,
                TypeShape::Enum { alternatives },
            )),
            statement,
        );

        CONFLICT_DECISION_PRODUCT_BUILDS.set(0);
        let report = analyze(&grammar);
        assert!(report.diagnostics.is_empty());
        assert_eq!(
            CONFLICT_DECISION_PRODUCT_BUILDS.get(),
            0,
            "disjoint FIRST languages must not build pairwise decision products",
        );
    }

    #[test]
    fn bounded_first_languages_skip_paired_products_for_shared_prefixes() {
        const BRANCH_COUNT: usize = 32;
        let mut grammar = GrammarDescription::new(
            "bounded-statement-like-alternatives",
            BRANCH_COUNT + 1,
            5,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let (shared, suffixes) = {
            let mut push_token = |grammar: &mut GrammarDescription, name: String| {
                let declaration = test_declaration(grammar, &mut next_byte, &name);
                let kind = grammar.push_kind(crate::KindDescription::new(&name, declaration));
                let atom = grammar.push_atom(crate::AtomDescription::new(
                    &name,
                    declaration,
                    AtomExpression::Kind(kind),
                ));
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Token {
                        atom,
                        text_predicate: None,
                    },
                ))
            };
            let shared = push_token(&mut grammar, "Shared".to_owned());
            let suffixes = (0..BRANCH_COUNT)
                .map(|branch| push_token(&mut grammar, format!("Suffix{branch}")))
                .collect::<Vec<_>>();
            (shared, suffixes)
        };

        let mut branches = Vec::with_capacity(BRANCH_COUNT);
        for (branch, suffix) in suffixes.into_iter().enumerate() {
            let name = format!("Branch{branch}");
            let owner = grammar.next_type_id();
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            let sites = [shared, suffix]
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let declaration =
                        test_declaration(&mut grammar, &mut next_byte, &format!("{name}::{index}"));
                    grammar.push_site(crate::ParseSite::new(
                        owner,
                        target,
                        declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Sequence { sites },
                )),
                owner,
            );
            branches.push(owner);
        }

        let statement = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "Statement");
        let alternatives = branches
            .into_iter()
            .enumerate()
            .map(|(branch, target)| {
                let declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Statement::Branch{branch}"),
                );
                let site = grammar.push_site(crate::ParseSite::new(
                    statement,
                    target,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Statement",
                declaration,
                TypeShape::Enum { alternatives },
            )),
            statement,
        );

        CONFLICT_DECISION_PRODUCT_BUILDS.set(0);
        let report = analyze(&grammar);
        assert!(report.diagnostics.is_empty());
        assert_eq!(
            CONFLICT_DECISION_PRODUCT_BUILDS.get(),
            0,
            "bounded FIRST languages already prove every shared prefix disjoint",
        );
    }

    #[test]
    fn bounded_multi_site_alternatives_skip_pairwise_products() {
        const BRANCH_COUNT: usize = 32;
        let mut grammar = GrammarDescription::new(
            "bounded-multi-site-alternatives",
            BRANCH_COUNT + 1,
            2,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let mut push_token = |grammar: &mut GrammarDescription, name: String| {
            let declaration = test_declaration(grammar, &mut next_byte, &name);
            let kind = grammar.push_kind(crate::KindDescription::new(&name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                &name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        };
        let shared = push_token(&mut grammar, "Shared".to_owned());
        let suffixes = (0..BRANCH_COUNT)
            .map(|branch| push_token(&mut grammar, format!("Suffix{branch}")))
            .collect::<Vec<_>>();
        let statement = grammar.next_type_id();
        let alternatives = suffixes
            .into_iter()
            .enumerate()
            .map(|(branch, suffix)| {
                let declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Statement::Branch{branch}"),
                );
                let sites = [shared, suffix]
                    .into_iter()
                    .enumerate()
                    .map(|(index, target)| {
                        let site_declaration = test_declaration(
                            &mut grammar,
                            &mut next_byte,
                            &format!("Statement::Branch{branch}::{index}"),
                        );
                        grammar.push_site(crate::ParseSite::new(
                            statement,
                            target,
                            site_declaration,
                            crate::ContainerShape::Direct,
                        ))
                    })
                    .collect();
                crate::Alternative { declaration, sites }
            })
            .collect();
        let statement_declaration = test_declaration(&mut grammar, &mut next_byte, "Statement");
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Statement",
                statement_declaration,
                TypeShape::Enum { alternatives },
            )),
            statement,
        );

        CONFLICT_DECISION_PRODUCT_BUILDS.set(0);
        let report = analyze(&grammar);
        assert!(report.diagnostics.is_empty());
        assert_eq!(
            CONFLICT_DECISION_PRODUCT_BUILDS.get(),
            0,
            "exact cached multi-site languages must reject every disjoint pair before the unbounded product",
        );
    }

    #[test]
    fn correlated_prefixes_bound_working_first_states_before_projection() {
        const BRANCH_COUNT: usize = 32;
        FIRST_SEQUENCE_PEAK_ROWS.set(0);
        FIRST_LANGUAGE_STATE_COUNT.set(0);
        FIRST_LANGUAGE_TRANSITION_OPERATIONS.set(0);
        FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.set(0);
        let (grammar, root) = correlated_pair_grammar(BRANCH_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let root = arena.build(
            grammar.max_lookahead,
            vec![DecisionState(vec![DecisionItem::Type(root)])],
        );
        let language = arena.materialize(root);

        assert_eq!(language.len(), BRANCH_COUNT * BRANCH_COUNT);
        assert!(
            FIRST_SEQUENCE_PEAK_ROWS.get() <= 4_096,
            "working FIRST must stay proportional to canonical states, not the eager product",
        );
        assert!(
            FIRST_LANGUAGE_STATE_COUNT.get() <= 4_096,
            "bounded symbolic composition must remain proportional to residual states",
        );
        assert!(
            FIRST_LANGUAGE_TRANSITION_OPERATIONS.get() <= 4_096,
            "bounded symbolic composition must not visit the Cartesian row product",
        );
        assert!(
            FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.get() <= BRANCH_COUNT * 4,
            "projected transition work must stay proportional to structural arcs, not kind width; inputs={}",
            FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.get(),
        );
    }

    #[test]
    fn canonical_witness_search_stops_after_first_correlated_path() {
        const BRANCH_COUNT: usize = 16;
        let (grammar, root) = correlated_pair_grammar(BRANCH_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let state = DecisionState(vec![DecisionItem::Type(root)]);
        let paired = paired_residual(state.clone(), state, &grammar);
        let analysis = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        DECISION_WITNESS_STATE_EXPANSIONS.set(0);

        let witness = analysis
            .decision_witness_at_depth(&paired, grammar.max_lookahead)
            .expect("a productive language overlaps itself");

        assert!(!witness.is_empty());
        assert!(
            DECISION_WITNESS_STATE_EXPANSIONS.get() <= grammar.max_lookahead,
            "canonical witness search must not enumerate unrelated correlated paths; expansions={}",
            DECISION_WITNESS_STATE_EXPANSIONS.get(),
        );
        DECISION_WITNESS_STATE_EXPANSIONS.set(0);
        assert_eq!(
            analysis.decision_witness_at_depth(&paired, grammar.max_lookahead),
            Some(witness),
        );
        assert_eq!(
            DECISION_WITNESS_STATE_EXPANSIONS.get(),
            0,
            "repeated conflict sites must share the exact residual witness cache",
        );
    }

    #[test]
    fn included_language_witness_skips_the_paired_cartesian_frontier() {
        const BRANCH_COUNT: usize = 32;
        let mut grammar = GrammarDescription::new(
            "included-language-witness",
            BRANCH_COUNT + 1,
            2,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let shared_declaration = test_declaration(&mut grammar, &mut next_byte, "SHARED");
        let shared_kind =
            grammar.push_kind(crate::KindDescription::new("SHARED", shared_declaration));
        let shared_atom = grammar.push_atom(crate::AtomDescription::new(
            "SHARED",
            shared_declaration,
            AtomExpression::Kind(shared_kind),
        ));
        let shared = grammar.push_type(crate::TypeDescription::new(
            "Shared",
            shared_declaration,
            TypeShape::Token {
                atom: shared_atom,
                text_predicate: None,
            },
        ));
        let tails = (0..BRANCH_COUNT)
            .map(|index| {
                let name = format!("TAIL_{index}");
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let kind = grammar.push_kind(crate::KindDescription::new(&name, declaration));
                let atom = grammar.push_atom(crate::AtomDescription::new(
                    &name,
                    declaration,
                    AtomExpression::Kind(kind),
                ));
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Token {
                        atom,
                        text_predicate: None,
                    },
                ))
            })
            .collect::<Vec<_>>();
        let choice = grammar.next_type_id();
        let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
        let alternatives = tails
            .into_iter()
            .enumerate()
            .map(|(index, tail)| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Choice::{index}"));
                let sites = [shared, tail]
                    .into_iter()
                    .enumerate()
                    .map(|(site_index, target)| {
                        let site_declaration = test_declaration(
                            &mut grammar,
                            &mut next_byte,
                            &format!("Choice::{index}::{site_index}"),
                        );
                        grammar.push_site(crate::ParseSite::new(
                            choice,
                            target,
                            site_declaration,
                            crate::ContainerShape::Direct,
                        ))
                    })
                    .collect();
                crate::Alternative { declaration, sites }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum { alternatives },
            )),
            choice,
        );
        let atom_kinds = expand_atoms(&grammar);
        let state = DecisionState(vec![DecisionItem::Type(choice)]);
        let paired = paired_residual(state.clone(), state, &grammar);
        let analysis = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        DECISION_PRODUCT_TRANSITION_PAIRS.set(0);

        let witness = analysis
            .decision_witness_at_depth(&paired, grammar.max_lookahead)
            .expect("a productive language overlaps itself");

        assert_eq!(witness.len(), grammar.max_lookahead);
        assert_eq!(
            DECISION_PRODUCT_TRANSITION_PAIRS.get(),
            0,
            "a proven included language must derive its witness without pairing every shared-token branch",
        );
    }

    #[test]
    fn production_sequence_materialization_skips_unused_deterministic_cells() {
        const BRANCH_COUNT: usize = 8;
        let (grammar, root) = correlated_pair_grammar(BRANCH_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let state = DecisionState(vec![DecisionItem::Type(root)]);
        let mut oracle = FirstLanguageArena::new(&grammar, &atom_kinds);
        let oracle_root = oracle.build(grammar.max_lookahead, vec![state.clone()]);
        let expected = oracle.materialize(oracle_root);

        FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.set(0);
        FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.set(0);
        let analysis = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        let actual = analysis.sequences_for_state(state);

        assert_eq!(actual, expected);
        assert_eq!(
            FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.get(),
            0,
            "public sequence materialization must not construct an unused deterministic product",
        );
        assert_eq!(
            FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.get(),
            0,
            "public sequence materialization must not precompute a duplicate projected graph",
        );
    }

    #[test]
    fn successful_analysis_materializes_aggregate_sequences_for_every_type() {
        let (grammar, _) = correlated_pair_grammar(4);
        FIRST_LANGUAGE_AGGREGATE_MATERIALIZATIONS.set(0);

        let report = analyze(&grammar);
        let output = report.output.expect("the grammar is conflict-free");

        assert_eq!(
            FIRST_LANGUAGE_AGGREGATE_MATERIALIZATIONS.get(),
            grammar.types.len(),
        );
        assert!(
            output
                .first_by_type
                .iter()
                .all(|facts| !facts.sequences.is_empty()),
            "successful semantic output retains the complete per-type FIRST-k projection",
        );
    }

    #[test]
    fn erroneous_analysis_skips_aggregate_sequence_publication() {
        let mut grammar = GrammarDescription::new("conflicting", 1, 1, TextEquivalence::Sensitive);
        let mut next_byte = 0;
        let token_declaration = test_declaration(&mut grammar, &mut next_byte, "A");
        let kind = grammar.push_kind(crate::KindDescription::new("A", token_declaration));
        let atom = grammar.push_atom(crate::AtomDescription::new(
            "A",
            token_declaration,
            AtomExpression::Kind(kind),
        ));
        let token = grammar.push_type(crate::TypeDescription::new(
            "A",
            token_declaration,
            TypeShape::Token {
                atom,
                text_predicate: None,
            },
        ));
        let choice = grammar.next_type_id();
        let choice_declaration = test_declaration(&mut grammar, &mut next_byte, "Choice");
        let alternatives = ["left", "right"]
            .into_iter()
            .map(|name| {
                let declaration = test_declaration(&mut grammar, &mut next_byte, name);
                let site = grammar.push_site(crate::ParseSite::new(
                    choice,
                    token,
                    declaration,
                    crate::ContainerShape::Direct,
                ));
                crate::Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Choice",
                choice_declaration,
                TypeShape::Enum { alternatives },
            )),
            choice,
        );
        FIRST_LANGUAGE_AGGREGATE_MATERIALIZATIONS.set(0);

        let report = analyze(&grammar);

        assert!(report.output.is_none());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::EnumFirstOverlap)
        );
        assert_eq!(FIRST_LANGUAGE_AGGREGATE_MATERIALIZATIONS.get(), 0);
    }

    #[test]
    fn json_table_like_pratt_sequence_bounds_projected_state_growth() {
        const BRANCH_COUNT: usize = 6;
        FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.set(0);
        FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.set(0);
        FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.set(0);
        let mut grammar = GrammarDescription::new(
            "json-table-like-pratt",
            BRANCH_COUNT * 4,
            5,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let tokens = (0..BRANCH_COUNT * 4)
            .map(|index| {
                let name = format!("K{index}");
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let kind = grammar.push_kind(crate::KindDescription::new(&name, declaration));
                let atom = grammar.push_atom(crate::AtomDescription::new(
                    &name,
                    declaration,
                    AtomExpression::Kind(kind),
                ));
                grammar.push_type(crate::TypeDescription::new(
                    format!("Token{index}"),
                    declaration,
                    TypeShape::Token {
                        atom,
                        text_predicate: None,
                    },
                ))
            })
            .collect::<Vec<_>>();
        let expr = grammar.next_type_id();
        let expression_declaration = test_declaration(&mut grammar, &mut next_byte, "Expr");
        let mut variants = Vec::new();
        for index in 0..BRANCH_COUNT {
            for (role, position, targets) in [
                (
                    "Atom",
                    PrattPosition::Atom,
                    vec![
                        (tokens[index], crate::ContainerShape::Direct),
                        (
                            expr,
                            crate::ContainerShape::zero_or_more(
                                crate::ContainerShape::Direct,
                                Some(tokens[BRANCH_COUNT * 3]),
                            ),
                        ),
                        (tokens[BRANCH_COUNT * 3 + 1], crate::ContainerShape::Direct),
                    ],
                ),
                (
                    "Prefix",
                    PrattPosition::Prefix { bp: 20 },
                    vec![
                        (tokens[BRANCH_COUNT + index], crate::ContainerShape::Direct),
                        (expr, crate::ContainerShape::Direct),
                    ],
                ),
                (
                    "Infix",
                    PrattPosition::Infix { lbp: 10, rbp: 11 },
                    vec![
                        (
                            tokens[BRANCH_COUNT * 2 + index],
                            crate::ContainerShape::Direct,
                        ),
                        (expr, crate::ContainerShape::Direct),
                    ],
                ),
                (
                    "Postfix",
                    PrattPosition::Postfix { bp: 15 },
                    vec![(
                        tokens[BRANCH_COUNT * 3 + index],
                        crate::ContainerShape::Direct,
                    )],
                ),
            ] {
                let declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("Expr::{role}{index}"),
                );
                let sites = targets
                    .into_iter()
                    .enumerate()
                    .map(|(site_index, (target, shape))| {
                        let site_declaration = test_declaration(
                            &mut grammar,
                            &mut next_byte,
                            &format!("Expr::{role}{index}::{site_index}"),
                        );
                        grammar.push_site(crate::ParseSite::new(
                            expr,
                            target,
                            site_declaration,
                            shape,
                        ))
                    })
                    .collect();
                variants.push(crate::PrattVariant {
                    declaration,
                    position,
                    sites,
                });
            }
        }
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Expr",
                expression_declaration,
                TypeShape::Pratt { variants },
            )),
            expr,
        );
        let token = tokens[0];
        let root = grammar.next_type_id();
        let declaration = test_declaration(&mut grammar, &mut next_byte, "JsonTableLike");
        let shapes = [
            (expr, crate::ContainerShape::Direct),
            (
                token,
                crate::ContainerShape::optional(crate::ContainerShape::Direct),
            ),
            (token, crate::ContainerShape::Direct),
            (expr, crate::ContainerShape::Direct),
            (
                token,
                crate::ContainerShape::optional(crate::ContainerShape::Direct),
            ),
            (
                token,
                crate::ContainerShape::optional(crate::ContainerShape::Direct),
            ),
            (token, crate::ContainerShape::Direct),
            (
                token,
                crate::ContainerShape::optional(crate::ContainerShape::Direct),
            ),
        ];
        let sites = shapes
            .into_iter()
            .enumerate()
            .map(|(index, (target, shape))| {
                let site_declaration = test_declaration(
                    &mut grammar,
                    &mut next_byte,
                    &format!("JsonTableLike::{index}"),
                );
                grammar.push_site(crate::ParseSite::new(root, target, site_declaration, shape))
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "JsonTableLike",
                declaration,
                TypeShape::Sequence { sites },
            )),
            root,
        );
        let atom_kinds = expand_atoms(&grammar);
        let mut oracle = FirstLanguageArena::new(&grammar, &atom_kinds);
        oracle.canonical_sites = (0..grammar.sites.len())
            .map(|index| SiteId::new(index.try_into().expect("test site identity width")))
            .collect::<Vec<_>>()
            .into();
        let oracle_root = oracle.build(
            grammar.max_lookahead,
            vec![DecisionState(vec![DecisionItem::Type(root)])],
        );
        let expected = oracle.materialize(oracle_root);

        FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.set(0);
        FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.set(0);
        FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.set(0);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let actual_root = arena.build(
            grammar.max_lookahead,
            vec![DecisionState(vec![DecisionItem::Type(root)])],
        );
        assert_eq!(
            arena.materialize(actual_root),
            expected,
            "site canonicalization must preserve the exact bounded FIRST language",
        );

        assert!(
            arena.decision_states.len() <= BRANCH_COUNT * 4
                && FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.get() <= BRANCH_COUNT * 3
                && FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.get() <= BRANCH_COUNT * 6
                && FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.get() <= BRANCH_COUNT * 32,
            "a JsonTable-shaped Pratt sequence must remain bounded by observable prefix languages: decisions={} components={} nodes={} states={} frontier_computations={} component_builds={} partition_inputs={}",
            arena.decision_states.len(),
            arena.projected_components.len(),
            arena.nodes.len(),
            arena.states.len(),
            FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.get(),
            FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.get(),
            FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.get(),
        );
    }

    #[test]
    fn cutoff_subsets_reuse_individual_residual_summaries() {
        const RESIDUAL_COUNT: usize = 12;
        FIRST_LANGUAGE_FRONTIER_EXPANSIONS.set(0);
        let (grammar, _) = correlated_pair_grammar(RESIDUAL_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for mask in 1_usize..(1_usize << RESIDUAL_COUNT) {
            let residuals = (0..RESIDUAL_COUNT)
                .filter(|index| mask & (1 << index) != 0)
                .map(|index| {
                    DecisionState(vec![DecisionItem::Type(TypeId::new(
                        index.try_into().expect("test type width"),
                    ))])
                })
                .collect();
            arena.build(0, residuals);
        }

        assert!(
            FIRST_LANGUAGE_FRONTIER_EXPANSIONS.get() <= RESIDUAL_COUNT,
            "each individual cutoff residual must be expanded once; expanded {}",
            FIRST_LANGUAGE_FRONTIER_EXPANSIONS.get(),
        );
        assert!(
            arena.states.len() <= RESIDUAL_COUNT,
            "cutoff subsets must collapse before residual-set memoization; retained {} states",
            arena.states.len(),
        );
    }

    #[test]
    fn cutoff_summary_does_not_retain_successors_beyond_the_bounded_language() {
        FIRST_LANGUAGE_CUTOFF_SUMMARY_COMPUTATIONS.set(0);
        FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.set(0);
        let (grammar, _) = correlated_pair_grammar(2);
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        arena.build(
            0,
            vec![DecisionState(vec![
                DecisionItem::Type(TypeId::new(0)),
                DecisionItem::Type(TypeId::new(1)),
            ])],
        );

        assert_eq!(
            arena.decision_states.len(),
            1,
            "cutoff facts must not intern successor states beyond the configured prefix",
        );
        assert_eq!(
            arena.decision_frontiers.iter().flatten().count(),
            0,
            "a cutoff-only residual must retain only its two-bit summary",
        );

        let residual = FirstLanguageDecisionStateId(0);
        arena.ensure_decision_frontier(residual);
        arena.ensure_decision_frontier(residual);
        arena.cutoff_summary(residual);
        assert_eq!(FIRST_LANGUAGE_CUTOFF_SUMMARY_COMPUTATIONS.get(), 1);
        assert_eq!(FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.get(), 1);
    }

    #[test]
    fn summary_only_frontier_matches_full_derivation_without_materializing_transitions() {
        FIRST_LANGUAGE_DECISION_TRANSITION_MATERIALIZATIONS.set(0);
        FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.set(0);
        let (grammar, items) = cutoff_summary_equivalence_grammar();
        let atom_kinds = expand_atoms(&grammar);
        let mut states = vec![DecisionState(Vec::new())];
        states.extend(items.iter().cloned().map(|item| DecisionState(vec![item])));
        states.extend(items.iter().cloned().flat_map(|left| {
            items
                .iter()
                .cloned()
                .map(move |right| DecisionState(vec![left.clone(), right]))
        }));
        let expected = states
            .iter()
            .map(|state| {
                let full = decision_frontier(state, &grammar, &atom_kinds);
                (u8::from(full.can_end) * FIRST_LANGUAGE_CUTOFF_END)
                    | (u8::from(!full.transitions.is_empty()) * FIRST_LANGUAGE_CUTOFF_CONTINUES)
            })
            .collect::<Vec<_>>();
        let facts = DecisionSummaryFacts::from_grammar(&grammar, &atom_kinds);
        FIRST_LANGUAGE_DECISION_TRANSITION_MATERIALIZATIONS.set(0);
        FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.set(0);

        for (state, expected) in states.iter().zip(expected) {
            assert_eq!(
                decision_cutoff_summary(state, &facts),
                expected,
                "summary mismatch for exhaustive state {state:?}",
            );
        }
        assert_eq!(
            FIRST_LANGUAGE_DECISION_TRANSITION_MATERIALIZATIONS.get(),
            0,
            "summary-only derivation must not construct normalized successor frontiers",
        );
        assert_eq!(
            FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.get(),
            0,
            "summary-only derivation must compose interned item facts rather than revisit full epsilon closures",
        );
    }

    #[test]
    fn cutoff_language_matches_full_projection_without_interning_successors() {
        let (grammar, items) = cutoff_summary_equivalence_grammar();
        let atom_kinds = expand_atoms(&grammar);
        let summary_facts = DecisionSummaryFacts::from_grammar(&grammar, &atom_kinds);
        let mut states = vec![DecisionState(Vec::new())];
        states.extend(items.into_iter().map(|item| DecisionState(vec![item])));
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for state in &states {
            let full = decision_frontier(state, &grammar, &atom_kinds);
            let mut expected = Vec::new();
            if full.can_end {
                expected.push(FirstSequence {
                    positions: Vec::new(),
                    ends: true,
                });
            }
            for transition in full.transitions {
                let summary = decision_cutoff_summary(&transition.next, &summary_facts);
                if summary & FIRST_LANGUAGE_CUTOFF_END != 0 {
                    expected.push(FirstSequence {
                        positions: vec![transition.position.clone()],
                        ends: true,
                    });
                }
                if summary & FIRST_LANGUAGE_CUTOFF_CONTINUES != 0 {
                    expected.push(FirstSequence {
                        positions: vec![transition.position],
                        ends: false,
                    });
                }
            }
            sort_dedup_first_sequences(&mut expected);

            let root = arena.build(1, vec![state.clone()]);
            assert_eq!(
                arena.materialize(root),
                expected,
                "cutoff projection mismatch for {state:?}",
            );
        }
        assert!(
            arena.decision_states.len() <= states.len(),
            "cutoff projection must not intern semantically unobservable successor states; inputs={} retained={}",
            states.len(),
            arena.decision_states.len(),
        );
    }

    #[test]
    fn cutoff_frontier_projection_avoids_full_decision_closure() {
        let (grammar, items) = cutoff_summary_equivalence_grammar();
        let atom_kinds = expand_atoms(&grammar);
        let items = items
            .into_iter()
            .filter(|item| match item {
                DecisionItem::RepeatTail {
                    target,
                    separator,
                    optional_trailing,
                } => grammar.sites.iter().any(|site| {
                    let semantics = container_semantics(site);
                    semantics.cardinality.is_repeated()
                        && site.target == *target
                        && semantics.separator_type() == *separator
                        && semantics
                            .separator
                            .is_some_and(|separator| separator.optional_trailing)
                            == *optional_trailing
                }),
                DecisionItem::Type(_) | DecisionItem::Site(_) => true,
            })
            .collect::<Vec<_>>();
        let mut states = vec![DecisionState(Vec::new())];
        states.extend(items.iter().cloned().map(|item| DecisionState(vec![item])));
        states.extend(items.iter().cloned().flat_map(|left| {
            items
                .iter()
                .cloned()
                .map(move |right| DecisionState(vec![left.clone(), right]))
        }));
        let expected = states
            .iter()
            .map(|state| {
                let full = decision_frontier(state, &grammar, &atom_kinds);
                let summary_facts = DecisionSummaryFacts::from_grammar(&grammar, &atom_kinds);
                let mut sequences = Vec::new();
                if full.can_end {
                    sequences.push(FirstSequence {
                        positions: Vec::new(),
                        ends: true,
                    });
                }
                for transition in full.transitions {
                    let summary = decision_cutoff_summary(&transition.next, &summary_facts);
                    if summary & FIRST_LANGUAGE_CUTOFF_END != 0 {
                        sequences.push(FirstSequence {
                            positions: vec![transition.position.clone()],
                            ends: true,
                        });
                    }
                    if summary & FIRST_LANGUAGE_CUTOFF_CONTINUES != 0 {
                        sequences.push(FirstSequence {
                            positions: vec![transition.position],
                            ends: false,
                        });
                    }
                }
                sort_dedup_first_sequences(&mut sequences);
                sequences
            })
            .collect::<Vec<_>>();
        FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.set(0);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for (state, expected) in states.into_iter().zip(expected) {
            let root = arena.build(1, vec![state]);
            assert_eq!(arena.materialize(root), expected);
        }

        assert_eq!(
            FIRST_LANGUAGE_DECISION_CLOSURE_STATE_VISITS.get(),
            0,
            "cutoff projection must compose fixed-point item facts without walking full decision closures",
        );
    }

    #[test]
    fn root_local_first_arenas_share_grammar_wide_cutoff_facts() {
        FIRST_LANGUAGE_CUTOFF_FACT_CONSTRUCTIONS.set(0);
        let (grammar, _) = cutoff_summary_equivalence_grammar();
        let atom_kinds = expand_atoms(&grammar);
        let analysis = FirstSequenceAnalysis::new(&grammar, &atom_kinds);

        for sites in grammar.sites.chunks(2).take(4) {
            let site_ids = sites
                .iter()
                .map(|site| {
                    SiteId::new(
                        grammar
                            .sites
                            .iter()
                            .position(|candidate| std::ptr::eq(candidate, site))
                            .expect("site belongs to grammar")
                            .try_into()
                            .expect("test site identity width"),
                    )
                })
                .collect::<Vec<_>>();
            analysis.sequences(&site_ids);
        }

        assert_eq!(
            FIRST_LANGUAGE_CUTOFF_FACT_CONSTRUCTIONS.get(),
            1,
            "one analysis run must solve immutable cutoff facts once across root-local arenas",
        );
    }

    #[test]
    fn cutoff_residual_unions_are_bitwise_or_homomorphic() {
        let (grammar, items) = cutoff_summary_equivalence_grammar();
        let atom_kinds = expand_atoms(&grammar);
        let mut states = vec![DecisionState(Vec::new())];
        states.extend(items.iter().cloned().map(|item| DecisionState(vec![item])));
        states.extend(items.iter().cloned().flat_map(|left| {
            items
                .iter()
                .cloned()
                .map(move |right| DecisionState(vec![left.clone(), right]))
        }));
        let expected = states
            .iter()
            .map(|state| {
                let full = decision_frontier(state, &grammar, &atom_kinds);
                (u8::from(full.can_end) * FIRST_LANGUAGE_CUTOFF_END)
                    | (u8::from(!full.transitions.is_empty()) * FIRST_LANGUAGE_CUTOFF_CONTINUES)
            })
            .collect::<Vec<_>>();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let residuals = states
            .into_iter()
            .map(|state| arena.intern_decision_state(state))
            .collect::<Vec<_>>();

        for (left_index, left) in residuals.iter().copied().enumerate() {
            for (right_index, right) in residuals.iter().copied().enumerate() {
                let node = arena.build_cutoff(&[left, right]);
                let node = &arena.nodes[node.index()];
                let actual = (u8::from(node.exact_end) * FIRST_LANGUAGE_CUTOFF_END)
                    | (u8::from(node.cutoff) * FIRST_LANGUAGE_CUTOFF_CONTINUES);
                assert_eq!(
                    actual,
                    expected[left_index] | expected[right_index],
                    "cutoff residual union must preserve existential end/continuation facts",
                );
            }
        }
    }

    #[test]
    fn residual_subsets_reuse_one_interned_frontier_per_decision_state() {
        const RESIDUAL_COUNT: usize = 8;
        FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.set(0);
        let (grammar, _) = correlated_pair_grammar(RESIDUAL_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for mask in 1_usize..(1_usize << RESIDUAL_COUNT) {
            let residuals = (0..RESIDUAL_COUNT)
                .filter(|index| mask & (1 << index) != 0)
                .map(|index| {
                    DecisionState(vec![DecisionItem::Type(TypeId::new(
                        index.try_into().expect("test type width"),
                    ))])
                })
                .collect();
            arena.build(1, residuals);
        }

        assert!(
            FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.get() <= arena.decision_states.len(),
            "each reachable interned decision state must derive its normalized frontier once; computations={} decision_states={}",
            FIRST_LANGUAGE_DECISION_FRONTIER_COMPUTATIONS.get(),
            arena.decision_states.len(),
        );
    }

    #[test]
    fn cutoff_residual_subsets_share_canonical_projected_frontiers() {
        const RESIDUAL_COUNT: usize = 12;
        FIRST_LANGUAGE_CUTOFF_COMPONENT_MEMBERSHIPS.set(0);
        FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS.set(0);
        let mut grammar =
            GrammarDescription::new("shared-cutoff-frontiers", 1, 1, TextEquivalence::Sensitive);
        let mut next_byte = 0;
        let kind_declaration = test_declaration(&mut grammar, &mut next_byte, "Word");
        let kind = grammar.push_kind(crate::KindDescription::new("Word", kind_declaration));
        let atom_declaration = test_declaration(&mut grammar, &mut next_byte, "WordAtom");
        let atom = grammar.push_atom(crate::AtomDescription::new(
            "WordAtom",
            atom_declaration,
            AtomExpression::Kind(kind),
        ));
        let token_declaration = test_declaration(&mut grammar, &mut next_byte, "Token");
        let token = grammar.push_type(crate::TypeDescription::new(
            "Token",
            token_declaration,
            TypeShape::Token {
                atom,
                text_predicate: None,
            },
        ));
        let aliases = (0..RESIDUAL_COUNT)
            .map(|index| {
                let alias = grammar.next_type_id();
                let name = format!("Alias{index}");
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let site_declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("{name}::0"));
                let site = grammar.push_site(crate::ParseSite::new(
                    alias,
                    token,
                    site_declaration,
                    crate::ContainerShape::Direct,
                ));
                assert_eq!(
                    grammar.push_type(crate::TypeDescription::new(
                        name,
                        declaration,
                        TypeShape::Alias { site },
                    )),
                    alias,
                );
                alias
            })
            .collect::<Vec<_>>();
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let expected = vec![FirstSequence {
            positions: vec![FirstPosition {
                kinds: KindSet::singleton(grammar.kinds.len(), kind)
                    .expect("test kind belongs to grammar"),
                text_predicate: None,
            }],
            ends: true,
        }];

        for mask in 1_usize..(1_usize << RESIDUAL_COUNT) {
            let residuals = aliases
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, alias)| DecisionState(vec![DecisionItem::Type(*alias)]))
                .collect::<Vec<_>>();
            let root = arena.build(1, residuals);
            assert_eq!(
                arena.materialize(root),
                expected,
                "shared projected frontier changed the exact bounded language for mask {mask}",
            );
        }

        assert!(
            FIRST_LANGUAGE_CUTOFF_COMPONENT_MEMBERSHIPS.get() <= 1,
            "equivalent residual subsets must scan their shared projected frontier once; component_scans={}",
            FIRST_LANGUAGE_CUTOFF_COMPONENT_MEMBERSHIPS.get(),
        );
        assert!(
            FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS.get() <= 1,
            "equivalent residual subsets must partition their shared transition once; partition_inputs={}",
            FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS.get(),
        );
        assert_eq!(
            arena.cutoff_frontiers.len(),
            1,
            "equivalent projected frontiers must share one canonical component structure",
        );
        assert!(
            arena.states.len() <= 1,
            "remaining-one states must use projected frontier identity rather than retain unobservable residual subsets; retained {} states",
            arena.states.len(),
        );
        assert!(
            arena.nodes.len() <= 2,
            "identical cutoff languages must retain only the consuming node and its terminal suffix; retained {}",
            arena.nodes.len(),
        );
        assert!(
            arena.decision_states.len() <= RESIDUAL_COUNT,
            "cutoff projection must not intern successors for shared residual subsets; retained {}",
            arena.decision_states.len(),
        );
    }

    #[test]
    fn depth_two_residual_subsets_share_canonical_projected_languages() {
        const RESIDUAL_COUNT: usize = 10;
        let mut grammar = GrammarDescription::new(
            "shared-depth-two-frontiers",
            1,
            2,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let kind_declaration = test_declaration(&mut grammar, &mut next_byte, "Word");
        let kind = grammar.push_kind(crate::KindDescription::new("Word", kind_declaration));
        let atom_declaration = test_declaration(&mut grammar, &mut next_byte, "WordAtom");
        let atom = grammar.push_atom(crate::AtomDescription::new(
            "WordAtom",
            atom_declaration,
            AtomExpression::Kind(kind),
        ));
        let token_declaration = test_declaration(&mut grammar, &mut next_byte, "Token");
        let token = grammar.push_type(crate::TypeDescription::new(
            "Token",
            token_declaration,
            TypeShape::Token {
                atom,
                text_predicate: None,
            },
        ));
        let pair = grammar.next_type_id();
        let pair_declaration = test_declaration(&mut grammar, &mut next_byte, "Pair");
        let pair_sites = (0..2)
            .map(|index| {
                let declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("Pair::{index}"));
                grammar.push_site(crate::ParseSite::new(
                    pair,
                    token,
                    declaration,
                    crate::ContainerShape::Direct,
                ))
            })
            .collect();
        assert_eq!(
            grammar.push_type(crate::TypeDescription::new(
                "Pair",
                pair_declaration,
                TypeShape::Sequence { sites: pair_sites },
            )),
            pair,
        );
        let aliases = (0..RESIDUAL_COUNT)
            .map(|index| {
                let alias = grammar.next_type_id();
                let name = format!("Alias{index}");
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let site_declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("{name}::0"));
                let site = grammar.push_site(crate::ParseSite::new(
                    alias,
                    pair,
                    site_declaration,
                    crate::ContainerShape::Direct,
                ));
                assert_eq!(
                    grammar.push_type(crate::TypeDescription::new(
                        name,
                        declaration,
                        TypeShape::Alias { site },
                    )),
                    alias,
                );
                alias
            })
            .collect::<Vec<_>>();
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let position = FirstPosition {
            kinds: KindSet::singleton(grammar.kinds.len(), kind)
                .expect("test kind belongs to grammar"),
            text_predicate: None,
        };
        let expected = vec![FirstSequence {
            positions: vec![position.clone(), position],
            ends: true,
        }];

        for mask in 1_usize..(1_usize << RESIDUAL_COUNT) {
            let residuals = aliases
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, alias)| DecisionState(vec![DecisionItem::Type(*alias)]))
                .collect::<Vec<_>>();
            let root = arena.build(2, residuals);
            assert_eq!(
                arena.materialize(root),
                expected,
                "shared projected depth-two language changed the exact bounded language for mask {mask}",
            );
        }

        assert!(
            arena.states.len() <= 2,
            "depth-two states must use projected language identity rather than retain unobservable residual subsets; retained {} states",
            arena.states.len(),
        );
        assert!(
            arena.nodes.len() <= 3,
            "identical depth-two languages must retain only their two consuming nodes and terminal suffix; retained {}",
            arena.nodes.len(),
        );
    }

    #[test]
    fn deeper_residual_subsets_share_recursive_projected_languages() {
        const RESIDUAL_COUNT: usize = 8;
        FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.set(0);
        let (mut grammar, root) = correlated_pair_grammar(1);
        let mut next_byte = 10_000;
        let aliases = (0..RESIDUAL_COUNT)
            .map(|index| {
                let alias = grammar.next_type_id();
                let name = format!("DepthThreeAlias{index}");
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let site_declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("{name}::0"));
                let site = grammar.push_site(crate::ParseSite::new(
                    alias,
                    root,
                    site_declaration,
                    crate::ContainerShape::Direct,
                ));
                assert_eq!(
                    grammar.push_type(crate::TypeDescription::new(
                        name,
                        declaration,
                        TypeShape::Alias { site },
                    )),
                    alias,
                );
                alias
            })
            .collect::<Vec<_>>();
        let atom_kinds = expand_atoms(&grammar);
        let position = FirstPosition {
            kinds: KindSet::singleton(grammar.kinds.len(), KindId::new(0))
                .expect("test kind belongs to grammar"),
            text_predicate: None,
        };
        let expected = vec![FirstSequence {
            positions: vec![position.clone(), position.clone(), position],
            ends: false,
        }];
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for mask in 1_usize..(1_usize << RESIDUAL_COUNT) {
            let residuals = aliases
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, alias)| DecisionState(vec![DecisionItem::Type(*alias)]))
                .collect::<Vec<_>>();
            let root = arena.build(3, residuals);
            assert_eq!(
                arena.materialize(root),
                expected,
                "shared recursive projection changed the exact bounded language for mask {mask}",
            );
        }

        assert!(
            arena.states.len() <= 3,
            "recursive projected states must retain language identities rather than raw residual subsets; retained {} states",
            arena.states.len(),
        );
        assert!(
            FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.get() <= RESIDUAL_COUNT * 4,
            "the projected tail must reuse successor components instead of eagerly walking every subset; component_builds={}",
            FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.get(),
        );
    }

    #[test]
    fn projected_language_identity_reaches_the_parent_of_the_factored_tail() {
        const RESIDUAL_COUNT: usize = 8;
        FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.set(0);
        let (mut grammar, root) = correlated_pair_grammar(1);
        let mut next_byte = 20_000;
        let aliases = (0..RESIDUAL_COUNT)
            .map(|index| {
                let alias = grammar.next_type_id();
                let name = format!("DepthFourAlias{index}");
                let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
                let site_declaration =
                    test_declaration(&mut grammar, &mut next_byte, &format!("{name}::0"));
                let site = grammar.push_site(crate::ParseSite::new(
                    alias,
                    root,
                    site_declaration,
                    crate::ContainerShape::Direct,
                ));
                assert_eq!(
                    grammar.push_type(crate::TypeDescription::new(
                        name,
                        declaration,
                        TypeShape::Alias { site },
                    )),
                    alias,
                );
                alias
            })
            .collect::<Vec<_>>();
        let atom_kinds = expand_atoms(&grammar);
        let position = FirstPosition {
            kinds: KindSet::singleton(grammar.kinds.len(), KindId::new(0))
                .expect("test kind belongs to grammar"),
            text_predicate: None,
        };
        let expected = vec![FirstSequence {
            positions: vec![
                position.clone(),
                position.clone(),
                position.clone(),
                position,
            ],
            ends: false,
        }];
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for mask in 1_usize..(1_usize << RESIDUAL_COUNT) {
            let residuals = aliases
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1 << index) != 0)
                .map(|(_, alias)| DecisionState(vec![DecisionItem::Type(*alias)]))
                .collect::<Vec<_>>();
            let root = arena.build(4, residuals);
            assert_eq!(
                arena.materialize(root),
                expected,
                "parent projection changed the exact bounded language for mask {mask}",
            );
        }

        assert!(
            arena.states.len() <= 4,
            "the state above the specialized tail must use projected language identities; retained {} states",
            arena.states.len(),
        );
        assert!(
            FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.get() <= RESIDUAL_COUNT * 5,
            "the parent projection must reuse canonical successor components; component_builds={}",
            FIRST_LANGUAGE_PROJECTED_COMPONENT_BUILDS.get(),
        );
    }

    #[test]
    fn cutoff_frontier_index_retains_one_component_copy_per_unique_frontier() {
        const FRONTIER_COUNT: usize = 64;
        let mut grammar = GrammarDescription::new(
            "cutoff-frontier-index",
            FRONTIER_COUNT,
            1,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        for index in 0..FRONTIER_COUNT {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            grammar.push_kind(crate::KindDescription::new(name, declaration));
        }
        let atom_kinds = Vec::new();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for index in 0..FRONTIER_COUNT {
            let position = arena.intern_position(FirstPosition {
                kinds: KindSet::singleton(
                    FRONTIER_COUNT,
                    KindId::new(index.try_into().expect("test kind identity width")),
                )
                .expect("test kind belongs to grammar"),
                text_predicate: None,
            });
            let frontier = FirstLanguageCutoffFrontier {
                can_end: index % 2 == 0,
                transitions: vec![FirstLanguageCutoffTransition {
                    position,
                    summary: if index % 3 == 0 {
                        FIRST_LANGUAGE_CUTOFF_END
                    } else {
                        FIRST_LANGUAGE_CUTOFF_CONTINUES
                    },
                }],
            };
            let first = arena.intern_cutoff_frontier(frontier.clone());
            let second = arena.intern_cutoff_frontier(frontier);
            assert_eq!(first, second, "equivalent frontiers must share identity");
        }

        assert_eq!(arena.cutoff_frontiers.len(), FRONTIER_COUNT);
        assert!(
            arena.retained_cutoff_frontier_component_copies() <= FRONTIER_COUNT,
            "the canonical index must reference arena-owned components rather than clone them; retained {} component copies for {FRONTIER_COUNT} unique frontiers",
            arena.retained_cutoff_frontier_component_copies(),
        );
    }

    #[test]
    fn cached_frontiers_intern_repeated_positions_across_compact_arcs() {
        const RESIDUAL_COUNT: usize = 64;
        let (grammar, _) = correlated_pair_grammar(RESIDUAL_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        for tail in 0..RESIDUAL_COUNT {
            let residual = arena.intern_decision_state(DecisionState(vec![
                DecisionItem::Type(TypeId::new(0)),
                DecisionItem::Type(TypeId::new(
                    tail.try_into().expect("test type identity width"),
                )),
            ]));
            arena.ensure_decision_frontier(residual);
        }

        assert_eq!(arena.cached_frontier_position_count(), 1);
        assert_eq!(arena.cached_frontier_arc_count(), RESIDUAL_COUNT);
        assert!(
            arena.cached_frontier_kind_words() <= grammar.kind_count.div_ceil(64),
            "cached arcs must retain one interned KindSet rather than one copy per arc; stored {} words",
            arena.cached_frontier_kind_words(),
        );
    }

    #[test]
    fn recursive_overlapping_cells_reuse_interned_decision_states() {
        const BRANCH_COUNT: usize = 24;
        FIRST_LANGUAGE_DECISION_STATE_CLONES.set(0);
        FIRST_LANGUAGE_DECISION_STATE_COMPARISONS.set(0);
        let (grammar, root) = overlapping_text_pair_grammar(BRANCH_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        arena.build(
            grammar.max_lookahead,
            vec![DecisionState(vec![DecisionItem::Type(root)])],
        );
        let cells = arena
            .nodes
            .iter()
            .map(|node| node.cells.len())
            .sum::<usize>();
        let residual_memberships = arena
            .states
            .keys()
            .map(|state| state.residuals.len())
            .sum::<usize>();

        assert!(
            FIRST_LANGUAGE_DECISION_STATE_CLONES.get() <= arena.decision_states.len() * 10,
            "recursive cells must carry interned residual identities; clones={} comparisons={} decision_states={} states={} nodes={}",
            FIRST_LANGUAGE_DECISION_STATE_CLONES.get(),
            FIRST_LANGUAGE_DECISION_STATE_COMPARISONS.get(),
            arena.decision_states.len(),
            arena.states.len(),
            arena.nodes.len(),
        );
        assert!(
            FIRST_LANGUAGE_DECISION_STATE_COMPARISONS.get() <= BRANCH_COUNT * 320,
            "recursive cells must compare compact residual identities; comparisons={} decision_states={} states={} nodes={} cells={} arcs={} residual_memberships={}",
            FIRST_LANGUAGE_DECISION_STATE_COMPARISONS.get(),
            arena.decision_states.len(),
            arena.states.len(),
            arena.nodes.len(),
            cells,
            arena.cached_frontier_arc_count(),
            residual_memberships,
        );
        assert!(
            arena.decision_states.len() <= BRANCH_COUNT * 2 + 3,
            "final-position projection must not retain successor decision states; retained {}",
            arena.decision_states.len(),
        );
        assert!(
            arena.cached_frontier_arc_count() <= BRANCH_COUNT * 5,
            "factored final-position frontiers must retain linear compact arcs; retained {}",
            arena.cached_frontier_arc_count(),
        );
        assert!(
            cells <= BRANCH_COUNT * 6 + 1,
            "overlapping final-position partitions plus the canonical union root must retain linear cells; retained {cells}",
        );
        assert!(
            residual_memberships <= BRANCH_COUNT * 5,
            "factored suffix states must retain linearly bounded residual memberships; retained {residual_memberships}",
        );
        assert!(
            arena.nodes.len() <= BRANCH_COUNT * BRANCH_COUNT,
            "overlapping cells must stay proportional to their canonical structural nodes",
        );
    }

    #[test]
    fn dense_cutoff_cells_fold_two_bit_summaries_without_residual_rescans() {
        const BRANCH_COUNT: usize = 48;
        FIRST_LANGUAGE_CUTOFF_RESIDUAL_VISITS.set(0);
        FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.set(0);
        let (grammar, root) = overlapping_text_pair_grammar(BRANCH_COUNT);
        let atom_kinds = expand_atoms(&grammar);
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);

        arena.build(
            grammar.max_lookahead,
            vec![DecisionState(vec![DecisionItem::Type(root)])],
        );

        let outcomes = arena
            .nodes
            .iter()
            .filter(|node| node.cells.is_empty())
            .map(|node| (node.exact_end, node.cutoff))
            .collect::<BTreeSet<_>>();
        assert!(
            FIRST_LANGUAGE_CUTOFF_RESIDUAL_VISITS.get()
                <= FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.get(),
            "cutoff cells must fold summaries while partitioning; residual_visits={} candidate_reads={} outcomes={}",
            FIRST_LANGUAGE_CUTOFF_RESIDUAL_VISITS.get(),
            FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.get(),
            outcomes.len(),
        );
        assert!(
            outcomes.len() <= 4,
            "two boolean cutoff facts admit only four structural outcomes",
        );
    }

    #[test]
    fn wide_kind_cutoff_transition_reuses_summary_before_partitioning() {
        const KIND_COUNT: usize = 256;
        FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.set(0);
        FIRST_LANGUAGE_FRONTIER_EXPANSIONS.set(0);
        let mut grammar = GrammarDescription::new(
            "wide-cutoff-transition",
            KIND_COUNT,
            1,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        for index in 0..KIND_COUNT {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            grammar.push_kind(crate::KindDescription::new(name, declaration));
        }
        let atom_kinds = Vec::new();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let next = arena.intern_decision_state(DecisionState(Vec::new()));
        let position = arena.intern_position(FirstPosition {
            kinds: KindSet::from_kinds(
                KIND_COUNT,
                (0..KIND_COUNT)
                    .map(|kind| KindId::new(kind.try_into().expect("test kind identity width"))),
            )
            .expect("test kinds belong to grammar"),
            text_predicate: None,
        });
        let transitions =
            arena.project_full_cutoff_transitions(&[FirstLanguageTransition { position, next }]);

        let cells = (0..16)
            .map(|_| arena.build_cutoff_cells(&transitions))
            .last()
            .expect("test performs cutoff builds");

        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].kinds.iter().count(), KIND_COUNT);
        assert!(
            FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.get() <= transitions.len(),
            "each interned successor summary must be carried across repeated cutoff builds; read {} summaries for {} transitions across {KIND_COUNT} kinds",
            FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.get(),
            transitions.len(),
        );
        assert!(
            FIRST_LANGUAGE_FRONTIER_EXPANSIONS.get() <= arena.decision_states.len(),
            "cutoff-summary computation must be bounded by unique interned decision states",
        );
    }

    #[test]
    fn cutoff_partition_factors_wide_kind_sets_before_text_cells() {
        const KIND_COUNT: usize = 128;
        FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS.set(0);
        let mut grammar = GrammarDescription::new(
            "wide-cutoff-partition",
            KIND_COUNT,
            1,
            TextEquivalence::AsciiInsensitive,
        );
        let mut next_byte = 0;
        for index in 0..KIND_COUNT {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            grammar.push_kind(crate::KindDescription::new(name, declaration));
        }
        let exact_a = grammar.push_text_predicate(TextPredicate::Exact("A".to_owned()));
        let one_of_ab =
            grammar.push_text_predicate(TextPredicate::OneOf(vec!["a".to_owned(), "B".to_owned()]));
        let not_b = grammar.push_text_predicate(TextPredicate::NotIn(vec!["b".to_owned()]));
        let atom_kinds = Vec::new();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let specifications = [
            ((0..96).collect::<Vec<_>>(), None, FIRST_LANGUAGE_CUTOFF_END),
            (
                (32..KIND_COUNT).collect::<Vec<_>>(),
                Some(exact_a),
                FIRST_LANGUAGE_CUTOFF_CONTINUES,
            ),
            (
                (0..KIND_COUNT).step_by(2).collect::<Vec<_>>(),
                Some(one_of_ab),
                FIRST_LANGUAGE_CUTOFF_END,
            ),
            (
                (1..KIND_COUNT).step_by(2).collect::<Vec<_>>(),
                Some(not_b),
                FIRST_LANGUAGE_CUTOFF_CONTINUES,
            ),
        ];
        let transitions = specifications
            .iter()
            .map(|(kinds, text_predicate, summary)| {
                let position = arena.intern_position(FirstPosition {
                    kinds: KindSet::from_kinds(
                        KIND_COUNT,
                        kinds.iter().map(|kind| {
                            KindId::new((*kind).try_into().expect("test kind identity width"))
                        }),
                    )
                    .expect("test kinds belong to grammar"),
                    text_predicate: *text_predicate,
                });
                FirstLanguageCutoffTransition {
                    position,
                    summary: *summary,
                }
            })
            .collect::<Vec<_>>();

        let cells = arena.build_cutoff_cells(&transitions);

        for kind_index in 0..KIND_COUNT {
            let kind = KindId::new(kind_index.try_into().expect("test kind identity width"));
            for text in ["A", "b", "other"] {
                let canonical = text.to_ascii_lowercase();
                let expected = transitions.iter().fold(0_u8, |summary, transition| {
                    let position = &arena.positions[transition.position.index()];
                    let matches = position.kinds.contains(kind)
                        && position.text_predicate.is_none_or(|predicate| {
                            match &grammar.text_predicates[predicate.index()] {
                                TextPredicate::Any => true,
                                TextPredicate::Exact(value) => {
                                    value.to_ascii_lowercase() == canonical
                                }
                                TextPredicate::OneOf(values) => values
                                    .iter()
                                    .any(|value| value.to_ascii_lowercase() == canonical),
                                TextPredicate::NotIn(values) => values
                                    .iter()
                                    .all(|value| value.to_ascii_lowercase() != canonical),
                            }
                        });
                    summary | if matches { transition.summary } else { 0 }
                });
                let matching = cells
                    .iter()
                    .filter(|cell| {
                        cell.kinds.contains(kind)
                            && match &cell.text {
                                TextPartition::Any => true,
                                TextPartition::Exact(value) => value == &canonical,
                                TextPartition::OneOf(values) => values.contains(&canonical),
                                TextPartition::NotIn(values) => !values.contains(&canonical),
                            }
                    })
                    .collect::<Vec<_>>();
                assert!(
                    matching.len() <= 1,
                    "cutoff cells must remain pairwise disjoint"
                );
                let actual = matching.first().map_or(0, |cell| {
                    let node = &arena.nodes[cell.next.index()];
                    (u8::from(node.exact_end) * FIRST_LANGUAGE_CUTOFF_END)
                        | (u8::from(node.cutoff) * FIRST_LANGUAGE_CUTOFF_CONTINUES)
                });
                assert_eq!(actual, expected, "kind={kind_index} text={text}");
            }
        }
        assert!(
            FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS.get() <= transitions.len(),
            "wide kind sets must remain symbolic before text partitioning; expanded {} transition-kind candidates from {} factored transitions",
            FIRST_LANGUAGE_CUTOFF_PARTITION_INPUTS.get(),
            transitions.len(),
        );
    }

    #[test]
    fn projected_partition_factors_wide_kind_sets_before_text_cells() {
        const KIND_COUNT: usize = 128;
        FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.set(0);
        let mut grammar = GrammarDescription::new(
            "wide-projected-partition",
            KIND_COUNT,
            2,
            TextEquivalence::AsciiInsensitive,
        );
        let mut next_byte = 0;
        for index in 0..KIND_COUNT {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            grammar.push_kind(crate::KindDescription::new(name, declaration));
        }
        let exact_a = grammar.push_text_predicate(TextPredicate::Exact("A".to_owned()));
        let one_of_ab =
            grammar.push_text_predicate(TextPredicate::OneOf(vec!["a".to_owned(), "B".to_owned()]));
        let not_b = grammar.push_text_predicate(TextPredicate::NotIn(vec!["b".to_owned()]));
        let atom_kinds = Vec::new();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let summaries = [
            FIRST_LANGUAGE_CUTOFF_END,
            FIRST_LANGUAGE_CUTOFF_CONTINUES,
            FIRST_LANGUAGE_CUTOFF_END | FIRST_LANGUAGE_CUTOFF_CONTINUES,
            FIRST_LANGUAGE_CUTOFF_CONTINUES,
        ];
        let components = summaries.map(|summary| {
            arena.intern_projected_component(FirstLanguageProjectedComponent {
                exact_end: summary & FIRST_LANGUAGE_CUTOFF_END != 0,
                cutoff: summary & FIRST_LANGUAGE_CUTOFF_CONTINUES != 0,
                transitions: Vec::new(),
            })
        });
        let specifications = [
            ((0..96).collect::<Vec<_>>(), None, components[0]),
            (
                (32..KIND_COUNT).collect::<Vec<_>>(),
                Some(exact_a),
                components[1],
            ),
            (
                (0..KIND_COUNT).step_by(2).collect::<Vec<_>>(),
                Some(one_of_ab),
                components[2],
            ),
            (
                (1..KIND_COUNT).step_by(2).collect::<Vec<_>>(),
                Some(not_b),
                components[3],
            ),
        ];
        let transitions = specifications
            .iter()
            .map(|(kinds, text_predicate, next)| {
                let position = arena.intern_position(FirstPosition {
                    kinds: KindSet::from_kinds(
                        KIND_COUNT,
                        kinds.iter().map(|kind| {
                            KindId::new((*kind).try_into().expect("test kind identity width"))
                        }),
                    )
                    .expect("test kinds belong to grammar"),
                    text_predicate: *text_predicate,
                });
                FirstLanguageProjectedTransition {
                    position,
                    next: *next,
                }
            })
            .collect::<Vec<_>>();

        let cells = arena.build_projected_cells(&transitions, 0);

        for kind_index in 0..KIND_COUNT {
            let kind = KindId::new(kind_index.try_into().expect("test kind identity width"));
            for text in ["A", "b", "other"] {
                let canonical = text.to_ascii_lowercase();
                let expected = transitions
                    .iter()
                    .filter(|transition| {
                        let position = &arena.positions[transition.position.index()];
                        position.kinds.contains(kind)
                            && position.text_predicate.is_none_or(|predicate| {
                                match &grammar.text_predicates[predicate.index()] {
                                    TextPredicate::Any => true,
                                    TextPredicate::Exact(value) => {
                                        value.to_ascii_lowercase() == canonical
                                    }
                                    TextPredicate::OneOf(values) => values
                                        .iter()
                                        .any(|value| value.to_ascii_lowercase() == canonical),
                                    TextPredicate::NotIn(values) => values
                                        .iter()
                                        .all(|value| value.to_ascii_lowercase() != canonical),
                                }
                            })
                    })
                    .fold(0_u8, |summary, transition| {
                        let component = &arena.projected_components[transition.next.index()];
                        summary
                            | (u8::from(component.exact_end) * FIRST_LANGUAGE_CUTOFF_END)
                            | (u8::from(component.cutoff) * FIRST_LANGUAGE_CUTOFF_CONTINUES)
                    });
                let matching = cells
                    .iter()
                    .filter(|cell| {
                        cell.kinds.contains(kind)
                            && match &cell.text {
                                TextPartition::Any => true,
                                TextPartition::Exact(value) => value == &canonical,
                                TextPartition::OneOf(values) => values.contains(&canonical),
                                TextPartition::NotIn(values) => !values.contains(&canonical),
                            }
                    })
                    .collect::<Vec<_>>();
                assert!(matching.len() <= 1, "projected cells must remain disjoint");
                let actual = matching.first().map_or(0, |cell| {
                    let node = &arena.nodes[cell.next.index()];
                    (u8::from(node.exact_end) * FIRST_LANGUAGE_CUTOFF_END)
                        | (u8::from(node.cutoff) * FIRST_LANGUAGE_CUTOFF_CONTINUES)
                });
                assert_eq!(actual, expected, "kind={kind_index} text={text}");
            }
        }
        assert!(
            FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.get() <= transitions.len(),
            "projected wide kind sets must remain symbolic; expanded {} transition-kind candidates from {} transitions",
            FIRST_LANGUAGE_PROJECTED_PARTITION_INPUTS.get(),
            transitions.len(),
        );
    }

    #[test]
    fn nary_projected_states_build_one_canonical_subset_without_node_unions() {
        const COMPONENT_COUNT: usize = 64;
        let mut grammar = GrammarDescription::new(
            "nary-projected-subset",
            COMPONENT_COUNT + 1,
            2,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        for index in 0..=COMPONENT_COUNT {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            grammar.push_kind(crate::KindDescription::new(name, declaration));
        }
        let atom_kinds = Vec::new();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let terminal = arena.intern_projected_component(FirstLanguageProjectedComponent {
            exact_end: true,
            cutoff: false,
            transitions: Vec::new(),
        });
        let shared = arena.intern_position(FirstPosition {
            kinds: KindSet::singleton(grammar.kind_count, KindId::new(0))
                .expect("shared kind belongs to grammar"),
            text_predicate: None,
        });
        let mut components = Vec::new();
        let mut expected = Vec::new();
        for index in 0..COMPONENT_COUNT {
            let suffix_position = FirstPosition {
                kinds: KindSet::singleton(
                    grammar.kind_count,
                    KindId::new((index + 1).try_into().expect("test kind identity width")),
                )
                .expect("suffix kind belongs to grammar"),
                text_predicate: None,
            };
            let suffix_position_id = arena.intern_position(suffix_position.clone());
            let suffix = arena.intern_projected_component(FirstLanguageProjectedComponent {
                exact_end: false,
                cutoff: false,
                transitions: vec![FirstLanguageProjectedTransition {
                    position: suffix_position_id,
                    next: terminal,
                }],
            });
            components.push(
                arena.intern_projected_component(FirstLanguageProjectedComponent {
                    exact_end: false,
                    cutoff: false,
                    transitions: vec![FirstLanguageProjectedTransition {
                        position: shared,
                        next: suffix,
                    }],
                }),
            );
            expected.push(FirstSequence {
                positions: vec![arena.positions[shared.index()].clone(), suffix_position],
                ends: true,
            });
        }
        sort_dedup_first_sequences(&mut expected);

        FIRST_LANGUAGE_NODE_UNION_MISSES.set(0);
        let root = arena.build_projected_state(2, components.clone());
        assert_eq!(
            arena.materialize(root),
            expected,
            "direct subset construction must preserve correlated suffixes",
        );
        assert_eq!(
            FIRST_LANGUAGE_NODE_UNION_MISSES.get(),
            0,
            "n-ary projected sets must not retain intermediate deterministic unions",
        );

        components.reverse();
        components.push(components[0]);
        assert_eq!(
            arena.build_projected_state(2, components),
            root,
            "component order and duplicates must share one canonical projected subset",
        );
    }

    #[test]
    fn deterministic_node_union_reuses_nested_mixed_languages() {
        const KIND_COUNT: usize = 128;
        const LANGUAGE_COUNT: usize = 12;
        let mut grammar = GrammarDescription::new(
            "nested-node-unions",
            KIND_COUNT,
            2,
            TextEquivalence::AsciiInsensitive,
        );
        let mut next_byte = 0;
        for index in 0..KIND_COUNT {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            grammar.push_kind(crate::KindDescription::new(name, declaration));
        }
        let predicates = (0..LANGUAGE_COUNT)
            .map(|index| {
                grammar.push_text_predicate(match index % 4 {
                    0 => TextPredicate::Any,
                    1 => TextPredicate::Exact(format!("word{index}")),
                    2 => {
                        TextPredicate::OneOf(vec![format!("word{index}"), format!("alias{index}")])
                    }
                    _ => TextPredicate::NotIn(vec![format!("excluded{index}")]),
                })
            })
            .collect::<Vec<_>>();
        let atom_kinds = Vec::new();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let terminal = arena.intern_node(FirstLanguageNode {
            exact_end: true,
            cutoff: false,
            cells: Vec::new(),
            covers: Vec::new(),
        });
        let mut atoms = Vec::new();
        for (index, predicate) in predicates.iter().copied().enumerate() {
            let kinds = KindSet::from_kinds(
                KIND_COUNT,
                (index * 7..KIND_COUNT)
                    .map(|kind| KindId::new(kind.try_into().expect("test kind identity width"))),
            )
            .expect("test kinds belong to grammar");
            atoms.push(arena.intern_node(FirstLanguageNode {
                exact_end: false,
                cutoff: false,
                cells: vec![FirstLanguageCell {
                    kinds: kinds.clone(),
                    text: TextPartition::from_predicate(Some(
                        &grammar.text_predicates[predicate.index()],
                    )),
                    next: terminal,
                }],
                covers: vec![FirstLanguageCover {
                    position: FirstPosition {
                        kinds,
                        text_predicate: Some(predicate),
                    },
                    next: terminal,
                }],
            }));
        }
        let mut nested = Vec::new();
        let mut accumulated = atoms[0];
        nested.push(accumulated);
        for atom in atoms.iter().copied().skip(1) {
            accumulated = arena.union_nodes(accumulated, atom);
            nested.push(accumulated);
        }
        let expected_materialized = nested
            .iter()
            .copied()
            .map(|node| arena.materialize(node))
            .collect::<Vec<_>>();
        FIRST_LANGUAGE_NODE_UNION_MISSES.set(0);

        for mask in 1_usize..(1_usize << LANGUAGE_COUNT) {
            let selected = (0..LANGUAGE_COUNT)
                .filter(|index| mask & (1 << index) != 0)
                .collect::<Vec<_>>();
            let expected_index = *selected.last().expect("mask is nonempty");
            let mut forward = nested[selected[0]];
            for index in selected.iter().copied().skip(1) {
                forward = arena.union_nodes(forward, nested[index]);
            }
            let mut reverse = nested[expected_index];
            for index in selected.iter().copied().rev().skip(1) {
                reverse = arena.union_nodes(reverse, nested[index]);
            }
            assert_eq!(forward, nested[expected_index], "forward mask={mask}");
            assert_eq!(reverse, nested[expected_index], "reverse mask={mask}");
            assert_eq!(
                arena.materialize(forward),
                expected_materialized[expected_index],
                "exact materialized union changed for mask={mask}",
            );
            assert!(first_language_cells_are_disjoint(
                &arena.nodes[forward.index()].cells,
                &grammar,
            ));
        }
        assert!(
            FIRST_LANGUAGE_NODE_UNION_MISSES.get() <= 78,
            "nested unions must compute at most one unordered node overlay per language pair; misses={}",
            FIRST_LANGUAGE_NODE_UNION_MISSES.get(),
        );
    }

    #[test]
    fn deterministic_node_union_preserves_correlated_suffixes() {
        const COMPONENT_COUNT: usize = 8;
        let mut grammar = GrammarDescription::new(
            "correlated-node-unions",
            COMPONENT_COUNT + 1,
            2,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        for index in 0..=COMPONENT_COUNT {
            let name = format!("K{index}");
            let declaration = test_declaration(&mut grammar, &mut next_byte, &name);
            grammar.push_kind(crate::KindDescription::new(name, declaration));
        }
        let atom_kinds = Vec::new();
        let mut arena = FirstLanguageArena::new(&grammar, &atom_kinds);
        let terminal = arena.intern_node(FirstLanguageNode {
            exact_end: true,
            cutoff: false,
            cells: Vec::new(),
            covers: Vec::new(),
        });
        let shared = KindSet::singleton(grammar.kinds.len(), KindId::new(0))
            .expect("shared kind belongs to grammar");
        let mut components = Vec::new();
        for index in 0..COMPONENT_COUNT {
            let suffix_kinds = KindSet::singleton(
                grammar.kinds.len(),
                KindId::new((index + 1).try_into().expect("test kind identity width")),
            )
            .expect("suffix kind belongs to grammar");
            let suffix = arena.intern_node(FirstLanguageNode {
                exact_end: false,
                cutoff: false,
                cells: vec![FirstLanguageCell {
                    kinds: suffix_kinds.clone(),
                    text: TextPartition::Any,
                    next: terminal,
                }],
                covers: vec![FirstLanguageCover {
                    position: FirstPosition {
                        kinds: suffix_kinds,
                        text_predicate: None,
                    },
                    next: terminal,
                }],
            });
            components.push(arena.intern_node(FirstLanguageNode {
                exact_end: false,
                cutoff: false,
                cells: vec![FirstLanguageCell {
                    kinds: shared.clone(),
                    text: TextPartition::Any,
                    next: suffix,
                }],
                covers: vec![FirstLanguageCover {
                    position: FirstPosition {
                        kinds: shared.clone(),
                        text_predicate: None,
                    },
                    next: suffix,
                }],
            }));
        }
        let component_languages = components
            .iter()
            .copied()
            .map(|node| arena.materialize(node))
            .collect::<Vec<_>>();

        for mask in 1_usize..(1_usize << COMPONENT_COUNT) {
            let selected = (0..COMPONENT_COUNT)
                .filter(|index| mask & (1 << index) != 0)
                .collect::<Vec<_>>();
            let mut expected = selected
                .iter()
                .flat_map(|index| component_languages[*index].clone())
                .collect::<Vec<_>>();
            sort_dedup_first_sequences(&mut expected);
            let mut forward = components[selected[0]];
            for index in selected.iter().copied().skip(1) {
                forward = arena.union_nodes(forward, components[index]);
            }
            let mut reverse = components[*selected.last().expect("mask is nonempty")];
            for index in selected.iter().copied().rev().skip(1) {
                reverse = arena.union_nodes(reverse, components[index]);
            }
            assert_eq!(arena.materialize(forward), expected, "forward mask={mask}");
            assert_eq!(arena.materialize(reverse), expected, "reverse mask={mask}");
            assert!(first_language_cells_are_disjoint(
                &arena.nodes[forward.index()].cells,
                &grammar,
            ));
        }
    }

    #[test]
    fn rectangle_grouping_bounds_interned_wide_position_states() {
        const KIND_COUNT: usize = 512;
        RECTANGLE_POSITION_STATES.set(0);
        let mut sequences = (0..4_096)
            .map(|index| {
                let kinds = [
                    index & 15,
                    16 + ((index >> 4) & 15),
                    32 + ((index >> 8) & 15),
                    48,
                    49,
                ];
                FirstSequence {
                    positions: kinds
                        .into_iter()
                        .map(|kind| FirstPosition {
                            kinds: KindSet::singleton(KIND_COUNT, KindId::new(kind)).unwrap(),
                            text_predicate: None,
                        })
                        .collect(),
                    ends: false,
                }
            })
            .collect::<Vec<_>>();

        sort_dedup_first_sequences(&mut sequences);

        assert_eq!(sequences.len(), 1, "the 16×16×16 rectangle is exact");
        assert!(
            RECTANGLE_POSITION_STATES.get() <= 512,
            "grouping must remain bounded by distinct wide positions, not row-key copies",
        );
    }

    #[test]
    fn nullable_epsilon_expansion_cannot_pump_through_a_protected_continuation() {
        let empty = TypeId::new(0);
        let left = TypeId::new(1);
        let right = TypeId::new(2);
        let left_tail = TypeId::new(3);
        let right_tail = TypeId::new(4);
        let ancestor = PairedDecisionState::Residual(
            DecisionState(vec![DecisionItem::Type(empty), DecisionItem::Type(left)]),
            DecisionState(vec![DecisionItem::Type(empty), DecisionItem::Type(right)]),
        );
        let successor = PairedDecisionSuccessor {
            state: PairedDecisionState::Residual(
                DecisionState(vec![
                    DecisionItem::Type(empty),
                    DecisionItem::Type(left_tail),
                    DecisionItem::Type(left),
                ]),
                DecisionState(vec![
                    DecisionItem::Type(empty),
                    DecisionItem::Type(right_tail),
                    DecisionItem::Type(right),
                ]),
            ),
            witness: DecisionWitness::Token(crate::LookaheadWitnessPosition {
                kind: KindId::new(0),
                left_text_predicate: None,
                right_text_predicate: None,
            }),
            // The epsilon closure popped E and inspected Left/Right before
            // recreating E. Depth one is therefore the protected boundary.
            left_minimum_stack_depth: 1,
            right_minimum_stack_depth: 1,
        };
        let parents = DecisionProductParents::new(ancestor);

        assert_eq!(pumping_ancestor(0, &successor, &parents), None);
    }

    #[test]
    fn paired_cycle_checks_share_one_ancestor_traversal() {
        const ANCESTORS: usize = 128;
        let states = (0..ANCESTORS)
            .map(|index| {
                let item = DecisionItem::Type(TypeId::new(index as u32));
                PairedDecisionState::Residual(
                    DecisionState(vec![item.clone()]),
                    DecisionState(vec![item]),
                )
            })
            .collect::<Vec<_>>();
        let mut parents = DecisionProductParents::new(states[0].clone());
        for index in 1..ANCESTORS {
            let parent = parents.intern(states[index - 1].clone());
            let child = parents.intern(states[index].clone());
            parents.incoming[child].push(DecisionParent {
                state: parent,
                left_minimum_stack_depth: 0,
                right_minimum_stack_depth: 0,
            });
        }
        let successor_item = DecisionItem::Type(TypeId::new(ANCESTORS as u32));
        let successor = PairedDecisionSuccessor {
            state: PairedDecisionState::Residual(
                DecisionState(vec![successor_item.clone()]),
                DecisionState(vec![successor_item]),
            ),
            witness: DecisionWitness::End,
            left_minimum_stack_depth: 0,
            right_minimum_stack_depth: 0,
        };

        DECISION_PRODUCT_ANCESTOR_STATE_VISITS.set(0);
        let mut ancestor_scratch = DecisionVisitScratch::default();
        let mut path_scratch = DecisionVisitScratch::default();
        assert!(!decision_successor_closes_cycle(
            ANCESTORS - 1,
            &successor,
            &parents,
            &mut ancestor_scratch,
            &mut path_scratch,
        ));
        assert!(
            DECISION_PRODUCT_ANCESTOR_STATE_VISITS.get() <= ANCESTORS + 1,
            "pumping and exact-cycle checks must share one parent traversal; visits={}",
            DECISION_PRODUCT_ANCESTOR_STATE_VISITS.get(),
        );
    }

    #[test]
    fn decision_product_preserves_continuations_inspected_by_nullable_expansion() {
        fn push_token(grammar: &mut GrammarDescription, next_byte: &mut u32, name: &str) -> TypeId {
            let declaration = test_declaration(grammar, next_byte, name);
            let kind = grammar.push_kind(crate::KindDescription::new(name, declaration));
            let atom = grammar.push_atom(crate::AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            grammar.push_type(crate::TypeDescription::new(
                name,
                declaration,
                TypeShape::Token {
                    atom,
                    text_predicate: None,
                },
            ))
        }

        fn push_recursive_sequence(
            grammar: &mut GrammarDescription,
            next_byte: &mut u32,
            name: &str,
            shared: TypeId,
            empty: TypeId,
            tail: TypeId,
        ) -> TypeId {
            let recursive = grammar.next_type_id();
            let declaration = test_declaration(grammar, next_byte, name);
            let sites = [shared, empty, tail, recursive]
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let site_declaration =
                        test_declaration(grammar, next_byte, &format!("{name}::{index}"));
                    grammar.push_site(crate::ParseSite::new(
                        recursive,
                        target,
                        site_declaration,
                        crate::ContainerShape::Direct,
                    ))
                })
                .collect();
            assert_eq!(
                grammar.push_type(crate::TypeDescription::new(
                    name,
                    declaration,
                    TypeShape::Sequence { sites },
                )),
                recursive,
            );
            recursive
        }

        let mut grammar = GrammarDescription::new(
            "protected-continuation-product",
            3,
            3,
            TextEquivalence::Sensitive,
        );
        let mut next_byte = 0;
        let shared = push_token(&mut grammar, &mut next_byte, "SHARED");
        let left_tail = push_token(&mut grammar, &mut next_byte, "LEFT_TAIL");
        let right_tail = push_token(&mut grammar, &mut next_byte, "RIGHT_TAIL");
        let empty_declaration = test_declaration(&mut grammar, &mut next_byte, "Empty");
        let empty = grammar.push_type(crate::TypeDescription::new(
            "Empty",
            empty_declaration,
            TypeShape::Sequence { sites: Vec::new() },
        ));
        let left = push_recursive_sequence(
            &mut grammar,
            &mut next_byte,
            "Left",
            shared,
            empty,
            left_tail,
        );
        let right = push_recursive_sequence(
            &mut grammar,
            &mut next_byte,
            "Right",
            shared,
            empty,
            right_tail,
        );

        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        let product = build_decision_product(
            paired_residual(
                DecisionState(vec![DecisionItem::Type(empty), DecisionItem::Type(left)]),
                DecisionState(vec![DecisionItem::Type(empty), DecisionItem::Type(right)]),
                &grammar,
            ),
            &first_sequences,
        );
        assert!(
            matches!(product, DecisionProduct::Acyclic(_)),
            "the recursive-looking successor inspected its continuation and cannot pump",
        );
    }

    #[test]
    fn invalid_decision_products_are_suppressed_at_the_analysis_publication_boundary() {
        let grammar = GrammarDescription::new("empty", 0, 1, TextEquivalence::Sensitive);
        let atom_kinds = expand_atoms(&grammar);
        let first_sequences = FirstSequenceAnalysis::new(&grammar, &atom_kinds);
        let mut decisions = crate::decision_plan::plan_decisions(&grammar, &[], &first_sequences);
        decisions.corrupt_site_product_for_test();

        let diagnostic = publish_decisions(&grammar, &[], &first_sequences, decisions)
            .expect_err("invalid decisions cannot become AnalysisOutput");
        assert_eq!(diagnostic.code, DiagnosticCode::InternalAnalysisInvariant);
        assert_eq!(diagnostic.severity, crate::Severity::Error);
        assert!(
            diagnostic
                .message
                .contains("invalid frozen decision product")
        );
    }
}
