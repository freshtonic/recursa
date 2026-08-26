use recursa_codegen::{
    Alternative, AtomDescription, AtomExpression, ContainerShape, DeclId, Declaration,
    DiagnosticCode, GrammarDescription, KindDescription, Location, ParseSite, PrattPosition,
    PrattVariant, Remedy, Severity, SourceId, TypeDescription, TypeId, TypeShape, analyze,
};

fn declaration(grammar: &mut GrammarDescription, name: &str, start: u32) -> DeclId {
    let location = grammar.push_location(Location::new(SourceId::new(0), start..start + 1));
    grammar.push_declaration(Declaration::new(name, location))
}

fn token(grammar: &mut GrammarDescription, type_name: &str, kind_name: &str, start: u32) -> TypeId {
    let declaration = declaration(grammar, type_name, start);
    let kind = grammar.push_kind(KindDescription::new(kind_name, declaration));
    let atom = grammar.push_atom(AtomDescription::new(
        kind_name,
        declaration,
        AtomExpression::Kind(kind),
    ));
    grammar.push_type(TypeDescription::new(
        type_name,
        declaration,
        TypeShape::Token {
            atom,
            text_predicate: None,
        },
    ))
}

fn left_recursive_through_box() -> GrammarDescription {
    let mut grammar = GrammarDescription::new(
        "left-recursive",
        2,
        1,
        recursa_codegen::TextEquivalence::AsciiInsensitive,
    );
    let plus = token(&mut grammar, "PlusTok", "Plus", 0);
    let literal = token(&mut grammar, "LitTok", "Lit", 2);
    let expr = TypeId::new(2);
    let add = TypeId::new(3);
    let expr_decl = declaration(&mut grammar, "E", 10);
    let add_decl = declaration(&mut grammar, "AddVariant", 20);
    let add_variant = declaration(&mut grammar, "Add", 30);
    let literal_variant = declaration(&mut grammar, "Lit", 32);
    let lhs_decl = declaration(&mut grammar, "lhs", 40);
    let op_decl = declaration(&mut grammar, "op", 42);
    let rhs_decl = declaration(&mut grammar, "rhs", 44);

    let add_site = grammar.push_site(ParseSite::new(
        expr,
        add,
        add_variant,
        ContainerShape::Direct,
    ));
    let literal_site = grammar.push_site(ParseSite::new(
        expr,
        literal,
        literal_variant,
        ContainerShape::Direct,
    ));
    let lhs = grammar.push_site(ParseSite::new(
        add,
        expr,
        lhs_decl,
        ContainerShape::boxed(ContainerShape::Direct),
    ));
    let op = grammar.push_site(ParseSite::new(add, plus, op_decl, ContainerShape::Direct));
    let rhs = grammar.push_site(ParseSite::new(
        add,
        expr,
        rhs_decl,
        ContainerShape::boxed(ContainerShape::Direct),
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "E",
            expr_decl,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: add_variant,
                        sites: vec![add_site],
                    },
                    Alternative {
                        declaration: literal_variant,
                        sites: vec![literal_site],
                    },
                ],
            },
        )),
        expr,
    );
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "AddVariant",
            add_decl,
            TypeShape::Sequence {
                sites: vec![lhs, op, rhs],
            },
        )),
        add,
    );
    grammar.entry_types.push(expr);
    grammar
}

