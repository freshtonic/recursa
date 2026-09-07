//! The JSON example is copied from `examples/json` at scaffold time, so the example remains the
//! single source of truth while its public behavior runs through the sole Recursa parser.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/table-driven-json";

#[test]
fn the_json_example_parses_every_input_through_the_recursa_parser() {
    let package = scaffold_fixture();

    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("table-driven-json-ok\n"), "{stdout}");
    let table_stdout = parser_lines(&stdout);
    let root = read_selected_root(&package);
    assert!(
        root.contains("__recursa_lr"),
        "root carries the tables and dispatcher"
    );
    // The example's own expectations, seen through the table-driven parser.
    assert!(
        table_stdout.contains("ok \"{}\" => object{} cursor=2 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"[]\" => array(none) cursor=2 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"[\\\"text\\\",-12.5e+2]\" => array[string(\"text\") number(-12.5e+2)] cursor=5 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"{\\\"answer\\\":[true,null,42]}\" => object{\"answer\": array[bool(true) null number(42)]} cursor=11 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("prefix first object{} cursor=2 eof=false\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("prefix fourth array[bool(true)] cursor=14 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"@\" => code=RCA4000 kind=LexicalError"),
        "{table_stdout}"
    );
    // A root whose selected alternative is one token long rejects the token after it (#118);
    // a closing token above accepted the prefix.
    assert!(table_stdout.contains("trailing \"null,\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 4, end: 5 } found=Some(\"fixed:,\") cursor=0\n"), "{table_stdout}");
    assert!(table_stdout.contains("trailing \"null true\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 5, end: 9 } found=Some(\"fixed:true\") cursor=0\n"), "{table_stdout}");
    assert!(table_stdout.contains("trailing \"42 ]\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 3, end: 4 } found=Some(\"fixed:]\") cursor=0\n"), "{table_stdout}");
    assert!(
        table_stdout.contains("trailing \"true\" => ok bool(true) cursor=1 eof=true\n"),
        "{table_stdout}"
    );
    // Provenance (#118): the occurrence tree, the root's elements and the Pretty rendering of
    // every positive input are printed for the sole parser.
    assert!(
        table_stdout.contains("prov \"{}\" => captured 0..2 ("),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("elements \"[]\" => ["),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("pretty \"{\\\"answer\\\":[true,null,42]}\" => \""),
        "{table_stdout}"
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
    copy_as(&fixture, package.path(), "Cargo.core.lock", "Cargo.lock");
    copy(&fixture, package.path(), "src/lib.rs");
    copy(&fixture, package.path(), "src/main.rs");

    // The example grammar, with the diagrams feature dropped (the fixture enables no features).
    let example = fs::read_to_string(workspace.join("examples/json/src/grammar.rs"))
        .expect("read the JSON example grammar");
    assert_eq!(
        example.matches("    diagrams,\n").count(),
        1,
        "the example declares diagrams once"
    );
    assert_eq!(
        example.matches("    derives(Visit),\n").count(),
        1,
        "the example declares derives once"
    );
    let grammar = example
        .replace("    diagrams,\n", "")
        .replace("    derives(Visit),\n", "    derives(Visit, Pretty),\n");
    fs::write(package.path().join("src/grammar.rs"), grammar)
        .expect("write the scaffolded grammar");

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

/// Selects the sole parser's tagged fixture lines and terminates each with a newline so the
/// assertions can match whole lines.
fn parser_lines(stdout: &str) -> String {
    let mut parser = String::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("lr ") {
            parser.push_str(rest);
            parser.push('\n');
        }
    }
    parser
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
