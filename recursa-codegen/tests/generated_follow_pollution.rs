//! FOLLOW pollution (#125) under both parser styles in one generation. The fixture's grammar
//! holds the two shapes the ticket names: a shared `opt(X)` helper whose merged lookaheads are a
//! reduce/reduce conflict that a per-site copy removes, and two token wrappers over one pattern
//! that compete on the end marker and are settled by declaration order. The `rd` and `lr`
//! parsers print the same values, cursors, provenance and Pretty renderings for every input,
//! and the same failing token, span and kind for every error; expected sets may differ
//! (`docs/table-driven-parsing.md`, section 6). The one documented difference is the bare
//! `FuncName` root at the end of input, where the declaration-order rule reaches its limit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const FIXTURE: &str = "tests/fixtures/generation/follow-pollution";

#[test]
fn both_parsers_agree_on_every_input_but_the_documented_wrapper_limit() {
    let package = scaffold_fixture();
    let rd = run_bin(&package, "recursa-generated-follow-pollution");
    assert_success(&rd);
    let rd_stdout = String::from_utf8(rd.stdout).unwrap();
    assert!(rd_stdout.ends_with("rd-ok\n"), "{rd_stdout}");
    let lr = run_bin(&package, "lr");
    assert_success(&lr);
    let lr_stdout = String::from_utf8(lr.stdout).unwrap();
    assert!(lr_stdout.ends_with("lr-ok\n"), "{lr_stdout}");

    // The shared-helper shape: every sentence parses to its own derivation under both styles,
    // so the helper copy the construction made changed nothing observable.
    for line in [
        "ok Statement \"a 1 c\" => A(A { x: Some(",
        "ok Statement \"a 1 d\" => A2(A2 { y: Y { x: X {",
        "ok Statement \"a c\" => A(A { x: None }) cursor=2 eof=true",
        "ok Statement \"b 1 d\" => B(B { x: Some(",
        "ok Statement \"b 1 c\" => B2(B2 { y: Y { x: X {",
        "ok Statement \"b d\" => B(B { x: None }) cursor=2 eof=true",
        // The token-wrapper shape: the root that continues past the wrapper takes the call on
        // `( )` and the column at the end of input.
        "ok Expr \"foo\" => Column(ColumnRef { name: ColId {",
        "ok Expr \"foo()\" => Call(FuncCall { name: TypeFunctionName {",
        "ok FuncName \"foo.bar\" => Qualified(QualifiedName { first: ColId {",
    ] {
        assert!(rd_stdout.contains(line), "rd lacks {line}\n{rd_stdout}");
        assert!(lr_stdout.contains(line), "lr lacks {line}\n{lr_stdout}");
    }

    // The limit of the declaration-order rule: `FuncName` can end only in the later-declared
    // wrapper, so at the end of input the `lr` parser reduces `ColIdText`, looks for `.` and
    // reports an unexpected end where `rd` accepts `Plain`.
    assert!(
        rd_stdout.contains("ok FuncName \"foo\" => Plain(TypeFunctionName {"),
        "{rd_stdout}"
    );
    assert!(
        lr_stdout.contains("err FuncName \"foo\" => code=RCA4101 kind=UnexpectedEof span=Span { start: 3, end: 3 } found=None cursor=0 expected=["),
        "{lr_stdout}"
    );

    // Everything else is equal line for line once expected sets are set aside.
    let rd_lines = comparable_lines(&rd_stdout, "rd-ok");
    let lr_lines = comparable_lines(&lr_stdout, "lr-ok");
    assert_eq!(lr_lines, rd_lines);
    assert!(
        rd_lines
            .iter()
            .any(|line| line.starts_with("prov Statement \"a 1 c\" => captured")),
        "{rd_lines:?}"
    );
    assert!(
        rd_lines.iter().any(
            |line| line.starts_with("err Statement \"a 1\" => code=RCA4101 kind=UnexpectedEof")
        ),
        "{rd_lines:?}"
    );
    assert!(
        rd_lines
            .iter()
            .any(|line| line.starts_with("err Statement \"b 1 x\" => ")),
        "{rd_lines:?}"
    );
}

/// Every printed line except the parser's own final marker and the documented `FuncName`
/// difference, with expected sets removed from error lines.
fn comparable_lines(stdout: &str, marker: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|line| *line != marker && !line.contains(" FuncName \"foo\" "))
        .map(|line| match line.find(" expected=") {
            Some(index) if line.starts_with("err ") => line[..index].to_owned(),
            _ => line.to_owned(),
        })
        .collect()
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
    copy(&fixture, package.path(), "src/bin/lr.rs");

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

fn run_bin(package: &TempDir, bin: &str) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--locked")
        .arg("--quiet")
        .arg("--bin")
        .arg(bin)
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

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
