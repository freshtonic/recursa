//! The two lookahead-filter fixtures through the sole LR parser (#139).
//!
//! `lookahead` holds the three shapes the substitution rule distinguishes: a filtered token
//! inside a rule (`NOT LIKE`, which becomes the merged kind outright), a filtered token whose
//! rest mixes triggers and names (`NOT a_expr`, which is twinned), and a filtered token that
//! ends its rule (`AnyNot`, which holds the merged kind only because the grammar names it at
//! that site, as `gram.y` writes `any_with: WITH | WITH_LA`). `lookahead-precedence` holds
//! PostgreSQL's window-frame conflict, which only the merged kind's `precedence` level settles.
//!
//! Both fixtures lost their driver test at 128a7dc, when the second parser style they compared
//! was retired. These run the one parser and compare the lines it prints.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

#[test]
fn the_lookahead_fixture_substitutes_twins_and_honors_an_admission() {
    let package = scaffold_fixture("tests/fixtures/generation/lookahead");
    let output = run_bin(&package, "recursa-generated-lookahead");
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("lookahead-ok\n"), "{stdout}");

    for line in [
        // The filter is what lets `a not between 1 and 2` be a test and `a not b` a conjunction:
        // without it the two shapes are a reduce/reduce conflict on `NOT`.
        "lr ok \"a not between 1 and 2;\" => Rule { cond: NotBetween(",
        "lr ok \"a not b;\" => Rule { cond: AndNot(",
        // `NOT a_expr` is twinned, so prefix `not` takes an operand that begins with a trigger
        // read as a name.
        "lr expr ok \"not like like b;\" => ExprRule { expr: Not(",
        "lr expr ok \"not a not like b;\" => ExprRule { expr: Not(",
        // The merged kind exists, is never lexed, and spells like the token it merges.
        "lr merged kind NOT_LA displays as not like not",
        // The admission: a rule-final filtered token the grammar names, both arms in use.
        "lr marked \"not between;\" => Marked { mark: Merged, name: Ident {",
        "lr marked \"not like;\" => Marked { mark: Merged, name: Ident {",
        "lr marked \"not a;\" => Marked { mark: Plain, name: Ident {",
    ] {
        assert!(stdout.contains(line), "fixture lacks {line}\n{stdout}");
    }

    // The error path never names the merged kind: `found` is the spelling the author wrote.
    assert!(
        stdout.contains("lr expr err \"a not;\" => code=RCA4100 kind=UnexpectedToken"),
        "{stdout}"
    );
    assert!(
        stdout.contains("found=Some(\"fixed:not\")"),
        "an expected or found atom reads as the spelling, never as `merged:`\n{stdout}"
    );
    assert!(!stdout.contains("merged:"), "{stdout}");
}

#[test]
fn the_lookahead_precedence_fixture_settles_the_frame_conflict_on_the_merged_kind() {
    let package = scaffold_fixture("tests/fixtures/generation/lookahead-precedence");
    let output = run_bin(&package, "recursa-generated-lookahead-precedence");
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with("lookahead-precedence-ok\n"), "{stdout}");

    for line in [
        "lr ok \"rows between a preceding and b preceding;\" => Frame { extent: Between {",
        // `between` and `like` as the column of a single bound: the reduce.
        "lr ok \"rows between preceding;\" => Frame { extent: Single(",
        "lr ok \"rows like preceding;\" => Frame { extent: Single(",
        // `NOT` before a trigger read as a name: the prefix twin inside a bound.
        "lr ok \"rows not like preceding;\" => Frame { extent: Single(",
        "lr ok \"rows not between preceding;\" => Frame { extent: Single(",
        // The input the merged kind's `%nonassoc` level decides: `ROWS BETWEEN . NOT_LA` is a
        // tie, and a tie at a `nonassoc` level is an error, as it is in PostgreSQL. The parser
        // reaches it as an unexpected token, because the level installed no action for the cell.
        "lr decision no => code=RCA4100",
    ] {
        assert!(stdout.contains(line), "fixture lacks {line}\n{stdout}");
    }
}

fn scaffold_fixture(fixture: &str) -> TempDir {
    let package = tempfile::tempdir().expect("temporary generated grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let fixture = manifest_dir.join(fixture);

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
