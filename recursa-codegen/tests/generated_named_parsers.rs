//! Named parsers (#119): `parsers(rd = recursive_descent, lr = table_driven)` emits both
//! parsers in one generation. The default parser owns the unqualified surface unchanged: the
//! fixture's `main.rs`, which uses only that surface, prints the same bytes whether or not the
//! `lr` parser is declared. The `lr` parser is reached through `parsers::lr::input`, and the
//! table-driven automaton is what runs there: on `print(1 + 2` its lookahead correction names
//! `+` beside `)`, which recursive descent's predictive dispatch never does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/named-parsers";

/// The configuration line of the fixture grammar as written.
const NAMED_PARSERS: &str = "parsers(rd = recursive_descent, lr = table_driven),";

/// The configuration the test substitutes to obtain the generation without the `lr` parser.
const DEFAULT_ONLY: &str = "parser_style = recursive_descent,";

#[test]
fn the_default_surface_prints_the_same_bytes_with_and_without_a_second_parser() {
    let package = scaffold_fixture();
    let grammar = package.path().join("src/grammar.rs");
    let named = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    assert_eq!(named.matches(NAMED_PARSERS).count(), 1);

    fs::write(&grammar, named.replace(NAMED_PARSERS, DEFAULT_ONLY)).expect("drop the lr parser");
    let without = run_bin(&package, "recursa-generated-named-parsers");
    assert_success(&without);
    let root = read_selected_root(&package);
    assert!(!root.contains("pub mod parsers"), "one parser declares no parsers module");

    fs::write(&grammar, &named).expect("restore the lr parser");
    let with = run_bin(&package, "recursa-generated-named-parsers");
    assert_success(&with);
    assert_eq!(
        String::from_utf8_lossy(&with.stdout),
        String::from_utf8_lossy(&without.stdout),
        "the default surface is unchanged by a second parser"
    );
    let stdout = String::from_utf8(with.stdout).unwrap();
    assert!(stdout.ends_with("default-surface-ok\n"), "{stdout}");
    // Recursive descent selects the complete alternative at the end of input and the enclosing
    // `Print` names only `)`.
    assert!(
        stdout.contains("err \"print(1 + 2\" => code=RCA4101 kind=UnexpectedEof span=Span { start: 11, end: 11 } found=None cursor=0 expected=[\"fixed:)\"]\n"),
        "{stdout}"
    );

    // One generation carries both parsers: the recursive-descent marker on the root input, the
    // table-driven marker, tables and dispatcher under `parsers::lr`.
    let root = read_selected_root(&package);
    assert!(root.contains("pub mod parsers"), "root declares the parsers module");
    assert!(root.contains("pub mod lr"), "root declares the lr parser module");
    assert!(root.contains("RecursiveDescent"), "the default parser is recursive descent");
    assert!(root.contains("TableDriven"), "the lr parser is table-driven");
    assert!(root.contains("__recursa_lr"), "root carries the tables and dispatcher");
}

#[test]
fn the_lr_input_selects_the_table_driven_parser() {
    let package = scaffold_fixture();
    let default = run_bin(&package, "recursa-generated-named-parsers");
    assert_success(&default);
    let default_stdout = String::from_utf8(default.stdout).unwrap();
    let lr = run_bin(&package, "lr");
    assert_success(&lr);
    let lr_stdout = String::from_utf8(lr.stdout).unwrap();
    assert!(lr_stdout.ends_with("lr-ok\n"), "{lr_stdout}");

    // Lookahead correction names every terminal that could have continued: the automaton ran.
    assert!(
        lr_stdout.contains("err \"print(1 + 2\" => code=RCA4101 kind=UnexpectedEof span=Span { start: 11, end: 11 } found=None cursor=0 expected=[\"fixed:)\", \"fixed:+\"]\n"),
        "{lr_stdout}"
    );
    // Everything else the two parsers print is equal: values, cursors, provenance, Pretty,
    // failing tokens and restored cursors.
    let default_lines: Vec<&str> = default_stdout
        .lines()
        .filter(|line| !line.starts_with("err \"print(1 + 2\"") && *line != "default-surface-ok")
        .collect();
    let lr_lines: Vec<&str> = lr_stdout
        .lines()
        .filter(|line| !line.starts_with("err \"print(1 + 2\"") && *line != "lr-ok")
        .collect();
    assert_eq!(lr_lines, default_lines);
    assert!(lr_lines.iter().any(|line| line.starts_with("prov \"let z = 1 + !2 + x;\" => captured")));
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
    copy(&fixture, package.path(), "src/grammar.rs");
    copy(&fixture, package.path(), "src/main.rs");
    copy(&fixture, package.path(), "src/bin/lr.rs");

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

fn run_bin(package: &TempDir, bin: &str) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .arg("--bin")
        .arg(bin)
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
