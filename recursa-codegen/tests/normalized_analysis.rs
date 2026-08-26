use recursa_codegen::{
    Alternative, AtomDescription, AtomExpression, ContainerShape, Declaration, GrammarDescription,
    KindDescription, KindId, Location, LocationId, ParseSite, RepetitionCardinality, SourceId,
    TypeDescription, TypeId, TypeShape, analyze,
};

fn representative_grammar() -> GrammarDescription {
    let mut grammar = GrammarDescription::new(
        "representative",
        2,
        1,
        recursa_codegen::TextEquivalence::AsciiInsensitive,
    );

    let source = SourceId::new(0);
    let number_location = grammar.push_location(Location::new(source, 0..6));
    let comma_location = grammar.push_location(Location::new(source, 7..12));
    let item_location = grammar.push_location(Location::new(source, 13..17));
    let list_location = grammar.push_location(Location::new(source, 18..22));

    let number_decl = grammar.push_declaration(Declaration::new("NUMBER", number_location));
    let comma_decl = grammar.push_declaration(Declaration::new("COMMA", comma_location));
    let item_decl = grammar.push_declaration(Declaration::new("Item", item_location));
    let list_decl = grammar.push_declaration(Declaration::new("List", list_location));

    let number_kind = grammar.push_kind(KindDescription::new("NUMBER", number_decl));
    let comma_kind = grammar.push_kind(KindDescription::new("COMMA", comma_decl));
    assert_eq!(number_kind, KindId::new(0));
    assert_eq!(comma_kind, KindId::new(1));

    let number_atom = grammar.push_atom(AtomDescription::new(
        "NUMBER",
        number_decl,
        AtomExpression::Kind(number_kind),
    ));
    let comma_atom = grammar.push_atom(AtomDescription::new(
        "COMMA",
        comma_decl,
        AtomExpression::Kind(comma_kind),
    ));

    let item = grammar.push_type(TypeDescription::new(
        "Item",
        item_decl,
        TypeShape::Token {
            atom: number_atom,
            text_predicate: None,
        },
    ));
    let comma = grammar.push_type(TypeDescription::new(
        "Comma",
        comma_decl,
        TypeShape::Token {
            atom: comma_atom,
            text_predicate: None,
        },
    ));
    let list = grammar.next_type_id();
    let optional_item = grammar.push_site(ParseSite::new(
        list,
        item,
        item_decl,
        ContainerShape::optional(ContainerShape::Direct),
    ));
    let required_comma = grammar.push_site(ParseSite::new(
        list,
        comma,
        comma_decl,
        ContainerShape::Direct,
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "List",
            list_decl,
            TypeShape::Sequence {
                sites: vec![optional_item, required_comma],
            },
        )),
        list,
    );
    grammar.entry_types.push(list);

    grammar
}

