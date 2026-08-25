use std::collections::BTreeSet;

use recursa_codegen::{
    AtomDescription, AtomExpression, ConflictRemedy, Declaration, DiagnosticCode,
    GrammarDescription, KindDescription, KindId, Location, ParseSite, PrattPosition, PrattVariant,
    SiteContainer, SourceId, TextPredicate, TypeDescription, TypeId, TypeShape, analyze,
};

struct Builder {
    grammar: GrammarDescription,
    next_byte: u32,
    atoms: Vec<recursa_codegen::AtomId>,
}

impl Builder {
    fn new(name: &str, kinds: &[&str]) -> Self {
        let mut builder = Self {
            grammar: GrammarDescription::new(name, kinds.len(), 1),
            next_byte: 0,
            atoms: Vec::new(),
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
            builder.atoms.push(atom);
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

    fn token(&mut self, name: &str, kind: usize) -> TypeId {
        self.token_with_atom(name, self.atoms[kind], None)
    }

    fn token_with_atom(
        &mut self,
        name: &str,
        atom: recursa_codegen::AtomId,
        text_predicate: Option<recursa_codegen::TextPredicateId>,
    ) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom,
                text_predicate,
            },
        ))
    }

    fn pratt(&mut self, name: &str, variants: &[(&str, TypeId, PrattPosition)]) -> TypeId {
        let owner = self.grammar.next_type_id();
        let type_declaration = self.declaration(name);
        let variants = variants
            .iter()
            .map(|(name, target, position)| {
                let declaration = self.declaration(name);
                let operator = self.grammar.push_site(ParseSite::new(
                    owner,
                    *target,
                    declaration,
                    SiteContainer::Direct,
                ));
                let mut sites = vec![operator];
                if matches!(
                    position,
                    PrattPosition::Prefix { .. } | PrattPosition::Infix { .. }
                ) {
                    sites.push(self.grammar.push_site(ParseSite::new(
                        owner,
                        owner,
                        declaration,
                        SiteContainer::Boxed,
                    )));
                }
                PrattVariant {
                    declaration,
                    position: *position,
                    sites,
                }
            })
            .collect();
        assert_eq!(
            self.grammar.push_type(TypeDescription::new(
                name,
                type_declaration,
                TypeShape::Pratt { variants },
            )),
            owner,
        );
        owner
    }
}

fn atom_atom() -> GrammarDescription {
    let mut builder = Builder::new("pratt-atom", &["AmbiguousKind"]);
    let a = builder.token("TokA", 0);
    let b = builder.token("TokB", 0);
    builder.pratt(
        "E",
        &[("A", a, PrattPosition::Atom), ("B", b, PrattPosition::Atom)],
    );
    builder.grammar
}

fn atom_prefix() -> GrammarDescription {
    let mut builder = Builder::new("pratt-prefix", &["Shared", "Other"]);
    let shared = builder.token("SharedTok", 0);
    let shared_again = builder.token("SharedTokAgain", 0);
    let other = builder.token("OtherTok", 1);
    builder.pratt(
        "E",
        &[
            ("Lit", shared, PrattPosition::Atom),
            ("Neg", shared_again, PrattPosition::Prefix { bp: 30 }),
            ("Op", other, PrattPosition::Infix { lbp: 10, rbp: 11 }),
        ],
    );
    builder.grammar
}

fn led_led(postfix: bool) -> GrammarDescription {
    let mut builder = Builder::new("pratt-led", &["Atom", "OpShared"]);
    let atom = builder.token("AtomTok", 0);
    let left = builder.token(if postfix { "InfixOp" } else { "OpA" }, 1);
    let right = builder.token(if postfix { "PostfixOp" } else { "OpB" }, 1);
    builder.pratt(
        "E",
        &[
            ("Lit", atom, PrattPosition::Atom),
            ("Plus", left, PrattPosition::Infix { lbp: 10, rbp: 11 }),
            (
                if postfix { "Bang" } else { "Minus" },
                right,
                if postfix {
                    PrattPosition::Postfix { bp: 20 }
                } else {
                    PrattPosition::Infix { lbp: 10, rbp: 11 }
                },
            ),
        ],
    );
    builder.grammar
}

