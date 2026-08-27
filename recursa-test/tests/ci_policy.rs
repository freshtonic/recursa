//! Regression tests for the continuous-integration execution policy.

use std::fs;
use std::path::Path;

/// Returns one top-level job body from the checked-in CI workflow.
fn job<'workflow>(workflow: &'workflow str, name: &str, next: Option<&str>) -> &'workflow str {
    let start_marker = format!("  {name}:\n");
    let start = workflow
        .find(&start_marker)
        .unwrap_or_else(|| panic!("CI workflow has no `{name}` job"));
    let body = &workflow[start + start_marker.len()..];
    match next {
        Some(next) => {
            let end_marker = format!("  {next}:\n");
            let end = body
                .find(&end_marker)
                .unwrap_or_else(|| panic!("CI workflow has no `{next}` job after `{name}`"));
            &body[..end]
        }
        None => body,
    }
}

#[test]
fn stable_is_exhaustive_while_msrv_checks_and_smoke_tests_generated_output() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("recursa-test belongs to the workspace root");
    let workflow = fs::read_to_string(workspace.join(".github/workflows/ci.yml"))
        .expect("read the checked-in CI workflow");
    let msrv = job(&workflow, "msrv", Some("stable"));
    let stable = job(&workflow, "stable", None);

    assert!(
        stable.contains("cargo test --workspace --all-targets --all-features"),
        "Stable must execute the exhaustive workspace suite",
    );
    assert!(
        msrv.contains("cargo +1.88.0 check --workspace --all-targets --all-features --locked"),
        "MSRV must compile every workspace target and feature",
    );
    assert!(
        !msrv.contains("test --workspace"),
        "MSRV must not duplicate the exhaustive workspace suite",
    );
    assert!(
        msrv.contains("--test generated_lexing")
            && msrv.contains(
                "a_downstream_crate_observes_typed_lexing_values_and_exact_matched_text",
            ),
        "MSRV must execute the representative generated downstream fixture",
    );
}
