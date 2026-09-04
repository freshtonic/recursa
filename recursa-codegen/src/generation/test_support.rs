use super::*;

pub(super) fn validated_generation_is_not_revalidated_during_rendering() {
    let package = tempfile::tempdir().expect("temporary validated rendering fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
#[tok("root")]
struct Root;
"#,
    )
    .expect("write validated rendering fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze validated rendering fixture");

    reset_emission_plan_validation_count();
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a validated Emission plan");
    assert_eq!(emission_plan_validation_count(), 1);

    render(&plan).expect("render the already validated Emission plan");
    assert_eq!(
        emission_plan_validation_count(),
        1,
        "rendering must consume the validation proof established while joining",
    );
}

pub(super) fn checked_rendering_retains_invalid_rust_diagnostics() {
    let (_package, mut plan) = joined_pratt_fixture();
    plan.module_anchor.segments[0].arguments =
        PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
            colon2_token: None,
            lt_token: Default::default(),
            args: Default::default(),
            gt_token: Default::default(),
        });

    let detail = match render(&plan) {
        Err(detail) => detail,
        Ok(_) => panic!("checked rendering must reject invalid generated Rust"),
    };
    assert!(
        detail.starts_with("root rendering is invalid Rust:"),
        "the syntax failure retains its stable rendering role: {detail}",
    );
}

pub(super) fn debug_directory_environment_is_always_tracked_by_cargo() {
    let mut cargo = Vec::new();
    emit_debug_directory_invalidation(&mut cargo);
    assert_eq!(cargo, ["cargo::rerun-if-env-changed=RECURSA_DEBUG_DIR"]);
}

pub(super) fn debug_bundle_is_deterministic_formatted_complete_and_identity_neutral() {
    let (_package, plan) = joined_pratt_fixture();
    let rendered = render(&plan).unwrap();
    let identity = plan.identity.clone();
    let authoritative_root = rendered.root.clone();
    let authoritative_nodes = rendered.nodes.clone();
    let destination = tempfile::tempdir().unwrap();
    fs::write(destination.path().join("owned-by-caller"), "preserve me").unwrap();

    let staged = stage_debug_bundle(destination.path(), &plan, &rendered).unwrap();
    let published = staged.published.clone();
    assert!(!published.exists(), "a staged bundle is not yet observable");
    staged.commit().unwrap();

    assert_eq!(plan.identity, identity);
    assert_eq!(rendered.root, authoritative_root);
    assert_eq!(rendered.nodes, authoritative_nodes);
    assert_eq!(
        fs::read_to_string(destination.path().join("owned-by-caller")).unwrap(),
        "preserve me"
    );
    let index = fs::read_to_string(published.join("index")).unwrap();
    assert!(index.starts_with(DEBUG_BUNDLE_FORMAT));
    assert_eq!(
        index
            .lines()
            .filter(|line| line.starts_with("root\t"))
            .count(),
        1
    );
    assert_eq!(
        index
            .lines()
            .filter(|line| line.starts_with("node\t"))
            .count(),
        plan.nodes.len()
    );
    assert!(
        index
            .lines()
            .any(|line| line.starts_with("capability\tconfigured:")),
        "configured capability bodies are indexed"
    );
    assert!(
        index.lines().any(|line| {
            line.starts_with("capability\tintrinsic-token:")
                && line.contains(":Debug+Clone+PartialEq+Eq+PartialOrd+Ord+Hash\t")
        }),
        "intrinsic content-token value traits are indexed separately from configured recipes",
    );
    for line in index.lines().skip(2) {
        let [_, _, private_name] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("debug index row is malformed: {line}");
        };
        let source = fs::read_to_string(published.join(private_name)).unwrap();
        let syntax = syn::parse_file(&source).expect("every logical view is parseable Rust");
        assert_eq!(
            prettyplease::unparse(&syntax),
            source,
            "every logical view is stable under the formatter"
        );
    }

    fs::write(published.join("root.rs"), "human notes, not Rust").unwrap();
    fs::write(published.join("index"), "human index edit").unwrap();
    fs::write(
        published.join("extra-notes.txt"),
        "keep out of generated view",
    )
    .unwrap();
    let repeated = stage_debug_bundle(destination.path(), &plan, &rendered).unwrap();
    assert!(repeated.staging.is_some());
    assert!(repeated.replacement.is_some());
    repeated.commit().unwrap();
    assert_eq!(fs::read_to_string(published.join("index")).unwrap(), index);
    assert!(!published.join("extra-notes.txt").exists());
    assert_eq!(plan.identity, identity);
    let regenerated_root = fs::read_to_string(published.join("root.rs")).unwrap();
    syn::parse_file(&regenerated_root).expect("human edits are replaced, not loaded");
}

pub(super) fn arbitrary_identity_collision_is_preserved_and_rejected_without_replacement() {
    let (_package, plan) = joined_pratt_fixture();
    let rendered = render(&plan).unwrap();
    let destination = tempfile::tempdir().unwrap();
    let collision = destination
        .path()
        .join(format!(".{DEBUG_BUNDLE_FORMAT}"))
        .join(&plan.identity);
    fs::create_dir_all(&collision).unwrap();
    fs::write(collision.join("caller-data"), "do not remove").unwrap();

    let error = match stage_debug_bundle(destination.path(), &plan, &rendered) {
        Ok(_) => panic!("an unowned identity collision must not be replaced"),
        Err(error) => error,
    };
    assert!(error.contains("not owned by Recursa"), "{error}");
    assert_eq!(
        fs::read_to_string(collision.join("caller-data")).unwrap(),
        "do not remove"
    );
}

#[cfg(all(feature = "diagrams", feature = "explorer"))]
pub(super) fn debug_bundle_includes_every_enabled_diagram_and_explorer_capability_body() {
    let (_package, plan) = joined_pratt_diagram_fixture();
    let rendered = render(&plan).unwrap();
    let files = render_debug_logical_files(&plan, &rendered).unwrap();
    let index = render_debug_index(&plan.identity, &files);
    assert!(index.contains("capability\tdiagrams:aggregate\t"));
    for node in &plan.nodes {
        assert!(
            index.contains(&format!("capability\tdiagrams:{}\t", node.semantic_key)),
            "missing diagram body for {}\n{index}",
            node.semantic_key
        );
    }
    for diagram in &plan.diagrams.as_ref().expect("diagram plan").tokens {
        assert!(
            index.contains(&format!(
                "capability\tdiagrams:token:{}\t",
                diagram.semantic_key
            )),
            "missing content-token diagram body for {}\n{index}",
            diagram.semantic_key
        );
    }
    assert!(index.contains("capability\texplorer:static-schema\t"));
    assert!(index.contains("capability\texplorer:lexer\t"));
    for token in &plan.tokens {
        assert!(
            index.contains(&format!(
                "capability\texplorer:token:{}\t",
                token.kind_variant
            )),
            "missing explorer token body for {}\n{index}",
            token.kind_variant
        );
    }
    for node in &plan.nodes {
        for application in &node.parse_applications {
            assert!(
                index.contains(&format!(
                    "capability\texplorer:node:{}:application-{}\t",
                    node.semantic_key,
                    application.application.index()
                )),
                "missing explorer application body for {} application {}\n{index}",
                node.semantic_key,
                application.application.index()
            );
        }
    }
}

pub(super) fn failed_debug_commit_is_cleaned_and_makes_the_generation_unselectable() {
    let (_package, plan) = joined_pratt_fixture();
    let rendered = render(&plan).unwrap();
    let destination = tempfile::tempdir().unwrap();
    let staged = stage_debug_bundle(destination.path(), &plan, &rendered).unwrap();
    let staging = staged.staging.clone().unwrap();
    fs::create_dir(&staged.published).unwrap();
    fs::write(staged.published.join("caller-data"), "do not remove").unwrap();
    let output = tempfile::tempdir().unwrap();

    let detail =
        publish_generation_and_debug(output.path(), &plan.identity, &rendered, Some(staged))
            .unwrap_err();
    assert!(detail.contains("debug bundle"), "{detail}");
    assert_eq!(selected_generation(output.path()), None);
    assert!(!staging.exists());
    assert_eq!(
        fs::read_to_string(
            destination
                .path()
                .join(format!(".{DEBUG_BUNDLE_FORMAT}"))
                .join(&plan.identity)
                .join("caller-data")
        )
        .unwrap(),
        "do not remove"
    );
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one debug emission failure is required");
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
}

#[cfg(unix)]
pub(super) fn debug_destination_and_bundle_entries_reject_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;

    let (_package, plan) = joined_pratt_fixture();
    let rendered = render(&plan).unwrap();
    let parent = tempfile::tempdir().unwrap();
    let target = parent.path().join("target");
    fs::create_dir(&target).unwrap();
    let destination = parent.path().join("debug-link");
    symlink(&target, &destination).unwrap();
    let error = match stage_debug_bundle(&destination, &plan, &rendered) {
        Ok(_) => panic!("a symlink destination must be rejected"),
        Err(error) => error,
    };
    assert!(error.contains("symbolic link"), "{error}");
    assert!(fs::read_dir(target).unwrap().next().is_none());
}

pub(super) fn nested_visit_variant_uses_the_path_relative_to_the_grammar_module() {
    assert_eq!(
        visit_variant_name("grammar", "grammar::public_left::Duplicate"),
        "__RecursaTarget_11_public_left_9_Duplicate"
    );
}

pub(super) fn root_visit_variants_are_encoded_away_from_reserved_and_nested_names() {
    assert_eq!(
        visit_variant_name("grammar", "grammar::Opaque"),
        "__RecursaTarget_6_Opaque"
    );
    assert_ne!(
        visit_variant_name(
            "grammar",
            "grammar::__RecursaTarget_11_public_left_9_Duplicate"
        ),
        visit_variant_name("grammar", "grammar::public_left::Duplicate")
    );
}

pub(super) fn snake_case_produces_stable_generated_element_names() {
    assert_eq!(snake_case("RECORD"), "record");
    assert_eq!(snake_case("LabelColon"), "label_colon");
    assert_eq!(snake_case(":"), "element");
}

pub(super) fn synthetic_fixed_spelling_uppercases_only_ascii_insensitive_keywords() {
    assert_eq!(
        fixed_pretty_spelling("record", TextEquivalence::AsciiInsensitive),
        "RECORD"
    );
    assert_eq!(
        fixed_pretty_spelling(":", TextEquivalence::AsciiInsensitive),
        ":"
    );
    assert_eq!(
        fixed_pretty_spelling("record", TextEquivalence::Sensitive),
        "record"
    );
}

pub(super) fn fixed_accessor_names_remain_valid_for_numeric_spellings() {
    assert_eq!(
        super::fixed_accessor_ident("123").unwrap().to_string(),
        "element_123_span"
    );
}

pub(super) fn predictive_compilation_rejects_missing_and_conflicting_frozen_products() {
    let position = DispatchPositionPlan {
        expected: ExpectedSet {
            words: vec![1],
            atoms: vec![0],
        },
        text_predicate: None,
    };
    let root = DispatchStateId::new(1);
    let states = vec![
        DispatchTrieNodePlan {
            target: Some(DispatchTargetPlan {
                variant: 0,
                mode: crate::DispatchTerminalMode::Immediate,
            }),
            edges: Vec::new(),
        },
        DispatchTrieNodePlan {
            target: None,
            edges: vec![DispatchTrieEdgePlan {
                position,
                next: DispatchStateId::new(0),
            }],
        },
    ];
    let mut dispatch = PredictiveDispatchPlan {
        frozen_identity: predictive_dispatch_identity(root, &states),
        root,
        states,
        frozen_sequence_count: 1,
        frozen_node_count: 2,
        balanced: None,
    };
    assert_eq!(validate_predictive_dispatch(&dispatch, 1).unwrap(), 2);
    dispatch.states[0].target.as_mut().unwrap().variant = 1;
    assert_eq!(
        validate_predictive_dispatch(&dispatch, 1).unwrap_err(),
        "frozen predictive dispatch references an unknown variant",
    );

    dispatch.states[0].target.as_mut().unwrap().variant = 0;
    dispatch.states[0].target.as_mut().unwrap().mode =
        crate::DispatchTerminalMode::FallbackAfterEdges;
    assert_eq!(
        validate_predictive_dispatch(&dispatch, 1).unwrap_err(),
        "frozen predictive dispatch fallback lacks competitor edges",
    );
    dispatch.states[0].target.as_mut().unwrap().mode = crate::DispatchTerminalMode::Immediate;
    dispatch.states[dispatch.root.index()].target = Some(DispatchTargetPlan {
        variant: 0,
        mode: crate::DispatchTerminalMode::Immediate,
    });
    assert_eq!(
        validate_predictive_dispatch(&dispatch, 1).unwrap_err(),
        "frozen predictive dispatch extends an immediate terminal",
    );
    dispatch.states[dispatch.root.index()].target = None;
    dispatch.states[dispatch.root.index()].edges[0]
        .position
        .text_predicate = Some(PlannedTextPredicate::Exact("corrupt".to_owned()));
    assert_eq!(
        validate_predictive_dispatch(&dispatch, 1).unwrap_err(),
        "the joined predictive plan diverges from its frozen identity",
    );
}