fn predicate_pratt(
    left_predicate: TextPredicate,
    right_predicate: TextPredicate,
    left_position: PrattPosition,
    right_position: PrattPosition,
) -> GrammarDescription {
    let mut builder = Builder::new("predicate-pratt", &["Word"]);
    let left_predicate = builder.grammar.push_text_predicate(left_predicate);
    let right_predicate = builder.grammar.push_text_predicate(right_predicate);
    let left = builder.token_with_atom("Left", builder.atoms[0], Some(left_predicate));
    let right = builder.token_with_atom("Right", builder.atoms[0], Some(right_predicate));
    builder.pratt(
        "E",
        &[
            ("Left", left, left_position),
            ("Right", right, right_position),
        ],
    );
    builder.grammar
}

#[test]
fn pratt_atom_atom_conflict_has_a_canonical_snapshot() {
    let report = analyze(atom_atom());

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, DiagnosticCode::PrattAtomOverlap);
    assert!(matches!(
        report.diagnostics[0].conflict_remedy,
        Some(ConflictRemedy::DisambiguatePrattVariants { .. })
    ));
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0202]: Pratt `E` has overlapping atom variants\n",
            "  primary source[0] 8..9: this atom variant can start with the witness kind\n",
            "  secondary source[0] 10..11: this atom variant can start with the same kind\n",
            "  overlap: {AmbiguousKind}\n",
            "  witness: AmbiguousKind\n",
            "  left proof:\n",
            "    - `E` references `TokA` at `A`\n",
            "    - FIRST propagates through `TokA`\n",
            "    - kind `AmbiguousKind` is declared by atom `AmbiguousKind`\n",
            "  right proof:\n",
            "    - `E` references `TokB` at `B`\n",
            "    - FIRST propagates through `TokB`\n",
            "    - kind `AmbiguousKind` is declared by atom `AmbiguousKind`\n",
            "  remedy: make the Pratt variants start with disjoint kinds or text predicates\n",
        ),
    );
}

#[test]
fn pratt_atom_prefix_conflict_has_a_canonical_snapshot() {
    let report = analyze(atom_prefix());

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, DiagnosticCode::PrattAtomOverlap);
    let witness = &report.diagnostics[0].conflict.as_ref().unwrap().witness;
    for proof in [witness.left, witness.right] {
        assert!(report.proofs().chain(proof).all(|node| !matches!(
            node.step,
            recursa_codegen::ProvenanceStep::PrattExtender { .. }
        )));
    }
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0202]: Pratt `E` has overlapping atom and prefix variants\n",
            "  primary source[0] 12..13: this atom variant can start with the witness kind\n",
            "  secondary source[0] 14..15: this prefix variant can start with the same kind\n",
            "  overlap: {Shared}\n",
            "  witness: Shared\n",
            "  left proof:\n",
            "    - `E` references `SharedTok` at `Lit`\n",
            "    - FIRST propagates through `SharedTok`\n",
            "    - kind `Shared` is declared by atom `Shared`\n",
            "  right proof:\n",
            "    - `E` references `SharedTokAgain` at `Neg`\n",
            "    - FIRST propagates through `SharedTokAgain`\n",
            "    - kind `Shared` is declared by atom `Shared`\n",
            "  remedy: make the Pratt variants start with disjoint kinds or text predicates\n",
        ),
    );
}

#[test]
fn pratt_led_led_conflict_has_a_canonical_snapshot() {
    let report = analyze(led_led(false));

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, DiagnosticCode::PrattLedOverlap);
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0203]: Pratt `E` has overlapping led variants\n",
            "  primary source[0] 14..15: this infix variant can start with the witness kind\n",
            "  secondary source[0] 16..17: this infix variant can start with the same kind\n",
            "  overlap: {OpShared}\n",
            "  witness: OpShared\n",
            "  left proof:\n",
            "    - Pratt `E` admits infix extender `Plus` with lbp 10 and rbp 11\n",
            "    - `E` references `OpA` at `Plus`\n",
            "    - FIRST propagates through `OpA`\n",
            "    - kind `OpShared` is declared by atom `OpShared`\n",
            "  right proof:\n",
            "    - Pratt `E` admits infix extender `Minus` with lbp 10 and rbp 11\n",
            "    - `E` references `OpB` at `Minus`\n",
            "    - FIRST propagates through `OpB`\n",
            "    - kind `OpShared` is declared by atom `OpShared`\n",
            "  remedy: make the Pratt variants start with disjoint kinds or text predicates\n",
        ),
    );
}

