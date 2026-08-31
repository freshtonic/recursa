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
fn transient_provenance_preserves_semantics_and_normalized_generated_artifacts() {
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

    // Each independently joined plan owns a fresh cache identity. First prove that each identity
    // is propagated coherently, then remove only that value from the generated-byte comparison.
    for (plan, rendered) in [
        (&transient_plan, &transient_rendered),
        (&compact_plan, &compact_rendered),
    ] {
        assert!(rendered.root.contains(&plan.identity));
        assert!(
            rendered
                .nodes
                .iter()
                .all(|(_, source)| source.contains(&plan.identity))
        );
        assert!(rendered.selector.contains(&plan.identity));
    }
    let transient_root = transient_rendered
        .root
        .replace(&transient_plan.identity, "<generation>");
    let compact_root = compact_rendered
        .root
        .replace(&compact_plan.identity, "<generation>");
    assert_eq!(transient_root, compact_root);
    assert_eq!(
        transient_rendered
            .nodes
            .iter()
            .map(|(name, source)| (
                name,
                source.replace(&transient_plan.identity, "<generation>")
            ))
            .collect::<Vec<_>>(),
        compact_rendered
            .nodes
            .iter()
            .map(|(name, source)| (name, source.replace(&compact_plan.identity, "<generation>")))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        transient_rendered.node_anchors,
        compact_rendered.node_anchors
    );
    assert_eq!(
        transient_rendered
            .selector
            .replace(&transient_plan.identity, "<generation>"),
        compact_rendered
            .selector
            .replace(&compact_plan.identity, "<generation>"),
    );
}
