use super::*;

/// Returns the detail of one emission-plan failure for `source` without invoking downstream Cargo.
///
/// The grammar must analyze cleanly, because emission planning is the seam under test:
/// `adjust` may corrupt the analyzed grammar for failures no authored source can express.
fn emission_plan_join_error(
    source: String,
    adjust: impl FnOnce(&mut crate::source_analysis::AnalyzedGrammar),
) -> String {
    let package = tempfile::tempdir().expect("temporary emission plan fixture");
    fs::write(package.path().join("grammar.rs"), source).expect("write emission plan fixture");
    let mut success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("emission diagnostic fixture reaches emission planning");
    adjust(&mut success.analyzed);
    let detail = match EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default()) {
        Ok(_) => panic!("an invalid declaration reached emission"),
        Err(detail) => detail,
    };
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one emission plan failure must produce one diagnostic")
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
    assert_eq!(diagnostic.stage(), DiagnosticStage::Emission);
    assert_eq!(
        diagnostic.primary().message(),
        "Recursa detected an internal generation invariant failure",
    );
    let location = diagnostic
        .primary()
        .location()
        .expect("emission diagnostics select the grammar root");
    assert_eq!(location.path(), Path::new("grammar.rs"));
    assert_eq!(location.range(), None);
    assert!(matches!(
        diagnostic.evidence(),
        crate::DiagnosticEvidence::InternalInvariant { producer, detail: evidence }
            if producer == "Emission" && evidence == &detail
    ));
    detail
}

#[cfg(feature = "explorer")]
#[test]
fn a_table_driven_default_parser_is_refused_beside_framing_or_the_explorer() {
    // Framing and the explorer adapter drive the default parser through facade seams bound to
    // the recursive-descent impl (#119): a table-driven default is refused at the renderer
    // selection point with a remedy, whether declared through `parser_style` or first in
    // `parsers(...)`; the same declaration with recursive descent first renders.
    let framing = include_str!("../../tests/fixtures/generation/framing/src/grammar.rs");
    let with_setting = |setting: &str| {
        framing.replace(
            "    max_lookahead = 2,\n",
            &format!("    max_lookahead = 2,\n    {setting}\n"),
        )
    };
    for setting in [
        "parser_style = table_driven,",
        "parsers(lr = table_driven, rd = recursive_descent),",
    ] {
        let (_package, analyzed, plan) = joined_plan_from_source(&with_setting(setting));
        let detail = render_by_parser_style(&plan, &analyzed.lowered)
            .err()
            .unwrap_or_else(|| panic!("{setting} is refused beside framing"));
        assert_eq!(
            detail,
            "the explorer and framing capabilities drive the default parser, which must be \
             recursive descent: declare a `recursive_descent` parser first in `parsers(...)` \
             or select `parser_style = recursive_descent`"
        );
    }
    let (_package, analyzed, plan) =
        joined_plan_from_source(&with_setting("parsers(rd = recursive_descent, lr = table_driven),"));
    let rendered = render_by_parser_style(&plan, &analyzed.lowered)
        .expect("a recursive-descent default parser renders beside framing");
    assert!(rendered.root.contains("pub mod parsers"));
}

#[test]
fn framing_planner_rejects_an_undefined_boundary_token_directly() {
    let source = include_str!("../../tests/fixtures/generation/framing/src/grammar.rs")
        .replace("boundary = SEMI", "boundary = MissingBoundary");
    assert_eq!(
        emission_plan_join_error(source, |_| {}),
        "framing token `MissingBoundary` is not defined",
    );
}

#[test]
fn framing_planner_rejects_an_ambiguous_boundary_token_directly() {
    let source = include_str!("../../tests/fixtures/generation/framing/src/grammar.rs").to_owned();
    assert_eq!(
        emission_plan_join_error(source, |analyzed| {
            let model = analyzed.lowered.source_modeled.source_model_mut();
            let duplicate = model
                .token_definitions
                .iter()
                .find(|definition| definition.name == "SEMI")
                .expect("framing boundary definition")
                .clone();
            model.token_definitions.push(duplicate);
        }),
        "framing token `SEMI` is ambiguous",
    );
}

