//! End-to-end proof of the `precedence` declaration (#120): the fixture generates with
//! `parsers(rd = recursive_descent, lr = table_driven)` (#119), its `precedence` block and its
//! `#[parse(prec = ...)]` overrides, and everything the `lr` parser's consumer observes equals
//! what the `rd` parser of the same generation produces, for every gram.y family of the issue.
//! Six inputs part the two styles, each pinned for both parsers: the declaration decides the
//! keyword where recursive descent reads on to the token that decides it, so one style
//! accepts what the other rejects, or the two build different trees. The table-driven outcome
//! is PostgreSQL's in every one of them, because it is what the gram.y declaration says. A generation with the recursive-descent
//! parser alone prints what `rd` printed, with and without the declaration, and the
//! declaration is inert under recursive descent down to every generated source: the node
//! sources and the root are byte-identical with and without it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/precedence";

/// The configuration of the fixture grammar as written, replaced to generate the
/// recursive-descent parser alone.
const NAMED_PARSERS: &str = "parsers(rd = recursive_descent, lr = table_driven),";

/// The fixture's `precedence` declaration, removed to obtain the undeclared twin.
const PRECEDENCE: &str = "    precedence {
        nonassoc(bp = 1) { LOCAL, SESSION },
        nonassoc(bp = 2) { CHARACTERISTICS },
        nonassoc(bp = 4) { IS },
        nonassoc(bp = 5) { BETWEEN, LIKE },
        nonassoc(bp = 6) { ESCAPE },
        nonassoc(bp = 8) { UNBOUNDED, NESTED },
        nonassoc(bp = 9) { NameText, PRECEDING, VALUE, PATH },
        left(bp = 10) { PLUS, MINUS },
        right(bp = 12) { UMINUS },
        left(bp = 20) { DOT },
    }
";

/// The fixture's overrides, which need the declaration and are removed with it.
const OVERRIDES: [&str; 2] = [
    "    #[parse(prec = UMINUS)]\n",
    "    #[parse(prec = UNBOUNDED)]\n",
];