pub(super) fn large_predictive_selection_emits_each_position_matcher_once() {
    let (_package, mut plan) = joined_pratt_fixture();
    let expected = |kind: usize| {
        let mut expected = ExpectedSet::empty(plan.kind_count);
        expected.words[kind / 64] |= 1 << (kind % 64);
        expected
    };
    let positions = [
        DispatchPositionPlan {
            expected: expected(0),
            text_predicate: Some(PlannedTextPredicate::Exact("SELECT".to_owned())),
        },
        DispatchPositionPlan {
            expected: expected(0),
            text_predicate: Some(PlannedTextPredicate::NotIn(vec!["SELECT".to_owned()])),
        },
        DispatchPositionPlan {
            expected: expected(1),
            text_predicate: Some(PlannedTextPredicate::OneOf(vec![
                "FROM".to_owned(),
                "WHERE".to_owned(),
            ])),
        },
        DispatchPositionPlan {
            expected: expected(1),
            text_predicate: Some(PlannedTextPredicate::NotIn(vec![
                "FROM".to_owned(),
                "WHERE".to_owned(),
            ])),
        },
    ];
    let mut states = vec![DispatchTrieNodePlan {
        target: Some(DispatchTargetPlan {
            variant: 0,
            mode: crate::DispatchTerminalMode::Immediate,
        }),
        edges: Vec::new(),
    }];
    for state in 1..=128 {
        states.push(DispatchTrieNodePlan {
            target: None,
            edges: positions
                .iter()
                .cloned()
                .map(|position| DispatchTrieEdgePlan {
                    position,
                    next: DispatchStateId::new(state - 1),
                })
                .collect(),
        });
    }
    let root = DispatchStateId::new(states.len() - 1);
    let dispatch = PredictiveDispatchPlan {
        frozen_identity: predictive_dispatch_identity(root, &states),
        root,
        frozen_sequence_count: positions.len(),
        frozen_node_count: states.len(),
        states,
        balanced: None,
    };
    assert_eq!(
        validate_predictive_dispatch(&dispatch, 1).unwrap(),
        dispatch.states.len(),
    );
    plan.expected_sets = ExpectedSetPool::from_values([expected(0), expected(1)]);

    let mut expected_index = 2_000_000;
    let mut instrumentation = RenderInstrumentation::default();
    let (selection, items) = render_predictive_selection(
        &plan,
        9_000,
        &dispatch,
        &quote!(context.follow()),
        &mut expected_index,
        &mut instrumentation,
    );
    let rendered = quote!(#items #selection).to_string();

    assert_eq!(instrumentation.frozen_dispatch_nodes_rendered, 129);
    assert!(
        rendered.contains("static __RECURSA_PRATT_9000_2000000 : recursa :: __private :: DispatchTable = recursa :: __private :: dispatch_table (b\""),
        "large Pratt state tables must be one packed literal rather than one HIR tuple per row",
    );
    assert!(
        !rendered.contains("fn __recursa_decode_pratt_"),
        "packed Pratt tables decode through the runtime walker, not a generated decoder",
    );
    assert!(
        !rendered.contains("type __RecursaPrattStateRow"),
        "packed Pratt tables must not reconstruct the tuple-row blowup",
    );
    assert_eq!(
        rendered.matches("fn __recursa_pratt_position_").count(),
        0,
        "Pratt positions are data rows rather than separately typechecked functions",
    );
    assert_eq!(
        rendered
            .matches("fn __recursa_pratt_match_position_")
            .count(),
        0,
        "the runtime table walker matches Pratt positions; no per-table matcher is generated",
    );
    assert!(
        rendered.contains("context . select_pratt (& __RECURSA_PRATT_9000_2000000 ,"),
        "Pratt selection walks its table through the runtime",
    );
}

pub(super) fn pratt_branches_are_typechecked_in_isolated_helpers() {
    let (_package, plan) = joined_pratt_fixture();
    let rendered = render(&plan).expect("the Pratt fixture renders");
    let expr_index = plan
        .nodes
        .iter()
        .position(|node| node.ident == "Expr")
        .expect("the fixture contains its Pratt Expr");
    let expr = &rendered.nodes[expr_index].1;
    let pratt = plan.nodes[expr_index]
        .parse_applications
        .iter()
        .find_map(|application| match &application.body {
            NodeBodyPlan::Pratt(pratt) => Some(pratt),
            NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
        })
        .expect("Expr has one materialized Pratt application");

    assert_eq!(
        expr.matches("fn __recursa_parse_pratt_nud_").count(),
        pratt.nud.len(),
        "each Pratt nud branch has exactly one separately typechecked function item",
    );
    assert_eq!(
        expr.matches("fn __recursa_parse_pratt_led_").count(),
        pratt.led.len(),
        "each Pratt led branch has exactly one separately typechecked function item",
    );
    assert_eq!(
        expr.matches("Self :: __recursa_parse_pratt_nud_").count(),
        pratt.nud.len(),
        "the Pratt control loop calls each isolated nud helper exactly once",
    );
    assert_eq!(
        expr.matches("Self :: __recursa_parse_pratt_led_").count(),
        pratt.led.len(),
        "the Pratt control loop calls each isolated led helper exactly once",
    );
    let dispatcher_start = expr
        .find("fn __recursa_parse_bp")
        .expect("the Pratt dispatcher is rendered");
    let helper_start = expr[dispatcher_start..]
        .find("fn __recursa_parse_pratt_nud_")
        .map(|offset| dispatcher_start + offset)
        .expect("the first isolated helper follows the dispatcher");
    assert!(
        helper_start - dispatcher_start < 20_000,
        "the fixture Pratt dispatcher remains a bounded control function",
    );
}

pub(super) fn predictive_variants_are_typechecked_in_isolated_helpers() {
    let source = r#"
recursa::grammar! { module = crate, max_lookahead = 1 }

recursa::tokens! {
    keywords { ALPHA => "ALPHA", BETA => "BETA", GAMMA => "GAMMA" }
}

#[derive(recursa::Node)]
enum Choice {
    #[tok(ALPHA)]
    Alpha,
    #[tok(BETA)]
    Beta,
}

#[derive(recursa::Node)]
enum Wrapped {
    Choice(Choice),
    #[tok(GAMMA)]
    Prefixed(Choice),
}
"#;
    let (_package, plan) = joined_pratt_fixture_from(source);
    let rendered = render(&plan).expect("the predictive fixture renders");
    let node = |name: &str| {
        let index = plan
            .nodes
            .iter()
            .position(|node| node.ident == name)
            .unwrap_or_else(|| panic!("the fixture contains {name}"));
        &rendered.nodes[index].1
    };
    let choice = node("Choice");

    assert_eq!(
        choice.matches("fn __recursa_parse_variant_").count(),
        0,
        "fixed-only variants parse through the runtime without a helper function",
    );
    assert_eq!(
        choice.matches("context . parse_fixed_variant (").count(),
        2,
        "predictive dispatch calls the fixed-variant runtime helper per fixed-only variant",
    );

    let wrapped = node("Wrapped");
    assert_eq!(
        wrapped
            .matches("context . parse_wrapping_variant ::")
            .count(),
        1,
        "a single direct-child variant parses through the wrapping runtime helper",
    );
    assert!(
        wrapped.contains("Self :: Choice ,)"),
        "the tuple constructor is passed as the wrapping function",
    );
    assert_eq!(
        wrapped.matches("fn __recursa_parse_variant_").count(),
        1,
        "a variant with fixed tokens around its child keeps its typechecked helper",
    );
    assert_eq!(
        wrapped.matches("context . parse_direct_child ::").count(),
        1,
        "the helper parses its direct child through the runtime element transaction",
    );
    assert!(
        !wrapped.contains("follow () . clone ()"),
        "generated parsers borrow the caller FOLLOW instead of cloning it",
    );
}

pub(super) fn large_ordinary_predictive_dispatch_uses_compact_tables() {
    let (_package, mut plan) = joined_pratt_fixture();
    let expected = |kind: usize| {
        let mut expected = ExpectedSet::empty(plan.kind_count);
        expected.words[kind / 64] |= 1 << (kind % 64);
        expected
    };
    let positions = [
        DispatchPositionPlan {
            expected: expected(0),
            text_predicate: Some(PlannedTextPredicate::Exact("SELECT".to_owned())),
        },
        DispatchPositionPlan {
            expected: expected(0),
            text_predicate: Some(PlannedTextPredicate::NotIn(vec!["SELECT".to_owned()])),
        },
        DispatchPositionPlan {
            expected: expected(1),
            text_predicate: Some(PlannedTextPredicate::OneOf(vec![
                "FROM".to_owned(),
                "WHERE".to_owned(),
            ])),
        },
        DispatchPositionPlan {
            expected: expected(1),
            text_predicate: Some(PlannedTextPredicate::NotIn(vec![
                "FROM".to_owned(),
                "WHERE".to_owned(),
            ])),
        },
    ];
    let mut states = vec![DispatchTrieNodePlan {
        target: Some(DispatchTargetPlan {
            variant: 0,
            mode: crate::DispatchTerminalMode::Immediate,
        }),
        edges: Vec::new(),
    }];
    for state in 1..=128 {
        states.push(DispatchTrieNodePlan {
            target: None,
            edges: positions
                .iter()
                .cloned()
                .map(|position| DispatchTrieEdgePlan {
                    position,
                    next: DispatchStateId::new(state - 1),
                })
                .collect(),
        });
    }
    let root = DispatchStateId::new(states.len() - 1);
    let root_extra = expected(2);
    let mut all_edges = expected(0);
    all_edges.union_assign(&expected(1));
    let mut root_failure = all_edges.clone();
    root_failure.union_assign(&root_extra);
    plan.expected_sets = ExpectedSetPool::from_values([
        ExpectedSet::empty(plan.kind_count),
        expected(0),
        expected(1),
        all_edges,
        root_extra.clone(),
        root_failure,
    ]);
    let mut expected_index = 1_000_000;
    let (selection, items) = render_predictive_graph_selection(
        &plan,
        9_001,
        root,
        &states,
        &quote!(0usize),
        Some(&root_extra),
        &mut expected_index,
    );
    let rendered = quote!(#items #selection).to_string();
    assert!(
        rendered.contains("static __RECURSA_DISPATCH_9001_1000000 : recursa :: __private :: DispatchTable = recursa :: __private :: dispatch_table (b\""),
        "ordinary dispatch must use one packed state literal; inline bytes={}",
        rendered.len(),
    );
    assert!(
        !rendered.contains("type __RecursaDispatchStateRow"),
        "ordinary dispatch must not emit one HIR tuple per state",
    );
    assert!(
        rendered.contains("context . select_predictive (& __RECURSA_DISPATCH_9001_1000000 ,"),
        "ordinary dispatch selects through the runtime table walker",
    );
    assert_eq!(
        rendered.matches("fn __recursa_dispatch_position_").count(),
        0,
        "ordinary positions are data rows rather than separately typechecked functions",
    );
    assert_eq!(
        rendered
            .matches("fn __recursa_dispatch_match_position_")
            .count(),
        0,
        "the runtime table walker matches positions; no per-table matcher is generated",
    );
    assert!(!rendered.contains("fn __recursa_decode_dispatch_"));
    assert!(
        rendered.len() < 40_000,
        "a 129-state graph must remain compact; bytes={}",
        rendered.len(),
    );
}

pub(super) fn single_digit_packed_dispatch_is_clippy_clean() {
    let (_package, mut plan) = joined_pratt_fixture();
    plan.expected_sets = ExpectedSetPool::from_values([ExpectedSet::empty(plan.kind_count)]);
    let states = vec![DispatchTrieNodePlan {
        target: Some(DispatchTargetPlan {
            variant: 0,
            mode: crate::DispatchTerminalMode::Immediate,
        }),
        edges: Vec::new(),
    }];
    let mut expected_index = 3_000_000;
    let (selection, items) = render_predictive_graph_selection(
        &plan,
        9_002,
        DispatchStateId::new(0),
        &states,
        &quote!(0usize),
        None,
        &mut expected_index,
    );
    let rendered = quote!(#items #selection).to_string();

    assert!(
        rendered.contains(", & [] , 1usize , 4usize , 1usize , &")
            && rendered.contains(":: __RECURSA_EXPECTED_SETS ,)"),
        "a single-digit table records width 1 and no spellings: {rendered}",
    );
    assert!(!rendered.contains("fn __recursa_decode_dispatch_"));
    assert!(rendered.contains(
        "context . select_predictive (& __RECURSA_DISPATCH_9002_3000000 , 0usize , 0usize) ?"
    ));
}

pub(super) fn grammar_wide_expected_set_pool_is_canonical() {
    let (_package, plan) = joined_pratt_fixture();
    let values = plan.expected_sets.values();
    assert!(
        values.windows(2).all(|pair| pair[0] < pair[1]),
        "grammar-wide expectation identities must be strictly canonical",
    );

    let plus = plan
        .nodes
        .iter()
        .flat_map(|node| &node.parse_applications)
        .find_map(|application| {
            let NodeBodyPlan::Pratt(pratt) = &application.body else {
                return None;
            };
            pratt.led.iter().find_map(|branch| {
                branch
                    .variant
                    .elements
                    .iter()
                    .find_map(|element| match &element.kind {
                        ElementParseKind::Fixed {
                            expected, spelling, ..
                        } if spelling == "+" => Some(expected.clone()),
                        _ => None,
                    })
            })
        })
        .expect("the Pratt fixture emits a PLUS expectation");
    let plus_id = plan
        .expected_sets
        .id(&plus)
        .expect("the emitted PLUS expectation belongs to the grammar-wide pool");
    assert_eq!(values[plus_id], plus);
}

pub(super) fn stale_grammar_wide_expected_set_pool_fails_before_rendering() {
    let (_package, mut plan) = joined_pratt_fixture();
    plan.expected_sets = ExpectedSetPool::from_plans(&[], &[], plan.kind_count);

    let detail = match render(&plan) {
        Err(detail) => detail,
        Ok(_) => panic!("a stale grammar-wide expectation pool cannot render"),
    };
    assert!(
        detail.contains("grammar-wide expectation pool"),
        "pool corruption must retain its invariant role: {detail}",
    );
}

pub(super) fn balanced_root_failure_belongs_to_grammar_wide_expected_set_pool() {
    let (_package, plan) = joined_pratt_fixture_from(include_str!(
        "../../tests/fixtures/generation/predictive/src/grammar.rs"
    ));
    let dispatch = plan
        .nodes
        .iter()
        .flat_map(|node| &node.parse_applications)
        .find_map(|application| match &application.body {
            NodeBodyPlan::Enum { dispatch, .. } if dispatch.balanced.is_some() => Some(dispatch),
            NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } | NodeBodyPlan::Pratt(_) => {
                None
            }
        })
        .expect("the predictive fixture contains balanced enum dispatch");
    let balanced = dispatch.balanced.as_ref().unwrap();
    let mut ordinary_root = ExpectedSet::empty(plan.kind_count);
    for edge in &dispatch.states[dispatch.root.index()].edges {
        ordinary_root.union_assign(&edge.position.expected);
    }
    let mut balanced_root = ordinary_root.clone();
    balanced_root.union_assign(&balanced.open);
    assert_ne!(ordinary_root, balanced_root);
    assert!(
        plan.expected_sets.id(&balanced_root).is_some(),
        "the failure set rendered at a balanced ordinary root must be pooled",
    );
    render(&plan).expect("every balanced dispatch expectation resolves through the pool");
}

pub(super) fn generated_expectations_use_one_grammar_wide_pool() {
    let (_package, plan) = joined_pratt_fixture();
    let rendered = render(&plan).expect("render the pooled-expectation fixture");
    assert_eq!(
        rendered
            .root
            .matches("static __RECURSA_EXPECTED_SETS")
            .count(),
        1,
        "the generated root owns one grammar-wide expectation pool",
    );
    assert_eq!(
        rendered.root.matches("__private :: follow_set").count(),
        plan.expected_sets.values().len() + 1,
        "the root emits one initializer per unique expectation plus root FOLLOW",
    );
    assert!(
        !rendered.root.contains("__RECURSA_TOKEN_START"),
        "content-token starts are addressed through the shared pool",
    );
    assert!(
        rendered
            .nodes
            .iter()
            .all(|(_, source)| !source.contains("__private :: follow_set")),
        "Node shards contain references, not local expectation definitions",
    );
    let references = rendered.root.matches("__RECURSA_EXPECTED_SETS").count()
        + rendered
            .nodes
            .iter()
            .map(|(_, source)| source.matches("__RECURSA_EXPECTED_SETS").count())
            .sum::<usize>();
    assert!(
        references > plan.expected_sets.values().len(),
        "the fixture must exercise reuse across more sites than unique pooled values",
    );
}

pub(super) fn large_expected_set_batch_uses_canonical_atom_index() {
    const KIND_COUNT: usize = 2_048;
    const POSITION_COUNT: usize = 4_096;

    let mut grammar = crate::GrammarDescription::new(
        "large expected-set lookup",
        KIND_COUNT,
        1,
        TextEquivalence::Sensitive,
    );
    for index in 0..KIND_COUNT {
        let declaration = crate::DeclId::new(index as u32);
        let kind = grammar.push_kind(crate::KindDescription::new(
            format!("K{index}"),
            declaration,
        ));
        grammar.push_atom(crate::AtomDescription::new(
            format!("A{index}"),
            declaration,
            AtomExpression::Kind(kind),
        ));
    }

    let atom_index = ExpectedAtomIndex::from_grammar(&grammar)
        .expect("the grammar's canonical atom identities fit the runtime width");
    let last_kind = crate::KindId::new((KIND_COUNT - 1) as u16);
    let mut kinds = KindSet::empty(KIND_COUNT);
    kinds.insert(last_kind).unwrap();
    let positions = [FirstPosition {
        kinds,
        text_predicate: None,
    }];

    let mut expected = ExpectedSet::empty(KIND_COUNT);
    for _ in 0..POSITION_COUNT {
        expected = ExpectedSet::from_positions(&positions, &atom_index, KIND_COUNT)
            .expect("indexed expectation planning succeeds");
    }
    assert_eq!(expected.atoms, vec![(KIND_COUNT - 1) as u32]);

    let legacy_atom_inspections = POSITION_COUNT * KIND_COUNT;
    let indexed_operations = grammar.atoms.len() + POSITION_COUNT;
    assert_eq!(legacy_atom_inspections, 8_388_608);
    assert_eq!(indexed_operations, 6_144);
    assert!(
        legacy_atom_inspections / indexed_operations > 1_000,
        "one grammar-wide build plus direct lookups must replace repeated linear atom scans",
    );
}

pub(super) fn overlapping_rectangles_preserve_full_ordinary_routes() {
    let (_package, plan) = joined_pratt_fixture_from(include_str!(
        "../../tests/fixtures/generation/predictive/src/grammar.rs"
    ));
    let dispatch = plan
        .nodes
        .iter()
        .find(|node| node.ident == "OverlappingRectangles")
        .and_then(|node| {
            node.parse_applications
                .iter()
                .find_map(|application| match &application.body {
                    NodeBodyPlan::Enum { dispatch, .. } => Some(dispatch),
                    NodeBodyPlan::Struct { .. } | NodeBodyPlan::Pratt(_) => None,
                })
        })
        .expect("the predictive fixture contains OverlappingRectangles dispatch");

    assert_eq!(dispatch.frozen_sequence_count, 2);
    assert_eq!(dispatch.frozen_node_count, 6);
    assert_eq!(dispatch.states.len(), 6);
    assert_eq!(
        validate_predictive_dispatch(dispatch, 2).unwrap(),
        6,
        "the four two-token paths have six unique states, not eight expanded occurrences",
    );

    let root = &dispatch.states[dispatch.root.index()];
    assert!(root.target.is_none());
    assert_eq!(root.edges.len(), 3);

    let mut complete_routes = 0;
    let mut terminal_variants = Vec::new();
    let mut suffix_widths = Vec::new();
    for edge in &root.edges {
        let child = &dispatch.states[edge.next.index()];
        assert_eq!(
            child.target, None,
            "ordinary enum dispatch must not commit before checking the complete route and caller FOLLOW",
        );
        suffix_widths.push(child.edges.len());
        for suffix in &child.edges {
            let terminal = &dispatch.states[suffix.next.index()];
            let target = terminal
                .target
                .expect("every complete two-token rectangle route is terminal");
            assert!(terminal.edges.is_empty());
            assert!(
                matches!(target.mode, crate::DispatchTerminalMode::CallerFollow),
                "complete ordinary routes retain the caller-FOLLOW guard before selection",
            );
            complete_routes += 1;
            terminal_variants.push(target.variant);
        }
    }

    suffix_widths.sort_unstable();
    terminal_variants.sort_unstable();
    assert_eq!(suffix_widths, [1, 1, 2]);
    assert_eq!(complete_routes, 4);
    assert_eq!(terminal_variants, [0, 0, 1, 1]);
}

pub(super) fn corrupted_joined_dispatch_fails_before_publication_with_rca9105() {
    let package = tempfile::tempdir().expect("temporary predictive plan fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
recursa::grammar! { module = crate, max_lookahead = 1 }

#[derive(recursa::Node)]
enum Root { A(A), B(B) }

#[derive(recursa::Node)]
#[tok("a")]
struct A;

#[derive(recursa::Node)]
#[tok("b")]
struct B;
"#,
    )
    .expect("write predictive plan fixture");
    let cfg = CargoCfgSnapshot::default();
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze predictive plan fixture");
    let mut plan = EmissionPlan::join_for_test(&success.analyzed, &cfg)
        .expect("join a complete predictive Emission plan");
    assert!(plan.instrumentation.frozen_dispatch_nodes_consumed > 0);
    render(&plan).expect("the renderer consumes every frozen trie node");

    let dispatch = plan
        .nodes
        .iter_mut()
        .flat_map(|node| &mut node.parse_applications)
        .find_map(|application| match &mut application.body {
            NodeBodyPlan::Enum { dispatch, .. } => Some(dispatch),
            NodeBodyPlan::Struct { .. } | NodeBodyPlan::Pratt(_) => None,
        })
        .expect("fixture contains one predictive dispatch");
    dispatch.states[dispatch.root.index()].edges[0]
        .position
        .text_predicate = Some(PlannedTextPredicate::Exact("corrupt".to_owned()));

    let detail = match render(&plan) {
        Err(detail) => detail,
        Ok(_) => panic!("corrupt joined plans cannot reach publication"),
    };
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one pre-publication invariant diagnostic is required");
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
    assert!(!package.path().join(SELECTED_GENERATION).exists());
}

fn joined_pratt_fixture() -> (tempfile::TempDir, EmissionPlan) {
    joined_pratt_fixture_from(include_str!(
        "../../tests/fixtures/generation/pratt/src/grammar.rs"
    ))
}

#[cfg(feature = "diagrams")]
fn joined_pratt_diagram_fixture() -> (tempfile::TempDir, EmissionPlan) {
    let source = include_str!("../../tests/fixtures/generation/pratt/src/grammar.rs").replacen(
        "module = crate::grammar,",
        "module = crate::grammar,\n    diagrams,",
        1,
    );
    joined_pratt_fixture_from(&source)
}

#[cfg(feature = "diagrams")]
pub(super) fn semantic_diagrams_cover_pratt_nodes_content_tokens_and_local_docs() {
    let (_package, plan) = joined_pratt_diagram_fixture();
    let diagrams = plan.diagrams.as_ref().expect("diagram plan");
    assert_eq!(diagrams.nodes.len(), plan.nodes.len());
    assert_eq!(diagrams.tokens.len(), plan.tokens.len());
    assert_eq!(diagrams.tokens.len(), 3);
    assert!(
        diagrams
            .tokens
            .iter()
            .all(|token| token.label == token.pattern)
    );
    let expression = diagrams
        .nodes
        .iter()
        .find(|node| node.ident == "Expr")
        .expect("Pratt diagram");
    let topology = format!("{:?}", expression.production);
    assert!(topology.contains("Choice"));
    assert!(topology.contains("prefix bp=35 Neg"));
    assert!(topology.contains("infix lbp=10 rbp=11 Add"));
    assert!(topology.contains("infix lbp=15 rbp=16 Custom"));
    assert!(topology.contains("postfix bp=40 Factorial"));
    let aggregate = recursa_diagrams::render_aggregate_document(diagrams);
    assert!(aggregate.contains("## Content tokens"));
    for binding in &plan.token_bindings {
        let rendered = render_token_binding(&plan, binding).to_string();
        assert!(
            rendered.contains("cfg_attr (doc , recursa :: __recursa_diagram_doc (token ="),
            "token bindings defer diagram rendering to `cargo doc`:\n{rendered}"
        );
        assert!(!rendered.contains("<svg"));
    }
    let decoded = DiagramPlan::decode(&diagrams.encode()).expect("published plans round-trip");
    assert_eq!(&decoded, diagrams);
    let rendered = render(&plan).expect("render the diagram-enabled plan");
    assert_eq!(
        rendered.diagram_plan.as_deref(),
        Some(diagrams.encode().as_str())
    );
    assert!(
        rendered
            .root
            .contains("cfg_attr (doc , recursa :: __recursa_diagram_doc (aggregate))"),
        "the root defers the aggregate document to `cargo doc`:\n{}",
        rendered.root
    );
    assert!(!rendered.root.contains("<svg"));
    for (index, (name, source)) in rendered.nodes.iter().enumerate() {
        assert!(
            source.contains(&format!(
                "cfg_attr (doc , recursa :: __recursa_diagram_doc (node = {index}usize))"
            )),
            "{name} defers its Node diagram to `cargo doc`:\n{source}"
        );
        assert!(!source.contains("<svg"), "{name} embeds SVG");
    }
}

#[cfg(feature = "diagrams")]
pub(super) fn semantic_diagrams_preserve_generic_applications_and_repetition_topology() {
    let (_package, plan) = joined_pratt_fixture_from(include_str!(
        "../../tests/fixtures/generation/generic-applications/src/grammar.rs"
    ));
    let diagrams = plan.diagrams.as_ref().expect("diagram plan");
    let wrap = diagrams
        .nodes
        .iter()
        .find(|node| node.ident == "Wrap")
        .expect("generic Wrap diagram");
    assert!(matches!(wrap.production, DiagramNode::Choice(_)));
    let separator = diagrams
        .nodes
        .iter()
        .find(|node| node.ident == "Separated")
        .expect("separated repetition diagram");
    let topology = format!("{:?}", separator.production);
    assert!(topology.contains("Repeat"));
    assert!(topology.contains("Terminal(\"and\")"));
    assert!(
        topology.contains("Optional"),
        "Vec is zero-or-more: {topology}"
    );
}

fn joined_pratt_fixture_from(source: &str) -> (tempfile::TempDir, EmissionPlan) {
    let package = tempfile::tempdir().expect("temporary Pratt plan fixture");
    fs::write(package.path().join("grammar.rs"), source).expect("write Pratt plan fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze Pratt plan fixture");
    let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a complete Pratt Emission plan");
    render(&plan).expect("a complete Pratt plan renders");
    (package, plan)
}

fn assert_corrupt_pratt_plan_is_unselectable(package: &Path, plan: &EmissionPlan) {
    let detail = match render(plan) {
        Err(detail) => detail,
        Ok(_) => panic!("corrupt Pratt plans cannot reach publication"),
    };
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one pre-publication invariant diagnostic is required");
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
    assert_eq!(diagnostic.stage(), DiagnosticStage::Emission);
    assert!(!package.join(SELECTED_GENERATION).exists());
}

pub(super) fn corrupted_pratt_branch_follow_and_failure_identities_fail_before_publication() {
    let (package, mut missing_plan) = joined_pratt_fixture();
    let pratt = missing_plan
        .nodes
        .iter_mut()
        .flat_map(|node| &mut node.parse_applications)
        .find_map(|application| match &mut application.body {
            NodeBodyPlan::Pratt(pratt) => Some(pratt),
            NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
        })
        .expect("fixture contains a Pratt plan");
    pratt.nud.clear();
    assert_corrupt_pratt_plan_is_unselectable(package.path(), &missing_plan);

    let (package, mut branch_plan) = joined_pratt_fixture();
    let pratt = branch_plan
        .nodes
        .iter_mut()
        .flat_map(|node| &mut node.parse_applications)
        .find_map(|application| match &mut application.body {
            NodeBodyPlan::Pratt(pratt) => Some(pratt),
            NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
        })
        .expect("fixture contains a Pratt plan");
    let variant_count = pratt.nud.len();
    let target = pratt
        .nud_dispatch
        .states
        .iter_mut()
        .find_map(|node| node.target.as_mut())
        .expect("nud dispatch contains a terminal");
    target.variant = (target.variant + 1) % variant_count;
    assert_corrupt_pratt_plan_is_unselectable(package.path(), &branch_plan);

    let (package, mut follow_plan) = joined_pratt_fixture();
    let pratt = follow_plan
        .nodes
        .iter_mut()
        .flat_map(|node| &mut node.parse_applications)
        .find_map(|application| match &mut application.body {
            NodeBodyPlan::Pratt(pratt) => Some(pratt),
            NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
        })
        .expect("fixture contains a Pratt plan");
    let follow = pratt
        .led
        .iter_mut()
        .flat_map(|branch| &mut branch.variant.elements)
        .find_map(|element| match &mut element.kind {
            ElementParseKind::Semantic { projection, .. } => match projection.as_mut() {
                SemanticParseProjection::Direct {
                    container,
                    pratt_min_bp: Some(_),
                    ..
                } => match container.as_mut() {
                    PlannedContainer::Direct { child_follow, .. } => Some(child_follow),
                    PlannedContainer::Optional { .. } | PlannedContainer::Repeated { .. } => None,
                },
                SemanticParseProjection::Direct { .. }
                | SemanticParseProjection::FixedPresence { .. } => None,
            },
            ElementParseKind::Fixed { .. } | ElementParseKind::OptionalFixedTokenGroup { .. } => {
                None
            }
        })
        .expect("fixture contains a recursive Pratt operand FOLLOW");
    *follow = PlannedFollow::Caller;
    assert_corrupt_pratt_plan_is_unselectable(package.path(), &follow_plan);

    let (package, mut failure_plan) = joined_pratt_fixture();
    let pratt = failure_plan
        .nodes
        .iter_mut()
        .flat_map(|node| &mut node.parse_applications)
        .find_map(|application| match &mut application.body {
            NodeBodyPlan::Pratt(pratt) => Some(pratt),
            NodeBodyPlan::Struct { .. } | NodeBodyPlan::Enum { .. } => None,
        })
        .expect("fixture contains a Pratt plan");
    pratt.nud_failure = PlannedFollow::Caller;
    assert_corrupt_pratt_plan_is_unselectable(package.path(), &failure_plan);
}

pub(super) fn corrupted_real_plan_family_content_fails_before_publication() {
    let (package, mut traits) = joined_pratt_fixture();
    traits.nodes[0]
        .configured_traits
        .pop()
        .expect("the trait fixture configures recipes");
    assert_plan_family_is_unselectable(package.path(), &traits, "trait-behavior");

    let (package, mut private_abi) = joined_pratt_fixture();
    private_abi.private_abi.protocol += 1;
    assert_plan_family_is_unselectable(package.path(), &private_abi, "private ABI");

    let (package, mut capabilities) = joined_pratt_fixture();
    capabilities
        .capabilities
        .pop()
        .expect("the fixture accepts at least one exact facade capability");
    assert_plan_family_is_unselectable(package.path(), &capabilities, "capability");

    let (package, mut pretty) = joined_pratt_fixture();
    let element = pretty
        .pretty
        .nodes
        .iter_mut()
        .flatten()
        .flat_map(|node| &mut node.variants)
        .flat_map(|variant| variant.iter_mut())
        .next()
        .expect("the trait fixture contains Pretty behavior");
    match element {
        PrettyElementBehavior::Fixed { spelling, .. } => spelling.push_str("-corrupt"),
        PrettyElementBehavior::Semantic { projection } => match projection {
            PrettySemanticProjection::Direct { repetition, .. } => {
                *repetition = Some(PrettyRepetitionBehavior {
                    separator: None,
                    trailing_separator: false,
                    hard_break: false,
                    leading_separator: false,
                    space_before_separator: false,
                    space_after_separator: true,
                });
            }
            PrettySemanticProjection::FixedPresence { .. } => {
                panic!(
                    "the selected fixture starts with ordinary syntax or a direct semantic field"
                )
            }
        },
        PrettyElementBehavior::OptionalFixedTokenGroup { .. } => {
            panic!("the selected fixture starts with ordinary syntax or a semantic field")
        }
    }
    assert_plan_family_is_unselectable(package.path(), &pretty, "Pretty");

    let (package, mut attachment) = joined_pratt_fixture();
    let fixed = attachment
        .pretty
        .nodes
        .iter_mut()
        .flatten()
        .flat_map(|node| &mut node.variants)
        .flat_map(|variant| variant.iter_mut())
        .find_map(|element| match element {
            PrettyElementBehavior::Semantic {
                projection:
                    PrettySemanticProjection::Direct {
                        attachment: Some(attachment),
                        ..
                    },
            } => attachment.before.first_mut(),
            PrettyElementBehavior::Fixed { .. }
            | PrettyElementBehavior::OptionalFixedTokenGroup { .. }
            | PrettyElementBehavior::Semantic { .. } => None,
        })
        .expect("the Pratt fixture contains a fixed attachment");
    let PrettyAttachedSyntaxBehavior::Required(fixed) = fixed else {
        panic!("the Pratt fixture attachment is required")
    };
    fixed.spelling.push_str("-corrupt");
    assert_plan_family_is_unselectable(package.path(), &attachment, "Pretty");

    #[cfg(feature = "diagrams")]
    {
        let (package, mut diagram) = joined_pratt_diagram_fixture();
        diagram
            .diagrams
            .as_mut()
            .expect("diagram request creates emitted content")
            .nodes[0]
            .production = DiagramNode::Comment("corrupt".to_owned());
        assert_plan_family_is_unselectable(package.path(), &diagram, "diagram");
    }
    #[cfg(not(feature = "diagrams"))]
    {
        let (_, plan) = joined_pratt_fixture();
        assert_eq!(plan.plan_families.diagrams, None);
    }

    #[cfg(feature = "explorer")]
    {
        let (package, mut explorer) = joined_pratt_fixture();
        explorer
            .explorer
            .as_mut()
            .expect("Visit fixture has an explorer plan")
            .roots
            .first_mut()
            .expect("Visit fixture has an explorer root")
            .capabilities
            .pretty ^= true;
        assert_plan_family_is_unselectable(package.path(), &explorer, "explorer");

        let (package, mut explorer_artifact) = joined_pratt_fixture();
        explorer_artifact
            .explorer
            .as_mut()
            .expect("Visit fixture has an extension emission plan")
            .artifacts
            .push(ExtensionArtifactPlan {
                logical_name: "grammar-index".to_owned(),
                content: "corrupt explorer artifact".to_owned(),
            });
        assert_plan_family_is_unselectable(package.path(), &explorer_artifact, "explorer");

        let (package, mut explorer_schema) = joined_pratt_fixture();
        explorer_schema
            .explorer
            .as_mut()
            .expect("Visit fixture has an explorer plan")
            .nodes
            .first_mut()
            .expect("Visit fixture has an explorer Node schema")
            .body = ExplorerNodeBodyPlan::Struct(ExplorerVariantPlan {
            variant: None,
            name: None,
            qualified_name: None,
            elements: Vec::new(),
        });
        assert_plan_family_is_unselectable(package.path(), &explorer_schema, "explorer");
    }
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_plan_freezes_the_complete_typed_static_schema() {
    let (_, plan) = joined_pratt_fixture();
    let explorer = plan
        .explorer
        .as_ref()
        .expect("Visit fixture has an explorer plan");

    assert_eq!(explorer.grammar.name, plan.grammar_name);
    assert_eq!(explorer.tokens.len(), plan.kind_count - 1);
    assert!(explorer.tokens.iter().any(|token| {
        token.canonical_text.as_deref() == Some("+")
            && token.kind == ExplorerTokenKindPlan::Fixed
            && token.highlight == ExplorerHighlightClassPlan::Punctuation
    }));
    assert!(explorer.tokens.iter().any(|token| {
        token.canonical_text.is_none()
            && token.kind == ExplorerTokenKindPlan::Content
            && token.highlight == ExplorerHighlightClassPlan::Content
    }));

    let node = explorer
        .nodes
        .iter()
        .find(|node| matches!(node.body, ExplorerNodeBodyPlan::Pratt(_)))
        .expect("Pratt application has a Node descriptor");
    assert!(
        explorer
            .roots
            .iter()
            .any(|root| root.application == node.application)
    );
    assert!(node.qualified_name.ends_with(&format!("::{}", node.name)));
    let variants = match &node.body {
        ExplorerNodeBodyPlan::Pratt(variants) => variants,
        ExplorerNodeBodyPlan::Struct(_) | ExplorerNodeBodyPlan::Enum(_) => {
            panic!("Pratt root keeps its complete variant table")
        }
    };
    assert!(variants.iter().all(|variant| {
        variant
            .qualified_name
            .as_ref()
            .is_some_and(|qualified| qualified.starts_with(&node.qualified_name))
    }));
    assert!(
        variants
            .iter()
            .flat_map(|variant| &variant.elements)
            .any(|element| matches!(element.target, Some(ExplorerElementTargetPlan::Node(_))))
    );
    let all_variants = explorer.nodes.iter().flat_map(|node| match &node.body {
        ExplorerNodeBodyPlan::Struct(variant) => std::slice::from_ref(variant),
        ExplorerNodeBodyPlan::Enum(variants) | ExplorerNodeBodyPlan::Pratt(variants) => {
            variants.as_slice()
        }
    });
    assert!(
        all_variants
            .clone()
            .flat_map(|variant| &variant.elements)
            .any(|element| matches!(
                element.target,
                Some(ExplorerElementTargetPlan::ContentToken(_))
            ))
    );
    assert!(
        all_variants
            .flat_map(|variant| &variant.elements)
            .any(|element| element.target.is_none() && element.token_kind.is_some())
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_rendering_consumes_its_frozen_schema_plan() {
    let (_, mut plan) = joined_pratt_fixture();
    let explorer = plan
        .explorer
        .as_mut()
        .expect("Visit fixture has an explorer plan");
    explorer.nodes[0].name = "FrozenExplorerSchema".to_owned();
    let element = explorer
        .nodes
        .iter_mut()
        .flat_map(|node| match &mut node.body {
            ExplorerNodeBodyPlan::Struct(variant) => std::slice::from_mut(variant),
            ExplorerNodeBodyPlan::Enum(variants) | ExplorerNodeBodyPlan::Pratt(variants) => {
                variants.as_mut_slice()
            }
        })
        .flat_map(|variant| &mut variant.elements)
        .find(|element| matches!(element.display, ExplorerDisplayPlan::Fixed(_)))
        .expect("fixture contains fixed explorer syntax");
    element.name = "frozen_element".to_owned();
    element.cardinality = ExplorerCardinalityPlan::OptionalOneOrMore;
    explorer
        .tokens
        .iter_mut()
        .find(|token| token.canonical_text.is_some())
        .expect("fixture contains a fixed Token descriptor")
        .canonical_text = Some("FROZEN_DISPLAY".to_owned());
    plan.plan_families.explorer = Some(plan_family_identity(explorer));

    let rendered = render(&plan).expect("self-consistent frozen explorer plan renders");
    let generated = std::iter::once(rendered.root.as_str())
        .chain(rendered.nodes.iter().map(|(_, source)| source.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    for frozen in [
        "FrozenExplorerSchema",
        "frozen_element",
        "OptionalOneOrMore",
        "FROZEN_DISPLAY",
    ] {
        assert!(
            generated.contains(frozen),
            "rendered explorer content must consume frozen decision {frozen}"
        );
    }
    assert!(
        generated.contains("Option < recursa :: __private :: ExplorerSampleFn >"),
        "capable roots must name the runtime sample callback alias instead of emitting a lint-prone nested function type"
    );
    assert!(
        generated.contains("explorer_sample_root :: <"),
        "capable roots must instantiate the shared generic sample path"
    );
    assert!(
        !generated.contains("Option < fn (& [u8]) -> Result"),
        "generated adapter bindings must not restore the type-complexity shape"
    );

    let schema_source = render_explorer_static_schema(
        &plan,
        plan.explorer
            .as_ref()
            .expect("Visit fixture has an explorer plan"),
    )
    .to_string();
    assert!(
        !schema_source.contains("'static"),
        "static schema aggregate references must use their inferred static lifetime"
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_output_omits_only_redundant_lifetimes() {
    let (_, plan) = joined_pratt_fixture();
    let explorer = plan
        .explorer
        .as_ref()
        .expect("Visit fixture has an explorer plan");
    let application_id = explorer
        .roots
        .first()
        .expect("Visit fixture has an explorer root")
        .application;
    let (node, application) = explorer_application_parse_plan(&plan.nodes, application_id);

    let mut lifetime_free = application.clone();
    lifetime_free.ty = syn::parse_quote!(LifetimeFree);
    let lifetime_free_source =
        render_node_application_explorer(&plan, explorer, node, &lifetime_free).to_string();
    assert!(
        !lifetime_free_source.contains("impl < '__recursa_input >"),
        "lifetime-free adapters must not emit an unconstrained input lifetime"
    );

    let mut borrowed = application.clone();
    borrowed.ty = syn::parse_quote!(Borrowed<'__recursa_input>);
    let borrowed_source =
        render_node_application_explorer(&plan, explorer, node, &borrowed).to_string();
    assert!(
        borrowed_source.contains("impl < '__recursa_input >"),
        "borrowed adapters must retain the input lifetime used by their application type"
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_generation_supports_semantic_targets_sharing_a_base_kind() {
    let shared_pattern_pair = r#"
recursa::grammar! {
    module = crate,
    spans = mandatory,
    framing(island = Item, boundary = SEMI),
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\n\r]+",
    }
    punctuation {
        SEMI => ";",
    }
}

#[derive(recursa::Node)]
pub struct Item<'input> {
    #[lex(pattern = r"[a-z]+")]
    pub first: FirstWord<'input>,
    #[lex(pattern = r"[a-z]+")]
    pub second: SecondWord<'input>,
}
"#;
    let keyword_admission_trio = r#"
recursa::grammar! {
    module = crate,
    spans = mandatory,
    framing(island = Item, boundary = SEMI),
}

recursa::tokens! {
    trivia {
        Whitespace => r"[ \t\n\r]+",
    }
    categories { UNRESERVED, RESERVED }
    keywords {
        SELECT => "select" in RESERVED,
        WINDOW => "window" in UNRESERVED,
    }
    admissions {
        NonReservedWord = UNRESERVED,
        ColId = NonReservedWord + RESERVED,
    }
    punctuation {
        SEMI => ";",
    }
}

#[derive(recursa::Node)]
pub struct Item<'input> {
    #[lex(pattern = r"[a-z]+", admits(ColId))]
    pub name: ColIdWord<'input>,
    #[lex(pattern = r"[a-z]+", admits(NonReservedWord))]
    pub label: NonReservedWordText<'input>,
    #[lex(pattern = r"[a-z]+")]
    pub bare: BareWord<'input>,
}
"#;
    for (label, source) in [
        ("two shared-pattern targets", shared_pattern_pair),
        ("three keyword-admission targets", keyword_admission_trio),
    ] {
        let package = tempfile::tempdir().expect("temporary shared-base-kind fixture");
        fs::write(package.path().join("grammar.rs"), source)
            .expect("write shared-base-kind fixture");
        let success = analyze_source_tree(
            SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .unwrap_or_else(|report| panic!("analyze the {label} fixture: {report:?}"));
        let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
            .unwrap_or_else(|detail| panic!("join the {label} plan: {detail}"));
        render(&plan).unwrap_or_else(|detail| panic!("render the {label} plan: {detail}"));
    }
}

#[cfg(feature = "explorer")]
fn compound_fixture_join_error(source: String) -> String {
    let package = tempfile::tempdir().expect("temporary compound plan fixture");
    fs::write(package.path().join("grammar.rs"), source).expect("write compound plan fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("collision fixture reaches emission planning");
    match EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default()) {
        Ok(_) => panic!("the authored name must collide with generated output"),
        Err(detail) => detail,
    }
}

#[cfg(feature = "explorer")]
fn compound_fixture_source() -> String {
    include_str!("../../tests/fixtures/generation/compound-nodes/src/grammar.rs").to_owned()
}

#[cfg(feature = "explorer")]
pub(super) fn view_collisions_are_rejected_at_the_emission_plan_seam() {
    let imported = compound_fixture_source().replace(
        "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        "mod authored { pub struct View; }\nuse authored::View;\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
    );
    let detail = compound_fixture_join_error(imported);
    assert!(
        detail.contains("generated view type `View` collides with an authored import"),
        "{detail}"
    );

    let token_binding =
        compound_fixture_source().replace("pub text: CountText<'input>", "pub text: View<'input>");
    let detail = compound_fixture_join_error(token_binding);
    assert!(
        detail
            .contains("generated view type `View` collides with a generated content-token binding"),
        "{detail}"
    );
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_value_collisions_are_rejected_at_the_emission_plan_seam() {
    for (source, name) in [
        (
            format!("{}\nstatic __RECURSA_EXPLORER_GRAMMAR: () = ();\n", compound_fixture_source()),
            "__RECURSA_EXPLORER_GRAMMAR",
        ),
        (
            compound_fixture_source().replace(
                "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
                "static __RECURSA_EXPLORER_NODE_SCHEMAS: () = ();\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            ),
            "__RECURSA_EXPLORER_NODE_SCHEMAS",
        ),
        (
            format!(
                "{}\nmod collision {{ pub fn value() {{}} }}\nuse collision::value as __RECURSA_EXPLORER_SCHEMA_0;\n",
                compound_fixture_source()
            ),
            "__RECURSA_EXPLORER_SCHEMA_0",
        ),
        (
            compound_fixture_source().replace(
                "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
                "struct __RECURSA_EXPLORER_SCHEMA_0;\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            ),
            "__RECURSA_EXPLORER_SCHEMA_0",
        ),
    ] {
        let detail = compound_fixture_join_error(source);
        assert!(detail.contains(&format!("generated explorer item `{name}` collides")), "{detail}");
    }
}

#[cfg(feature = "explorer")]
pub(super) fn explorer_method_collisions_are_rejected_at_the_emission_plan_seam() {
    let root = format!(
        "{}\nimpl<'input> Record<'input> {{ pub fn explorer() {{}} }}\n",
        compound_fixture_source()
    );
    let detail = compound_fixture_join_error(root);
    assert!(
        detail.contains("generated explorer entry method `Record::explorer` collides"),
        "{detail}"
    );

    let descendant = compound_fixture_source().replace(
        "    pub struct Duplicate;\n}\n\npub mod public_right",
        "    pub struct Duplicate;\n\n    impl<'input> super::Record<'input> { pub fn explorer() {} }\n}\n\npub mod public_right",
    );
    let detail = compound_fixture_join_error(descendant);
    assert!(
        detail.contains("generated explorer entry method `Record::explorer` collides"),
        "{detail}"
    );
}

pub(super) fn authored_pretty_authority_rejects_emitted_declaration_corruption() {
    let package = tempfile::tempdir().expect("temporary Pretty authority fixture");
    fs::write(
        package.path().join("grammar.rs"),
        include_str!("../../tests/fixtures/generation/pratt/src/grammar.rs"),
    )
    .expect("write Pretty authority fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze Pretty authority fixture");
    let lowered = &success.analyzed.lowered;
    let authority = authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
        .expect("derive Pretty authority from source");
    let mut plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a complete Pretty plan");
    let element = plan
        .nodes
        .iter_mut()
        .flat_map(|node| match &mut node.body {
            NodeBodyPlan::Struct { elements, .. } => elements.iter_mut().collect::<Vec<_>>(),
            NodeBodyPlan::Enum { variants, .. } => variants
                .iter_mut()
                .flat_map(|variant| variant.elements.iter_mut())
                .collect(),
            NodeBodyPlan::Pratt(pratt) => pratt
                .nud
                .iter_mut()
                .flat_map(|branch| branch.variant.elements.iter_mut())
                .chain(
                    pratt
                        .led
                        .iter_mut()
                        .flat_map(|branch| branch.variant.elements.iter_mut()),
                )
                .collect(),
        })
        .find(|element| matches!(element.kind, ElementParseKind::Semantic { .. }))
        .expect("Pretty fixture contains a semantic declaration element");
    let ElementParseKind::Semantic { projection, .. } = &mut element.kind else {
        unreachable!("selected a semantic element")
    };
    let SemanticParseProjection::Direct {
        opaque_provenance, ..
    } = projection.as_mut()
    else {
        unreachable!("selected fixture semantic field is direct")
    };
    *opaque_provenance = !*opaque_provenance;
    plan.pretty = emitted_pretty_plan(&plan.nodes, &plan.tokens);

    let authority_after_corruption =
        authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
            .expect("source authority remains derivable");
    assert_eq!(authority, authority_after_corruption);
    assert_ne!(Some(plan.pretty.clone()), authority);
    assert!(
        plan.validate()
            .expect_err("corrupt emitted Pretty plan must diverge")
            .contains("Pretty plan family")
    );
}

pub(super) fn pretty_pratt_recursion_uses_typed_node_identity_not_the_final_path_segment() {
    let package = tempfile::tempdir().expect("temporary typed Pratt Pretty fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
                recursa::grammar! {
                    module = crate::grammar,
                    derives(Pretty),
                    keyword_matching = sensitive,
                    max_lookahead = 1,
                }
                recursa::tokens! {
                    ignore = r"[ \t\n\r]+",
                    punctuation { OPEN => "(", CLOSE => ")", PLUS => "+" }
                }
                pub mod foreign {
                    #[derive(recursa::Node)]
                    pub struct Expr<'input> {
                        #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
                        pub value: ForeignExprText<'input>,
                    }
                }
                #[derive(recursa::Node)]
                pub struct Number<'input> {
                    #[lex(pattern = r"[0-9]+")]
                    pub value: NumberText<'input>,
                }
                #[derive(recursa::Node)]
                #[pratt]
                pub enum Expr<'input, T> {
                    Foreign(foreign::Expr<'input>),
                    Generic(T),
                    Group(#[tok(OPEN, this, CLOSE)] Box<Self>),
                    #[parse(infix, lbp = 10, rbp = 11)]
                    #[tok(PLUS)]
                    Add(Box<Self>, Box<Self>),
                }
                #[derive(recursa::Node)]
                pub struct Root<'input> {
                    pub expression: Expr<'input, Number<'input>>,
                }
            "#,
    )
    .expect("write typed Pratt Pretty fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze typed Pratt Pretty fixture");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join typed Pratt Pretty fixture");
    let index = plan
        .nodes
        .iter()
        .position(|node| node.semantic_key == "grammar::Expr")
        .expect("fixture has the owning Pratt Expr");
    let behavior = plan.pretty.nodes[index]
        .as_ref()
        .expect("owning Expr has Pretty behavior");
    let pratt = behavior
        .pratt
        .as_ref()
        .expect("Expr has typed Pratt behavior");

    assert_eq!(
        pratt.variants[0].operands,
        vec![PrettyPrattOperandBehavior::Ordinary],
        "foreign::Expr must not become recursive merely because its final ident matches",
    );
    assert_eq!(
        pratt.variants[1].operands,
        vec![PrettyPrattOperandBehavior::Ordinary]
    );
    assert!(matches!(
        pratt.variants[2].operands.as_slice(),
        [PrettyPrattOperandBehavior::Recursive { owner, applications, .. }]
            if *owner == behavior.node && applications == &behavior.applications
    ));
    assert!(matches!(
        pratt.variants[3].operands.as_slice(),
        [
            PrettyPrattOperandBehavior::Recursive { .. },
            PrettyPrattOperandBehavior::Recursive { .. }
        ]
    ));
    assert!(
        pratt.grouping.is_some(),
        "typed Self atom supplies grouping"
    );
}

pub(super) fn pretty_pratt_variants_are_typechecked_in_isolated_helpers() {
    let (_package, plan) = joined_pratt_fixture();
    let rendered = render(&plan).expect("the Pratt fixture renders");
    let expr_index = plan
        .nodes
        .iter()
        .position(|node| node.ident == "Expr")
        .expect("the fixture contains its Pratt Expr");
    let variant_count = plan.pretty.nodes[expr_index]
        .as_ref()
        .and_then(|behavior| behavior.pratt.as_ref())
        .expect("Expr has typed Pratt Pretty behavior")
        .variants
        .len();
    let expr = &rendered.nodes[expr_index].1;

    assert_eq!(
        expr.matches("fn __recursa_pretty_pratt_variant_").count(),
        variant_count,
        "each Pratt Pretty variant must have a separately typechecked function item",
    );
    assert_eq!(
        expr.matches("self . __recursa_pretty_pratt_variant_")
            .count(),
        variant_count,
        "the compact Pratt Pretty dispatcher must call every isolated variant helper",
    );
}

pub(super) fn pretty_pratt_grouping_follows_a_unique_authored_helper_path() {
    let (_package, plan) = joined_pratt_fixture_from(
        r#"
recursa::grammar! { module = crate, derives(Pretty) }
recursa::tokens! {
    ignore = r"[ \t\r\n]+",
    keywords { ANY => "any", QUERY => "query" }
    punctuation { OPEN => "(", CLOSE => ")", COMMA => ",", DOT => ".", STAR => "*", PLUS => "+" }
}

#[derive(recursa::Node)]
struct Integer<'input> {
    #[lex(pattern = r"[0-9]+")]
    value: IntegerText<'input>,
}

#[derive(recursa::Node)]
enum ParenthesizedSuffix {
    #[tok(DOT, STAR)]
    Star,
}

#[derive(recursa::Node)]
enum ParenthesizedOpen {
    #[tok(OPEN)]
    Value,
}

#[derive(recursa::Node)]
enum ParenthesizedClose {
    #[tok(CLOSE)]
    Value,
}

#[derive(recursa::Node)]
#[tok(QUERY)]
struct Query<'input> {
    value: Box<Expr<'input>>,
}

#[derive(recursa::Node)]
enum ParenthesizedContent<'input> {
    Exprs(#[sep(COMMA)] recursa::Vec1<Expr<'input>>),
    Query(Query<'input>),
}

#[derive(recursa::Node)]
struct ParenthesizedExpr<'input> {
    open: ParenthesizedOpen,
    content: ParenthesizedContent<'input>,
    close: ParenthesizedClose,
    suffix: Option<ParenthesizedSuffix>,
}

#[derive(recursa::Node)]
struct QuantifiedExpr<'input> {
    #[tok(ANY, OPEN, this, CLOSE)]
    content: ParenthesizedContent<'input>,
}

#[derive(recursa::Node)]
#[pratt]
enum Expr<'input> {
    Integer(Integer<'input>),
    Quantified(QuantifiedExpr<'input>),
    Parenthesized(ParenthesizedExpr<'input>),
    #[parse(infix, lbp = 10, rbp = 11)]
    #[tok(PLUS)]
    Add(Box<Self>, Box<Self>),
}
"#,
    );
    let index = plan
        .nodes
        .iter()
        .position(|node| node.semantic_key == "Expr")
        .expect("fixture has the owning Pratt Expr");
    let grouping = plan.pretty.nodes[index]
        .as_ref()
        .and_then(|behavior| behavior.pratt.as_ref())
        .and_then(|pratt| pratt.grouping.as_ref())
        .expect("the helper path supplies Pratt grouping syntax");

    assert_eq!(
        grouping
            .before
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        ["("]
    );
    assert_eq!(
        grouping
            .after
            .iter()
            .map(|fixed| fixed.spelling.as_str())
            .collect::<Vec<_>>(),
        [")"]
    );
}

pub(super) fn every_typed_pretty_pratt_decision_is_part_of_family_authority() {
    let (package, plan) = joined_pratt_fixture();

    fn behavior(plan: &mut EmissionPlan) -> &mut PrettyNodeBehavior {
        let index = plan
            .nodes
            .iter()
            .position(|node| node.ident == "Expr")
            .expect("fixture has Expr");
        plan.pretty.nodes[index].as_mut().expect("Expr has Pretty")
    }
    fn pratt(plan: &mut EmissionPlan) -> &mut PrettyPrattBehavior {
        behavior(plan).pratt.as_mut().expect("Expr is Pratt")
    }
    fn reject(package: &Path, mut plan: EmissionPlan, corrupt: impl FnOnce(&mut EmissionPlan)) {
        corrupt(&mut plan);
        assert_plan_family_is_unselectable(package, &plan, "Pretty");
    }

    reject(package.path(), plan.clone(), |plan| {
        behavior(plan).node = NodeId::next(999);
    });
    reject(package.path(), plan.clone(), |plan| {
        behavior(plan).applications.clear();
    });
    reject(package.path(), plan.clone(), |plan| {
        let grouping = pratt(plan).grouping.as_mut().expect("grouping");
        assert_eq!(grouping.before.len(), 2);
        grouping.before[1].spelling.push('!');
    });
    reject(package.path(), plan.clone(), |plan| {
        let grouping = pratt(plan).grouping.as_mut().expect("grouping");
        assert_eq!(grouping.after.len(), 2);
        grouping.after.remove(0);
    });
    reject(package.path(), plan.clone(), |plan| {
        pratt(plan).grouping.as_mut().expect("grouping").after[1].rendering =
            PrettyFixedRendering::Punctuation;
    });
    reject(package.path(), plan.clone(), |plan| {
        pratt(plan).variants[0].parenthesization.entry_lbp = Some(1);
    });
    reject(package.path(), plan.clone(), |plan| {
        pratt(plan).variants[0].parenthesization.exit_rbp = 1;
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = pratt(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| matches!(operand, PrettyPrattOperandBehavior::Recursive { .. }))
            .expect("recursive operand");
        let PrettyPrattOperandBehavior::Recursive { owner, .. } = recursive else {
            unreachable!()
        };
        *owner = NodeId::next(999);
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = pratt(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| matches!(operand, PrettyPrattOperandBehavior::Recursive { .. }))
            .expect("recursive operand");
        let PrettyPrattOperandBehavior::Recursive { applications, .. } = recursive else {
            unreachable!()
        };
        applications.clear();
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = pratt(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| matches!(operand, PrettyPrattOperandBehavior::Recursive { .. }))
            .expect("recursive operand");
        let PrettyPrattOperandBehavior::Recursive { minimum, .. } = recursive else {
            unreachable!()
        };
        *minimum = PrettyPrattMinimum::Binding(255);
    });
    reject(package.path(), plan.clone(), |plan| {
        let recursive = pratt(plan)
            .variants
            .iter_mut()
            .flat_map(|variant| &mut variant.operands)
            .find(|operand| matches!(operand, PrettyPrattOperandBehavior::Recursive { .. }))
            .expect("recursive operand");
        let PrettyPrattOperandBehavior::Recursive { next, .. } = recursive else {
            unreachable!()
        };
        *next = PrettyPrattNext::Binding(255);
    });
    reject(package.path(), plan.clone(), |plan| {
        let fixed = behavior(plan)
            .variants
            .iter_mut()
            .flatten()
            .find(|element| matches!(element, PrettyElementBehavior::Fixed { .. }))
            .expect("fixed Pretty element");
        let PrettyElementBehavior::Fixed {
            soft_break_before, ..
        } = fixed
        else {
            unreachable!()
        };
        *soft_break_before = !*soft_break_before;
    });
    reject(package.path(), plan.clone(), |plan| {
        let fixed = behavior(plan)
            .variants
            .iter_mut()
            .flatten()
            .find(|element| matches!(element, PrettyElementBehavior::Fixed { .. }))
            .expect("fixed Pretty element");
        let PrettyElementBehavior::Fixed {
            soft_break_after, ..
        } = fixed
        else {
            unreachable!()
        };
        *soft_break_after = !*soft_break_after;
    });
    reject(package.path(), plan, |plan| {
        let semantic = behavior(plan)
            .variants
            .iter_mut()
            .flatten()
            .find(|element| matches!(element, PrettyElementBehavior::Semantic { .. }))
            .expect("semantic Pretty element");
        let PrettyElementBehavior::Semantic {
            projection: PrettySemanticProjection::Direct { provenance, .. },
        } = semantic
        else {
            unreachable!()
        };
        *provenance = PrettyProvenanceBehavior::Opaque;
    });
}

pub(super) fn recursive_concrete_pretty_impls_do_not_publish_cyclic_child_bounds() {
    let package = tempfile::tempdir().expect("temporary recursive Pretty fixture");
    let source = include_str!("../../tests/fixtures/source-analysis/json/grammar.rs").replacen(
        "recursa::grammar! {",
        "recursa::grammar! {\n    derives(Pretty),",
        1,
    );
    fs::write(package.path().join("grammar.rs"), source).expect("write recursive Pretty fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze recursive Pretty fixture");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join recursive Pretty fixture");
    let rendered = render(&plan).expect("render recursive Pretty fixture");
    let nodes = rendered
        .nodes
        .iter()
        .map(|(_, node)| node.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(nodes.contains("recursa :: Pretty for Value"));
    for concrete in ["Value", "Object", "Member", "Array"] {
        assert!(
            !nodes.contains(&format!("recursa :: Pretty for {concrete} where")),
            "concrete {concrete} Pretty impl must not carry child bounds",
        );
    }
}

pub(super) fn recursive_concrete_visit_impls_do_not_publish_cyclic_child_bounds() {
    let package = tempfile::tempdir().expect("temporary recursive Visit fixture");
    let source = include_str!("../../tests/fixtures/source-analysis/json/grammar.rs").replacen(
        "recursa::grammar! {",
        "recursa::grammar! {\n    derives(Visit),",
        1,
    );
    fs::write(package.path().join("grammar.rs"), source).expect("write recursive Visit fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze recursive Visit fixture");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join recursive Visit fixture");
    let nodes = render(&plan)
        .expect("render recursive Visit fixture")
        .nodes
        .into_iter()
        .map(|(_, node)| node)
        .collect::<Vec<_>>()
        .join("\n");

    for cyclic_child in ["Value", "Object", "Member", "Array"] {
        assert!(
            !nodes.contains(&format!(
                "crate :: grammar :: {cyclic_child} < 'input > : recursa :: Visit"
            )),
            "recursive child {cyclic_child} must not become a Visit where-bound",
        );
    }
}

pub(super) fn visit_bodies_use_the_runtime_step_and_child_list_macros() {
    let source =
        include_str!("../../tests/fixtures/generation/generic-applications/src/grammar.rs")
            .replacen("    diagrams,\n", "", 1)
            .replacen(
                "PartialEq, Pretty, serde::Serialize",
                "PartialEq, Pretty, Visit, VisitMut, serde::Serialize",
                1,
            );
    let (_package, plan) = joined_pratt_fixture_from(&source);
    let render_visit = |ident: &str| {
        let node = plan
            .nodes
            .iter()
            .find(|node| node.ident == ident)
            .unwrap_or_else(|| panic!("generic fixture contains {ident}"));
        format!(
            "{}\n{}",
            render_node_visit(&plan, node, false),
            render_node_visit(&plan, node, true),
        )
    };
    let single = render_visit("KeywordName");
    let pair = render_visit("Pair");

    assert!(single.contains("__private :: visit_node ! (visitor ,"));
    assert!(single.contains("__private :: visit_node_mut ! (visitor ,"));
    assert!(
        !single.contains("total_enter"),
        "the enter/exit plumbing lives in the runtime macro"
    );
    assert!(
        single.contains("recursa :: Visit :: visit (& self . value , visitor) ? ;"),
        "a single child stays a direct call: {single}"
    );
    assert!(
        pair.contains("visit_children ! (ref self visitor ; left , right) ;"),
        "two children render as one compact list: {pair}"
    );
    assert!(pair.contains("visit_children ! (mut self visitor ; left , right) ;"));
}

pub(super) fn visit_bounds_only_cover_node_type_parameters() {
    let source =
        include_str!("../../tests/fixtures/generation/generic-applications/src/grammar.rs")
            .replacen("    diagrams,\n", "", 1)
            .replacen(
                "PartialEq, Pretty, serde::Serialize",
                "PartialEq, Pretty, Visit, VisitMut, serde::Serialize",
                1,
            )
            .replacen(
                "pub struct Pair<'left, 'right>",
                "#[node(omit(Visit, VisitMut))]\npub struct Pair<'left, 'right>",
                1,
            );
    let (_package, plan) = joined_pratt_fixture_from(&source);
    let render_visit = |ident: &str| {
        let node = plan
            .nodes
            .iter()
            .find(|node| node.ident == ident)
            .unwrap_or_else(|| panic!("generic fixture contains {ident}"));
        format!(
            "{}\n{}",
            render_node_visit(&plan, node, false),
            render_node_visit(&plan, node, true),
        )
    };
    let generic = render_visit("LifetimeNode");
    let concrete = render_visit("GenericKeywordRoot");
    let pair = render_visit("Pair");
    let audit = render_visit("AuditRoot");

    assert!(concrete.contains("recursa :: Visit for GenericKeywordRoot"));
    assert!(generic.contains("T : recursa :: Visit"));
    assert!(generic.contains("T : recursa :: VisitMut"));
    assert!(pair.contains("Name < 'right > : recursa :: Visit"));
    assert!(pair.contains("Name < 'right > : recursa :: VisitMut"));
    assert!(audit.contains("Pair < 'outer , 'outer > : recursa :: Visit"));
    assert!(audit.contains("Pair < 'outer , 'outer > : recursa :: VisitMut"));
    assert!(
        !concrete
            .contains("Wrap < crate :: grammar :: KeywordName < 'input > > : recursa :: Visit"),
        "concrete lifetime-bearing child types resolve through their generated Visit impl",
    );
    assert!(
        !concrete
            .contains("Wrap < crate :: grammar :: KeywordName < 'input > > : recursa :: VisitMut"),
        "concrete lifetime-bearing child types resolve through their generated VisitMut impl",
    );
}

pub(super) fn every_local_pretty_hint_dimension_is_part_of_family_authority() {
    let source = include_str!("../../tests/fixtures/generation/repetitions/src/grammar.rs.in")
        .replace("__RECURSA_SPANS_MODE__", "off");
    let package = tempfile::tempdir().expect("temporary Pretty hint authority fixture");
    fs::write(package.path().join("grammar.rs"), source)
        .expect("write Pretty hint authority fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze Pretty hint authority fixture");
    let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join a complete Pretty hint plan");

    fn pretty_node<'a>(plan: &'a mut EmissionPlan, ident: &str) -> &'a mut PrettyNodeBehavior {
        let index = plan
            .nodes
            .iter()
            .position(|node| node.ident == ident)
            .unwrap_or_else(|| panic!("fixture contains {ident}"));
        plan.pretty.nodes[index]
            .as_mut()
            .unwrap_or_else(|| panic!("{ident} has Pretty behavior"))
    }
    fn semantic(node: &mut PrettyNodeBehavior) -> &mut PrettyElementBehavior {
        node.variants
            .iter_mut()
            .flatten()
            .find_map(|element| match element {
                PrettyElementBehavior::Semantic {
                    projection: PrettySemanticProjection::Direct { .. },
                } => Some(element),
                PrettyElementBehavior::Fixed { .. }
                | PrettyElementBehavior::OptionalFixedTokenGroup { .. }
                | PrettyElementBehavior::Semantic { .. } => None,
            })
            .expect("selected fixture node has a semantic field")
    }
    fn repetition_behavior(node: &mut PrettyNodeBehavior) -> &mut PrettyRepetitionBehavior {
        let PrettyElementBehavior::Semantic {
            projection:
                PrettySemanticProjection::Direct {
                    repetition: Some(repetition),
                    ..
                },
        } = semantic(node)
        else {
            panic!("selected fixture node has a repeated semantic field")
        };
        repetition
    }

    let mut node_hint = plan.clone();
    assert!(!pretty_node(&mut node_hint, "NodeGroupPretty").pretty.indent);
    pretty_node(&mut node_hint, "NodeGroupPretty").pretty.indent = true;
    assert_plan_family_is_unselectable(package.path(), &node_hint, "Pretty");

    let mut variant_hint = plan.clone();
    pretty_node(&mut variant_hint, "VariantGroupPretty").variant_pretty[0].indent = true;
    assert_plan_family_is_unselectable(package.path(), &variant_hint, "Pretty");

    let mut field_hint = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection: PrettySemanticProjection::Direct { pretty, .. },
    } = semantic(pretty_node(&mut field_hint, "AttachmentPretty"))
    else {
        unreachable!("semantic helper returns semantic behavior")
    };
    pretty.trailing_separator = Some(PrettyTrailingSeparator::Never);
    assert_plan_family_is_unselectable(package.path(), &field_hint, "Pretty");

    let mut separator_policy = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection: PrettySemanticProjection::Direct { pretty, .. },
    } = semantic(pretty_node(&mut separator_policy, "LeadingPretty"))
    else {
        unreachable!("semantic helper returns semantic behavior")
    };
    pretty.separator = Some(PrettySeparatorPosition::Trailing);
    assert_plan_family_is_unselectable(package.path(), &separator_policy, "Pretty");

    let mut inconsistent_group = plan.clone();
    pretty_node(&mut inconsistent_group, "InconsistentIndentPretty")
        .pretty
        .group = Some(PrettyGroup::Consistent);
    assert_plan_family_is_unselectable(package.path(), &inconsistent_group, "Pretty");

    let mut sequence_layout = plan.clone();
    pretty_node(&mut sequence_layout, "NodeGroupPretty").variant_layouts[0] =
        PrettySequenceLayout::Delimited;
    assert_plan_family_is_unselectable(package.path(), &sequence_layout, "Pretty");

    let mut fixed_rendering = plan.clone();
    let rendering = pretty_node(&mut fixed_rendering, "NodeGroupPretty")
        .variants
        .iter_mut()
        .flatten()
        .find_map(|element| match element {
            PrettyElementBehavior::Fixed { rendering, .. } => Some(rendering),
            PrettyElementBehavior::OptionalFixedTokenGroup { .. }
            | PrettyElementBehavior::Semantic { .. } => None,
        })
        .expect("fixture has ordinary fixed syntax");
    assert_eq!(*rendering, PrettyFixedRendering::Word);
    *rendering = PrettyFixedRendering::Punctuation;
    assert_plan_family_is_unselectable(package.path(), &fixed_rendering, "Pretty");

    let mut attachment_layout = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection:
            PrettySemanticProjection::Direct {
                attachment: Some(attachment),
                ..
            },
    } = semantic(pretty_node(&mut attachment_layout, "AttachedOptional"))
    else {
        panic!("fixture has an attached field")
    };
    assert_eq!(attachment.layout, PrettySequenceLayout::Delimited);
    attachment.layout = PrettySequenceLayout::Linear;
    assert_plan_family_is_unselectable(package.path(), &attachment_layout, "Pretty");

    let mut attached_fixed_rendering = plan.clone();
    let PrettyElementBehavior::Semantic {
        projection:
            PrettySemanticProjection::Direct {
                attachment: Some(attachment),
                ..
            },
    } = semantic(pretty_node(
        &mut attached_fixed_rendering,
        "AttachedOptional",
    ))
    else {
        panic!("fixture has an attached field")
    };
    let PrettyAttachedSyntaxBehavior::Required(fixed) = &mut attachment.before[0] else {
        panic!("fixture delimiter is required")
    };
    let rendering = &mut fixed.rendering;
    assert_eq!(*rendering, PrettyFixedRendering::Opening);
    *rendering = PrettyFixedRendering::Punctuation;
    assert_plan_family_is_unselectable(package.path(), &attached_fixed_rendering, "Pretty");

    let mut separator_spelling = plan.clone();
    repetition_behavior(pretty_node(&mut separator_spelling, "LeadingPretty"))
        .separator
        .as_mut()
        .expect("fixture has a separator")
        .push_str("-corrupt");
    assert_plan_family_is_unselectable(package.path(), &separator_spelling, "Pretty");

    let mut trailing_separator = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut trailing_separator, "LeadingPretty"));
    assert!(repetition.trailing_separator);
    repetition.trailing_separator = false;
    assert_plan_family_is_unselectable(package.path(), &trailing_separator, "Pretty");

    let mut leading_separator = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut leading_separator, "LeadingPretty"));
    assert!(repetition.leading_separator);
    repetition.leading_separator = false;
    assert_plan_family_is_unselectable(package.path(), &leading_separator, "Pretty");

    let mut word_separator_spacing = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut word_separator_spacing, "WordSeparated"));
    assert!(repetition.space_before_separator);
    repetition.space_before_separator = false;
    assert_plan_family_is_unselectable(package.path(), &word_separator_spacing, "Pretty");

    let mut separator_trailing_spacing = plan.clone();
    let repetition = repetition_behavior(pretty_node(
        &mut separator_trailing_spacing,
        "LeadingPretty",
    ));
    assert!(repetition.space_after_separator);
    repetition.space_after_separator = false;
    assert_plan_family_is_unselectable(package.path(), &separator_trailing_spacing, "Pretty");

    let mut top_level_break = plan.clone();
    let repetition = repetition_behavior(pretty_node(&mut top_level_break, "MixedProgram"));
    assert!(repetition.hard_break);
    repetition.hard_break = false;
    assert_plan_family_is_unselectable(package.path(), &top_level_break, "Pretty");

    let mut attachment_presence = plan;
    let presence = pretty_node(&mut attachment_presence, "HintAttachedOptional")
        .variants
        .iter_mut()
        .flatten()
        .find_map(|element| match element {
            PrettyElementBehavior::Semantic {
                projection:
                    PrettySemanticProjection::Direct {
                        attachment: Some(_),
                        presence,
                        ..
                    },
            } => Some(presence),
            PrettyElementBehavior::Fixed { .. }
            | PrettyElementBehavior::OptionalFixedTokenGroup { .. }
            | PrettyElementBehavior::Semantic { .. } => None,
        })
        .expect("fixture has a conditionally present attachment unit");
    assert_eq!(*presence, PrettyPresence::Optional);
    *presence = PrettyPresence::Always;
    assert_plan_family_is_unselectable(package.path(), &attachment_presence, "Pretty");
}

