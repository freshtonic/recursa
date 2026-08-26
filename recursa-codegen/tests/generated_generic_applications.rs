use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/generic-applications";

#[test]
fn a_generic_node_can_be_parsed_through_multiple_closed_applications() {
    let package = scaffold_fixture();
    let output = run_fixture(&package);

    assert_success(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "generic-applications-ok"
    );
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generic-applications package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative);
    }

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generic-applications fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generic-applications fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generic-applications fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run generic-applications fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