fn mutually_left_recursive() -> GrammarDescription {
    let mut grammar = GrammarDescription::new(
        "mutual",
        2,
        1,
        recursa_codegen::TextEquivalence::AsciiInsensitive,
    );
    let tok1 = token(&mut grammar, "Tok1", "Tok1", 0);
    let tok2 = token(&mut grammar, "Tok2", "Tok2", 2);
    let a = TypeId::new(2);
    let b_wrap = TypeId::new(3);
    let b = TypeId::new(4);
    let a_wrap = TypeId::new(5);
    let a_decl = declaration(&mut grammar, "A", 10);
    let b_wrap_decl = declaration(&mut grammar, "BWrap", 20);
    let b_decl = declaration(&mut grammar, "B", 30);
    let a_wrap_decl = declaration(&mut grammar, "AWrap", 40);
    let a_to_b = declaration(&mut grammar, "A::ViaB", 50);
    let b_wrap_to_b = declaration(&mut grammar, "BWrap.inner", 52);
    let b_to_a = declaration(&mut grammar, "B::ViaA", 54);
    let a_wrap_to_a = declaration(&mut grammar, "AWrap.inner", 56);

    let a_b_site = grammar.push_site(ParseSite::new(a, b_wrap, a_to_b, ContainerShape::Direct));
    let a_tok_site = grammar.push_site(ParseSite::new(a, tok1, a_decl, ContainerShape::Direct));
    let b_wrap_site = grammar.push_site(ParseSite::new(
        b_wrap,
        b,
        b_wrap_to_b,
        ContainerShape::Direct,
    ));
    let b_a_site = grammar.push_site(ParseSite::new(b, a_wrap, b_to_a, ContainerShape::Direct));
    let b_tok_site = grammar.push_site(ParseSite::new(b, tok2, b_decl, ContainerShape::Direct));
    let a_wrap_site = grammar.push_site(ParseSite::new(
        a_wrap,
        a,
        a_wrap_to_a,
        ContainerShape::Direct,
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "A",
            a_decl,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: a_to_b,
                        sites: vec![a_b_site],
                    },
                    Alternative {
                        declaration: a_decl,
                        sites: vec![a_tok_site],
                    },
                ],
            },
        )),
        a,
    );
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "BWrap",
            b_wrap_decl,
            TypeShape::Alias { site: b_wrap_site },
        )),
        b_wrap,
    );
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "B",
            b_decl,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: b_to_a,
                        sites: vec![b_a_site],
                    },
                    Alternative {
                        declaration: b_decl,
                        sites: vec![b_tok_site],
                    },
                ],
            },
        )),
        b,
    );
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "AWrap",
            a_wrap_decl,
            TypeShape::Alias { site: a_wrap_site },
        )),
        a_wrap,
    );
    grammar.entry_types.push(a);
    grammar
}

#[test]
fn direct_container_left_recursion_has_a_located_structured_snapshot() {
    let report = analyze(left_recursive_through_box());

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code, DiagnosticCode::LeftRecursion);
    assert_eq!(diagnostic.severity, Severity::Error);
    assert!(diagnostic.primary.location.is_some());
    assert_eq!(diagnostic.secondary.len(), 1);
    assert!(matches!(
        diagnostic.remedy,
        Some(Remedy::BreakLeftRecursion { ref cycle })
            if cycle == &[TypeId::new(2), TypeId::new(3)]
    ));
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0100]: types `E`, `AddVariant` form a left-recursive cycle\n",
            "  primary source[0] 30..31: `E` reaches `AddVariant` before consuming a token\n",
            "  secondary source[0] 40..41: `AddVariant` reaches `E` before consuming a token\n",
            "  remedy: express recursive branches as Pratt led variants or move recursion after a required token\n",
        ),
    );
}

#[test]
fn mutual_left_recursion_has_one_deterministic_cycle_snapshot() {
    let report = analyze(mutually_left_recursive());

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0100]: types `A`, `BWrap`, `B`, `AWrap` form a left-recursive cycle\n",
            "  primary source[0] 50..51: `A` reaches `BWrap` before consuming a token\n",
            "  secondary source[0] 52..53: `BWrap` reaches `B` before consuming a token\n",
            "  secondary source[0] 54..55: `B` reaches `AWrap` before consuming a token\n",
            "  secondary source[0] 56..57: `AWrap` reaches `A` before consuming a token\n",
            "  remedy: express recursive branches as Pratt led variants or move recursion after a required token\n",
        ),
    );
}

#[test]
fn pratt_led_recursion_is_excluded_but_prefix_recursion_is_not() {
    let led = recursive_pratt(PrattPosition::Infix { lbp: 10, rbp: 11 });
    let prefix = recursive_pratt(PrattPosition::Prefix { bp: 20 });
    let atom = recursive_pratt(PrattPosition::Atom);

    assert!(analyze(led).diagnostics.is_empty());
    let prefix_report = analyze(prefix);
    assert!(prefix_report.output.is_none());
    assert_eq!(
        prefix_report.diagnostics[0].code,
        DiagnosticCode::LeftRecursion
    );
    let atom_report = analyze(atom);
    assert!(atom_report.output.is_none());
    assert_eq!(
        atom_report.diagnostics[0].code,
        DiagnosticCode::LeftRecursion
    );
}

fn recursive_pratt(position: PrattPosition) -> GrammarDescription {
    let mut grammar = GrammarDescription::new(
        "pratt",
        1,
        1,
        recursa_codegen::TextEquivalence::AsciiInsensitive,
    );
    let atom = token(&mut grammar, "AtomTok", "Atom", 0);
    let expr = TypeId::new(1);
    let expr_decl = declaration(&mut grammar, "Expr", 10);
    let atom_decl = declaration(&mut grammar, "Expr::Atom", 20);
    let recursive_decl = declaration(&mut grammar, "Expr::Recursive", 30);
    let atom_site = grammar.push_site(ParseSite::new(
        expr,
        atom,
        atom_decl,
        ContainerShape::Direct,
    ));
    let recursive_site = grammar.push_site(ParseSite::new(
        expr,
        expr,
        recursive_decl,
        ContainerShape::boxed(ContainerShape::Direct),
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "Expr",
            expr_decl,
            TypeShape::Pratt {
                variants: vec![
                    PrattVariant {
                        declaration: atom_decl,
                        position: PrattPosition::Atom,
                        sites: vec![atom_site],
                    },
                    PrattVariant {
                        declaration: recursive_decl,
                        position,
                        sites: vec![recursive_site],
                    },
                ],
            },
        )),
        expr,
    );
    grammar.entry_types.push(expr);
    grammar
}