pub(super) fn ascii_insensitive_word_separators_share_canonical_pretty_family_authority() {
    let package = tempfile::tempdir().expect("temporary word-separator authority fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
                recursa::grammar! {
                    module = crate::grammar,
                    derives(Pretty),
                    keyword_matching = ascii_insensitive,
                    max_lookahead = 1,
                }

                recursa::tokens! {
                    ignore = r"[ \t\n\r]+",
                    keywords { AND => "and" }
                    punctuation { LBRACKET => "[", RBRACKET => "]" }
                }

                #[derive(recursa::Node)]
                pub struct Name<'input> {
                    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
                    pub value: NameText<'input>,
                }

                #[derive(recursa::Node)]
                pub struct Separated<T> {
                    #[tok(LBRACKET, this, RBRACKET)]
                    #[sep(AND)]
                    pub values: Vec<T>,
                }

                #[derive(recursa::Node)]
                pub struct Root<'input> {
                    pub value: Separated<Name<'input>>,
                }
            "#,
    )
    .expect("write word-separator authority fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze word-separator authority fixture");
    let lowered = &success.analyzed.lowered;
    let authority = authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
        .expect("derive word-separator Pretty authority from source");
    let mut plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("canonical word separator joins one Pretty family");
    assert_eq!(Some(plan.pretty.clone()), authority);
    let application_separator =
        plan.nodes
            .iter()
            .flat_map(|node| &node.parse_applications)
            .flat_map(|application| match &application.body {
                NodeBodyPlan::Struct { elements, .. } => elements.iter().collect::<Vec<_>>(),
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter()
                    .flat_map(|variant| variant.elements.iter())
                    .collect(),
                NodeBodyPlan::Pratt(_) => Vec::new(),
            })
            .find_map(|element| match &element.kind {
                ElementParseKind::Semantic { projection, .. } => match projection.as_ref() {
                    SemanticParseProjection::Direct {
                        separator: Some(separator),
                        attachment: Some(_),
                        ..
                    } => Some(separator),
                    SemanticParseProjection::Direct { .. }
                    | SemanticParseProjection::FixedPresence { .. } => None,
                },
                ElementParseKind::Fixed { .. }
                | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
            })
            .expect("fixture materializes an attached word-separated application");
    assert_eq!(application_separator, "AND");

    let separator =
        plan.nodes
            .iter_mut()
            .flat_map(|node| match &mut node.body {
                NodeBodyPlan::Struct { elements, .. } => elements.iter_mut().collect::<Vec<_>>(),
                NodeBodyPlan::Enum { variants, .. } => variants
                    .iter_mut()
                    .flat_map(|variant| variant.elements.iter_mut())
                    .collect(),
                NodeBodyPlan::Pratt(_) => Vec::new(),
            })
            .find_map(|element| match &mut element.kind {
                ElementParseKind::Semantic { projection, .. } => match projection.as_mut() {
                    SemanticParseProjection::Direct {
                        separator: Some(separator),
                        attachment: Some(_),
                        ..
                    } => Some(separator),
                    SemanticParseProjection::Direct { .. }
                    | SemanticParseProjection::FixedPresence { .. } => None,
                },
                ElementParseKind::Fixed { .. }
                | ElementParseKind::OptionalFixedTokenGroup { .. } => None,
            })
            .expect("fixture contains an attached word-separated repetition");
    assert_eq!(separator, "AND");
    *separator = "and".to_owned();
    plan.pretty = emitted_pretty_plan(&plan.nodes, &plan.tokens);

    assert_ne!(Some(plan.pretty.clone()), authority);
    assert!(
        plan.validate()
            .expect_err("raw word separator must diverge from Pretty authority")
            .contains("Pretty plan family")
    );
}

