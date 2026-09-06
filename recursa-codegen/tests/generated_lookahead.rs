//! End-to-end proof of the lookahead filter (#116) and its twin inside a rule (#121): the
//! `NOT BETWEEN` fixture, with gram.y's `NOT a_expr | NOT_LA a_expr` shape beside it, generates
//! with `parsers(rd = recursive_descent, lr = table_driven)` (#119) and its `lookahead`
//! declaration, and everything the `lr` parser's consumer observes equals what the `rd` parser
//! of the same generation produces. A generation with the recursive-descent parser alone prints what `rd`
//! printed, with and without the declaration. The declaration is inert under recursive descent
//! down to the generated node sources, which are byte-identical; only the root differs, by the
//! merged `TokenKind` variant.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/lookahead";

/// The configuration of the fixture grammar as written, replaced to generate the
/// recursive-descent parser alone.
const NAMED_PARSERS: &str = "parsers(rd = recursive_descent, lr = table_driven),";

/// The fixture's `lookahead` declaration, removed to obtain the undeclared twin.
const LOOKAHEAD: &str = "    lookahead { NOT_LA = NOT before { BETWEEN, IN, LIKE } }\n";

#[test]
fn the_lookahead_fixture_parses_like_recursive_descent_with_and_without_the_filter() {
    let package = scaffold_fixture();

    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("lookahead-ok\n"), "{stdout}");
    let (recursive_stdout, table_stdout) = split_by_parser(&stdout);
    assert_eq!(
        table_stdout, recursive_stdout,
        "the two parsers must agree on every line"
    );

    let table_files = read_selected_generation(&package);
    let table_root = &table_files["root.rs"];
    assert!(
        table_root.contains("__recursa_lr"),
        "root carries the tables and dispatcher"
    );
    assert!(
        table_root.contains("NOT_LA"),
        "the merged kind is a TokenKind variant"
    );

    // `NOT` before a trigger is a test; a bare `NOT` opens a negated condition.
    assert!(table_stdout.contains("ok \"a not between 1 and 2;\" => Rule { cond: NotBetween(NotBetween { subject: Subject { name: Name { text: __RecursaContentToken1 { text: ContentText(\"a\") } } }, low: Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } }, high: Number { text: __RecursaContentToken2 { text: ContentText(\"2\") } } }) } cursor=7 eof=true kinds=[NameText not between NumberText and NumberText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"a not b;\" => Rule { cond: AndNot(AndNot { operand: Operand { name: Name { text: __RecursaContentToken1 { text: ContentText(\"a\") } } }, inner: Name { name: Name { text: __RecursaContentToken1 { text: ContentText(\"b\") } } } }) } cursor=4 eof=true kinds=[NameText not NameText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"a not not b;\" => Rule { cond: AndNot(AndNot { operand: Operand { name: Name { text: __RecursaContentToken1 { text: ContentText(\"a\") } } }, inner: Not { mark: NotMark, inner: Name { name: Name { text: __RecursaContentToken1 { text: ContentText(\"b\") } } } } }) } cursor=5 eof=true kinds=[NameText not not NameText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"not a not in (3);\" => Rule { cond: Not { mark: NotMark, inner: NotIn(NotIn { subject: Subject { name: Name { text: __RecursaContentToken1 { text: ContentText(\"a\") } } }, member: Number { text: __RecursaContentToken2 { text: ContentText(\"3\") } } }) } } cursor=8 eof=true kinds=[not NameText not in ( NumberText ) ;]\n"), "{table_stdout}");
    // gram.y's `a_expr` shape (#121): prefix `NOT` is twinned with `NOT_LA`, so `not` before a
    // trigger read as a name (`like`, `between`) is a negation, `NOT a NOT LIKE b` is
    // `NOT (a NOT LIKE b)`, and a merged token keeps its lexed kind and span.
    assert!(table_stdout.contains("expr ok \"not a not like b;\" => ExprRule { expr: Not(NotLike(Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"a\"), kind: NameText } }), Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"b\"), kind: NameText } }))) } cursor=6 eof=true kinds=[not NameText not like NameText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("expr ok \"a not between 1 and 2;\" => ExprRule { expr: NotBetween(Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"a\"), kind: NameText } }), Number(Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } }), Number(Number { text: __RecursaContentToken2 { text: ContentText(\"2\") } })) } cursor=7 eof=true kinds=[NameText not between NumberText and NumberText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("expr ok \"not not a;\" => ExprRule { expr: Not(Not(Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"a\"), kind: NameText } }))) } cursor=4 eof=true kinds=[not not NameText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("expr ok \"not like like b;\" => ExprRule { expr: Not(Like(Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"like\"), kind: LIKE } }), Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"b\"), kind: NameText } }))) } cursor=5 eof=true kinds=[not like like NameText ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("expr ok \"not between;\" => ExprRule { expr: Not(Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"between\"), kind: BETWEEN } })) } cursor=3 eof=true kinds=[not between ;]\n"), "{table_stdout}");
    assert!(
        table_stdout.contains(
            "expr elements \"not between;\" => [expr:SemanticField@0..11 semi:FixedToken@11..12]\n"
        ),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("expr pretty \"not between;\" => \"not between;\"\n"),
        "{table_stdout}"
    );
    assert!(table_stdout.contains("expr entry Not(Ident(Ident { text: __RecursaContentToken0 { text: ContentText(\"between\"), kind: BETWEEN } })) cursor=2\n"), "{table_stdout}");
    assert!(table_stdout.contains("expr err \"not in;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 4, end: 6 } found=Some(\"fixed:in\") cursor=0 leaked=0\n"), "{table_stdout}");
    // The merged kind exists, displays as the token it merges and is never lexed.
    assert!(
        table_stdout.contains("merged kind NOT_LA displays as not like not\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("lexed [NOT, NameText, BETWEEN, NameText, IN, LIKE]\n"),
        "{table_stdout}"
    );
    // A root parse stops where recursive descent stops when tokens follow the root.
    assert!(
        table_stdout
            .contains("prefix third code=RCA4100 found=Some(\"fixed:between\") cursor=11\n"),
        "{table_stdout}"
    );
    // Every grammar type is an entry point.
    assert!(table_stdout.contains("entry NotIn { subject: Subject { name: Name { text: __RecursaContentToken1 { text: ContentText(\"a\") } } }, member: Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } } } cursor=6\n"), "{table_stdout}");
    assert!(table_stdout.contains("entry Subject { name: Name { text: __RecursaContentToken1 { text: ContentText(\"a\") } } } cursor=1\n"), "{table_stdout}");
    assert!(
        table_stdout.contains("entry NotMark cursor=1\n"),
        "{table_stdout}"
    );
    // Failures name the lexed token, restore the cursor and drop every partial value.
    assert!(table_stdout.contains("err \"a between not 1 and 2;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 10, end: 13 } found=Some(\"fixed:not\") cursor=0 leaked=0\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"a not between;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 13, end: 14 } found=Some(\"fixed:;\") cursor=0 leaked=0\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"a not between 1 and 2\" => code=RCA4101 kind=UnexpectedEof span=Span { start: 21, end: 21 } found=None cursor=0 leaked=0\n"), "{table_stdout}");

    // The recursive-descent parser alone prints what `rd` printed beside `lr`.
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    assert_eq!(source.matches(NAMED_PARSERS).count(), 1);
    assert_eq!(source.matches(LOOKAHEAD).count(), 1);
    let recursive_source = source.replace(NAMED_PARSERS, "parser_style = recursive_descent,");
    fs::write(&grammar, &recursive_source).expect("keep the recursive-descent parser alone");
    let driver = package.path().join("src/main.rs");
    let driver_source = fs::read_to_string(&driver).expect("read scaffolded driver");
    fs::write(
        &driver,
        remove_fence(
            &driver_source,
            "    // lr-parser-start",
            "    // lr-parser-end\n",
        ),
    )
    .expect("remove the lr pass, which names a parser that no longer exists");
    let recursive = run_fixture(&package);
    assert_success(&recursive);
    let (alone_stdout, absent) = split_by_parser(&String::from_utf8(recursive.stdout).unwrap());
    assert!(absent.is_empty());
    assert_eq!(
        alone_stdout, recursive_stdout,
        "a second parser changes nothing the default parser prints"
    );
    let recursive_files = read_selected_generation(&package);
    let recursive_root = &recursive_files["root.rs"];
    assert!(
        recursive_root.contains("RecursiveDescent") && !recursive_root.contains("__recursa_lr")
    );
    assert!(
        recursive_root.contains("NOT_LA"),
        "the merged kind exists under recursive descent too"
    );

    // The declaration is inert under recursive descent: removing it changes no observable
    // line and no generated node source; only the root loses the merged kind.
    fs::write(&grammar, recursive_source.replace(LOOKAHEAD, ""))
        .expect("remove the lookahead declaration");
    let driver_source = fs::read_to_string(&driver).expect("read scaffolded driver");
    fs::write(
        &driver,
        remove_fence(
            &driver_source,
            "    // merged-kind-evidence-start",
            "    // merged-kind-evidence-end\n",
        ),
    )
    .expect("remove the merged-kind evidence, which names a kind that no longer exists");
    let undeclared = run_fixture(&package);
    assert_success(&undeclared);
    let (undeclared_stdout, _) = split_by_parser(&String::from_utf8(undeclared.stdout).unwrap());
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

/// Removes one fenced block of the driver, from the line holding `start` through `end`.
fn remove_fence(source: &str, start: &str, end: &str) -> String {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("the driver fences {start}"));
    let end = source
        .find(end)
        .map(|position| position + end.len())
        .unwrap_or_else(|| panic!("the driver fences {end}"));
    format!("{}{}", &source[..start], &source[end..])
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
            files.insert(
                name,
                fs::read_to_string(&path).expect("read a generated file"),
            );
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
