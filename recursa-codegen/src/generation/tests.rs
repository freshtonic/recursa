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