pub(super) fn generic_top_level_breaks_keep_typed_family_and_closed_application_authority() {
    let package = tempfile::tempdir().expect("temporary generic top-level fixture");
    fs::write(
        package.path().join("grammar.rs"),
        r#"
                recursa::grammar! {
                    module = crate::grammar,
                    derives(Pretty),
                    keyword_matching = sensitive,
                    max_lookahead = 1,
                }

                recursa::tokens! {
                    ignore = r"[ \t\n\r]+",
                    punctuation { SEMICOLON => ";" }
                }

                #[derive(recursa::Node)]
                pub struct Name<'input> {
                    #[lex(pattern = r"[A-Za-z_][A-Za-z0-9_]*")]
                    pub value: NameText<'input>,
                }

                #[derive(recursa::Node)]
                pub struct Integer<'input> {
                    #[lex(pattern = r"[0-9]+")]
                    pub value: IntegerText<'input>,
                }

                #[derive(recursa::Node)]
                pub enum Statement<T> { Value(T) }

                #[derive(recursa::Node)]
                pub struct Program<T> {
                    #[sep(SEMICOLON)]
                    pub statements: Vec<Statement<T>>,
                }

                #[derive(recursa::Node)]
                pub struct Root<'input> {
                    pub names: Program<Name<'input>>,
                    pub integers: Program<Integer<'input>>,
                }
            "#,
    )
    .expect("write generic top-level fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze generic top-level fixture");
    let lowered = &success.analyzed.lowered;
    let authority = authoritative_pretty_plan(lowered, lowered.source_modeled.resolved())
        .expect("derive generic top-level Pretty authority");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join generic top-level applications");
    assert_eq!(Some(plan.pretty.clone()), authority);

    let program = plan
        .nodes
        .iter()
        .find(|node| node.ident == "Program")
        .expect("Program declaration plan");
    let NodeBodyPlan::Struct { elements, .. } = &program.body else {
        panic!("Program is a struct declaration")
    };
    let ElementParseKind::Semantic { projection, .. } = &elements[0].kind else {
        panic!("generic Program declaration keeps an application-independent enum-family break")
    };
    let SemanticParseProjection::Direct {
        repetition_break: PlannedRepetitionBreak::TopLevelStatementFamily(statement_family),
        terminal_application: None,
        ..
    } = projection.as_ref()
    else {
        panic!("generic Program declaration keeps an application-independent enum-family break")
    };

    let closed_breaks = program
        .parse_applications
        .iter()
        .map(|application| {
            let NodeBodyPlan::Struct { elements, .. } = &application.body else {
                panic!("Program application is a struct")
            };
            let ElementParseKind::Semantic { projection, .. } = &elements[0].kind else {
                panic!("closed Program application keeps an exact enum-application break")
            };
            let SemanticParseProjection::Direct {
                terminal_application: Some(terminal_application),
                repetition_break: PlannedRepetitionBreak::TopLevelStatement(break_application),
                ..
            } = projection.as_ref()
            else {
                panic!("closed Program application keeps an exact enum-application break")
            };
            assert_eq!(terminal_application, break_application);
            (*terminal_application, application.application)
        })
        .collect::<Vec<_>>();
    assert_eq!(closed_breaks.len(), 2);
    assert_ne!(closed_breaks[0].0, closed_breaks[1].0);
    assert!(plan.nodes.iter().any(|node| {
        node.parse_applications.iter().any(|application| {
            application.node == *statement_family
                && matches!(application.body, NodeBodyPlan::Enum { .. })
        })
    }));
    plan.validate()
        .expect("typed generic break family validates");
}

