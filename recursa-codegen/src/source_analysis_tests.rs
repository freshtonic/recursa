#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write as _;

    use crate::DiagnosticStage;

    struct CargoFile(std::fs::File);

    impl crate::source_analysis::CargoOutput for CargoFile {
        fn emit(&mut self, instruction: String) {
            writeln!(self.0, "{instruction}").expect("write Cargo build-script directive");
        }
    }

    fn fixture(source: &str) -> tempfile::TempDir {
        let package = tempfile::tempdir().expect("temporary source-analysis fixture");
        std::fs::write(package.path().join("grammar.rs"), source)
            .expect("write source-analysis fixture");
        package
    }

    fn assert_projection(name: &str) {
        let actual = crate::semantic_projection_tests::projection(name);
        let path = crate::semantic_projection_tests::crate_root()
            .join("tests/fixtures/source-analysis/projections")
            .join(format!("{name}.semantic"));
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert_eq!(actual, expected, "semantic projection drift for {name}");
    }

    #[test]
    fn global_discovery_user_errors_keep_authored_codes_without_fabricated_locations() {
        let not_a_directory = tempfile::NamedTempFile::new().expect("manifest path fixture");
        let invalid_manifest = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(
                not_a_directory.path(),
                "grammar.rs",
            ),
            &mut Vec::new(),
        )
        .expect_err("a file cannot be used as the Cargo manifest directory");
        assert_eq!(invalid_manifest.diagnostics().len(), 1);
        assert_eq!(invalid_manifest.diagnostics()[0].code().as_str(), "RCA1001");
        assert!(
            invalid_manifest.diagnostics()[0]
                .primary()
                .location()
                .is_none()
        );

        let package = fixture("recursa::grammar! { module = crate }");
        let absolute_root = package.path().join("grammar.rs");
        let absolute_path = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::new(
                package.path(),
                &absolute_root,
                crate::lowering::RustEdition::Rust2024,
                crate::source_syntax::CargoCfgSnapshot::default(),
            ),
            &mut Vec::new(),
        )
        .expect_err("the selected root must be manifest-relative");
        assert_eq!(absolute_path.diagnostics().len(), 1);
        assert_eq!(absolute_path.diagnostics()[0].code().as_str(), "RCA1002");
        assert!(
            absolute_path.diagnostics()[0]
                .primary()
                .location()
                .is_none()
        );

        let package_escape = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "../outside.rs"),
            &mut Vec::new(),
        )
        .expect_err("the selected root cannot escape the Cargo package");
        assert_eq!(package_escape.diagnostics().len(), 1);
        assert_eq!(package_escape.diagnostics()[0].code().as_str(), "RCA1003");
        assert!(
            package_escape.diagnostics()[0]
                .primary()
                .location()
                .is_none()
        );
    }

    #[test]
    fn diagnostic_phases_collect_independent_failures_and_suppress_downstream_phases() {
        let package = fixture(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    left: MissingLeft,
    right: MissingRight,
}
"#,
        );
        let mut cargo = Vec::new();
        let error = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut cargo,
        )
        .expect_err("two unresolved references must stop before lowering and semantic analysis");

        assert_eq!(error.diagnostics().len(), 2);
        assert!(
            error
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.stage() == DiagnosticStage::Resolution)
        );
        assert!(
            error
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.code().as_str() == "RCA2012")
        );

        let package = fixture(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = "a", display = "A")]
    left: Left,
    #[lex(pattern = "b", display = "B")]
    right: Right,
}
"#,
        );
        let error = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect_err("independent source-model failures must stop before materialization and semantic analysis");
        assert_eq!(error.diagnostics().len(), 2);
        assert!(
            error
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.stage() == DiagnosticStage::Lowering)
        );
        assert!(
            error
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.code().as_str() == "RCA3111")
        );

        let package = fixture(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Leaf;

#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"\bword")]
    word: Word,
    nested: Vec<Vec<Leaf>>,
}
"#,
        );
        let error = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect_err("lexical and structural materialization failures share Lowering");
        let codes = error
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            codes,
            std::collections::BTreeSet::from(["RCA3004", "RCA3206"])
        );
        assert!(
            error
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.stage() == DiagnosticStage::Lowering)
        );
    }

    #[test]
    fn generation_failure_renders_once_and_retains_invalidation_inputs() {
        let package = fixture(
            r#"
recursa::grammar! { module = crate }

#[derive(recursa::Node)]
struct Root { missing: Missing }
"#,
        );
        let mut cargo = Vec::new();
        let error = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut cargo,
        )
        .expect_err("unresolved reference must fail source analysis");

        assert_eq!(cargo, ["cargo::rerun-if-changed=grammar.rs"]);
        let rendered = error.to_string();
        assert_eq!(rendered, error.report().canonical_rendering());
        assert_eq!(rendered.matches("error[RCA2012]").count(), 1);
        let as_error: &dyn std::error::Error = &error;
        assert!(as_error.source().is_none());
    }

    #[test]
    fn successful_analysis_warnings_cross_the_cargo_output_boundary_once() {
        let package = fixture(
            r#"
recursa::grammar! { module = crate, max_lookahead = 1 }
recursa::tokens! { keywords { SELECT => "SELECT" } }

#[derive(recursa::Node)]
#[tok(SELECT)]
struct Select;

#[derive(recursa::Node)]
struct Inner {
    maybe: Option<Select>,
}

#[derive(recursa::Node)]
struct Root {
    #[sep(SELECT)]
    items: Vec<Inner>,
}
"#,
        );
        let directives = tempfile::NamedTempFile::new().expect("Cargo output capture");
        let mut cargo = CargoFile(
            directives
                .reopen()
                .expect("open Cargo output capture for writing"),
        );

        let success = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut cargo,
        )
        .expect("a nullable FOLLOW warning must coexist with analysis output");
        drop(cargo);

        let warnings = success.build_report.warnings().collect::<Vec<_>>();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().all(|warning| {
            warning.code().as_str() == "RCA0300"
                && matches!(
                    warning.remedies(),
                    [crate::DiagnosticRemedy::ReviewNullableFollow { site }]
                        if site.contains("::")
                            && !site.chars().all(|character| character.is_ascii_digit())
                )
        }));
        let directives =
            std::fs::read_to_string(directives.path()).expect("read Cargo output boundary capture");
        assert_eq!(
            directives.matches("cargo::warning=[RCA0300]").count(),
            warnings.len(),
        );
        assert_eq!(
            directives
                .matches("cargo::rerun-if-changed=grammar.rs")
                .count(),
            1
        );
    }

    #[test]
    fn duplicate_admission_warning_crosses_the_full_analysis_boundary() {
        let package = fixture(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! {
    categories { UNRESERVED }
    keywords { WINDOW => "window" in UNRESERVED }
    admissions { Names = UNRESERVED + { WINDOW } }
}
#[derive(recursa::Node)]
struct Root {
    #[lex(pattern = r"[A-Za-z]+", admits(Names))]
    name: Name,
}
"#,
        );
        let mut cargo = Vec::new();

        let success = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut cargo,
        )
        .expect("duplicate union membership is advisory");

        let warnings = success.build_report.warnings().collect::<Vec<_>>();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code().as_str(), "RCA3119");
        assert_eq!(
            cargo
                .iter()
                .filter(|directive| directive.contains("cargo::warning=[RCA3119]"))
                .count(),
            1,
        );
    }

    #[test]
    fn source_analysis_reuses_published_decision_validation_but_corruption_checks_remain_available()
    {
        let package = fixture(
            r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { SELECT => "select" } }

#[derive(recursa::Node)]
#[tok(SELECT)]
struct Root;
"#,
        );
        crate::decision_plan::reset_independent_decision_validations_for_test();
        crate::decision_plan::reset_decision_plan_constructions_for_test();

        let success = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect("the source grammar is valid");

        assert_eq!(
            crate::decision_plan::independent_decision_validations_for_test(),
            0,
            "publication already validated the originating decision product",
        );
        assert_eq!(
            crate::decision_plan::decision_plan_constructions_for_test(),
            1,
            "publication must validate the product without reconstructing it",
        );

        let output = success
            .analyzed
            .analysis
            .output
            .as_ref()
            .expect("successful analysis output");
        let mut corrupted = output.decisions().clone();
        corrupted.corrupt_site_product_for_test();
        assert!(
            corrupted
                .validate(&success.analyzed.lowered.grammar, output.first_by_type())
                .is_err(),
            "the independent validation seam must still reject corrupted products",
        );
        assert_eq!(
            crate::decision_plan::independent_decision_validations_for_test(),
            1,
        );
    }

    #[test]
    fn malformed_full_pipeline_inputs_return_diagnostics_without_panicking() {
        let cases = [
            ("{{", "grammar.rs"),
            ("recursa::grammar! { module = }", "grammar.rs"),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
struct Root { #[lex(pattern = r"\bword")] word: Word }
"#,
                "grammar.rs",
            ),
            ("", "../outside.rs"),
        ];

        for (source, selected_root) in cases {
            let package = fixture(source);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::source_analysis::analyze_source_tree(
                    crate::source_analysis::SourceAnalysisInput::rust_2024(
                        package.path(),
                        selected_root,
                    ),
                    &mut Vec::new(),
                )
            }));
            let result = result.expect("malformed input must not panic");
            let error = result.expect_err("malformed input must return structured diagnostics");
            assert!(!error.diagnostics().is_empty());
        }
    }

    #[test]
    fn invalid_presence_and_optional_fixed_syntax_reports_source_analysis_diagnostics() {
        let cases = [
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
struct Root { value: bool }
"#,
                "RCA2012",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
struct Root { #[presence(this)] value: bool }
"#,
                "RCA3129",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
struct Root { #[presence()] value: bool }
"#,
                "RCA3129",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
struct Root { #[presence] value: bool }
"#,
                "RCA3129",
            ),
            (
                r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { OR => "or" } }
#[derive(recursa::Node)]
struct Root {
    #[presence(OR)]
    #[presence(OR)]
    value: bool,
}
"#,
                "RCA3129",
            ),
            (
                r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { OR => "or" } }
#[derive(recursa::Node)]
struct Root { #[presence(optional(OR))] value: bool }
"#,
                "RCA3129",
            ),
            (
                r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { OR => "or" } }
#[derive(recursa::Node)]
#[tok(OR)]
struct Flag;
#[derive(recursa::Node)]
struct Root { #[presence(OR)] value: Flag }
"#,
                "RCA3129",
            ),
            (
                r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { OR => "or", REPLACE => "replace" } }
#[derive(recursa::Node)]
#[tok(optional())]
struct Root;
"#,
                "RCA3103",
            ),
            (
                r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { OR => "or", REPLACE => "replace" } }
#[derive(recursa::Node)]
#[tok(optional(OR, optional(REPLACE)))]
struct Root;
"#,
                "RCA3103",
            ),
            (
                r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { OR => "or" } }
#[derive(recursa::Node)]
#[tok(optional(OR, this))]
struct Root;
"#,
                "RCA3103",
            ),
            (
                r#"
recursa::grammar! { module = crate }
recursa::tokens! { keywords { OR => "or" } }
#[derive(recursa::Node)]
struct Root {
    #[presence(OR)]
    #[lex(pattern = "or")]
    value: bool,
}
"#,
                "RCA3129",
            ),
            (
                r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
#[kwd("OLD")]
struct Root;
"#,
                "RCA3113",
            ),
        ];

        for (source, code) in cases {
            let package = fixture(source);
            let error = crate::source_analysis::analyze_source_tree(
                crate::source_analysis::SourceAnalysisInput::rust_2024(
                    package.path(),
                    "grammar.rs",
                ),
                &mut Vec::new(),
            )
            .expect_err("invalid presence/fixed-group syntax must fail source analysis");
            assert!(
                error
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| diagnostic.code().as_str() == code),
                "missing {code} for source:\n{source}\n{}",
                error.report().canonical_rendering(),
            );
        }

        let package = fixture(
            r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
struct Root { #[presence(this)] value: bool }
"#,
        );
        let error = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect_err("invalid presence syntax must fail source analysis");
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "RCA3129")
        );
    }

    const GREEDY_GRAMMAR_PREFIX: &str = r#"
