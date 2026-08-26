use recursa_codegen::{
    Alternative, AtomDescription, AtomExpression, ConflictRemedy, ContainerShape, Declaration,
    DiagnosticCode, GrammarDescription, KindDescription, KindId, Location, ParseSite,
    PrattPosition, PrattVariant, Severity, SourceId, TextPredicate, TypeDescription, TypeId,
    TypeShape, analyze,
};

struct Builder {
    grammar: GrammarDescription,
    next_byte: u32,
    atoms: Vec<recursa_codegen::AtomId>,
}

impl Builder {
    fn new(name: &str, kinds: &[&str]) -> Self {
        let mut builder = Self {
            grammar: GrammarDescription::new(
                name,
                kinds.len(),
                1,
                recursa_codegen::TextEquivalence::AsciiInsensitive,
            ),
            next_byte: 0,
            atoms: Vec::new(),
        };
        for &name in kinds {
            let declaration = builder.declaration(name);
            let kind = builder
                .grammar
                .push_kind(KindDescription::new(name, declaration));
            builder
                .atoms
                .push(builder.grammar.push_atom(AtomDescription::new(
                    name,
                    declaration,
                    AtomExpression::Kind(kind),
                )));
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

    fn token(&mut self, name: &str, kind: usize) -> TypeId {
        self.token_with_predicate(name, kind, None)
    }

    fn token_from_atom(&mut self, name: &str, atom: recursa_codegen::AtomId) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom,
                text_predicate: None,
            },
        ))
    }

    fn category(&mut self, name: &str, members: &[usize]) -> recursa_codegen::AtomId {
        let declaration = self.declaration(name);
        self.grammar.push_atom(AtomDescription::new(
            name,
            declaration,
            AtomExpression::Category(members.iter().map(|index| self.atoms[*index]).collect()),
        ))
    }

    fn token_with_predicate(
        &mut self,
        name: &str,
        kind: usize,
        predicate: Option<recursa_codegen::TextPredicateId>,
    ) -> TypeId {
        let declaration = self.declaration(name);
        self.grammar.push_type(TypeDescription::new(
            name,
            declaration,
            TypeShape::Token {
                atom: self.atoms[kind],
                text_predicate: predicate,
            },
        ))
    }

    fn site(
        &mut self,
        owner: TypeId,
        target: TypeId,
        name: &str,
        container: ContainerShape,
    ) -> recursa_codegen::SiteId {
        let declaration = self.declaration(name);
        self.grammar
            .push_site(ParseSite::new(owner, target, declaration, container))
    }
}

fn nested_nullable_follow(predicate_match: bool) -> (GrammarDescription, recursa_codegen::SiteId) {
    let mut builder = Builder::new("nested-follow", &["Word"]);
    let select = builder
        .grammar
        .push_text_predicate(TextPredicate::Exact("SELECT".into()));
    let following = builder.grammar.push_text_predicate(TextPredicate::Exact(
        if predicate_match { "SELECT" } else { "FROM" }.into(),
    ));
    let maybe = builder.token_with_predicate("Maybe", 0, Some(select));
    let after = builder.token_with_predicate("After", 0, Some(following));

    let inner = builder.grammar.next_type_id();
    let inner_decl = builder.declaration("Inner");
    let maybe_site = builder.site(
        inner,
        maybe,
        "maybe",
        ContainerShape::optional(ContainerShape::Direct),
    );
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Inner",
            inner_decl,
            TypeShape::Sequence {
                sites: vec![maybe_site],
            },
        )),
        inner,
    );

    let root = builder.grammar.next_type_id();
    let root_decl = builder.declaration("Root");
    let inner_site = builder.site(
        root,
        inner,
        "inner",
        ContainerShape::zero_or_more(ContainerShape::Direct, Some(after)),
    );
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "Root",
            root_decl,
            TypeShape::Sequence {
                sites: vec![inner_site],
            },
        )),
        root,
    );
    builder.grammar.entry_types.push(root);
    (builder.grammar, maybe_site)
}

