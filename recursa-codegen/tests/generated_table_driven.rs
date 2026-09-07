//! End-to-end proof of the table-driven tracer (#113): the fixture grammar generates once with
//! `parsers(rd = recursive_descent, lr = table_driven)` (#119), the fixture drives every input
//! through both parsers in one binary, and everything the `lr` consumer observes equals what
//! the `rd` parser produces. Error parity (#117) is checked line by line: the failing token,
//! span, kind and restored cursor must be equal, and the table-driven expected set must contain
//! recursive descent's, with the one principled difference pinned in
//! [`EXPECTED_SET_DIFFERENCES`].

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/table-driven";

#[test]
fn the_table_driven_parser_parses_like_the_recursive_descent_parser_of_the_same_generation() {
    let package = scaffold_fixture();

    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("table-driven-ok\n"), "{stdout}");
    let (recursive_stdout, table_stdout) = split_by_parser(&stdout);

    // One generation carries both parsers: the recursive-descent default on the root input,
    // the table-driven tables, dispatcher and marker under `parsers::lr`.
    let root = read_selected_root(&package);
    assert!(
        root.contains("RecursiveDescent"),
        "the default parser is recursive descent"
    );
    assert!(
        root.contains("pub mod parsers"),
        "root declares the extra parsers"
    );
    assert!(root.contains("pub mod lr"), "root declares the lr parser");
    assert!(
        root.contains("__recursa_lr"),
        "root carries the tables and dispatcher"
    );
    assert!(
        root.contains("TableDriven"),
        "the lr input names the table-driven marker"
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
    // A root parse stops where recursive descent stops when tokens follow the root.
    assert!(table_stdout.contains("prefix first Let { assignment: Assignment { name: Name { text: __RecursaContentToken0 { text: ContentText(\"x\") } }, value: Number(Number { text: __RecursaContentToken1 { text: ContentText(\"5\") } }) } } cursor=5\n"), "{table_stdout}");
    assert!(table_stdout.contains("prefix second Print { print: Print { value: Name { name: Name { text: __RecursaContentToken0 { text: ContentText(\"x\") } } } } } cursor=10 eof=false\n"), "{table_stdout}");
    assert!(
        table_stdout.contains("prefix third code=RCA4100 found=Some(\"fixed:true\") cursor=10\n"),
        "{table_stdout}"
    );
    // Provenance (#118): with capture requested the occurrence tree equals recursive descent's,
    // node by node, including every fixed token, and so do the root's elements and the Pretty
    // rendering; the twin comparison below holds every `prov`, `elements` and `pretty` line to
    // byte equality.
    assert!(table_stdout.contains("prov \"let x = 5;\" => captured 0..10 (0..10: (0..10: (0..10: (0..3) (4..5: (4..5: (4..5: (4..5)))) (6..7) (8..9: (8..9: (8..9: (8..9: (8..9: (8..9)))))) (9..10))))\n"), "{table_stdout}");
    assert!(
        table_stdout.contains("elements \"let x = 5;\" => [let_assignment:SemanticField@0..10]\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("pretty \"print(!!false);\" => \"print(! ! false);\"\n"),
        "{table_stdout}"
    );
    // A root decided by its FOLLOW rejects the token after it as recursive descent does, with
    // the same failing token and an expected set of what could have continued (#118).
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

    // The recursive-descent parser of the same generation prints the same lines.
    assert_twins_agree(&table_stdout, &recursive_stdout);
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

/// Splits the fixture's output into the lines of the two parsers, each without its tag and
/// terminated by a newline so that the assertions can match whole lines.
fn split_by_parser(stdout: &str) -> (String, String) {
    let mut rd = String::new();
    let mut lr = String::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("rd ") {
            rd.push_str(rest);
            rd.push('\n');
        } else if let Some(rest) = line.strip_prefix("lr ") {
            lr.push_str(rest);
            lr.push('\n');
        }
    }
    (rd, lr)
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The error lines whose expected sets differ between the styles, as `(line head, table-driven
/// set, recursive-descent set)`.
///
/// At the end of input recursive descent's predictive dispatch selects the complete alternative
/// (`Value::Number`) rather than failing, so the error is raised by the enclosing `Print` and
/// names only `)`. Lookahead correction explores from the state before that choice was forced
/// and names every terminal that could have continued, `+` included. Before a token, as in
/// `let x = 1 print(2);`, the dispatch fails at the token itself and both styles agree.
const EXPECTED_SET_DIFFERENCES: &[(&str, &str, &str)] = &[(
    "err \"print(1 + 2\"",
    "[\"fixed:)\", \"fixed:+\"]",
    "[\"fixed:)\"]",
)];

/// Asserts that the two styles' outputs agree line by line, with error expected sets compared
/// as sets: the table-driven set must contain the recursive-descent set, and the lines where it
/// is strictly larger must be exactly [`EXPECTED_SET_DIFFERENCES`].
fn assert_twins_agree(table: &str, recursive: &str) {
    let table_lines: Vec<&str> = table.lines().collect();
    let recursive_lines: Vec<&str> = recursive.lines().collect();
    assert_eq!(
        table_lines.len(),
        recursive_lines.len(),
        "the two parser styles must print the same lines:\n{table}\n---\n{recursive}"
    );
    let mut differences = Vec::new();
    for (table_line, recursive_line) in table_lines.iter().zip(&recursive_lines) {
        if table_line == recursive_line {
            continue;
        }
        let (table_head, table_expected) = split_expected(table_line);
        let (recursive_head, recursive_expected) = split_expected(recursive_line);
        assert_eq!(
            table_head, recursive_head,
            "the failing token, span, kind and cursor must agree:\n{table_line}\n{recursive_line}"
        );
        let table_names = expected_names(table_expected);
        let recursive_names = expected_names(recursive_expected);
        assert!(
            recursive_names
                .iter()
                .all(|name| table_names.contains(name)),
            "the table-driven expected set must contain recursive descent's:\n{table_line}\n{recursive_line}"
        );
        let head = table_line
            .split_once(" =>")
            .map_or(*table_line, |(head, _)| head);
        differences.push((
            head.to_owned(),
            table_expected.to_owned(),
            recursive_expected.to_owned(),
        ));
    }
    let differences: Vec<(&str, &str, &str)> = differences
        .iter()
        .map(|(head, table, recursive)| (head.as_str(), table.as_str(), recursive.as_str()))
        .collect();
    assert_eq!(
        differences, EXPECTED_SET_DIFFERENCES,
        "the expected-set differences are pinned"
    );
}

/// Splits an error line into everything before its expected set and the set's Debug text.
fn split_expected(line: &str) -> (&str, &str) {
    line.split_once(" expected=")
        .unwrap_or_else(|| panic!("only error lines with an expected set may differ: {line}"))
}

/// Parses the Debug text of a list of expected names.
fn expected_names(list: &str) -> Vec<&str> {
    let inner = list
        .strip_prefix('[')
        .and_then(|list| list.strip_suffix(']'))
        .unwrap_or_else(|| panic!("an expected set is a Debug list: {list}"));
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split("\", \"")
        .map(|name| name.trim_start_matches('"').trim_end_matches('"'))
        .collect()
}