fn assert_plan_family_is_unselectable(package: &Path, plan: &EmissionPlan, family: &str) {
    let detail = match render(plan) {
        Err(detail) => detail,
        Ok(_) => panic!("a corrupted family cannot render"),
    };
    assert!(detail.contains(family), "detail={detail}");
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one pre-publication invariant diagnostic is required");
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
    assert!(!package.join(SELECTED_GENERATION).exists());
}

pub(super) fn incomplete_staging_is_unselectable() {
    let output = tempfile::tempdir().unwrap();
    let staging = output.path().join("staging");
    let published = output.path().join("published");
    fs::create_dir(&staging).unwrap();
    let rendered = RenderedGeneration {
        root: "fn root() {}".to_owned(),
        nodes: vec![("node-0.rs".to_owned(), "fn node() {}".to_owned())],
        node_anchors: vec!["anchor-0".to_owned()],
        selector: "fn selected() {}".to_owned(),
        diagram_plan: None,
    };
    assert_eq!(
        stage_and_publish(&staging, &published, &rendered, || {
            Err("injected staging interruption".to_owned())
        })
        .unwrap_err(),
        "injected staging interruption",
    );
    assert!(!published.exists());
    assert!(staging.join("root.rs").is_file());
    assert!(!staging.join("node-0.rs").exists());
}

