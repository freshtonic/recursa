use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/one-node";

#[test]
fn a_downstream_crate_loads_and_parses_one_generated_node_through_the_facade() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let main = package.path().join("src/main.rs");
    let valid_source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    let valid_main = fs::read_to_string(&main).expect("read scaffolded downstream consumer");

    let initial = run_fixture(&package);

    assert_success(&initial);
    assert_eq!(
        String::from_utf8(initial.stdout).unwrap().trim(),
        "one-node-ok"
    );

    fs::write(
        &grammar,
        "recursa::grammar! { module = crate::grammar }\n\npub struct Broken {\n",
    )
    .expect("replace the selected grammar with invalid Rust");
    let failed = run_fixture(&package);
    assert!(
        !failed.status.success(),
        "a hard generation failure must stop the Cargo build"
    );
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("RCA2001"),
        "the invalid selected grammar must fail in generation, not only in downstream compilation"
    );
    assert!(
        !String::from_utf8_lossy(&failed.stdout).contains("one-node-ok"),
        "Cargo must not run the stale previously built binary after generation fails"
    );

    let revised_source = valid_source
        .replace("Word", "Name")
        .replace("r#match", "r#loop");
    let revised_main = valid_main
        .replace("Word", "Name")
        .replace("r#match", "r#loop");
    assert_ne!(revised_source, valid_source);
    assert_ne!(revised_main, valid_main);
    fs::write(&grammar, revised_source).expect("replace the failure with a revised valid grammar");
    fs::write(&main, revised_main).expect("use the revised generated grammar API");
    let restored = run_fixture(&package);
    assert_success(&restored);
    assert_eq!(
        String::from_utf8(restored.stdout).unwrap().trim(),
        "one-node-ok"
    );
}

#[test]
fn a_node_body_rejects_the_same_semantic_key_from_a_different_physical_declaration() {
    let package = scaffold_fixture();
    fs::copy(
        package.path().join("src/grammar.rs"),
        package.path().join("src/analyzed.rs"),
    )
    .expect("copy the grammar to a physically distinct analyzed source");
    fs::write(
        package.path().join("build.rs"),
        r#"fn main() -> Result<(), recursa_codegen::GenerateError> {
    recursa_codegen::generate("src/analyzed.rs")?;
    Ok(())
}
"#,
    )
    .expect("select the replacement declaration for analysis");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "a replacement physical declaration must not load the compiled Node body"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("RCA9002"),
        "the declaration-local anchor mismatch must fail at the Node loader\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    copy(&fixture, package.path(), "build.rs");
    copy(&fixture, package.path(), "Cargo.lock");
    copy(&fixture, package.path(), "src/lib.rs");
    copy(&fixture, package.path(), "src/grammar.rs");
    copy(&fixture, package.path(), "src/main.rs");

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
        .env("CARGO_TARGET_DIR", target_dir(package));
    command.output().expect("run generated grammar fixture")
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
