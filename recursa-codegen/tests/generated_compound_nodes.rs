use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/m4/compound-nodes";

#[test]
fn a_downstream_crate_parses_authored_compound_nodes_and_views() {
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
        "compound-nodes-ok"
    );
}

#[test]
fn a_concrete_view_name_collision_is_a_deterministic_generation_error() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            "pub struct RecordView;\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        ),
    )
    .expect("insert an authored view-name collision");

    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "the generated view collision must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(
        stderr.contains(
            "generated concrete view `RecordView` collides with an authored item beside Node `Record`"
        ),
        "stderr:\n{stderr}",
    );
}

#[test]
fn an_imported_view_name_collision_is_a_deterministic_generation_error() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
            "mod authored { pub struct RecordView; }\nuse authored::RecordView;\n\n#[derive(recursa::Node)]\n#[tok(RECORD, this)]",
        ),
    )
    .expect("insert an imported view-name collision");

    assert_view_collision(run_fixture(&package), "RecordView");
}

#[test]
fn a_generated_token_binding_collision_is_a_deterministic_generation_error() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read compound grammar");
    fs::write(
        &grammar,
        source.replace(
            "pub text: CountText<'input>",
            "pub text: RecordView<'input>",
        ),
    )
    .expect("insert a generated token-binding collision");

    assert_view_collision(run_fixture(&package), "RecordView");
}

#[test]
fn source_edits_select_a_fresh_generation_in_the_same_build_directory() {
    let package = scaffold_fixture();
    let first = run_fixture(&package);
    assert!(
        first.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let before = generation_directories(&target_dir(&package));
    assert!(
        !before.is_empty(),
        "the first build must publish a Generation"
    );

    let grammar = package.path().join("src/grammar.rs");
    let mut source = fs::read_to_string(&grammar).expect("read compound grammar");
    source.push_str("\n// identity-changing source edit\n");
    fs::write(&grammar, source).expect("edit compound grammar");
    let second = run_fixture(&package);
    assert!(
        second.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let after = generation_directories(&target_dir(&package));
    assert!(
        after.len() > before.len(),
        "before={before:?}\nafter={after:?}"
    );
}

fn assert_view_collision(output: Output, name: &str) {
    assert!(
        !output.status.success(),
        "the generated view collision must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("RCA9105"), "stderr:\n{stderr}");
    assert!(stderr.contains(name), "stderr:\n{stderr}");
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary compound grammar package");
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
        .expect("read compound grammar fixture manifest")
        .replace("{{RECURSA}}", &toml_path(&workspace.join("recursa")))
        .replace(
            "{{RECURSA_CODEGEN}}",
            &toml_path(&workspace.join("recursa-codegen")),
        );
    fs::write(package.path().join("Cargo.toml"), manifest)
        .expect("write compound grammar fixture manifest");
    package
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy compound grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run compound grammar fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn generation_directories(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|name| name == "recursa-generations")
        {
            if let Ok(generations) = fs::read_dir(&path) {
                found.extend(
                    generations
                        .flatten()
                        .map(|generation| generation.path())
                        .filter(|generation| generation.is_dir()),
                );
            }
        } else {
            found.extend(generation_directories(&path));
        }
    }
    found
}
