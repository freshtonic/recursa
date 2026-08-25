use recursa_codegen::{
    Alternative, AtomDescription, AtomExpression, ConflictRemedy, Declaration, DiagnosticCode,
    GrammarDescription, KindDescription, KindId, KindSet, Location, ParseSite, ProvenanceStep,
    SiteContainer, SourceId, TextPredicate, TypeDescription, TypeId, TypeShape, analyze,
};

struct Builder {
    grammar: GrammarDescription,
    next_byte: u32,
    direct_atoms: Vec<recursa_codegen::AtomId>,
}

impl Builder {
    fn new(name: &str, kinds: &[&str]) -> Self {
        let mut builder = Self {
            grammar: GrammarDescription::new(name, kinds.len(), 1),
            next_byte: 0,
            direct_atoms: Vec::new(),
        };
        for &name in kinds {
            let declaration = builder.declaration(name);
            let kind = builder
                .grammar
                .push_kind(KindDescription::new(name, declaration));
            let atom = builder.grammar.push_atom(AtomDescription::new(
                name,
                declaration,
                AtomExpression::Kind(kind),
            ));
            builder.direct_atoms.push(atom);
        }
        builder
    }

    fn declaration(&mut self, name: &str) -> recursa_codegen::DeclId {
        let start = self.next_byte;
        self.next_byte += 2;
        let location = self
            .grammar
            .push_location(Location::new(SourceId::new(0), start..start + 1));
        self.grammar
            .push_declaration(Declaration::new(name, location))
    }

    fn atom(&mut self, name: &str, expression: AtomExpression) -> recursa_codegen::AtomId {
        let declaration = self.declaration(name);
        self.grammar
            .push_atom(AtomDescription::new(name, declaration, expression))
    }

    fn token(
        &mut self,
        name: &str,
        atom: recursa_codegen::AtomId,
        predicate: Option<recursa_codegen::TextPredicateId>,
    ) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom,
                text_predicate: predicate,
            },
        ))
    }

    fn choice(&mut self, name: &str, alternatives: &[TypeId]) -> TypeId {
        let owner = self.grammar.next_type_id();
        let type_declaration = self.declaration(name);
        let alternatives = alternatives
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let declaration = self.declaration(&format!("{name}::{index}"));
                let site = self.grammar.push_site(ParseSite::new(
                    owner,
                    *target,
                    declaration,
                    SiteContainer::Direct,
                ));
                Alternative {
                    declaration,
                    sites: vec![site],
                }
            })
            .collect();
        assert_eq!(
            self.grammar.push_type(TypeDescription::new(
                name,
                type_declaration,
                TypeShape::Enum { alternatives },
            )),
            owner,
        );
        owner
    }
}

fn target_keyword_collision(reverse_members: bool) -> GrammarDescription {
    let mut builder = Builder::new("target-keyword", &["Word", "Format", "Json"]);
    let mut category_members = vec![builder.direct_atoms[1], builder.direct_atoms[2]];
    if reverse_members {
        category_members.reverse();
    }
    let category = builder.atom("UNRESERVED", AtomExpression::Category(category_members));
    let class = builder.atom("bare_label", AtomExpression::Class(vec![category]));
    let target = builder.atom(
        "ColId",
        AtomExpression::Target {
            base: builder.direct_atoms[0],
            admits: vec![class],
        },
    );
    let target_type = builder.token("ColId", target, None);
    let keyword_type = builder.token("FORMAT", builder.direct_atoms[1], None);
    let root = builder.choice("Choice", &[target_type, keyword_type]);
    builder.grammar.entry_types.push(root);
    builder.grammar
}