recursa::grammar! { module = crate, max_lookahead = 1 }
recursa::tokens! { keywords { SELECT => "SELECT", FROM => "FROM" } }

#[derive(recursa::Node)]
#[tok(SELECT)]
struct Select;

#[derive(recursa::Node)]
#[tok(FROM)]
struct From;
"#;

    /// `Inner = Select?` repeated with a `SELECT` separator: the separator is caller FOLLOW
    /// of `Inner::maybe`, so `Inner::maybe` and `Root::items` each report RCA0300 with overlap
    /// `SELECT`. The root site is always accepted, so only the inner attribute varies.
    fn greedy_grammar(inner_attribute: &str) -> String {
        format!(
            r#"{GREEDY_GRAMMAR_PREFIX}
#[derive(recursa::Node)]
struct Inner {{
    {inner_attribute}
    maybe: Option<Select>,
}}

#[derive(recursa::Node)]
struct Root {{
    #[greedy(SELECT)]
    #[sep(SELECT)]
    items: Vec<Inner>,
}}
"#
        )
    }

    fn greedy_analysis(source: &str) -> (Vec<String>, Result<Vec<String>, Vec<String>>) {
        let package = fixture(source);
        let directives = tempfile::NamedTempFile::new().expect("Cargo output capture");
        let mut cargo = CargoFile(directives.reopen().expect("open Cargo output capture"));
        let outcome = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut cargo,
        );
        drop(cargo);
        let lines = std::fs::read_to_string(directives.path())
            .expect("read Cargo output capture")
            .lines()
            .map(ToOwned::to_owned)
            .collect();
        let outcome = match outcome {
            Ok(success) => Ok(success
                .build_report
                .warnings()
                .map(|warning| warning.code().as_str().to_owned())
                .collect()),
            Err(error) => Err(error
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code().as_str().to_owned())
                .collect()),
        };
        (lines, outcome)
    }

    #[test]
    fn greedy_acceptance_suppresses_the_rca0300_warning_across_the_cargo_boundary() {
        let (baseline_lines, baseline) = greedy_analysis(&greedy_grammar(""));
        assert_eq!(baseline, Ok(vec!["RCA0300".to_owned()]));
        assert!(baseline_lines.iter().any(|line| line.contains("[RCA0300]")));

        let (lines, accepted) = greedy_analysis(&greedy_grammar("#[greedy(SELECT)]"));
        assert_eq!(
            accepted,
            Ok(Vec::new()),
            "the matching acceptance retires the warning"
        );
        assert!(
            lines.iter().all(|line| !line.contains("cargo::warning=")),
            "no warning crosses the Cargo boundary:\n{}",
            lines.join("\n"),
        );
    }

    #[test]
    fn stale_and_undeclared_greedy_acceptances_fail_source_analysis() {
        let (_, stale) = greedy_analysis(&greedy_grammar("#[greedy(SELECT, FROM)]"));
        let stale = stale.expect_err("a declared kind outside the overlap is a stale acceptance");
        assert!(stale.contains(&"RCA0302".to_owned()), "{stale:?}");
        assert!(
            stale.contains(&"RCA0300".to_owned()),
            "the mismatched site keeps its advisory warning: {stale:?}"
        );

        let (_, undeclared) = greedy_analysis(&format!(
            r#"{GREEDY_GRAMMAR_PREFIX}
#[derive(recursa::Node)]
enum Either {{
    Select(Select),
    From(From),
}}

#[derive(recursa::Node)]
struct Inner {{
    #[greedy(SELECT)]
    maybe: Option<Either>,
}}

#[derive(recursa::Node)]
struct SelectList {{
    #[greedy(SELECT)]
    #[sep(SELECT)]
    items: Vec<Inner>,
}}

#[derive(recursa::Node)]
struct FromList {{
    #[greedy(FROM)]
    #[sep(FROM)]
    items: Vec<Inner>,
}}
"#
        ));
        let undeclared =
            undeclared.expect_err("an overlap kind outside the declaration is undeclared");
        assert!(undeclared.contains(&"RCA0303".to_owned()), "{undeclared:?}");
        assert!(
            !undeclared.contains(&"RCA0302".to_owned()),
            "{undeclared:?}"
        );
    }

    #[test]
    fn malformed_greedy_acceptances_report_source_analysis_diagnostics() {
        for attribute in [
            "#[greedy]",
            "#[greedy()]",
            "#[greedy(NOPE)]",
            "#[greedy(SELECT, SELECT)]",
        ] {
            let (_, outcome) = greedy_analysis(&greedy_grammar(attribute));
            assert_eq!(
                outcome,
                Err(vec!["RCA3131".to_owned()]),
                "{attribute} must be rejected with RCA3131",
            );
        }
        let (_, repeated) =
            greedy_analysis(&greedy_grammar("#[greedy(SELECT)]\n    #[greedy(SELECT)]"));
        assert_eq!(repeated, Err(vec!["RCA3131".to_owned()]));
    }

    #[test]
    fn invalid_pratt_extension_filter_reports_source_analysis_diagnostic() {
        let package = fixture(
            r#"
recursa::grammar! { module = crate }
#[derive(recursa::Node)]
struct Value;
#[derive(recursa::Node)]
struct Root {
    #[parse(pratt(exclude()))]
    value: Value,
}
"#,
        );
        let error = crate::source_analysis::analyze_source_tree(
            crate::source_analysis::SourceAnalysisInput::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect_err("an empty Pratt extension filter must fail source analysis");
        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "RCA3130"),
            "missing RCA3130:\n{}",
            error.report().canonical_rendering(),
        );
    }

    #[test]
    fn semantic_analysis_conflict_remedies_keep_typed_semantic_operands_without_ids() {
        let mut grammar = crate::GrammarDescription::new(
            "remedy-adaptation",
            0,
            2,
            crate::TextEquivalence::Sensitive,
        );
        let declaration = |grammar: &mut crate::GrammarDescription, name: &str| {
            let location =
                grammar.push_location(crate::Location::new(crate::SourceId::new(0), 0..1));
            grammar.push_declaration(crate::Declaration::new(name, location))
        };
        let left = declaration(&mut grammar, "Expr::Left");
        let right = declaration(&mut grammar, "Expr::Right");
        let expr_declaration = declaration(&mut grammar, "Expr");
        let expr = grammar.push_type(crate::TypeDescription::new(
            "Expr",
            expr_declaration,
            crate::TypeShape::Sequence { sites: Vec::new() },
        ));
        let site_declaration = declaration(&mut grammar, "Expr::operand");
        let site = grammar.push_site(crate::ParseSite::new(
            expr,
            expr,
            site_declaration,
            crate::ContainerShape::Direct,
        ));

        let shared = crate::source_analysis::adapt_conflict_remedy(
            &crate::ConflictRemedy::RestructureSharedPrefix { left, right },
            &grammar,
        );
        assert!(matches!(
            shared,
            crate::DiagnosticRemedy::RestructureSharedPrefix { left, right }
                if left == "Expr::Left" && right == "Expr::Right"
        ));
        let pratt = crate::source_analysis::adapt_conflict_remedy(
            &crate::ConflictRemedy::RestructurePrattSharedPrefix {
                pratt: expr,
                left,
                right,
            },
            &grammar,
        );
        assert!(matches!(
            pratt,
            crate::DiagnosticRemedy::RestructurePrattSharedPrefix {
                pratt,
                left,
                right,
            } if pratt == "Expr" && left == "Expr::Left" && right == "Expr::Right"
        ));
        let nullable = crate::source_analysis::adapt_conflict_remedy(
            &crate::ConflictRemedy::ReviewNullableFollow { site },
            &grammar,
        );
        assert!(matches!(
            nullable,
            crate::DiagnosticRemedy::ReviewNullableFollow { site }
                if site == "Expr::operand"
        ));
    }

    #[test]
    fn canonical_projection_matches_toy() {
        assert_projection("toy");
    }

    #[test]
    fn canonical_projection_matches_arith() {
        assert_projection("arith");
    }

    #[test]
    fn canonical_projection_matches_json() {
        assert_projection("json");
    }

    #[test]
    fn canonical_projection_matches_sql() {
        assert_projection("sql");
    }

    #[test]
    fn canonical_projection_matches_optq() {
        assert_projection("optq");
    }

    #[test]
    fn canonical_projection_matches_cskw() {
        assert_projection("cskw");
    }

    #[test]
    fn projection_candidate_command_never_updates_reviewed_expectations() {
        let root = crate::semantic_projection_tests::crate_root()
            .parent()
            .expect("recursa-codegen has a workspace parent")
            .to_owned();
        let script =
            std::fs::read_to_string(root.join("scripts/update-semantic-projection-candidates.sh"))
                .expect("read candidate command");
        let writer =
            std::fs::read_to_string(root.join("recursa-codegen/src/semantic_projection_tests.rs"))
                .expect("read candidate writer");

        assert!(script.contains("target/semantic-projection-candidates"));
        assert!(script.contains("--ignored --exact --nocapture"));
        assert!(writer.contains("target/semantic-projection-candidates.staging"));
        assert!(!writer.contains("std::fs::write(expected"));
        assert!(!writer.contains("std::fs::rename(&staging, &expected"));
    }

    #[test]
    fn source_analysis_stays_private_and_generation_owns_the_publication_surface() {
        let root = crate::semantic_projection_tests::crate_root();
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("read public seam");
        let source_analysis = std::fs::read_to_string(root.join("src/source_analysis.rs"))
            .expect("read source-analysis implementation");
        let generation = std::fs::read_to_string(root.join("src/generation.rs"))
            .expect("read generation implementation");
        let publication = std::fs::read_to_string(root.join("src/generation/publication.rs"))
            .expect("read generation publication implementation");

        assert!(!lib.contains("pub mod source_analysis"));
        assert!(lib.contains("mod generation;"));
        assert!(lib.contains("pub use generation::{Generation, generate};"));
        assert!(!source_analysis.contains("OUT_DIR"));
        assert!(!source_analysis.contains("RECURSA_GENERATION"));
        assert!(!source_analysis.contains("fs::rename"));
        assert!(generation.contains("pub fn generate"));
        assert!(generation.contains("OUT_DIR"));
        assert!(!generation.contains("rustc-env=RECURSA_GENERATION"));
        assert!(publication.contains("SELECTED_GENERATION"));
        assert!(generation.contains("mod publication;"));
        assert!(publication.contains("fs::rename"));
    }
}