#[test]
fn invalid_references_are_reported_before_dependent_semantics() {
    let mut grammar = left_recursive_through_box();
    grammar.entry_types.insert(0, TypeId::new(99));

    let report = analyze(grammar);

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, DiagnosticCode::InvalidReference);
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "error[RCA0002]: entry list references missing type 99\n",
            "  primary <normalized input>: entry list references missing type 99\n",
            "  remedy: correct the normalized reference before semantic analysis\n",
        ),
    );
}

#[test]
fn independent_recursive_components_are_collected_in_source_order() {
    let mut grammar = GrammarDescription::new(
        "independent",
        0,
        1,
        recursa_codegen::TextEquivalence::AsciiInsensitive,
    );
    let first = TypeId::new(0);
    let second = TypeId::new(1);
    let first_decl = declaration(&mut grammar, "First", 10);
    let second_decl = declaration(&mut grammar, "Second", 20);
    let first_site = grammar.push_site(ParseSite::new(
        first,
        first,
        first_decl,
        ContainerShape::boxed(ContainerShape::Direct),
    ));
    let second_site = grammar.push_site(ParseSite::new(
        second,
        second,
        second_decl,
        ContainerShape::boxed(ContainerShape::Direct),
    ));
    grammar.push_type(TypeDescription::new(
        "First",
        first_decl,
        TypeShape::Alias { site: first_site },
    ));
    grammar.push_type(TypeDescription::new(
        "Second",
        second_decl,
        TypeShape::Alias { site: second_site },
    ));

    let first_run = analyze(grammar.clone());
    let second_run = analyze(grammar);

    assert_eq!(first_run.diagnostics, second_run.diagnostics);
    assert_eq!(first_run.diagnostics.len(), 2);
    assert_eq!(first_run.diagnostics[0].code, DiagnosticCode::LeftRecursion);
    assert_eq!(first_run.diagnostics[1].code, DiagnosticCode::LeftRecursion);
    assert!(first_run.diagnostics[0].message.contains("`First`"));
    assert!(first_run.diagnostics[1].message.contains("`Second`"));
}

#[test]
fn independent_structural_failures_are_collected_before_recursion_analysis() {
    let mut grammar = left_recursive_through_box();
    grammar.entry_types = vec![TypeId::new(99), TypeId::new(98)];

    let report = analyze(grammar);

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 2);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::InvalidReference)
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "entry list references missing type 99",
            "entry list references missing type 98",
        ],
    );
}

#[test]
fn independent_atom_membership_cycles_are_each_reported_once() {
    let mut grammar = GrammarDescription::new(
        "atom-cycles",
        0,
        1,
        recursa_codegen::TextEquivalence::AsciiInsensitive,
    );
    let a_decl = declaration(&mut grammar, "A", 10);
    let b_decl = declaration(&mut grammar, "B", 12);
    let c_decl = declaration(&mut grammar, "C", 20);
    let d_decl = declaration(&mut grammar, "D", 22);
    grammar.push_atom(AtomDescription::new(
        "A",
        a_decl,
        AtomExpression::Category(vec![recursa_codegen::AtomId::new(1)]),
    ));
    grammar.push_atom(AtomDescription::new(
        "B",
        b_decl,
        AtomExpression::Class(vec![recursa_codegen::AtomId::new(0)]),
    ));
    grammar.push_atom(AtomDescription::new(
        "C",
        c_decl,
        AtomExpression::Category(vec![recursa_codegen::AtomId::new(3)]),
    ));
    grammar.push_atom(AtomDescription::new(
        "D",
        d_decl,
        AtomExpression::Class(vec![recursa_codegen::AtomId::new(2)]),
    ));

    let first = analyze(grammar.clone());
    let repeated = analyze(grammar);

    assert!(first.output.is_none());
    assert_eq!(first.diagnostics, repeated.diagnostics);
    assert_eq!(first.diagnostics.len(), 2);
    assert_eq!(
        first
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            "atoms `A`, `B` form a membership cycle",
            "atoms `C`, `D` form a membership cycle",
        ],
    );
}
