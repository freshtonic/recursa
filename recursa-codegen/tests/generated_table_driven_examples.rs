//! Every non-Pratt example grammar in the repository parses all of its inputs identically under
//! both parser styles (#114). The JSON example is the one Pratt-free example; its grammar is
//! copied from `examples/json` at scaffold time, so the example stays the single source of
//! truth, and generated once with `parser_style = table_driven` and once as written.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/table-driven-json";

/// The example grammar's configuration line after which the parser style is inserted.
const LOOKAHEAD: &str = "max_lookahead = 1,";

/// The line the scaffold inserts, and the test flips to obtain the recursive-descent twin.
const TABLE_DRIVEN: &str = "parser_style = table_driven,";

#[test]
fn the_json_example_parses_every_input_identically_under_both_styles() {
    let package = scaffold_fixture();

    let table_driven = run_fixture(&package);
    assert_success(&table_driven);
    let table_stdout = String::from_utf8(table_driven.stdout).unwrap();
    assert!(table_stdout.ends_with("table-driven-json-ok\n"), "{table_stdout}");
    let root = read_selected_root(&package);
    assert!(root.contains("__recursa_lr"), "root carries the tables and dispatcher");
    assert!(!root.contains("RecursiveDescent"), "a table-driven generation emits no recursive-descent parser");
    // The example's own expectations, seen through the table-driven parser.
    assert!(table_stdout.contains("ok \"{}\" => object{} cursor=2 eof=true\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"[]\" => array(none) cursor=2 eof=true\n"), "{table_stdout}");
    assert!(
        table_stdout.contains("ok \"[\\\"text\\\",-12.5e+2]\" => array[string(\"text\") number(-12.5e+2)] cursor=5 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"{\\\"answer\\\":[true,null,42]}\" => object{\"answer\": array[bool(true) null number(42)]} cursor=11 eof=true\n"),
        "{table_stdout}"
    );
    assert!(table_stdout.contains("prefix first object{} cursor=2 eof=false\n"), "{table_stdout}");
    assert!(table_stdout.contains("prefix fourth array[bool(true)] cursor=14 eof=true\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"@\" => code=RCA4000 kind=LexicalError"), "{table_stdout}");

    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    assert_eq!(source.matches(TABLE_DRIVEN).count(), 1);
    fs::write(&grammar, source.replace(TABLE_DRIVEN, "parser_style = recursive_descent,"))
        .expect("flip the fixture to recursive descent");
    let recursive = run_fixture(&package);
    assert_success(&recursive);
    let recursive_stdout = String::from_utf8(recursive.stdout).unwrap();
    assert_eq!(table_stdout, recursive_stdout, "the two parser styles must agree on every line");
    let root = read_selected_root(&package);
    assert!(root.contains("RecursiveDescent") && !root.contains("__recursa_lr"));
}

fn scaffold_fixture() -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(FIXTURE);

    copy(&fixture, package.path(), "build.rs");
    copy_as(&fixture, package.path(), "Cargo.core.lock", "Cargo.lock");
    copy(&fixture, package.path(), "src/lib.rs");
    copy(&fixture, package.path(), "src/main.rs");

    // The example grammar, with the diagrams feature dropped (the fixture enables no features)
    // and the table-driven style selected.
    let example = fs::read_to_string(workspace.join("examples/json/src/grammar.rs"))
        .expect("read the JSON example grammar");
    assert_eq!(example.matches("    diagrams,\n").count(), 1, "the example declares diagrams once");
    assert_eq!(example.matches(LOOKAHEAD).count(), 1, "the example configures lookahead once");
    let grammar = example
        .replace("    diagrams,\n", "")
        .replace(LOOKAHEAD, &format!("{LOOKAHEAD}\n    {TABLE_DRIVEN}"));
    fs::write(package.path().join("src/grammar.rs"), grammar).expect("write the scaffolded grammar");

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

fn read_selected_root(package: &TempDir) -> String {
    let selected = find_selected_generation(&target_dir(package));
    let selector = fs::read_to_string(&selected).expect("read selected Generation");
    let identity = selector
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// recursa-generation:"))
        .expect("selector names its generation");
    let root = selected
        .parent()
        .unwrap()
        .join("recursa-generations")
        .join(identity)
        .join("root.rs");
    fs::read_to_string(&root).expect("read the selected root")
}

fn copy(fixture: &Path, package: &Path, relative: &str) {
    let destination = package.join(relative);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(relative), destination).expect("copy generated grammar fixture file");
}

fn copy_as(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy generated grammar fixture file");
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
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run generated grammar fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}

fn find_selected_generation(directory: &Path) -> PathBuf {
    let mut found = Vec::new();
    find_named(directory, "recursa-selected.rs", &mut found);
    assert_eq!(found.len(), 1, "one selected Generation: {found:?}");
    found.pop().unwrap()
}

fn find_named(directory: &Path, name: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            find_named(&path, name, found);
        } else if path.file_name().is_some_and(|file| file == name) {
            found.push(path);
        }
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
