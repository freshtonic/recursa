//! End-to-end proof of the Pratt shapes gram.y writes (#122, #123, #124) through the sole
//! Recursa parser, including values, cursors, provenance, Pretty output, and failures.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/pratt-shapes";

/// A `Name` atom as the fixture's Debug output spells it.
fn name(text: &str) -> String {
    format!("Name(Name {{ text: __RecursaContentToken0 {{ text: ContentText({text:?}) }} }})")
}

#[test]
fn an_attached_optional_operand_parses_through_the_recursa_parser() {
    let package = scaffold(FIXTURE);
    let debug = tempfile::tempdir().unwrap();

    let output = run_fixture(&package, Some(debug.path()));
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("pratt-shapes-ok\n"), "{stdout}");
    let table_stdout = parser_lines(&stdout);
    let root = read_selected_root(&package);
    assert!(
        root.contains("__recursa_lr"),
        "root carries the tables and dispatcher"
    );

    // The escape operand takes the operand minimum: `+` binds inside it, `like` and `and` do
    // not; `escape` attaches to the innermost `like`, whether it is an infix or a postfix
    // variant, and to an operand of another variant when that operand is itself a `like`.
    let (a, b, c, d) = (name("a"), name("b"), name("c"), name("d"));
    for (source, value) in [
        ("a like b", format!("Like({a}, {b}, None)")),
        ("a like b escape c", format!("Like({a}, {b}, Some({c}))")),
        (
            "a like b escape c + d",
            format!("Like({a}, {b}, Some(Add({c}, {d})))"),
        ),
        (
            "a like b + c escape d",
            format!("Like({a}, Add({b}, {c}), Some({d}))"),
        ),
        (
            "a like b escape c like d",
            format!("Like(Like({a}, {b}, Some({c})), {d}, None)"),
        ),
        (
            "a like b escape c and d",
            format!("And(Like({a}, {b}, Some({c})), {d})"),
        ),
        (
            "a and b like c escape d",
            format!("And({a}, Like({b}, {c}, Some({d})))"),
        ),
        (
            "a like b not like c escape d",
            format!("NotLike(Like({a}, {b}, None), {c}, Some({d}))"),
        ),
        (
            "a not like b escape c",
            format!("NotLike({a}, {b}, Some({c}))"),
        ),
        (
            "a not like b like c escape d",
            format!("NotLike({a}, Like({b}, {c}, Some({d})), None)"),
        ),
        (
            "a not like b not like c escape d",
            format!("NotLike({a}, NotLike({b}, {c}, Some({d})), None)"),
        ),
    ] {
        let line = format!("ok {source:?} => {value} cursor=");
        assert!(
            table_stdout.contains(&line),
            "missing {line:?} in\n{table_stdout}"
        );
    }
    assert!(
        table_stdout.contains("ok \"(a like b) escape c\" => Parenthesized(")
            && table_stdout.contains("cursor=5 eof=false\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"a like b escape\" => code=RCA4101"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"escape a\" => code=RCA4100"),
        "{table_stdout}"
    );

    // The automaton dump: the attachments around the escape operand carry the operand
    // minimum, and every `escape` decision is resolved by precedence as a shift.
    let index_path = find_one_debug_index(debug.path());
    let dump = fs::read_to_string(index_path.parent().unwrap().join("automaton.txt"))
        .expect("the bundle carries the automaton dump");
    assert!(dump.contains("-> ESCAPE grammar::Expr[min 7]"), "{dump}");
    assert!(dump.contains("-> ESCAPE grammar::Expr[min 6]"), "{dump}");
    assert!(dump.contains("resolved by precedence (shift)."), "{dump}");
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");
}

#[test]
fn a_content_token_operator_on_a_prefix_variant_parses_through_the_recursa_parser() {
    let package = scaffold(FIXTURE);
    let debug = tempfile::tempdir().unwrap();

    let output = run_fixture(&package, Some(debug.path()));
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let table_stdout = parser_lines(&stdout);

    // The prefix operator takes its binding power: its operand binds `+` and `*` and stops
    // before `like` and `and`; the infix operator of the same class is left-associative at
    // its level, between `like` and `+`.
    let (a, b, c, d) = (name("a"), name("b"), name("c"), name("d"));
    let op = |text: &str| format!("__RecursaContentToken1 {{ text: ContentText({text:?}) }}");
    for (source, value) in [
        ("@a", format!("CustomPrefix({}, {a})", op("@"))),
        (
            "@ a + b",
            format!("CustomPrefix({}, Add({a}, {b}))", op("@")),
        ),
        (
            "@ a like b",
            format!("Like(CustomPrefix({}, {a}), {b}, None)", op("@")),
        ),
        (
            "@ @ a * b",
            format!(
                "CustomPrefix({}, CustomPrefix({}, Multiply({a}, {b})))",
                op("@"),
                op("@")
            ),
        ),
        (
            "a ~@ b + c",
            format!("CustomInfix({a}, {}, Add({b}, {c}))", op("~@")),
        ),
        (
            "a + b ~@ c",
            format!("CustomInfix(Add({a}, {b}), {}, {c})", op("~@")),
        ),
        (
            "a ~@ b ~@ c",
            format!(
                "CustomInfix(CustomInfix({a}, {}, {b}), {}, {c})",
                op("~@"),
                op("~@")
            ),
        ),
        (
            "a ~@ @ b",
            format!(
                "CustomInfix({a}, {}, CustomPrefix({}, {b}))",
                op("~@"),
                op("@")
            ),
        ),
        (
            "a like @ b escape @ c + d",
            format!(
                "Like({a}, CustomPrefix({}, {b}), Some(CustomPrefix({}, Add({c}, {d}))))",
                op("@"),
                op("@")
            ),
        ),
    ] {
        let line = format!("ok {source:?} => {value} cursor=");
        assert!(
            table_stdout.contains(&line),
            "missing {line:?} in
{table_stdout}"
        );
    }
    assert!(
        table_stdout.contains("err \"@\" => code=RCA4101"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"~@ like a\" => code=RCA4100"),
        "{table_stdout}"
    );

    // The lowering gives the prefix rule its binding power as precedence, so the automaton
    // resolves the operand's boundary and stays conflict-free.
    let index_path = find_one_debug_index(debug.path());
    let dump = fs::read_to_string(index_path.parent().unwrap().join("automaton.txt"))
        .expect("the bundle carries the automaton dump");
    assert!(
        dump.contains("grammar::Expr -> OperatorText grammar::Expr"),
        "{dump}"
    );
    assert!(
        dump.contains("grammar::Expr -> grammar::Expr OperatorText grammar::Expr"),
        "{dump}"
    );
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");
}