pub(super) fn same_identity_cannot_reuse_a_different_generated_payload() {
    let output = tempfile::tempdir().unwrap();
    let original = publication_fixture("g_same");
    publish(output.path(), "g_same", &original).unwrap();
    invalidate_selection(output.path()).unwrap();

    let mut changed = publication_fixture("g_same");
    changed.root.push_str(" changed");
    let error = publish(output.path(), "g_same", &changed).unwrap_err();
    assert!(error.contains("Generation identity collision"));
    assert_eq!(selected_generation(output.path()), None);
    assert_eq!(
        fs::read_to_string(output.path().join("recursa-generations/g_same/root.rs")).unwrap(),
        original.root
    );

    publish(output.path(), "g_same", &original).unwrap();
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some("g_same")
    );
}

fn publication_fixture(identity: &str) -> RenderedGeneration {
    RenderedGeneration {
        root: format!("// {identity} root"),
        nodes: vec![("node-0.rs".to_owned(), format!("// {identity} node"))],
        node_anchors: vec!["anchor-0".to_owned()],
        selector: format!(
            "// recursa-generation:{identity}\ninclude!(concat!(env!(\"OUT_DIR\"), \"/recursa-generations/{identity}/root.rs\"));\n"
        ),
        diagram_plan: Some(format!("// {identity} diagram plan")),
    }
}