#[test]
fn empty_and_multiple_roots_seed_eof_separately_and_deterministically() {
    let mut builder = Builder::new("roots", &["A", "B", "Unused"]);
    let a = builder.token("A", 0);
    let b = builder.token("B", 1);
    let unused = builder.token("Unused", 2);
    let unreachable = builder.grammar.next_type_id();
    let unreachable_decl = builder.declaration("Unreachable");
    let unreachable_a = builder.site(unreachable, a, "unreachable_a", ContainerShape::Direct);
    let unreachable_b = builder.site(unreachable, b, "unreachable_b", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "Unreachable",
        unreachable_decl,
        TypeShape::Sequence {
            sites: vec![unreachable_a, unreachable_b],
        },
    ));

    let empty = analyze(builder.grammar.clone());
    let empty_output = empty.output.as_ref().unwrap();
    assert!(
        empty_output
            .follow_by_type()
            .iter()
            .all(|facts| { facts.kinds().is_empty() && !facts.eof() })
    );
    assert!(
        empty_output
            .follow_by_site()
            .iter()
            .all(|facts| { facts.kinds().is_empty() && !facts.eof() })
    );

    builder.grammar.entry_types = vec![a, b];
    let canonical = analyze(builder.grammar.clone());
    builder.grammar.entry_types.reverse();
    let shuffled = analyze(builder.grammar);
    let output = canonical.output.as_ref().unwrap();
    assert!(output.follow(a).unwrap().eof());
    assert!(output.follow(b).unwrap().eof());
    assert!(!output.follow(unused).unwrap().eof());
    assert!(output.follow(unreachable).unwrap().kinds().is_empty());
    assert!(
        output
            .follow_site(unreachable_a)
            .unwrap()
            .kinds()
            .is_empty()
    );
    assert!(output.follow(a).unwrap().kinds().is_empty());
    assert!(output.follow(a).unwrap().eof_proof().is_some());
    assert_eq!(canonical.output, shuffled.output);
    assert_eq!(canonical.proofs(), shuffled.proofs());
}

#[test]
fn nested_nullable_tail_warning_is_predicate_aware_and_keeps_output() {
    let (grammar, maybe_site) = nested_nullable_follow(true);
    let report = analyze(grammar);

    assert!(report.output.is_some());
    assert_eq!(report.diagnostics.len(), 2);
    let warning = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            matches!(
                diagnostic.conflict_remedy,
                Some(ConflictRemedy::ReviewNullableFollow { site }) if site == maybe_site
            )
        })
        .unwrap();
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.code, DiagnosticCode::NullableFollowOverlap);
    assert!(matches!(
        warning.conflict_remedy,
        Some(ConflictRemedy::ReviewNullableFollow { site }) if site == maybe_site
    ));
    assert_eq!(
        warning
            .conflict
            .as_ref()
            .unwrap()
            .overlap
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(0)]
    );
    assert_eq!(
        report.canonical_diagnostics(),
        concat!(
            "warning[RCA0300]: nullable use site in `Inner` may consume a caller FOLLOW kind\n",
            "  primary source[0] 8..9: this nullable use site can consume the witness kind\n",
            "  secondary source[0] 12..13: the same kind can follow this use site\n",
            "  overlap: {Word}\n",
            "  witness: Word\n",
            "  left proof:\n",
            "    - FIRST propagates through `Maybe`\n",
            "    - text is refined by Exact(\"SELECT\")\n",
            "    - kind `Word` is declared by atom `Word`\n",
            "  right proof:\n",
            "    - FOLLOW of `Inner` propagates to `Maybe` at `maybe`\n",
            "    - repetition in `Root` continues after `inner`\n",
            "    - FIRST propagates through `After`\n",
            "    - text is refined by Exact(\"SELECT\")\n",
            "    - kind `Word` is declared by atom `Word`\n",
            "  remedy: make the use site required or separate it from its possible caller FOLLOW\n",
            "warning[RCA0300]: nullable use site in `Root` may consume a caller FOLLOW kind\n",
            "  primary source[0] 12..13: this nullable use site can consume the witness kind\n",
            "  secondary source[0] 4..5: the same kind can follow this use site\n",
            "  overlap: {Word}\n",
            "  witness: Word\n",
            "  left proof:\n",
            "    - FIRST propagates through `Inner`\n",
            "    - `Inner` references `Maybe` at `maybe`\n",
            "    - FIRST propagates through `Maybe`\n",
            "    - text is refined by Exact(\"SELECT\")\n",
            "    - kind `Word` is declared by atom `Word`\n",
            "  right proof:\n",
            "    - repetition in `Root` continues after `inner`\n",
            "    - FIRST propagates through `After`\n",
            "    - text is refined by Exact(\"SELECT\")\n",
            "    - kind `Word` is declared by atom `Word`\n",
            "  remedy: make the use site required or separate it from its possible caller FOLLOW\n",
        )
    );

    let (refined, _) = nested_nullable_follow(false);
    assert!(analyze(refined).diagnostics.is_empty());
}