fn partially_overlapping_targets() -> GrammarDescription {
    let mut builder = Builder::new(
        "partial-targets",
        &["BaseA", "BaseB", "SharedA", "SharedB", "Left", "Right"],
    );
    let shared = builder.atom(
        "SHARED",
        AtomExpression::Category(vec![builder.direct_atoms[2], builder.direct_atoms[3]]),
    );
    let left = builder.atom(
        "LEFT",
        AtomExpression::Category(vec![shared, builder.direct_atoms[4]]),
    );
    let right = builder.atom(
        "RIGHT",
        AtomExpression::Category(vec![shared, builder.direct_atoms[5]]),
    );
    let target_a = builder.atom(
        "TargetA",
        AtomExpression::Target {
            base: builder.direct_atoms[0],
            admits: vec![left],
        },
    );
    let target_b = builder.atom(
        "TargetB",
        AtomExpression::Target {
            base: builder.direct_atoms[1],
            admits: vec![right],
        },
    );
    let a = builder.token("TargetA", target_a, None);
    let b = builder.token("TargetB", target_b, None);
    builder.choice("Choice", &[a, b]);
    builder.grammar
}

fn refined_or_disjoint(disjoint: bool) -> GrammarDescription {
    let mut builder = Builder::new("refined", &["Word", "Other"]);
    let select = builder
        .grammar
        .push_text_predicate(TextPredicate::Exact("SELECT".to_owned()));
    let from = builder
        .grammar
        .push_text_predicate(TextPredicate::Exact("FROM".to_owned()));
    let left = builder.token("Left", builder.direct_atoms[0], Some(select));
    let right_atom = if disjoint {
        builder.direct_atoms[1]
    } else {
        builder.direct_atoms[0]
    };
    let right = builder.token("Right", right_atom, Some(from));
    builder.choice("Choice", &[left, right]);
    builder.grammar
}

fn nullable_tail(required_tail: bool) -> GrammarDescription {
    let mut builder = Builder::new("nullable-tail", &["Token"]);
    let token = builder.token("Token", builder.direct_atoms[0], None);
    let owner = builder.grammar.next_type_id();
    let owner_decl = builder.declaration("Container");
    let first_decl = builder.declaration("first");
    let tail_decl = builder.declaration("tail");
    let first = builder.grammar.push_site(ParseSite::new(
        owner,
        token,
        first_decl,
        SiteContainer::Optional,
    ));
    let tail = builder.grammar.push_site(ParseSite::new(
        owner,
        token,
        tail_decl,
        if required_tail {
            SiteContainer::Direct
        } else {
            SiteContainer::Optional
        },
    ));
    builder.grammar.push_type(TypeDescription::new(
        "Container",
        owner_decl,
        TypeShape::Sequence {
            sites: vec![first, tail],
        },
    ));
    builder.grammar
}

fn disjoint_targets() -> GrammarDescription {
    let mut builder = Builder::new(
        "disjoint-targets",
        &["BaseA", "BaseB", "AdmittedA", "AdmittedB"],
    );
    let left = builder.atom(
        "LEFT",
        AtomExpression::Category(vec![builder.direct_atoms[2]]),
    );
    let right = builder.atom(
        "RIGHT",
        AtomExpression::Category(vec![builder.direct_atoms[3]]),
    );
    let target_a = builder.atom(
        "TargetA",
        AtomExpression::Target {
            base: builder.direct_atoms[0],
            admits: vec![left],
        },
    );
    let target_b = builder.atom(
        "TargetB",
        AtomExpression::Target {
            base: builder.direct_atoms[1],
            admits: vec![right],
        },
    );
    let a = builder.token("TargetA", target_a, None);
    let b = builder.token("TargetB", target_b, None);
    builder.choice("Choice", &[a, b]);
    builder.grammar
}