pub(super) fn every_failed_publication_checkpoint_invalidates_stale_selection_and_recovers() {
    for checkpoint in [
        PublicationCheckpoint::BeforeStaging,
        PublicationCheckpoint::DuringStaging,
        PublicationCheckpoint::BeforeSelection,
        PublicationCheckpoint::DuringSelection,
    ] {
        let output = tempfile::tempdir().unwrap();
        let old = publication_fixture("g_old");
        publish_with_checkpoint(output.path(), "g_old", &old, |_| Ok(())).unwrap();
        assert_eq!(selected_generation(output.path()).as_deref(), Some("g_old"));

        invalidate_selection(output.path()).unwrap();
        let next = publication_fixture("g_next");
        let error = publish_with_checkpoint(output.path(), "g_next", &next, |point| {
            if point == checkpoint {
                Err(format!("injected {point:?} fault"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error, format!("injected {checkpoint:?} fault"));
        assert_eq!(selected_generation(output.path()), None);

        publish_with_checkpoint(output.path(), "g_next", &next, |_| Ok(())).unwrap();
        assert_eq!(
            selected_generation(output.path()).as_deref(),
            Some("g_next")
        );
        assert!(
            output
                .path()
                .join("recursa-generations/g_next/root.rs")
                .is_file()
        );
        assert!(
            output
                .path()
                .join("recursa-generations/g_next/node-0.rs")
                .is_file()
        );
    }
}

#[cfg(unix)]
pub(super) fn filesystem_invalidation_failure_poisons_stale_selection_and_recovers() {
    use std::os::unix::fs::PermissionsExt as _;

    let output = tempfile::tempdir().unwrap();
    let old = publication_fixture("g_old");
    publish_with_checkpoint(output.path(), "g_old", &old, |_| Ok(())).unwrap();
    assert_eq!(selected_generation(output.path()).as_deref(), Some("g_old"));

    let original_permissions = fs::metadata(output.path()).unwrap().permissions();
    let mut non_writable = original_permissions.clone();
    non_writable.set_mode(0o555);
    fs::set_permissions(output.path(), non_writable).unwrap();
    let error = invalidate_selection(output.path()).unwrap_err();
    fs::set_permissions(output.path(), original_permissions).unwrap();

    assert!(error.contains("cannot invalidate"), "{error}");
    assert_eq!(selected_generation(output.path()), None);
    let poisoned = fs::read_to_string(output.path().join(SELECTED_GENERATION)).unwrap();
    assert!(poisoned.contains("compile_error!"));
    assert!(!poisoned.contains("recursa-generation:g_old"));

    invalidate_selection(output.path()).unwrap();
    let next = publication_fixture("g_next");
    publish_with_checkpoint(output.path(), "g_next", &next, |_| Ok(())).unwrap();
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some("g_next")
    );
}

pub(super) fn filesystem_rename_failures_leave_no_loadable_or_staged_generation() {
    let output = tempfile::tempdir().unwrap();
    let rendered = publication_fixture("g_next");
    let published = output.path().join("recursa-generations/g_next");
    let error = publish_with_checkpoint(output.path(), "g_next", &rendered, |checkpoint| {
        if checkpoint == PublicationCheckpoint::DuringStaging {
            fs::create_dir_all(&published).unwrap();
            fs::write(published.join("block"), "external collision").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.contains("publication barrier"), "{error}");
    assert_eq!(selected_generation(output.path()), None);
    assert_no_transaction_staging(output.path());

    fs::remove_dir_all(&published).unwrap();
    fs::create_dir(output.path().join(SELECTED_GENERATION)).unwrap();
    let error =
        publish_with_checkpoint(output.path(), "g_next", &rendered, |_| Ok(())).unwrap_err();
    assert!(error.contains("atomically select"), "{error}");
    assert_eq!(selected_generation(output.path()), None);
    assert_no_transaction_staging(output.path());
}

pub(super) fn warnings_publish_one_complete_generation_and_remain_on_the_result() {
    let package = tempfile::tempdir().unwrap();
    fs::write(
        package.path().join("grammar.rs"),
        r#"
recursa::grammar! { module = crate, max_lookahead = 1 }
recursa::tokens! { keywords { SELECT => "SELECT" } }

#[derive(recursa::Node)]
#[tok(SELECT)]
struct Select;

#[derive(recursa::Node)]
struct Inner { maybe: Option<Select> }

#[derive(recursa::Node)]
struct Root {
    #[sep(SELECT)]
    items: Vec<Inner>,
}
"#,
    )
    .unwrap();
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("warnings retain analyzable output");
    assert!(success.build_report.warnings().next().is_some());
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default()).unwrap();
    let rendered = render(&plan).unwrap();
    let output = tempfile::tempdir().unwrap();
    publish(output.path(), &plan.identity, &rendered).unwrap();

    let generation = Generation {
        report: success.build_report,
        reused: false,
    };
    assert!(generation.report().warnings().next().is_some());
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(plan.identity.as_str())
    );
    let published = output
        .path()
        .join("recursa-generations")
        .join(&plan.identity);
    assert!(published.join("root.rs").is_file());
    assert!(published.join("loader-map").is_file());
    assert_eq!(
        fs::read_dir(published)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("node-"))
            .count(),
        rendered.nodes.len()
    );
}

fn assert_no_transaction_staging(out_dir: &Path) {
    let mut pending = vec![out_dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let name = path.file_name().unwrap().to_string_lossy();
                assert!(!name.starts_with(".recursa-selector-"), "{path:?}");
            }
        }
    }
    let generations = out_dir.join("recursa-generations");
    if let Ok(entries) = fs::read_dir(generations) {
        for entry in entries {
            let path = entry.unwrap().path();
            assert!(
                !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".staging-"),
                "{path:?}"
            );
        }
    }
}