#[test]
fn containers_separators_variants_and_nullable_sccs_propagate_follow() {
    let mut builder = Builder::new("containers", &["Item", "Sep", "End", "Alt"]);
    let item = builder.token("Item", 0);
    let separator = builder.token("Separator", 1);
    let end = builder.token("End", 2);
    let alt = builder.token("Alt", 3);

    let list = builder.grammar.next_type_id();
    let list_decl = builder.declaration("List");
    let repeat = builder.site(
        list,
        item,
        "items",
        ContainerShape::optional(ContainerShape::boxed(ContainerShape::one_or_more(
            ContainerShape::boxed(ContainerShape::Direct),
            Some(separator),
        ))),
    );
    builder.grammar.push_type(TypeDescription::new(
        "List",
        list_decl,
        TypeShape::Sequence {
            sites: vec![repeat],
        },
    ));

    let choice = builder.grammar.next_type_id();
    let choice_decl = builder.declaration("Choice");
    let list_variant_decl = builder.declaration("Choice::List");
    let alt_variant_decl = builder.declaration("Choice::Alt");
    let list_variant = builder.grammar.push_site(ParseSite::new(
        choice,
        list,
        list_variant_decl,
        ContainerShape::boxed(ContainerShape::Direct),
    ));
    let alt_variant = builder.grammar.push_site(ParseSite::new(
        choice,
        alt,
        alt_variant_decl,
        ContainerShape::Direct,
    ));
    builder.grammar.push_type(TypeDescription::new(
        "Choice",
        choice_decl,
        TypeShape::Enum {
            alternatives: vec![
                Alternative {
                    declaration: list_variant_decl,
                    sites: vec![list_variant],
                },
                Alternative {
                    declaration: alt_variant_decl,
                    sites: vec![alt_variant],
                },
            ],
        },
    ));

    let root = builder.grammar.next_type_id();
    let root_decl = builder.declaration("Root");
    let choice_site = builder.site(root, choice, "choice", ContainerShape::Direct);
    let end_site = builder.site(root, end, "end", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "Root",
        root_decl,
        TypeShape::Sequence {
            sites: vec![choice_site, end_site],
        },
    ));
    builder.grammar.entry_types.push(root);

    let report = analyze(builder.grammar);
    let output = report.output.as_ref().unwrap();
    assert!(report.diagnostics.is_empty());
    assert_eq!(
        output
            .follow(item)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(1), KindId::new(2)]
    );
    assert_eq!(
        output
            .follow(separator)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(0)]
    );
    assert_eq!(
        output
            .follow_site(repeat)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(1), KindId::new(2)]
    );
    assert!(output.follow(end).unwrap().eof());
    assert!(output.follow(alt).unwrap().kinds().contains(KindId::new(2)));
}

