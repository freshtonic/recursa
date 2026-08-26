use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const SPIKE_REVISION: &str = "b410d2f123cd7510a8a40e9e5b971936a2fd8463";

const FIXTURES: [(&str, &str); 6] = [
    ("toy", "crate::toy"),
    ("arith", "crate::arith"),
    ("json", "crate::json"),
    ("sql", "crate::sql"),
    ("optq", "crate::optq"),
    ("cskw", "crate::cskw"),
];

const REQUIRED_IDS: [&str; 22] = [
    "M3-NORM-001",
    "M3-LOWER-001",
    "M3-DISC-001",
    "M3-RESOLVE-001",
    "M3-RESOLVE-002",
    "M3-LANG-001",
    "M3-MATERIAL-001",
    "M3-LEX-001",
    "M3-DIAG-001",
    "M3-DIAG-002",
    "M3-DIAG-003",
    "M3-DIAG-004",
    "M3-DIAG-005",
    "M3-PROJ-001",
    "M3-PROJ-002",
    "M3-PROJ-003",
    "M3-PROJ-004",
    "M3-PROJ-005",
    "M3-PROJ-006",
    "M3-PROJ-007",
    "M3-SCOPE-001",
    "M3-CI-001",
];

const KNOWN_EVIDENCE: [&str; 22] = [
    "cargo test -p recursa-codegen --test normalized_analysis recursive_container_shape_preserves_box_placement_and_optional_vec1_cardinality --locked",
    "cargo test -p recursa-codegen lowering::tests::canonical_projection_is_symbolic_and_byte_deterministic --locked",
    "cargo test -p recursa-codegen discovery::tests::cargo_1_88_invalidation_covers_edits_failures_candidates_and_cfg_switches --locked",
    "cargo test -p recursa-codegen resolution::tests::resolves_anchored_local_paths_self_and_the_closed_catalog --locked",
    "cargo test -p recursa-codegen resolution::tests::synthesizes_module_local_content_token_bindings_with_one_semantic_identity --locked",
    "cargo test -p recursa-codegen source_model::tests::lowers_positional_tokens_and_presence_nodes_in_source_order --locked",
    "cargo test -p recursa-codegen materialization::tests::arithmetic_generic_application_reaches_a_successful_m2_analysis --locked",
    "cargo test -p recursa-codegen lexical::tests::contextual_keyword_projection_is_stable_under_source_permutations --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::diagnostic_phases_collect_independent_failures_and_suppress_downstream_phases --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::generation_failure_renders_once_and_retains_invalidation_inputs --locked",
    "cargo test -p recursa-codegen m3_contract_tests::every_registered_code_has_real_producer_evidence --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::successful_analysis_warnings_cross_the_cargo_output_boundary_once --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::malformed_full_pipeline_inputs_return_diagnostics_without_panicking --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::canonical_projection_matches_toy --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::canonical_projection_matches_arith --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::canonical_projection_matches_json --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::canonical_projection_matches_sql --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::canonical_projection_matches_optq --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::canonical_projection_matches_cskw --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::projection_candidate_command_never_updates_reviewed_expectations --locked",
    "cargo test -p recursa-codegen m3_pipeline::tests::m3_pipeline_has_no_generation_publication_surface --locked",
    "cargo test --workspace --all-targets --locked",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("recursa-codegen has a repository parent")
        .to_path_buf()
}

#[test]
fn six_m3_fixture_sources_are_parseable_rust_2024_inputs_with_pinned_provenance() {
    for (name, module) in FIXTURES {
        let path = repository_root()
            .join("recursa-codegen/tests/fixtures/m3")
            .join(name)
            .join("grammar.rs");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

        syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parse {} as Rust: {error}", path.display()));
        assert!(source.contains("Rust 2024"), "{}", path.display());
        assert!(source.contains(SPIKE_REVISION), "{}", path.display());
        assert!(
            source.contains(&format!("module = {module}")),
            "{}",
            path.display()
        );
        assert!(!source.contains("#[kwd"), "{}", path.display());
        assert!(!source.contains("display ="), "{}", path.display());
    }
}

#[test]
fn m3_acceptance_matrix_is_complete_green_and_uses_known_evidence() {
    let path = repository_root().join("docs/acceptance/m3.md");
    let document = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let rows = matrix_rows(&document);

    let actual_ids = rows.iter().map(|row| row[0]).collect::<Vec<_>>();
    assert_eq!(actual_ids, REQUIRED_IDS);
    assert_eq!(
        actual_ids.iter().copied().collect::<BTreeSet<_>>().len(),
        REQUIRED_IDS.len()
    );

    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), 8, "matrix row {}", row[0]);
        assert_eq!(row[1], "M3", "milestone for {}", row[0]);
        assert!(!row[2].is_empty(), "originating decision for {}", row[0]);
        assert!(!row[3].is_empty(), "tier or fixture for {}", row[0]);
        assert!(!row[4].is_empty(), "evidence for {}", row[0]);
        assert!(!row[5].is_empty(), "expected result for {}", row[0]);
        assert!(
            matches!(row[6], "unit" | "integration" | "tooling" | "ci"),
            "automation class for {}",
            row[0]
        );
        assert_eq!(row[7], "Green", "status for {}", row[0]);

        let evidence = row[4].trim_matches('`');
        assert_eq!(
            evidence, KNOWN_EVIDENCE[index],
            "{} must name its exact executable evidence command",
            row[0]
        );
        assert_local_link_exists(&path, row[3], row[0]);
    }

    assert_eq!(
        rows.iter()
            .filter(|row| row[0].starts_with("M3-PROJ-") && row[0] != "M3-PROJ-007")
            .count(),
        6
    );

    for issue in [34, 40, 43, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58] {
        assert!(
            document.contains(&format!("/issues/{issue}")),
            "matrix omits originating decision #{issue}"
        );
    }
    assert!(document.contains("private, line-oriented transport seam"));
    assert!(document.contains(
        "parses each cited `#[test]` function and checks the exact diagnostic code literals in its asserted output"
    ));
    assert!(
        document.contains(
            "publication-specific transport evidence is deferred to the M4 acceptance row"
        )
    );
    assert!(!document.contains("real line-oriented Cargo-output file boundary"));
}

#[test]
fn projection_candidate_command_is_immutable_and_never_auto_accepts() {
    let path = repository_root().join("docs/acceptance/m3.md");
    let document = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

    assert!(document.contains(
        "cargo test -p recursa-codegen --lib m3_projection_tests::write_m3_projection_candidates --locked -- --ignored --exact --nocapture"
    ));
    assert!(document.contains("target/m3-projection-candidates"));
    assert!(document.contains("stages all six"));
    assert!(document.contains("publishes none"));
    assert!(document.contains("prints the complete candidate directory"));
    assert!(document.contains("compares its six files"));
    assert!(document.contains("manually copies"));
    assert!(document.contains("linked semantic decision"));
    assert!(document.contains("never writes reviewed expectations"));
}

fn matrix_rows(document: &str) -> Vec<Vec<&str>> {
    document
        .lines()
        .filter(|line| line.starts_with("| M3-"))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_local_link_exists(matrix_path: &Path, cell: &str, id: &str) {
    let target = cell
        .split("](")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap_or_else(|| panic!("{id} tier/fixture cell has no local link"));
    assert!(
        !target.contains("://"),
        "{id} tier/fixture link is not local"
    );
    let resolved = matrix_path
        .parent()
        .expect("acceptance document has a parent")
        .join(target);
    assert!(
        resolved.exists(),
        "{id} missing local link {}",
        resolved.display()
    );
}
