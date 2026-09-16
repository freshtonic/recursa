//! A restricted expression proves itself through the generated parser, not through the lowered
//! rules alone: `a between b and c` closes its `BETWEEN` clause at the `and` that the boolean
//! operator would otherwise consume, and `a between (b and c) and d` reaches boolean `and`
//! through the parenthesized atom, which keeps the full language exactly as `gram.y`'s
//! `b_expr: '(' a_expr ')'` does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// The source-analysis fixture whose grammar this test compiles and runs.
const FIXTURE: &str = "tests/fixtures/source-analysis/restricted";

#[test]
fn a_downstream_crate_parses_a_restricted_interior_operand() {
    let package = scaffold();
    let output = run(&package);
    assert!(
        output.status.success(),
        "restricted fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "restricted-ok"
    );
}

fn scaffold() -> TempDir {
    let package = tempfile::tempdir().expect("temporary restricted grammar package");
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("recursa-codegen belongs to the workspace root");
    let grammar = fs::read_to_string(manifest_dir.join(FIXTURE).join("grammar.rs"))
        .expect("read the restricted grammar fixture");

    fs::create_dir_all(package.path().join("src")).expect("create package source directory");
    fs::write(
        package.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "recursa-generated-restricted"
version = "0.0.0"
edition = "2024"
rust-version = "1.88"
build = "build.rs"

[dependencies]
recursa = {{ path = {:?} }}

[build-dependencies]
recursa-codegen = {{ path = {:?} }}

[workspace]
"#,
            workspace.join("recursa"),
            workspace.join("recursa-codegen"),
        ),
    )
    .expect("write package manifest");
    fs::write(
        package.path().join("build.rs"),
        "fn main() -> Result<(), recursa_codegen::GenerateError> {\n    \
         recursa_codegen::generate(\"src/restricted.rs\")?;\n    Ok(())\n}\n",
    )
    .expect("write build script");
    fs::write(package.path().join("src/restricted.rs"), grammar)
        .expect("write the restricted grammar");
    fs::write(package.path().join("src/main.rs"), MAIN).expect("write the behaviour executable");
    package
}

/// The downstream behaviour the restriction must produce.
const MAIN: &str = r#"pub mod restricted;

fn parse(source: &str) -> restricted::Expr<'_> {
    let lexed = restricted::lex(source);
    assert!(lexed.errors().next().is_none(), "{source} lexes");
    let mut input = lexed.input();
    let parsed = restricted::Expr::parse(&mut input)
        .unwrap_or_else(|error| panic!("{source} parses: {error}"));
    assert!(input.is_eof(), "{source} is fully consumed");
    parsed.into_ast()
}

fn name<'a>(expression: &'a restricted::Expr<'_>) -> &'a str {
    let restricted::Expr::Name(name) = expression else {
        panic!("expected a name");
    };
    name.text.text()
}

fn main() {
    // The interior operand stops at `and`, which closes the clause instead of opening a
    // boolean operand: without the restriction this is a reduce/reduce conflict.
    let restricted::Expr::Between(low, middle, high) = parse("a between b and c") else {
        panic!("between is the root");
    };
    assert_eq!(name(&low), "a");
    assert_eq!(name(&middle), "b");
    assert_eq!(name(&high), "c");

    // The parenthesized atom keeps the full language, so boolean `and` is reachable inside it.
    let restricted::Expr::Between(low, middle, high) = parse("a between (b and c) and d") else {
        panic!("between is the root of the grouped form");
    };
    assert_eq!(name(&low), "a");
    let restricted::Expr::Parenthesized(inner) = *middle else {
        panic!("the restricted operand is the parenthesized atom");
    };
    let restricted::Expr::And(left, right) = *inner else {
        panic!("boolean AND is reachable inside the parenthesized atom");
    };
    assert_eq!(name(&left), "b");
    assert_eq!(name(&right), "c");
    assert_eq!(name(&high), "d");

    // The restriction removes nothing from the base: boolean `and` still parses on its own, and
    // `+` still parses inside the restricted operand.
    assert!(matches!(parse("a and b"), restricted::Expr::And(_, _)));
    let restricted::Expr::Between(_, middle, _) = parse("a between b + c and d") else {
        panic!("between is the root of the arithmetic form");
    };
    assert!(matches!(*middle, restricted::Expr::Add(_, _)));

    println!("restricted-ok");
}
"#;

fn run(package: &TempDir) -> Output {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Command::new(cargo)
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(package.path())
        .env_remove("RECURSA_GENERATION")
        .env_remove("RECURSA_DEBUG_DIR")
        .env("CARGO_TERM_COLOR", "never")
        .env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", "")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_DIR", target_dir(package))
        .output()
        .expect("run the restricted grammar fixture")
}

fn target_dir(package: &TempDir) -> PathBuf {
    package.path().join("target")
}
