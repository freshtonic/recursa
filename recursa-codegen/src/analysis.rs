#![deny(missing_docs, clippy::missing_docs_in_private_items)]

//! Source-independent semantic analysis over a normalized grammar.
//!
//! The pipeline validates dense identities before computing nullable, FIRST,
//! FOLLOW, Pratt extender, conflict, and provenance facts. Successful facts
//! are frozen into decision products; failed prerequisites suppress every
//! dependent phase so diagnostics never describe invented semantics.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::ir::{ContainerCardinality, ContainerSemantics};
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
    let first_by_type = compute_first(grammar, &atom_kinds);
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
    semantic: &[FirstFacts],
) -> Vec<FirstSequence> {
    let previous = semantic
        .iter()
        .map(|facts| facts.sequences.clone())
        .collect::<Vec<_>>();
    first_sequences_for_sites(sites, grammar, semantic, &previous, grammar.max_lookahead)
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

    let sequences = compute_first_sequences(grammar, atom_kinds, &facts, grammar.max_lookahead);
    for (facts, sequences) in facts.iter_mut().zip(sequences) {
        facts.sequences = sequences;
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

/// Computes bounded FIRST sequences for predictive dispatch.
fn compute_first_sequences(
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
    semantic: &[FirstFacts],
    max_depth: usize,
) -> Vec<Vec<FirstSequence>> {
    let mut sequences = vec![Vec::new(); grammar.types.len()];
    loop {
        let previous = sequences.clone();
        for (index, description) in grammar.types.iter().enumerate() {
            sequences[index] = first_sequences_for_shape(
                &description.shape,
                grammar,
                atom_kinds,
                semantic,
                &previous,
                max_depth,
            );
        }
        if sequences == previous {
            return sequences;
        }
    }
}

/// Derives the first sequences for shape from normalized grammar structure and previously established semantic facts.
fn first_sequences_for_shape(
    shape: &TypeShape,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
    semantic: &[FirstFacts],
    previous: &[Vec<FirstSequence>],
    max_depth: usize,
) -> Vec<FirstSequence> {
    let mut sequences = match shape {
        TypeShape::Token {
            atom,
            text_predicate,
        } => vec![FirstSequence {
            positions: vec![FirstPosition {
                kinds: atom_kinds[atom.index()].clone(),
                text_predicate: *text_predicate,
            }],
            ends: true,
        }],
        TypeShape::Alias { site } => {
            first_sequences_for_sites(&[*site], grammar, semantic, previous, max_depth)
        }
        TypeShape::Sequence { sites } => {
            first_sequences_for_sites(sites, grammar, semantic, previous, max_depth)
        }
        TypeShape::Enum { alternatives } => alternatives
            .iter()
            .flat_map(|alternative| {
                first_sequences_for_sites(
                    &alternative.sites,
                    grammar,
                    semantic,
                    previous,
                    max_depth,
                )
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
            .flat_map(|variant| {
                first_sequences_for_sites(&variant.sites, grammar, semantic, previous, max_depth)
            })
            .collect(),
    };
    sort_dedup_first_sequences(&mut sequences);
    sequences
}

/// Derives the first sequences for sites from normalized grammar structure and previously established semantic facts.
fn first_sequences_for_sites(
    sites: &[SiteId],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    previous: &[Vec<FirstSequence>],
    max_depth: usize,
) -> Vec<FirstSequence> {
    let mut combined = vec![FirstSequence {
        positions: Vec::new(),
        ends: true,
    }];
    for site_id in sites {
        let site = &grammar.sites[site_id.index()];
        let choices = sequences_for_site(site, semantic, previous, max_depth);
        combined = concatenate_sequence_choices(&combined, &choices, max_depth);
    }
    sort_dedup_first_sequences(&mut combined);
    combined
}

/// Derives the sequences for site from normalized grammar structure and previously established semantic facts.
fn sequences_for_site(
    site: &crate::ParseSite,
    semantic: &[FirstFacts],
    previous: &[Vec<FirstSequence>],
    max_depth: usize,
) -> Vec<FirstSequence> {
    let target = &previous[site.target.index()];
    let semantics = container_semantics(site);
    let separator = semantics
        .separator_type()
        .map(|id| previous[id.index()].as_slice());
    let mut sequences = match semantics.cardinality {
        ContainerCardinality::One => target.clone(),
        ContainerCardinality::Optional => with_empty_sequence(target.clone()),
        ContainerCardinality::ZeroOrMore => repeat_sequences(target, separator, false, max_depth),
        ContainerCardinality::OneOrMore => repeat_sequences(target, separator, true, max_depth),
        ContainerCardinality::OptionalOneOrMore => {
            with_empty_sequence(repeat_sequences(target, separator, true, max_depth))
        }
    }
    .into_iter()
    .filter(|sequence| {
        !sequence.positions.is_empty() || site_nullable(site, &semantic[site.target.index()])
    })
    .collect::<Vec<_>>();
    sort_dedup_first_sequences(&mut sequences);
    sequences
}

/// Derives the with empty sequence from normalized grammar structure and previously established semantic facts.
fn with_empty_sequence(mut sequences: Vec<FirstSequence>) -> Vec<FirstSequence> {
    sequences.push(FirstSequence {
        positions: Vec::new(),
        ends: true,
    });
    sort_dedup_first_sequences(&mut sequences);
    sequences
}

/// Derives the repeat sequences from normalized grammar structure and previously established semantic facts.
fn repeat_sequences(
    target: &[FirstSequence],
    separator: Option<&[FirstSequence]>,
    at_least_one: bool,
    max_depth: usize,
) -> Vec<FirstSequence> {
    let mut result = if at_least_one {
        target.to_vec()
    } else {
        with_empty_sequence(target.to_vec())
    };
    let mut frontier = target.to_vec();
    loop {
        let before = result.clone();
        let continuation = if let Some(separator) = separator {
            concatenate_sequence_choices(separator, target, max_depth)
        } else {
            target.to_vec()
        };
        frontier = concatenate_sequence_choices(&frontier, &continuation, max_depth);
        result.extend(frontier.iter().cloned());
        sort_dedup_first_sequences(&mut result);
        if result == before
            || frontier
                .iter()
                .all(|sequence| sequence.positions.len() >= max_depth)
        {
            return result;
        }
    }
}

/// Derives the concatenate sequence choices from normalized grammar structure and previously established semantic facts.
fn concatenate_sequence_choices(
    prefixes: &[FirstSequence],
    suffixes: &[FirstSequence],
    max_depth: usize,
) -> Vec<FirstSequence> {
    let mut combined = Vec::new();
    for prefix in prefixes {
        if !prefix.ends {
            combined.push(prefix.clone());
            continue;
        }
        for suffix in suffixes {
            let mut positions = prefix.positions.clone();
            let combined_length = positions.len().saturating_add(suffix.positions.len());
            positions.extend(
                suffix
                    .positions
                    .iter()
                    .take(max_depth - positions.len())
                    .cloned(),
            );
            combined.push(FirstSequence {
                positions,
                ends: suffix.ends && combined_length <= max_depth,
            });
        }
    }
    sort_dedup_first_sequences(&mut combined);
    combined
}

/// Sorts dedup first sequences.
fn sort_dedup_first_sequences(sequences: &mut Vec<FirstSequence>) {
    sequences.sort_by(compare_first_sequences);
    sequences.dedup();
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Carries the decision state consumed while deriving source-independent grammar facts.
struct DecisionState(Vec<DecisionItem>);

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
fn decision_frontier(
    start: &DecisionState,
    grammar: &GrammarDescription,
    atom_kinds: &[KindSet],
) -> DecisionFrontier {
    // Explore the bounded lookahead product while deduplicating frontier states to terminate recursive shared prefixes.
    let mut frontier = DecisionFrontier::default();
    let mut best_minimum_depth = BTreeMap::from([(start.clone(), start.0.len())]);
    let mut queue = VecDeque::from([(start.clone(), start.0.len())]);
    while let Some((state, minimum_stack_depth)) = queue.pop_front() {
        if best_minimum_depth.get(&state) != Some(&minimum_stack_depth) {
            continue;
        }
        let Some((first, rest)) = state.0.split_first() else {
            frontier.can_end = true;
            continue;
        };
        match first {
            DecisionItem::Type(type_id) => {
                let description = &grammar.types[type_id.index()];
                match &description.shape {
                    TypeShape::Token {
                        atom,
                        text_predicate,
                    } => frontier.transitions.push(DecisionTransition {
                        position: FirstPosition {
                            kinds: atom_kinds[atom.index()].clone(),
                            text_predicate: *text_predicate,
                        },
                        next: DecisionState(rest.to_vec()),
                        minimum_stack_depth,
                    }),
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
