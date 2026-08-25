use crate::{
    AtomExpression, AtomId, ConflictEvidence, ConflictRemedy, ConflictWitness, Diagnostic,
    DiagnosticCode, GrammarDescription, KindId, KindSet, Location, PrattPosition, ProofArena,
    ProofId, ProvenanceStep, ProvenancedKindSet, SiteContainer, SiteId, TextPredicate,
    TextPredicateId, TypeId, TypeShape,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisReport {
    pub diagnostics: Vec<Diagnostic>,
    pub output: Option<AnalysisOutput>,
    locations: Vec<Location>,
    proofs: ProofArena,
    diagnostic_context: DiagnosticContext,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DiagnosticContext {
    kind_names: Vec<String>,
    atom_names: Vec<String>,
    declaration_names: Vec<String>,
    type_names: Vec<String>,
    text_predicates: Vec<TextPredicate>,
}

impl DiagnosticContext {
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
    pub fn locations(&self) -> &[Location] {
        &self.locations
    }

    pub fn proofs(&self) -> &ProofArena {
        &self.proofs
    }

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
    fn kind_name(&self, kind: KindId) -> &str {
        self.kind_names
            .get(kind.index())
            .map_or("<missing kind>", String::as_str)
    }

    fn atom_name(&self, atom: AtomId) -> &str {
        self.atom_names
            .get(atom.index())
            .map_or("<missing atom>", String::as_str)
    }

    fn declaration_name(&self, declaration: crate::DeclId) -> &str {
        self.declaration_names
            .get(declaration.index())
            .map_or("<missing declaration>", String::as_str)
    }

    fn type_name(&self, type_id: TypeId) -> &str {
        self.type_names
            .get(type_id.index())
            .map_or("<missing type>", String::as_str)
    }

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
            ProvenanceStep::TextPredicate { predicate, .. } => {
                let predicate = self.text_predicates.get(predicate.index()).map_or_else(
                    || "<missing predicate>".to_owned(),
                    |value| format!("{value:?}"),
                );
                format!("text is refined by {predicate}")
            }
        }
    }
}

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
pub struct AnalysisOutput {
    first_by_type: Vec<FirstFacts>,
    extenders_by_type: Vec<Option<ExtenderFacts>>,
    first_provenance_by_type: Vec<Vec<ProvenancedKindSet>>,
    extender_provenance_by_type: Vec<Option<Vec<ProvenancedKindSet>>>,
}

impl AnalysisOutput {
    pub fn first(&self, type_id: TypeId) -> Option<&FirstFacts> {
        self.first_by_type.get(type_id.index())
    }

    pub fn first_by_type(&self) -> &[FirstFacts] {
        &self.first_by_type
    }

    pub fn extenders(&self, type_id: TypeId) -> Option<&ExtenderFacts> {
        self.extenders_by_type
            .get(type_id.index())
            .and_then(Option::as_ref)
    }

    pub fn extenders_by_type(&self) -> &[Option<ExtenderFacts>] {
        &self.extenders_by_type
    }

    pub fn first_provenance_by_type(&self) -> &[Vec<ProvenancedKindSet>] {
        &self.first_provenance_by_type
    }

    pub fn extender_provenance(&self, type_id: TypeId) -> Option<&[ProvenancedKindSet]> {
        self.extender_provenance_by_type
            .get(type_id.index())
            .and_then(Option::as_deref)
    }