fn nullable_run(fully_nullable: bool) -> GrammarDescription {
    let mut builder = Builder::new("nullable-run", &["K0", "K1", "K2"]);
    let a_atom = builder.atom(
        "A",
        AtomExpression::Category(vec![builder.direct_atoms[0], builder.direct_atoms[1]]),
    );
    let b_atom = builder.atom(
        "B",
        AtomExpression::Category(vec![builder.direct_atoms[0], builder.direct_atoms[2]]),
    );
    let c_atom = builder.atom(
        "C",
        AtomExpression::Category(vec![builder.direct_atoms[1], builder.direct_atoms[2]]),
    );
    let a = builder.token("A", a_atom, None);
    let b = builder.token("B", b_atom, None);
    let c = builder.token("C", c_atom, None);
    let owner = builder.grammar.next_type_id();
    let owner_decl = builder.declaration("Run");
    let a_decl = builder.declaration("a");
    let b_decl = builder.declaration("b");
    let c_decl = builder.declaration("c");
    let a = builder
        .grammar
        .push_site(ParseSite::new(owner, a, a_decl, SiteContainer::Optional));
    let b = builder
        .grammar
        .push_site(ParseSite::new(owner, b, b_decl, SiteContainer::Optional));
    let c = builder.grammar.push_site(ParseSite::new(
        owner,
        c,
        c_decl,
        if fully_nullable {
            SiteContainer::Optional
        } else {
            SiteContainer::Direct
        },
    ));
    builder.grammar.push_type(TypeDescription::new(
        "Run",
        owner_decl,
        TypeShape::Sequence {
            sites: vec![a, b, c],
        },
    ));
    builder.grammar
}

fn predicate_pair(left: Option<TextPredicate>, right: Option<TextPredicate>) -> GrammarDescription {
    let mut builder = Builder::new("predicates", &["Word"]);
    let left = left.map(|predicate| builder.grammar.push_text_predicate(predicate));
    let right = right.map(|predicate| builder.grammar.push_text_predicate(predicate));
    let left = builder.token("Left", builder.direct_atoms[0], left);
    let right = builder.token("Right", builder.direct_atoms[0], right);
    builder.choice("Choice", &[left, right]);
    builder.grammar
}

fn delayed_shorter_proof() -> (GrammarDescription, TypeId, TypeId) {
    let mut builder = Builder::new("shortest-proof", &["Kind"]);
    let category = builder.atom(
        "category",
        AtomExpression::Category(vec![builder.direct_atoms[0]]),
    );
    let class = builder.atom("class", AtomExpression::Class(vec![category]));
    let category_two = builder.atom("category_two", AtomExpression::Category(vec![class]));
    let class_two = builder.atom("class_two", AtomExpression::Class(vec![category_two]));
    let long_atom = builder.atom(
        "long_target",
        AtomExpression::Target {
            base: class_two,
            admits: Vec::new(),
        },
    );
    let long = builder.token("Long", long_atom, None);

    let owner = builder.grammar.next_type_id();
    let short_alias = TypeId::new(owner.get() + 1);
    let short_token = TypeId::new(owner.get() + 2);
    let owner_decl = builder.declaration("Expr");
    let long_decl = builder.declaration("Expr::Long");
    let short_decl = builder.declaration("Expr::Short");
    let long_site = builder.grammar.push_site(ParseSite::new(
        owner,
        long,
        long_decl,
        SiteContainer::Optional,
    ));
    let short_site = builder.grammar.push_site(ParseSite::new(
        owner,
        short_alias,
        short_decl,
        SiteContainer::Optional,
    ));
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Expr",
            owner_decl,
            TypeShape::Sequence {
                sites: vec![long_site, short_site],
            },
        )),
        owner,
    );
    let alias_decl = builder.declaration("ShortAlias");
    let alias_site_decl = builder.declaration("ShortAlias::inner");
    let alias_site = builder.grammar.push_site(ParseSite::new(
        short_alias,
        short_token,
        alias_site_decl,
        SiteContainer::Direct,
    ));
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "ShortAlias",
            alias_decl,
            TypeShape::Alias { site: alias_site },
        )),
        short_alias,
    );
    assert_eq!(
        builder.token("Short", builder.direct_atoms[0], None),
        short_token,
    );
    (builder.grammar, owner, short_alias)
}

fn singleton_enum_overlap() -> GrammarDescription {
    let mut builder = Builder::new("singleton-enum", &["K"]);
    let left = builder.token("Left", builder.direct_atoms[0], None);
    let right = builder.token("Right", builder.direct_atoms[0], None);
    builder.choice("Choice", &[left, right]);
    builder.grammar
}

