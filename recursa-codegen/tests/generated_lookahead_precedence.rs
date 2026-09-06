//! End-to-end proof that a lookahead filter's merged kind carries a declared precedence level
//! (#128): the window-frame fixture names `NOT_LA` in its `precedence` block, as gram.y names it
//! (`%nonassoc BETWEEN IN_P LIKE ILIKE SIMILAR NOT_LA`, `gram.y:837`), and generates with
//! `parsers(rd = recursive_descent, lr = table_driven)` (#119).
//!
//! Three things are proved here. The two parsers of the one generation print the same line for
//! every input, accepted and rejected alike. Taking `NOT_LA` out of the block leaves the
//! recursive-descent generation byte-identical, generation identity included, because the
//! declaration is no input of it. And taking it out of the table-driven generation is what the
//! ticket is about: the construction fails with `RCA0400` on the lookahead `NOT_LA`, the pg-sql
//! conflict `ColIdText -> BETWEEN .` against `WindowFrameBody -> BETWEEN . NOT_LA`, which no
//! other declaration could retire.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/lookahead-precedence";

/// The configuration of the fixture grammar as written, replaced to generate the
/// recursive-descent parser alone.
const NAMED_PARSERS: &str = "parsers(rd = recursive_descent, lr = table_driven),";

/// The fixture's precedence level as written.
const DECLARED_LEVEL: &str = "nonassoc(bp = 20) { BETWEEN, LIKE, NOT_LA },";

/// The same level without the merged kind, which is all a grammar could say before #128. It has
/// the line count of the declared level, so no Node's loader anchor moves.
const UNDECLARED_LEVEL: &str = "nonassoc(bp = 20) { BETWEEN, LIKE },";

#[test]
fn the_merged_kind_precedence_fixture_parses_like_recursive_descent_and_needs_its_level() {
    let package = scaffold_fixture();

    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("lookahead-precedence-ok\n"), "{stdout}");
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

    // `between` and `like` name a column, so the single bound is what a bare one is.
    assert!(table_stdout.contains("ok \"rows between preceding;\" => Frame { extent: Single(Bound { offset: Column(Ident { text: __RecursaContentToken0 { text: ContentText(\"between\"), kind: BETWEEN } }) }) } cursor=4 eof=true kinds=[rows between preceding ;]\n"), "{table_stdout}");
    assert!(table_stdout.contains("ok \"rows like preceding;\" => Frame { extent: Single(Bound { offset: Column(Ident { text: __RecursaContentToken0 { text: ContentText(\"like\"), kind: LIKE } }) }) } cursor=4 eof=true kinds=[rows like preceding ;]\n"), "{table_stdout}");
    // The prefix `NOT` twin: `not` before a trigger read as a name is still a negation.
    assert!(table_stdout.contains("ok \"rows not between preceding;\" => Frame { extent: Single(Bound { offset: Not(Column(Ident { text: __RecursaContentToken0 { text: ContentText(\"between\"), kind: BETWEEN } })) }) } cursor=5 eof=true kinds=[rows not between preceding ;]\n"), "{table_stdout}");
    // The frame extent, and a bound that begins with the prefix, are unaffected.
    assert!(table_stdout.contains("ok \"rows between not a preceding and b preceding;\" => Frame { extent: Between { low: Bound { offset: Not(Column(Ident { text: __RecursaContentToken0 { text: ContentText(\"a\"), kind: IdentText } })) }, high: Bound { offset: Column(Ident { text: __RecursaContentToken0 { text: ContentText(\"b\"), kind: IdentText } }) } } } cursor=9 eof=true kinds=[rows between not IdentText preceding and IdentText preceding ;]\n"), "{table_stdout}");
    // `ROWS BETWEEN . NOT_LA`: the level makes the tie an error action, as gram.y's
    // `%nonassoc` does, and recursive descent reaches the same verdict by reading on.
    assert!(
        table_stdout.contains("decision no => code=RCA4100 kind=UnexpectedToken cursor=0\n"),
        "{table_stdout}"
    );

    // The recursive-descent parser alone prints what `rd` printed beside `lr`.
    let grammar = package.path().join("src/grammar.rs");
    let source = fs::read_to_string(&grammar).expect("read scaffolded grammar");
    assert_eq!(source.matches(NAMED_PARSERS).count(), 1);
    assert_eq!(source.matches(DECLARED_LEVEL).count(), 1);
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
    assert!(
        recursive_files["root.rs"].contains("RecursiveDescent")
            && !recursive_files["root.rs"].contains("__recursa_lr")
    );

    // Naming the merged kind is inert under recursive descent: every generated file, the
    // generation identity included, is byte-identical without it.
    fs::write(
        &grammar,
        recursive_source.replace(DECLARED_LEVEL, UNDECLARED_LEVEL),
    )
    .expect("take the merged kind out of the precedence block");
    let undeclared = run_fixture(&package);
    assert_success(&undeclared);
    let (undeclared_stdout, _) = split_by_parser(&String::from_utf8(undeclared.stdout).unwrap());
    assert_eq!(
        undeclared_stdout, recursive_stdout,
        "the declaration changes nothing recursive descent observes"
    );
    assert_eq!(
        read_selected_generation(&package),
        recursive_files,
        "the recursive-descent generation is byte-identical without the merged kind's level"
    );

    // The table-driven generation is what needs the level: without it the construction fails
    // on the pg-sql conflict this ticket exists for.
    fs::write(&grammar, source.replace(DECLARED_LEVEL, UNDECLARED_LEVEL))
        .expect("restore both parsers without the merged kind's level");
    fs::write(&driver, driver_source).expect("restore the driver");
    let conflicted = run_fixture(&package);
    assert!(
        !conflicted.status.success(),
        "without the merged kind's level the construction must fail"
    );
    let stderr = String::from_utf8_lossy(&conflicted.stderr).into_owned();
    assert!(
        stderr.contains("error[RCA0400]: shift/reduce conflict"),
        "{stderr}"
    );
    assert!(stderr.contains("on lookahead `NOT_LA`"), "{stderr}");
    assert!(stderr.contains("reduce using rule"), "{stderr}");
    assert!(stderr.contains("IdentText -> BETWEEN"), "{stderr}");
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