    pub fn extender_provenance_by_type(&self) -> &[Option<Vec<ProvenancedKindSet>>] {
        &self.extender_provenance_by_type
    }

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
pub struct FirstFacts {
    kinds: KindSet,
    pub positions: Vec<FirstPosition>,
    pub nullable: bool,
}

impl FirstFacts {
    pub fn kinds(&self) -> &KindSet {
        &self.kinds
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtenderFacts {
    kinds: KindSet,
    pub positions: Vec<FirstPosition>,
}

impl ExtenderFacts {
    pub fn kinds(&self) -> &KindSet {
        &self.kinds
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirstPosition {
    pub kinds: KindSet,
    pub text_predicate: Option<TextPredicateId>,
}

pub fn analyze(grammar: GrammarDescription) -> AnalysisReport {
    let locations = grammar.locations.clone();
    let diagnostic_context = DiagnosticContext::from_grammar(&grammar);
    let diagnostics = validate(&grammar);
    if !diagnostics.is_empty() {
        return AnalysisReport {
            diagnostics,
            output: None,
            locations,
            proofs: ProofArena::default(),
            diagnostic_context,
        };
    }

    let atom_kinds = expand_atoms(&grammar);
    let first_by_type = compute_first(&grammar, &atom_kinds);
    let diagnostics = detect_left_recursion(&grammar, &first_by_type);
    if !diagnostics.is_empty() {
        return AnalysisReport {
            diagnostics,
            output: None,
            locations,
            proofs: ProofArena::default(),
            diagnostic_context,
        };
    }
    let extenders_by_type = compute_extenders(&grammar, &first_by_type);
    let (mut proofs, first_provenance) = compute_first_provenance(&grammar, &first_by_type);
    let extender_provenance =
        compute_extender_provenance(&grammar, &first_by_type, &first_provenance, &mut proofs);
    let diagnostics = detect_conflicts(&grammar, &first_by_type, &first_provenance, &mut proofs);
    if !diagnostics.is_empty() {
        return AnalysisReport {
            diagnostics,
            output: None,
            locations,
            proofs,
            diagnostic_context,
        };
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
    AnalysisReport {
        diagnostics: Vec::new(),
        output: Some(AnalysisOutput {
            first_by_type,
            extenders_by_type,
            first_provenance_by_type,
            extender_provenance_by_type,
        }),
        locations,
        proofs,
        diagnostic_context,
    }
}

fn validate(grammar: &GrammarDescription) -> Vec<Diagnostic> {
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
        if let SiteContainer::Repeat {
            separator: Some(separator),
            ..
        } = site.container
            && separator.index() >= grammar.types.len()
        {
            diagnostics.push(Diagnostic::invalid_reference(format!(
                "site {index} references missing separator type {}",
                separator.get(),
            )));
        }
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

fn compute_first(grammar: &GrammarDescription, atom_kinds: &[KindSet]) -> Vec<FirstFacts> {
    let empty = || first_facts(Vec::new(), false, grammar.kind_count);
    let mut facts = vec![empty(); grammar.types.len()];
    loop {
        let previous = facts.clone();
        for (index, description) in grammar.types.iter().enumerate() {
            facts[index] = first_for_shape(&description.shape, grammar, atom_kinds, &previous);
        }
        if facts == previous {
            return facts;
        }
    }
}

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

fn site_nullable(site: &crate::ParseSite, target: &FirstFacts) -> bool {
    match site.container {
        SiteContainer::Direct | SiteContainer::Boxed => target.nullable,
        SiteContainer::Optional => true,
        SiteContainer::Repeat { at_least_one, .. } => !at_least_one || target.nullable,
    }
}

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

fn first_facts(positions: Vec<FirstPosition>, nullable: bool, kind_count: usize) -> FirstFacts {
    let mut kinds = KindSet::empty(kind_count);
    for position in &positions {
        kinds.union_assign(&position.kinds);
    }
    FirstFacts {
        kinds,
        positions,
        nullable,
    }
}

#[derive(Clone, Copy, Debug)]
struct FirstCornerEdge {
    from: TypeId,
    to: TypeId,
    declaration: crate::DeclId,
}

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

fn diagnostic_source_key(
    diagnostic: &Diagnostic,
    grammar: &GrammarDescription,
) -> (u32, u32, u32, crate::DiagnosticCode, String) {
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
        diagnostic.message.clone(),
    )
}

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

fn strongly_connected_indices(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct Tarjan<'a> {
        edges: &'a [Vec<usize>],
        next_index: usize,
        indices: Vec<Option<usize>>,
        lowlinks: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }

    impl Tarjan<'_> {
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
struct ProvenancedPosition {
    kinds: ProvenancedKindSet,
    text_predicate: Option<TextPredicateId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProvenancedFirst {
    positions: Vec<ProvenancedPosition>,
    nullable: bool,
}

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
fn first_for_shape_provenance(
    owner: TypeId,
    description: &crate::TypeDescription,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    atom_provenance: &[ProvenancedKindSet],
    previous: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> ProvenancedFirst {
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

fn merge_provenanced_positions(
    into: &mut Vec<ProvenancedPosition>,
    from: &[ProvenancedPosition],
    proofs: &ProofArena,
) {
    for position in from {
        merge_provenanced_position(into, position.clone(), proofs);
    }
}

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
                let first = first_for_sites_provenance(
                    owner,
                    &variant.sites,
                    grammar,
                    semantic,
                    first_provenance,
                    proofs,
                );
                merge_provenanced_positions(&mut positions, &first.positions, proofs);
            }
            Some(positions)
        })
        .collect()
}

const fn is_pratt_nud(position: PrattPosition) -> bool {
    matches!(position, PrattPosition::Atom | PrattPosition::Prefix { .. })
}

const fn is_pratt_led(position: PrattPosition) -> bool {
    matches!(
        position,
        PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
    )
}

fn detect_conflicts(
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
) -> Vec<Diagnostic> {
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
                        if let Some(evidence) = overlap_evidence(
                            &left_first.positions,
                            &right_first.positions,
                            grammar,
                            proofs,
                        ) {
                            diagnostics.push(Diagnostic::conflict(
                                DiagnosticCode::EnumFirstOverlap,
                                format!(
                                    "enum `{}` has alternatives with overlapping FIRST sets",
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
                                ConflictRemedy::DisambiguateAlternatives {
                                    left: left.declaration,
                                    right: right.declaration,
                                },
                            ));
                        }
                    }
                }
            }
            TypeShape::Sequence { sites } => detect_static_conflicts_for_sites(
                owner,
                sites,
                grammar,
                semantic,
                provenanced,
                proofs,
                &mut diagnostics,
            ),
            TypeShape::Pratt { variants } => {
                detect_pratt_conflicts(
                    owner,
                    &description.name,
                    variants,
                    grammar,
                    semantic,
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
                        provenanced,
                        proofs,
                        &mut diagnostics,
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
fn detect_pratt_conflicts(
    owner: TypeId,
    owner_name: &str,
    variants: &[crate::PrattVariant],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
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
        provenanced,
        proofs,
        diagnostics,
    );
}

#[allow(clippy::too_many_arguments)]
fn detect_pratt_group_conflicts(
    owner: TypeId,
    owner_name: &str,
    variants: &[&crate::PrattVariant],
    code: DiagnosticCode,
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for left_index in 0..variants.len() {
        for right_index in left_index + 1..variants.len() {
            let left = variants[left_index];
            let right = variants[right_index];
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
            if let Some(evidence) = overlap_evidence(
                &left_first.positions,
                &right_first.positions,
                grammar,
                proofs,
            ) {
                let left_position = pratt_position_name(left.position);
                let right_position = pratt_position_name(right.position);
                diagnostics.push(Diagnostic::conflict(
                    code,
                    format!(
                        "Pratt `{owner_name}` has overlapping {}",
                        pratt_overlap_name(left.position, right.position),
                    ),
                    declaration_label(
                        left.declaration,
                        grammar,
                        format!("this {left_position} variant can start with the witness kind"),
                    ),
                    declaration_label(
                        right.declaration,
                        grammar,
                        format!("this {right_position} variant can start with the same kind"),
                    ),
                    evidence,
                    ConflictRemedy::DisambiguatePrattVariants {
                        pratt: owner,
                        left: left.declaration,
                        right: right.declaration,
                    },
                ));
            }
        }
    }
}

const fn pratt_position_name(position: PrattPosition) -> &'static str {
    match position {
        PrattPosition::Atom => "atom",
        PrattPosition::Prefix { .. } => "prefix",
        PrattPosition::Infix { .. } => "infix",
        PrattPosition::Postfix { .. } => "postfix",
    }
}

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
fn detect_static_conflicts_for_sites(
    owner: TypeId,
    sites: &[SiteId],
    grammar: &GrammarDescription,
    semantic: &[FirstFacts],
    provenanced: &[Vec<ProvenancedPosition>],
    proofs: &mut ProofArena,
    diagnostics: &mut Vec<Diagnostic>,
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
        if let Some(evidence) =
            overlap_evidence(&current.positions, &tail.positions, grammar, proofs)
        {
            let tail_site = sites[index + 1];
            diagnostics.push(Diagnostic::conflict(
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
                ConflictRemedy::MakeSiteRequired { site: site_id },
            ));
        }
    }
}

fn declaration_label(
    declaration: crate::DeclId,
    grammar: &GrammarDescription,
    message: impl Into<String>,
) -> crate::DiagnosticLabel {
    crate::DiagnosticLabel::located(grammar.declarations[declaration.index()].location, message)
}

fn overlap_evidence(
    left: &[ProvenancedPosition],
    right: &[ProvenancedPosition],
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
            ) {
                continue;
            }
            let intersection = left_position
                .kinds
                .kinds()
                .intersection(right_position.kinds.kinds())
                .expect("positions have the grammar width");
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

fn proof_pair_is_better(
    candidate: (ProofId, ProofId),
    current: (ProofId, ProofId),
    proofs: &ProofArena,
) -> bool {
    compare_proof_pairs(candidate, current, proofs).is_lt()
}

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

fn predicates_overlap(
    left: Option<TextPredicateId>,
    right: Option<TextPredicateId>,
    predicates: &[TextPredicate],
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
        (TextPredicate::Exact(left), TextPredicate::Exact(right)) => text_eq(left, right),
        (TextPredicate::Exact(exact), TextPredicate::OneOf(options))
        | (TextPredicate::OneOf(options), TextPredicate::Exact(exact)) => {
            options.iter().any(|option| text_eq(exact, option))
        }
        (TextPredicate::Exact(exact), TextPredicate::NotIn(excluded))
        | (TextPredicate::NotIn(excluded), TextPredicate::Exact(exact)) => {
            !excluded.iter().any(|value| text_eq(exact, value))
        }
        (TextPredicate::OneOf(left), TextPredicate::OneOf(right)) => left
            .iter()
            .any(|left| right.iter().any(|right| text_eq(left, right))),
        (TextPredicate::OneOf(options), TextPredicate::NotIn(excluded))
        | (TextPredicate::NotIn(excluded), TextPredicate::OneOf(options)) => options
            .iter()
            .any(|option| !excluded.iter().any(|value| text_eq(option, value))),
        (TextPredicate::NotIn(_), TextPredicate::NotIn(_)) => true,
    }
}

fn text_eq(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

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

fn render_kind_set(set: &KindSet) -> String {
    let members = set
        .iter()
        .map(|kind| kind.get().to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{members}}}")
}
