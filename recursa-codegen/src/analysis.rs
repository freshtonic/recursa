use crate::{
    AtomExpression, AtomId, Diagnostic, GrammarDescription, KindSet, Location, PrattPosition,
    SiteContainer, SiteId, TextPredicateId, TypeId, TypeShape,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisReport {
    pub diagnostics: Vec<Diagnostic>,
    pub output: Option<AnalysisOutput>,
    locations: Vec<Location>,
}

impl AnalysisReport {
    pub fn locations(&self) -> &[Location] {
        &self.locations
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
            if let Some(remedy) = &diagnostic.remedy {
                writeln!(rendered, "  remedy: {}", remedy.canonical_text())
                    .expect("writing to a String cannot fail");
            }
        }
        rendered
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
    let diagnostics = validate(&grammar);
    if !diagnostics.is_empty() {
        return AnalysisReport {
            diagnostics,
            output: None,
            locations,
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
        };
    }
    let extenders_by_type = compute_extenders(&grammar, &first_by_type);
    AnalysisReport {
        diagnostics: Vec::new(),
        output: Some(AnalysisOutput {
            first_by_type,
            extenders_by_type,
        }),
        locations,
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