#[test]
fn nullable_separator_closes_repeat_item_separator_and_outer_follow() {
    let mut builder = Builder::new("nullable-separator", &["Item", "Sep", "End"]);
    let item_token = builder.token("Item", 0);
    let separator_token = builder.token("Sep", 1);
    let end = builder.token("End", 2);

    let item = builder.grammar.next_type_id();
    let item_decl = builder.declaration("NullableItem");
    let optional_item = builder.site(
        item,
        item_token,
        "optional_item",
        ContainerShape::optional(ContainerShape::Direct),
    );
    builder.grammar.push_type(TypeDescription::new(
        "NullableItem",
        item_decl,
        TypeShape::Sequence {
            sites: vec![optional_item],
        },
    ));

    let separator = builder.grammar.next_type_id();
    let separator_decl = builder.declaration("Separator");
    let optional_separator = builder.site(
        separator,
        separator_token,
        "optional_separator",
        ContainerShape::optional(ContainerShape::Direct),
    );
    builder.grammar.push_type(TypeDescription::new(
        "Separator",
        separator_decl,
        TypeShape::Sequence {
            sites: vec![optional_separator],
        },
    ));

    let root = builder.grammar.next_type_id();
    let root_decl = builder.declaration("Root");
    let repeat = builder.site(
        root,
        item,
        "items",
        ContainerShape::zero_or_more(ContainerShape::Direct, Some(separator)),
    );
    let end_site = builder.site(root, end, "end", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "Root",
        root_decl,
        TypeShape::Sequence {
            sites: vec![repeat, end_site],
        },
    ));
    builder.grammar.entry_types.push(root);

    let report = analyze(builder.grammar);
    let output = report.output.as_ref().unwrap();
    let all = vec![KindId::new(0), KindId::new(1), KindId::new(2)];
    assert_eq!(
        output
            .follow(item_token)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        all
    );
    assert_eq!(
        output
            .follow(separator)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        all
    );
    assert_eq!(
        output
            .follow_site(repeat)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        all
    );
}

#[test]
fn recursive_follow_scc_propagates_eof_without_conflicting_with_it() {
    let mut builder = Builder::new("recursive-follow", &["Tok"]);
    let token = builder.token("Tok", 0);
    let node = builder.grammar.next_type_id();
    let node_decl = builder.declaration("Node");
    let token_site = builder.site(node, token, "token", ContainerShape::Direct);
    let recursive_site = builder.site(
        node,
        node,
        "next",
        ContainerShape::optional(ContainerShape::Direct),
    );
    builder.grammar.push_type(TypeDescription::new(
        "Node",
        node_decl,
        TypeShape::Sequence {
            sites: vec![token_site, recursive_site],
        },
    ));
    builder.grammar.entry_types.push(node);

    let report = analyze(builder.grammar);
    let output = report.output.as_ref().unwrap();
    assert!(report.diagnostics.is_empty());
    assert!(output.follow(node).unwrap().eof());
    assert!(output.follow_site(recursive_site).unwrap().eof());
    assert!(output.follow(token).unwrap().eof());
    assert!(
        output
            .follow(token)
            .unwrap()
            .kinds()
            .contains(KindId::new(0))
    );
}

