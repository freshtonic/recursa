use recursa_codegen::{
    Alternative, AtomDescription, AtomExpression, ContainerShape, Declaration, DispatchKind,
    FollowRecipe, GrammarDescription, KindDescription, Location, ParseSite, SourceId,
    TypeDescription, TypeShape, analyze,
};

fn declaration(grammar: &mut GrammarDescription, name: &str, byte: u32) -> recursa_codegen::DeclId {
    let location = grammar.push_location(Location::new(SourceId::new(0), byte..byte + 1));
    grammar.push_declaration(Declaration::new(name, location))
}

fn grammar_with_decisions(reverse_alternatives: bool) -> GrammarDescription {
    let mut grammar = GrammarDescription::new(
        "decisions",
        3,
        2,
        recursa_codegen::TextEquivalence::Sensitive,
    );
    let mut tokens = Vec::new();
    for (index, name) in ["A", "B", "C"].into_iter().enumerate() {
        let declaration = declaration(&mut grammar, name, index as u32);
        let kind = grammar.push_kind(KindDescription::new(name, declaration));
        let atom = grammar.push_atom(AtomDescription::new(
            name,
            declaration,
            AtomExpression::Kind(kind),
        ));
        tokens.push(grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom,
                text_predicate: None,
            },
        )));
    }

    let mixed_declaration = declaration(&mut grammar, "Mixed", 10);
    let mixed = grammar.next_type_id();
    let optional_a_declaration = declaration(&mut grammar, "Mixed::a", 11);
    let optional_a = grammar.push_site(ParseSite::new(
        mixed,
        tokens[0],
        optional_a_declaration,
        ContainerShape::optional(ContainerShape::Direct),
    ));
    let optional_b_declaration = declaration(&mut grammar, "Mixed::b", 12);
    let optional_b = grammar.push_site(ParseSite::new(
        mixed,
        tokens[1],
        optional_b_declaration,
        ContainerShape::optional(ContainerShape::Direct),
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "Mixed",
            mixed_declaration,
            TypeShape::Sequence {
                sites: vec![optional_a, optional_b],
            },
        )),
        mixed,
    );

    let static_declaration = declaration(&mut grammar, "Static", 20);
    let static_owner = grammar.next_type_id();
    let static_optional_declaration = declaration(&mut grammar, "Static::a", 21);
    let static_optional = grammar.push_site(ParseSite::new(
        static_owner,
        tokens[0],
        static_optional_declaration,
        ContainerShape::optional(ContainerShape::Direct),
    ));
    let required_c_declaration = declaration(&mut grammar, "Static::c", 22);
    let required_c = grammar.push_site(ParseSite::new(
        static_owner,
        tokens[2],
        required_c_declaration,
        ContainerShape::Direct,
    ));
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "Static",
            static_declaration,
            TypeShape::Sequence {
                sites: vec![static_optional, required_c],
            },
        )),
        static_owner,
    );

    let choice_declaration = declaration(&mut grammar, "Choice", 30);
    let choice = grammar.next_type_id();
    let left_declaration = declaration(&mut grammar, "Choice::Left", 31);
    let left_site = grammar.push_site(ParseSite::new(
        choice,
        tokens[0],
        left_declaration,
        ContainerShape::Direct,
    ));
    let left_tail_declaration = declaration(&mut grammar, "Choice::Left::tail", 33);
    let left_tail = grammar.push_site(ParseSite::new(
        choice,
        tokens[2],
        left_tail_declaration,
        ContainerShape::Direct,
    ));
    let right_declaration = declaration(&mut grammar, "Choice::Right", 32);
    let right_site = grammar.push_site(ParseSite::new(
        choice,
        tokens[1],
        right_declaration,
        ContainerShape::Direct,
    ));
    let right_tail_declaration = declaration(&mut grammar, "Choice::Right::tail", 34);
    let right_tail = grammar.push_site(ParseSite::new(
        choice,
        tokens[2],
        right_tail_declaration,
        ContainerShape::Direct,
    ));
    let mut alternatives = vec![
        Alternative {
            declaration: left_declaration,
            sites: vec![left_site, left_tail],
        },
        Alternative {
            declaration: right_declaration,
            sites: vec![right_site, right_tail],
        },
    ];
    if reverse_alternatives {
        alternatives.reverse();
    }
    assert_eq!(
        grammar.push_type(TypeDescription::new(
            "Choice",
            choice_declaration,
            TypeShape::Enum { alternatives },
        )),
        choice,
    );
    grammar.entry_types = vec![mixed, static_owner, choice];
    grammar
}

#[test]
fn analysis_freezes_all_follow_recipes_and_permutation_invariant_dispatch() {
    let grammar = grammar_with_decisions(false);
    let output = analyze(&grammar).output.expect("decision grammar is valid");
    let recipes = output.decisions().follow_recipes_by_site();
    assert!(matches!(recipes[0], FollowRecipe::StaticAndCaller { .. }));
    assert!(matches!(recipes[1], FollowRecipe::Caller));
    assert!(matches!(recipes[2], FollowRecipe::Static { .. }));

    let canonical = output.decisions().canonical_facts(&grammar);
    assert!(canonical.contains("dispatch Choice enum"));
    assert!(canonical.contains("follow Mixed::a static+caller"));
    assert!(canonical.contains("follow Mixed::b caller"));
    assert!(canonical.contains("follow Static::a static"));

    let enum_dispatch = output
        .decisions()
        .dispatches()
        .iter()
        .find(|dispatch| dispatch.kind() == DispatchKind::Enum)
        .expect("the enum dispatch is frozen");
    assert_eq!(enum_dispatch.trie().recovery_commitment(), None);
    assert!(
        enum_dispatch
            .trie()
            .edges()
            .iter()
            .all(|edge| edge.next().recovery_commitment().is_some())
    );
    assert!(
        enum_dispatch.trie().edges().iter().all(|edge| {
            edge.next().edges().iter().all(|leaf| {
                leaf.next().edges().is_empty() && leaf.next().recovery_commitment().is_some()
            })
        }),
        "a unique consumed leaf remains a frozen recovery commitment",
    );

    let reversed = grammar_with_decisions(true);
    let reversed_output = analyze(&reversed)
        .output
        .expect("permuted decision grammar is valid");
    assert_eq!(
        output
            .decisions()
            .dispatches()
            .iter()
            .filter(|dispatch| dispatch.kind() == DispatchKind::Enum)
            .map(|dispatch| dispatch.canonical_facts(&grammar))
            .collect::<Vec<_>>(),
        reversed_output
            .decisions()
            .dispatches()
            .iter()
            .filter(|dispatch| dispatch.kind() == DispatchKind::Enum)
            .map(|dispatch| dispatch.canonical_facts(&reversed))
            .collect::<Vec<_>>(),
    );
}
