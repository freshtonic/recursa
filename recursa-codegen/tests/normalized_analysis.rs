use recursa_codegen::lr::{Automaton, AutomatonOptions, LrGrammar, NoPrecedence, TypeRoles, lower};
use recursa_codegen::{
    Alternative, AtomDescription, AtomExpression, ContainerShape, Declaration, GrammarDescription,
    KindDescription, KindId, Location, ParseSite, RepetitionCardinality, SourceId, TypeDescription,
    TypeId, TypeShape,
};

fn lower_grammar(grammar: &GrammarDescription) -> LrGrammar {
    let roles = TypeRoles {
        start_types: grammar.entry_types.clone(),
        ..TypeRoles::default()
    };
    lower(grammar, &roles).expect("fixture lowers to an LR grammar")
}

fn automaton(grammar: &LrGrammar) -> Automaton {
    Automaton::build(grammar, &NoPrecedence, AutomatonOptions::default())
}

fn start_lookahead_names(
    grammar: &LrGrammar,
    automaton: &Automaton,
    type_id: TypeId,
) -> Vec<String> {
    let start = automaton
        .start_state(type_id)
        .expect("fixture type has a start state");
    automaton.states()[start.index()]
        .actions()
        .keys()
        .filter(|terminal| **terminal != grammar.end_marker())
        .map(|terminal| grammar.terminal_name(*terminal).to_owned())
        .collect()
}

fn type_nullable(grammar: &LrGrammar, name: &str) -> bool {
    let type_id = grammar
        .nonterminal_by_name(name)
        .unwrap_or_else(|| panic!("fixture has a {name} nonterminal"));
    grammar.nullable_nonterminals()[type_id.index()]
}

fn representative_grammar() -> GrammarDescription {
    let mut grammar = GrammarDescription::new(
        "representative",
        2,
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
fn normalized_grammar_lowers_to_the_expected_lr_entry_language() {
    let grammar = lower_grammar(&representative_grammar());
    let automaton = automaton(&grammar);

    assert!(automaton.conflicts().is_empty());
    assert_eq!(
        start_lookahead_names(&grammar, &automaton, TypeId::new(2)),
        ["Item", "Comma"],
    );
    assert!(!type_nullable(&grammar, "List"));
}

#[test]
fn first_fixed_point_converges_for_a_recursive_normalized_grammar() {
    let mut normalized = recursive_grammar();
    normalized.entry_types.push(TypeId::new(3));
    let grammar = lower_grammar(&normalized);
    let automaton = automaton(&grammar);

    assert!(automaton.conflicts().is_empty());
    assert_eq!(
        start_lookahead_names(&grammar, &automaton, TypeId::new(2)),
        ["Leaf", "Comma"],
    );
    assert_eq!(
        start_lookahead_names(&grammar, &automaton, TypeId::new(3)),
        ["Comma"],
    );
    assert!(!type_nullable(&grammar, "Node"));
    assert!(!type_nullable(&grammar, "Branch"));
}

#[test]
fn text_predicates_are_rejected_at_the_lr_lowering_seam() {
    let mut grammar = representative_grammar();
    let predicate =
        grammar.push_text_predicate(recursa_codegen::TextPredicate::Exact("number".to_owned()));
    let TypeShape::Token { text_predicate, .. } = &mut grammar.types[0].shape else {
        panic!("fixture item is a token type");
    };
    *text_predicate = Some(predicate);
    let roles = TypeRoles {
        start_types: grammar.entry_types.clone(),
        ..TypeRoles::default()
    };

    let diagnostics = lower(&grammar, &roles).unwrap_err();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "RCA0403");
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

    let lowered = lower_grammar(&grammar);

    assert_eq!(grammar.sites[0].container, shape);
    assert!(type_nullable(&lowered, "List"));
    assert!(automaton(&lowered).conflicts().is_empty());
}

#[test]
fn every_supported_container_cardinality_reaches_lr_nullability_analysis() {
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

        let lowered = lower_grammar(&grammar);

        assert_eq!(grammar.sites[0].container, shape);
        assert_eq!(type_nullable(&lowered, "List"), expected_nullable);
        assert_eq!(
            start_lookahead_names(&lowered, &automaton(&lowered), TypeId::new(2)),
            ["Item"],
        );
    }
}