fn source_ranked_members(equal_source: bool) -> (GrammarDescription, recursa_codegen::AtomId) {
    let mut grammar = GrammarDescription::new("proof-ranking", 1, 1);
    let source = SourceId::new(0);
    let kind_location = grammar.push_location(Location::new(source, 0..1));
    let first_location = grammar.push_location(Location::new(
        source,
        if equal_source { 10..11 } else { 100..101 },
    ));
    let second_location = grammar.push_location(Location::new(source, 10..11));
    let category_location = grammar.push_location(Location::new(source, 200..201));
    let token_location = grammar.push_location(Location::new(source, 300..301));
    let kind_decl = grammar.push_declaration(Declaration::new("K", kind_location));
    let inserted_first =
        grammar.push_declaration(Declaration::new("InsertedFirst", first_location));
    let inserted_second =
        grammar.push_declaration(Declaration::new("InsertedSecond", second_location));
    let category_decl = grammar.push_declaration(Declaration::new("Both", category_location));
    let token_decl = grammar.push_declaration(Declaration::new("Token", token_location));
    let kind = grammar.push_kind(KindDescription::new("K", kind_decl));

    let (first_atom_decl, second_atom_decl) = if equal_source {
        (inserted_second, inserted_first)
    } else {
        (inserted_first, inserted_second)
    };
    let first_atom = grammar.push_atom(AtomDescription::new(
        "FirstAtom",
        first_atom_decl,
        AtomExpression::Kind(kind),
    ));
    let second_atom = grammar.push_atom(AtomDescription::new(
        "SecondAtom",
        second_atom_decl,
        AtomExpression::Kind(kind),
    ));
    let both = grammar.push_atom(AtomDescription::new(
        "Both",
        category_decl,
        AtomExpression::Category(vec![second_atom, first_atom]),
    ));
    grammar.push_type(TypeDescription::new(
        "Token",
        token_decl,
        TypeShape::Token {
            atom: both,
            text_predicate: None,
        },
    ));

    let expected = if equal_source {
        first_atom
    } else {
        second_atom
    };
    (grammar, expected)
}

#[test]
fn target_keyword_overlap_retains_symbolic_proof_chains() {
    let report = analyze(target_keyword_collision(false));

    assert!(report.output.is_none());
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code, DiagnosticCode::EnumFirstOverlap);
    let conflict = diagnostic.conflict.as_ref().unwrap();
    assert_eq!(
        conflict.overlap.iter().collect::<Vec<_>>(),
        [KindId::new(1)]
    );
    assert_eq!(conflict.witness.kind, KindId::new(1));
    let left_steps = report
        .proofs()
        .chain(conflict.witness.left)
        .map(|node| &node.step)
        .collect::<Vec<_>>();
    assert!(
        left_steps
            .iter()
            .any(|step| matches!(step, ProvenanceStep::CategoryMembership { .. }))
    );
    assert!(
        left_steps
            .iter()
            .any(|step| matches!(step, ProvenanceStep::ClassMembership { .. }))
    );
    assert!(
        left_steps
            .iter()
            .any(|step| matches!(step, ProvenanceStep::TargetAdmission { .. }))
    );
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RestructureSharedPrefix { .. })
    ));
    assert!(report.canonical_diagnostics().contains("FORMAT"));
    assert!(report.canonical_diagnostics().contains("UNRESERVED"));
    assert!(report.canonical_diagnostics().contains("ColId"));
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0200]: enum `Choice` has alternatives with an unresolved shared prefix\n",
            "  primary source[0] 18..19: this alternative can start with the witness kind\n",
            "  secondary source[0] 20..21: this alternative can start with the same kind\n",
            "  overlap: {Format}\n",
            "  witness: Format\n",
            "  left proof:\n",
            "    - `Choice` references `ColId` at `Choice::0`\n",
            "    - FIRST propagates through `ColId`\n",
            "    - target `ColId` admits it\n",
            "    - class `bare_label` includes it\n",
            "    - category `UNRESERVED` includes it\n",
            "    - kind `Format` is declared by atom `Format`\n",
            "  right proof:\n",
            "    - `Choice` references `FORMAT` at `Choice::1`\n",
            "    - FIRST propagates through `FORMAT`\n",
            "    - kind `Format` is declared by atom `Format`\n",
            "  shared prefix: Format\n",
            "  lookahead: no finite depth separates this shared prefix\n",
            "  remedy: restructure the alternatives into a shared-prefix type\n",
        ),
    );
}

