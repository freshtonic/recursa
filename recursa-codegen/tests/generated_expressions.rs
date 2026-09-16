//! End-to-end proof of `docs/declared-precedence.md` through a generated parser: a prefix
//! override taken from a precedence-only name, left, right and nonassociative infix levels, a
//! postfix operator, an interior operand behind a restricted expression, an `ESCAPE`-style
//! keyword one level above its operator, and the `RCA0400` build error an undeclared operator
//! earns.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// The generation fixture this test scaffolds, runs, and mutates.
const FIXTURE: &str = "tests/fixtures/generation/expressions";

/// The level the variant grammar drops so that `STAR` has no declared precedence.
const STAR_LEVEL: &str = "        left(bp = 30) { STAR },\n";

#[test]
fn the_expressions_fixture_decides_every_operator_by_its_declared_level() {
    let package = scaffold(FIXTURE, None);
    let output = run_fixture(&package);
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("expressions-ok\n"), "{stdout}");
    let lines = parser_lines(&stdout);

    // A prefix override through a precedence-only name. `MINUS` is the rule's only terminal, so
    // without `#[parse(prec = UMINUS)]` the prefix form would sit at the additive level and
    // negate the product instead of multiplying the negation.
    for expected in [
        "ok \"- a * b\" => mul(neg(a), b) eof=true\n",
        "ok \"- a + b\" => add(neg(a), b) eof=true\n",
        // `UMINUS` is below `BANG` and above `CARET`, exactly as its declared position says.
        "ok \"- a ^ b\" => pow(neg(a), b) eof=true\n",
        "ok \"- a !\" => neg(fac(a)) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // Left, right and nonassociative infix levels, and a postfix operator above all of them.
    for expected in [
        "ok \"a + b + c\" => add(add(a, b), c) eof=true\n",
        "ok \"a + b * c\" => add(a, mul(b, c)) eof=true\n",
        "ok \"a ^ b ^ c\" => pow(a, pow(b, c)) eof=true\n",
        "ok \"a = b + c\" => eq(a, add(b, c)) eof=true\n",
        "ok \"a !\" => fac(a) eof=true\n",
        "ok \"a ^ b !\" => pow(a, fac(b)) eof=true\n",
        "ok \"a ! !\" => fac(fac(a)) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }
    // A nonassociative tie is an error action, not a parse.
    assert!(
        lines.contains("err \"a = b = c\" => code=RCA4100 kind=UnexpectedToken"),
        "{lines}"
    );
    assert!(
        lines.contains("err \"a like b like c\" => code=RCA4100 kind=UnexpectedToken"),
        "{lines}"
    );

    // An interior operand through a restricted expression: the `and` of a `between` clause
    // closes it, and the parenthesized atom keeps the full language, as `b_expr` does.
    for expected in [
        "ok \"a and b\" => and(a, b) eof=true\n",
        "ok \"a between b and c\" => between(a, b, c) eof=true\n",
        "ok \"a between b + c and d\" => between(a, add(b, c), d) eof=true\n",
        "ok \"a between ( b and c ) and d\" => between(a, paren(and(b, c)), d) eof=true\n",
        // `#[parse(prec = BETWEEN)]` keeps the completed clause above boolean `and`.
        "ok \"a between b and c and d\" => and(between(a, b, c), d) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }

    // `ESCAPE` one level above `LIKE`: the keyword is shifted out of the completed comparison,
    // while a token below `LIKE` reduces it first.
    for expected in [
        "ok \"a like b escape c\" => like(a, b, escape c) eof=true\n",
        "ok \"a like b\" => like(a, b) eof=true\n",
        "ok \"a like b and c\" => and(like(a, b), c) eof=true\n",
        "ok \"a like b + c escape d\" => like(a, add(b, c), escape d) eof=true\n",
    ] {
        assert!(lines.contains(expected), "{expected}\n{lines}");
    }
    assert!(
        lines.contains("err \"a like b escape\" => code=RCA4101 kind=UnexpectedEof"),
        "{lines}"
    );
    assert!(
        lines.contains("err \"a between b and\" => code=RCA4101 kind=UnexpectedEof"),
        "{lines}"
    );
}

#[test]
fn an_undeclared_operator_is_an_rca0400_build_error_with_an_authoring_remedy() {
    let grammar = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(FIXTURE)
            .join("src/grammar.rs"),
    )
    .expect("read the expressions grammar");
    assert_eq!(grammar.matches(STAR_LEVEL).count(), 1, "{grammar}");
    let undeclared = grammar.replace(STAR_LEVEL, "");

    let package = scaffold(FIXTURE, Some(&undeclared));
    let output = run_fixture(&package);
    assert!(
        !output.status.success(),
        "an undeclared operator must fail the build\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[RCA0400]"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("shift/reduce conflict"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("on lookahead `STAR`"), "stderr:\n{stderr}");
    // The remedy is an authoring instruction, not an internal-invariant report.
    assert!(
        stderr.contains("declare a precedence level for the lookahead token"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("add `#[parse(prec = NAME)]` to the rule that reduces"),
        "stderr:\n{stderr}"
    );
    assert!(!stderr.contains("RCA9105"), "stderr:\n{stderr}");
}

/// Copies the fixture package into a temporary directory; `grammar` replaces its grammar file.
fn scaffold(fixture: &str, grammar: Option<&str>) -> TempDir {
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
    if let Some(grammar) = grammar {
        fs::write(package.path().join("src/grammar.rs"), grammar).expect("write the grammar");
    }
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

/// Selects the generated parser's tagged fixture lines and terminates each with a newline so
/// the assertions can match whole lines.
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

/// Copies one fixture file into the scaffolded package.
fn copy(fixture: &Path, package: &Path, source: &str, destination: &str) {
    let destination = package.join(destination);
    fs::create_dir_all(destination.parent().unwrap()).expect("create fixture directory");
    fs::copy(fixture.join(source), destination).expect("copy generated grammar fixture file");
}

/// Renders a path for a TOML string literal.
fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

/// Builds and runs the scaffolded fixture binary.
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
        .env("CARGO_TARGET_DIR", package.path().join("target"))
        .output()
        .expect("run generated grammar fixture")
}

/// Fails with both streams when the fixture did not run.
fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
