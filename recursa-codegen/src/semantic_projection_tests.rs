use std::path::{Path, PathBuf};

pub(crate) const FIXTURES: [&str; 6] = ["toy", "arith", "json", "sql", "optq", "cskw"];

pub(crate) fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_owned()
}

fn workspace_root() -> PathBuf {
    crate_root()
        .parent()
        .expect("recursa-codegen is in the workspace root")
        .to_owned()
}

pub(crate) fn projection(name: &str) -> String {
    let success = analyzed(name);
    crate::semantic_projection::canonical_fixture_projection(name, &success.analyzed)
}

fn analyzed(name: &str) -> crate::source_analysis::SourceAnalysisSuccess {
    let fixture = crate_root()
        .join("tests/fixtures/source-analysis")
        .join(name);
    crate::source_analysis::analyze_source_tree(
        crate::source_analysis::SourceAnalysisInput::rust_2024(&fixture, "grammar.rs"),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("fixture {name} failed source analysis:\n{error}"))
}

#[test]
fn six_rust_2024_fixtures_match_location_and_id_free_semantic_projections() {
    let expected = crate_root().join("tests/fixtures/source-analysis/projections");
    for name in FIXTURES {
        let actual = projection(name);
        let path = expected.join(format!("{name}.semantic"));
        let pinned = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert_eq!(actual, pinned, "semantic projection drift for {name}");
        assert!(!actual.contains("/private/"));
        assert!(!actual.contains(".rs:"));
        assert!(!actual.contains("type["));
        assert!(!actual.contains("site["));
    }
}

#[test]
fn projection_exposes_parse_site_swaps_and_pratt_lhs_correlation_loss() {
    use crate::materialization::MaterializedFieldLowering;

    let mut success = analyzed("arith");
    let original =
        crate::semantic_projection::canonical_fixture_projection("arith", &success.analyzed);
    let distinct_parse_sites = success
        .analyzed
        .lowered
        .lowering_map
        .fields
        .iter()
        .enumerate()
        .map(|(index, correlation)| {
            let MaterializedFieldLowering::ParseSite(site) = correlation.lowered;
            (index, site)
        })
        .collect::<Vec<_>>();
    let ((left_index, left), (right_index, right)) = distinct_parse_sites
        .iter()
        .enumerate()
        .find_map(|(offset, left)| {
            distinct_parse_sites[offset + 1..]
                .iter()
                .find(|right| left.1 != right.1)
                .map(|right| (*left, *right))
        })
        .expect("arithmetic fixture has distinct correlated parse sites");
    success.analyzed.lowered.lowering_map.fields[left_index].lowered =
        MaterializedFieldLowering::ParseSite(right);
    success.analyzed.lowered.lowering_map.fields[right_index].lowered =
        MaterializedFieldLowering::ParseSite(left);
    let swapped =
        crate::semantic_projection::canonical_fixture_projection("arith", &success.analyzed);
    assert_ne!(swapped, original, "same-typed field swaps must be visible");

    let mut success = analyzed("arith");
    success.analyzed.lowered.lowering_map.fields.pop();
    let missing_field =
        crate::semantic_projection::canonical_fixture_projection("arith", &success.analyzed);
    assert_ne!(
        missing_field, original,
        "field correlation loss must be visible"
    );
}

#[test]
#[ignore = "candidate-only maintainer command; never updates committed expectations"]
fn write_semantic_projection_candidates() {
    let root = workspace_root();
    let candidate = root.join("target/semantic-projection-candidates");
    let staging = root.join("target/semantic-projection-candidates.staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).expect("remove stale candidate staging directory");
    }
    std::fs::create_dir_all(&staging).expect("create candidate staging directory");
    let projections = FIXTURES
        .into_iter()
        .map(|name| (name, projection(name)))
        .collect::<Vec<_>>();
    for (name, projection) in projections {
        std::fs::write(staging.join(format!("{name}.semantic")), projection)
            .expect("write projection candidate");
    }
    if candidate.exists() {
        std::fs::remove_dir_all(&candidate).expect("replace old candidate directory");
    }
    std::fs::rename(&staging, &candidate).expect("publish complete candidate set");
    eprintln!("wrote candidates to {}", candidate.display());
}