#[test]
fn partial_target_intersection_is_complete_and_has_a_canonical_witness() {
    let report = analyze(partially_overlapping_targets());
    let conflict = report.diagnostics[0].conflict.as_ref().unwrap();

    assert_eq!(
        conflict.overlap.iter().collect::<Vec<_>>(),
        [KindId::new(2), KindId::new(3)],
    );
    assert_eq!(conflict.witness.kind, KindId::new(2));
}

#[test]
fn disjoint_kinds_and_refined_text_predicates_do_not_conflict() {
    assert!(analyze(refined_or_disjoint(true)).diagnostics.is_empty());
    assert!(analyze(refined_or_disjoint(false)).diagnostics.is_empty());
}

#[test]
fn static_nullable_overlap_is_an_error_but_a_fully_nullable_tail_is_not() {
    let report = analyze(nullable_tail(true));

    assert!(report.output.is_none());
    assert_eq!(
        report.diagnostics[0].code,
        DiagnosticCode::StaticUseSiteOverlap
    );
    let conflict = report.diagnostics[0].conflict.as_ref().unwrap();
    assert_eq!(
        conflict.overlap.iter().collect::<Vec<_>>(),
        [KindId::new(0)]
    );
    assert!(matches!(
        report.diagnostics[0].conflict_remedy,
        Some(ConflictRemedy::RaiseMaxLookahead {
            configured: 1,
            sufficient: 2,
        })
    ));
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0201]: nullable use site in `Container` overlaps its required tail\n",
            "  primary source[0] 6..7: this use site is nullable and can consume the witness kind\n",
            "  secondary source[0] 8..9: the following required tail can start with the same kind\n",
            "  overlap: {Token}\n",
            "  witness: Token\n",
            "  left proof:\n",
            "    - `Container` references `Token` at `first`\n",
            "    - FIRST propagates through `Token`\n",
            "    - kind `Token` is declared by atom `Token`\n",
            "  right proof:\n",
            "    - `Container` references `Token` at `tail`\n",
            "    - FIRST propagates through `Token`\n",
            "    - kind `Token` is declared by atom `Token`\n",
            "  shared prefix: Token\n",
            "  lookahead: configured depth 1 is exhausted; depth 2 disambiguates\n",
            "  remedy: raise max_lookahead from 1 to 2\n",
        ),
    );
    let fully_nullable = analyze(nullable_tail(false));
    assert!(fully_nullable.output.is_some());
    assert!(
        fully_nullable
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == recursa_codegen::Severity::Warning)
    );
}

#[test]
fn provenance_and_witnesses_are_deterministic_under_member_reordering() {
    let canonical = analyze(target_keyword_collision(false));
    let reordered = analyze(target_keyword_collision(true));

    assert_eq!(canonical.diagnostics, reordered.diagnostics);
    assert_eq!(canonical.proofs(), reordered.proofs());
    assert_eq!(
        canonical.canonical_diagnostics(),
        reordered.canonical_diagnostics()
    );
}

#[test]
fn every_first_membership_has_exactly_one_proof() {
    let report = analyze(refined_or_disjoint(true));
    let output = report.output.unwrap();

    for provenanced in output.first_provenance_by_type().iter().flatten() {
        for index in 0..provenanced.kinds().kind_count() {
            let kind = KindId::new(index as u16);
            assert_eq!(
                provenanced.kinds().contains(kind),
                provenanced.proof(kind).is_some()
            );
        }
    }
}

#[test]
fn static_nullable_runs_check_each_required_boundary_and_skip_fully_nullable_tails() {
    let report = analyze(nullable_run(false));

    assert_eq!(report.diagnostics.len(), 2);
    assert_eq!(
        report.diagnostics[0]
            .conflict
            .as_ref()
            .unwrap()
            .overlap
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(0), KindId::new(1)],
    );
    assert_eq!(
        report.diagnostics[1]
            .conflict
            .as_ref()
            .unwrap()
            .overlap
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(2)],
    );
    assert!(
        report
            .proofs()
            .nodes()
            .iter()
            .any(|node| matches!(node.step, ProvenanceStep::NullablePrefix { .. }))
    );
    let fully_nullable = analyze(nullable_run(true));
    assert!(fully_nullable.output.is_some());
    assert!(
        fully_nullable
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == recursa_codegen::Severity::Warning)
    );
}

