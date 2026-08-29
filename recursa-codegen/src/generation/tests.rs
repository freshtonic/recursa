use super::*;

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