#[test]
fn framing_planner_requires_a_fixed_boundary_token_directly() {
    let source = include_str!("../../tests/fixtures/generation/framing/src/grammar.rs")
        .replace("boundary = SEMI", "boundary = Quoted");
    assert_eq!(
        emission_plan_join_error(source, |_| {}),
        "framing boundary must name one fixed token",
    );
}

#[test]
fn framing_planner_requires_a_closed_physical_line_token_directly() {
    let source = include_str!("../../tests/fixtures/generation/framing/src/grammar.rs")
        .replace("line = [MetaLine]", "line = [RawBlock]");
    assert_eq!(
        emission_plan_join_error(source, |_| {}),
        "framing token `RawBlock` must use a closed physical-line matcher",
    );
}

#[test]
fn framing_planner_requires_a_closed_delimited_token_directly() {
    let source = include_str!("../../tests/fixtures/generation/framing/src/grammar.rs")
        .replace("delimited = [RawBlock]", "delimited = [MetaLine]");
    assert_eq!(
        emission_plan_join_error(source, |_| {}),
        "framing token `MetaLine` must use a closed same- or nested-delimiter matcher",
    );
}

#[test]
fn framing_planner_rejects_overlapping_atomic_ownership_directly() {
    let source = include_str!("../../tests/fixtures/generation/framing/src/grammar.rs")
        .replace("line = [MetaLine]", "line = [MetaLine, MetaLine]");
    assert_eq!(
        emission_plan_join_error(source, |_| {}),
        "framing boundary, line, and delimited token ownership must be disjoint",
    );
}

/// Returns the greedy fixture grammar with its one `exclude(Factorial)` name replaced.
///
/// The fixture's `Excluded::expr` is the only authored `#[parse(pratt(exclude(...)))]` site,
/// and `Expr` has the NUD variant `Integer` beside the led variants `Factorial` and `Like`.
fn greedy_fixture_excluding(name: &str) -> String {
    include_str!("../../tests/fixtures/generation/greedy/src/grammar.rs")
        .replace("exclude(Factorial)", &format!("exclude({name})"))
}

#[test]
fn source_analysis_rejects_an_exclusion_that_names_no_variant() {
    // A misspelled exclusion must never widen a site's language in silence. Since #122 an
    // exclusion may name a variant of any Pratt position, so materialization resolves the
    // name itself and rejects one that resolves to nothing, before emission is reached.
    let package = tempfile::tempdir().expect("temporary exclusion fixture");
    fs::write(
        package.path().join("grammar.rs"),
        greedy_fixture_excluding("Factorail"),
    )
    .expect("write exclusion fixture");
    let error = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .err()
    .expect("an unresolvable exclusion reaches emission");
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one unresolvable exclusion must produce one diagnostic")
    };
    assert_eq!(diagnostic.code().as_str(), "RCA3212");
    assert_eq!(diagnostic.stage(), DiagnosticStage::Lowering);
    assert_eq!(
        diagnostic.primary().message(),
        "`expr` excludes `Factorail`, which is not a variant of Pratt Node `grammar::Expr`",
    );
}

#[test]
fn emission_failures_use_the_registered_generation_code_and_preserve_detail() {
    let error = failure(
        Path::new("src/grammar.rs"),
        "the frozen emission plan is incomplete",
    );
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one emission invariant failure must produce one diagnostic");
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
    assert_eq!(diagnostic.stage(), DiagnosticStage::Emission);
    assert!(matches!(
        diagnostic.evidence(),
        crate::DiagnosticEvidence::InternalInvariant { producer, detail }
            if producer == "Emission" && detail == "the frozen emission plan is incomplete"
    ));
}