#[test]
fn multi_node_follow_scc_converges_before_provenance() {
    let mut builder = Builder::new("multi-node-scc", &["A", "B"]);
    let a_token = builder.token("A", 0);
    let b_token = builder.token("B", 1);
    let a = builder.grammar.next_type_id();
    let b = TypeId::new(a.get() + 1);

    let a_decl = builder.declaration("NodeA");
    let a_token_site = builder.site(a, a_token, "a_token", ContainerShape::Direct);
    let a_to_b = builder.site(
        a,
        b,
        "a_to_b",
        ContainerShape::optional(ContainerShape::Direct),
    );
    builder.grammar.push_type(TypeDescription::new(
        "NodeA",
        a_decl,
        TypeShape::Sequence {
            sites: vec![a_token_site, a_to_b],
        },
    ));

    let b_decl = builder.declaration("NodeB");
    let b_token_site = builder.site(b, b_token, "b_token", ContainerShape::Direct);
    let b_to_a = builder.site(
        b,
        a,
        "b_to_a",
        ContainerShape::optional(ContainerShape::Direct),
    );
    assert_eq!(
        builder.grammar.push_type(TypeDescription::new(
            "NodeB",
            b_decl,
            TypeShape::Sequence {
                sites: vec![b_token_site, b_to_a],
            },
        )),
        b
    );
    builder.grammar.entry_types.push(a);

    let report = analyze(builder.grammar);
    let output = report.output.as_ref().unwrap();
    assert!(output.follow(a).unwrap().eof());
    assert!(output.follow(b).unwrap().eof());
    assert!(output.follow_site(a_to_b).unwrap().eof());
    assert!(output.follow_site(b_to_a).unwrap().eof());
    assert_eq!(
        output
            .follow(a_token)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(1)]
    );
    assert_eq!(
        output
            .follow(b_token)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(0)]
    );
}

#[test]
fn static_error_kinds_suppress_duplicate_nullable_follow_warnings() {
    let mut builder = Builder::new("static-suppression", &["K"]);
    let token = builder.token("Token", 0);
    let owner = builder.grammar.next_type_id();
    let owner_decl = builder.declaration("Owner");
    let optional = builder.site(
        owner,
        token,
        "optional",
        ContainerShape::optional(ContainerShape::Direct),
    );
    let required = builder.site(owner, token, "required", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "Owner",
        owner_decl,
        TypeShape::Sequence {
            sites: vec![optional, required],
        },
    ));
    builder.grammar.entry_types.push(owner);

    let report = analyze(builder.grammar);
    assert!(report.output.is_none());
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code,
        DiagnosticCode::StaticUseSiteOverlap
    );
}

fn shared_declaration_warning_grammar(reverse_roots: bool) -> GrammarDescription {
    let mut builder = Builder::new("warning-dedup", &["K0", "K1"]);
    let multi_atom = builder.category("MultiKinds", &[0, 1]);
    let multi = builder.token_from_atom("Multi", multi_atom);
    let k0 = builder.token("K0", 0);
    let shared_declaration = builder.declaration("shared_optional");

    let static_owner = builder.grammar.next_type_id();
    let static_owner_decl = builder.declaration("StaticOwner");
    let static_optional = builder.grammar.push_site(ParseSite::new(
        static_owner,
        multi,
        shared_declaration,
        ContainerShape::optional(ContainerShape::Direct),
    ));
    let required = builder.site(static_owner, k0, "required", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "StaticOwner",
        static_owner_decl,
        TypeShape::Sequence {
            sites: vec![static_optional, required],
        },
    ));

    let mut roots = vec![static_owner];
    for suffix in ["A", "B"] {
        let inner = builder.grammar.next_type_id();
        let inner_decl = builder.declaration(&format!("Inner{suffix}"));
        let optional = builder.grammar.push_site(ParseSite::new(
            inner,
            multi,
            shared_declaration,
            ContainerShape::optional(ContainerShape::Direct),
        ));
        builder.grammar.push_type(TypeDescription::new(
            format!("Inner{suffix}"),
            inner_decl,
            TypeShape::Sequence {
                sites: vec![optional],
            },
        ));

        let root = builder.grammar.next_type_id();
        let root_decl = builder.declaration(&format!("Root{suffix}"));
        let repeat = builder.site(
            root,
            inner,
            &format!("repeat{suffix}"),
            ContainerShape::zero_or_more(ContainerShape::Direct, Some(multi)),
        );
        builder.grammar.push_type(TypeDescription::new(
            format!("Root{suffix}"),
            root_decl,
            TypeShape::Sequence {
                sites: vec![repeat],
            },
        ));
        roots.push(root);
    }
    if reverse_roots {
        roots.reverse();
    }
    builder.grammar.entry_types = roots;
    builder.grammar
}

