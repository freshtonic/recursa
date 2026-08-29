#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Source-independent semantic analysis over a normalized grammar.
//!
//! The pipeline validates dense identities before computing nullable, FIRST,
//! FOLLOW, Pratt extender, conflict, and provenance facts. Successful facts
//! are frozen into decision products; failed prerequisites suppress every
//! dependent phase so diagnostics never describe invented semantics.

#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};

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
    /// Bounded lookahead sequences used to distinguish shared prefixes.
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
    populate_first_sequences(grammar, &atom_kinds, &mut first_by_type);
    let extenders_by_type = compute_extenders(grammar, &first_by_type);
    let (mut proofs, first_provenance) = compute_first_provenance(grammar, &first_by_type);
    let extender_provenance =
        compute_extender_provenance(grammar, &first_by_type, &first_provenance, &mut proofs);
    let mut static_conflict_kinds_by_declaration =
        vec![KindSet::empty(grammar.kind_count); grammar.declarations.len()];
    let mut diagnostics = detect_conflicts(
        grammar,
        &first_by_type,
        &first_provenance,
        &atom_kinds,
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
        match publish_decisions(
            grammar,
            &first_by_type,
            crate::decision_plan::plan_decisions(grammar, &first_by_type),
        ) {
            Ok(decisions) => Some(decisions),
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                None
            }
        }
    };
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
    decisions: crate::DecisionProducts,
) -> Result<crate::DecisionProducts, Box<Diagnostic>> {
    decisions.validate(grammar, semantic).map_err(|invariant| {
        Box::new(Diagnostic::internal_analysis_invariant(format!(
            "invalid frozen decision product: {invariant}"
        )))
    })?;
    Ok(decisions)
}

/// Derives the first sequences for decision sites from normalized grammar structure and previously established semantic facts.
pub(crate) fn first_sequences_for_decision_sites(
    sites: &[SiteId],
    grammar: &GrammarDescription,
    _semantic: &[FirstFacts],
) -> Vec<FirstSequence> {
    let atom_kinds = expand_atoms(grammar);
    let mut language = FirstLanguageArena::new(grammar, &atom_kinds);
    let root = language.build(
        grammar.max_lookahead,
        vec![DecisionState::from_sites(sites)],
    );
    language.materialize(root)
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
    atom_kinds: &[KindSet],
    semantic: &mut [FirstFacts],
) {
    let mut language = FirstLanguageArena::new(grammar, atom_kinds);
    let mut roots = Vec::with_capacity(grammar.types.len());
    for index in 0..grammar.types.len() {
        roots.push(language.build(
            grammar.max_lookahead,
            vec![DecisionState(vec![DecisionItem::Type(TypeId::new(
                index.try_into().expect("validated type width"),
            ))])],
        ));
    }
    for (facts, root) in semantic.iter_mut().zip(roots) {
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
    atom_kinds: &'a [KindSet],
    /// Fixed-point end/continuation facts used only at the bounded cutoff.
    decision_summary_facts: DecisionSummaryFacts,
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
    /// Creates an empty symbolic FIRST arena for one grammar analysis.
    fn new(grammar: &'a GrammarDescription, atom_kinds: &'a [KindSet]) -> Self {
        Self {
            grammar,
            atom_kinds,
            decision_summary_facts: DecisionSummaryFacts::from_grammar(grammar),
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
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells: self.build_cells(&transitions, remaining - 1),
            covers: self.build_covers(&transitions, remaining - 1),
        };
        debug_assert!(first_language_cells_are_disjoint(&node.cells, self.grammar));
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
        if components.len() > 1 {
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
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells: self.build_projected_cells(&transitions, remaining - 1),
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
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells: self.build_depth_two_cells(&transitions),
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
        let node = FirstLanguageNode {
            exact_end,
            cutoff: false,
            cells: self.build_cutoff_cells(&transitions),
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
            self.atom_kinds,
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
        let frontier = decision_frontier(
            &self.decision_states[residual.index()],
            self.grammar,
            self.atom_kinds,
        );
        let mut transitions = Vec::with_capacity(frontier.transitions.len());
        for transition in frontier.transitions {
            #[cfg(test)]
            FIRST_LANGUAGE_CUTOFF_SUMMARY_READS
                .set(FIRST_LANGUAGE_CUTOFF_SUMMARY_READS.get().saturating_add(1));
            let summary = decision_cutoff_summary(&transition.next, &self.decision_summary_facts);
            let position = self.intern_position(transition.position);
            transitions.push(FirstLanguageCutoffTransition { position, summary });
        }
        self.sort_dedup_cutoff_transitions(&mut transitions);
        let frontier = FirstLanguageCutoffFrontier {
            can_end: frontier.can_end,
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
        debug_assert!(remaining > 0);
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
    /// Counts distinct commutative deterministic node overlays.
    static FIRST_LANGUAGE_NODE_UNION_MISSES: Cell<usize> = const { Cell::new(0) };
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
    },
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
                },
            ]
        } else {
            vec![DecisionItem::Type(site.target)]
        };
        items.extend(tail.iter().copied().map(DecisionItem::Site));
        Self(items)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Grammar-wide least-fixed-point facts needed to summarize cutoff residuals compositionally.
struct DecisionSummaryFacts {
    /// Exact-end and continuation facts indexed by type identity.
    by_type: Vec<u8>,
    /// Exact-end and continuation facts indexed by parse-site identity.
    by_site: Vec<u8>,
}

impl DecisionSummaryFacts {
    /// Solves the monotone two-bit grammar equations without optimistic recursion defaults.
    fn from_grammar(grammar: &GrammarDescription) -> Self {
        let mut by_type = vec![0_u8; grammar.types.len()];
        loop {
            let by_site = grammar
                .sites
                .iter()
                .map(|site| decision_site_summary(site, &by_type))
                .collect::<Vec<_>>();
            let next = grammar
                .types
                .iter()
                .map(|description| match &description.shape {
                    TypeShape::Token { .. } => FIRST_LANGUAGE_CUTOFF_CONTINUES,
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
                return Self { by_type, by_site };
            }
            by_type = next;
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
            DecisionItem::RepeatTail { target, separator } => {
                decision_repeat_tail_summary(*target, *separator, &self.by_type)
            }
        }
    }
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
                decision_repeat_tail_summary(site.target, semantics.separator_type(), by_type),
            ]) | FIRST_LANGUAGE_CUTOFF_END
        }
        ContainerCardinality::OneOrMore => decision_sequence_summary([
            target,
            decision_repeat_tail_summary(site.target, semantics.separator_type(), by_type),
        ]),
    }
}