#[test]
fn warnings_publish_one_complete_generation_and_remain_on_the_result() {
    test_support::warnings_publish_one_complete_generation_and_remain_on_the_result();
}

#[test]
fn emission_lookup_index_bounds_work_and_caches_pratt_products() {
    let package = tempfile::tempdir().expect("temporary generation-provenance fixture");
    fs::write(
        package.path().join("grammar.rs"),
        include_str!("../../tests/fixtures/generation/pratt/src/grammar.rs"),
    )
    .expect("write generation-provenance fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze Pratt fixture for emission lookup work");
    reset_emission_lookup_instrumentation();
    EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join indexed Pratt emission plan");
    let lookup_work = emission_lookup_instrumentation();
    assert_eq!(
        lookup_work.dispatch_candidates, 5,
        "the optimization must preserve the small dispatch lookup path",
    );
    assert_eq!(
        lookup_work.failure_edge_candidates, 0,
        "failure-edge joins must use their dense semantic key",
    );
    assert_eq!(
        lookup_work.container_machine_candidates, 0,
        "container joins must use their dense site key",
    );
    assert!(
        lookup_work.pratt_extender_builds <= 17,
        "each distinct Pratt owner/threshold product must be built at most once",
    );
    assert!(
        lookup_work.pratt_extender_branch_candidates <= 153,
        "nine Pratt branches may be visited at most once for each distinct product",
    );
}

#[test]
fn transient_provenance_preserves_semantics_identity_and_generated_artifacts() {
    let package = tempfile::tempdir().expect("temporary generation-provenance fixture");
    fs::write(
        package.path().join("grammar.rs"),
        include_str!("../../tests/fixtures/generation/pratt/src/grammar.rs"),
    )
    .expect("write generation-provenance fixture");
    let transient = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze Pratt fixture with transient provenance");
    let transient_projection =
        crate::semantic_projection::canonical_semantic_projection(&transient.analyzed);
    let transient_plan =
        EmissionPlan::join_for_test(&transient.analyzed, &CargoCfgSnapshot::default())
            .expect("join transient-provenance Emission plan");
    let transient_rendered =
        render(&transient_plan).expect("render transient-provenance Generation");

    let mut compact = transient.clone();
    compact.analyzed.analysis = crate::analysis::analyze(&compact.analyzed.lowered.grammar);
    assert_eq!(
        transient
            .analyzed
            .analysis
            .output
            .as_ref()
            .expect("successful transient output")
            .decisions(),
        compact
            .analyzed
            .analysis
            .output
            .as_ref()
            .expect("successful compact output")
            .decisions(),
        "proof publication policy must not alter frozen decisions",
    );
    assert_eq!(
        transient_projection,
        crate::semantic_projection::canonical_semantic_projection(&compact.analyzed),
        "proof publication policy must not alter semantic projection",
    );
    let compact_plan = EmissionPlan::join_for_test(&compact.analyzed, &CargoCfgSnapshot::default())
        .expect("join compact-provenance Emission plan");
    let compact_rendered = render(&compact_plan).expect("render compact-provenance Generation");

    assert_eq!(
        transient_plan
            .instrumentation
            .frozen_dispatch_nodes_consumed,
        compact_plan.instrumentation.frozen_dispatch_nodes_consumed,
    );
    assert_eq!(
        transient_plan.plan_families.parse, compact_plan.plan_families.parse,
        "frozen parse identity differs",
    );
    assert_eq!(transient_plan.identity, compact_plan.identity);
    assert_eq!(transient_rendered.root, compact_rendered.root);
    assert_eq!(transient_rendered.nodes, compact_rendered.nodes);
    assert_eq!(
        transient_rendered.node_anchors,
        compact_rendered.node_anchors
    );
    assert_eq!(transient_rendered.selector, compact_rendered.selector);
}

