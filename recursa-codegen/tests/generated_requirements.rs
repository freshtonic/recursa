use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/requirements";

#[test]
fn a_build_removes_a_config_gated_element_exactly_as_a_cfg_does() {
    let package = scaffold_fixture();
    let output = run_fixture(&package, &[]);

    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    // The baseline build states `v1`, so every later element is gone: the subquery alias is
    // required again, and the `v3` filter variant does not exist. Nothing reports a
    // requirement, because nothing above the baseline survives.
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "build=v1\nscale=v1,v2,v3\nplain=- | -\nwith_alias=- | -\n",
    );
}

#[test]
fn a_newer_build_reports_the_requirement_an_older_configuration_would_reject() {
    let package = scaffold_fixture();
    let output = run_fixture(&package, &["--features", "since-v3"]);

    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let reported = String::from_utf8(output.stdout).unwrap();
    let lines = reported.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "build=v3");
    assert_eq!(lines[1], "scale=v1,v2,v3");
    // A statement with no gated construct needs only the baseline.
    assert_eq!(lines[2], "plain=- | -");
    assert_eq!(lines[3], "with_alias=- | -");
    // The requirement belongs to the absence of the alias, and it carries the gate's message
    // data and the span of the construct that lacks the value.
    assert_eq!(
        lines[4],
        "missing_alias=v2@14..31/42601/Subquery.alias | v2@14..31/42601/Subquery.alias",
    );
    // A detached value answers the same question without a span.
    assert_eq!(lines[5], "detached=v2@?/42601/Subquery.alias");
    // The minimum is the highest requirement; the first construct above a floor is the
    // earliest one in source order. The two answers differ here, and both are right.
    assert_eq!(
        lines[6],
        "nearby=v3@32..46/-/Filter::Nearby | v2@14..31/42601/Subquery.alias",
    );
    assert_eq!(
        lines[7],
        "nearby_above_v2=v3@32..46/-/Filter::Nearby | v3@32..46/-/Filter::Nearby",
    );
    // A repetition field records once, at the element that holds the values.
    assert_eq!(
        lines[8],
        "columns=v3@36..42/-/Alias.columns | v3@36..42/-/Alias.columns",
    );
    assert_eq!(lines.len(), 9, "{reported}");
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated requirements package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in [
        "build.rs",
        "Cargo.lock",
        "src/lib.rs",
        "src/grammar.rs",
        "src/main.rs",
    ] {
        copy(&fixture, package.path(), relative);
    }

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generated requirements fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generated requirements fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination)
        .expect("copy generated requirements fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir, features: &[&str]) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .args(features)
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run generated requirements fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}
