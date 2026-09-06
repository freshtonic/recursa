//! Predictive dispatch over enum variants that share a long suffix (#127): the frozen
//! trie commits where the variants part instead of walking into the suffix's language.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/shared-suffix";

#[test]
fn shared_suffix_variants_commit_where_they_part() {
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
        "shared-suffix-ok"
    );
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary shared-suffix package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        let destination = package.path().join(relative);
        fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
        fs::copy(fixture.join(relative), destination).expect("copy shared-suffix fixture file");
    }
    let lock = fs::read_to_string(manifest_dir.join("tests/fixtures/generation/lexing/Cargo.lock"))
        .expect("read shared generated-fixture lockfile")
        .replace(
            "recursa-generated-lexing",
            "recursa-generated-shared-suffix",
        );
    fs::write(package.path().join("Cargo.lock"), lock).expect("write shared-suffix lockfile");

    let manifest = fs::read_to_string(fixture.join("Cargo.toml.in"))
        .expect("read shared-suffix fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write shared-suffix fixture manifest");
    package
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
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run shared-suffix fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}