fn recursive_grammar() -> GrammarDescription {
    let mut grammar = GrammarDescription::new(
        "recursive",
        2,
        1,
        recursa_codegen::TextEquivalence::AsciiInsensitive,
    );
    let source = SourceId::new(0);
    let location = grammar.push_location(Location::new(source, 0..1));
    let number_decl = grammar.push_declaration(Declaration::new("NUMBER", location));
    let comma_decl = grammar.push_declaration(Declaration::new("COMMA", location));
    let leaf_decl = grammar.push_declaration(Declaration::new("Leaf", location));
    let node_decl = grammar.push_declaration(Declaration::new("Node", location));
    let branch_decl = grammar.push_declaration(Declaration::new("Branch", location));

    let number = grammar.push_kind(KindDescription::new("NUMBER", number_decl));
    let comma = grammar.push_kind(KindDescription::new("COMMA", comma_decl));
    let number_atom = grammar.push_atom(AtomDescription::new(
        "NUMBER",
        number_decl,
        AtomExpression::Kind(number),
    ));
    let comma_atom = grammar.push_atom(AtomDescription::new(
        "COMMA",
        comma_decl,
        AtomExpression::Kind(comma),
    ));
    let leaf = grammar.push_type(TypeDescription::new(
        "Leaf",
        leaf_decl,
        TypeShape::Token {
            atom: number_atom,
            text_predicate: None,
        },
    ));
    let comma_token = grammar.push_type(TypeDescription::new(
        "Comma",
        comma_decl,
        TypeShape::Token {
            atom: comma_atom,
            text_predicate: None,
        },
    ));

    let node = grammar.next_type_id();
    let branch = recursa_codegen::TypeId::new(node.get() + 1);
    let leaf_site = grammar.push_site(ParseSite::new(
        node,
        leaf,
        leaf_decl,
        ContainerShape::Direct,
    ));
    let branch_site = grammar.push_site(ParseSite::new(
        node,
        branch,
        branch_decl,
        ContainerShape::Direct,
    ));
    let comma_site = grammar.push_site(ParseSite::new(
        branch,
        comma_token,
        comma_decl,
        ContainerShape::Direct,
    ));
    let recursive_site = grammar.push_site(ParseSite::new(
        branch,
        node,
        node_decl,
        ContainerShape::boxed(ContainerShape::Direct),
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "Node",
            node_decl,
            TypeShape::Enum {
                alternatives: vec![
                    Alternative {
                        declaration: leaf_decl,
                        sites: vec![leaf_site],
                    },
                    Alternative {
                        declaration: branch_decl,
                        sites: vec![branch_site],
                    },
                ],
            },
        )),
        node,
    );
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "Branch",
            branch_decl,
            TypeShape::Sequence {
                sites: vec![comma_site, recursive_site],
            },
        )),
        branch,
    );
    grammar.entry_types.push(node);
    grammar
}

#[test]
fn normalized_grammar_produces_first_and_nullable_facts() {
    let report = analyze(representative_grammar());

    assert!(
        report.diagnostics.is_empty(),
        "unexpected diagnostics: {:#?}",
        report.diagnostics,
    );
    let output = report.output.expect("a clean grammar has semantic output");
    let item = output.first(recursa_codegen::TypeId::new(0)).unwrap();
    let list = output.first(recursa_codegen::TypeId::new(2)).unwrap();

    assert_eq!(item.positions.len(), 1);
    assert_eq!(
        item.positions[0].kinds.iter().collect::<Vec<_>>(),
        [KindId::new(0)]
    );
    assert!(!item.nullable);
    assert_eq!(list.positions.len(), 1);
    assert_eq!(
        list.positions[0].kinds.iter().collect::<Vec<_>>(),
        [KindId::new(0), KindId::new(1)]
    );
    assert!(!list.nullable);
}

#[test]
fn invalid_normalized_reference_returns_diagnostics_without_output() {
    let mut grammar = representative_grammar();
    grammar.entry_types.push(recursa_codegen::TypeId::new(99));

    let report = analyze(grammar);

    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code.as_str(), "RCA0002");
}

#[test]
fn first_fixed_point_converges_for_a_recursive_normalized_grammar() {
    let report = analyze(recursive_grammar());

    assert!(report.diagnostics.is_empty());
    let output = report.output.unwrap();
    let node = output.first(recursa_codegen::TypeId::new(2)).unwrap();
    let branch = output.first(recursa_codegen::TypeId::new(3)).unwrap();

    assert_eq!(
        node.positions[0].kinds.iter().collect::<Vec<_>>(),
        [KindId::new(0), KindId::new(1)],
    );
    assert_eq!(
        branch.positions[0].kinds.iter().collect::<Vec<_>>(),
        [KindId::new(1)],
    );
    assert!(!node.nullable);
    assert!(!branch.nullable);
}

#[test]
fn nullable_sites_and_text_predicates_survive_the_analysis_seam() {
    let mut grammar = representative_grammar();
    let predicate =
        grammar.push_text_predicate(recursa_codegen::TextPredicate::Exact("number".to_owned()));
    let TypeShape::Token { text_predicate, .. } = &mut grammar.types[0].shape else {
        panic!("fixture item is a token type");
    };
    *text_predicate = Some(predicate);
    let TypeShape::Sequence { sites } = &mut grammar.types[2].shape else {
        panic!("fixture list is a sequence type");
    };
    sites.truncate(1);

    let report = analyze(grammar);

    assert!(report.diagnostics.is_empty());
    let output = report.output.unwrap();
    let item = output.first(recursa_codegen::TypeId::new(0)).unwrap();
    let list = output.first(recursa_codegen::TypeId::new(2)).unwrap();
    assert_eq!(item.positions[0].text_predicate, Some(predicate));
    assert!(list.nullable);
}

