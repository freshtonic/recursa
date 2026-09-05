//! End-to-end proof of the lookahead filter (#116): the `NOT BETWEEN` fixture generates and
//! runs under `parser_style = table_driven` with its `lookahead` declaration, and everything its
//! consumer observes equals what the recursive-descent twin produces, with and without the
//! declaration. The declaration is inert under recursive descent down to the generated node
//! sources, which are byte-identical; only the root differs, by the merged `TokenKind` variant.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/lookahead";

/// The line of the fixture grammar the test flips to obtain the recursive-descent twin.
const TABLE_DRIVEN: &str = "parser_style = table_driven,";

/// The fixture's `lookahead` declaration, removed to obtain the undeclared twin.
const LOOKAHEAD: &str = "    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }\n";

#[test]
fn the_lookahead_fixture_parses_like_its_recursive_descent_twin_with_and_without_the_filter() {
    let package = scaffold_fixture();

    let table_driven = run_fixture(&package);
    assert_success(&table_driven);
    let table_stdout = String::from_utf8(table_driven.stdout).unwrap();
    assert!(table_stdout.ends_with("lookahead-ok\n"), "{table_stdout}");

    let table_files = read_selected_generation(&package);
    let table_root = &table_files["root.rs"];
    assert!(table_root.contains("__recursa_lr"), "root carries the tables and dispatcher");
    assert!(table_root.contains("NOT_LA"), "the merged kind is a TokenKind variant");

    // `NOT` before a trigger is a test; a bare `NOT` opens a negated condition.
    assert!(table_stdout.contains("ok \"a not between 1 and 2;\" => Rule { cond: NotBetween(NotBetween { subject: Subject { name: Name { text: __RecursaContentToken0 { text: ContentText(\"a\") } } }, low: Number { text: __RecursaContentToken1 { text: ContentText(\"1\") } }, high: Number { text: __RecursaContentToken1 { text: ContentText(\"2\") } } }) } cursor=7 eof=true kinds=[NameText not between NumberText and NumberText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"a not b;\" => Rule { cond: AndNot(AndNot { operand: Operand { name: Name { text: __RecursaContentToken0 { text: ContentText(\"a\") } } }, inner: Name { name: Name { text: __RecursaContentToken0 { text: ContentText(\"b\") } } } }) } cursor=4 eof=true kinds=[NameText not NameText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"a not not b;\" => Rule { cond: AndNot(AndNot { operand: Operand { name: Name { text: __RecursaContentToken0 { text: ContentText(\"a\") } } }, inner: Not { mark: NotMark, inner: Name { name: Name { text: __RecursaContentToken0 { text: ContentText(\"b\") } } } } }) } cursor=5 eof=true kinds=[NameText not not NameText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"not a not in (3);\" => Rule { cond: Not { mark: NotMark, inner: NotIn(NotIn { subject: Subject { name: Name { text: __RecursaContentToken0 { text: ContentText(\"a\") } } }, member: Number { text: __RecursaContentToken1 { text: ContentText(\"3\") } } }) } } cursor=8 eof=true kinds=[not NameText not in ( NumberText ) ;]\n"), "{table_stdout}");
    // The merged kind exists, displays as the token it merges and is never lexed.
    assert!(table_stdout.contains("merged kind NOT_LA displays as not like not\n"), "{table_stdout}");
    assert!(table_stdout.contains("lexed [NOT, NameText, BETWEEN, NameText, IN, LIKE]\n"), "{table_stdout}");
    // A root parse stops where recursive descent stops when tokens follow the root.
    assert!(table_stdout.contains("prefix third code=RCA4100 found=Some(\"fixed:between\") cursor=11\n"), "{table_stdout}");
    // Every grammar type is an entry point.
    assert!(table_stdout.contains("entry NotIn { subject: Subject { name: Name { text: __RecursaContentToken0 { text: ContentText(\"a\") } } }, member: Number { text: __RecursaContentToken1 { text: ContentText(\"1\") } } } cursor=6\n"), "{table_stdout}");
    assert!(table_stdout.contains("entry Subject { name: Name { text: __RecursaContentToken0 { text: ContentText(\"a\") } } } cursor=1\n"), "{table_stdout}");
    assert!(table_stdout.contains("entry NotMark cursor=1\n"), "{table_stdout}");
    // Failures name the lexed token, restore the cursor and drop every partial value.
    assert!(table_stdout.contains("err \"a between not 1 and 2;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 10, end: 13 } found=Some(\"fixed:not\") cursor=0 leaked=0\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"a not between;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 13, end: 14 } found=Some(\"fixed:;\") cursor=0 leaked=0\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"a not between 1 and 2\" => code=RCA4101 kind=UnexpectedEof span=Span { start: 21, end: 21 } found=None cursor=0 leaked=0\n"), "{table_stdout}");

    // The twin: the same grammar under recursive descent prints the same lines.
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    assert_eq!(source.matches(TABLE_DRIVEN).count(), 1);
    assert_eq!(source.matches(LOOKAHEAD).count(), 1);
    let recursive_source = source.replace(TABLE_DRIVEN, "parser_style = recursive_descent,");
    fs::write(&grammar, &recursive_source).expect("flip the fixture to recursive descent");
    let recursive = run_fixture(&package);
    assert_success(&recursive);
    let recursive_stdout = String::from_utf8(recursive.stdout).unwrap();
    assert_eq!(table_stdout, recursive_stdout, "the two parser styles must agree on every line");
    let recursive_files = read_selected_generation(&package);
    let recursive_root = &recursive_files["root.rs"];
    assert!(recursive_root.contains("RecursiveDescent") && !recursive_root.contains("__recursa_lr"));
    assert!(recursive_root.contains("NOT_LA"), "the merged kind exists under recursive descent too");

    // The declaration is inert under recursive descent: removing it changes no observable
    // line and no generated node source; only the root loses the merged kind.
    fs::write(&grammar, recursive_source.replace(LOOKAHEAD, ""))
        .expect("remove the lookahead declaration");
    let driver = package.path().join("src/main.rs");
    let driver_source = fs::read_to_string(&driver).expect("read scaffolded driver");
    let start = driver_source
        .find("    // merged-kind-evidence-start")
        .expect("the driver fences its merged-kind evidence");
    let end = driver_source
        .find("    // merged-kind-evidence-end\n")
        .map(|end| end + "    // merged-kind-evidence-end\n".len())
        .expect("the driver fences its merged-kind evidence");
    fs::write(&driver, format!("{}{}", &driver_source[..start], &driver_source[end..]))
        .expect("remove the merged-kind evidence, which names a kind that no longer exists");
    let undeclared = run_fixture(&package);
    assert_success(&undeclared);
    let undeclared_stdout = String::from_utf8(undeclared.stdout).unwrap();
    let merged_lines = |stdout: &str| {
        stdout
            .lines()
            .filter(|line| !line.starts_with("merged kind ") && !line.starts_with("lexed "))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        merged_lines(&recursive_stdout),
        merged_lines(&undeclared_stdout),
        "the declaration changes nothing recursive descent observes"
    );
    let undeclared_files = read_selected_generation(&package);
    // Every generated source embeds its generation identity in the handshake; the merged kind
    // changes the identity, so the node sources are compared with the identity normalized.
    let nodes = |files: &BTreeMap<String, String>| {
        let identity = files["identity"].as_str();
        files
            .iter()
            .filter(|(name, _)| name.starts_with("node-") && name.ends_with(".rs"))
            .map(|(name, text)| (name.clone(), text.replace(identity, "IDENTITY")))
            .collect::<BTreeMap<_, _>>()
    };
    assert!(!nodes(&recursive_files).is_empty());
    assert_eq!(
        nodes(&recursive_files),
        nodes(&undeclared_files),
        "recursive-descent node sources are byte-identical with and without the declaration"
    );
    let undeclared_root = &undeclared_files["root.rs"];
    assert!(!undeclared_root.contains("NOT_LA"));
    assert_ne!(recursive_root, undeclared_root);
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

/// Reads every file of the selected generation, keyed by file name.
fn read_selected_generation(package: &TempDir) -> BTreeMap<String, String> {
    let selected = find_selected_generation(&target_dir(package));
    let selector = fs::read_to_string(&selected).expect("read selected Generation");
    let identity = selector
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// recursa-generation:"))
        .expect("selector names its generation");
    let directory = selected
        .parent()
        .unwrap()
        .join("recursa-generations")
        .join(identity);
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(&directory).expect("read the selected generation directory") {
        let path = entry.unwrap().path();
        if path.is_file() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            files.insert(name, fs::read_to_string(&path).expect("read a generated file"));
        }
    }
    assert!(files.contains_key("root.rs"), "{directory:?} holds root.rs");
    files.insert("identity".to_owned(), identity.to_owned());
    files
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
    command.output().expect("run generated grammar fixture")
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