#[test]
fn pratt_infix_postfix_conflict_has_a_canonical_snapshot() {
    let grammar = led_led(true);
    let pratt = TypeId::new((grammar.types.len() - 1) as u32);
    let pratt_declaration = grammar.types[pratt.index()].declaration;
    let TypeShape::Pratt { variants } = &grammar.types[pratt.index()].shape else {
        unreachable!()
    };
    let expected = variants
        .iter()
        .filter(|variant| {
            matches!(
                variant.position,
                PrattPosition::Infix { .. } | PrattPosition::Postfix { .. }
            )
        })
        .map(|variant| (variant.declaration, variant.position))
        .collect::<Vec<_>>();
    let report = analyze(grammar);

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, DiagnosticCode::PrattLedOverlap);
    let witness = &report.diagnostics[0].conflict.as_ref().unwrap().witness;
    for (proof, (variant, position)) in [witness.left, witness.right].into_iter().zip(expected) {
        assert!(report.proofs().chain(proof).any(|node| matches!(
            node.step,
            recursa_codegen::ProvenanceStep::PrattExtender {
                pratt: actual_pratt,
                pratt_declaration: actual_pratt_declaration,
                variant: actual_variant,
                position: actual_position,
            } if actual_pratt == pratt
                && actual_pratt_declaration == pratt_declaration
                && actual_variant == variant
                && actual_position == position
        )));
    }
    let mut reachable = BTreeSet::from([witness.left, witness.right]);
    let mut pending = reachable.iter().copied().collect::<Vec<_>>();
    while let Some(proof) = pending.pop() {
        if let Some(parent) = report.proofs().get(proof).unwrap().parent
            && reachable.insert(parent)
        {
            pending.push(parent);
        }
    }
    assert_eq!(reachable.len(), report.proofs().nodes().len());
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0203]: Pratt `E` has overlapping infix and postfix variants\n",
            "  primary source[0] 14..15: this infix variant can start with the witness kind\n",
            "  secondary source[0] 16..17: this postfix variant can start with the same kind\n",
            "  overlap: {OpShared}\n",
            "  witness: OpShared\n",
            "  left proof:\n",
            "    - Pratt `E` admits infix extender `Plus` with lbp 10 and rbp 11\n",
            "    - `E` references `InfixOp` at `Plus`\n",
            "    - FIRST propagates through `InfixOp`\n",
            "    - kind `OpShared` is declared by atom `OpShared`\n",
            "  right proof:\n",
            "    - Pratt `E` admits postfix extender `Bang` with bp 20\n",
            "    - `E` references `PostfixOp` at `Bang`\n",
            "    - FIRST propagates through `PostfixOp`\n",
            "    - kind `OpShared` is declared by atom `OpShared`\n",
            "  remedy: make the Pratt variants start with disjoint kinds or text predicates\n",
        ),
    );
}

#[test]
fn pratt_conflicts_use_complete_symbolic_intersections_and_text_refinements() {
    let mut builder = Builder::new("symbolic-pratt", &["Left", "SharedA", "SharedB", "Right"]);
    let left_atom = builder.atom(
        "LEFT_SET",
        AtomExpression::Category(vec![builder.atoms[0], builder.atoms[1], builder.atoms[2]]),
    );
    let right_atom = builder.atom(
        "RIGHT_SET",
        AtomExpression::Category(vec![builder.atoms[1], builder.atoms[2], builder.atoms[3]]),
    );
    let left = builder.token_with_atom("LeftTok", left_atom, None);
    let right = builder.token_with_atom("RightTok", right_atom, None);
    builder.pratt(
        "E",
        &[
            ("Left", left, PrattPosition::Atom),
            ("Right", right, PrattPosition::Atom),
        ],
    );
    let report = analyze(builder.grammar);
    let conflict = report.diagnostics[0].conflict.as_ref().unwrap();
    assert_eq!(
        conflict.overlap.iter().collect::<Vec<_>>(),
        [KindId::new(1), KindId::new(2)]
    );

    let mut refined = Builder::new("refined-pratt", &["Word"]);
    let select = refined
        .grammar
        .push_text_predicate(TextPredicate::Exact("SELECT".into()));
    let from = refined
        .grammar
        .push_text_predicate(TextPredicate::Exact("FROM".into()));
    let select = refined.token_with_atom("Select", refined.atoms[0], Some(select));
    let from = refined.token_with_atom("From", refined.atoms[0], Some(from));
    refined.pratt(
        "E",
        &[
            ("Select", select, PrattPosition::Atom),
            ("From", from, PrattPosition::Atom),
        ],
    );
    assert!(analyze(refined.grammar).diagnostics.is_empty());
}