#[test]
fn text_predicate_overlap_matrix_refines_shared_kinds() {
    let cases = [
        (None, Some(TextPredicate::Exact("a".into())), true),
        (
            Some(TextPredicate::Any),
            Some(TextPredicate::Exact("a".into())),
            true,
        ),
        (
            Some(TextPredicate::Exact("SELECT".into())),
            Some(TextPredicate::Exact("select".into())),
            true,
        ),
        (
            Some(TextPredicate::Exact("SELECT".into())),
            Some(TextPredicate::Exact("FROM".into())),
            false,
        ),
        (
            Some(TextPredicate::Exact("a".into())),
            Some(TextPredicate::OneOf(vec!["a".into(), "b".into()])),
            true,
        ),
        (
            Some(TextPredicate::Exact("a".into())),
            Some(TextPredicate::NotIn(vec!["a".into()])),
            false,
        ),
        (
            Some(TextPredicate::Exact("a".into())),
            Some(TextPredicate::NotIn(vec!["b".into()])),
            true,
        ),
        (
            Some(TextPredicate::OneOf(vec!["a".into()])),
            Some(TextPredicate::OneOf(vec!["b".into()])),
            false,
        ),
        (
            Some(TextPredicate::OneOf(vec!["a".into(), "b".into()])),
            Some(TextPredicate::OneOf(vec!["b".into(), "c".into()])),
            true,
        ),
        (
            Some(TextPredicate::OneOf(vec!["a".into()])),
            Some(TextPredicate::NotIn(vec!["a".into()])),
            false,
        ),
        (
            Some(TextPredicate::OneOf(vec!["a".into(), "b".into()])),
            Some(TextPredicate::NotIn(vec!["a".into()])),
            true,
        ),
        (
            Some(TextPredicate::NotIn(vec!["a".into()])),
            Some(TextPredicate::NotIn(vec!["b".into()])),
            true,
        ),
    ];

    for (left, right, expected_overlap) in cases {
        let report = analyze(predicate_pair(left, right));
        assert_eq!(
            !report.diagnostics.is_empty(),
            expected_overlap,
            "unexpected predicate overlap result",
        );
    }
}

#[test]
fn disjoint_targets_are_accepted_and_text_predicates_have_proofs() {
    assert!(analyze(disjoint_targets()).diagnostics.is_empty());

    let report = analyze(refined_or_disjoint(false));
    let output = report.output.as_ref().unwrap();
    let proof = output.first_provenance_by_type()[0][0]
        .proof(KindId::new(0))
        .unwrap();
    let steps = report
        .proofs()
        .chain(proof)
        .map(|node| &node.step)
        .collect::<Vec<_>>();
    assert!(
        steps
            .iter()
            .any(|step| matches!(step, ProvenanceStep::TextPredicate { .. }))
    );
    assert!(
        steps
            .iter()
            .any(|step| matches!(step, ProvenanceStep::FirstPropagation { .. }))
    );
    assert!(matches!(
        steps.last(),
        Some(ProvenanceStep::DirectDeclaration { .. })
    ));
}

#[test]
fn enum_alternative_and_category_member_order_do_not_change_results() {
    let canonical = analyze(target_keyword_collision(false));
    let mut reordered_grammar = target_keyword_collision(true);
    let TypeShape::Enum { alternatives } = &mut reordered_grammar.types[2].shape else {
        panic!("fixture root is an enum");
    };
    alternatives.reverse();
    let reordered = analyze(reordered_grammar);

    assert_eq!(canonical.diagnostics, reordered.diagnostics);
    assert_eq!(canonical.proofs(), reordered.proofs());
    assert_eq!(
        canonical.canonical_diagnostics(),
        reordered.canonical_diagnostics()
    );
}