#[test]
fn recursive_container_shape_preserves_box_placement_and_optional_vec1_cardinality() {
    let mut grammar = representative_grammar();
    let shape = ContainerShape::Optional(Box::new(ContainerShape::Boxed(Box::new(
        ContainerShape::Repeat {
            element: Box::new(ContainerShape::Boxed(Box::new(ContainerShape::Direct))),
            cardinality: RepetitionCardinality::OneOrMore,
            separator: None,
        },
    ))));
    grammar.sites[0].container = shape.clone();
    let TypeShape::Sequence { sites } = &mut grammar.types[2].shape else {
        panic!("fixture list is a sequence type");
    };
    sites.truncate(1);

    let report = analyze(grammar.clone());

    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == recursa_codegen::Severity::Warning),
        "unexpected diagnostics: {:#?}",
        report.diagnostics,
    );
    assert_eq!(grammar.sites[0].container, shape);
    let list = report
        .output
        .unwrap()
        .first(TypeId::new(2))
        .unwrap()
        .clone();
    assert!(list.nullable);
    assert_eq!(
        list.positions[0].kinds.iter().collect::<Vec<_>>(),
        [KindId::new(0)],
    );
}

#[test]
fn unsupported_container_cardinality_is_rejected_before_analysis() {
    let invalid_shapes = [
        ContainerShape::optional(ContainerShape::optional(ContainerShape::Direct)),
        ContainerShape::optional(ContainerShape::zero_or_more(ContainerShape::Direct, None)),
        ContainerShape::zero_or_more(ContainerShape::optional(ContainerShape::Direct), None),
        ContainerShape::zero_or_more(
            ContainerShape::zero_or_more(ContainerShape::Direct, None),
            None,
        ),
        ContainerShape::one_or_more(
            ContainerShape::one_or_more(ContainerShape::Direct, None),
            None,
        ),
    ];

    for shape in invalid_shapes {
        let mut grammar = representative_grammar();
        grammar.sites[0].container = shape;

        let report = analyze(grammar);

        assert!(report.output.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code.as_str(), "RCA0001");
        assert_eq!(
            report.diagnostics[0].primary.location,
            Some(LocationId::new(2))
        );
    }
}

#[test]
fn every_supported_container_cardinality_reaches_first_and_nullable_analysis() {
    let cases = [
        (ContainerShape::Direct, false),
        (
            ContainerShape::boxed(ContainerShape::optional(ContainerShape::boxed(
                ContainerShape::Direct,
            ))),
            true,
        ),
        (
            ContainerShape::zero_or_more(ContainerShape::boxed(ContainerShape::Direct), None),
            true,
        ),
        (
            ContainerShape::boxed(ContainerShape::one_or_more(
                ContainerShape::boxed(ContainerShape::Direct),
                None,
            )),
            false,
        ),
        (
            ContainerShape::optional(ContainerShape::boxed(ContainerShape::one_or_more(
                ContainerShape::Direct,
                None,
            ))),
            true,
        ),
    ];

    for (shape, expected_nullable) in cases {
        let mut grammar = representative_grammar();
        grammar.sites[0].container = shape.clone();
        let TypeShape::Sequence { sites } = &mut grammar.types[2].shape else {
            panic!("fixture list is a sequence type");
        };
        sites.truncate(1);

        let report = analyze(grammar.clone());

        assert_eq!(grammar.sites[0].container, shape);
        let output = report
            .output
            .expect("supported container has semantic output");
        let first = output.first(TypeId::new(2)).unwrap();
        assert_eq!(first.nullable, expected_nullable);
        assert_eq!(
            first.positions[0].kinds.iter().collect::<Vec<_>>(),
            [KindId::new(0)],
        );
    }
}