#[test]
fn pratt_nud_and_led_predicate_matrices_respect_refinements() {
    let disjoint_nud = predicate_pratt(
        TextPredicate::Exact("SELECT".into()),
        TextPredicate::Exact("FROM".into()),
        PrattPosition::Atom,
        PrattPosition::Prefix { bp: 30 },
    );
    let overlapping_nud = predicate_pratt(
        TextPredicate::Exact("SELECT".into()),
        TextPredicate::OneOf(vec!["SELECT".into(), "FROM".into()]),
        PrattPosition::Atom,
        PrattPosition::Prefix { bp: 30 },
    );
    let disjoint_led = predicate_pratt(
        TextPredicate::Exact("AND".into()),
        TextPredicate::Exact("OR".into()),
        PrattPosition::Infix { lbp: 10, rbp: 11 },
        PrattPosition::Postfix { bp: 20 },
    );
    let overlapping_led = predicate_pratt(
        TextPredicate::Exact("AND".into()),
        TextPredicate::NotIn(vec!["OR".into()]),
        PrattPosition::Infix { lbp: 10, rbp: 11 },
        PrattPosition::Postfix { bp: 20 },
    );

    assert!(analyze(disjoint_nud).diagnostics.is_empty());
    assert_eq!(
        analyze(overlapping_nud).diagnostics[0].code,
        DiagnosticCode::PrattAtomOverlap
    );
    assert!(analyze(disjoint_led).diagnostics.is_empty());
    assert_eq!(
        analyze(overlapping_led).diagnostics[0].code,
        DiagnosticCode::PrattLedOverlap
    );
}

#[test]
fn prefix_prefix_and_postfix_postfix_use_their_pratt_conflict_families() {
    let prefix = predicate_pratt(
        TextPredicate::Any,
        TextPredicate::Any,
        PrattPosition::Prefix { bp: 20 },
        PrattPosition::Prefix { bp: 30 },
    );
    let postfix = predicate_pratt(
        TextPredicate::Any,
        TextPredicate::Any,
        PrattPosition::Postfix { bp: 20 },
        PrattPosition::Postfix { bp: 30 },
    );

    assert_eq!(
        analyze(prefix).diagnostics[0].code,
        DiagnosticCode::PrattAtomOverlap
    );
    assert_eq!(
        analyze(postfix).diagnostics[0].code,
        DiagnosticCode::PrattLedOverlap
    );
}

#[test]
fn independent_pratt_conflicts_are_collected_in_source_order_and_deterministically() {
    let mut builder = Builder::new("multiple-pratt", &["K"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 0);
    builder.pratt(
        "First",
        &[
            ("First::A", a, PrattPosition::Atom),
            ("First::B", b, PrattPosition::Atom),
        ],
    );
    builder.pratt(
        "Second",
        &[
            ("Second::A", a, PrattPosition::Atom),
            ("Second::B", b, PrattPosition::Atom),
        ],
    );
    let mut canonical = builder.grammar;
    for (type_index, start) in [(2, 100), (3, 20)] {
        let TypeShape::Pratt { variants } = &canonical.types[type_index].shape else {
            panic!("fixture type is Pratt");
        };
        for (offset, variant) in variants.iter().enumerate() {
            let location = canonical.declarations[variant.declaration.index()].location;
            canonical.locations[location.index()].range =
                start + offset as u32 * 2..start + offset as u32 * 2 + 1;
        }
    }
    let mut reordered = canonical.clone();
    for description in &mut reordered.types {
        if let TypeShape::Pratt { variants } = &mut description.shape {
            variants.reverse();
        }
    }
    let first = analyze(canonical);
    let repeated = analyze(reordered);

    assert_eq!(first.diagnostics, repeated.diagnostics);
    assert_eq!(first.proofs(), repeated.proofs());
    assert_eq!(
        first.canonical_diagnostics(),
        repeated.canonical_diagnostics()
    );
    assert_eq!(first.diagnostics.len(), 2);
    assert!(first.diagnostics[0].message.contains("`Second`"));
    assert!(first.diagnostics[1].message.contains("`First`"));
}