/// Joins the Pratt fixture with `setting` appended to its root `grammar!` configuration.
fn joined_pratt_plan_with_setting(
    setting: &str,
) -> (
    tempfile::TempDir,
    crate::source_analysis::AnalyzedGrammar,
    EmissionPlan,
) {
    let source = include_str!("../../tests/fixtures/generation/pratt/src/grammar.rs").replace(
        "    max_lookahead = 1,\n}",
        &format!("    max_lookahead = 1,\n    {setting}\n}}"),
    );
    let package = tempfile::tempdir().expect("temporary parser-style fixture");
    fs::write(package.path().join("grammar.rs"), source).expect("write parser-style fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze parser-style fixture");
    let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join parser-style Emission plan");
    (package, success.analyzed, plan)
}

/// Analyzes and joins one inline grammar source.
fn joined_plan_from_source(
    source: &str,
) -> (
    tempfile::TempDir,
    crate::source_analysis::AnalyzedGrammar,
    EmissionPlan,
) {
    let package = tempfile::tempdir().expect("temporary inline fixture");
    fs::write(package.path().join("grammar.rs"), source).expect("write inline fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("analyze inline fixture");
    let plan = EmissionPlan::join_for_test(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("join inline Emission plan");
    (package, success.analyzed, plan)
}

#[test]
fn a_lookahead_declaration_leaves_recursive_descent_node_sources_byte_identical() {
    let fixture = include_str!("../../tests/fixtures/generation/lookahead/src/grammar.rs");
    let named_parsers = "parsers(rd = recursive_descent, lr = table_driven),";
    let lookahead = "    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }\n";
    assert_eq!(fixture.matches(named_parsers).count(), 1);
    assert_eq!(fixture.matches(lookahead).count(), 1);
    let declared = fixture.replace(named_parsers, "parser_style = recursive_descent,");
    let undeclared = declared.replace(lookahead, "");
    let (_declared_package, _, declared_plan) = joined_plan_from_source(&declared);
    let (_undeclared_package, _, undeclared_plan) = joined_plan_from_source(&undeclared);
    let (_table_package, table_analyzed, table_plan) = joined_plan_from_source(fixture);

    let declared_rendered = render(&declared_plan).expect("render declared");
    let undeclared_rendered = render(&undeclared_plan).expect("render undeclared");
    assert!(!declared_rendered.nodes.is_empty());
    // Every generated source embeds its generation identity in the handshake; the merged kind
    // changes the identity, so the comparison reads the sources with the identity normalized.
    let normalized = |rendered: &RenderedGeneration, identity: &str| {
        rendered
            .nodes
            .iter()
            .map(|(name, text)| (name.clone(), text.replace(identity, "IDENTITY")))
            .collect::<Vec<_>>()
    };
    let declared_nodes = normalized(&declared_rendered, &declared_plan.identity);
    let undeclared_nodes = normalized(&undeclared_rendered, &undeclared_plan.identity);
    if declared_nodes != undeclared_nodes {
        if let Some(directory) = std::env::var_os("RECURSA_TEST_DUMP_DIR") {
            for (label, nodes) in [("declared", &declared_nodes), ("undeclared", &undeclared_nodes)] {
                for (name, text) in nodes {
                    fs::write(Path::new(&directory).join(format!("{label}-{name}")), text).unwrap();
                }
            }
        }
    }
    assert_eq!(
        declared_nodes, undeclared_nodes,
        "a lookahead declaration is inert under recursive descent"
    );
    assert!(declared_rendered.root.contains("NOT_LA"), "the merged kind is a TokenKind variant");
    assert!(!undeclared_rendered.root.contains("NOT_LA"));
    assert_ne!(
        declared_plan.identity, undeclared_plan.identity,
        "the merged kind must re-generate"
    );

    let table_rendered = render_by_parser_style(&table_plan, &table_analyzed.lowered)
        .expect("render table-driven");
    assert!(table_rendered.root.contains("__recursa_lr"));
    assert!(table_rendered.root.contains("NOT_LA"));
    assert_ne!(table_rendered.nodes, declared_rendered.nodes);
}

#[test]
fn a_precedence_declaration_leaves_recursive_descent_sources_byte_identical() {
    let fixture = include_str!("../../tests/fixtures/generation/precedence/src/grammar.rs");
    let named_parsers = "parsers(rd = recursive_descent, lr = table_driven),";
    let precedence_start = fixture.find("    precedence {").expect("the fixture declares precedence");
    let precedence_end = fixture[precedence_start..]
        .find("    }\n")
        .map(|end| precedence_start + end + "    }\n".len())
        .expect("the declaration closes");
    let precedence = &fixture[precedence_start..precedence_end];
    let overrides = ["    #[parse(prec = UMINUS)]\n", "    #[parse(prec = UNBOUNDED)]\n"];
    assert_eq!(fixture.matches(named_parsers).count(), 1);
    for override_ in overrides {
        assert_eq!(fixture.matches(override_).count(), 1, "{override_}");
    }
    let declared = fixture.replace(named_parsers, "parser_style = recursive_descent,");
    // Blank lines keep every later declaration on its line: a Node's loader anchor hashes
    // its line, so deleting lines would move the identity for a reason that is not the
    // declaration.
    let mut undeclared = declared.replace(precedence, &"\n".repeat(precedence.lines().count()));
    for override_ in overrides {
        undeclared = undeclared.replace(override_, "\n");
    }
    let (_declared_package, _, declared_plan) = joined_plan_from_source(&declared);
    let (_undeclared_package, _, undeclared_plan) = joined_plan_from_source(&undeclared);
    let (_table_package, table_analyzed, table_plan) = joined_plan_from_source(fixture);

    let declared_rendered = render(&declared_plan).expect("render declared");
    let undeclared_rendered = render(&undeclared_plan).expect("render undeclared");
    assert!(!declared_rendered.nodes.is_empty());
    assert_eq!(
        declared_plan.identity, undeclared_plan.identity,
        "the declaration is no input of the recursive-descent generation identity"
    );
    assert_eq!(
        declared_rendered.nodes, undeclared_rendered.nodes,
        "a precedence declaration is inert under recursive descent"
    );
    assert_eq!(declared_rendered.root, undeclared_rendered.root);

    let table_rendered = render_by_parser_style(&table_plan, &table_analyzed.lowered)
        .expect("render table-driven");
    assert!(table_rendered.root.contains("__recursa_lr"));
    assert_ne!(table_rendered.nodes, declared_rendered.nodes);
}

#[test]
fn parser_style_leaves_an_unchanged_grammar_identity_in_place_and_table_driven_moves_it() {
    let (_omitted_package, omitted, omitted_plan) = joined_pratt_plan_with_setting("");
    let (_explicit_package, explicit, explicit_plan) =
        joined_pratt_plan_with_setting("parser_style = recursive_descent,");
    let (_table_package, table, table_plan) =
        joined_pratt_plan_with_setting("parser_style = table_driven,");
    let (_named_package, _named, named_plan) =
        joined_pratt_plan_with_setting("parsers(rd = recursive_descent, lr = table_driven),");

    assert!(omitted_plan.parsers.is_implicit_default());
    assert_eq!(explicit_plan.parsers, omitted_plan.parsers);
    assert_eq!(
        omitted_plan.identity, explicit_plan.identity,
        "an explicit recursive_descent selection states the implicit default"
    );
    assert_ne!(
        omitted_plan.identity, table_plan.identity,
        "selecting table_driven must re-generate"
    );
    assert_ne!(
        omitted_plan.identity, named_plan.identity,
        "declaring named parsers must re-generate"
    );
    assert_ne!(table_plan.identity, named_plan.identity);

    let projection = crate::semantic_projection::canonical_semantic_projection(&omitted);
    assert_eq!(
        projection,
        crate::semantic_projection::canonical_semantic_projection(&explicit),
        "parser style is a generation choice, not a semantic fact"
    );
    assert_eq!(
        projection,
        crate::semantic_projection::canonical_semantic_projection(&table),
    );
    assert_eq!(
        render(&omitted_plan).expect("render omitted").nodes,
        render(&explicit_plan).expect("render explicit").nodes,
    );
}

#[test]
fn the_renderer_selection_point_routes_table_driven_and_named_parsers() {
    // The Pratt fixture reaches the table-driven renderer, whose analysis lowers the binding
    // powers to precedence and then reports the fixture's genuine conflict, the optional
    // `ESCAPE` clause after a `LIKE` operand (a container, #114); the refusal is an emission
    // failure like any other renderer invariant, and the recursive-descent default renders as
    // before. The Pratt fixtures that render table-driven are the `generated_pratt_table_driven`
    // integration test's. Named parsers reach the same point (#119): the table-driven `lr`
    // parser runs the analysis and reports the conflict beside the recursive-descent default.
    let is_the_escape_conflict = |detail: &str| {
        detail.starts_with("the table-driven parser cannot be generated: ")
            && detail.contains("error[RCA0400]: shift/reduce conflict")
            && detail.contains("on lookahead `ESCAPE`")
    };
    let (_package, analyzed, named) =
        joined_pratt_plan_with_setting("parsers(rd = recursive_descent, lr = table_driven),");
    let detail = match render_by_parser_style(&named, &analyzed.lowered) {
        Ok(_) => panic!("the optional ESCAPE clause is a shift/reduce conflict"),
        Err(detail) => detail,
    };
    assert!(is_the_escape_conflict(&detail), "{detail}");
    let error = failure(Path::new("grammar.rs"), &detail);
    let [diagnostic] = error.report().diagnostics() else {
        panic!("one renderer failure must produce one diagnostic")
    };
    assert_eq!(diagnostic.code().as_str(), "RCA9105");
    assert_eq!(diagnostic.stage(), DiagnosticStage::Emission);

    // A table-driven default parser reports the same conflict, unless the explorer generation
    // feature makes every eligible node an explorer root, in which case the default parser is
    // refused first: the explorer adapter drives the recursive-descent impl.
    let (_package, analyzed, plan) =
        joined_pratt_plan_with_setting("parser_style = table_driven,");
    let detail = render_by_parser_style(&plan, &analyzed.lowered)
        .err()
        .expect("a table-driven default parser does not render the Pratt fixture");
    if cfg!(feature = "explorer") {
        assert!(detail.starts_with("the explorer and framing capabilities drive the default parser"), "{detail}");
    } else {
        assert!(is_the_escape_conflict(&detail), "{detail}");
    }

    let (_package, analyzed, recursive) = joined_pratt_plan_with_setting("");
    let default_rendering = render_by_parser_style(&recursive, &analyzed.lowered)
        .expect("the default parser style renders as before");

    // One named recursive-descent parser renders the implicit default's files: the name is a
    // generation-identity fact and nothing else.
    let (_package, analyzed, only_named) =
        joined_pratt_plan_with_setting("parsers(rd = recursive_descent),");
    assert_ne!(only_named.identity, recursive.identity);
    let named_rendering = render_by_parser_style(&only_named, &analyzed.lowered)
        .expect("one named recursive-descent parser renders");
    let normalize = |text: &str, identity: &str| text.replace(identity, "IDENTITY");
    assert_eq!(
        normalize(&named_rendering.root, &only_named.identity),
        normalize(&default_rendering.root, &recursive.identity)
    );
    assert_eq!(
        named_rendering
            .nodes
            .iter()
            .map(|(name, text)| (name.clone(), normalize(text, &only_named.identity)))
            .collect::<Vec<_>>(),
        default_rendering
            .nodes
            .iter()
            .map(|(name, text)| (name.clone(), normalize(text, &recursive.identity)))
            .collect::<Vec<_>>()
    );
}