#[test]
fn warnings_dedup_by_class_and_declaration_after_partial_static_subtraction() {
    let forward = analyze(shared_declaration_warning_grammar(false));
    let reverse = analyze(shared_declaration_warning_grammar(true));
    assert_eq!(forward.diagnostics, reverse.diagnostics);
    assert_eq!(forward.proofs(), reverse.proofs());
    assert_eq!(forward.diagnostics.len(), 4);
    assert_eq!(
        forward.diagnostics[0].code,
        DiagnosticCode::StaticUseSiteOverlap
    );
    assert_eq!(
        forward.diagnostics[1].code,
        DiagnosticCode::NullableFollowOverlap
    );

    let shared = forward
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.code == DiagnosticCode::NullableFollowOverlap
                && diagnostic.primary.message.contains("nullable use site")
                && diagnostic
                    .conflict_remedy
                    .as_ref()
                    .is_some_and(|remedy| matches!(remedy, ConflictRemedy::ReviewNullableFollow { site } if *site != recursa_codegen::SiteId::new(3) && *site != recursa_codegen::SiteId::new(5)))
        })
        .collect::<Vec<_>>();
    assert_eq!(shared.len(), 1);
    assert_eq!(
        shared[0]
            .conflict
            .as_ref()
            .unwrap()
            .overlap
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(1)]
    );
    assert_eq!(
        shared[0].conflict.as_ref().unwrap().witness.kind,
        KindId::new(1)
    );

    let repeat_warning = forward
        .diagnostics
        .iter()
        .find(|diagnostic| {
            matches!(
                diagnostic.conflict_remedy,
                Some(ConflictRemedy::ReviewNullableFollow { site })
                    if site == recursa_codegen::SiteId::new(3)
            )
        })
        .unwrap();
    assert_eq!(
        repeat_warning
            .conflict
            .as_ref()
            .unwrap()
            .overlap
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(0), KindId::new(1)]
    );
    assert_eq!(
        repeat_warning.conflict.as_ref().unwrap().witness.kind,
        KindId::new(0)
    );
}