#[test]
fn the_precedence_fixture_parses_like_recursive_descent_with_and_without_the_declaration() {
    let package = scaffold_fixture();

    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("precedence-ok\n"), "{stdout}");
    let (recursive_stdout, table_stdout) = split_by_parser(&stdout);
    let shared = |lines: &str| {
        lines
            .lines()
            .filter(|line| !line.starts_with("parts "))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shared(&table_stdout),
        shared(&recursive_stdout),
        "the two parsers must agree on every shared line"
    );
    assert!(
        shared(&table_stdout)
            .iter()
            .filter(|line| line.starts_with("ok "))
            .count()
            >= 29
    );

    let table_files = read_selected_generation(&package);
    let table_root = &table_files["root.rs"];
    assert!(
        table_root.contains("__recursa_lr"),
        "root carries the tables and dispatcher"
    );

    // One line per family, as the table-driven parser printed it; the shared lines above
    // prove recursive descent printed each of them too.
    // `%nonassoc ESCAPE` above `LIKE`: the escape clause is shifted.
    assert!(table_stdout.contains("ok \"select a like b escape 'c';\" => Stmt { body: Select(SelectStmt { target: Like(Column(Name { text: __RecursaContentToken1 { text: ContentText(\"a\"), kind: NameText } }), Column(Name { text: __RecursaContentToken1 { text: ContentText(\"b\"), kind: NameText } }), Some(EscapeClause { escape: StringLit { text: __RecursaContentToken3 { text: ContentText(\"'c'\") } } })), label: None }) } cursor=7 eof=true\n"), "{table_stdout}");
    // `ESCAPE` before a token no escape clause can start stays a bare label.
    assert!(table_stdout.contains("ok \"select a escape;\" => Stmt { body: Select(SelectStmt { target: Column(Name { text: __RecursaContentToken1 { text: ContentText(\"a\"), kind: NameText } }), label: Some(BareLabel { text: __RecursaContentToken0 { text: ContentText(\"escape\"), kind: ESCAPE } }) }) } cursor=4 eof=true\n"), "{table_stdout}");
    // The bare `JSON` alternative, `%prec UNBOUNDED`.
    assert!(table_stdout.contains("ok \"select a is json;\" => Stmt { body: Select(SelectStmt { target: IsJson(Column(Name { text: __RecursaContentToken1 { text: ContentText(\"a\"), kind: NameText } }), Json), label: None }) } cursor=5 eof=true\n"), "{table_stdout}");
    // `%nonassoc NESTED` below `PATH`: `PATH` opens the nested column.
    assert!(table_stdout.contains("ok \"columns ( nested path 'p' );\" => Stmt { body: Json(JsonStmt { column: Nested(NestedColumn { path: StringLit { text: __RecursaContentToken3 { text: ContentText(\"'p'\") } } }) }) } cursor=7 eof=true\n"), "{table_stdout}");
    // `UNBOUNDED` meets no shift on `PATH`, so it stays a column name.
    assert!(table_stdout.contains("ok \"columns ( unbounded path );\" => Stmt { body: Json(JsonStmt { column: Regular(RegularColumn { name: Name { text: __RecursaContentToken1 { text: ContentText(\"unbounded\"), kind: UNBOUNDED } }, ty: Name { text: __RecursaContentToken1 { text: ContentText(\"path\"), kind: PATH } } }) }) } cursor=6 eof=true\n"), "{table_stdout}");
    // `BETWEEN` (5) below `'-'` (10): the operator opens the frame extent.
    assert!(table_stdout.contains("ok \"rows between - 1 preceding and current row;\" => Stmt { body: Window(WindowStmt { extent: Between { low: Offset(FrameOffset { offset: Negate(Number(Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } })) }), high: CurrentRow } }) } cursor=9 eof=true\n"), "{table_stdout}");
    // `BETWEEN` before `PRECEDING`, which opens no bound, is a column name.
    assert!(table_stdout.contains("ok \"rows between preceding;\" => Stmt { body: Window(WindowStmt { extent: Single(Offset(FrameOffset { offset: Column(Name { text: __RecursaContentToken1 { text: ContentText(\"between\"), kind: BETWEEN } }) })) }) } cursor=4 eof=true\n"), "{table_stdout}");
    // `%prec UMINUS` below `'.'`: the indirection is shifted.
    assert!(table_stdout.contains("ok \"select ( select x ) . y;\" => Stmt { body: Select(SelectStmt { target: SubField(SubSelect { column: Name { text: __RecursaContentToken1 { text: ContentText(\"x\"), kind: NameText } } }, Name { text: __RecursaContentToken1 { text: ContentText(\"y\"), kind: NameText } }), label: None }) } cursor=8 eof=true\n"), "{table_stdout}");
    // The literal `SESSION CHARACTERISTICS` transaction form.
    assert!(table_stdout.contains("ok \"set session characteristics;\" => Stmt { body: Set(Characteristics) } cursor=4 eof=true\n"), "{table_stdout}");

    // The six inputs the parsers part on, each pinned for both.
    assert!(recursive_stdout.contains("parts ok \"select a like b escape;\" => Stmt { body: Select(SelectStmt { target: Like(Column(Name { text: __RecursaContentToken1 { text: ContentText(\"a\"), kind: NameText } }), Column(Name { text: __RecursaContentToken1 { text: ContentText(\"b\"), kind: NameText } }), None), label: Some(BareLabel { text: __RecursaContentToken0 { text: ContentText(\"escape\"), kind: ESCAPE } }) }) }\n"), "{recursive_stdout}");
    assert!(table_stdout.contains("parts err \"select a like b escape;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 22, end: 23 } found=Some(\"fixed:;\") cursor=0 leaked=0\n"), "{table_stdout}");
    assert!(recursive_stdout.contains("parts ok \"select a is json value;\" => Stmt { body: Select(SelectStmt { target: IsJson(Column(Name { text: __RecursaContentToken1 { text: ContentText(\"a\"), kind: NameText } }), Json), label: Some(BareLabel { text: __RecursaContentToken0 { text: ContentText(\"value\"), kind: VALUE } }) }) }\n"), "{recursive_stdout}");
    assert!(table_stdout.contains("parts ok \"select a is json value;\" => Stmt { body: Select(SelectStmt { target: IsJson(Column(Name { text: __RecursaContentToken1 { text: ContentText(\"a\"), kind: NameText } }), JsonValue), label: None }) }\n"), "{table_stdout}");
    assert!(recursive_stdout.contains("parts err \"select a is json value value;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 23, end: 28 } found=Some(\"fixed:value\") cursor=0 leaked=0\n"), "{recursive_stdout}");
    assert!(table_stdout.contains("parts ok \"select a is json value value;\" => Stmt { body: Select(SelectStmt { target: IsJson(Column(Name { text: __RecursaContentToken1 { text: ContentText(\"a\"), kind: NameText } }), JsonValue), label: Some(BareLabel { text: __RecursaContentToken0 { text: ContentText(\"value\"), kind: VALUE } }) }) }\n"), "{table_stdout}");
    assert!(recursive_stdout.contains("parts ok \"columns ( nested path );\" => Stmt { body: Json(JsonStmt { column: Regular(RegularColumn { name: Name { text: __RecursaContentToken1 { text: ContentText(\"nested\"), kind: NESTED } }, ty: Name { text: __RecursaContentToken1 { text: ContentText(\"path\"), kind: PATH } } }) }) }\n"), "{recursive_stdout}");
    assert!(table_stdout.contains("parts err \"columns ( nested path );\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 22, end: 23 } found=Some(\"fixed:)\") cursor=0 leaked=0\n"), "{table_stdout}");
    assert!(recursive_stdout.contains("parts err \"rows between + 1 preceding;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 13, end: 14 } found=Some(\"fixed:+\") cursor=0 leaked=0\n"), "{recursive_stdout}");
    assert!(table_stdout.contains("parts ok \"rows between + 1 preceding;\" => Stmt { body: Window(WindowStmt { extent: Single(Offset(FrameOffset { offset: Add(Column(Name { text: __RecursaContentToken1 { text: ContentText(\"between\"), kind: BETWEEN } }), Number(Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } })) })) }) }\n"), "{table_stdout}");
    assert!(recursive_stdout.contains("parts ok \"set session characteristics = 1;\" => Stmt { body: Set(Assign(SetAssign { scope: Some(Session), name: Name { text: __RecursaContentToken1 { text: ContentText(\"characteristics\"), kind: CHARACTERISTICS } }, value: Number(Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } }) })) }\n"), "{recursive_stdout}");
    assert!(table_stdout.contains("parts err \"set session characteristics = 1;\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 28, end: 29 } found=Some(\"fixed:=\") cursor=0 leaked=0\n"), "{table_stdout}");

    // The recursive-descent parser alone prints what `rd` printed beside `lr`.
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    assert_eq!(source.matches(NAMED_PARSERS).count(), 1);
    assert_eq!(source.matches(PRECEDENCE).count(), 1);
    for override_ in OVERRIDES {
        assert_eq!(source.matches(override_).count(), 1, "{override_}");
    }
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

    // The declaration is inert under recursive descent: removing it, with the overrides that
    // need it, changes no observable line and no generated source at all.
    // Blank lines keep every later declaration on its line: a Node's loader anchor hashes
    // its line, so deleting lines would move the identity for a reason that is not the
    // declaration.
    let mut undeclared_source =
        recursive_source.replace(PRECEDENCE, &"\n".repeat(PRECEDENCE.lines().count()));
    for override_ in OVERRIDES {
        undeclared_source = undeclared_source.replace(override_, "\n");
    }
    fs::write(&grammar, undeclared_source).expect("remove the precedence declaration");
    let undeclared = run_fixture(&package);
    assert_success(&undeclared);
    let (undeclared_stdout, _) = split_by_parser(&String::from_utf8(undeclared.stdout).unwrap());
    assert_eq!(
        undeclared_stdout, recursive_stdout,
        "the declaration changes nothing recursive descent observes"
    );
    let undeclared_files = read_selected_generation(&package);
    let sources = |files: &BTreeMap<String, String>| {
        files
            .iter()
            .filter(|(name, _)| name.ends_with(".rs"))
            .map(|(name, text)| (name.clone(), text.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    assert!(sources(&recursive_files).len() > 1);
    assert_eq!(
        recursive_files["identity"], undeclared_files["identity"],
        "the declaration is no input of the recursive-descent generation identity"
    );
    assert_eq!(
        sources(&recursive_files),
        sources(&undeclared_files),
        "recursive-descent sources are byte-identical with and without the declaration"
    );
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
