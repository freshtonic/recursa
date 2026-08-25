use crate::{
    AtomExpression, AtomId, Diagnostic, GrammarDescription, KindSet, PrattPosition, SiteContainer,
    SiteId, TextPredicateId, TypeId, TypeShape,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisReport {
    pub diagnostics: Vec<Diagnostic>,
    pub output: Option<AnalysisOutput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisOutput {
    first_by_type: Vec<FirstFacts>,
}

impl AnalysisOutput {
    pub fn first(&self, type_id: TypeId) -> Option<&FirstFacts> {
        self.first_by_type.get(type_id.index())
    }

    pub fn first_by_type(&self) -> &[FirstFacts] {
        &self.first_by_type
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirstFacts {
    pub positions: Vec<FirstPosition>,
    pub nullable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FirstPosition {
    pub kinds: KindSet,
    pub text_predicate: Option<TextPredicateId>,
}

pub fn analyze(grammar: GrammarDescription) -> AnalysisReport {
    let diagnostics = validate(&grammar);
    if !diagnostics.is_empty() {
        return AnalysisReport {
            diagnostics,
            output: None,
        };
    }

    let atom_kinds = expand_atoms(&grammar);
    let first_by_type = compute_first(&grammar, &atom_kinds);
    AnalysisReport {
        diagnostics: Vec::new(),
        output: Some(AnalysisOutput { first_by_type }),
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
    fn visit(atom: AtomId, grammar: &GrammarDescription, state: &mut [u8]) -> Option<AtomId> {
        if atom.index() >= grammar.atoms.len() {
            return None;
        }
        match state[atom.index()] {
            1 => return Some(atom),
            2 => return None,
            _ => {}
        }
        state[atom.index()] = 1;
        for reference in grammar.atoms[atom.index()].expression.references() {
            if let Some(cycle) = visit(reference, grammar, state) {
                return Some(cycle);
            }
        }
        state[atom.index()] = 2;
        None
    }

    let mut state = vec![0; grammar.atoms.len()];
    for index in 0..grammar.atoms.len() {
        if let Some(atom) = visit(AtomId::new(index as u32), grammar, &mut state) {
            diagnostics.push(Diagnostic::invalid_grammar(format!(
                "atom {} participates in a membership cycle",
                atom.get(),
            )));
            break;
        }
    }
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
    let empty = || FirstFacts {
        positions: Vec::new(),
        nullable: false,
    };
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
            positions: vec![FirstPosition {
                kinds: atom_kinds[atom.index()].clone(),
                text_predicate: *text_predicate,
            }],
            nullable: false,
        },
        TypeShape::Alias { site } => first_for_sites(&[*site], grammar, facts),
        TypeShape::Sequence { sites } => first_for_sites(sites, grammar, facts),
        TypeShape::Enum { alternatives } => {
            let mut result = FirstFacts {
                positions: Vec::new(),
                nullable: false,
            };
            for alternative in alternatives {
                let alternative = first_for_sites(&alternative.sites, grammar, facts);
                merge_positions(&mut result.positions, &alternative.positions);
                result.nullable |= alternative.nullable;
            }
            result
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
            FirstFacts {
                positions,
                nullable: false,
            }
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
    FirstFacts {
        positions,
        nullable,
    }
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