/// Solves the least fixed point for one repeat-tail item's epsilon-or-continuation language.
fn decision_repeat_tail_summary(target: TypeId, separator: Option<TypeId>, by_type: &[u8]) -> u8 {
    let continuation = if let Some(separator) = separator {
        decision_sequence_summary([by_type[separator.index()], by_type[target.index()]])
    } else {
        by_type[target.index()]
    };
    FIRST_LANGUAGE_CUTOFF_END | (continuation & FIRST_LANGUAGE_CUTOFF_CONTINUES)
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

#[derive(Default)]
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Carries the decision parent consumed while deriving source-independent grammar facts.
struct DecisionParent {
    /// Supplies the state consumed by semantic analysis.
    state: PairedDecisionState,
    /// Supplies the left minimum stack depth consumed by semantic analysis.
    left_minimum_stack_depth: usize,
    /// Supplies the right minimum stack depth consumed by semantic analysis.
    right_minimum_stack_depth: usize,
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

/// Derives the analyze decision from normalized grammar structure and previously established semantic facts.
fn analyze_decision(
    left: DecisionState,
    right: DecisionState,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
) -> DecisionOutcome {
    let start = paired_residual(left, right, grammar);
    let graph = build_decision_product(start.clone(), grammar, atom_kinds);
    let sufficient_depth = if decision_graph_has_cycle(&start, &graph) {
        None
    } else {
        Some(longest_decision_path(&start, &graph, &mut BTreeMap::new()) + 1)
    };
    let witness_path =
        decision_witness_at_depth(&start, &graph, grammar.max_lookahead).unwrap_or_default();
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
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
) -> BTreeMap<PairedDecisionState, Vec<(PairedDecisionState, DecisionWitness)>> {
    let mut graph = BTreeMap::new();
    let mut parents = BTreeMap::from([(start.clone(), Vec::new())]);
    let mut queue = VecDeque::from([start]);
    while let Some(state) = queue.pop_front() {
        let mut successors = Vec::new();
        for successor in paired_decision_successors(&state, grammar, atom_kinds) {
            let pumped = pumping_ancestor(&state, &successor, &parents);
            if pumped.is_none() {
                let parent = DecisionParent {
                    state: state.clone(),
                    left_minimum_stack_depth: successor.left_minimum_stack_depth,
                    right_minimum_stack_depth: successor.right_minimum_stack_depth,
                };
                let incoming = parents.entry(successor.state.clone()).or_default();
                if incoming.binary_search(&parent).is_err() {
                    incoming.push(parent);
                    incoming.sort();
                    // A new derivation to an existing residual can establish a
                    // boundary-safe pumping proof that its first derivation did
                    // not. Revisit that residual with the expanded parent graph.
                    queue.push_back(successor.state.clone());
                }
            }
            successors.push((pumped.unwrap_or(successor.state), successor.witness));
        }
        graph.insert(state, successors);
    }
    graph
}

/// Derives the pumping ancestor from normalized grammar structure and previously established semantic facts.
fn pumping_ancestor(
    current: &PairedDecisionState,
    successor: &PairedDecisionSuccessor,
    parents: &BTreeMap<PairedDecisionState, Vec<DecisionParent>>,
) -> Option<PairedDecisionState> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([current]);
    while let Some(candidate) = queue.pop_front() {
        if !visited.insert(candidate) {
            continue;
        }
        if paired_residual_self_embeds(candidate, &successor.state)
            && path_preserves_continuations(candidate, current, successor, parents)
        {
            return Some(candidate.clone());
        }
        if let Some(incoming) = parents.get(candidate) {
            queue.extend(incoming.iter().map(|parent| &parent.state));
        }
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
    ancestor: &PairedDecisionState,
    current: &PairedDecisionState,
    successor: &PairedDecisionSuccessor,
    parents: &BTreeMap<PairedDecisionState, Vec<DecisionParent>>,
) -> bool {
    let PairedDecisionState::Residual(ancestor_left, ancestor_right) = ancestor else {
        return false;
    };
    let left_continuation = &ancestor_left.0[1..];
    let right_continuation = &ancestor_right.0[1..];
    if successor.left_minimum_stack_depth <= left_continuation.len()
        || successor.right_minimum_stack_depth <= right_continuation.len()
    {
        return false;
    }
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([current]);
    while let Some(state) = queue.pop_front() {
        if !visited.insert(state) {
            continue;
        }
        if state == ancestor {
            return true;
        }
        let PairedDecisionState::Residual(left, right) = state else {
            continue;
        };
        if !left.0.ends_with(left_continuation) || !right.0.ends_with(right_continuation) {
            continue;
        }
        for parent in parents.get(state).into_iter().flatten() {
            if parent.left_minimum_stack_depth > left_continuation.len()
                && parent.right_minimum_stack_depth > right_continuation.len()
            {
                queue.push_back(&parent.state);
            }
        }
    }
    false
}

/// Derives the paired decision successors from normalized grammar structure and previously established semantic facts.
fn paired_decision_successors(
    state: &PairedDecisionState,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
) -> Vec<PairedDecisionSuccessor> {
    if matches!(state, PairedDecisionState::End) {
        return vec![PairedDecisionSuccessor {
            state: PairedDecisionState::End,
            witness: DecisionWitness::End,
            left_minimum_stack_depth: 0,
            right_minimum_stack_depth: 0,
        }];
    }
    let PairedDecisionState::Residual(left, right) = state else {
        unreachable!()
    };
    let left = decision_frontier(left, grammar, atom_kinds);
    let right = decision_frontier(right, grammar, atom_kinds);
    let mut successors = Vec::new();
    if left.can_end && right.can_end {
        successors.push(PairedDecisionSuccessor {
            state: PairedDecisionState::End,
            witness: DecisionWitness::End,
            left_minimum_stack_depth: 0,
            right_minimum_stack_depth: 0,
        });
    }
    for left in &left.transitions {
        for right in &right.transitions {
            if !predicates_overlap(
                left.position.text_predicate,
                right.position.text_predicate,
                &grammar.text_predicates,
                grammar.text_equivalence,
            ) {
                continue;
            }
            let intersection = left
                .position
                .kinds
                .intersection(&right.position.kinds)
                .expect("decision positions have the grammar width");
            let Some(kind) = intersection.iter().next() else {
                continue;
            };
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
    }
    successors.sort();
    successors.dedup();
    successors
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
            left.target == right.target && left.container.semantics() == right.container.semantics()
        }
        (
            DecisionItem::RepeatTail {
                target: left_target,
                separator: left_separator,
            },
            DecisionItem::RepeatTail {
                target: right_target,
                separator: right_separator,
            },
        ) => left_target == right_target && left_separator == right_separator,
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
            DecisionItem::RepeatTail { target, separator } => {
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
                    },
                ]);
                queue_decision_expansion(
                    expansion,
                    rest,
                    minimum_stack_depth,
                    &mut best_minimum_depth,
                    &mut queue,
                );
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

/// Derives the decision graph has cycle from normalized grammar structure and previously established semantic facts.
fn decision_graph_has_cycle(
    start: &PairedDecisionState,
    graph: &BTreeMap<PairedDecisionState, Vec<(PairedDecisionState, DecisionWitness)>>,
) -> bool {
    fn visit(
        state: &PairedDecisionState,
        graph: &BTreeMap<PairedDecisionState, Vec<(PairedDecisionState, DecisionWitness)>>,
        visiting: &mut BTreeSet<PairedDecisionState>,
        visited: &mut BTreeSet<PairedDecisionState>,
    ) -> bool {
        if visiting.contains(state) {
            return true;
        }
        if !visited.insert(state.clone()) {
            return false;
        }
        visiting.insert(state.clone());
        let cyclic = graph[state]
            .iter()
            .any(|(successor, _)| visit(successor, graph, visiting, visited));
        visiting.remove(state);
        cyclic
    }
    visit(start, graph, &mut BTreeSet::new(), &mut BTreeSet::new())
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
fn decision_witness_at_depth(
    start: &PairedDecisionState,
    graph: &BTreeMap<PairedDecisionState, Vec<(PairedDecisionState, DecisionWitness)>>,
    depth: usize,
) -> Option<Vec<DecisionWitness>> {
    let mut frontier = BTreeMap::from([(start.clone(), Vec::new())]);
    for _ in 0..depth {
        let mut next = BTreeMap::new();
        for (state, path) in frontier {
            for (successor, witness) in &graph[&state] {
                let mut candidate = path.clone();
                if !matches!(candidate.last(), Some(DecisionWitness::End)) {
                    candidate.push(witness.clone());
                }
                let slot = next.entry(successor.clone()).or_insert(candidate.clone());
                if candidate < *slot {
                    *slot = candidate;
                }
            }
        }
        if next.is_empty() {
            return None;
        }
        frontier = next;
    }
    frontier.into_values().min()
}

/// Produces canonical ambiguity diagnostics from overlapping semantic decisions.
fn detect_conflicts(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    provenanced: &[Vec<ProvenancedPosition>],
    atom_kinds: &[KindSet],
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
                        let decision = analyze_decision(
                            DecisionState::from_sites(&left.sites),
                            DecisionState::from_sites(&right.sites),
                            grammar,
                            atom_kinds,
                        );
                        if decision
                            .sufficient_depth
                            .is_some_and(|depth| depth <= grammar.max_lookahead)
                        {
                            continue;
                        }
                        let evidence = overlap_evidence(
                            &left_first.positions,
                            &right_first.positions,
                            grammar,
                            proofs,
                        );
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
                provenanced,
                atom_kinds,
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
                    provenanced,
                    atom_kinds,
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
                        provenanced,
                        atom_kinds,
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

#[allow(clippy::too_many_arguments)]
/// Diagnoses overlapping Pratt roles and extender selections.
fn detect_pratt_conflicts(
    owner: TypeId,
    owner_name: &str,
    variants: &[crate::PrattVariant],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    provenanced: &[Vec<ProvenancedPosition>],
    atom_kinds: &[KindSet],
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
        provenanced,
        atom_kinds,
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
        provenanced,
        atom_kinds,
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
    provenanced: &[Vec<ProvenancedPosition>],
    atom_kinds: &[KindSet],
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
            let decision = analyze_decision(
                DecisionState::from_sites(&left.sites),
                DecisionState::from_sites(&right.sites),
                grammar,
                atom_kinds,
            );
            if decision
                .sufficient_depth
                .is_some_and(|depth| depth <= grammar.max_lookahead)
            {
                continue;
            }
            let evidence = overlap_evidence(
                &left_first.positions,
                &right_first.positions,
                grammar,
                proofs,
            );
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
    provenanced: &[Vec<ProvenancedPosition>],
    atom_kinds: &[KindSet],
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
        let tail_sites = &sites[index + 1..];
        let decision = analyze_decision(
            DecisionState::consuming_site_and_tail(site, tail_sites),
            DecisionState::from_sites(tail_sites),
            grammar,
            atom_kinds,
        );
        if decision
            .sufficient_depth
            .is_some_and(|depth| depth <= grammar.max_lookahead)
        {
            continue;
        }
        let sufficient_depth = decision.sufficient_depth;
        let current =
            first_for_sites_provenance(owner, &[site_id], grammar, semantic, provenanced, proofs);
        if let Some(evidence) =
            overlap_evidence(&current.positions, &tail.positions, grammar, proofs)
        {
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
                items.push(DecisionItem::RepeatTail { target, separator });
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
        let facts = DecisionSummaryFacts::from_grammar(&grammar);
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
        let summary_facts = DecisionSummaryFacts::from_grammar(&grammar);
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
        let parents = BTreeMap::from([(ancestor.clone(), Vec::new())]);

        assert_eq!(pumping_ancestor(&ancestor, &successor, &parents), None);
    }

    #[test]
    fn invalid_decision_products_are_suppressed_at_the_analysis_publication_boundary() {
        let grammar = GrammarDescription::new("empty", 0, 1, TextEquivalence::Sensitive);
        let mut decisions = crate::decision_plan::plan_decisions(&grammar, &[]);
        decisions.corrupt_site_product_for_test();

        let diagnostic = publish_decisions(&grammar, &[], decisions)
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
