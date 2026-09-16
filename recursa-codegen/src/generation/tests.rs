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

/// A `Negate` prefix variant whose `MINUS` has no declared precedence and no
/// `#[parse(prec = ...)]` override, so the LALR(1) construction reaches every operator token
/// with an unresolved shift/reduce conflict in the `MINUS Expr .` state (`docs/declared-
/// precedence.md`'s evidence case, before its `#[parse(prec = UMINUS)]` override fix).
const UNRESOLVED_PREFIX_CONFLICT_GRAMMAR: &str = r#"
recursa::grammar! {
    module = crate::grammar,
    keyword_matching = sensitive,
}

recursa::tokens! {
    ignore = r"[ \t\n\r]+",
    punctuation {
        PLUS => "+",
        MINUS => "-",
        STAR => "*",
    }
    precedence {
        left(bp = 10) { PLUS },
        left(bp = 20) { STAR },
    }
}

recursa::ast_node! {
    pub struct Integer {
        #[lex(pattern = r"[0-9]+")]
        pub text: IntegerText,
    }
}

recursa::ast_node! {
    pub enum Expr {
        Integer(Integer),
        Negate(#[tok(MINUS, this)] boxed!(Self)),
        Multiply(boxed!(Self), #[tok(STAR, this)] boxed!(Self)),
        Add(boxed!(Self), #[tok(PLUS, this)] boxed!(Self)),
    }
}
"#;

/// An unresolved conflict in an authored grammar is an ordinary `RCA0400` build error, not the
/// internal-invariant `RCA9105`: it stays at the `Analysis` stage, keeps the automaton dump
/// content (state path, items and competing actions) as its evidence witness, and its summary
/// carries the authoring remedy of declaring a precedence level or an override.
#[test]
fn an_unresolved_shift_reduce_conflict_is_an_ordinary_rca0400_build_error() {
    let package = tempfile::tempdir().expect("temporary conflict fixture");
    fs::write(
        package.path().join("grammar.rs"),
        UNRESOLVED_PREFIX_CONFLICT_GRAMMAR,
    )
    .expect("write conflict fixture");
    let success = analyze_source_tree(
        SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
        &mut Vec::new(),
    )
    .expect("the conflict is only reachable once source analysis succeeds");
    let plan = EmissionPlan::join(&success.analyzed, &CargoCfgSnapshot::default())
        .expect("the grammar has no emission-plan-level defect");
    let error = match render_lr_parser(&plan, &success.analyzed.lowered) {
        Ok(_) => panic!("MINUS has no declared precedence and no override: this must conflict"),
        Err(LrBuildError::Internal(detail)) => {
            panic!("expected unresolved conflicts, not an internal invariant: {detail}")
        }
        Err(LrBuildError::Diagnostics(diagnostics)) => {
            lr_conflict_error(Path::new("grammar.rs"), &diagnostics)
        }
    };
    let diagnostics = error.report().diagnostics();
    assert!(!diagnostics.is_empty(), "at least one conflict is reported");
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.code().as_str(), "RCA0400");
        assert_eq!(diagnostic.stage(), DiagnosticStage::Analysis);
        assert!(
            diagnostic
                .primary()
                .message()
                .contains("declare a precedence level for the lookahead token"),
            "{}",
            diagnostic.primary().message()
        );
        assert!(
            diagnostic
                .primary()
                .message()
                .contains("`#[parse(prec = NAME)]`"),
            "{}",
            diagnostic.primary().message()
        );
        let crate::DiagnosticEvidence::SemanticWitness { witness, .. } = diagnostic.evidence()
        else {
            panic!("a conflict's evidence is a semantic witness");
        };
        assert!(
            witness.iter().any(|line| line.starts_with("path: ")),
            "{witness:?}"
        );
        assert!(
            witness.iter().any(|line| line.starts_with("item: ")),
            "{witness:?}"
        );
        assert!(
            witness.iter().any(|line| line.starts_with("action: ")),
            "{witness:?}"
        );
    }
}
