use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/requirements";

/// The AST storage the scaffolded grammar declares.
///
/// One grammar template serves both, because the two parse wrappers answer the same two
/// questions with the same two methods.
#[derive(Clone, Copy)]
enum Ast {
    /// Ordinary nested storage: a parse returns `Parsed<Query>`.
    Value,
    /// `arena_ast`: a parse returns `ArenaParsed`.
    Arena,
}

/// Every line a `v3` build prints, whichever AST storage it holds.
///
/// The arena build is required to print exactly these, so the two storages agree on every
/// configuration and on every span.
const REPORTED_BY_V3: &[&str] = &[
    "build=v3",
    "scale=v1,v2,v3",
    // A statement with no gated construct needs only the baseline.
    "plain=- | -",
    "with_alias=- | -",
    // The requirement belongs to the absence of the alias, and it carries the gate's message
    // data and the span of the construct that lacks the value.
    "missing_alias=v2@14..31/42601/Subquery.alias | v2@14..31/42601/Subquery.alias",
    // A detached value answers the same question without a span.
    "detached=v2@?/42601/Subquery.alias",
    // The minimum is the highest requirement; the first construct above a floor is the
    // earliest one in source order. The two answers differ here, and both are right.
    "nearby=v3@38..46/-/Filter::Nearby | v2@14..31/42601/Subquery.alias",
    "nearby_above_v2=v3@38..46/-/Filter::Nearby | v3@38..46/-/Filter::Nearby",
    // The gated variant is the second filter of the repetition, so its span is the item's own
    // extent and not the extent of the whole list.
    "nearby_second=v3@25..33/-/Filter::Nearby | v3@25..33/-/Filter::Nearby",
    // A repetition field records once, at the element that holds the values.
    "columns=v3@36..42/-/Alias.columns | v3@36..42/-/Alias.columns",
];

#[test]
fn a_build_removes_a_config_gated_element_exactly_as_a_cfg_does() {
    let package = scaffold_fixture(Ast::Value);
    let output = run_fixture(&package, Ast::Value, &[]);

    // The baseline build states `v1`, so every later element is gone: the subquery alias is
    // required again, and the `v3` filter variant does not exist. Nothing reports a
    // requirement, because nothing above the baseline survives.
    assert_eq!(
        reported(&output),
        ["build=v1", "scale=v1,v2,v3", "plain=- | -", "with_alias=- | -"],
    );
}

#[test]
fn a_newer_build_reports_the_requirement_an_older_configuration_would_reject() {
    let package = scaffold_fixture(Ast::Value);
    let output = run_fixture(&package, Ast::Value, &["since-v3"]);

    assert_eq!(reported(&output), REPORTED_BY_V3);
}

#[test]
fn an_arena_parse_reports_every_requirement_with_the_span_a_value_parse_reports() {
    let package = scaffold_fixture(Ast::Arena);
    let output = run_fixture(&package, Ast::Arena, &["since-v3"]);

    // An arena-parsed value scans from its own root occurrence cursor, so it answers with the
    // same configuration and the same extent as the value form: the absence of the alias, the
    // gated variant inside the filter repetition, and the gated optional column list.
    assert_eq!(reported(&output), REPORTED_BY_V3);
}

/// Returns the lines the fixture printed, or fails with everything it wrote.
fn reported(output: &Output) -> Vec<&str> {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    std::str::from_utf8(&output.stdout)
        .expect("fixture output is text")
        .lines()
        .collect()
}

fn scaffold_fixture(ast: Ast) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated requirements package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in ["build.rs", "Cargo.lock", "src/lib.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative);
    }

    let grammar = fs::read_to_string(fixture.join("src/grammar.rs.in"))
        .expect("read generated requirements fixture grammar")
        .replace(
            "{{AST}}",
            match ast {
                Ast::Value => "",
                Ast::Arena => "arena_ast,",
            },
        );
    fs::write(package.path().join("src/grammar.rs"), grammar)
        .expect("write generated requirements fixture grammar");

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

/// Runs the scaffolded fixture, with the feature that states which grammar it holds.
fn run_fixture(package: &TempDir, ast: Ast, features: &[&str]) -> Output {
    let mut selected = features.to_vec();
    if matches!(ast, Ast::Arena) {
        selected.push("arena-ast");
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet");
    if !selected.is_empty() {
        command.arg("--features").arg(selected.join(","));
    }
    command
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