#[test]
fn pratt_follow_warnings_filter_extenders_by_site_min_bp() {
    let mut builder = Builder::new("pratt-follow", &["Atom", "Low", "High", "Post"]);
    let atom = builder.token("Atom", 0);
    let low = builder.token("Low", 1);
    let high = builder.token("High", 2);
    let post = builder.token("Post", 3);
    let expr = builder.grammar.next_type_id();
    let expr_decl = builder.declaration("Expr");
    let atom_decl = builder.declaration("Expr::Atom");
    let low_decl = builder.declaration("Expr::Low");
    let high_decl = builder.declaration("Expr::High");
    let post_decl = builder.declaration("Expr::Post");
    let atom_site = builder.site(expr, atom, "atom", ContainerShape::Direct);
    let low_site = builder.site(expr, low, "low", ContainerShape::Direct);
    let high_site = builder.site(expr, high, "high", ContainerShape::Direct);
    let post_site = builder.site(expr, post, "post", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
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
                    declaration: low_decl,
                    position: PrattPosition::Infix { lbp: 10, rbp: 11 },
                    sites: vec![low_site],
                },
                PrattVariant {
                    declaration: high_decl,
                    position: PrattPosition::Infix { lbp: 20, rbp: 21 },
                    sites: vec![high_site],
                },
                PrattVariant {
                    declaration: post_decl,
                    position: PrattPosition::Postfix { bp: 15 },
                    sites: vec![post_site],
                },
            ],
        },
    ));

    let low_context = builder.grammar.next_type_id();
    let low_context_decl = builder.declaration("LowContext");
    let low_expr = builder.grammar.push_site(
        ParseSite::new(low_context, expr, low_context_decl, ContainerShape::Direct).with_min_bp(15),
    );
    let low_tail = builder.site(low_context, low, "low_tail", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "LowContext",
        low_context_decl,
        TypeShape::Sequence {
            sites: vec![low_expr, low_tail],
        },
    ));

    let high_context = builder.grammar.next_type_id();
    let high_context_decl = builder.declaration("HighContext");
    let high_expr = builder.grammar.push_site(
        ParseSite::new(
            high_context,
            expr,
            high_context_decl,
            ContainerShape::Direct,
        )
        .with_min_bp(15),
    );
    let high_tail = builder.site(high_context, high, "high_tail", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "HighContext",
        high_context_decl,
        TypeShape::Sequence {
            sites: vec![high_expr, high_tail],
        },
    ));

    let post_context = builder.grammar.next_type_id();
    let post_context_decl = builder.declaration("PostContext");
    let post_expr = builder.grammar.push_site(
        ParseSite::new(
            post_context,
            expr,
            post_context_decl,
            ContainerShape::Direct,
        )
        .with_min_bp(15),
    );
    let post_tail = builder.site(post_context, post, "post_tail", ContainerShape::Direct);
    builder.grammar.push_type(TypeDescription::new(
        "PostContext",
        post_context_decl,
        TypeShape::Sequence {
            sites: vec![post_expr, post_tail],
        },
    ));
    builder.grammar.entry_types = vec![low_context, high_context, post_context];

    let report = analyze(builder.grammar);
    assert!(report.output.is_some());
    assert_eq!(report.diagnostics.len(), 2);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == DiagnosticCode::PrattFollowOverlap)
    );
    let high_warning = report
        .diagnostics
        .iter()
        .find(|diagnostic| {
            matches!(
                diagnostic.conflict_remedy,
                Some(ConflictRemedy::ReviewPrattFollow { site, min_bp: 15 }) if site == high_expr
            )
        })
        .unwrap();
    assert!(matches!(
        report.diagnostics[1].conflict_remedy,
        Some(ConflictRemedy::ReviewPrattFollow { site, min_bp: 15 }) if site == post_expr
    ));
    assert_eq!(
        high_warning
            .conflict
            .as_ref()
            .unwrap()
            .overlap
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(2)]
    );
    let high_proof = high_warning.conflict.as_ref().unwrap().witness.left;
    assert!(report.proofs().chain(high_proof).any(|node| matches!(
        node.step,
        recursa_codegen::ProvenanceStep::PrattExtender {
            pratt,
            pratt_declaration,
            variant,
            position: PrattPosition::Infix { lbp: 20, rbp: 21 },
        } if pratt == expr && pratt_declaration == expr_decl && variant == high_decl
    )));
    assert!(high_warning.primary.location.is_some());
    let output = report.output.as_ref().unwrap();
    assert_eq!(
        output
            .follow(expr)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        vec![KindId::new(1), KindId::new(2), KindId::new(3)]
    );
    assert_eq!(
        output
            .follow_site(low_expr)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(1)]
    );
    assert_eq!(
        output
            .follow_site(high_expr)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(2)]
    );
    assert_eq!(
        output
            .follow_site(post_expr)
            .unwrap()
            .kinds()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(3)]
    );
}

#[test]
fn every_follow_membership_has_exactly_one_proof() {
    let (grammar, _) = nested_nullable_follow(true);
    let report = analyze(grammar);
    let output = report.output.unwrap();

    for facts in output
        .follow_by_type()
        .iter()
        .chain(output.follow_by_site())
    {
        for index in 0..facts.kinds().kind_count() {
            let kind = KindId::new(index as u16);
            assert_eq!(facts.kinds().contains(kind), facts.proof(kind).is_some());
        }
        assert_eq!(facts.eof(), facts.eof_proof().is_some());
    }
}