#[test]
fn an_exclusion_of_atoms_parses_through_the_recursa_parser() {
    let package = scaffold(FIXTURE);
    let debug = tempfile::tempdir().unwrap();

    let output = run_fixture(&package, Some(debug.path()));
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let table_stdout = parser_lines(&stdout);

    // `default` starts the second alternative only, because the first one's operand excludes
    // the atom; inside the parentheses of an atom the full language returns; an excluded
    // extender still ends the operand.
    let (a, b, c) = (name("a"), name("b"), name("c"));
    for (source, value) in [
        (
            "xmlnamespaces(a as b)",
            format!(
                "Xmlnamespaces(Named {{ uri: {a}, label: Name {{ text: __RecursaContentToken0 {{ text: ContentText(\"b\") }} }} }})"
            ),
        ),
        (
            "xmlnamespaces(a + b like c as d)",
            format!(
                "Xmlnamespaces(Named {{ uri: Like(Add({a}, {b}), {c}, None), label: Name {{ text: __RecursaContentToken0 {{ text: ContentText(\"d\") }} }} }})"
            ),
        ),
        (
            "xmlnamespaces(default a)",
            format!("Xmlnamespaces(Default {{ uri: {a} }})"),
        ),
        (
            "xmlnamespaces(default a + b)",
            format!("Xmlnamespaces(Default {{ uri: Add({a}, {b}) }})"),
        ),
        (
            "xmlnamespaces(default default)",
            "Xmlnamespaces(Default { uri: Default })".to_owned(),
        ),
        (
            "xmlnamespaces((default) as b)",
            format!(
                "Xmlnamespaces(Named {{ uri: Parenthesized(Default), label: Name {{ text: __RecursaContentToken0 {{ text: ContentText(\"b\") }} }} }})"
            ),
        ),
        (
            "index ((a + (select b)))",
            format!("Index(IndexElem {{ elem: Parenthesized(Add({a}, Subquery({b}))) }})"),
        ),
        (
            "index (a like b)",
            format!("Index(IndexElem {{ elem: Like({a}, {b}, None) }})"),
        ),
        ("(select a) + b", format!("Add(Subquery({a}), {b})")),
    ] {
        let line = format!("ok {source:?} => {value} cursor=");
        assert!(
            table_stdout.contains(&line),
            "missing {line:?} in\n{table_stdout}"
        );
    }
    assert!(
        table_stdout.contains("err \"xmlnamespaces(default a as b)\" => code=RCA4100"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"index (select a)\" => code=RCA4100"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"index ((select a))\" => code=RCA4100"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"index (a + (select b))\" => code=RCA4100"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("err \"index (a and b)\" => code=RCA4100"),
        "{table_stdout}"
    );

    // The exclusion clones drop the excluded atoms as they drop excluded extenders, and carry
    // the exclusion into their operands.
    let index_path = find_one_debug_index(debug.path());
    let dump = fs::read_to_string(index_path.parent().unwrap().join("automaton.txt"))
        .expect("the bundle carries the automaton dump");
    assert!(
        dump.contains("grammar::Expr[excl Default] -> grammar::Name"),
        "{dump}"
    );
    assert!(
        !dump.contains("grammar::Expr[excl Default] -> DEFAULT"),
        "{dump}"
    );
    assert!(dump.contains("grammar::Expr[excl Subquery,And] -> grammar::Expr[excl Subquery,And] PLUS grammar::Expr[excl Subquery,And]"), "{dump}");
    assert!(
        !dump.contains("grammar::Expr[excl Subquery,And] -> LPAREN SELECT"),
        "{dump}"
    );
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");
}

/// Copies the fixture package into a temporary directory.
fn scaffold(fixture: &str) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(fixture);
    for relative in ["build.rs", "src/lib.rs", "src/grammar.rs", "src/main.rs"] {
        copy(&fixture, package.path(), relative, relative);
    }
    copy(&fixture, package.path(), "Cargo.core.lock", "Cargo.lock");
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

fn read_selected_root(package: &TempDir) -> String {
    let mut found = Vec::new();
    find_named(
        &package.path().join("target"),
        "recursa-selected.rs",
        &mut found,
    );
    assert_eq!(found.len(), 1, "one selected Generation: {found:?}");
    let selected = found.pop().unwrap();
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

fn copy(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy generated grammar fixture file");
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn run_fixture(package: &TempDir, debug_directory: Option<&Path>) -> Output {
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
        .env("CARGO_TARGET_DIR", package.path().join("target"));
    match debug_directory {
        Some(directory) => command.env("RECURSA_DEBUG_DIR", directory),
        None => command.env_remove("RECURSA_DEBUG_DIR"),
    };
    command.output().expect("run generated grammar fixture")
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

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