#[test]
fn a_shorter_proof_that_arrives_later_replaces_the_first_path() {
    let (grammar, owner, short_alias) = delayed_shorter_proof();
    let report = analyze(grammar);
    let output = report.output.as_ref().unwrap();
    let proof = output.first_provenance_by_type()[owner.index()][0]
        .proof(KindId::new(0))
        .unwrap();
    let steps = report
        .proofs()
        .chain(proof)
        .map(|node| &node.step)
        .collect::<Vec<_>>();

    assert!(steps.iter().any(|step| matches!(
        step,
        ProvenanceStep::Reference { target, .. } if *target == short_alias
    )));
    assert!(
        !steps
            .iter()
            .any(|step| matches!(step, ProvenanceStep::TargetAdmission { .. }))
    );
}

#[test]
fn partial_overlap_witnesses_end_at_the_canonical_direct_declarations() {
    let report = analyze(partially_overlapping_targets());
    let witness = &report.diagnostics[0].conflict.as_ref().unwrap().witness;

    assert!(matches!(
        report.proofs().chain(witness.left).last().unwrap().step,
        ProvenanceStep::DirectDeclaration {
            kind,
            ..
        } if kind == KindId::new(2)
    ));
    assert!(matches!(
        report.proofs().chain(witness.right).last().unwrap().step,
        ProvenanceStep::DirectDeclaration {
            kind,
            ..
        } if kind == KindId::new(2)
    ));
}

#[test]
fn singleton_enum_overlap_matches_the_spike_scalar_baseline() {
    let report = analyze(singleton_enum_overlap());
    let diagnostic = &report.diagnostics[0];
    let conflict = diagnostic.conflict.as_ref().unwrap();

    assert_eq!(diagnostic.code, DiagnosticCode::EnumFirstOverlap);
    assert_eq!(
        conflict.overlap,
        KindSet::singleton(1, KindId::new(0)).unwrap(),
    );
    assert_eq!(conflict.witness.kind, KindId::new(0));
    assert!(matches!(
        diagnostic.conflict_remedy,
        Some(ConflictRemedy::RestructureSharedPrefix { .. })
    ));
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0200]: enum `Choice` has alternatives with an unresolved shared prefix\n",
            "  primary source[0] 8..9: this alternative can start with the witness kind\n",
            "  secondary source[0] 10..11: this alternative can start with the same kind\n",
            "  overlap: {K}\n",
            "  witness: K\n",
            "  left proof:\n",
            "    - `Choice` references `Left` at `Choice::0`\n",
            "    - FIRST propagates through `Left`\n",
            "    - kind `K` is declared by atom `K`\n",
            "  right proof:\n",
            "    - `Choice` references `Right` at `Choice::1`\n",
            "    - FIRST propagates through `Right`\n",
            "    - kind `K` is declared by atom `K`\n",
            "  shared prefix: K\n",
            "  lookahead: no finite depth separates this shared prefix\n",
            "  remedy: restructure the alternatives into a shared-prefix type\n",
        ),
    );
}

#[test]
fn equal_hop_proofs_prefer_physical_source_order_over_insertion_order() {
    let (grammar, expected_atom) = source_ranked_members(false);
    let report = analyze(grammar);
    let proof = report.output.as_ref().unwrap().first_provenance_by_type()[0][0]
        .proof(KindId::new(0))
        .unwrap();

    assert!(matches!(
        report.proofs().chain(proof).last().unwrap().step,
        ProvenanceStep::DirectDeclaration { atom, .. } if atom == expected_atom
    ));
}

#[test]
fn equal_hop_equal_source_proofs_use_typed_identity_as_the_final_tie_break() {
    let (grammar, expected_atom) = source_ranked_members(true);
    let report = analyze(grammar);
    let proof = report.output.as_ref().unwrap().first_provenance_by_type()[0][0]
        .proof(KindId::new(0))
        .unwrap();

    assert!(matches!(
        report.proofs().chain(proof).last().unwrap().step,
        ProvenanceStep::DirectDeclaration { atom, .. } if atom == expected_atom
    ));
}
