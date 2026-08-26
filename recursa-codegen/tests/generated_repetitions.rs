use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/repetitions";

#[test]
fn a_downstream_crate_honors_capture_off() {
    run_case(CaptureMode::Off);
}

#[test]
fn a_downstream_crate_honors_optional_capture_and_parses_repetitions() {
    run_case(CaptureMode::Optional);
}

#[test]
fn a_downstream_crate_honors_mandatory_capture() {
    run_case(CaptureMode::Mandatory);
}

#[derive(Clone, Copy)]
enum CaptureMode {
    Off,
    Optional,
    Mandatory,
}

impl CaptureMode {
    const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Optional => "optional",
            Self::Mandatory => "mandatory",
        }
    }
}

fn run_case(mode: CaptureMode) {
    let package = scaffold_fixture(mode);
    let output = run_fixture(&package, mode);

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("repetitions-ok-{}", mode.name())
    );
}

fn scaffold_fixture(mode: CaptureMode) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    copy(&fixture, package.path(), "build.rs");
    copy(&fixture, package.path(), "Cargo.lock");
    copy(&fixture, package.path(), "src/lib.rs");
    copy(&fixture, package.path(), "src/main.rs");

    let grammar = fs::read_to_string(fixture.join("src/grammar.rs.in"))
        .expect("read generated grammar fixture source")
        .replace("__RECURSA_SPANS_MODE__", mode.name());
    fs::write(package.path().join("src/grammar.rs"), grammar)
        .expect("write generated grammar fixture source");

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read generated grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write generated grammar fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generated grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir, mode: CaptureMode) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("RECURSA_CAPTURE_MODE", mode.name())
        .env("CARGO_TARGET_DIR", package.path().join("target"));
    command.output().expect("run generated grammar fixture")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
