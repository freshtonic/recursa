//! End-to-end proof of the table-driven container lowering (#114): the container fixture
//! grammar generates once with `parsers(rd = recursive_descent, lr = table_driven)` (#119),
//! every positive input parses through the `lr` parser to the value the `rd` parser of the same
//! generation produces, and every failing input restores the cursor and drops its partial
//! values.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/table-driven-containers";

#[test]
fn every_container_shape_parses_like_the_recursive_descent_parser_of_the_same_generation() {
    let package = scaffold_fixture();

    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("table-driven-containers-ok\n"), "{stdout}");
    let (recursive_stdout, table_stdout) = split_by_parser(&stdout);
    let root = read_selected_root(&package);
    assert!(root.contains("__recursa_lr"), "root carries the tables and dispatcher");
    assert!(root.contains("RecursiveDescent") && root.contains("pub mod lr"), "one generation carries both parsers");

    // Spot checks on the shapes the value stack must build correctly.
    assert!(
        table_stdout.contains("ok \"let one if not exists x = 2 as y;\" => Let(Let { unless: true, name: Name { text: __RecursaContentToken1 { text: ContentText(\"x\"), kind: NameText } }, value: Number(Number { text: __RecursaContentToken2 { text: ContentText(\"2\") } }), alias: Some(Alias { name: Name { text: __RecursaContentToken1 { text: ContentText(\"y\"), kind: NameText } } }) }) cursor=11 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"f(1, 2,);\" => Call(Call { callee: Callee { text: __RecursaContentToken0 { text: ContentText(\"f\"), kind: NameText } }, args: Args { values: [Number(Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } }), Number(Number { text: __RecursaContentToken2 { text: ContentText(\"2\") } })] } }) cursor=8 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"mods one three four six x;\" => Modifiers(Modifiers { four: true, five: false, six: true, name: Name { text: __RecursaContentToken1 { text: ContentText(\"x\"), kind: NameText } } }) cursor=7 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"options as out temp;\" => Options(Options { a: Some(KwAs), b: None, c: Some(KwOut), d: None, e: Some(KwTemp) }) cursor=5 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"marks !! : !;\" => Marks(Marks { marks: [Bang, Bang], tail: Some(Vec1 { values: [Bang] }) }) cursor=6 eof=true\n"),
        "{table_stdout}"
    );
    assert!(
        table_stdout.contains("ok \"print (1)|(as)|([]);\" => Print(Print { negated: false, value: Tuple(Tuple { cells: Vec1 { values: [Number(Number { text: __RecursaContentToken2 { text: ContentText(\"1\") } }), Name { name: Name { text: __RecursaContentToken1 { text: ContentText(\"as\"), kind: AS } } }, List(List { items: None })] } }), guard: None }) cursor=14 eof=true\n"),
        "{table_stdout}"
    );
    // A failing input is transactional with vectors and boxes on the value stack.
    assert!(
        table_stdout.contains("err \"{ print 1; { let if exists x = 2; } f() }\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 40, end: 41 } found=Some(\"fixed:}\") cursor=0 leaked=0\n"),
        "{table_stdout}"
    );

    // The recursive-descent parser of the same generation prints the same lines.
    // Provenance (#118): erased groups, presences and envelopes, repetitions with their
    // separators, attachments and helper-lowered optionals all fold to recursive descent's
    // occurrence tree, which the twin comparison below holds to byte equality.
    assert!(table_stdout.contains("prov \"let one if not exists x = 2 as y;\" => captured 0..33 (0..33: (0..33: (0..33: (0..3) (4..7: (4..7)) (8..21: (8..10) (11..14: (11..14)) (15..21)) (22..23: (22..23: (22..23: (22..23)))) (24..25) (26..27: (26..27: (26..27: (26..27: (26..27: (26..27)))))) (28..32: (28..32: (28..30) (31..32: (31..32: (31..32: (31..32)))))) (32..33))))\n"), "{table_stdout}");
    assert!(table_stdout.contains("prov \"marks :;\" => captured 0..8 (0..8: (0..8: (0..8: (0..5) _ (6..7) (7..7) (7..8))))\n"), "{table_stdout}");
    assert!(table_stdout.contains("pretty \"let one if not exists x = 2 as y;\" => \"let one if not exists x= 2 as y;\"\n"), "{table_stdout}");
    // A struct root ending in a content token accepts the prefix; a repetition tail rejects
    // the token after it, as recursive descent does (#118).
    assert!(table_stdout.contains("trailing Param \"x y\" => ok Param { mode: None, name: Name { text: __RecursaContentToken1 { text: ContentText(\"x\"), kind: NameText } } } cursor=1 eof=false\n"), "{table_stdout}");
    assert!(table_stdout.contains("trailing Value \"(1) x\" => code=RCA4100 kind=UnexpectedToken span=Span { start: 4, end: 5 }"), "{table_stdout}");

    assert_eq!(table_stdout, recursive_stdout, "the two parsers must agree on every line");
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
