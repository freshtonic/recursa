//! End-to-end proof of the Pratt shapes gram.y writes (#122, #123, #124): the `pratt-shapes`
//! fixture generates once with `parsers(rd = recursive_descent, lr = table_driven)` and
//! everything the `lr` parser's consumer observes (values, cursors, provenance, elements,
//! Pretty renderings, failing tokens and restored cursors) equals what the `rd` parser of the
//! same generation produces.

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
fn an_attached_optional_operand_parses_alike_under_both_parsers() {
    let package = scaffold(FIXTURE);
    let debug = tempfile::tempdir().unwrap();

    let output = run_fixture(&package, Some(debug.path()));
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("pratt-shapes-ok\n"), "{stdout}");
    let (recursive_stdout, table_stdout) = split_by_parser(&stdout);
    let root = read_selected_root(&package);
    assert!(root.contains("__recursa_lr"), "root carries the tables and dispatcher");
    assert!(root.contains("RecursiveDescent") && root.contains("pub mod lr"), "one generation carries both parsers");

    // The escape operand takes the operand minimum: `+` binds inside it, `like` and `and` do
    // not; `escape` attaches to the innermost `like`, whether it is an infix or a postfix
    // variant, and to an operand of another variant when that operand is itself a `like`.
    let (a, b, c, d) = (name("a"), name("b"), name("c"), name("d"));
    for (source, value) in [
        ("a like b", format!("Like({a}, {b}, None)")),
        ("a like b escape c", format!("Like({a}, {b}, Some({c}))")),
        ("a like b escape c + d", format!("Like({a}, {b}, Some(Add({c}, {d})))")),
        ("a like b + c escape d", format!("Like({a}, Add({b}, {c}), Some({d}))")),
        ("a like b escape c like d", format!("Like(Like({a}, {b}, Some({c})), {d}, None)")),
        ("a like b escape c and d", format!("And(Like({a}, {b}, Some({c})), {d})")),
        ("a and b like c escape d", format!("And({a}, Like({b}, {c}, Some({d})))")),
        ("a like b not like c escape d", format!("NotLike(Like({a}, {b}, None), {c}, Some({d}))")),
        ("a not like b escape c", format!("NotLike({a}, {b}, Some({c}))")),
        ("a not like b like c escape d", format!("NotLike({a}, Like({b}, {c}, Some({d})), None)")),
        ("a not like b not like c escape d", format!("NotLike({a}, NotLike({b}, {c}, Some({d})), None)")),
    ] {
        let line = format!("ok {source:?} => {value} cursor=");
        assert!(table_stdout.contains(&line), "missing {line:?} in\n{table_stdout}");
    }
    assert!(table_stdout.contains("ok \"(a like b) escape c\" => Parenthesized(") && table_stdout.contains("cursor=5 eof=false\n"), "{table_stdout}");
    assert!(table_stdout.contains("err \"a like b escape\" => code=RCA4101"), "{table_stdout}");
    assert!(table_stdout.contains("err \"escape a\" => code=RCA4100"), "{table_stdout}");

    // The automaton dump: the attachments around the escape operand carry the operand
    // minimum, and every `escape` decision is resolved by precedence as a shift.
    let index_path = find_one_debug_index(debug.path());
    let dump = fs::read_to_string(index_path.parent().unwrap().join("automaton.txt"))
        .expect("the bundle carries the automaton dump");
    assert!(dump.contains("-> ESCAPE grammar::Expr[min 7]"), "{dump}");
    assert!(dump.contains("-> ESCAPE grammar::Expr[min 6]"), "{dump}");
    assert!(dump.contains("resolved by precedence (shift)."), "{dump}");
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");

    // The recursive-descent parser of the same generation prints the same lines.
    assert_eq!(table_stdout, recursive_stdout, "the two parsers must agree on every line");
}

#[test]
fn a_content_token_operator_on_a_prefix_variant_parses_alike_under_both_parsers() {
    let package = scaffold(FIXTURE);
    let debug = tempfile::tempdir().unwrap();

    let output = run_fixture(&package, Some(debug.path()));
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let (recursive_stdout, table_stdout) = split_by_parser(&stdout);

    // The prefix operator takes its binding power: its operand binds `+` and `*` and stops
    // before `like` and `and`; the infix operator of the same class is left-associative at
    // its level, between `like` and `+`.
    let (a, b, c, d) = (name("a"), name("b"), name("c"), name("d"));
    let op = |text: &str| format!("__RecursaContentToken1 {{ text: ContentText({text:?}) }}");
    for (source, value) in [
        ("@a", format!("CustomPrefix({}, {a})", op("@"))),
        ("@ a + b", format!("CustomPrefix({}, Add({a}, {b}))", op("@"))),
        ("@ a like b", format!("Like(CustomPrefix({}, {a}), {b}, None)", op("@"))),
        ("@ @ a * b", format!("CustomPrefix({}, CustomPrefix({}, Multiply({a}, {b})))", op("@"), op("@"))),
        ("a ~@ b + c", format!("CustomInfix({a}, {}, Add({b}, {c}))", op("~@"))),
        ("a + b ~@ c", format!("CustomInfix(Add({a}, {b}), {}, {c})", op("~@"))),
        ("a ~@ b ~@ c", format!("CustomInfix(CustomInfix({a}, {}, {b}), {}, {c})", op("~@"), op("~@"))),
        ("a ~@ @ b", format!("CustomInfix({a}, {}, CustomPrefix({}, {b}))", op("~@"), op("@"))),
        (
            "a like @ b escape @ c + d",
            format!("Like({a}, CustomPrefix({}, {b}), Some(CustomPrefix({}, Add({c}, {d}))))", op("@"), op("@")),
        ),
    ] {
        let line = format!("ok {source:?} => {value} cursor=");
        assert!(table_stdout.contains(&line), "missing {line:?} in
{table_stdout}");
    }
    assert!(table_stdout.contains("err \"@\" => code=RCA4101"), "{table_stdout}");
    assert!(table_stdout.contains("err \"~@ like a\" => code=RCA4100"), "{table_stdout}");

    // The lowering gives the prefix rule its binding power as precedence, so the automaton
    // resolves the operand's boundary and stays conflict-free.
    let index_path = find_one_debug_index(debug.path());
    let dump = fs::read_to_string(index_path.parent().unwrap().join("automaton.txt"))
        .expect("the bundle carries the automaton dump");
    assert!(dump.contains("grammar::Expr -> OperatorText grammar::Expr"), "{dump}");
    assert!(dump.contains("grammar::Expr -> grammar::Expr OperatorText grammar::Expr"), "{dump}");
    assert!(dump.ends_with("No conflicts.\n"), "{dump}");

    assert_eq!(table_stdout, recursive_stdout, "the two parsers must agree on every line");
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

fn read_selected_root(package: &TempDir) -> String {
    let mut found = Vec::new();
    find_named(&package.path().join("target"), "recursa-selected.rs", &mut found);
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
