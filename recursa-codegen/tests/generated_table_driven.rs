//! End-to-end proof of the sole Recursa parser's tracer grammar (#113), including
//! values, provenance, cursors, errors, and the generated automaton dump.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/table-driven";

#[test]
fn the_recursa_parser_preserves_tracer_behavior() {
    let package = scaffold_fixture();

    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("table-driven-ok\n"), "{stdout}");
    let table_stdout = parser_lines(&stdout);

    let root = read_selected_root(&package);
    assert!(
        root.contains("__recursa_lr"),
        "root carries the tables and dispatcher"
    );
    assert!(
        root.contains("__recursa_reduce"),
        "root carries the reduce dispatcher"
    );

    // Positive inputs parse to equal values and leave the cursor at the end.
    assert!(
        table_stdout.contains("ok \"let x = 5;\" => Let { assignment: Assignment { name: Name { text: __RecursaContentToken0 { text: ContentText(\"x\") } }, value: Number(Number { text: __RecursaContentToken1 { text: ContentText(\"5\") } }) } } cursor=5 eof=true"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"print(!!false);\" => Print { print: Print { value: Negated { mark: Bang, inner: Negated { mark: Bang, inner: False } } } } cursor=7 eof=true"),
        "{table_stdout}"
    );
    // A root parse observes the parser's strict prefix boundary when tokens follow it.
    assert!(table_stdout.contains("prefix first Let { assignment: Assignment { name: Name { text: __RecursaContentToken0 { text: ContentText(\"x\") } }, value: Number(Number { text: __RecursaContentToken1 { text: ContentText(\"5\") } }) } } cursor=5\n"), "{table_stdout}");
    assert!(table_stdout.contains("prefix second Print { print: Print { value: Name { name: Name { text: __RecursaContentToken0 { text: ContentText(\"x\") } } } } } cursor=10 eof=false\n"), "{table_stdout}");
    assert!(
        table_stdout.contains("prefix third code=RCA4100 found=Some(\"fixed:true\") cursor=10\n"),
        "{table_stdout}"
    );
    // Provenance (#118): capture retains fixed tokens, root elements, and Pretty rendering.
    assert!(table_stdout.contains("prov \"let x = 5;\" => captured 0..10 (0..10: (0..10: (0..10: (0..3) (4..5: (4..5: (4..5: (4..5)))) (6..7) (8..9: (8..9: (8..9: (8..9: (8..9: (8..9)))))) (9..10))))\n"), "{table_stdout}");
    assert!(
        table_stdout.contains("elements \"let x = 5;\" => [let_assignment:SemanticField@0..10]\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("pretty \"print(!!false);\" => \"print(! ! false);\"\n"),
        "{table_stdout}"
    );
    // A root rejects an unconsumed token with the token and continuations it reports (#118).
    assert!(table_stdout.contains("trailing \"true x\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 5, end: 6 } found=Some(\"content:\\\"[A-Za-z_][A-Za-z0-9_]*\\\":priority=None\") cursor=0 expected=[]\n"), "{table_stdout}");
    assert!(table_stdout.contains("trailing \"5 + 6 x\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 6, end: 7 } found=Some(\"content:\\\"[A-Za-z_][A-Za-z0-9_]*\\\":priority=None\") cursor=0 expected=[\"fixed:+\"]\n"), "{table_stdout}");
    assert!(table_stdout.contains("trailing \"count x\" => ok Name { text: __RecursaContentToken0 { text: ContentText(\"count\") } } cursor=1\n"), "{table_stdout}");
    // Every grammar type is an entry point.
    assert!(table_stdout.contains("entry Negated { mark: Bang, inner: Negated { mark: Bang, inner: Name { name: Name { text: __RecursaContentToken0 { text: ContentText(\"x\") } } } } } cursor=3\n"), "{table_stdout}");
    assert!(table_stdout.contains("entry Name { text: __RecursaContentToken0 { text: ContentText(\"count\") } } cursor=1\n"), "{table_stdout}");
    // Failures name the failing token, restore the cursor and drop every partial value.
    assert!(table_stdout.contains("err \"\" => code=RCA4101 kind=UnexpectedEof span=Span { start: 0, end: 0 } found=None cursor=0 leaked=0 expected=[\"fixed:let\", \"fixed:print\"]\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"let x = ;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 8, end: 9 } found=Some(\"fixed:;\") cursor=0 leaked=0 expected=[\"fixed:true\", \"fixed:false\", \"fixed:!\", \"content:\\\"[A-Za-z_][A-Za-z0-9_]*\\\":priority=None\", \"content:\\\"[0-9]+\\\":priority=None\"]\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"let x = !!!!!!!!(1);\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 16, end: 17 } found=Some(\"fixed:(\") cursor=0 leaked=0 expected=[\"fixed:true\", \"fixed:false\", \"fixed:!\", \"content:\\\"[A-Za-z_][A-Za-z0-9_]*\\\":priority=None\", \"content:\\\"[0-9]+\\\":priority=None\"]\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"let x = 1 print(2);\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 10, end: 15 } found=Some(\"fixed:print\") cursor=0 leaked=0 expected=[\"fixed:;\", \"fixed:+\"]\n"), "{table_stdout}");
}

#[test]
fn the_debug_bundle_of_a_table_driven_generation_carries_the_automaton_dump() {
    let package = scaffold_fixture();
    let destination = tempfile::tempdir().unwrap();
    assert_success(&run_fixture_with_debug(&package, destination.path()));
    let index_path = find_one_debug_index(destination.path());
    let index = fs::read_to_string(&index_path).unwrap();
    assert_eq!(
        index
            .lines()
            .filter(|line| line.starts_with("automaton\t"))
            .count(),
        1,
        "index:\n{index}"
    );
    let dump = fs::read_to_string(index_path.parent().unwrap().join("automaton.txt"))
        .expect("the bundle carries the automaton dump");
    assert!(
        dump.contains("$accept(grammar::Statement) -> grammar::Statement"),
        "{dump}"
    );
    assert!(dump.contains("State 0"), "{dump}");
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

fn cargo_run(package: &TempDir, debug_directory: Option<&Path>) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", target_dir(package));
    match debug_directory {
        Some(directory) => command.env("RECURSA_DEBUG_DIR", directory),
        None => command.env_remove("RECURSA_DEBUG_DIR"),
    };
    command.output().expect("run generated grammar fixture")
}

fn run_fixture(package: &TempDir) -> Output {
    cargo_run(package, None)
}

fn run_fixture_with_debug(package: &TempDir, debug_directory: &Path) -> Output {
    cargo_run(package, Some(debug_directory))
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

fn find_one_debug_index(directory: &Path) -> PathBuf {
    let mut found = Vec::new();
    find_named(directory, "index", &mut found);
    assert_eq!(found.len(), 1, "one generated-code debug index: {found:?}");
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
