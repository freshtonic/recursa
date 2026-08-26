#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write as _;

    use crate::DiagnosticStage;

    struct CargoFile(std::fs::File);

    impl crate::m3::CargoOutput for CargoFile {
        fn emit(&mut self, instruction: String) {
            writeln!(self.0, "{instruction}").expect("write Cargo build-script directive");
        }
    }

    fn fixture(source: &str) -> tempfile::TempDir {
        let package = tempfile::tempdir().expect("temporary M3 fixture");
        std::fs::write(package.path().join("grammar.rs"), source).expect("write M3 fixture");
        package
    }

    fn assert_projection(name: &str) {
        let actual = crate::m3_projection_tests::projection(name);
        let path = crate::m3_projection_tests::crate_root()
            .join("tests/fixtures/m3/projections")
            .join(format!("{name}.semantic"));
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert_eq!(actual, expected, "semantic projection drift for {name}");
    }

    #[test]
    fn global_discovery_user_errors_keep_authored_codes_without_fabricated_locations() {
        let not_a_directory = tempfile::NamedTempFile::new().expect("manifest path fixture");
        let invalid_manifest = crate::m3::analyze_source_tree(
            crate::m3::M3Input::rust_2024(not_a_directory.path(), "grammar.rs"),
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
        let absolute_path = crate::m3::analyze_source_tree(
            crate::m3::M3Input::new(
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

        let package_escape = crate::m3::analyze_source_tree(
            crate::m3::M3Input::rust_2024(package.path(), "../outside.rs"),
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
        let error = crate::m3::analyze_source_tree(
            crate::m3::M3Input::rust_2024(package.path(), "grammar.rs"),
            &mut cargo,
        )
        .expect_err("two unresolved references must stop before lowering and M2");

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
        let error = crate::m3::analyze_source_tree(
            crate::m3::M3Input::rust_2024(package.path(), "grammar.rs"),
            &mut Vec::new(),
        )
        .expect_err("independent source-model failures must stop before materialization and M2");
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
        let error = crate::m3::analyze_source_tree(
            crate::m3::M3Input::rust_2024(package.path(), "grammar.rs"),
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
        let error = crate::m3::analyze_source_tree(
            crate::m3::M3Input::rust_2024(package.path(), "grammar.rs"),
            &mut cargo,
        )
        .expect_err("unresolved reference must fail M3");

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

        let success = crate::m3::analyze_source_tree(
            crate::m3::M3Input::rust_2024(package.path(), "grammar.rs"),
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
                crate::m3::analyze_source_tree(
                    crate::m3::M3Input::rust_2024(package.path(), selected_root),
                    &mut Vec::new(),
                )
            }));
            let result = result.expect("malformed input must not panic");
            let error = result.expect_err("malformed input must return structured diagnostics");
            assert!(!error.diagnostics().is_empty());
        }
    }

    #[test]
    fn m2_conflict_remedies_keep_typed_semantic_operands_without_ids() {
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

        let shared = crate::m3::adapt_conflict_remedy(
            &crate::ConflictRemedy::RestructureSharedPrefix { left, right },
            &grammar,
        );
        assert!(matches!(
            shared,
            crate::DiagnosticRemedy::RestructureSharedPrefix { left, right }
                if left == "Expr::Left" && right == "Expr::Right"
        ));
        let pratt = crate::m3::adapt_conflict_remedy(
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
        let nullable = crate::m3::adapt_conflict_remedy(
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
        let root = crate::m3_projection_tests::crate_root()
            .parent()
            .expect("recursa-codegen has a workspace parent")
            .to_owned();
        let script =
            std::fs::read_to_string(root.join("scripts/update-m3-projection-candidates.sh"))
                .expect("read candidate command");
        let writer =
            std::fs::read_to_string(root.join("recursa-codegen/src/m3_projection_tests.rs"))
                .expect("read candidate writer");

        assert!(script.contains("target/m3-projection-candidates"));
        assert!(script.contains("--ignored --exact --nocapture"));
        assert!(writer.contains("target/m3-projection-candidates.staging"));
        assert!(!writer.contains("std::fs::write(expected"));
        assert!(!writer.contains("std::fs::rename(&staging, &expected"));
    }

    #[test]
    fn m3_stays_private_and_m4_owns_the_generation_publication_surface() {
        let root = crate::m3_projection_tests::crate_root();
        let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("read public seam");
        let m3 = std::fs::read_to_string(root.join("src/m3.rs")).expect("read M3 implementation");
        let generation = std::fs::read_to_string(root.join("src/generation.rs"))
            .expect("read M4 generation implementation");

        assert!(!lib.contains("pub mod m3"));
        assert!(lib.contains("mod generation;"));
        assert!(lib.contains("pub use generation::{Generation, generate};"));
        assert!(!m3.contains("OUT_DIR"));
        assert!(!m3.contains("RECURSA_GENERATION"));
        assert!(!m3.contains("fs::rename"));
        assert!(generation.contains("pub fn generate"));
        assert!(generation.contains("OUT_DIR"));
        assert!(!generation.contains("rustc-env=RECURSA_GENERATION"));
        assert!(generation.contains("SELECTED_GENERATION"));
        assert!(generation.contains("fs::rename"));
    }
}