/// Grammar whose analysis reports warnings, so reuse must replay them.
const REUSE_GRAMMAR: &str = r#"
recursa::grammar! { module = crate, max_lookahead = 1 }
recursa::tokens! { keywords { SELECT => "SELECT" } }

#[derive(recursa::Node)]
#[tok(SELECT)]
struct Select;

#[derive(recursa::Node)]
struct Inner { maybe: Option<Select> }

#[derive(recursa::Node)]
struct Root {
    #[sep(SELECT)]
    items: Vec<Inner>,
}
"#;

/// One in-process generation run over a package directory and an output directory.
fn generate_reuse_fixture(
    package: &Path,
    out_dir: &Path,
    debug_directory: Option<PathBuf>,
) -> (Generation, Vec<String>) {
    let request = GenerateRequest {
        out_dir: out_dir.to_path_buf(),
        manifest_dir: package.to_path_buf(),
        selected_root: PathBuf::from("grammar.rs"),
        debug_directory,
        cfg: CargoCfgSnapshot::default(),
    };
    let mut cargo = Vec::new();
    let generation = generate_with(&request, &mut cargo).expect("the reuse fixture generates");
    (generation, cargo)
}

/// Keeps only the Cargo warning directives of one build-script output.
fn warning_lines(cargo: &[String]) -> Vec<String> {
    cargo
        .iter()
        .filter(|line| line.starts_with("cargo::warning="))
        .cloned()
        .collect()
}

/// Keeps only the Cargo invalidation directives of one build-script output.
fn invalidation_lines(cargo: &[String]) -> Vec<String> {
    cargo
        .iter()
        .filter(|line| line.starts_with("cargo::rerun-if-"))
        .cloned()
        .collect()
}

pub(super) fn unchanged_sources_reuse_the_published_generation_without_analysis() {
    let package = tempfile::tempdir().unwrap();
    fs::write(package.path().join("grammar.rs"), REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();

    let (first, first_cargo) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    assert!(first.report().warnings().next().is_some());
    let identity = selected_generation(output.path()).expect("first run selects a Generation");
    assert!(!warning_lines(&first_cargo).is_empty());
    assert!(output.path().join("recursa-reuse").is_dir());

    let (second, second_cargo) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(
        second.reused(),
        "unchanged sources must reuse the Generation"
    );
    assert!(second.report().warnings().next().is_none());
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str())
    );
    assert_eq!(
        warning_lines(&second_cargo),
        warning_lines(&first_cargo),
        "reuse must replay exactly the recorded warnings"
    );
    assert_eq!(
        invalidation_lines(&second_cargo),
        invalidation_lines(&first_cargo),
        "reuse must keep every Cargo invalidation directive"
    );
    assert_no_transaction_staging(output.path());
}

pub(super) fn comment_only_edits_reuse_the_published_generation() {
    let package = tempfile::tempdir().unwrap();
    let grammar = package.path().join("grammar.rs");
    fs::write(&grammar, REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    let identity = selected_generation(output.path()).unwrap();

    let edited = format!("// leading comment\n{REUSE_GRAMMAR}\n/* trailing\n   block */\n");
    fs::write(&grammar, edited).unwrap();
    let (second, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(
        second.reused(),
        "comment-only edits must reuse the Generation"
    );
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str())
    );
}

pub(super) fn semantic_edits_do_not_reuse_the_published_generation() {
    let package = tempfile::tempdir().unwrap();
    let grammar = package.path().join("grammar.rs");
    fs::write(&grammar, REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    let identity = selected_generation(output.path()).unwrap();

    let edited = format!(
        "{REUSE_GRAMMAR}\n#[derive(recursa::Node)]\nstruct Extra {{ first: Select, second: Select }}\n"
    );
    fs::write(&grammar, edited).unwrap();
    let (second, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!second.reused(), "a semantic edit must analyze again");
    let relocated = selected_generation(output.path()).unwrap();
    assert_ne!(relocated, identity);

    // Going back to the first grammar hits the first entry again.
    fs::write(&grammar, REUSE_GRAMMAR).unwrap();
    let (third, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(third.reused());
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str())
    );
}

pub(super) fn missing_or_incomplete_generations_are_not_reused() {
    let package = tempfile::tempdir().unwrap();
    fs::write(package.path().join("grammar.rs"), REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());
    let identity = selected_generation(output.path()).unwrap();
    let published = output.path().join("recursa-generations").join(&identity);

    // A Generation directory that lost its loader map is never re-selected.
    fs::remove_file(published.join("loader-map")).unwrap();
    invalidate_selection(output.path()).unwrap();
    let key = fs::read_dir(output.path().join("recursa-reuse"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .find(|name| name.starts_with("r_"))
        .expect("the first run recorded a reuse entry");
    assert!(reuse_published(output.path(), &key).unwrap().is_none());
    assert!(!output.path().join(SELECTED_GENERATION).exists());

    // A missing Generation directory falls through to a fresh publication.
    fs::remove_dir_all(&published).unwrap();
    let (second, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(
        !second.reused(),
        "a missing Generation must be published again"
    );
    assert_eq!(
        selected_generation(output.path()).as_deref(),
        Some(identity.as_str())
    );
    assert!(published.join("loader-map").is_file());

    let (third, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(third.reused());
}

pub(super) fn debug_directory_requests_bypass_reuse() {
    let package = tempfile::tempdir().unwrap();
    fs::write(package.path().join("grammar.rs"), REUSE_GRAMMAR).unwrap();
    let output = tempfile::tempdir().unwrap();
    let (first, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(!first.reused());

    let debug = tempfile::tempdir().unwrap();
    let (second, _) = generate_reuse_fixture(
        package.path(),
        output.path(),
        Some(debug.path().to_path_buf()),
    );
    assert!(
        !second.reused(),
        "a debug bundle request must run the complete pipeline"
    );
    assert!(second.report().warnings().next().is_some());

    let (third, _) = generate_reuse_fixture(package.path(), output.path(), None);
    assert!(third.reused());
}
