use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/predictive";

#[test]
fn frozen_first_k_and_follow_plans_drive_generated_predictive_parsers() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);

    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "predictive-ok"
    );

    let first_selection = generated_selection(package.path());
    let grammar_path = package.path().join("src/grammar.rs");
    let grammar = fs::read_to_string(&grammar_path).expect("read copied predictive grammar");
    fs::write(&grammar_path, grammar).expect("force Cargo to repeat predictive generation");

    let repeated = run_fixture(&package);
    assert!(
        repeated.status.success(),
        "repeated fixture generation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&repeated.stdout),
        String::from_utf8_lossy(&repeated.stderr),
    );
    assert_eq!(generated_selection(package.path()), first_selection);
}

fn generated_selection(package: &Path) -> Vec<u8> {
    let mut selections = Vec::new();
    collect_generated_selections(&target_dir_path(package), &mut selections);
    assert_eq!(
        selections.len(),
        1,
        "predictive fixture must publish exactly one selectable generation"
    );
    fs::read(selections.pop().unwrap()).expect("read generated predictive selection")
}

fn collect_generated_selections(directory: &Path, selections: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("read target entry").path();
        if path.is_dir() {
            collect_generated_selections(&path, selections);
        } else if path
            .file_name()
            .is_some_and(|name| name == "recursa-selected.rs")
        {
            selections.push(path);
        }
    }
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary predictive package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative);
    }
    let lock = fs::read_to_string(manifest_dir.join("tests/fixtures/m4/lexing/Cargo.lock"))
        .expect("read shared generated-fixture lockfile")
        .replace("recursa-generated-lexing", "recursa-generated-predictive");
    fs::write(package.path().join("Cargo.lock"), lock).expect("write predictive fixture lockfile");

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read predictive fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write predictive fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy predictive fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run predictive fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    target_dir_path(package.path())
}

fn target_dir_path(package: &Path) -> PathBuf {
    package.join("target")
}
